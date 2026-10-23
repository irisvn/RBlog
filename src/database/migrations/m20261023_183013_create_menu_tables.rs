use rustcms::async_trait;
use rustcms::migration::*;

use super::refuse_if_rows;

/// `menus` (found by a unique `key`: `primary`, `footer`, …) and their `menu_items`, which link to
/// a URL or to a post, page, category or tag (`kind` + `target_id`). Items go with their menu and
/// with their parent item (`CASCADE`).
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait]
impl MigrationTrait for Migration {
    /// Schema change and migration record commit together, so a failed run can be retried.
    fn use_transaction(&self) -> Option<bool> {
        Some(true)
    }

    /// `create_table` skips what already exists (MySQL commits DDL statement by statement, so a
    /// failed run is retried on a partly changed schema).
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        create_table(
            manager,
            Table::create()
                .table(Menus::Table)
                .col(ColumnDef::new(Menus::Id).big_integer().not_null().auto_increment().primary_key())
                .col(ColumnDef::new(Menus::Key).string_len(32).not_null())
                .col(ColumnDef::new(Menus::Name).string_len(120).not_null())
                .col(ColumnDef::new(Menus::CreatedAt).instant_now(manager).not_null())
                .col(ColumnDef::new(Menus::UpdatedAt).instant_now(manager).not_null())
                .index(Index::create().name("menus_key_key").col(Menus::Key).unique())
                .to_owned(),
        )
        .await?;
        create_table(
            manager,
            Table::create()
                .table(MenuItems::Table)
                .col(ColumnDef::new(MenuItems::Id).big_integer().not_null().auto_increment().primary_key())
                .col(ColumnDef::new(MenuItems::MenuId).big_integer().not_null())
                .col(ColumnDef::new(MenuItems::ParentId).big_integer().null())
                .col(ColumnDef::new(MenuItems::Label).string_len(120).not_null())
                // url | post | page | category | tag
                .col(ColumnDef::new(MenuItems::Kind).string_len(16).not_null())
                .col(ColumnDef::new(MenuItems::TargetId).big_integer().null())
                .col(ColumnDef::new(MenuItems::Url).string_len(2048).null())
                .col(ColumnDef::new(MenuItems::NewTab).boolean().not_null().default(false))
                .col(ColumnDef::new(MenuItems::Position).integer().not_null().default(0))
                .col(ColumnDef::new(MenuItems::CreatedAt).instant_now(manager).not_null())
                .col(ColumnDef::new(MenuItems::UpdatedAt).instant_now(manager).not_null())
                .index(
                    Index::create()
                        .name("menu_items_menu_id_parent_id_position_idx")
                        .col(MenuItems::MenuId)
                        .col(MenuItems::ParentId)
                        .col(MenuItems::Position),
                )
                .index(Index::create().name("menu_items_parent_id_idx").col(MenuItems::ParentId))
                .foreign_key(
                    ForeignKey::create()
                        .name("menu_items_menu_id_fkey")
                        .from(MenuItems::Table, MenuItems::MenuId)
                        .to(Menus::Table, Menus::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("menu_items_parent_id_fkey")
                        .from(MenuItems::Table, MenuItems::ParentId)
                        .to(MenuItems::Table, MenuItems::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await
    }

    /// Refuses while menus exist: they would be lost.
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        refuse_if_rows(manager, &["menu_items", "menus"], "1=1", "delete the menus first").await?;
        drop_table(manager, MenuItems::Table).await?;
        drop_table(manager, Menus::Table).await
    }
}

#[derive(DeriveIden)]
enum Menus {
    Table,
    Id,
    Key,
    Name,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum MenuItems {
    Table,
    Id,
    MenuId,
    ParentId,
    Label,
    Kind,
    TargetId,
    Url,
    NewTab,
    Position,
    CreatedAt,
    UpdatedAt,
}
