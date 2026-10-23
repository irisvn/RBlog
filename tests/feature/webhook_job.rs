//! The webhook delivery job and its poller, against a local axum receiver on 127.0.0.1 (with
//! `[webhook] allow_insecure_local` set through the test configuration).

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use rblog::bootstrap;
use rblog::database::migrations::Migrator;
use rblog::jobs::deliver_content_event::{DeliverContentEvent, NO_WEBHOOK};
use rblog::models::content_event;
use rblog::services::content_sync;
use rblog::services::events::{self, WebhookConfig};
use rblog::services::webhook::{self, verify};
use rustcms::axum::Router;
use rustcms::axum::body::Bytes;
use rustcms::axum::http::{HeaderMap, StatusCode};
use rustcms::axum::routing::post;
use rustcms::models::job;
use rustcms_testing::{TestApp, TestDb, db_test, test_config};
use sea_orm::prelude::{DateTimeUtc, Uuid};
use sea_orm::{ActiveModelTrait, EntityTrait, PaginatorTrait, Set};
use serde_json::{Map, Value, json};

const SECRET: &str = "whsec-job-test-secret";

type Seen = Arc<Mutex<Vec<(HeaderMap, Bytes)>>>;

/// A receiver on 127.0.0.1: `/hook` 204, `/busy` 503, `/bad` 400 echoing the secret.
async fn receiver() -> (SocketAddr, Seen) {
    let seen = Seen::default();
    let answer = |status: StatusCode, seen: Seen| {
        move |headers: HeaderMap, body: Bytes| async move {
            seen.lock().unwrap().push((headers, body));
            (status, format!("refused, key {SECRET}"))
        }
    };
    let app = Router::new()
        .route("/hook", post(answer(StatusCode::NO_CONTENT, seen.clone())))
        .route("/busy", post(answer(StatusCode::SERVICE_UNAVAILABLE, seen.clone())))
        .route("/bad", post(answer(StatusCode::BAD_REQUEST, seen.clone())));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { rustcms::axum::serve(listener, app).await.unwrap() });
    (addr, seen)
}

/// The application with loopback deliveries allowed.
async fn app(db: TestDb) -> TestApp {
    let config = test_config(db.url()).with("webhook.allow_insecure_local", true).unwrap();
    TestApp::with_config(bootstrap::build, config, db).await
}

/// Saves Settings → Integrations directly (quicker than through the panel).
async fn configure(app: &TestApp, url: &str) {
    let state = app.state();
    let txn = rustcms::db::begin_write(state.db()).await.unwrap();
    let mut values = Map::new();
    values.insert("webhook_url".into(), json!(url));
    values.insert("preview_path".into(), json!("/preview"));
    let secrets = BTreeMap::from([("webhook_secret".to_owned(), SECRET.to_owned())]);
    rustcms::settings::write(&txn, state.keyring(), "integrations", 1, values, &secrets, None).await.unwrap();
    txn.commit().await.unwrap();
}

fn ago(seconds: u64) -> DateTimeUtc {
    let at = DateTimeUtc::from(SystemTime::now() - Duration::from_secs(seconds));
    DateTimeUtc::from_timestamp_micros(at.timestamp_micros()).unwrap()
}

/// Writes an event, and enqueues its job in the same transaction when `enqueue`.
async fn event(app: &TestApp, created_at: DateTimeUtc, enqueue: bool) -> Uuid {
    let id = Uuid::now_v7();
    let txn = rustcms::db::begin_write(app.state().db()).await.unwrap();
    content_event::ActiveModel {
        id: Set(id),
        kind: Set("post.published".into()),
        subject_type: Set("post".into()),
        subject_id: Set(Some(42)),
        payload: Set(json!({
            "tags": ["post:42", "posts", "sitemap"],
            "paths": { "old": ["/bai-cu"], "new": ["/xin-chao"] },
        })),
        created_at: Set(created_at),
        delivered_at: Set(None),
        attempts: Set(0),
        last_error: Set(None),
        job_id: Set(None),
        gave_up_at: Set(None),
    }
    .insert(&txn)
    .await
    .unwrap();
    if enqueue {
        events::enqueue(&txn, id).await.unwrap();
    }
    txn.commit().await.unwrap();
    id
}

