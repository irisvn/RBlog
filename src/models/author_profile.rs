//! `author_profiles`: the public side of a user who writes, one per user.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "author_profiles")]
pub struct Model {
    /// The `users` row (deleted with it).
    #[sea_orm(primary_key, auto_increment = false)]
    pub user_id: i64,
    /// ASCII, unique (`services::slug`).
    #[sea_orm(unique)]
    pub slug: String,
    pub bio: Option<String>,
    pub avatar_media_id: Option<Uuid>,
    pub website: Option<String>,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
