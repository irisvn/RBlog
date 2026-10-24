//! Seeders: `rustcms db:seed` runs [`DatabaseSeeder`], which calls the others
//! (`rustcms make:seeder <Name>` adds them here). Seeders may run more than
//! once: find rows before creating them.

use rustcms::database::Seeder;
use rustcms::{AppState, async_trait};

pub mod content_api_seeder;
pub mod demo_images;
pub mod demo_seeder;
pub mod role_seeder;
// rustcms:seeder-mods

pub struct DatabaseSeeder;

#[async_trait]
impl Seeder for DatabaseSeeder {
    fn name(&self) -> &'static str {
        "database"
    }

    async fn run(&self, state: &AppState) -> anyhow::Result<()> {
        rustcms::database::call(
            state,
            &[
                &content_api_seeder::ContentApiSeeder,
                &role_seeder::RoleSeeder,
                // Not `demo_seeder::DemoSeeder`: demo content runs only on request
                // (`rustcms db:seed --class demo`).
                // rustcms:seeder-calls
            ],
        )
        .await
    }
}
