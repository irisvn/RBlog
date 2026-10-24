//! Jobs, schedules, seeders and console commands of the application
//! (`rustcms make:job`, `make:seeder`, `make:command` add them here).

use std::time::Duration;

use rustcms::async_trait;
use rustcms::prelude::*;
use rustcms::schedule::Schedule;

use crate::jobs::publish_due::PublishDue;
use crate::jobs::redispatch_content_events::RedispatchContentEvents;
use crate::services::events::WebhookConfig;
use crate::services::version::{self, Build, ReleasesConfig};

pub struct AppServiceProvider;

#[async_trait]
impl ServiceProvider for AppServiceProvider {
    fn register(&self, app: &mut AppBuilder) {
        // `rustcms db:seed` runs this one (it calls the others).
        app.seeder(crate::database::seeders::DatabaseSeeder);
        app.seeder(crate::database::seeders::content_api_seeder::ContentApiSeeder);
        app.seeder(crate::database::seeders::role_seeder::RoleSeeder);
        app.seeder(crate::database::seeders::demo_seeder::DemoSeeder);
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
        // Scheduled posts and pages that are due go live (`services::publishing::publish_due`), all of
        // them, so a run after downtime catches up.
        app.schedule(
            Schedule::every("publish-due", Duration::from_secs(60)).job(|| PublishDue {}).without_overlapping(),
        );
        app.command(crate::console::media_move_disk::MediaMoveDisk);
        // What this binary is (Integrations → Version); tests bind another build after this.
        app.bind(Build::compiled());
        // A newer RBlog release (`services::version::check`): once per slot across instances.
        app.schedule(
            Schedule::every(version::SCHEDULE, version::PERIOD)
                .task(|ctx| async move {
                    let Ok(found) = version::check(&ctx.state, version::Trigger::Schedule).await? else {
                        return Ok(Some("skipped".to_owned()));
                    };
                    Ok(Some(version::summary(&found)))
                })
                .timeout(Duration::from_secs(90)),
        );
        app.command(crate::console::content_sync_public_media::ContentSyncPublicMedia);
        app.command(crate::console::version_check::VersionCheck);
        // rustcms:commands
    }

    /// Refuses to boot on an invalid `[webhook]` section (`config/webhook.toml`), including
    /// `allow_insecure_local = true` in production, or an invalid `[releases]` section
    /// (`config/releases.toml`), including a `url` in production. Then records what this process
    /// runs as (`services::version::install`).
    async fn boot(&self, state: &AppState) -> anyhow::Result<()> {
        WebhookConfig::of(state.config())?;
        ReleasesConfig::of(state.config())?;
        version::install(state.service::<Build>().map_or_else(Build::compiled, |b| (*b).clone()));
        Ok(())
    }
}
