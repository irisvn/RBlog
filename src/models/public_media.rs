//! `public_media`: which public content uses which media. A media item is public while at least
//! one row names it.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "public_media")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub media_id: Uuid,
    /// `post`, `page`, `author`, …
    #[sea_orm(primary_key, auto_increment = false)]
    pub subject_type: String,
    #[sea_orm(primary_key, auto_increment = false)]
    pub subject_id: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
