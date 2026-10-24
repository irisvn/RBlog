//! The content outbox: every change the public site must learn about writes a `content_events`
//! row in the change's own transaction, with its webhook delivery job
//! (`crate::services::events::enqueue`): both commit or roll back with the change.
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

use rustcms::{AppError, AppResult};
use sea_orm::prelude::{ChronoUtc, DateTimeUtc, Uuid};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseTransaction, EntityTrait, QueryFilter, QueryOrder, QuerySelect, Set,
};
use serde_json::{Value, json};

use crate::models::{author_profile, category, content_event, menu, menu_item, slug_history, tag};
use crate::services::content::{self, Col, Content, Kind};
use crate::services::{public_media, revisions, search};

/// What happened to a record. Posts and pages also go through the publishing workflow and the
/// trash; categories, tags and menus are only created, updated and deleted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    Created,
    Updated,
    /// Went live (published now, or a scheduled one fell due).
    Published,
    /// Back to a draft from published or scheduled.
    Unpublished,
    /// Moved to the trash.
    Trashed,
    /// Back from the trash.
    Restored,
    /// Deleted for good.
    Deleted,
}

impl Change {
    /// `created`, `updated`, `published`, …: the second half of an event kind
    /// (`category.created`, `post.published`).
    pub fn as_str(self) -> &'static str {
        match self {
            Change::Created => "created",
            Change::Updated => "updated",
            Change::Published => "published",
            Change::Unpublished => "unpublished",
            Change::Trashed => "trashed",
            Change::Restored => "restored",
            Change::Deleted => "deleted",
        }
    }
}

