use rustcms::async_trait;
use rustcms::migration::*;
use sea_orm::{ConnectionTrait, Statement};

/// The journal of `media:move-disk`: one row per media file copied to another disk, so a move
/// interrupted half-way resumes where it stopped, and `--cleanup` knows what the old disk keeps.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait]
impl MigrationTrait for Migration {
    /// Schema change and migration record commit together, so a failed run can be retried.
    fn use_transaction(&self) -> Option<bool> {
        Some(true)
    }

    /// `create_table` and `instant` skip what already exists (MySQL commits DDL statement by
    /// statement, so a failed run is retried on a partly changed schema).
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        create_table(
            manager,
            Table::create()
                .table(MediaMoves::Table)
                .col(ColumnDef::new(MediaMoves::MediaId).uuid().not_null().primary_key())
                .col(ColumnDef::new(MediaMoves::FromDisk).string_len(16).not_null())
                // The object key, kept so the old copy can be deleted even after the media row is gone.
                .col(ColumnDef::new(MediaMoves::Key).string_len(128).not_null())
                .col(ColumnDef::new(MediaMoves::ToDisk).string_len(16).not_null())
                .col(ColumnDef::new(MediaMoves::CopiedAt).instant_now(manager).not_null())
                .to_owned(),
        )
        .await
    }

    /// Refuses while the journal still lists old copies: they could no longer be cleaned up.
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        if manager.has_table("media_moves").await? {
            let row = db
                .query_one_raw(Statement::from_string(
                    db.get_database_backend(),
                    "SELECT COUNT(*) AS n FROM media_moves",
                ))
                .await?;
            let pending: i64 = row.map(|r| r.try_get("", "n")).transpose()?.unwrap_or(0);
            if pending > 0 {
                return Err(DbErr::Migration(format!(
                    "media_moves lists {pending} old cop(y/ies): run `rblog media:move-disk --to <disk> --cleanup` first"
                )));
            }
        }
        drop_table(manager, MediaMoves::Table).await
    }
}

#[derive(DeriveIden)]
enum MediaMoves {
    Table,
    MediaId,
    FromDisk,
    Key,
    ToDisk,
    CopiedAt,
}
