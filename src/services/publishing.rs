//! The publi    if !transition.applies(row.status()) {hing workflow of posts and pages: the **only** code that changes `status` and
//! `published_at` (both guarded: forms never write them).
//!
//! | Transition | From | To | Who |
//! |---|---|---|---|
//! | [`Transition::Submit`] | draft | pending | may update it (`submit`) |
//! | [`Transition::Publish`] | draft, pending, scheduled | published, `published_at` = now | `posts.publish` / `pages.publish`, and its author unless `posts.edit_others` |
//! | [`Transition::Schedule`] | draft, pending, scheduled | scheduled, `published_at` = the time (future) | as Publish |
//! | [`Transition::Unpublish`] | published, scheduled | draft, no `published_at` | `posts.unpublish` / `pages.unpublish`, and its author unless `posts.edit_others` |
//! | [`publish_due`] | scheduled and due | published | the scheduler (system) |
//!
//! Which states a transition leaves is [`Transition::applies`], the one rule: the post and page
//! resources offer each workflow action only where it applies (`visible_when`), and
//! [`transition`] refuses it elsewhere.
//!
//! Each transition locks the row, checks it is not trashed, the transition and the actor's
//! permission ([`crate::policies::content::allows`], the policy's rule), writes the status and
//! syncs the change ([`content_sync::content_changed`]: search, public media, outbox event) in
//! the caller's transaction.

use rustcms::admin::Ctx;
use rustcms::audit::{AuditContext, AuditEntry};
use rustcms::{AppError, AppResult, ValidationErrors};
use sea_orm::prelude::DateTimeUtc;
use sea_orm::{
    ColumnTrait, DatabaseConnection, DatabaseTransaction, EntityTrait, QueryFilter, QueryOrder, QuerySelect,
};
use serde_json::json;

use crate::models::post::PostStatus;
use crate::models::{page, post};
use crate::policies::content::{self as rules, Facts};
use crate::services::content::{self, Col, Kind, value};
use crate::services::content_sync::{self, Change};

/// A change of publishing state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transition {
    /// Draft → pending review.
    Submit,
    /// Live now.
    Publish,
    /// Live at this (future) instant.
    Schedule(DateTimeUtc),
    /// Back to a draft.
    Unpublish,
}

impl Transition {
    /// The policy ability it needs (the action's).
    pub fn ability(self) -> &'static str {
        match self {
            Self::Submit => "submit",
            Self::Publish | Self::Schedule(_) => "publish",
            Self::Unpublish => "unpublish",
        }
    }

    /// Whether it applies to a post or page in `status`: the one rule of which transitions a
    /// record offers (the resources' `visible_when`) and accepts ([`transition`]).
    pub fn applies(self, status: Option<PostStatus>) -> bool {
        status.is_some_and(|s| self.from().contains(&s))
    }

    /// The states it leaves.
    fn from(self) -> &'static [PostStatus] {
        match self {
            Self::Submit => &[PostStatus::Draft],
            Self::Publish | Self::Schedule(_) => &[PostStatus::Draft, PostStatus::Pending, PostStatus::Scheduled],
            Self::Unpublish => &[PostStatus::Published, PostStatus::Scheduled],
        }
    }
}

/// Messages of refused transitions (translated by the caller).
pub const NOT_FROM_THIS_STATUS: &str = "This change does not apply to its current status";
pub const NOT_IN_THE_FUTURE: &str = "must be in the future";

/// Applies `transition` to post or page `id` for the user of `ctx`, in `txn`.
///
/// - 404: no such post or page, or trashed;
/// - 403: the user may not (permission, ownership, or a contributor's limits);
/// - 422: the transition does not leave its current status, or a schedule is not in the future
///   (on the `at` field).
pub async fn transition(
    ctx: &Ctx,
    txn: &DatabaseTransaction,
    kind: Kind,
    id: i64,
    transition: Transition,
) -> AppResult<()> {
    if !rules::admits(ctx, kind, transition.ability()) {
        return Err(AppError::Forbidden);
    }
    let row = content::lock(txn, kind, id).await?.filter(|r| r.deleted_at.is_none()).ok_or(AppError::NotFound)?;
    // Out of the user's sight: as if it did not exist.
    if !rules::allows(ctx, kind, "view", Some(&Facts::of(&row))) {
        return Err(AppError::NotFound);
    }
    if !transition.applies(row.status()) {
        return Err(content::form_error(ctx.t(NOT_FROM_THIS_STATUS)).into());
    }
    if !rules::allows(ctx, kind, transition.ability(), Some(&Facts::of(&row))) {
        return Err(AppError::Forbidden);
    }
    let now = content_sync::now();
    let (to, published_at, change) = match transition {
        Transition::Submit => (PostStatus::Pending, row.published_at, Change::Updated),
        Transition::Publish => (PostStatus::Published, Some(now), Change::Published),
        Transition::Schedule(at) => {
            if at <= now {
                return Err(ValidationErrors::field("at", ctx.t(NOT_IN_THE_FUTURE)).into());
            }
            // A scheduled post is not public: the outbox reports it as an update.
            (PostStatus::Scheduled, Some(at), Change::Updated)
        }
        Transition::Unpublish => (PostStatus::Draft, None, Change::Unpublished),
    };
    write(txn, kind, id, to, published_at, now).await?;
    content_sync::content_changed(txn, kind, id, change, content_sync::actor_id(ctx)).await
}