/// Writes one undelivered event (UUID v7 id, so ids sort by creation) and dispatches its delivery
/// job. Call it with the transaction of the change it reports: the event and its job commit or
/// roll back with it.
pub async fn record_event(
    txn: &DatabaseTransaction,
    kind: &str,
    subject_type: &str,
    subject_id: Option<i64>,
    payload: Value,
) -> AppResult<()> {
    let id = Uuid::now_v7();
    content_event::ActiveModel {
        id: Set(id),
        kind: Set(kind.to_owned()),
        subject_type: Set(subject_type.to_owned()),
        subject_id: Set(subject_id),
        payload: Set(payload),
        created_at: Set(now()),
        delivered_at: Set(None),
        attempts: Set(0),
        last_error: Set(None),
        job_id: Set(None),
        gave_up_at: Set(None),
    }
    .insert(txn)
    .await?;
    crate::services::events::enqueue(txn, id).await?;
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

/// An author profile was created, updated or deleted (see [`category_changed`]; `id` is the
/// user's). Posts show their author, and the sitemap lists author pages.
pub async fn author_changed(
    txn: &DatabaseTransaction,
    change: Change,
    id: i64,
    slug: &str,
    old_slugs: &[String],
) -> AppResult<()> {
    let payload = taxonomy_payload("author", change, slug, old_slugs);
    record_event(txn, &format!("author.{}", change.as_str()), "author", Some(id), payload).await
}

/// A menu, or one of its items, was created, updated or deleted: `change` is what happened to
/// the menu itself (an item change is an update of its menu).
pub async fn menu_changed(txn: &DatabaseTransaction, change: Change, id: i64, key: &str) -> AppResult<()> {
    record_event(txn, &format!("menu.{}", change.as_str()), "menu", Some(id), menu_payload(key)).await
}

/// The user a hook or action runs for (`None` for a principal that is not a user).
pub fn actor_id(ctx: &rustcms::admin::Ctx) -> Option<i64> {
    ctx.principal.id.parse().ok()
}

/// The one entry point of every write to a post or page (form saves, link changes, the trash,
/// publishing, revision restores, the scheduler), in the write's transaction, once the row is
/// written. Under the row's lock it:
///
/// 1. recomputes `search_text` (title, excerpt and body text, [`search::normalize`]) and
///    `reading_minutes` (200 words a minute, at least 1 with a body);
/// 2. recomputes its `public_media` rows (`public_media::sync`);
/// 3. writes a revision when the edited content changed (`revisions::record`, as `actor`);
/// 4. records the outbox event `{post|page}.{change}` with its cache tags and paths
///    ([`content_payload`]).
///
/// A form save that also changed tags calls this twice in one transaction (from
/// `after_relation_change`, then `after_save`): a `created` or `updated` event is recorded once,
/// as the first call reports everything ([`content_links_changed`]).
pub async fn content_changed(
    txn: &DatabaseTransaction,
    kind: Kind,
    id: i64,
    change: Change,
    actor: Option<i64>,
) -> AppResult<()> {
    sync(txn, kind, id, change, actor, None).await
}

/// [`content_changed`] after revision `revision_id` was restored: an update, whose pruning keeps
/// that revision (`revisions::record`).
pub async fn content_restored(
    txn: &DatabaseTransaction,
    kind: Kind,
    id: i64,
    actor: Option<i64>,
    revision_id: i64,
) -> AppResult<()> {
    sync(txn, kind, id, Change::Updated, actor, Some(revision_id)).await
}

async fn sync(
    txn: &DatabaseTransaction,
    kind: Kind,
    id: i64,
    change: Change,
    actor: Option<i64>,
    keep_revision: Option<i64>,
) -> AppResult<()> {
    let Some(mut row) = content::lock(txn, kind, id).await? else {
        return Err(AppError::internal(format!("content_sync: {} {id} is gone", kind.subject())));
    };
    let at = now();
    let (search_text, reading_minutes) = derived(&row);
    if row.search_text.as_deref() != Some(search_text.as_str()) || row.reading_minutes != reading_minutes {
        content::set(
            txn,
            kind,
            id,
            vec![
                (Col::SearchText, content::value(search_text.clone())),
                (Col::ReadingMinutes, content::value(reading_minutes)),
            ],
        )
        .await?;
        row.search_text = Some(search_text);
        row.reading_minutes = reading_minutes;
    }
    public_media::sync(txn, media_subject(kind), id).await?;

    let tag_ids = content::tag_ids(txn, kind, id).await?;
    let previous = revisions::latest(txn, kind, id).await?;
    revisions::record(txn, &row, &tag_ids, previous.as_ref(), actor, at, keep_revision).await?;

    let last_event = last_event_at(txn, kind, id).await?;
    if matches!(change, Change::Created | Change::Updated) && last_event.is_some_and(|e| e >= row.updated_at) {
        // Reported already by this write (see above).
        return Ok(());
    }
    // Slugs replaced since the last event (a rename in this write).
    let old_slugs = old_slugs(txn, kind, id, last_event).await?;
    let mut categories = Vec::new();
    let mut tags = Vec::new();
    if kind == Kind::Post {
        let mut category_ids: Vec<i64> = row.category_id.into_iter().collect();
        let mut all_tags = tag_ids.clone();
        if let Some(previous) = &previous {
            category_ids.extend(previous.category_id().filter(|c| Some(*c) != row.category_id));
            all_tags.extend(previous.tag_ids().into_iter().filter(|t| !tag_ids.contains(t)));
        }
        categories =
            slugs_of::<category::Entity>(txn, category::Column::Id, category::Column::Slug, &category_ids).await?;
        tags = slugs_of::<tag::Entity>(txn, tag::Column::Id, tag::Column::Slug, &all_tags).await?;
    }
    let menus = menus_linking(txn, kind, id).await?;
    let authors = author_slug(txn, row.author_id).await?;
    let subject = Subject {
        kind,
        id,
        slug: &row.slug,
        public: row.is_public_at(at),
        old_slugs: &old_slugs,
        categories: &categories,
        tags: &tags,
        menus: &menus,
        authors: &authors,
    };
    record_event(
        txn,
        &format!("{}.{}", kind.subject(), change.as_str()),
        kind.subject(),
        Some(id),
        content_payload(&subject),
    )
    .await
}

/// `after_relation_change` of a post's tags. The record is the post as the write stores it:
///
/// - in a create from the form (no event yet, `created_at = updated_at`), the post is reported
///   as created, with its tags, and `after_save` then records nothing more;
/// - otherwise (the relation manager, or an edit form), `updated_at` moves to now (links count
///   as an edit) and the post is reported as updated.
pub async fn content_links_changed(
    txn: &DatabaseTransaction,
    kind: Kind,
    record: &Content,
    actor: Option<i64>,
) -> AppResult<()> {
    let creating = record.created_at == record.updated_at && last_event_at(txn, kind, record.id).await?.is_none();
    if creating {
        return content_changed(txn, kind, record.id, Change::Created, actor).await;
    }
    content::set(txn, kind, record.id, vec![(Col::UpdatedAt, content::value(now()))]).await?;
    content_changed(txn, kind, record.id, Change::Updated, actor).await
}

/// `after_force_delete` of a post or page: its row is gone (and, with it, its tags and
/// revisions). Records the `deleted` event (the slugs it had redirect to nothing: their history
/// goes too) and drops its `public_media` rows.
pub async fn content_deleted(txn: &DatabaseTransaction, deleted: &Content) -> AppResult<()> {
    let (kind, id) = (deleted.kind, deleted.id);
    let old_slugs = old_slugs(txn, kind, id, None).await?;
    slug_history::Entity::delete_many()
        .filter(slug_history::Column::SubjectType.eq(kind.subject()))
        .filter(slug_history::Column::SubjectId.eq(id))
        .exec(txn)
        .await?;
    public_media::sync(txn, media_subject(kind), id).await?;
    let categories = match deleted.category_id {
        Some(c) => slugs_of::<category::Entity>(txn, category::Column::Id, category::Column::Slug, &[c]).await?,
        None => Vec::new(),
    };
    let menus = menus_linking(txn, kind, id).await?;
    let authors = author_slug(txn, deleted.author_id).await?;
    let subject = Subject {
        kind,
        id,
        slug: &deleted.slug,
        public: false,
        old_slugs: &old_slugs,
        categories: &categories,
        tags: &[],
        menus: &menus,
        authors: &authors,
    };
    record_event(txn, &format!("{}.deleted", kind.subject()), kind.subject(), Some(id), content_payload(&subject)).await
}

/// `search_text` and `reading_minutes` of a post or page as stored.
pub fn derived(row: &Content) -> (String, i32) {
    let body = row.body.as_ref().map(search::plain_text).unwrap_or_default();
    let text = [row.title.as_str(), row.excerpt.as_deref().unwrap_or_default(), body.as_str()].join("\n");
    let words = body.split_whitespace().count();
    let minutes = if body.trim().is_empty() { 0 } else { words.div_ceil(WORDS_PER_MINUTE).max(1) };
    (search::normalize(&text), i32::try_from(minutes).unwrap_or(i32::MAX))
}

/// Reading speed of `reading_minutes`.
pub const WORDS_PER_MINUTE: usize = 200;

fn media_subject(kind: Kind) -> public_media::Subject {
    match kind {
        Kind::Post => public_media::Subject::Post,
        Kind::Page => public_media::Subject::Page,
    }
}

/// When the newest outbox event of a post or page was recorded (this transaction's included).
async fn last_event_at(txn: &DatabaseTransaction, kind: Kind, id: i64) -> AppResult<Option<DateTimeUtc>> {
    Ok(content_event::Entity::find()
        .select_only()
        .column_as(content_event::Column::CreatedAt.max(), "at")
        .filter(content_event::Column::SubjectType.eq(kind.subject()))
        .filter(content_event::Column::SubjectId.eq(id))
        .into_tuple::<Option<DateTimeUtc>>()
        .one(txn)
        .await?
        .flatten())
}

/// The old slugs of a post or page kept since `after` (all of them when `None`).
async fn old_slugs(
    txn: &DatabaseTransaction,
    kind: Kind,
    id: i64,
    after: Option<DateTimeUtc>,
) -> AppResult<Vec<String>> {
    let mut query = slug_history::Entity::find()
        .select_only()
        .column(slug_history::Column::OldSlug)
        .filter(slug_history::Column::SubjectType.eq(kind.subject()))
        .filter(slug_history::Column::SubjectId.eq(id));
    if let Some(after) = after {
        query = query.filter(slug_history::Column::CreatedAt.gt(after));
    }
    Ok(query.order_by_asc(slug_history::Column::Id).into_tuple().all(txn).await?)
}

/// The slugs of the rows of `E` with these ids, in the ids' order (missing rows skipped).
async fn slugs_of<E: EntityTrait>(
    txn: &DatabaseTransaction,
    id_col: E::Column,
    slug_col: E::Column,
    ids: &[i64],
) -> AppResult<Vec<String>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let found: Vec<(i64, String)> = E::find()
        .select_only()
        .column(id_col)
        .column(slug_col)
        .filter(id_col.is_in(ids.iter().copied()))
        .into_tuple()
        .all(txn)
        .await?;
    Ok(ids.iter().filter_map(|id| found.iter().find(|(f, _)| f == id).map(|(_, s)| s.clone())).collect())
}

