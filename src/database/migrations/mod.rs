//! Migrations, one per change (`rustcms make:migration <name>` adds them here).

use rustcms::migration::{MigrationTrait, MigratorTrait};

mod m20261023_180109_create_media_moves_table;
// rustcms:migration-mods

pub struct Migrator;

impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        let mut migrations = rustcms::migration::framework();
        let app: Vec<Box<dyn MigrationTrait>> = vec![
            Box::new(m20261023_180109_create_media_moves_table::Migration),
            // rustcms:migrations
        ];
        migrations.extend(app);
        migrations
    }
}
