//! Integrations → Version (`services::version`, `{admin}/api/x/system/version`): versions compared
//! strictly, GitHub's releases asked on a local fake server (`[releases] url`), the result kept in
//! `release_checks`, the 6-hour schedule, the manual check's rate limit, who may see the page and
//! the dashboard's notice. The running build is bound per test (`Build`): the test binary itself
//! is a `source` or `development` build.

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use rblog::bootstrap;
use rblog::database::migrations::Migrator;
use rblog::models::release_check;
use rblog::services::version::{
    self, Build, Channel, Failure, GitHubRelease, NOTES_MAX, ReleasesConfig, Trigger, newest, parse, rank,
};
use rustcms::axum::Router;
use rustcms::axum::http::{HeaderMap, StatusCode, header};
use rustcms::axum::response::{IntoResponse, Response};
use rustcms::axum::routing::get;
use rustcms_testing::{Factory, TestApp, TestClient, TestDb, UserFactory, db_test, test_config};
use sea_orm::{ActiveModelTrait, ConnectionTrait, Set, Statement};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::support::holding;

/// Notes with markup and line breaks: they must come back as the same text.
const NOTES: &str = "## What's new\n\n- <script>alert(1)</script> is text\n- **bold** stays Markdown\r\nlast line";

fn build(channel: Channel) -> Build {
    Build { version: "0.1.0".into(), commit: Some("0123456789abcdef0123456789abcdef01234567".into()), channel }
}

fn release(tag: &str) -> Value {
    json!({
        "tag_name": tag,
        "draft": false,
        "prerelease": false,
        "body": format!("Notes of {tag}"),
        "html_url": format!("https://github.com/irisvn/RBlog/releases/tag/{tag}"),
        "published_at": "2026-10-20T08:00:00Z",
    })
}

/// GitHub's list with a newer release (0.2.0), and newer drafts and prereleases to skip.
fn newer_list() -> Value {
    let mut draft = release("v9.0.0");
    draft["draft"] = json!(true);
    let mut pre = release("v8.0.0");
    pre["prerelease"] = json!(true);
    let mut current = release("v0.2.0");
    current["body"] = json!(NOTES);
    json!([draft, pre, release("v1.0.0-rc.1"), current, release("v0.1.0"), release("v0.0.9")])
}

type Seen = Arc<Mutex<Vec<HeaderMap>>>;

/// What `/switch` answers: set by the test.
#[derive(Clone, Default)]
struct Fake {
    seen: Seen,
    mode: Arc<Mutex<&'static str>>,
    calls: Arc<AtomicUsize>,
}

async fn answer(mode: &str) -> Response {
    match mode {
        "newer" => rustcms::axum::Json(newer_list()).into_response(),
        "same" => rustcms::axum::Json(json!([release("v0.1.0")])).into_response(),
        "older" => rustcms::axum::Json(json!([release("v0.0.9")])).into_response(),
        "even-newer" => rustcms::axum::Json(json!([release("v0.3.0"), release("v0.2.0")])).into_response(),
        "empty" => rustcms::axum::Json(json!([])).into_response(),
        "previews" => {
            let mut pre = release("v0.3.0");
            pre["prerelease"] = json!(true);
            rustcms::axum::Json(json!([pre, {"tag_name": "v0.4.0", "draft": true}])).into_response()
        }
        "malformed" => (StatusCode::OK, "[{not json").into_response(),
        "object" => rustcms::axum::Json(json!({"message": "Not Found"})).into_response(),
        "error" => (StatusCode::INTERNAL_SERVER_ERROR, "oops").into_response(),
        "limited" => (StatusCode::FORBIDDEN, "API rate limit exceeded").into_response(),
        "redirect" => (StatusCode::FOUND, [(header::LOCATION, "/newer")]).into_response(),
        "slow" => {
            tokio::time::sleep(Duration::from_secs(5)).await;
            rustcms::axum::Json(newer_list()).into_response()
        }
        "huge" => (StatusCode::OK, vec![b' '; version::BODY_MAX + 1]).into_response(),
        other => panic!("unknown mode {other}"),
    }
}

