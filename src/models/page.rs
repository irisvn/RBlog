//! `pages`: standalone content (About, Contact, …): posts without category, tags or `featured`.
//! Same workflow (`post::PostStatus`) and soft delete as posts.

use sea_orm::Condition;
use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "pages")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub title: String,
    /// ASCII, unique over live and trashed pages (`services::slug`).
    #[sea_orm(unique)]
    pub slug: String,
    pub excerpt: Option<String>,
    /// A RustCMS rich-text document (`rustcms::admin::rich_text`).
    pub body: Option<Json>,
    pub cover_media_id: Option<Uuid>,
    pub og_media_id: Option<Uuid>,
    pub author_id: i64,
    /// A [`super::post::PostStatus`] (`as_str`).
    pub status: String,
    pub published_at: Option<DateTimeUtc>,
    pub meta_title: Option<String>,
    pub meta_description: Option<String>,
    pub canonical_url: Option<String>,
    /// Title, excerpt and body text, normalized for search (`services::search::normalize`).
    pub search_text: Option<String>,
    pub reading_minutes: i32,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
    /// Set while the page is in the trash.
    pub deleted_at: Option<DateTimeUtc>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::page_revision::Entity")]
    Revisions,
}

impl Related<super::page_revision::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Revisions.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}

/// Pages the public site shows at `now`: published, due (`published_at <= now`) and not trashed.
pub fn public_at(now: DateTimeUtc) -> Condition {
    Condition::all()
        .add(Column::Status.eq(super::post::PostStatus::Published.as_str()))
        .add(Column::PublishedAt.lte(now))
        .add(Column::DeletedAt.is_null())
}

impl Model {
    /// The page's status (`None` for a value no code writes).
    pub fn status(&self) -> Option<super::post::PostStatus> {
        super::post::PostStatus::parse(&self.status)
    }

    /// Whether the public site shows it at `now` (see [`public_at`]).
    pub fn is_public_at(&self, now: DateTimeUtc) -> bool {
        self.status == super::post::PostStatus::Published.as_str()
            && self.deleted_at.is_none()
            && self.published_at.is_some_and(|at| at <= now)
    }
}
