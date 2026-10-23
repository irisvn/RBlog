use rustcms::async_trait;
use rustcms::migration::*;

use super::refuse_if_rows;

/// `posts` (with a category and the `post_tag` pivot) and `pages`, which share their columns:
/// rich-text body (JSON), cover and OG media, workflow status, SEO fields, the normalized
/// `search_text`, and soft delete (`deleted_at`). Slugs stay unique over trashed rows too.
///
/// The media columns have no foreign key: RustCMS soft-deletes media rows and checks references
/// itself.
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
        let mut posts = Table::create();
        posts
            .table(Posts::Table)
            .col(ColumnDef::new(Content::Id).big_integer().not_null().auto_increment().primary_key());
        content_columns(&mut posts, manager, "posts");
        create_table(
            manager,
            posts
                .col(ColumnDef::new(Posts::CategoryId).big_integer().null())
                .col(ColumnDef::new(Posts::Featured).boolean().not_null().default(false))
                .index(Index::create().name("posts_category_id_idx").col(Posts::CategoryId))
                .foreign_key(
                    ForeignKey::create()
                        .name("posts_category_id_fkey")
                        .from(Posts::Table, Posts::CategoryId)
                        .to(Categories::Table, Categories::Id)
                        .on_delete(ForeignKeyAction::SetNull),
                )
                .to_owned(),
        )
        .await?;
        create_table(
            manager,
            Table::create()
                .table(PostTag::Table)
                .col(ColumnDef::new(PostTag::PostId).big_integer().not_null())
                .col(ColumnDef::new(PostTag::TagId).big_integer().not_null())
                .col(ColumnDef::new(PostTag::Position).integer().not_null().default(0))
                .primary_key(Index::create().col(PostTag::PostId).col(PostTag::TagId))
                .index(Index::create().name("post_tag_tag_id_post_id_idx").col(PostTag::TagId).col(PostTag::PostId))
                .foreign_key(
                    ForeignKey::create()
                        .name("post_tag_post_id_fkey")
                        .from(PostTag::Table, PostTag::PostId)
                        .to(Posts::Table, Content::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("post_tag_tag_id_fkey")
                        .from(PostTag::Table, PostTag::TagId)
                        .to(Tags::Table, Tags::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;
        let mut pages = Table::create();
        pages
            .table(Pages::Table)
            .col(ColumnDef::new(Content::Id).big_integer().not_null().auto_increment().primary_key());
        content_columns(&mut pages, manager, "pages");
        create_table(manager, pages).await
    }

    /// Refuses while posts or pages exist: they would be lost.
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        refuse_if_rows(manager, &["pages", "posts"], "1=1", "delete the posts and pages first").await?;
        drop_table(manager, Pages::Table).await?;
        drop_table(manager, PostTag::Table).await?;
        drop_table(manager, Posts::Table).await
    }
}

/// The columns, indexes and foreign keys `posts` and `pages` share (after `id`).
fn content_columns(table: &mut TableCreateStatement, manager: &SchemaManager, name: &str) {
    table
        .col(ColumnDef::new(Content::Title).string_len(255).not_null())
        .col(ColumnDef::new(Content::Slug).string_len(200).not_null())
        .col(ColumnDef::new(Content::Excerpt).text().null())
        // Rich text: no default, ever (CLAUDE.md).
        .col(ColumnDef::new(Content::Body).json().null())
        .col(ColumnDef::new(Content::CoverMediaId).uuid().null())
        .col(ColumnDef::new(Content::OgMediaId).uuid().null())
        .col(ColumnDef::new(Content::AuthorId).big_integer().not_null())
        // draft | pending | scheduled | published (`models::post::PostStatus`).
        .col(ColumnDef::new(Content::Status).string_len(16).not_null().default("draft"))
        .col(ColumnDef::new(Content::PublishedAt).instant(manager).null())
        .col(ColumnDef::new(Content::MetaTitle).string_len(255).null())
        .col(ColumnDef::new(Content::MetaDescription).string_len(500).null())
        .col(ColumnDef::new(Content::CanonicalUrl).string_len(2048).null())
        // Accent-free text for LIKE search (`services::search`); `Text` is only 64 KiB.
        .col(ColumnDef::new(Content::SearchText).custom(Alias::new("mediumtext")).null())
        .col(ColumnDef::new(Content::ReadingMinutes).integer().not_null().default(0))
        .col(ColumnDef::new(Content::CreatedAt).instant_now(manager).not_null())
        .col(ColumnDef::new(Content::UpdatedAt).instant_now(manager).not_null())
        .col(ColumnDef::new(Content::DeletedAt).instant(manager).null())
        .index(Index::create().name(format!("{name}_slug_key")).col(Content::Slug).unique())
        // The scheduler: due `scheduled` rows.
        .index(
            Index::create()
                .name(format!("{name}_status_deleted_at_published_at_id_idx"))
                .col(Content::Status)
                .col(Content::DeletedAt)
                .col(Content::PublishedAt)
                .col(Content::Id),
        )
        // Lists: live (or trashed) rows of a status, newest first.
        .index(
            Index::create()
                .name(format!("{name}_deleted_at_status_published_at_id_idx"))
                .col(Content::DeletedAt)
                .col(Content::Status)
                .col(Content::PublishedAt)
                .col(Content::Id),
        )
        .index(Index::create().name(format!("{name}_author_id_idx")).col(Content::AuthorId))
        .foreign_key(
            ForeignKey::create()
                .name(format!("{name}_author_id_fkey"))
                .from(Alias::new(name), Content::AuthorId)
                .to(Users::Table, Users::Id)
                .on_delete(ForeignKeyAction::Restrict),
        );
}

#[derive(DeriveIden)]
enum Content {
    Id,
    Title,
    Slug,
    Excerpt,
    Body,
    CoverMediaId,
    OgMediaId,
    AuthorId,
    Status,
    PublishedAt,
    MetaTitle,
    MetaDescription,
    CanonicalUrl,
    SearchText,
    ReadingMinutes,
    CreatedAt,
    UpdatedAt,
    DeletedAt,
}

#[derive(DeriveIden)]
enum Posts {
    Table,
    CategoryId,
    Featured,
}

#[derive(DeriveIden)]
enum Pages {
    Table,
}

#[derive(DeriveIden)]
enum PostTag {
    Table,
    PostId,
    TagId,
    Position,
}

#[derive(DeriveIden)]
enum Categories {
    Table,
    Id,
}

#[derive(DeriveIden)]
enum Tags {
    Table,
    Id,
}

#[derive(DeriveIden)]
enum Users {
    Table,
    Id,
}
