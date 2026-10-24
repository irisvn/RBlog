//! `public_media`: which public content uses which media. `GET /api/v1/media/{id}/{name}` serves
//! a media item only while a row names it, so an image of a draft stays private.
//!
//! [`sync`] recomputes the rows of one post or page; every write that changes what it shows or
//! whether it is public calls it in its own transaction. [`backfill`] rebuilds every row
//! (`rblog content:sync-public-media`).
//!
//! The images of the public settings (the logo, the default share image) are public too: rows of
//! subject `settings`, one subject per settings group ([`SettingsImages`]), written by the
//! settings pages' `after_save` ([`sync_settings`]).
//!
//! A public post's rows also name its author's avatar: the avatar is public while one of their
//! posts is (`GET /api/v1/content/authors/{slug}` shows it then), and changing a post's author
//! moves it with the post.
//!
//! Concurrency: [`sync`] reads the post or page `FOR UPDATE`. A writer that changed the row holds
//! that lock until it commits, so a concurrent sync (another writer's, or [`backfill`]) waits and
//! then reads the committed row: it can never write back the rows of an older version.

use std::collections::{BTreeSet, HashMap};

use rustcms::admin::media::entity as media;
use rustcms::admin::rich_text;
use sea_orm::prelude::Uuid;
use sea_orm::sea_query::{Alias, Expr, Query};
use sea_orm::{
    ColumnTrait, Condition, ConnectionTrait, DatabaseConnection, DbErr, EntityTrait, QueryFilter, QueryOrder,
    QuerySelect, Set,
};
use serde_json::Value;

use rustcms::crypto::Keyring;

use crate::models::{author_profile, page, post, public_media};
use crate::settings::general_settings::GeneralSettings;
use crate::settings::seo_settings::SeoSettings;

/// The media types served publicly (and in previews): images only.
pub const IMAGE_MIMES: [&str; 4] = ["image/png", "image/jpeg", "image/gif", "image/webp"];

/// What [`sync`] recomputes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Subject {
    /// A post: cover, OG image, body images and its author's avatar, while it is public.
    Post,
    /// A page: cover, OG image and body images, while it is public.
    Page,
    /// Every post of an author (`subject_id` is the user): call it when their avatar changes.
    Author,
}

impl Subject {
    /// The `public_media.subject_type` value (an author's rows are their posts' rows).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Post | Self::Author => "post",
            Self::Page => "page",
        }
    }
}

/// The `public_media.subject_type` of the image settings' rows ([`sync_settings`]).
pub const SETTINGS: &str = "settings";

/// The settings groups that show images publicly: the `subject_id` of their `public_media` rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i64)]
pub enum SettingsImages {
    /// Settings → General: the logo.
    General = 1,
    /// Settings → SEO and social: the default share image.
    Seo = 2,
}

impl SettingsImages {
    pub const ALL: [Self; 2] = [Self::General, Self::Seo];

    pub fn id(self) -> i64 {
        self as i64
    }
}

/// Makes the `public_media` rows of settings group `group` exactly `media` (its image settings
/// as saved). Call it in the save's transaction (`SettingsPage::after_save`): the settings row is
/// locked by the save, so two saves of one group never interleave.
pub async fn sync_settings<C: ConnectionTrait>(
    txn: &C,
    group: SettingsImages,
    media: impl IntoIterator<Item = Uuid>,
) -> Result<(), DbErr> {
    replace(txn, SETTINGS, group.id(), media.into_iter().collect()).await
}

/// The media ids a rich-text body shows (none when it is empty or not a valid document).
pub fn body_media_ids(body: Option<&Value>) -> Vec<Uuid> {
    body.and_then(|body| rich_text::parse(body).ok()).map(|doc| rich_text::media_ids(&doc)).unwrap_or_default()
}

