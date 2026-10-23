//! `rustcms db:seed --class content_api` (also called by `DatabaseSeeder`): the API-guard role
//! `content-reader` and the system user `frontend@rblog.local` that Content API keys belong to
//! (`services::content_api`).

use rustcms::database::Seeder;
use rustcms::{AppState, async_trait};

pub struct ContentApiSeeder;

#[async_trait]
impl Seeder for ContentApiSeeder {
    fn name(&self) -> &'static str {
        "content_api"
    }

    /// Runs again safely: the role and the user are created only when missing.
    async fn run(&self, state: &AppState) -> anyhow::Result<()> {
        crate::services::content_api::ensure_frontend_user(state).await?;
        Ok(())
    }
}
