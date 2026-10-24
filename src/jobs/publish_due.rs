//! The `publish_due` job: the `publish-due` schedule's work, every minute
//! (`providers/app_service_provider.rs`). It publishes every scheduled post and page that is due
//! (`crate::services::publishing::publish_due`), all of them, so a run after downtime catches up.

use std::time::Duration;

use rustcms::async_trait;
use rustcms::queue::{Job, JobCtx};
use serde::{Deserialize, Serialize};

use crate::services::publishing;

/// No payload: it reads the schedule from the posts and pages.
#[derive(Debug, Serialize, Deserialize)]
pub struct PublishDue {}

#[async_trait]
impl Job for PublishDue {
    const KIND: &'static str = "publish_due";

    fn describe(&self) -> Option<String> {
        Some("publish scheduled posts and pages that are due".into())
    }

    /// The next minute's run is the retry: a failed run is not repeated.
    fn max_attempts() -> u32 {
        1
    }

    fn timeout() -> Duration {
        Duration::from_secs(50)
    }

    /// Safe to repeat: a row is published only while it is still scheduled and due.
    async fn handle(self, ctx: JobCtx) -> anyhow::Result<()> {
        publishing::publish_due(ctx.state.db()).await?;
        Ok(())
    }
}