/// Recomputes the `public_media` rows of a post or page (or of every post of an author): its
/// media while it is public (published, due and not trashed), none otherwise.
///
/// Call it in the transaction of the write that changed the subject, after the change.
pub async fn sync<C: ConnectionTrait>(txn: &C, subject: Subject, subject_id: i64) -> Result<(), DbErr> {
    match subject {
        Subject::Post => sync_post(txn, subject_id).await,
        Subject::Page => sync_page(txn, subject_id).await,
        Subject::Author => {
            let posts: Vec<i64> = post::Entity::find()
                .select_only()
                .column(post::Column::Id)
                .filter(post::Column::AuthorId.eq(subject_id))
                .order_by_asc(post::Column::Id)
                .into_tuple()
                .all(txn)
                .await?;
            for id in posts {
                sync_post(txn, id).await?;
            }
            Ok(())
        }
    }
}

async fn sync_post<C: ConnectionTrait>(txn: &C, id: i64) -> Result<(), DbErr> {
    let found = post::Entity::find_by_id(id).lock_exclusive().one(txn).await?;
    // The clock is read once the lock is held: a writer that published meanwhile has committed.
    let now = super::content_sync::now();
    let found = found.filter(|p| p.is_public_at(now));
    let mut media = BTreeSet::new();
    if let Some(p) = found {
        media = content_media(p.cover_media_id, p.og_media_id, p.body.as_ref());
        let profile = author_profile::Entity::find_by_id(p.author_id).one(txn).await?;
        media.extend(profile.and_then(|a| a.avatar_media_id));
    }
    replace(txn, Subject::Post.as_str(), id, media).await
}

async fn sync_page<C: ConnectionTrait>(txn: &C, id: i64) -> Result<(), DbErr> {
    let found = page::Entity::find_by_id(id).lock_exclusive().one(txn).await?;
    // The clock is read once the lock is held: a writer that published meanwhile has committed.
    let now = super::content_sync::now();
    let found = found.filter(|p| p.is_public_at(now));
    let media = found.map(|p| content_media(p.cover_media_id, p.og_media_id, p.body.as_ref())).unwrap_or_default();
    replace(txn, Subject::Page.as_str(), id, media).await
}

/// Cover, OG image and body images.
fn content_media(cover: Option<Uuid>, og: Option<Uuid>, body: Option<&Value>) -> BTreeSet<Uuid> {
    cover.into_iter().chain(og).chain(body_media_ids(body)).collect()
}

/// Makes the rows of `subject` (a `subject_type`) exactly `media`.
async fn replace<C: ConnectionTrait>(
    txn: &C,
    subject: &str,
    subject_id: i64,
    media: BTreeSet<Uuid>,
) -> Result<(), DbErr> {
    let of_subject = Condition::all()
        .add(public_media::Column::SubjectType.eq(subject))
        .add(public_media::Column::SubjectId.eq(subject_id));
    let mut stale = public_media::Entity::delete_many().filter(of_subject);
    if !media.is_empty() {
        stale = stale.filter(public_media::Column::MediaId.is_not_in(media.iter().copied()));
    }
    stale.exec(txn).await?;
    if media.is_empty() {
        return Ok(());
    }
    let rows = media.into_iter().map(|media_id| public_media::ActiveModel {
        media_id: Set(media_id),
        subject_type: Set(subject.to_owned()),
        subject_id: Set(subject_id),
    });
    // Rows already there stay.
    public_media::Entity::insert_many(rows).on_conflict_do_nothing().exec_without_returning(txn).await?;
    Ok(())
}

/// What [`backfill`] did.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct BackfillReport {
    /// Posts, pages and settings groups synced.
    pub subjects: u64,
    /// Rows of posts, pages and settings groups that no longer exist (or of unknown subject
    /// types), removed.
    pub orphans: u64,
}

/// Subjects synced per transaction.
const BATCH: u64 = 200;

