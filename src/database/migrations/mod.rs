//! Migrations, one per change (`rustcms make:migration <name>` adds them here).

use rustcms::migration::{DbErr, MigrationTrait, MigratorTrait, SchemaManager};
use sea_orm::{ConnectionTrait, Statement};

mod m20261023_180109_create_media_moves_table;
mod m20261023_183009_create_taxonomy_tables;
mod m20261023_183010_create_posts_and_pages_tables;
mod m20261023_183011_create_revision_tables;
mod m20261023_183013_create_menu_tables;
mod m20261023_183014_create_author_profiles_table;
mod m20261023_183015_create_content_sync_tables;
mod m20261023_190924_add_delivery_columns_to_content_events;
mod m20261024_020523_add_subject_index_to_content_events;
mod m20261024_150000_create_release_checks_table;
// rustcms:migration-mods

pub struct Migrator;

impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        let mut migrations = rustcms::migration::framework();
        let app: Vec<Box<dyn MigrationTrait>> = vec![
            Box::new(m20261023_180109_create_media_moves_table::Migration),
            Box::new(m20261023_183009_create_taxonomy_tables::Migration),
            Box::new(m20261023_183010_create_posts_and_pages_tables::Migration),
            Box::new(m20261023_183011_create_revision_tables::Migration),
            Box::new(m20261023_183013_create_menu_tables::Migration),
            Box::new(m20261023_183014_create_author_profiles_table::Migration),
            Box::new(m20261023_183015_create_content_sync_tables::Migration),
            Box::new(m20261023_190924_add_delivery_columns_to_content_events::Migration),
            Box::new(m20261024_020523_add_subject_index_to_content_events::Migration),
            Box::new(m20261024_150000_create_release_checks_table::Migration),
            // rustcms:migrations
        ];
        migrations.extend(app);
        migrations
    }
}

/// Refuses, before anything is dropped, while one of `tables` holds a row matching `filter`
/// (`"1=1"` for any row): a `down` must not lose data. Tables already gone are skipped.
async fn refuse_if_rows(manager: &SchemaManager<'_>, tables: &[&str], filter: &str, hint: &str) -> Result<(), DbErr> {
    let db = manager.get_connection();
    for table in tables {
        if !manager.has_table(*table).await? {
            continue;
        }
        let row = db
            .query_one_raw(Statement::from_string(
                db.get_database_backend(),
                format!("SELECT COUNT(*) AS n FROM {table} WHERE {filter}"),
            ))
            .await?;
        let rows: i64 = row.map(|r| r.try_get("", "n")).transpose()?.unwrap_or(0);
        if rows > 0 {
            return Err(DbErr::Migration(format!("{table} holds {rows} row(s): {hint}")));
        }
    }
    Ok(())
}
