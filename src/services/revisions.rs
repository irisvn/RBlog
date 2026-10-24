//! Revisions of posts and pages (`post_revisions`, `page_revisions`): written by the server only,
//! from [`crate::services::content_sync`], when the hash of the edited content changes. Each post or
//! page keeps its [`KEEP`] newest; older ones are pruned in the same transaction, under the row
//! lock of the post or page that `content_sync` holds.
//!
//! The edited content is the title, excerpt, body, cover, OG image, SEO fields and, for posts, the
//! category and the tag ids. The status, slug and dates are not: restoring a revision never
//! publishes, unpublishes or moves anything ([`restore`]).

use std::collections::BTreeSet;

use rustcms::admin::{Ctx, rich_text};
use rustcms::audit::AuditEntry;
use rustcms::{AppError, AppResult};
use sea_orm::prelude::{DateTimeUtc, Uuid};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseTransaction, EntityTrait, QueryFilter, QueryOrder,
    QuerySelect, Set,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::models::{category, page_revision, post_revision, post_tag, tag};
use crate::policies::content::{self, Facts};
use crate::services::content::{self as rows, Col, Content, Kind, value};
use crate::services::content_sync;

/// Revisions kept per post or page.
pub const KEEP: u64 = 30;

/// A revision of either kind, as [`restore`] and [`latest`] read it.
#[derive(Clone, Debug, PartialEq)]
pub struct Revision {
    pub id: i64,
    /// The post or page.
    pub parent_id: i64,
    pub title: String,
    pub excerpt: Option<String>,
    pub body: Option<Value>,
    pub snapshot: Value,
    pub content_hash: String,
}

impl From<post_revision::Model> for Revision {
    fn from(r: post_revision::Model) -> Self {
        Self {
            id: r.id,
            parent_id: r.post_id,
            title: r.title,
            excerpt: r.excerpt,
            body: r.body,
            snapshot: r.snapshot,
            content_hash: r.content_hash,
        }
    }
}

impl From<page_revision::Model> for Revision {
    fn from(r: page_revision::Model) -> Self {
        Self {
            id: r.id,
            parent_id: r.page_id,
            title: r.title,
            excerpt: r.excerpt,
            body: r.body,
            snapshot: r.snapshot,
            content_hash: r.content_hash,
        }
    }
}

impl Revision {
    /// The category the post had (`None` for pages and posts without one).
    pub fn category_id(&self) -> Option<i64> {
        self.snapshot.get("category_id").and_then(Value::as_str).and_then(|s| s.parse().ok())
    }

    /// The tag ids the post had, in order (none for pages).
    pub fn tag_ids(&self) -> Vec<i64> {
        self.snapshot
            .get("tag_ids")
            .and_then(Value::as_array)
            .map(|ids| ids.iter().filter_map(|id| id.as_str().and_then(|s| s.parse().ok())).collect())
            .unwrap_or_default()
    }

    fn uuid(&self, key: &str) -> Option<Uuid> {
        self.snapshot.get(key).and_then(Value::as_str).and_then(|s| s.parse().ok())
    }

    fn text(&self, key: &str) -> Option<String> {
        self.snapshot.get(key).and_then(Value::as_str).map(str::to_owned)
    }
}

/// What a revision stores besides title, excerpt and body. 64-bit ids are strings (ADR 0003).
pub fn snapshot(content: &Content, tag_ids: &[i64]) -> Value {
    let id = |v: Option<Uuid>| v.map(|v| Value::String(v.to_string())).unwrap_or(Value::Null);
    let mut snapshot = json!({
        "cover_media_id": id(content.cover_media_id),
        "og_media_id": id(content.og_media_id),
        "meta_title": content.meta_title,
        "meta_description": content.meta_description,
        "canonical_url": content.canonical_url,
    });
    if content.kind == Kind::Post {
        snapshot["category_id"] = content.category_id.map(|c| Value::String(c.to_string())).unwrap_or(Value::Null);
        snapshot["tag_ids"] = Value::Array(tag_ids.iter().map(|t| Value::String(t.to_string())).collect());
    }
    snapshot
}

