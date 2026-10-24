use rustcms::async_trait;
use rustcms::migration::*;
use sea_orm::{ConnectionTrait, Statement};

/// What the last release check found (Integrations → Version, `services::version`): one row,
/// id 1, shared by every instance and kept across restarts. Only a cache of GitHub's releases:
/// the next check (every 6 hours) writes it again.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait]
impl MigrationTrait for Migration {
    /// Schema change and migration record commit together, so a failed run can be retried.
    fn use_transaction(&self) -> Option<bool> {
        Some(true)
    }

    /// `create_table` and `INSERT IGNORE` skip what already exists (MySQL commits DDL statement by
    /// statement, so a failed run is retried on a partly changed schema).
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        create_table(
            manager,
            Table::create()
                .table(ReleaseChecks::Table)
                .col(ColumnDef::new(ReleaseChecks::Id).big_integer().not_null().primary_key())
                // When the last check started (manual or scheduled): the manual check's rate
                // limit, and the token that lets only the newest check write its result.
                .col(ColumnDef::new(ReleaseChecks::AttemptedAt).instant(manager).null())
                // When a check last succeeded.
                .col(ColumnDef::new(ReleaseChecks::CheckedAt).instant(manager).null())
                // The newest release then (`X.Y.Z`), its rank (major·10¹² + minor·10⁶ + patch) for
                // comparisons in SQL, its notes (plain text), page and date.
                .col(ColumnDef::new(ReleaseChecks::LatestVersion).string_len(32).null())
                .col(ColumnDef::new(ReleaseChecks::LatestRank).big_integer().null())
                .col(ColumnDef::new(ReleaseChecks::Notes).custom(Alias::new("mediumtext")).null())
                .col(ColumnDef::new(ReleaseChecks::Url).string_len(512).null())
                .col(ColumnDef::new(ReleaseChecks::PublishedAt).instant(manager).null())
                // Why the last attempt failed (a code), its HTTP status, and when.
                .col(ColumnDef::new(ReleaseChecks::Error).string_len(32).null())
                .col(ColumnDef::new(ReleaseChecks::ErrorStatus).integer().null())
                .col(ColumnDef::new(ReleaseChecks::ErrorAt).instant(manager).null())
                .to_owned(),
        )
        .await?;
        let db = manager.get_connection();
        db.execute_raw(Statement::from_string(
            db.get_database_backend(),
            "INSERT IGNORE INTO release_checks (id) VALUES (1)",
        ))
        .await?;
        Ok(())
    }

    /// Drops the table: it holds only what the next check fetches again.
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        drop_table(manager, ReleaseChecks::Table).await
    }
}

#[derive(DeriveIden)]
enum ReleaseChecks {
    Table,
    Id,
    AttemptedAt,
    CheckedAt,
    LatestVersion,
    LatestRank,
    Notes,
    Url,
    PublishedAt,
    Error,
    ErrorStatus,
    ErrorAt,
}
