use rustcms::async_trait;
use rustcms::migration::*;

use super::refuse_if_rows;

/// `post_revisions` and `page_revisions`: snapshots of the edited content, written by the server
/// only when `content_hash` changes. They go with their post or page (`CASCADE`); a deleted
/// author leaves the revision without one (`SET NULL`).
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
        create_table(manager, revisions(manager, "post_revisions", "post_id", "posts")).await?;
        create_table(manager, revisions(manager, "page_revisions", "page_id", "pages")).await
    }

    /// Refuses while revisions exist: the history would be lost.
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        refuse_if_rows(manager, &["page_revisions", "post_revisions"], "1=1", "delete the revisions first").await?;
        drop_table(manager, Alias::new("page_revisions")).await?;
        drop_table(manager, Alias::new("post_revisions")).await
    }
}

/// A revisions table `name` whose `parent` column points to `parent_table`.
fn revisions(manager: &SchemaManager, name: &str, parent: &str, parent_table: &str) -> TableCreateStatement {
    let (table, column) = (Alias::new(name), Alias::new(parent));
    Table::create()
        .table(table.clone())
        .col(ColumnDef::new(Revision::Id).big_integer().not_null().auto_increment().primary_key())
        .col(ColumnDef::new(column.clone()).big_integer().not_null())
        .col(ColumnDef::new(Revision::Title).string_len(255).not_null())
        .col(ColumnDef::new(Revision::Excerpt).text().null())
        // Rich text: no default, ever (CLAUDE.md).
        .col(ColumnDef::new(Revision::Body).json().null())
        // Cover, OG, category, tag ids and SEO fields at the time of the revision.
        .col(ColumnDef::new(Revision::Snapshot).json().not_null())
        // SHA-256 (hex) of the edited content: a save that changes nothing writes no revision.
        .col(ColumnDef::new(Revision::ContentHash).char_len(64).not_null())
        .col(ColumnDef::new(Revision::AuthorId).big_integer().null())
        .col(ColumnDef::new(Revision::CreatedAt).instant_now(manager).not_null())
        .index(
            Index::create()
                .name(format!("{name}_{parent}_created_at_id_idx"))
                .col(column.clone())
                .col(Revision::CreatedAt)
                .col(Revision::Id),
        )
        .index(Index::create().name(format!("{name}_author_id_idx")).col(Revision::AuthorId))
        .foreign_key(
            ForeignKey::create()
                .name(format!("{name}_{parent}_fkey"))
                .from(table.clone(), column)
                .to(Alias::new(parent_table), Revision::Id)
                .on_delete(ForeignKeyAction::Cascade),
        )
        .foreign_key(
            ForeignKey::create()
                .name(format!("{name}_author_id_fkey"))
                .from(table, Revision::AuthorId)
                .to(Users::Table, Users::Id)
                .on_delete(ForeignKeyAction::SetNull),
        )
        .to_owned()
}

#[derive(DeriveIden)]
enum Revision {
    Id,
    Title,
    Excerpt,
    Body,
    Snapshot,
    ContentHash,
    AuthorId,
    CreatedAt,
}

#[derive(DeriveIden)]
enum Users {
    Table,
    Id,
}