/// A fake GitHub on 127.0.0.1: `/{mode}` answers that way; `/switch` as `fake.mode` says; and
/// `/race` answers 0.2.0 after 800 ms the first time, 0.3.0 at once afterwards.
async fn github() -> (SocketAddr, Fake) {
    let fake = Fake::default();
    *fake.mode.lock().unwrap() = "newer";
    let state = fake.clone();
    let app = Router::new()
        .route(
            "/switch",
            get({
                let state = state.clone();
                move |headers: HeaderMap| async move {
                    state.seen.lock().unwrap().push(headers);
                    let mode = *state.mode.lock().unwrap();
                    answer(mode).await
                }
            }),
        )
        .route(
            "/race",
            get({
                let state = state.clone();
                move || async move {
                    if state.calls.fetch_add(1, Ordering::SeqCst) == 0 {
                        tokio::time::sleep(Duration::from_millis(800)).await;
                        rustcms::axum::Json(json!([release("v0.2.0")])).into_response()
                    } else {
                        rustcms::axum::Json(json!([release("v0.3.0")])).into_response()
                    }
                }
            }),
        )
        .route(
            "/{mode}",
            get(|rustcms::axum::extract::Path(mode): rustcms::axum::extract::Path<String>| async move {
                answer(Box::leak(mode.into_boxed_str())).await
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { rustcms::axum::serve(listener, app).await.unwrap() });
    (addr, fake)
}

/// A server that answers 200 with a chunked body (no length) larger than the limit.
async fn chunked_giant() -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut request = [0u8; 4096];
                let _ = socket.read(&mut request).await;
                let head = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\n\r\n";
                if socket.write_all(head.as_bytes()).await.is_err() {
                    return;
                }
                let chunk = vec![b' '; 64 * 1024];
                for _ in 0..(version::BODY_MAX / chunk.len() + 4) {
                    let frame =
                        [format!("{:x}\r\n", chunk.len()).into_bytes(), chunk.clone(), b"\r\n".to_vec()].concat();
                    if socket.write_all(&frame).await.is_err() {
                        return;
                    }
                }
                let _ = socket.write_all(b"0\r\n\r\n").await;
            });
        }
    });
    addr
}

fn config(url: String) -> ReleasesConfig {
    ReleasesConfig { url, timeout_secs: 1 }
}

/// The application asking `url` for releases, running as `build`.
async fn app_with(db: TestDb, url: &str, running: Build) -> TestApp {
    let config = test_config(db.url())
        .with("releases.url", url)
        .and_then(|c| c.with("releases.timeout_secs", 1))
        .expect("test configuration");
    TestApp::with_config(
        move |config| {
            let mut app = bootstrap::build(config);
            app.bind(running);
            app
        },
        config,
        db,
    )
    .await
}

async fn super_admin(app: &TestApp) -> TestClient<'_> {
    let user = UserFactory::new().create(app.state().db()).await;
    app.make_super_admin(&user).await;
    app.acting_as(&user).await
}

fn uri(app: &TestApp, rest: &str) -> String {
    app.admin_api(&format!("x/system/version{rest}"))
}

async fn kept(app: &TestApp) -> release_check::Model {
    version::last(app.state().db()).await.unwrap()
}

/// Moves the last attempt `seconds` into the past (the cooldown is then over).
async fn age_attempt(app: &TestApp, seconds: i64) {
    let row = kept(app).await;
    let mut model: release_check::ActiveModel = row.clone().into();
    model.attempted_at =
        Set(row.attempted_at.map(|at| at - sea_orm::sea_query::prelude::chrono::TimeDelta::seconds(seconds)));
    model.update(app.state().db()).await.unwrap();
}