async fn load(app: &TestApp, id: Uuid) -> content_event::Model {
    content_event::Entity::find_by_id(id).one(app.state().db()).await.unwrap().unwrap()
}

async fn edit(app: &TestApp, id: Uuid, change: fn(&mut content_event::ActiveModel)) {
    let mut model: content_event::ActiveModel = load(app, id).await.into();
    change(&mut model);
    model.update(app.state().db()).await.unwrap();
}

async fn run_jobs(app: &TestApp) -> usize {
    rustcms::queue::run_until_idle(app.state(), None, 10).await.unwrap()
}

async fn jobs(app: &TestApp) -> Vec<job::Model> {
    job::Entity::find().all(app.state().db()).await.unwrap()
}

db_test!(delivers_the_signed_body_once, migrate = Migrator, |db: TestDb| async move {
    let (addr, seen) = receiver().await;
    let app = app(db).await;
    configure(&app, &format!("http://127.0.0.1:{}/hook", addr.port())).await;
    let id = event(&app, ago(0), true).await;
    let created_at = load(&app, id).await.created_at;

    assert_eq!(run_jobs(&app).await, 1);
    let received = seen.lock().unwrap().clone();
    assert_eq!(received.len(), 1);
    let (headers, body) = &received[0];
    assert_eq!(headers.get(webhook::HEADER_EVENT_ID).unwrap().to_str().unwrap(), id.to_string());
    let timestamp: i64 = headers.get(webhook::HEADER_TIMESTAMP).unwrap().to_str().unwrap().parse().unwrap();
    let signature = headers.get(webhook::HEADER_SIGNATURE).unwrap().to_str().unwrap();
    assert!(verify(SECRET.as_bytes(), timestamp, &id.to_string(), body, signature));
    let body: Value = serde_json::from_slice(body).unwrap();
    assert_eq!(
        body,
        json!({
            "id": id.to_string(),
            "kind": "post.published",
            "subject": { "type": "post", "id": "42" },
            "tags": ["post:42", "posts", "sitemap"],
            "paths": { "old": ["/bai-cu"], "new": ["/xin-chao"] },
            "created_at": created_at.format("%Y-%m-%dT%H:%M:%S%.6fZ").to_string(),
        })
    );

    let delivered = load(&app, id).await;
    assert!(delivered.delivered_at.is_some());
    assert_eq!(delivered.attempts, 1);
    assert_eq!(delivered.last_error, None);
    assert!(jobs(&app).await.is_empty(), "the job is done");

    // A second run (a repeated job) finds the event delivered and does not POST again.
    rustcms::queue::enqueue(app.state().db(), &DeliverContentEvent { event_id: id }).await.unwrap();
    assert_eq!(run_jobs(&app).await, 1);
    assert_eq!(seen.lock().unwrap().len(), 1);
    assert_eq!(load(&app, id).await.attempts, 1);
});

db_test!(recording_an_event_dispatches_its_delivery, migrate = Migrator, |db: TestDb| async move {
    let app = app(db).await;
    let db = app.state().db();
    let payload = json!({ "tags": ["settings"], "paths": { "old": [], "new": [] } });

    let txn = rustcms::db::begin_write(db).await.unwrap();
    content_sync::record_event(&txn, "settings.updated", "settings", None, payload.clone()).await.unwrap();
    txn.commit().await.unwrap();
    let recorded = content_event::Entity::find().all(db).await.unwrap();
    assert_eq!(recorded.len(), 1);
    let jobs = jobs(&app).await;
    assert_eq!(jobs.len(), 1);
    assert_eq!(recorded[0].job_id, Some(jobs[0].id));
    assert_eq!(jobs[0].payload, json!({ "event_id": recorded[0].id.to_string() }));

    // A change that rolls back leaves neither its event nor its job.
    let txn = rustcms::db::begin_write(db).await.unwrap();
    content_sync::record_event(&txn, "settings.updated", "settings", None, payload).await.unwrap();
    txn.rollback().await.unwrap();
    assert_eq!(content_event::Entity::find().count(db).await.unwrap(), 1);
    assert_eq!(job::Entity::find().count(db).await.unwrap(), 1);
});

