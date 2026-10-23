//! The content outbox: every change the public site must learn about writes a `content_events`
//! row in the change's own transaction. Delivery (webhooks, phase P3) reads the undelivered rows
//! later; nothing is dispatched here.
//!
//! The payload is what the frontend needs to revalidate:
//!
//! ```json
//! { "tags": ["category:du-lich", "category:du-lich-cu", "posts", "sitemap"],
//!   "paths": { "old": ["/categories/du-lich-cu"], "new": ["/categories/du-lich"] } }
//! ```
//!
//! - `tags`: the cache tags of `docs/api.md` (`category:{slug}`, `tag:{slug}`, `menu:{key}`,
//!   `posts`, `sitemap`, …), without duplicates. A rename lists the old and the new slug.
//! - `paths.old`: public paths that no longer show this content (an old slug, a deleted
//!   record); `paths.new`: the paths that show it now.

use rustcms::AppResult;
use sea_orm::prelude::{ChronoUtc, DateTimeUtc, Uuid};
use sea_orm::{ActiveModelTrait, DatabaseTransaction, Set};
use serde_json::{Value, json};

use crate::models::content_event;

/// What happened to a record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    Created,
    Updated,
    Deleted,
}

impl Change {
    /// `created`, `updated` or `deleted`: the second half of an event kind (`category.created`).
    pub fn as_str(self) -> &'static str {
        match self {
            Change::Created => "created",
            Change::Updated => "updated",
            Change::Deleted => "deleted",
        }
    }
}

/// Writes one undelivered event (UUID v7 id, so ids sort by creation). Call it with the
/// transaction of the change it reports: the event commits or rolls back with it.
pub async fn record_event(
    txn: &DatabaseTransaction,
    kind: &str,
    subject_type: &str,
    subject_id: Option<i64>,
    payload: Value,
) -> AppResult<()> {
    content_event::ActiveModel {
        id: Set(Uuid::now_v7()),
        kind: Set(kind.to_owned()),
        subject_type: Set(subject_type.to_owned()),
        subject_id: Set(subject_id),
        payload: Set(payload),
        created_at: Set(now()),
        delivered_at: Set(None),
        attempts: Set(0),
        last_error: Set(None),
    }
    .insert(txn)
    .await?;
    Ok(())
}

/// A category was created, updated or deleted. `old_slugs` are the slugs it had before this
/// change (a rename); its posts, its page and the sitemap change with it.
pub async fn category_changed(
    txn: &DatabaseTransaction,
    change: Change,
    id: i64,
    slug: &str,
    old_slugs: &[String],
) -> AppResult<()> {
    let payload = taxonomy_payload("category", change, slug, old_slugs);
    record_event(txn, &format!("category.{}", change.as_str()), "category", Some(id), payload).await
}

/// A tag was created, updated or deleted (see [`category_changed`]).
pub async fn tag_changed(
    txn: &DatabaseTransaction,
    change: Change,
    id: i64,
    slug: &str,
    old_slugs: &[String],
) -> AppResult<()> {
    let payload = taxonomy_payload("tag", change, slug, old_slugs);
    record_event(txn, &format!("tag.{}", change.as_str()), "tag", Some(id), payload).await
}

/// A menu, or one of its items, was created, updated or deleted: `change` is what happened to
/// the menu itself (an item change is an update of its menu).
pub async fn menu_changed(txn: &DatabaseTransaction, change: Change, id: i64, key: &str) -> AppResult<()> {
    record_event(txn, &format!("menu.{}", change.as_str()), "menu", Some(id), menu_payload(key)).await
}

/// The frontend path of a public subject: `/posts/{slug}`, `/{slug}` (a page),
/// `/categories/{slug}`, `/tags/{slug}`, `/authors/{slug}` (docs/api.md, "Paths"). One place,
/// shared by the outbox payloads and the Content API's menus and sitemap.
pub fn public_path(subject_type: &str, slug: &str) -> String {
    match subject_type {
        "post" => format!("/posts/{slug}"),
        "page" => format!("/{slug}"),
        "category" => format!("/categories/{slug}"),
        "tag" => format!("/tags/{slug}"),
        "author" => format!("/authors/{slug}"),
        other => format!("/{other}/{slug}"),
    }
}

/// The payload of a category or tag change (`kind`: `category` or `tag`).
pub fn taxonomy_payload(kind: &str, change: Change, slug: &str, old_slugs: &[String]) -> Value {
    let mut tags = vec![format!("{kind}:{slug}")];
    let mut old: Vec<String> = Vec::new();
    for previous in old_slugs.iter().filter(|s| s.as_str() != slug) {
        push_unique(&mut tags, format!("{kind}:{previous}"));
        push_unique(&mut old, public_path(kind, previous));
    }
    // Posts show their category and tags; the sitemap lists their pages.
    tags.extend(["posts".to_owned(), "sitemap".to_owned()]);
    let current = public_path(kind, slug);
    let new = if change == Change::Deleted {
        push_unique(&mut old, current);
        Vec::new()
    } else {
        vec![current]
    };
    json!({ "tags": tags, "paths": { "old": old, "new": new } })
}

/// The payload of a menu change: the menu is no page of its own.
pub fn menu_payload(key: &str) -> Value {
    json!({ "tags": [format!("menu:{key}")], "paths": { "old": [], "new": [] } })
}

fn push_unique(list: &mut Vec<String>, value: String) {
    if !list.contains(&value) {
        list.push(value);
    }
}

/// Now, to the microsecond: what a `DATETIME(6)` column stores, so a value written and read
/// back compares equal.
pub fn now() -> DateTimeUtc {
    let now = ChronoUtc::now();
    DateTimeUtc::from_timestamp_micros(now.timestamp_micros()).unwrap_or(now)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rename_lists_both_slugs() {
        let payload = taxonomy_payload("category", Change::Updated, "du-lich", &["du-lich-cu".to_owned()]);
        assert_eq!(
            payload,
            json!({
                "tags": ["category:du-lich", "category:du-lich-cu", "posts", "sitemap"],
                "paths": { "old": ["/categories/du-lich-cu"], "new": ["/categories/du-lich"] }
            })
        );
    }

    #[test]
    fn a_deleted_record_has_only_old_paths() {
        let payload = taxonomy_payload("tag", Change::Deleted, "rust", &[]);
        assert_eq!(
            payload,
            json!({ "tags": ["tag:rust", "posts", "sitemap"], "paths": { "old": ["/tags/rust"], "new": [] } })
        );
    }

    #[test]
    fn an_unchanged_slug_is_not_an_old_one() {
        let payload = taxonomy_payload("tag", Change::Updated, "rust", &["rust".to_owned()]);
        assert_eq!(payload["tags"], json!(["tag:rust", "posts", "sitemap"]));
        assert_eq!(payload["paths"]["old"], json!([]));
    }

    #[test]
    fn menus_have_one_tag() {
        assert_eq!(menu_payload("primary"), json!({ "tags": ["menu:primary"], "paths": { "old": [], "new": [] } }));
    }
}
