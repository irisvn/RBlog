//! Writes of categories and tags: their slug, the old slugs kept for redirects
//! (`slug_history`) and the outbox event, in the write's transaction.
//!
//! A write goes through [`slug_for_create`] or [`slug_for_update`] before it is stored, then
//! [`saved`] (or [`deleted`]) once it is. Every write sets the record's timestamps itself to one
//! instant ([`content_sync::now`]), the same for the `slug_history` row of a rename, so that
//! [`saved`] can tell from the stored row alone whether the write created the record and which
//! slugs it replaced: a create has `created_at = updated_at`, and a rename's history row has
//! `created_at = updated_at` of the record.

use rustcms::{AppResult, ValidationErrors};
use sea_orm::prelude::DateTimeUtc;
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseTransaction, EntityTrait, QueryFilter, QuerySelect, Set};

use crate::models::{category, slug_history, tag};
use crate::services::content_sync::{self, Change};
use crate::services::slug::{Slugged, slugify, unique_slug};

/// The message of a typed slug without a letter or digit (translated by the caller).
pub const SLUG_WITHOUT_LETTERS: &str = "must contain a letter or a digit";

/// A table of the taxonomy: categories or tags.
pub trait Taxonomy: Slugged + Default {
    /// `subject_type` in `slug_history` and `content_events`.
    const SUBJECT: &'static str;
}

impl Taxonomy for category::Entity {
    const SUBJECT: &'static str = "category";
}

impl Taxonomy for tag::Entity {
    const SUBJECT: &'static str = "tag";
}

/// The slug of a new record: `typed` made a slug (a 422 on `slug` when nothing is left of it),
/// or, when blank or absent, a free slug made from `name`. A typed slug that is taken is
/// refused by the unique index (the resource maps it to a field error).
pub async fn slug_for_create<E: Taxonomy>(
    txn: &DatabaseTransaction,
    name: &str,
    typed: Option<&str>,
) -> AppResult<String> {
    resolve::<E>(txn, name, typed, None).await
}

/// The slug after an update of record `id`, or `None` when it stays as it is: `typed` is what
/// the request sent for `slug` (`None`: not sent, keep it; `Some(None)` or blank: make it again
/// from `name`, the new or current name). A changed slug keeps the current one in
/// `slug_history`, written at `at` (the record's new `updated_at`).
pub async fn slug_for_update<E: Taxonomy>(
    txn: &DatabaseTransaction,
    id: i64,
    current: &str,
    name: &str,
    typed: Option<Option<&str>>,
    at: DateTimeUtc,
) -> AppResult<Option<String>> {
    let Some(typed) = typed else { return Ok(None) };
    let slug = resolve::<E>(txn, name, typed, Some(id)).await?;
    if slug == current {
        return Ok(None);
    }
    // An old slug points at one record: the latest that had it.
    slug_history::Entity::delete_many()
        .filter(slug_history::Column::SubjectType.eq(E::SUBJECT))
        .filter(slug_history::Column::OldSlug.eq(current))
        .exec(txn)
        .await?;
    slug_history::ActiveModel {
        subject_type: Set(E::SUBJECT.to_owned()),
        subject_id: Set(id),
        old_slug: Set(current.to_owned()),
        created_at: Set(at),
        ..Default::default()
    }
    .insert(txn)
    .await?;
    Ok(Some(slug))
}

async fn resolve<E: Taxonomy>(
    txn: &DatabaseTransaction,
    name: &str,
    typed: Option<&str>,
    except_id: Option<i64>,
) -> AppResult<String> {
    let slug = match typed.map(str::trim).filter(|s| !s.is_empty()) {
        Some(typed) => {
            let slug: String = slugify(typed).chars().take(E::MAX_LEN).collect();
            let slug = slug.trim_end_matches('-').to_owned();
            if slug.is_empty() {
                return Err(ValidationErrors::field("slug", SLUG_WITHOUT_LETTERS).into());
            }
            slug
        }
        None => unique_slug(txn, E::default(), name, except_id).await?,
    };
    // A live record now answers this slug: it no longer redirects elsewhere.
    slug_history::Entity::delete_many()
        .filter(slug_history::Column::SubjectType.eq(E::SUBJECT))
        .filter(slug_history::Column::OldSlug.eq(slug.as_str()))
        .exec(txn)
        .await?;
    Ok(slug)
}

/// Records the outbox event of a stored write (create or update) of record `id`, now `slug`.
pub async fn saved<E: Taxonomy>(
    txn: &DatabaseTransaction,
    id: i64,
    slug: &str,
    created_at: DateTimeUtc,
    updated_at: DateTimeUtc,
) -> AppResult<()> {
    let change = if created_at == updated_at { Change::Created } else { Change::Updated };
    // The slugs this very write replaced (see the module documentation).
    let old: Vec<String> = slug_history::Entity::find()
        .select_only()
        .column(slug_history::Column::OldSlug)
        .filter(slug_history::Column::SubjectType.eq(E::SUBJECT))
        .filter(slug_history::Column::SubjectId.eq(id))
        .filter(slug_history::Column::CreatedAt.eq(updated_at))
        .into_tuple()
        .all(txn)
        .await?;
    event::<E>(txn, change, id, slug, &old).await
}

/// Records the outbox event of a deleted record and forgets its old slugs: they would redirect
/// to nothing.
pub async fn deleted<E: Taxonomy>(txn: &DatabaseTransaction, id: i64, slug: &str) -> AppResult<()> {
    // The frontend may still cache pages under the old slugs (they redirected here): purge them too.
    let old: Vec<String> = slug_history::Entity::find()
        .select_only()
        .column(slug_history::Column::OldSlug)
        .filter(slug_history::Column::SubjectType.eq(E::SUBJECT))
        .filter(slug_history::Column::SubjectId.eq(id))
        .into_tuple()
        .all(txn)
        .await?;
    slug_history::Entity::delete_many()
        .filter(slug_history::Column::SubjectType.eq(E::SUBJECT))
        .filter(slug_history::Column::SubjectId.eq(id))
        .exec(txn)
        .await?;
    event::<E>(txn, Change::Deleted, id, slug, &old).await
}

async fn event<E: Taxonomy>(
    txn: &DatabaseTransaction,
    change: Change,
    id: i64,
    slug: &str,
    old: &[String],
) -> AppResult<()> {
    match E::SUBJECT {
        "category" => content_sync::category_changed(txn, change, id, slug, old).await,
        _ => content_sync::tag_changed(txn, change, id, slug, old).await,
    }
}