db_test!(a_503_is_recorded_and_retried, migrate = Migrator, |db: TestDb| async move {
    let (addr, seen) = receiver().await;
    let app = app(db).await;
    configure(&app, &format!("http://127.0.0.1:{}/busy", addr.port())).await;
    let id = event(&app, ago(0), true).await;

    assert_eq!(run_jobs(&app).await, 1, "the retry waits for its backoff");
    assert_eq!(seen.lock().unwrap().len(), 1);
    let failed = load(&app, id).await;
    assert_eq!(failed.delivered_at, None);
    assert_eq!(failed.gave_up_at, None);
    assert_eq!(failed.attempts, 1, "the bookkeeping commits although the job failed");
    let error = failed.last_error.unwrap();
    assert!(error.contains("503"), "{error}");
    assert!(!error.contains(SECRET), "redacted: {error}");

    let jobs = jobs(&app).await;
    assert_eq!(jobs.len(), 1);
    let retry = &jobs[0];
    assert_eq!(Some(retry.id), failed.job_id);
    assert_eq!((retry.attempts, retry.max_attempts), (1, 8));
    assert_eq!(retry.failed_at, None, "the job asks for a retry");
    assert!(retry.available_at > ago(0) + Duration::from_secs(20), "after its backoff (30 s)");

    // A later run of the same job on an event that has given up meanwhile (a run whose worker
    // vanished after recording it) stops without sending.
    edit(&app, id, |m| m.gave_up_at = Set(Some(ago(1)))).await;
    let mut due: job::ActiveModel = retry.clone().into();
    due.available_at = Set(ago(1));
    due.update(app.state().db()).await.unwrap();
    assert_eq!(run_jobs(&app).await, 1);
    assert_eq!(seen.lock().unwrap().len(), 1, "no second POST");
    assert_eq!(load(&app, id).await.attempts, 1);
    let ended = job::Entity::find_by_id(retry.id).one(app.state().db()).await.unwrap().unwrap();
    assert!(ended.failed_at.is_some(), "failed without retrying");
});

db_test!(a_redispatched_job_keeps_the_event_budget, migrate = Migrator, |db: TestDb| async move {
    let (addr, seen) = receiver().await;
    let app = app(db).await;
    configure(&app, &format!("http://127.0.0.1:{}/busy", addr.port())).await;
    // Seven attempts made, then the job was lost.
    let id = event(&app, ago(300), false).await;
    edit(&app, id, |m| m.attempts = Set(7)).await;

    assert_eq!(events::redispatch_lost(app.state().db()).await.unwrap(), 1);
    assert_eq!(run_jobs(&app).await, 1);
    assert_eq!(seen.lock().unwrap().len(), 1);
    let failed = load(&app, id).await;
    assert_eq!(failed.attempts, 8);
    assert!(failed.gave_up_at.is_some(), "the eighth attempt was the last");
    let jobs = jobs(&app).await;
    assert_eq!(jobs.len(), 1);
    assert!(jobs[0].failed_at.is_some(), "no retry beyond the event's 8 attempts");
});

db_test!(a_400_gives_up_without_retrying, migrate = Migrator, |db: TestDb| async move {
    let (addr, seen) = receiver().await;
    let app = app(db).await;
    configure(&app, &format!("http://127.0.0.1:{}/bad", addr.port())).await;
    let id = event(&app, ago(0), true).await;

    assert_eq!(run_jobs(&app).await, 1);
    assert_eq!(seen.lock().unwrap().len(), 1);
    let failed = load(&app, id).await;
    assert_eq!(failed.delivered_at, None);
    assert!(failed.gave_up_at.is_some());
    assert_eq!(failed.attempts, 1);
    let error = failed.last_error.unwrap();
    assert!(error.contains("400") && !error.contains(SECRET), "{error}");

    let jobs = jobs(&app).await;
    assert_eq!(jobs.len(), 1);
    assert!(jobs[0].failed_at.is_some(), "failed at once");
    assert_eq!(jobs[0].attempts, 1);
    assert_eq!(run_jobs(&app).await, 0);
    assert_eq!(seen.lock().unwrap().len(), 1);
    // Nor does the poller start it again.
    assert_eq!(events::redispatch_lost(app.state().db()).await.unwrap(), 0);

    // An operator retry on the Queue page starts a new series of attempts.
    let clock = rustcms::queue::Clock::new(5);
    rustcms::queue::manage::retry(app.state().db(), jobs[0].id, &clock).await.unwrap().unwrap();
    edit(&app, id, |m| m.attempts = Set(7)).await;
    assert_eq!(run_jobs(&app).await, 1);
    assert_eq!(seen.lock().unwrap().len(), 2, "sent again");
    let again = load(&app, id).await;
    assert_eq!(again.attempts, 1, "a new series");
    assert!(again.gave_up_at.is_some(), "refused again");
});

