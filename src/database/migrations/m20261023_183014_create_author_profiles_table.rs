use rustcms::async_trait;
use rustcms::migration::*;

use super::refuse_if_rows;

/// `author_profiles`: the public side of a user who writes (slug, bio, avatar, website), one per
/// user, deleted with the user (`CASCADE`). The avatar media column has no foreign key: RustCMS
/// soft-deletes media rows and checks references itself.
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
                .table(AuthorProfiles::Table)
                .col(ColumnDef::new(AuthorProfiles::UserId).big_integer().not_null().primary_key())
                .col(ColumnDef::new(AuthorProfiles::Slug).string_len(160).not_null())
                .col(ColumnDef::new(AuthorProfiles::Bio).text().null())
                .col(ColumnDef::new(AuthorProfiles::AvatarMediaId).uuid().null())
                .col(ColumnDef::new(AuthorProfiles::Website).string_len(2048).null())
                .col(ColumnDef::new(AuthorProfiles::CreatedAt).instant_now(manager).not_null())
                .col(ColumnDef::new(AuthorProfiles::UpdatedAt).instant_now(manager).not_null())
                .index(Index::create().name("author_profiles_slug_key").col(AuthorProfiles::Slug).unique())
                .foreign_key(
                    ForeignKey::create()
                        .name("author_profiles_user_id_fkey")
                        .from(AuthorProfiles::Table, AuthorProfiles::UserId)
                        .to(Users::Table, Users::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await
    }

    /// Refuses while profiles exist: they would be lost.
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        refuse_if_rows(manager, &["author_profiles"], "1=1", "delete the author profiles first").await?;
        drop_table(manager, AuthorProfiles::Table).await
    }
}

#[derive(DeriveIden)]
enum AuthorProfiles {
    Table,
    UserId,
    Slug,
    Bio,
    AvatarMediaId,
    Website,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Users {
    Table,
    Id,
}