/// The profile slug of `author_id`, when they have a profile (none otherwise).
async fn author_slug(txn: &DatabaseTransaction, author_id: i64) -> AppResult<Vec<String>> {
    Ok(author_profile::Entity::find_by_id(author_id).one(txn).await?.map(|a| a.slug).into_iter().collect())
}

/// The keys of the menus with an item linking to this post or page.
async fn menus_linking(txn: &DatabaseTransaction, kind: Kind, id: i64) -> AppResult<Vec<String>> {
    let menu_ids: Vec<i64> = menu_item::Entity::find()
        .select_only()
        .column(menu_item::Column::MenuId)
        .filter(menu_item::Column::Kind.eq(kind.subject()))
        .filter(menu_item::Column::TargetId.eq(id))
        .distinct()
        .into_tuple()
        .all(txn)
        .await?;
    if menu_ids.is_empty() {
        return Ok(Vec::new());
    }
    Ok(menu::Entity::find()
        .select_only()
        .column(menu::Column::Key)
        .filter(menu::Column::Id.is_in(menu_ids))
        .order_by_asc(menu::Column::Key)
        .into_tuple()
        .all(txn)
        .await?)
}

/// What [`content_payload`] reports about a post or page.
#[derive(Debug)]
pub struct Subject<'a> {
    pub kind: Kind,
    pub id: i64,
    pub slug: &'a str,
    /// Whether the public site shows it now.
    pub public: bool,
    /// Slugs it had, replaced since the last event.
    pub old_slugs: &'a [String],
    /// Its category, and the one it left (posts).
    pub categories: &'a [String],
    /// Its tags, and the ones it lost (posts).
    pub tags: &'a [String],
    /// Menus linking to it (they show only public content).
    pub menus: &'a [String],
    /// Its author's profile slug (their page lists it).
    pub authors: &'a [String],
}