/// SHA-256 (hex) of the edited content, over canonical JSON (keys sorted at every level), so the
/// order MySQL returns JSON keys in never makes two equal documents differ.
pub fn content_hash(title: &str, excerpt: Option<&str>, body: Option<&Value>, snapshot: &Value) -> String {
    let edited = json!({ "title": title, "excerpt": excerpt, "body": body, "snapshot": snapshot });
    let mut canonical = String::new();
    write_canonical(&edited, &mut canonical);
    hex::encode(Sha256::digest(canonical.as_bytes()))
}

fn write_canonical(value: &Value, out: &mut String) {
    match value {
        Value::Object(map) => {
            let sorted: BTreeSet<&String> = map.keys().collect();
            out.push('{');
            for (i, key) in sorted.into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&Value::String(key.clone()).to_string());
                out.push(':');
                write_canonical(&map[key], out);
            }
            out.push('}');
        }
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_canonical(item, out);
            }
            out.push(']');
        }
        other => out.push_str(&other.to_string()),
    }
}

/// The newest revision of a post or page.
pub async fn latest<C: ConnectionTrait>(db: &C, kind: Kind, parent_id: i64) -> AppResult<Option<Revision>> {
    Ok(match kind {
        Kind::Post => post_revision::Entity::find()
            .filter(post_revision::Column::PostId.eq(parent_id))
            .order_by_desc(post_revision::Column::Id)
            .one(db)
            .await?
            .map(Revision::from),
        Kind::Page => page_revision::Entity::find()
            .filter(page_revision::Column::PageId.eq(parent_id))
            .order_by_desc(page_revision::Column::Id)
            .one(db)
            .await?
            .map(Revision::from),
    })
}

/// Writes a revision of `content` (as stored now, with `tag_ids`) unless the newest one has the
/// same content hash, then prunes beyond [`KEEP`] (never `keep`: the revision being restored, so
/// the action that restores it still reports it). Returns whether one was written.
///
/// Call it with the post or page row locked (`content_sync` does): two writers then never prune
/// or compare against each other's half-written history.
pub async fn record(
    txn: &DatabaseTransaction,
    content: &Content,
    tag_ids: &[i64],
    previous: Option<&Revision>,
    author_id: Option<i64>,
    at: DateTimeUtc,
    keep: Option<i64>,
) -> AppResult<bool> {
    let snapshot = snapshot(content, tag_ids);
    let hash = content_hash(&content.title, content.excerpt.as_deref(), content.body.as_ref(), &snapshot);
    if previous.is_some_and(|p| p.content_hash == hash) {
        return Ok(false);
    }
    match content.kind {
        Kind::Post => {
            post_revision::ActiveModel {
                post_id: Set(content.id),
                title: Set(content.title.clone()),
                excerpt: Set(content.excerpt.clone()),
                body: Set(content.body.clone()),
                snapshot: Set(snapshot),
                content_hash: Set(hash),
                author_id: Set(author_id),
                created_at: Set(at),
                ..Default::default()
            }
            .insert(txn)
            .await?;
            let stale: Vec<i64> = post_revision::Entity::find()
                .select_only()
                .column(post_revision::Column::Id)
                .filter(post_revision::Column::PostId.eq(content.id))
                .order_by_desc(post_revision::Column::Id)
                .offset(KEEP)
                .limit(1_000)
                .into_tuple()
                .all(txn)
                .await?;
            let stale: Vec<i64> = stale.into_iter().filter(|id| Some(*id) != keep).collect();
            if !stale.is_empty() {
                post_revision::Entity::delete_many().filter(post_revision::Column::Id.is_in(stale)).exec(txn).await?;
            }
        }
        Kind::Page => {
            page_revision::ActiveModel {
                page_id: Set(content.id),
                title: Set(content.title.clone()),
                excerpt: Set(content.excerpt.clone()),
                body: Set(content.body.clone()),
                snapshot: Set(snapshot),
                content_hash: Set(hash),
                author_id: Set(author_id),
                created_at: Set(at),
                ..Default::default()
            }
            .insert(txn)
            .await?;
            let stale: Vec<i64> = page_revision::Entity::find()
                .select_only()
                .column(page_revision::Column::Id)
                .filter(page_revision::Column::PageId.eq(content.id))
                .order_by_desc(page_revision::Column::Id)
                .offset(KEEP)
                .limit(1_000)
                .into_tuple()
                .all(txn)
                .await?;
            let stale: Vec<i64> = stale.into_iter().filter(|id| Some(*id) != keep).collect();
            if !stale.is_empty() {
                page_revision::Entity::delete_many().filter(page_revision::Column::Id.is_in(stale)).exec(txn).await?;
            }
        }
    }
    Ok(true)
}

