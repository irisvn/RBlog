//! The `deliver_content_event` job (registered in `providers/app_service_provider.rs`): one
//! webhook POST of a `content_events` row (`docs/api.md`, "Webhook"). Dispatch it with
//! `crate::services::events::enqueue` in the transaction that wrote the event.
//!
//! Each run:
//! - does nothing when the event is gone or already delivered (a repeated run, a second job);
//! - with no webhook URL in Settings → Integrations, marks the event delivered with
//!   `last_error = "no webhook configured"`: there is no receiver to tell, and a backlog sent
//!   to a receiver configured months later would only revalidate stale tags;
//! - otherwise POSTs [`events::body`] and records the outcome on the event, in its own committed
//!   write, before returning: a failed run still leaves `attempts` and `last_error` behind.
//!   A retryable failure returns a `Failure` (the queue retries after [`Job::backoff`]); a
//!   permanent one returns `Failure::permanent()` (no retry) and sets `gave_up_at`, as does the
//!   failure of the event's 8th attempt, counted across every job of the event;
//! - on an event that gave up, starts a new series of 8 attempts when it is the job's first run
//!   (an operator retried the failed job on the Queue page), and stops at once otherwise (a
//!   rerun of the run that gave up, after its worker vanished).
//!
//! Messages are those of `services::webhook`: they name the host and never hold the secret, the
//! signature or the URL's path and query. Nothing here logs.

use std::time::Duration;

use rustcms::queue::{Failure, Job, JobCtx};
use rustcms::{AppState, async_trait};
use sea_orm::prelude::{Expr, Uuid};
use sea_orm::sea_query::ExprTrait;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use serde::{Deserialize, Serialize};

use crate::models::content_event::{self, Column};
use crate::services::events::{self, MAX_ATTEMPTS, WebhookConfig};
use crate::services::webhook::{self, DeliveryError, WebhookTarget};
use crate::settings::integration_settings::IntegrationSettings;

/// `last_error` of an event marked delivered because no webhook URL is set.
pub const NO_WEBHOOK: &str = "no webhook configured";
/// `last_error` is a `varchar(500)`.
const ERROR_CHARS: usize = 500;

/// Ids only, never secrets: the event is read again when the job runs.
#[derive(Debug, Serialize, Deserialize)]
pub struct DeliverContentEvent {
    pub event_id: Uuid,
}

#[async_trait]
impl Job for DeliverContentEvent {
    const KIND: &'static str = "deliver_content_event";

    fn describe(&self) -> Option<String> {
        Some(format!("content event {}", self.event_id))
    }

    /// 8 runs, 30 s apart at first and doubling (30 s, 1, 2, 4, 8, 16, 32 min): about an hour.
    fn max_attempts() -> u32 {
        MAX_ATTEMPTS
    }

    fn backoff(attempt: u32) -> Duration {
        let seconds = 30u64.saturating_mul(1 << std::cmp::min(attempt.saturating_sub(1), 16));
        Duration::from_secs(std::cmp::min(seconds, 3_600))
    }

    /// A delivery takes at most 10 s; the rest is database work.
    fn timeout() -> Duration {
        Duration::from_secs(60)
    }

    /// Runs at least once: a run after a success finds the event delivered and does nothing.
    async fn handle(self, ctx: JobCtx) -> anyhow::Result<()> {
        let db = ctx.state.db();
        let Some(mut event) = content_event::Entity::find_by_id(self.event_id).one(db).await? else {
            return Ok(());
        };
        if event.delivered_at.is_some() {
            return Ok(());
        }
        if event.gave_up_at.is_some() {
            if ctx.attempt > 1 {
                // A rerun of the run that gave up, whose worker vanished before the queue failed
                // the job: the delivery has ended.
                let ended = Failure::new("webhook_gave_up", "the delivery of this event has ended");
                return Err(ended.permanent().into());
            }
            // A first run on an event that gave up: an operator retried its failed job on the
            // Queue page (attempts start over there). A new series of attempts starts.
            reopen(&ctx.state, event.id).await?;
            event.attempts = 0;
            event.gave_up_at = None;
        }
        let config = WebhookConfig::of(ctx.state.config())?;
        let outcome = match prepare(&ctx.state, &config).await {
            Prepared::NoWebhook => {
                record(&ctx.state, event.id, Outcome::NoWebhook).await?;
                return Ok(());
            }
            Prepared::Failed(error) => Err(error),
            Prepared::Ready(target, secret) => {
                let body = serde_json::to_vec(&events::body(&event))?;
                webhook::deliver(&target, secret.as_bytes(), &event.id.to_string(), &body).await.map(|_| ())
            }
        };
        match outcome {
            Ok(()) => {
                record(&ctx.state, event.id, Outcome::Delivered).await?;
                Ok(())
            }
            Err(error) => {
                let last_run = ctx.attempt >= Self::max_attempts();
                // The budget is the event's: a job the poller dispatched again after a lost one
                // gets only the attempts left.
                let spent = event.attempts.saturating_add(1) >= MAX_ATTEMPTS as i32;
                let give_up = !error.is_retryable() || last_run || spent;
                if !record(&ctx.state, event.id, Outcome::Failed { error: error.message(), give_up }).await? {
                    // Delivered meanwhile by another run: nothing left to do.
                    return Ok(());
                }
                let message = error.message().to_owned();
                Err(if !error.is_retryable() {
                    Failure::new("webhook_refused", message).permanent().into()
                } else if spent && !last_run {
                    Failure::new("webhook_attempts_spent", message).permanent().into()
                } else {
                    Failure::new("webhook_retryable", message).into()
                })
            }
        }
    }
}

