//! `slug_history`: slugs a post, page, category or tag had before, so the frontend can redirect.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "slug_history")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    /// `post`, `page`, `category` or `tag`.
    pub subject_type: String,
    pub subject_id: i64,
    /// Unique per `subject_type`.
    pub old_slug: String,
    pub created_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
