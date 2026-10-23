use rustcms::async_trait;
use rustcms::migration::*;

use super::refuse_if_rows;

/// What the webhook delivery needs on `content_events` (`crate::services::events`):
/// - `job_id`: the delivery job last dispatched for the event. While a `jobs` row with this id
///   exists (waiting, running, or failed for an operator to retry), the poller leaves the event
///   alone; it re-dispatches only events whose job is gone or was never dispatched.
/// - `gave_up_at`: set when delivery stopped (a permanent refusal, or the last retry failed), so
///   the poller does not start it again once the failed job has been pruned.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait]
impl MigrationTrait for Migration {
    /// Schema change and migration record commit together, so a failed run can be retried.
    fn use_transaction(&self) -> Option<bool> {
        Some(true)
    }

    /// `add_column` skips what already exists (MySQL commits DDL statement by statement).
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        add_column(manager, ContentEvents::Table, ColumnDef::new(ContentEvents::JobId).uuid().null()).await?;
        add_column(manager, ContentEvents::Table, ColumnDef::new(ContentEvents::GaveUpAt).instant(manager).null()).await
    }

    /// Refuses while an undelivered event has given up: without `gave_up_at`, a later `up` would
    /// let the poller send it again unasked. `job_id` only steers redelivery, and the code
    /// before this migration has no poller.
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // A rerun after `gave_up_at` was dropped (MySQL commits each DDL) has nothing to check.
        if manager.has_column("content_events", "gave_up_at").await? {
            refuse_if_rows(
                manager,
                &["content_events"],
                "delivered_at IS NULL AND gave_up_at IS NOT NULL",
                "deliver or delete the content events whose webhook delivery gave up first",
            )
            .await?;
        }
        drop_column(manager, ContentEvents::Table, ContentEvents::GaveUpAt).await?;
        drop_column(manager, ContentEvents::Table, ContentEvents::JobId).await
    }
}

#[derive(DeriveIden)]
enum ContentEvents {
    Table,
    JobId,
    GaveUpAt,
}