db_test!(without_a_webhook_the_event_is_marked_delivered, migrate = Migrator, |db: TestDb| async move {
    let app = app(db).await;
    let id = event(&app, ago(0), true).await;

    assert_eq!(run_jobs(&app).await, 1);
    let event = load(&app, id).await;
    assert!(event.delivered_at.is_some());
    assert_eq!(event.last_error.as_deref(), Some(NO_WEBHOOK));
    assert_eq!(event.attempts, 0, "nothing was sent");
    assert!(jobs(&app).await.is_empty());
});

db_test!(the_poller_redispatches_only_lost_events, migrate = Migrator, |db: TestDb| async move {
    let app = app(db).await;
    let state = app.state();
    // Lost: undelivered, old, no job (recorded before delivery existed).
    let lost = event(&app, ago(300), false).await;
    // Kept: too recent, delivered, given up, out of attempts, or with a job still queued.
    let recent = event(&app, ago(10), false).await;
    let delivered = event(&app, ago(300), false).await;
    let gave_up = event(&app, ago(300), false).await;
    let exhausted = event(&app, ago(300), false).await;
    let queued = event(&app, ago(300), true).await;
    edit(&app, delivered, |m| m.delivered_at = Set(Some(ago(200)))).await;
    edit(&app, gave_up, |m| m.gave_up_at = Set(Some(ago(200)))).await;
    edit(&app, exhausted, |m| m.attempts = Set(8)).await;
    let queued_job = load(&app, queued).await.job_id.unwrap();

    assert_eq!(events::redispatch_lost(state.db()).await.unwrap(), 1);
    let redispatched = load(&app, lost).await.job_id.expect("a job for the lost event");
    let job = job::Entity::find_by_id(redispatched).one(state.db()).await.unwrap().unwrap();
    assert_eq!(job.kind, "deliver_content_event");
    assert_eq!(job.payload, json!({ "event_id": lost.to_string() }));
    for id in [recent, delivered, gave_up, exhausted] {
        assert_eq!(load(&app, id).await.job_id, None, "{id} is left alone");
    }
    assert_eq!(load(&app, queued).await.job_id, Some(queued_job));
    assert_eq!(job::Entity::find().count(state.db()).await.unwrap(), 2);

    // Its job is now queued: the next run dispatches nothing.
    assert_eq!(events::redispatch_lost(state.db()).await.unwrap(), 0);
    assert_eq!(job::Entity::find().count(state.db()).await.unwrap(), 2);
});

#[test]
fn insecure_local_is_refused_in_production() {
    let config = |env: &str, allow: Value| {
        test_config("mysql://unused").with("app.env", env).unwrap().with("webhook.allow_insecure_local", allow).unwrap()
    };
    assert!(WebhookConfig::of(&config("testing", json!(true))).unwrap().allow_insecure_local);
    assert!(!WebhookConfig::of(&config("production", json!(false))).unwrap().allow_insecure_local);
    let refused = WebhookConfig::of(&config("production", json!(true))).unwrap_err();
    assert!(refused.to_string().contains("allow_insecure_local"), "{refused}");
    assert!(WebhookConfig::of(&config("local", json!("yes"))).is_err(), "a value of the wrong kind");
    // The shipped file keeps it off.
    assert!(!WebhookConfig::of(&test_config("mysql://unused")).unwrap().allow_insecure_local);
}
