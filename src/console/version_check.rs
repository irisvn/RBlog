//! `version:check` (registered in `providers/app_service_provider.rs`): checks GitHub for a newer
//! RBlog release now, as the 6-hourly schedule does (`services::version::check`), and prints what
//! it found. Integrations → Version shows the same.

use rustcms::{AppCommand, AppState, async_trait};

use crate::services::version::{self, Trigger};

pub struct VersionCheck;

#[async_trait]
impl AppCommand for VersionCheck {
    fn definition(&self) -> rustcms::clap::Command {
        rustcms::clap::Command::new("version:check").about("Check GitHub for a newer RBlog release now")
    }

    async fn run(&self, state: &AppState, _args: &rustcms::clap::ArgMatches) -> anyhow::Result<()> {
        let build = version::current();
        let Ok(found) = version::check(state, Trigger::Schedule).await? else {
            anyhow::bail!("a check that is not manual is never refused");
        };
        println!("RBlog {} ({:?} build): {}", build.version, build.channel, version::summary(&found));
        Ok(())
    }
}