/// Messages of [`restore`]'s refusals (translated by the caller).
pub const BODY_INVALID: &str = "This version cannot be restored: its body is no longer valid";
pub const MEDIA_UNAVAILABLE: &str = "This version cannot be restored: one of its images is no longer available to you";
pub const REFERENCE_UNAVAILABLE: &str =
    "This version cannot be restored: its category or one of its tags is not available to you";

/// Restores revision `revision_id` of post or page `parent_id`, as the user of `ctx`:
///
/// 1. locks the post or page (not trashed) and reads the revision again under that lock: it must
///    be one of this post's or page's (404 otherwise);
/// 2. checks the user may update it (403), and, when the tags change, attach and detach them;
/// 3. parses the body again and checks every image (cover, OG, body): it exists, is not trashed,
///    is an image, and the user may see it (422 otherwise);
/// 4. writes the content back (status, slug and dates unchanged) and syncs it (`content_sync`),
///    which writes a new revision.
///
/// A category deleted since is left empty; tags deleted since are skipped.
pub async fn restore(
    ctx: &Ctx,
    txn: &DatabaseTransaction,
    kind: Kind,
    parent_id: i64,
    revision_id: i64,
) -> AppResult<()> {
    let current =
        rows::lock(txn, kind, parent_id).await?.filter(|c| c.deleted_at.is_none()).ok_or(AppError::NotFound)?;
    let revision: Revision = match kind {
        Kind::Post => post_revision::Entity::find_by_id(revision_id)
            .filter(post_revision::Column::PostId.eq(parent_id))
            .one(txn)
            .await?
            .map(Revision::from),
        Kind::Page => page_revision::Entity::find_by_id(revision_id)
            .filter(page_revision::Column::PageId.eq(parent_id))
            .one(txn)
            .await?
            .map(Revision::from),
    }
    .ok_or(AppError::NotFound)?;
    let facts = Facts::of(&current);
    if !content::allows(ctx, kind, "update", Some(&facts)) {
        return Err(AppError::Forbidden);
    }

    // The body must still be a valid document, and every image one the user may use.
    let body = match &revision.body {
        Some(body) if !body.is_null() => {
            let document = rich_text::parse(body).map_err(|_| rows::form_error(ctx.t(BODY_INVALID)))?;
            Some((body.clone(), rich_text::media_ids(&document)))
        }
        _ => None,
    };
    let cover = revision.uuid("cover_media_id");
    let og = revision.uuid("og_media_id");
    let media: BTreeSet<Uuid> =
        cover.into_iter().chain(og).chain(body.iter().flat_map(|(_, ids)| ids.clone())).collect();
    if !media_usable(ctx, txn, &media).await? {
        return Err(rows::form_error(ctx.t(MEDIA_UNAVAILABLE)).into());
    }

    let now = content_sync::now();
    let mut values = vec![
        (Col::Title, value(revision.title.clone())),
        (Col::Excerpt, value(revision.excerpt.clone())),
        (Col::Body, value(body.map(|(b, _)| b))),
        (Col::CoverMediaId, value(cover)),
        (Col::OgMediaId, value(og)),
        (Col::MetaTitle, value(revision.text("meta_title"))),
        (Col::MetaDescription, value(revision.text("meta_description"))),
        (Col::CanonicalUrl, value(revision.text("canonical_url"))),
        (Col::UpdatedAt, value(now)),
    ];
    if kind == Kind::Post {
        let category = match revision.category_id() {
            Some(id) => category::Entity::find_by_id(id).one(txn).await?.map(|c| c.id),
            None => None,
        };
        // A category the post does not have now must be one the user may choose, as in the form.
        if let Some(id) = category.filter(|c| Some(*c) != current.category_id)
            && ctx.record("categories", &id.to_string()).await.is_err()
        {
            return Err(rows::form_error(ctx.t(REFERENCE_UNAVAILABLE)).into());
        }
        values.push((Col::CategoryId, value(category)));
        restore_tags(ctx, txn, &facts, parent_id, &revision.tag_ids()).await?;
    }
    rows::set(txn, kind, parent_id, values).await?;
    ctx.audit(
        txn,
        AuditEntry::new("revision_restored", kind.resource())
            .key(parent_id)
            .meta(json!({ "revision_id": revision_id.to_string() })),
    )
    .await?;
    content_sync::content_restored(txn, kind, parent_id, content_sync::actor_id(ctx), revision_id).await
}

