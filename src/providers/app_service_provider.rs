//! Jobs, schedules, seeders and console commands of the application
//! (`rustcms make:job`, `make:seeder`, `make:command` add them here).

use std::time::Duration;

use rustcms::async_trait;
use rustcms::prelude::*;
use rustcms::schedule::Schedule;

use crate::jobs::redispatch_content_events::RedispatchContentEvents;
use crate::services::events::WebhookConfig;

pub struct AppServiceProvider;

#[async_trait]
impl ServiceProvider for AppServiceProvider {
    fn register(&self, app: &mut AppBuilder) {
        // `rustcms db:seed` runs this one (it calls the others).
        app.seeder(crate::database::seeders::DatabaseSeeder);
        // rustcms:seeders
        app.job::<crate::jobs::deliver_content_event::DeliverContentEvent>();
        // rustcms:jobs
        // Webhook deliveries whose job was lost (`services::events::redispatch_lost`); it also
        // registers its job.
        app.schedule(
            Schedule::every("content-events-redispatch", Duration::from_secs(60))
                .job(|| RedispatchContentEvents {})
                .without_overlapping(),
        );
        app.command(crate::console::media_move_disk::MediaMoveDisk);
        // rustcms:commands
    }

    /// Refuses to boot on an invalid `[webhook]` section (`config/webhook.toml`), including
    /// `allow_insecure_local = true` in production.
    async fn boot(&self, state: &AppState) -> anyhow::Result<()> {
        WebhookConfig::of(state.config())?;
        Ok(())
    }
}
