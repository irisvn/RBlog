use rustcms::async_trait;
use rustcms::migration::*;

use super::refuse_if_rows;

/// What keeps the frontend in sync with the content:
/// - `content_events`: the transactional outbox (written in the change's own transaction,
///   delivered later as webhooks);
/// - `public_media`: which published content uses which media (an image is public while one
///   row names it);
/// - `preview_tokens`: hashed tokens that show one unpublished post or page;
/// - `slug_history`: old slugs, so the frontend can redirect.
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
                .table(ContentEvents::Table)
                .col(ColumnDef::new(ContentEvents::Id).uuid().not_null().primary_key())
                .col(ColumnDef::new(ContentEvents::Kind).string_len(48).not_null())
                .col(ColumnDef::new(ContentEvents::SubjectType).string_len(16).not_null())
                .col(ColumnDef::new(ContentEvents::SubjectId).big_integer().null())
                // Cache tags, old and new paths.
                .col(ColumnDef::new(ContentEvents::Payload).json().not_null())
                .col(ColumnDef::new(ContentEvents::CreatedAt).instant_now(manager).not_null())
                .col(ColumnDef::new(ContentEvents::DeliveredAt).instant(manager).null())
                .col(ColumnDef::new(ContentEvents::Attempts).integer().not_null().default(0))
                .col(ColumnDef::new(ContentEvents::LastError).string_len(500).null())
                // The poller: undelivered events, oldest first.
                .index(
                    Index::create()
                        .name("content_events_delivered_at_created_at_idx")
                        .col(ContentEvents::DeliveredAt)
                        .col(ContentEvents::CreatedAt),
                )
                .to_owned(),
        )
        .await?;
        create_table(
            manager,
            Table::create()
                .table(PublicMedia::Table)
                .col(ColumnDef::new(PublicMedia::MediaId).uuid().not_null())
                .col(ColumnDef::new(PublicMedia::SubjectType).string_len(16).not_null())
                .col(ColumnDef::new(PublicMedia::SubjectId).big_integer().not_null())
                .primary_key(
                    Index::create().col(PublicMedia::MediaId).col(PublicMedia::SubjectType).col(PublicMedia::SubjectId),
                )
                .index(
                    Index::create()
                        .name("public_media_subject_type_subject_id_idx")
                        .col(PublicMedia::SubjectType)
                        .col(PublicMedia::SubjectId),
                )
                .to_owned(),
        )
        .await?;
        create_table(
            manager,
            Table::create()
                .table(PreviewTokens::Table)
                .col(ColumnDef::new(PreviewTokens::Id).big_integer().not_null().auto_increment().primary_key())
                // SHA-256 (hex) of the token; the token itself is never stored.
                .col(ColumnDef::new(PreviewTokens::TokenHash).char_len(64).not_null())
                .col(ColumnDef::new(PreviewTokens::SubjectType).string_len(16).not_null())
                .col(ColumnDef::new(PreviewTokens::SubjectId).big_integer().not_null())
                .col(ColumnDef::new(PreviewTokens::CreatedBy).big_integer().not_null())
                .col(ColumnDef::new(PreviewTokens::ExpiresAt).instant(manager).not_null())
                .col(ColumnDef::new(PreviewTokens::RevokedAt).instant(manager).null())
                .col(ColumnDef::new(PreviewTokens::CreatedAt).instant_now(manager).not_null())
                .index(Index::create().name("preview_tokens_token_hash_key").col(PreviewTokens::TokenHash).unique())
                .index(
                    Index::create()
                        .name("preview_tokens_subject_type_subject_id_idx")
                        .col(PreviewTokens::SubjectType)
                        .col(PreviewTokens::SubjectId),
                )
                .index(Index::create().name("preview_tokens_created_by_idx").col(PreviewTokens::CreatedBy))
                .foreign_key(
                    ForeignKey::create()
                        .name("preview_tokens_created_by_fkey")
                        .from(PreviewTokens::Table, PreviewTokens::CreatedBy)
                        .to(Users::Table, Users::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;
        create_table(
            manager,
            Table::create()
                .table(SlugHistory::Table)
                .col(ColumnDef::new(SlugHistory::Id).big_integer().not_null().auto_increment().primary_key())
                // post | page | category | tag
                .col(ColumnDef::new(SlugHistory::SubjectType).string_len(16).not_null())
                .col(ColumnDef::new(SlugHistory::SubjectId).big_integer().not_null())
                .col(ColumnDef::new(SlugHistory::OldSlug).string_len(200).not_null())
                .col(ColumnDef::new(SlugHistory::CreatedAt).instant_now(manager).not_null())
                .index(
                    Index::create()
                        .name("slug_history_subject_type_old_slug_key")
                        .col(SlugHistory::SubjectType)
                        .col(SlugHistory::OldSlug)
                        .unique(),
                )
                .to_owned(),
        )
        .await
    }

    /// Refuses while events wait for delivery or old slugs would stop redirecting. Delivered
    /// events, `public_media` (rebuilt by its backfill) and preview tokens (short-lived) may go.
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        refuse_if_rows(
            manager,
            &["content_events"],
            "delivered_at IS NULL",
            "deliver the pending content events first",
        )
        .await?;
        refuse_if_rows(manager, &["slug_history"], "1=1", "the old slugs would stop redirecting").await?;
        drop_table(manager, SlugHistory::Table).await?;
        drop_table(manager, PreviewTokens::Table).await?;
        drop_table(manager, PublicMedia::Table).await?;
        drop_table(manager, ContentEvents::Table).await
    }
}

#[derive(DeriveIden)]
enum ContentEvents {
    Table,
    Id,
    Kind,
    SubjectType,
    SubjectId,
    Payload,
    CreatedAt,
    DeliveredAt,
    Attempts,
    LastError,
}

#[derive(DeriveIden)]
enum PublicMedia {
    Table,
    MediaId,
    SubjectType,
    SubjectId,
}

#[derive(DeriveIden)]
enum PreviewTokens {
    Table,
    Id,
    TokenHash,
    SubjectType,
    SubjectId,
    CreatedBy,
    ExpiresAt,
    RevokedAt,
    CreatedAt,
}

#[derive(DeriveIden)]
enum SlugHistory {
    Table,
    Id,
    SubjectType,
    SubjectId,
    OldSlug,
    CreatedAt,
}

#[derive(DeriveIden)]
enum Users {
    Table,
    Id,
}