/// Rebuilds `public_media` for every post and page (by batches, each in its own transaction) and
/// for the image settings, then removes the rows of subjects that no longer exist. Safe while the
/// site runs.
pub async fn backfill(db: &DatabaseConnection, keyring: &Keyring) -> rustcms::AppResult<BackfillReport> {
    let mut report = BackfillReport::default();
    {
        let txn = rustcms::db::begin_write(db).await?;
        // A settings save runs under the RBAC lock from its first statement (RustCMS re-checks
        // the saver's permissions in the write), its `after_save` sync included. Taken first
        // here too, it serializes this with every save, a group's first one included (no row to
        // lock yet): the reads below see the last committed save, and none commits until this
        // does.
        rustcms::auth::rbac::lock(&txn).await?;
        let general = rustcms::settings::load::<GeneralSettings, _>(&txn, keyring).await?;
        let seo = rustcms::settings::load::<SeoSettings, _>(&txn, keyring).await?;
        sync_settings(&txn, SettingsImages::General, general.logo).await?;
        sync_settings(&txn, SettingsImages::Seo, seo.og_image).await?;
        txn.commit().await?;
        report.subjects += SettingsImages::ALL.len() as u64;
    }
    for subject in [Subject::Post, Subject::Page] {
        let mut after = 0i64;
        loop {
            let ids = match subject {
                Subject::Page => ids_after::<page::Entity>(db, page::Column::Id, after).await?,
                _ => ids_after::<post::Entity>(db, post::Column::Id, after).await?,
            };
            let Some(&last) = ids.last() else { break };
            let txn = rustcms::db::begin_write(db).await?;
            for id in &ids {
                sync(&txn, subject, *id).await?;
            }
            txn.commit().await?;
            report.subjects += ids.len() as u64;
            after = last;
        }
    }
    for (subject, table) in [(Subject::Post, "posts"), (Subject::Page, "pages")] {
        let existing = Query::select().expr(Expr::col(Alias::new("id"))).from(Alias::new(table)).to_owned();
        let removed = public_media::Entity::delete_many()
            .filter(public_media::Column::SubjectType.eq(subject.as_str()))
            .filter(public_media::Column::SubjectId.not_in_subquery(existing))
            .exec(db)
            .await?;
        report.orphans += removed.rows_affected;
    }
    let groups = SettingsImages::ALL.map(SettingsImages::id);
    let unknown_groups = public_media::Entity::delete_many()
        .filter(public_media::Column::SubjectType.eq(SETTINGS))
        .filter(public_media::Column::SubjectId.is_not_in(groups))
        .exec(db)
        .await?;
    report.orphans += unknown_groups.rows_affected;
    let known = [Subject::Post.as_str(), Subject::Page.as_str(), SETTINGS];
    let unknown =
        public_media::Entity::delete_many().filter(public_media::Column::SubjectType.is_not_in(known)).exec(db).await?;
    report.orphans += unknown.rows_affected;
    Ok(report)
}

/// The next [`BATCH`] keys of `E` above `after`, in order.
async fn ids_after<E: EntityTrait>(db: &DatabaseConnection, key: E::Column, after: i64) -> Result<Vec<i64>, DbErr> {
    E::find().select_only().column(key).filter(key.gt(after)).order_by_asc(key).limit(BATCH).into_tuple().all(db).await
}

/// The media rows among `ids` that may be shown at all: images ([`IMAGE_MIMES`]), not trashed.
pub async fn images<C: ConnectionTrait>(
    db: &C,
    ids: impl IntoIterator<Item = Uuid>,
) -> Result<HashMap<Uuid, media::Model>, DbErr> {
    let ids: BTreeSet<Uuid> = ids.into_iter().collect();
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let rows = media::Entity::find()
        .filter(media::Column::Id.is_in(ids))
        .filter(media::Column::DeletedAt.is_null())
        .filter(media::Column::Mime.is_in(IMAGE_MIMES))
        .all(db)
        .await?;
    Ok(rows.into_iter().map(|m| (m.id, m)).collect())
}

/// The media rows among `ids` that are public now: [`images`] named by a `public_media` row.
pub async fn public_images<C: ConnectionTrait>(
    db: &C,
    ids: impl IntoIterator<Item = Uuid>,
) -> Result<HashMap<Uuid, media::Model>, DbErr> {
    let ids: BTreeSet<Uuid> = ids.into_iter().collect();
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let public: BTreeSet<Uuid> = public_media::Entity::find()
        .select_only()
        .column(public_media::Column::MediaId)
        .filter(public_media::Column::MediaId.is_in(ids))
        .distinct()
        .into_tuple()
        .all(db)
        .await?
        .into_iter()
        .collect();
    images(db, public).await
}