enum Prepared {
    NoWebhook,
    Failed(DeliveryError),
    Ready(WebhookTarget, String),
}

/// The target and secret from Settings → Integrations, read at every run.
async fn prepare(state: &AppState, config: &WebhookConfig) -> Prepared {
    // The settings error may name the group or a field, never a secret's value; it is not kept.
    let Ok(settings) = rustcms::settings::get::<IntegrationSettings>(state).await else {
        return Prepared::Failed(DeliveryError::Retryable("cannot read the integration settings".into()));
    };
    let Some(url) = settings.webhook_url() else {
        return Prepared::NoWebhook;
    };
    let Some(secret) = settings.webhook_secret.expose().filter(|s| !s.is_empty()) else {
        return Prepared::Failed(DeliveryError::Permanent("no webhook secret configured".into()));
    };
    match WebhookTarget::parse(url, config.allow_insecure_local) {
        Ok(target) => Prepared::Ready(target, secret.to_owned()),
        Err(invalid) => Prepared::Failed(invalid.into()),
    }
}

enum Outcome<'a> {
    Delivered,
    NoWebhook,
    Failed { error: &'a str, give_up: bool },
}

/// Starts a new series of attempts on an event that gave up (`attempts` counts the series).
async fn reopen(state: &AppState, event_id: Uuid) -> anyhow::Result<()> {
    let txn = rustcms::db::begin_write(state.db()).await?;
    content_event::Entity::update_many()
        .col_expr(Column::Attempts, Expr::value(0))
        .col_expr(Column::GaveUpAt, Expr::value(Option::<sea_orm::prelude::DateTimeUtc>::None))
        .filter(Column::Id.eq(event_id))
        .filter(Column::DeliveredAt.is_null())
        .filter(Column::GaveUpAt.is_not_null())
        .exec(&txn)
        .await?;
    txn.commit().await?;
    Ok(())
}

/// Writes the outcome on the event in its own transaction, committed whatever the job returns
/// next. Only an undelivered event changes; returns whether it did.
async fn record(state: &AppState, event_id: Uuid, outcome: Outcome<'_>) -> anyhow::Result<bool> {
    let now = events::now();
    let attempts = Expr::col(Column::Attempts).add(1);
    let update = content_event::Entity::update_many();
    let update = match outcome {
        Outcome::Delivered => update
            .col_expr(Column::DeliveredAt, Expr::value(now))
            .col_expr(Column::Attempts, attempts)
            .col_expr(Column::LastError, Expr::value(Option::<String>::None))
            .col_expr(Column::GaveUpAt, Expr::value(Option::<sea_orm::prelude::DateTimeUtc>::None)),
        // Not an attempt: nothing was sent.
        Outcome::NoWebhook => {
            update.col_expr(Column::DeliveredAt, Expr::value(now)).col_expr(Column::LastError, Expr::value(NO_WEBHOOK))
        }
        Outcome::Failed { error, give_up } => {
            let error: String = error.chars().take(ERROR_CHARS).collect();
            let update = update.col_expr(Column::Attempts, attempts).col_expr(Column::LastError, Expr::value(error));
            if give_up { update.col_expr(Column::GaveUpAt, Expr::value(now)) } else { update }
        }
    };
    let txn = rustcms::db::begin_write(state.db()).await?;
    let updated =
        update.filter(Column::Id.eq(event_id)).filter(Column::DeliveredAt.is_null()).exec(&txn).await?.rows_affected;
    txn.commit().await?;
    Ok(updated == 1)
}