#[test]
fn versions_are_compared_strictly() {
    assert_eq!(parse("v0.2.0"), Some((0, 2, 0)));
    assert_eq!(parse("1.10.3"), Some((1, 10, 3)));
    for bad in
        ["0.2", "v1.0.0-rc.1", "1.0.0+build", "01.2.3", "1.2.3.4", "v", "", "latest", "1..2", "1.2.x", "1000000.0.0"]
    {
        assert_eq!(parse(bad), None, "{bad}");
    }
    assert!(rank("0.10.0") > rank("0.9.9"));
    assert!(rank("1.0.0") > rank("0.999999.999999"));
    assert_eq!(rank("999999.999999.999999"), Some(999_999_999_999_999_999));

    // Drafts, prereleases and tags that are not `vX.Y.Z` are skipped; the highest version wins,
    // whatever GitHub's order.
    let list: Vec<GitHubRelease> = serde_json::from_value(newer_list()).unwrap();
    let found = newest(list).unwrap();
    assert_eq!(found.version, "0.2.0");
    assert_eq!(found.notes.as_deref(), Some(NOTES));
    assert_eq!(found.url.as_deref(), Some("https://github.com/irisvn/RBlog/releases/tag/v0.2.0"));
    assert!(found.published_at.is_some());
    let unordered: Vec<GitHubRelease> =
        serde_json::from_value(json!([release("v0.9.9"), release("v0.10.0"), release("v0.2.0")])).unwrap();
    assert_eq!(newest(unordered).unwrap().version, "0.10.0");
    let only_previews: Vec<GitHubRelease> =
        serde_json::from_value(json!([{"tag_name": "v2.0.0", "draft": true}])).unwrap();
    assert_eq!(newest(only_previews), None);

    // Notes are cut at 100 000 characters (multi-byte ones count once); a page elsewhere than
    // github.com is dropped.
    let long: Vec<GitHubRelease> = serde_json::from_value(json!([{
        "tag_name": "v1.0.0",
        "body": "ệ".repeat(NOTES_MAX + 10),
        "html_url": "javascript:alert(1)",
    }]))
    .unwrap();
    let found = newest(long).unwrap();
    assert_eq!(found.notes.unwrap().chars().count(), NOTES_MAX);
    assert_eq!(found.url, None);
}

#[test]
fn development_and_source_builds_are_never_compared() {
    for channel in [Channel::Development, Channel::Source] {
        assert!(!build(channel).is_older_than("9.0.0"), "{channel:?}");
    }
    assert!(build(Channel::Release).is_older_than("0.2.0"));
    assert!(!build(Channel::Release).is_older_than("0.1.0"));
    assert!(!build(Channel::Release).is_older_than("0.0.9"));
    assert!(!build(Channel::Release).is_older_than("main"));
}

