use rustcms::async_trait;
use rustcms::migration::*;

/// `content_events (subject_type, subject_id, created_at)`: every write of a post or page reads
/// the newest event of its subject (`services::content_sync::content_changed`), and the
/// scheduler does it for each post that falls due. Without it, each read scans the outbox.
#[derive(DeriveMigrationName)]
pub struct Migration;

const NAME: &str = "content_events_subject_type_subject_id_created_at_idx";

#[async_trait]
impl MigrationTrait for Migration {
    /// Schema change and migration record commit together, so a failed run can be retried.
    fn use_transaction(&self) -> Option<bool> {
        Some(true)
    }

    /// `create_index` skips an index that already exists (MySQL commits DDL statement by
    /// statement).
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        create_index(
            manager,
            ContentEvents::Table,
            Index::create()
                .name(NAME)
                .table(ContentEvents::Table)
                .col(ContentEvents::SubjectType)
                .col(ContentEvents::SubjectId)
                .col(ContentEvents::CreatedAt)
                .to_owned(),
        )
        .await
    }

    /// An index holds no data: dropping it loses nothing.
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        if manager.has_index("content_events", NAME).await? {
            manager.drop_index(Index::drop().name(NAME).table(ContentEvents::Table).to_owned()).await?;
        }
        Ok(())
    }
}

#[derive(DeriveIden)]
enum ContentEvents {
    Table,
    SubjectType,
    SubjectId,
    CreatedAt,
}