/// Makes the post's tags those of the revision that still exist, in its order. Removing or adding
/// tags needs `detach_tags` / `attach_tags` on the post, as in its form.
async fn restore_tags(
    ctx: &Ctx,
    txn: &DatabaseTransaction,
    facts: &Facts,
    post_id: i64,
    wanted: &[i64],
) -> AppResult<()> {
    let existing: BTreeSet<i64> = if wanted.is_empty() {
        BTreeSet::new()
    } else {
        tag::Entity::find()
            .select_only()
            .column(tag::Column::Id)
            .filter(tag::Column::Id.is_in(wanted.iter().copied()))
            .into_tuple::<i64>()
            .all(txn)
            .await?
            .into_iter()
            .collect()
    };
    let wanted: Vec<i64> = wanted.iter().copied().filter(|id| existing.contains(id)).collect();
    let current = rows::tag_ids(txn, Kind::Post, post_id).await?;
    if current == wanted {
        return Ok(());
    }
    let current_set: BTreeSet<i64> = current.iter().copied().collect();
    let wanted_set: BTreeSet<i64> = wanted.iter().copied().collect();
    let attaching = wanted_set.difference(&current_set).next().is_some();
    let detaching = current_set.difference(&wanted_set).next().is_some();
    if (attaching && !content::allows(ctx, Kind::Post, "attach_tags", Some(facts)))
        || (detaching && !content::allows(ctx, Kind::Post, "detach_tags", Some(facts)))
    {
        return Err(AppError::Forbidden);
    }
    // Tags the post does not have now must be ones the user may choose, as in the form.
    for id in wanted_set.difference(&current_set) {
        if ctx.record("tags", &id.to_string()).await.is_err() {
            return Err(rows::form_error(ctx.t(REFERENCE_UNAVAILABLE)).into());
        }
    }
    post_tag::Entity::delete_many().filter(post_tag::Column::PostId.eq(post_id)).exec(txn).await?;
    if !wanted.is_empty() {
        let rows = wanted.iter().enumerate().map(|(position, tag_id)| post_tag::ActiveModel {
            post_id: Set(post_id),
            tag_id: Set(*tag_id),
            position: Set(position as i32),
        });
        post_tag::Entity::insert_many(rows).exec_without_returning(txn).await?;
    }
    Ok(())
}

/// Whether every media id is an image that exists, is not trashed, and the user may see: their
/// own upload, or any with `media.view_all` / `media.manage_all` (the media library's scope), and
/// `media.view`.
pub async fn media_usable(ctx: &Ctx, txn: &DatabaseTransaction, ids: &BTreeSet<Uuid>) -> AppResult<bool> {
    if ids.is_empty() {
        return Ok(true);
    }
    if !ctx.can("media.view_any") || !ctx.can("media.view") {
        return Ok(false);
    }
    let found = crate::services::public_media::images(txn, ids.iter().copied()).await?;
    if found.len() != ids.len() {
        return Ok(false);
    }
    let sees_all = ctx.can(rustcms::admin::media::VIEW_ALL) || ctx.can("media.manage_all");
    let me = content_sync::actor_id(ctx);
    Ok(sees_all || found.values().all(|m| Some(m.owner_id) == me))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hash_ignores_key_order() {
        let a = json!({"type": "doc", "version": 1, "content": [{"type": "paragraph", "content": []}]});
        let b = json!({"content": [{"content": [], "type": "paragraph"}], "version": 1, "type": "doc"});
        let snapshot = json!({"cover_media_id": null, "tag_ids": ["1", "2"]});
        assert_eq!(content_hash("T", None, Some(&a), &snapshot), content_hash("T", None, Some(&b), &snapshot));
        assert_ne!(content_hash("T", None, Some(&a), &snapshot), content_hash("T2", None, Some(&a), &snapshot));
        let reordered = json!({"cover_media_id": null, "tag_ids": ["2", "1"]});
        assert_ne!(content_hash("T", None, Some(&a), &snapshot), content_hash("T", None, Some(&a), &reordered));
    }
}
