//! Posts and pages as one kind of thing: the columns they share ([`Content`]), read and
//! written per [`Kind`]. `publishing`, `content_sync` and `revisions` work on these, so a rule
//! is written once for both.

use sea_orm::prelude::{DateTimeUtc, Expr, Uuid};
use sea_orm::sea_query::SimpleExpr;
use sea_orm::{ColumnTrait, ConnectionTrait, DbErr, EntityTrait, QueryFilter, QuerySelect};
use serde_json::Value;

use crate::models::post::PostStatus;
use crate::models::{page, post, post_tag};

/// A post or a page.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Post,
    Page,
}

impl Kind {
    /// `post` or `page`: `subject_type` in `slug_history`, `content_events`, `public_media`.
    pub fn subject(self) -> &'static str {
        match self {
            Self::Post => "post",
            Self::Page => "page",
        }
    }

    /// The admin resource's slug, which prefixes its permissions (`posts.publish`).
    pub fn resource(self) -> &'static str {
        match self {
            Self::Post => "posts",
            Self::Page => "pages",
        }
    }

    /// `posts.{ability}` or `pages.{ability}`.
    pub fn permission(self, ability: &str) -> String {
        format!("{}.{ability}", self.resource())
    }
}

/// The columns posts and pages share, as stored. Pages have no category, tags or `featured`.
#[derive(Clone, Debug, PartialEq)]
pub struct Content {
    pub kind: Kind,
    pub id: i64,
    pub title: String,
    pub slug: String,
    pub excerpt: Option<String>,
    pub body: Option<Value>,
    pub cover_media_id: Option<Uuid>,
    pub og_media_id: Option<Uuid>,
    /// Posts only.
    pub category_id: Option<i64>,
    pub author_id: i64,
    pub status: String,
    pub published_at: Option<DateTimeUtc>,
    pub meta_title: Option<String>,
    pub meta_description: Option<String>,
    pub canonical_url: Option<String>,
    pub search_text: Option<String>,
    pub reading_minutes: i32,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
    pub deleted_at: Option<DateTimeUtc>,
}

impl From<post::Model> for Content {
    fn from(p: post::Model) -> Self {
        Self {
            kind: Kind::Post,
            id: p.id,
            title: p.title,
            slug: p.slug,
            excerpt: p.excerpt,
            body: p.body,
            cover_media_id: p.cover_media_id,
            og_media_id: p.og_media_id,
            category_id: p.category_id,
            author_id: p.author_id,
            status: p.status,
            published_at: p.published_at,
            meta_title: p.meta_title,
            meta_description: p.meta_description,
            canonical_url: p.canonical_url,
            search_text: p.search_text,
            reading_minutes: p.reading_minutes,
            created_at: p.created_at,
            updated_at: p.updated_at,
            deleted_at: p.deleted_at,
        }
    }
}

impl From<page::Model> for Content {
    fn from(p: page::Model) -> Self {
        Self {
            kind: Kind::Page,
            id: p.id,
            title: p.title,
            slug: p.slug,
            excerpt: p.excerpt,
            body: p.body,
            cover_media_id: p.cover_media_id,
            og_media_id: p.og_media_id,
            category_id: None,
            author_id: p.author_id,
            status: p.status,
            published_at: p.published_at,
            meta_title: p.meta_title,
            meta_description: p.meta_description,
            canonical_url: p.canonical_url,
            search_text: p.search_text,
            reading_minutes: p.reading_minutes,
            created_at: p.created_at,
            updated_at: p.updated_at,
            deleted_at: p.deleted_at,
        }
    }
}

impl Content {
    /// Its status (`None` for a value no code writes).
    pub fn status(&self) -> Option<PostStatus> {
        PostStatus::parse(&self.status)
    }

    /// Whether the public site shows it at `now`: published, due and not trashed.
    pub fn is_public_at(&self, now: DateTimeUtc) -> bool {
        self.status == PostStatus::Published.as_str()
            && self.deleted_at.is_none()
            && self.published_at.is_some_and(|at| at <= now)
    }
}

/// Post or page `id`, trashed or not, locked `FOR UPDATE` until the transaction ends.
pub async fn lock<C: ConnectionTrait>(db: &C, kind: Kind, id: i64) -> Result<Option<Content>, DbErr> {
    Ok(match kind {
        Kind::Post => post::Entity::find_by_id(id).lock_exclusive().one(db).await?.map(Content::from),
        Kind::Page => page::Entity::find_by_id(id).lock_exclusive().one(db).await?.map(Content::from),
    })
}

