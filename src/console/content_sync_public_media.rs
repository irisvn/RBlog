//! `content:sync-public-media` (registered in `providers/app_service_provider.rs`): rebuilds the
//! `public_media` table from the content (`services::public_media::backfill`). Run it once before
//! opening the Content API, and whenever the table may have drifted.

use rustcms::{AppCommand, AppState, async_trait};

use crate::services::public_media;

pub struct ContentSyncPublicMedia;

#[async_trait]
impl AppCommand for ContentSyncPublicMedia {
    fn definition(&self) -> rustcms::clap::Command {
        rustcms::clap::Command::new("content:sync-public-media")
            .about("Rebuild the list of media the public site may load (images of published content)")
    }

    async fn run(&self, state: &AppState, _args: &rustcms::clap::ArgMatches) -> anyhow::Result<()> {
        let report = public_media::backfill(state.db(), state.keyring()).await?;
        println!(
            "Synced the public media of {} post(s), page(s) and author(s); removed {} row(s) of deleted content.",
            report.subjects, report.orphans
        );
        Ok(())
    }
}
