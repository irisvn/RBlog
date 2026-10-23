//! The `redispatch_content_events` job: the `content-events-redispatch` schedule's work, every
//! minute (`providers/app_service_provider.rs`). It dispatches a new delivery job for each
//! undelivered event whose job was lost (`crate::services::events::redispatch_lost`).

use std::time::Duration;

use rustcms::async_trait;
use rustcms::queue::{Job, JobCtx};
use serde::{Deserialize, Serialize};

use crate::services::events;

/// No payload: it reads the outbox.
#[derive(Debug, Serialize, Deserialize)]
pub struct RedispatchContentEvents {}

#[async_trait]
impl Job for RedispatchContentEvents {
    const KIND: &'static str = "redispatch_content_events";

    fn describe(&self) -> Option<String> {
        Some("redispatch lost webhook deliveries".into())
    }

    /// The next minute's run is the retry: a failed run is not repeated.
    fn max_attempts() -> u32 {
        1
    }

    fn timeout() -> Duration {
        Duration::from_secs(50)
    }

    /// Safe to repeat: an event that has a job again is skipped.
    async fn handle(self, ctx: JobCtx) -> anyhow::Result<()> {
        events::redispatch_lost(ctx.state.db()).await?;
        Ok(())
    }
}
