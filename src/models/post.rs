//! `posts`: articles, with a category, tags (`post_tag`), revisions and a publishing workflow
//! (`PostStatus`). Trashed posts keep `deleted_at` set, and their slug.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "posts")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub title: String,
    /// ASCII, unique over live and trashed posts (`services::slug`).
    #[sea_orm(unique)]
    pub slug: String,
    pub excerpt: Option<String>,
    /// A RustCMS rich-text document (`rustcms::admin::rich_text`).
    pub body: Option<Json>,
    pub cover_media_id: Option<Uuid>,
    pub og_media_id: Option<Uuid>,
    pub category_id: Option<i64>,
    pub author_id: i64,
    /// A [`PostStatus`] (`as_str`).
    pub status: String,
    /// When it went (or goes, once `scheduled`) live.
    pub published_at: Option<DateTimeUtc>,
    pub featured: bool,
    pub meta_title: Option<String>,
    pub meta_description: Option<String>,
    pub canonical_url: Option<String>,
    /// Title, excerpt and body text, normalized for search (`services::search::normalize`).
    pub search_text: Option<String>,
    pub reading_minutes: i32,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
    /// Set while the post is in the trash.
    pub deleted_at: Option<DateTimeUtc>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::category::Entity",
        from = "Column::CategoryId",
        to = "super::category::Column::Id",
        on_delete = "SetNull"
    )]
    Category,
    #[sea_orm(has_many = "super::post_tag::Entity")]
    PostTag,
    #[sea_orm(has_many = "super::post_revision::Entity")]
    Revisions,
}

impl Related<super::category::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Category.def()
    }
}

impl Related<super::post_tag::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::PostTag.def()
    }
}

/// The post's tags.
impl Related<super::tag::Entity> for Entity {
    fn to() -> RelationDef {
        super::post_tag::Relation::Tag.def()
    }

    fn via() -> Option<RelationDef> {
        Some(super::post_tag::Relation::Post.def().rev())
    }
}

impl Related<super::post_revision::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Revisions.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}

/// Where a post or page is in the publishing workflow (`posts.status`, `pages.status`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PostStatus {
    /// Being written.
    Draft,
    /// Submitted for review.
    Pending,
    /// Goes live at `published_at`.
    Scheduled,
    /// Live since `published_at`.
    Published,
}

impl PostStatus {
    /// Every status, in workflow order.
    pub const ALL: [PostStatus; 4] = [Self::Draft, Self::Pending, Self::Scheduled, Self::Published];

    /// The stored value.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Pending => "pending",
            Self::Scheduled => "scheduled",
            Self::Published => "published",
        }
    }

    /// The status a stored value names, if any.
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|status| status.as_str() == value)
    }
}

impl Model {
    /// The post's status (`None` for a value no code writes).
    pub fn status(&self) -> Option<PostStatus> {
        PostStatus::parse(&self.status)
    }
}

#[cfg(test)]
mod tests {
    use super::PostStatus;

    #[test]
    fn statuses_round_trip() {
        for status in PostStatus::ALL {
            assert_eq!(PostStatus::parse(status.as_str()), Some(status));
        }
        assert_eq!(PostStatus::parse("Draft"), None);
        assert_eq!(PostStatus::parse(""), None);
    }
}
