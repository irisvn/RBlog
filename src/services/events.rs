//! Delivering the `content_events` outbox as webhooks (`docs/api.md`, "Webhook").
//!
//! - [`enqueue`] dispatches the delivery job of an event in the transaction that wrote it.
//! - [`crate::jobs::deliver_content_event`] POSTs [`body`] once per run and records the outcome
//!   on the event.
//! - [`redispatch_lost`] (the `content-events-redispatch` schedule, every minute) dispatches a new
//!   job for undelivered events whose job is gone: never dispatched (events older than this
//!   feature), or deleted.
//! - [`WebhookConfig`]: `[webhook]` in `config/webhook.toml`.

use std::time::Duration;

use rustcms::models::job;
use rustcms::{AppError, AppResult, Config, Environment};
use sea_orm::prelude::{DateTimeUtc, Expr, Uuid};
use sea_orm::sea_query::{ExprTrait, Query};
use sea_orm::{
    ColumnTrait, DatabaseConnection, DatabaseTransaction, EntityTrait, QueryFilter, QueryOrder, QuerySelect,
    TransactionTrait,
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::jobs::deliver_content_event::DeliverContentEvent;
use crate::models::content_event::{self, Column};

/// Delivery attempts (POSTs) of an event: the job's runs, and the poller's limit.
pub const MAX_ATTEMPTS: u32 = 8;
/// An undelivered event this old, with no job, is redispatched by the poller. Its own job is
/// dispatched with it, so only a lost job leaves it waiting that long.
pub const LOST_AFTER: Duration = Duration::from_secs(120);
/// Events redispatched per transaction.
pub const BATCH: u64 = 100;
/// Batches per poller run: at most 2,000 events a minute.
const MAX_BATCHES: usize = 20;

/// `[webhook]` (`config/webhook.toml`).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WebhookConfig {
    /// Development only: also deliver over `http://` and to loopback addresses. Refused in
    /// production.
    pub allow_insecure_local: bool,
}

impl WebhookConfig {
    /// Reads `[webhook]`. A value of the wrong kind is an error, and so is
    /// `allow_insecure_local = true` with `APP_ENV=production`: the app refuses to boot rather
    /// than open loopback to the webhook.
    pub fn of(config: &Config) -> anyhow::Result<Self> {
        let webhook: Self = config.section("webhook")?;
        if webhook.allow_insecure_local && config.app.env == Environment::Production {
            anyhow::bail!("[webhook] allow_insecure_local must not be true when APP_ENV=production");
        }
        Ok(webhook)
    }
}

/// Dispatches the delivery job of `event_id` in `txn`, and records it on the event. Call it in
/// the transaction that wrote the event, after the insert: the job exists only if the event
/// commits, and the poller then knows the event has a job.
pub async fn enqueue(txn: &DatabaseTransaction, event_id: Uuid) -> AppResult<Uuid> {
    let job_id = rustcms::queue::dispatch(txn, &DeliverContentEvent { event_id }).await?;
    let updated = content_event::Entity::update_many()
        .col_expr(Column::JobId, Expr::value(job_id))
        .filter(Column::Id.eq(event_id))
        .exec(txn)
        .await?;
    if updated.rows_affected != 1 {
        return Err(AppError::internal("events::enqueue: write the content event before enqueuing it"));
    }
    Ok(job_id)
}

/// The JSON the receiver gets for `event` (`docs/api.md`, "Webhook"). Its shape is a contract:
/// add fields, never rename or remove them.
pub fn body(event: &content_event::Model) -> Value {
    let list = |value: Option<&Value>| value.filter(|v| v.is_array()).cloned().unwrap_or_else(|| json!([]));
    let paths = event.payload.get("paths");
    json!({
        "id": event.id.to_string(),
        "kind": event.kind,
        // i64 travels as a string (ADR 0003).
        "subject": { "type": event.subject_type, "id": event.subject_id.map(|id| id.to_string()) },
        "tags": list(event.payload.get("tags")),
        "paths": {
            "old": list(paths.and_then(|p| p.get("old"))),
            "new": list(paths.and_then(|p| p.get("new"))),
        },
        "created_at": event.created_at.format("%Y-%m-%dT%H:%M:%S%.6fZ").to_string(),
    })
}

/// Dispatches a new delivery job for each lost event: undelivered, not given up, fewer than
/// [`MAX_ATTEMPTS`] attempts, older than [`LOST_AFTER`], and whose `job_id` names no `jobs` row.
/// A job still waiting, running or failed (an operator may retry it on the Queue page) keeps its
/// event. Returns how many were dispatched.
pub async fn redispatch_lost(db: &DatabaseConnection) -> AppResult<usize> {
    let mut dispatched = 0;
    for _ in 0..MAX_BATCHES {
        let cutoff = DateTimeUtc::from(std::time::SystemTime::now() - LOST_AFTER);
        let has_job = Query::select()
            .expr(Expr::val(1))
            .from(job::Entity)
            .and_where(Expr::col((job::Entity, job::Column::Id)).equals((content_event::Entity, Column::JobId)))
            .to_owned();
        let lost: Vec<(Uuid, Option<Uuid>)> = content_event::Entity::find()
            .select_only()
            .column(Column::Id)
            .column(Column::JobId)
            .filter(Column::DeliveredAt.is_null())
            .filter(Column::GaveUpAt.is_null())
            .filter(Column::Attempts.lt(MAX_ATTEMPTS as i32))
            .filter(Column::CreatedAt.lt(cutoff))
            .filter(Expr::exists(has_job).not())
            .order_by_asc(Column::CreatedAt)
            .limit(BATCH)
            .into_tuple()
            .all(db)
            .await?;
        let found = lost.len();

        let txn = rustcms::db::begin_write(db).await?;
        for (event_id, old_job) in lost {
            // A savepoint per event: if the event changed since it was read (delivered, given
            // up, or redispatched), its new job is rolled back with the update.
            let event = txn.begin().await?;
            let job_id = rustcms::queue::dispatch(&event, &DeliverContentEvent { event_id }).await?;
            let old_job = match old_job {
                Some(id) => Column::JobId.eq(id),
                None => Column::JobId.is_null(),
            };
            let updated = content_event::Entity::update_many()
                .col_expr(Column::JobId, Expr::value(job_id))
                .filter(Column::Id.eq(event_id))
                .filter(Column::DeliveredAt.is_null())
                .filter(Column::GaveUpAt.is_null())
                .filter(old_job)
                .exec(&event)
                .await?;
            if updated.rows_affected == 1 {
                event.commit().await?;
                dispatched += 1;
            } else {
                event.rollback().await?;
            }
        }
        txn.commit().await?;

        if found < BATCH as usize {
            break;
        }
    }
    Ok(dispatched)
}

/// The current time, to the microsecond (as `DATETIME(6)` stores it).
pub(crate) fn now() -> DateTimeUtc {
    let now = DateTimeUtc::from(std::time::SystemTime::now());
    DateTimeUtc::from_timestamp_micros(now.timestamp_micros()).unwrap_or(now)
}