async fn write(
    txn: &DatabaseTransaction,
    kind: Kind,
    id: i64,
    to: PostStatus,
    published_at: Option<DateTimeUtc>,
    now: DateTimeUtc,
) -> AppResult<()> {
    let values =
        vec![(Col::Status, value(to.as_str())), (Col::PublishedAt, value(published_at)), (Col::UpdatedAt, value(now))];
    content::set(txn, kind, id, values).await?;
    Ok(())
}

/// Rows due per transaction of [`publish_due`].
pub const BATCH: u64 = 100;

/// Publishes every scheduled post and page that is due (`published_at <= now`, not trashed), as
/// the system: the scheduler's work (`jobs::publish_due`), every minute. It takes **all** of them,
/// [`BATCH`] per transaction, so a run after downtime catches up. Each row is locked and checked
/// again (still scheduled, due, not trashed): a post unpublished or rescheduled meanwhile is left
/// alone. `published_at` stays the scheduled time. Returns how many went live.
pub async fn publish_due(db: &DatabaseConnection) -> AppResult<u64> {
    let mut published = 0;
    for kind in [Kind::Post, Kind::Page] {
        loop {
            let now = content_sync::now();
            let mut ids = due_ids(db, kind, now).await?;
            // Locked in key order, as the panel's writes and actions lock rows: two of them never
            // wait on each other crosswise.
            ids.sort_unstable();
            if ids.is_empty() {
                break;
            }
            let txn = rustcms::db::begin_write(db).await?;
            let mut done = 0;
            for id in &ids {
                let Some(row) = content::lock(&txn, kind, *id).await? else { continue };
                let now = content_sync::now();
                let due = row.deleted_at.is_none()
                    && row.status() == Some(PostStatus::Scheduled)
                    && row.published_at.is_some_and(|at| at <= now);
                if !due {
                    continue;
                }
                write(&txn, kind, *id, PostStatus::Published, row.published_at, now).await?;
                content_sync::content_changed(&txn, kind, *id, Change::Published, None).await?;
                rustcms::audit::record(
                    &txn,
                    &AuditContext::system(Some("publish-due")),
                    AuditEntry::new("published", kind.resource())
                        .key(id)
                        .meta(json!({ "scheduled_for": row.published_at })),
                )
                .await?;
                done += 1;
            }
            txn.commit().await?;
            published += done;
            if (ids.len() as u64) < BATCH || done == 0 {
                break;
            }
        }
    }
    Ok(published)
}

/// The next due scheduled rows of `kind`, oldest schedule first (the scheduler's index).
async fn due_ids(db: &DatabaseConnection, kind: Kind, now: DateTimeUtc) -> AppResult<Vec<i64>> {
    let scheduled = PostStatus::Scheduled.as_str();
    Ok(match kind {
        Kind::Post => {
            post::Entity::find()
                .select_only()
                .column(post::Column::Id)
                .filter(post::Column::Status.eq(scheduled))
                .filter(post::Column::DeletedAt.is_null())
                .filter(post::Column::PublishedAt.lte(now))
                .order_by_asc(post::Column::PublishedAt)
                .order_by_asc(post::Column::Id)
                .limit(BATCH)
                .into_tuple()
                .all(db)
                .await?
        }
        Kind::Page => {
            page::Entity::find()
                .select_only()
                .column(page::Column::Id)
                .filter(page::Column::Status.eq(scheduled))
                .filter(page::Column::DeletedAt.is_null())
                .filter(page::Column::PublishedAt.lte(now))
                .order_by_asc(page::Column::PublishedAt)
                .order_by_asc(page::Column::Id)
                .limit(BATCH)
                .into_tuple()
                .all(db)
                .await?
        }
    })
}
