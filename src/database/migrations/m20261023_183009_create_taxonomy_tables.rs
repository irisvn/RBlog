use rustcms::async_trait;
use rustcms::migration::*;

use super::refuse_if_rows;

/// The taxonomy of posts: `categories` (one per post, with SEO fields) and `tags` (many per
/// post). Slugs are ASCII (`services::slug`), unique over the whole table.
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
                .table(Categories::Table)
                .col(ColumnDef::new(Categories::Id).big_integer().not_null().auto_increment().primary_key())
                .col(ColumnDef::new(Categories::Name).string_len(120).not_null())
                .col(ColumnDef::new(Categories::Slug).string_len(160).not_null())
                .col(ColumnDef::new(Categories::Description).text().null())
                .col(ColumnDef::new(Categories::MetaTitle).string_len(255).null())
                .col(ColumnDef::new(Categories::MetaDescription).string_len(500).null())
                .col(ColumnDef::new(Categories::CreatedAt).instant_now(manager).not_null())
                .col(ColumnDef::new(Categories::UpdatedAt).instant_now(manager).not_null())
                .index(Index::create().name("categories_slug_key").col(Categories::Slug).unique())
                .to_owned(),
        )
        .await?;
        create_table(
            manager,
            Table::create()
                .table(Tags::Table)
                .col(ColumnDef::new(Tags::Id).big_integer().not_null().auto_increment().primary_key())
                .col(ColumnDef::new(Tags::Name).string_len(120).not_null())
                .col(ColumnDef::new(Tags::Slug).string_len(160).not_null())
                .col(ColumnDef::new(Tags::Description).text().null())
                .col(ColumnDef::new(Tags::CreatedAt).instant_now(manager).not_null())
                .col(ColumnDef::new(Tags::UpdatedAt).instant_now(manager).not_null())
                .index(Index::create().name("tags_slug_key").col(Tags::Slug).unique())
                .to_owned(),
        )
        .await
    }

    /// Refuses while either table has rows: they would be lost.
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        refuse_if_rows(manager, &["tags", "categories"], "1=1", "delete the categories and tags first").await?;
        drop_table(manager, Tags::Table).await?;
        drop_table(manager, Categories::Table).await
    }
}

#[derive(DeriveIden)]
enum Categories {
    Table,
    Id,
    Name,
    Slug,
    Description,
    MetaTitle,
    MetaDescription,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Tags {
    Table,
    Id,
    Name,
    Slug,
    Description,
    CreatedAt,
    UpdatedAt,
}
