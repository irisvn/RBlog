//! `menu_items`: the links of a menu, in `position` order, optionally under a parent item.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "menu_items")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub menu_id: i64,
    /// The parent item (same menu), deleted with it.
    pub parent_id: Option<i64>,
    pub label: String,
    /// `url`, `post`, `page`, `category` or `tag`.
    pub kind: String,
    /// The linked post, page, category or tag (`kind` other than `url`).
    pub target_id: Option<i64>,
    /// The link (`kind` = `url`).
    pub url: Option<String>,
    pub new_tab: bool,
    pub position: i32,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::menu::Entity",
        from = "Column::MenuId",
        to = "super::menu::Column::Id",
        on_delete = "Cascade"
    )]
    Menu,
    #[sea_orm(belongs_to = "Entity", from = "Column::ParentId", to = "Column::Id", on_delete = "Cascade")]
    Parent,
}

impl Related<super::menu::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Menu.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