#[test]
fn the_releases_url_is_for_tests_only() {
    let with = |env: &str, key: &str, value: Value| {
        test_config("mysql://unused").with("app.env", env).unwrap().with(key, value).unwrap()
    };
    assert_eq!(ReleasesConfig::of(&test_config("mysql://unused")).unwrap().endpoint(), version::RELEASES_URL);
    assert!(ReleasesConfig::of(&with("testing", "releases.url", json!("http://127.0.0.1:1/r"))).is_ok());
    let refused = ReleasesConfig::of(&with("production", "releases.url", json!("http://127.0.0.1:1/r"))).unwrap_err();
    assert!(refused.to_string().contains("production"), "{refused}");
    assert!(ReleasesConfig::of(&with("testing", "releases.url", json!("file:///etc/passwd"))).is_err());
    assert!(ReleasesConfig::of(&with("testing", "releases.timeout_secs", json!(0))).is_err());
    assert!(ReleasesConfig::of(&with("testing", "releases.timeout_secs", json!(21))).is_err());
    assert!(
        ReleasesConfig::of(&with("testing", "releases.timeout_secs", json!("10"))).is_err(),
        "a value of the wrong kind"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_check_reads_a_fake_github() {
    let (addr, fake) = github().await;
    let running = build(Channel::Release);
    let base = format!("http://{addr}");

    // Newer: 0.2.0 (drafts and prereleases skipped), with only a user agent about us.
    let found = version::fetch(&config(format!("{base}/switch")), &running).await.unwrap().unwrap();
    assert_eq!(found.version, "0.2.0");
    assert!(running.is_older_than(&found.version));
    let headers = fake.seen.lock().unwrap().pop().unwrap();
    assert_eq!(headers.get(header::USER_AGENT).unwrap(), "rblog/0.1.0");
    assert!(headers.get(header::AUTHORIZATION).is_none() && headers.get(header::COOKIE).is_none());
    // The same version, an older one, none at all: no update.
    for (mode, latest) in [("same", Some("0.1.0")), ("older", Some("0.0.9")), ("empty", None)] {
        let found = version::fetch(&config(format!("{base}/{mode}")), &running).await.unwrap();
        assert_eq!(found.as_ref().map(|r| r.version.as_str()), latest, "{mode}");
        assert!(!found.is_some_and(|r| running.is_older_than(&r.version)), "{mode}");
    }

    // Failures, each with its reason.
    let fails = async |path: &str| version::fetch(&config(format!("{base}/{path}")), &running).await.unwrap_err();
    assert_eq!(fails("malformed").await, Failure::Malformed);
    assert_eq!(fails("object").await, Failure::Malformed);
    assert_eq!(fails("error").await, Failure::Http(500));
    assert_eq!(fails("limited").await, Failure::RateLimited(403));
    assert_eq!(fails("redirect").await, Failure::Http(302), "redirects are not followed");
    assert_eq!(fails("huge").await, Failure::TooLarge);
    let started = Instant::now();
    assert_eq!(fails("slow").await, Failure::Timeout);
    assert!(started.elapsed() < Duration::from_secs(3), "the time limit holds: {:?}", started.elapsed());
    let giant = chunked_giant().await;
    let found = version::fetch(&config(format!("http://{giant}/")), &running).await;
    assert_eq!(found.unwrap_err(), Failure::TooLarge, "a body without a length is bounded too");
    // Nobody listens on port 1.
    let found = version::fetch(&config("http://127.0.0.1:1/".into()), &running).await;
    assert_eq!(found.unwrap_err(), Failure::Unreachable);
}

db_test!(a_check_is_kept_and_survives_a_reload, migrate = Migrator, |db: TestDb| async move {
    let (addr, fake) = github().await;
    let url = db.url().to_owned();
    let app = app_with(db, &format!("http://{addr}/switch"), build(Channel::Release)).await;
    let admin = super_admin(&app).await;

    // Before any check.
    let before = admin.get(&uri(&app, "")).await;
    before.assert_status(StatusCode::OK).assert_json("/state", json!("not_checked"));
    before.assert_json("/update", Value::Null).assert_json("/release", Value::Null);
    before.assert_json("/running/version", json!("0.1.0")).assert_json("/running/channel", json!("release"));
    assert!(before.json()["running"]["mysql"].as_str().unwrap().starts_with("8.4"), "{}", before.json());

    // Checked: 0.2.0 is out, its notes come back as the same text (never HTML).
    let checked = admin.post_json(&uri(&app, "/check"), &json!({})).await;
    checked.assert_status(StatusCode::OK).assert_json("/state", json!("update")).assert_json("/update", json!("0.2.0"));
    checked.assert_json("/release/notes", json!(NOTES));
    checked.assert_json("/release/url", json!("https://github.com/irisvn/RBlog/releases/tag/v0.2.0"));
    checked.assert_json("/check/error", Value::Null);
    let row = kept(&app).await;
    assert_eq!((row.latest_version.as_deref(), row.latest_rank), (Some("0.2.0"), rank("0.2.0")));

    // A failed check keeps the last good result and says why; the next good one clears it.
    *fake.mode.lock().unwrap() = "malformed";
    version::check(app.state(), Trigger::Schedule).await.unwrap().unwrap().unwrap_err();
    let failed = admin.get(&uri(&app, "")).await;
    failed.assert_json("/check/error/code", json!("malformed")).assert_json("/update", json!("0.2.0"));
    *fake.mode.lock().unwrap() = "limited";
    version::check(app.state(), Trigger::Schedule).await.unwrap().unwrap().unwrap_err();
    admin
        .get(&uri(&app, ""))
        .await
        .assert_json("/check/error/code", json!("rate_limited"))
        .assert_json("/check/error/status", json!(403));
    *fake.mode.lock().unwrap() = "newer";
    version::check(app.state(), Trigger::Schedule).await.unwrap().unwrap().unwrap();
    admin.get(&uri(&app, "")).await.assert_json("/check/error", Value::Null).assert_json("/update", json!("0.2.0"));

    // Another process (a new connection) reads the same result.
    let other = sea_orm::Database::connect(url.as_str()).await.unwrap();
    let reloaded = version::last(&other).await.unwrap();
    assert_eq!(reloaded.latest_version.as_deref(), Some("0.2.0"));
    assert_eq!(reloaded.notes.as_deref(), Some(NOTES));
    assert!(reloaded.checked_at.is_some() && reloaded.error.is_none());

    // A list of only drafts and prereleases (the stable ones on a later page): the last one is kept.
    *fake.mode.lock().unwrap() = "previews";
    let found = version::check(app.state(), Trigger::Schedule).await.unwrap().unwrap();
    assert_eq!(found.unwrap_err(), Failure::OnlyPreviews);
    admin
        .get(&uri(&app, ""))
        .await
        .assert_json("/update", json!("0.2.0"))
        .assert_json("/check/error/code", json!("only_previews"));

    // Releases all gone: none is known any more.
    *fake.mode.lock().unwrap() = "empty";
    version::check(app.state(), Trigger::Schedule).await.unwrap().unwrap().unwrap();
    admin.get(&uri(&app, "")).await.assert_json("/state", json!("no_release")).assert_json("/release", Value::Null);
});

db_test!(an_older_check_never_overwrites_a_newer_one, migrate = Migrator, |db: TestDb| async move {
    let (addr, _fake) = github().await;
    let app = app_with(db, &format!("http://{addr}/race"), build(Channel::Release)).await;
    // The first check waits 800 ms for 0.2.0; the second, started meanwhile, gets 0.3.0 at once.
    let (first, second) = tokio::join!(version::check(app.state(), Trigger::Schedule), async {
        tokio::time::sleep(Duration::from_millis(300)).await;
        version::check(app.state(), Trigger::Schedule).await
    });
    assert_eq!(first.unwrap().unwrap().unwrap().unwrap().version, "0.2.0");
    assert_eq!(second.unwrap().unwrap().unwrap().unwrap().version, "0.3.0");
    assert_eq!(kept(&app).await.latest_version.as_deref(), Some("0.3.0"));
});

db_test!(a_manual_check_waits_a_minute, migrate = Migrator, |db: TestDb| async move {
    let (addr, _fake) = github().await;
    let app = app_with(db, &format!("http://{addr}/switch"), build(Channel::Release)).await;
    let admin = super_admin(&app).await;
    let other = super_admin(&app).await;

    let first = admin.post_json(&uri(&app, "/check"), &json!({})).await;
    first.assert_status(StatusCode::OK);
    assert!(first.json()["check"]["next_manual_at"].is_string());
    // Anyone, within the minute: 429 with how long to wait.
    let refused = other.post_json(&uri(&app, "/check"), &json!({})).await;
    refused.assert_status(StatusCode::TOO_MANY_REQUESTS);
    let wait: u64 = refused.header("retry-after").unwrap().parse().unwrap();
    assert!((1..=60).contains(&wait), "Retry-After: {wait}");
    let attempted = kept(&app).await.attempted_at;
    // The schedule is never held back.
    version::check(app.state(), Trigger::Schedule).await.unwrap().unwrap().unwrap();
    assert!(kept(&app).await.attempted_at > attempted);
    admin.post_json(&uri(&app, "/check"), &json!({})).await.assert_status(StatusCode::TOO_MANY_REQUESTS);
    // A minute later: allowed again.
    age_attempt(&app, 61).await;
    admin.post_json(&uri(&app, "/check"), &json!({})).await.assert_status(StatusCode::OK);
});

db_test!(concurrent_manual_checks_queue_on_the_row, migrate = Migrator, |db: TestDb| async move {
    let (addr, _fake) = github().await;
    let app = app_with(db, &format!("http://{addr}/switch"), build(Channel::Release)).await;
    // Five at once: one runs, the others are told to wait; none fails on a lock.
    let check = || version::check(app.state(), Trigger::Manual);
    let (a, b, c, d, e) = tokio::join!(check(), check(), check(), check(), check());
    let results = [a, b, c, d, e];
    let ran = results.iter().filter(|r| matches!(r, Ok(Ok(_)))).count();
    let refused = results.iter().filter(|r| matches!(r, Ok(Err(_)))).count();
    assert_eq!((ran, refused), (1, 4), "{results:?}");
});

db_test!(the_check_runs_every_six_hours, migrate = Migrator, |db: TestDb| async move {
    let app = app_with(db, "http://127.0.0.1:1/", build(Channel::Release)).await;
    let admin = super_admin(&app).await;
    let schedules = admin.get(&app.admin_api("schedules")).await;
    schedules.assert_status(StatusCode::OK);
    let list = schedules.json();
    let found = list["data"].as_array().unwrap().iter().find(|s| s["name"] == version::SCHEDULE).cloned();
    let found = found.unwrap_or_else(|| panic!("no `{}` schedule: {list}", version::SCHEDULE));
    assert_eq!(found["rule"], "every 21600s");
});

db_test!(only_system_version_sees_the_version, migrate = Migrator, |db: TestDb| async move {
    let (addr, _fake) = github().await;
    let app = app_with(db, &format!("http://{addr}/switch"), build(Channel::Release)).await;
    version::check(app.state(), Trigger::Schedule).await.unwrap().unwrap().unwrap();
    let anonymous = app.client();
    anonymous.fetch_csrf().await;
    // Everything an editor has, and every other `release_updates.*` permission: still nothing.
    let mut others: Vec<&str> = rblog::database::seeders::role_seeder::EDITOR.to_vec();
    others.extend(["integrations.view", "integrations.manage", "release_updates.view_any", "release_updates.view"]);
    let nobody = holding(&app, "no-version", &others).await;
    let nobody = app.acting_as(&nobody).await;
    let holder = holding(&app, "version", &[version::PERMISSION]).await;
    let holder = app.acting_as(&holder).await;

    anonymous.get(&uri(&app, "")).await.assert_status(StatusCode::UNAUTHORIZED);
    anonymous.post_json(&uri(&app, "/check"), &json!({})).await.assert_status(StatusCode::UNAUTHORIZED);
    let routes = [
        (uri(&app, ""), false),
        (uri(&app, "/check"), true),
        (app.admin_api("pages/version"), false),
        (app.admin_api("resources/release_updates/records"), false),
        (app.admin_api("resources/release_updates/records/1"), false),
        (app.admin_api("dashboard/widgets/rblog_update"), false),
    ];
    for (route, write) in &routes {
        let refused = if *write { nobody.post_json(route, &json!({})).await } else { nobody.get(route).await };
        refused.assert_status(StatusCode::FORBIDDEN);
    }
    // (The check above started less than a minute ago.)
    age_attempt(&app, 61).await;
    for (route, write) in &routes {
        let allowed = if *write { holder.post_json(route, &json!({})).await } else { holder.get(route).await };
        allowed.assert_status(StatusCode::OK);
    }
    // The page is in the holder's navigation only.
    let pages = |panel: Value| panel["pages"].as_array().unwrap().iter().any(|p| p["slug"] == "version");
    assert!(pages(holder.get(&app.admin_api("panel")).await.json()));
    assert!(!pages(nobody.get(&app.admin_api("panel")).await.json()));

    // The kept row is written by the server alone, even a super admin cannot change it.
    let admin = super_admin(&app).await;
    let record = app.admin_api("resources/release_updates/records/1");
    admin
        .send(rustcms::axum::http::Method::PATCH, &record, Some(&json!({"latest_version": "9.9.9"})))
        .await
        .assert_status(StatusCode::FORBIDDEN);
    admin.send(rustcms::axum::http::Method::DELETE, &record, None).await.assert_status(StatusCode::FORBIDDEN);
    admin
        .post_json(&app.admin_api("resources/release_updates/records"), &json!({"latest_version": "9.9.9"}))
        .await
        .assert_status(StatusCode::FORBIDDEN);
    assert_eq!(kept(&app).await.latest_version.as_deref(), Some("0.2.0"));
});

/// Whether `client`'s dashboard lists the notice, and the versions it shows (`None`: refused).
async fn notice(app: &TestApp, client: &TestClient<'_>) -> (bool, Option<Vec<String>>) {
    let dashboard = client.get(&app.admin_api("dashboard")).await;
    dashboard.assert_status(StatusCode::OK);
    let listed = dashboard.json()["widgets"].as_array().unwrap().iter().any(|w| w["name"] == "rblog_update");
    let data = client.get(&app.admin_api("dashboard/widgets/rblog_update")).await;
    let shown = (data.status == StatusCode::OK).then(|| {
        data.json()["records"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["title"].as_str().unwrap_or_default().to_owned())
            .collect()
    });
    (listed, shown)
}

db_test!(the_dashboard_notice_is_for_system_version_only, migrate = Migrator, |db: TestDb| async move {
    let (addr, fake) = github().await;
    let app = app_with(db, &format!("http://{addr}/switch"), build(Channel::Release)).await;
    let holder = holding(&app, "version", &[version::PERMISSION]).await;
    let holder = app.acting_as(&holder).await;
    let mut editor_permissions: Vec<&str> = rblog::database::seeders::role_seeder::EDITOR.to_vec();
    editor_permissions.extend(["release_updates.view_any", "release_updates.view"]);
    let editor = holding(&app, "editor", &editor_permissions).await;
    let editor = app.acting_as(&editor).await;

    // Not checked yet: listed, empty.
    assert_eq!(notice(&app, &holder).await, (true, Some(vec![])));
    // A newer release: shown to the holder, never to the editor.
    version::check(app.state(), Trigger::Schedule).await.unwrap().unwrap().unwrap();
    assert_eq!(notice(&app, &holder).await, (true, Some(vec!["0.2.0".to_owned()])));
    assert_eq!(notice(&app, &editor).await, (false, None));
    // Up to date: nothing to show.
    *fake.mode.lock().unwrap() = "same";
    version::check(app.state(), Trigger::Schedule).await.unwrap().unwrap().unwrap();
    assert_eq!(notice(&app, &holder).await, (true, Some(vec![])));
});

db_test!(a_development_build_shows_no_notice, migrate = Migrator, |db: TestDb| async move {
    let (addr, _fake) = github().await;
    let app = app_with(db, &format!("http://{addr}/switch"), build(Channel::Development)).await;
    let admin = super_admin(&app).await;
    let checked = admin.post_json(&uri(&app, "/check"), &json!({})).await;
    // The newest release is shown, never compared.
    checked
        .assert_status(StatusCode::OK)
        .assert_json("/state", json!("not_compared"))
        .assert_json("/update", Value::Null);
    checked.assert_json("/release/version", json!("0.2.0")).assert_json("/running/channel", json!("development"));
    assert_eq!(notice(&app, &admin).await, (true, Some(vec![])));
});

db_test!(newer_migrations_are_reported, migrate = Migrator, |db: TestDb| async move {
    let app = app_with(db, "http://127.0.0.1:1/", build(Channel::Release)).await;
    let admin = super_admin(&app).await;
    admin.get(&uri(&app, "")).await.assert_json("/newer_migrations", json!([]));
    // A migration this binary does not know: applied by a newer version (rolled back since).
    let db = app.state().db();
    db.execute_raw(Statement::from_string(
        db.get_database_backend(),
        "INSERT INTO seaql_migrations (version, applied_at) VALUES ('m29990101_000000_from_the_future', 0)",
    ))
    .await
    .unwrap();
    admin.get(&uri(&app, "")).await.assert_json("/newer_migrations", json!(["m29990101_000000_from_the_future"]));
});