/// Post or page `id`, trashed or not, without a lock.
pub async fn find<C: ConnectionTrait>(db: &C, kind: Kind, id: i64) -> Result<Option<Content>, DbErr> {
    Ok(match kind {
        Kind::Post => post::Entity::find_by_id(id).one(db).await?.map(Content::from),
        Kind::Page => page::Entity::find_by_id(id).one(db).await?.map(Content::from),
    })
}

/// A column both tables have.
#[derive(Clone, Copy, Debug)]
pub enum Col {
    Title,
    Excerpt,
    Body,
    CoverMediaId,
    OgMediaId,
    /// Posts only: ignored for pages.
    CategoryId,
    Status,
    PublishedAt,
    MetaTitle,
    MetaDescription,
    CanonicalUrl,
    SearchText,
    ReadingMinutes,
    UpdatedAt,
}

/// Sets `values` on post or page `id` (one `UPDATE`).
pub async fn set<C: ConnectionTrait>(db: &C, kind: Kind, id: i64, values: Vec<(Col, SimpleExpr)>) -> Result<(), DbErr> {
    match kind {
        Kind::Post => {
            let mut update = post::Entity::update_many().filter(post::Column::Id.eq(id));
            for (col, value) in values {
                let col = match col {
                    Col::Title => post::Column::Title,
                    Col::Excerpt => post::Column::Excerpt,
                    Col::Body => post::Column::Body,
                    Col::CoverMediaId => post::Column::CoverMediaId,
                    Col::OgMediaId => post::Column::OgMediaId,
                    Col::CategoryId => post::Column::CategoryId,
                    Col::Status => post::Column::Status,
                    Col::PublishedAt => post::Column::PublishedAt,
                    Col::MetaTitle => post::Column::MetaTitle,
                    Col::MetaDescription => post::Column::MetaDescription,
                    Col::CanonicalUrl => post::Column::CanonicalUrl,
                    Col::SearchText => post::Column::SearchText,
                    Col::ReadingMinutes => post::Column::ReadingMinutes,
                    Col::UpdatedAt => post::Column::UpdatedAt,
                };
                update = update.col_expr(col, value);
            }
            update.exec(db).await?;
        }
        Kind::Page => {
            let mut update = page::Entity::update_many().filter(page::Column::Id.eq(id));
            for (col, value) in values {
                let col = match col {
                    Col::Title => page::Column::Title,
                    Col::Excerpt => page::Column::Excerpt,
                    Col::Body => page::Column::Body,
                    Col::CoverMediaId => page::Column::CoverMediaId,
                    Col::OgMediaId => page::Column::OgMediaId,
                    Col::CategoryId => continue,
                    Col::Status => page::Column::Status,
                    Col::PublishedAt => page::Column::PublishedAt,
                    Col::MetaTitle => page::Column::MetaTitle,
                    Col::MetaDescription => page::Column::MetaDescription,
                    Col::CanonicalUrl => page::Column::CanonicalUrl,
                    Col::SearchText => page::Column::SearchText,
                    Col::ReadingMinutes => page::Column::ReadingMinutes,
                    Col::UpdatedAt => page::Column::UpdatedAt,
                };
                update = update.col_expr(col, value);
            }
            update.exec(db).await?;
        }
    }
    Ok(())
}

/// A refusal of the whole form (an action's dialog, a record's save), not of one field.
pub fn form_error(message: impl Into<String>) -> rustcms::ValidationErrors {
    let mut errors = rustcms::ValidationErrors::new();
    errors.add_form(message);
    errors
}

/// A value for [`set`].
pub fn value<V: Into<sea_orm::Value>>(v: V) -> SimpleExpr {
    Expr::value(v)
}

/// The tag ids of post `id`, in their order (none for a page).
pub async fn tag_ids<C: ConnectionTrait>(db: &C, kind: Kind, id: i64) -> Result<Vec<i64>, DbErr> {
    if kind == Kind::Page {
        return Ok(Vec::new());
    }
    use sea_orm::QueryOrder;
    post_tag::Entity::find()
        .select_only()
        .column(post_tag::Column::TagId)
        .filter(post_tag::Column::PostId.eq(id))
        .order_by_asc(post_tag::Column::Position)
        .order_by_asc(post_tag::Column::TagId)
        .into_tuple()
        .all(db)
        .await
}
