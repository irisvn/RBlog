//! `page_revisions`: snapshots of a page's edited content, written by the server when
//! `content_hash` changes.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "page_revisions")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub page_id: i64,
    pub title: String,
    pub excerpt: Option<String>,
    pub body: Option<Json>,
    /// Cover, OG and SEO fields at the time.
    pub snapshot: Json,
    /// SHA-256 (hex) of the edited content.
    pub content_hash: String,
    /// Who saved it (`None` once the user is deleted).
    pub author_id: Option<i64>,
    pub created_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::page::Entity",
        from = "Column::PageId",
        to = "super::page::Column::Id",
        on_delete = "Cascade"
    )]
    Page,
}

impl Related<super::page::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Page.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