/// The payload of a post or page event:
///
/// - tags: `post:{id}` and `posts` (pages: `page:{id}`, `pages`), `category:{slug}` and
///   `tag:{slug}` of what it has and what it left, `author:{slug}` of its author's profile,
///   `menu:{key}` of menus linking to it, and `sitemap`;
/// - paths: `new` is its path while it is public; `old` its replaced slugs' paths, and its own
///   path once it is not public (a draft, scheduled, trashed or deleted).
pub fn content_payload(subject: &Subject<'_>) -> Value {
    let kind = subject.kind.subject();
    let mut tags = vec![format!("{kind}:{}", subject.id), subject.kind.resource().to_owned()];
    for category in subject.categories {
        push_unique(&mut tags, format!("category:{category}"));
    }
    for tag in subject.tags {
        push_unique(&mut tags, format!("tag:{tag}"));
    }
    for author in subject.authors {
        push_unique(&mut tags, format!("author:{author}"));
    }
    for menu in subject.menus {
        push_unique(&mut tags, format!("menu:{menu}"));
    }
    tags.push("sitemap".to_owned());
    let current = public_path(kind, subject.slug);
    let mut old: Vec<String> = Vec::new();
    for previous in subject.old_slugs.iter().filter(|s| s.as_str() != subject.slug) {
        push_unique(&mut old, public_path(kind, previous));
    }
    let new = if subject.public {
        vec![current]
    } else {
        push_unique(&mut old, current);
        Vec::new()
    };
    json!({ "tags": tags, "paths": { "old": old, "new": new } })
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

/// The payload of a category, tag or author change (`kind`: `category`, `tag` or `author`).
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
    fn a_post_payload_names_what_shows_it() {
        let old = ["cu".to_owned()];
        let categories = ["du-lich".to_owned()];
        let tags = ["rust".to_owned(), "go".to_owned()];
        let authors = ["an".to_owned()];
        let menus = ["primary".to_owned()];
        let mut subject = Subject {
            kind: Kind::Post,
            id: 7,
            slug: "moi",
            public: true,
            old_slugs: &old,
            categories: &categories,
            tags: &tags,
            menus: &menus,
            authors: &authors,
        };
        assert_eq!(
            content_payload(&subject),
            json!({
                "tags": ["post:7", "posts", "category:du-lich", "tag:rust", "tag:go", "author:an", "menu:primary", "sitemap"],
                "paths": { "old": ["/posts/cu"], "new": ["/posts/moi"] }
            })
        );
        // Not public: its own path is an old one.
        subject.public = false;
        assert_eq!(content_payload(&subject)["paths"], json!({ "old": ["/posts/cu", "/posts/moi"], "new": [] }));
        let page = Subject {
            kind: Kind::Page,
            old_slugs: &[],
            categories: &[],
            tags: &[],
            menus: &[],
            authors: &[],
            ..subject
        };
        assert_eq!(
            content_payload(&page),
            json!({ "tags": ["page:7", "pages", "sitemap"], "paths": { "old": ["/moi"], "new": [] } })
        );
    }

    #[test]
    fn reading_time_counts_body_words() {
        use crate::services::content::Content;
        let body = |words: usize| {
            json!({"type": "doc", "version": 1, "content": [
                {"type": "paragraph", "content": [{"type": "text", "text": "chữ ".repeat(words)}]}
            ]})
        };
        let row = |body: Option<Value>| Content {
            kind: Kind::Post,
            id: 1,
            title: "Xin chào Việt Nam".into(),
            slug: "x".into(),
            excerpt: Some("Phở".into()),
            body,
            cover_media_id: None,
            og_media_id: None,
            category_id: None,
            author_id: 1,
            status: "draft".into(),
            published_at: None,
            meta_title: None,
            meta_description: None,
            canonical_url: None,
            search_text: None,
            reading_minutes: 0,
            created_at: now(),
            updated_at: now(),
            deleted_at: None,
        };
        assert_eq!(derived(&row(None)), ("xin chao viet nam pho".to_owned(), 0));
        assert_eq!(derived(&row(Some(body(1)))).1, 1);
        assert_eq!(derived(&row(Some(body(200)))).1, 1);
        assert_eq!(derived(&row(Some(body(201)))).1, 2);
    }

    #[test]
    fn menus_have_one_tag() {
        assert_eq!(menu_payload("primary"), json!({ "tags": ["menu:primary"], "paths": { "old": [], "new": [] } }));
    }
}
