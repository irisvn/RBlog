//! The scheduler (`services::publishing::publish_due`): every due scheduled post and page goes
//! live, all of them after downtime, and nothing else; the dashboard's post widgets count what
//! each role sees.

use rblog::database::migrations::Migrator;
use rblog::models::{page, post, public_media};
use rblog::services::content_sync::now;
use rblog::services::publishing::{self, BATCH};
use rustcms::axum::http::{Method, StatusCode};
use rustcms_testing::{TestDb, db_test};
use sea_orm::prelude::DateTimeUtc;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, IntoActiveModel, PaginatorTrait, QueryFilter, Set};
use serde_json::json;

use crate::support::content;
use crate::support::posts::*;

fn in_hours(hours: i64) -> String {
    let at = DateTimeUtc::from_timestamp(now().timestamp() + hours * 3600, 0).unwrap();
    at.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// Moves a scheduled post's date `hours` into the past: the time it was due came while the app
/// was down.
async fn due_since(db: &sea_orm::DatabaseConnection, key: &str, hours: i64) -> DateTimeUtc {
    let at = DateTimeUtc::from_timestamp_micros(now().timestamp_micros() - hours * 3_600_000_000).unwrap();
    let mut row = post_row(db, key).await.into_active_model();
    row.published_at = Set(Some(at));
    row.update(db).await.unwrap();
    at
}

db_test!(publish_due_catches_up_after_downtime, migrate = Migrator, |db: TestDb| async move {
    let app = content::app(db).await;
    let db = app.state().db();
    let (editor, _) = signed_in(&app, "Biên", EDITOR_ROLE).await;
    let cover = content::stored_media(&app, "bia.png", "image/png", b"\x89PNG fake").await;
    let mut due = Vec::new();
    for n in 0..3 {
        let key = create(
            &app,
            &editor,
            "posts",
            json!({"title": format!("Hẹn {n}"), "cover_media_id": cover.id.to_string()}),
        )
        .await;
        run(&app, &editor, "posts", "schedule", &key, Some(json!({"at": in_hours(2)}))).await;
        due.push(key);
    }
    // Not public while scheduled: no public media.
    assert_eq!(public_media::Entity::find().count(db).await.unwrap(), 0);
    let later = create(&app, &editor, "posts", json!({"title": "Còn lâu"})).await;
    run(&app, &editor, "posts", "schedule", &later, Some(json!({"at": in_hours(24)}))).await;
    let trashed = create(&app, &editor, "posts", json!({"title": "Thùng rác"})).await;
    run(&app, &editor, "posts", "schedule", &trashed, Some(json!({"at": in_hours(2)}))).await;
    editor
        .send(Method::DELETE, &records(&app, "posts", Some(&trashed)), None)
        .await
        .assert_status(StatusCode::NO_CONTENT);
    let page = create(&app, &editor, "pages", json!({"title": "Trang hẹn giờ"})).await;
    run(&app, &editor, "pages", "schedule", &page, Some(json!({"at": in_hours(2)}))).await;

    // Nothing is due yet.
    assert_eq!(publishing::publish_due(db).await.unwrap(), 0);
    // The app was down while they fell due.
    let mut dates = Vec::new();
    for (n, key) in due.iter().enumerate() {
        dates.push(due_since(db, key, 3 - n as i64).await);
    }
    due_since(db, &trashed, 1).await;
    let mut page_row_ = page_row(db, &page).await.into_active_model();
    page_row_.published_at =
        Set(Some(DateTimeUtc::from_timestamp_micros(now().timestamp_micros() - 60_000_000).unwrap()));
    page_row_.update(db).await.unwrap();

    assert_eq!(publishing::publish_due(db).await.unwrap(), 4);
    for (key, at) in due.iter().zip(&dates) {
        let row = post_row(db, key).await;
        assert_eq!((row.status.as_str(), row.published_at), ("published", Some(*at)), "the scheduled time is kept");
        assert_eq!(event_kinds(db, "post", key).await.last().unwrap(), "post.published");
    }
    assert_eq!(page_row(db, &page).await.status, "published");
    assert_eq!(post_row(db, &later).await.status, "scheduled");
    assert_eq!(post_row(db, &trashed).await.status, "scheduled", "a trashed post stays in the trash");
    // Their cover is public now (the known gap: media of a post that falls due).
    let rows = public_media::Entity::find().filter(public_media::Column::MediaId.eq(cover.id)).count(db).await.unwrap();
    assert_eq!(rows, 3);
    let client = app.client();
    client.get(&format!("/api/v1/media/{}/bia.png", cover.id)).await.assert_status(StatusCode::OK);
    // A second run finds nothing.
    assert_eq!(publishing::publish_due(db).await.unwrap(), 0);
});

db_test!(publish_due_takes_every_batch, migrate = Migrator, |db: TestDb| async move {
    let app = content::app(db).await;
    let db = app.state().db();
    let writer = content::author(db, "An", None).await;
    let total = BATCH as i64 + 20;
    for n in 0..total {
        let mut row = content::post(&writer, &format!("bai-{n}"), &format!("Bài {n}"), 1);
        row.status = Set("scheduled".into());
        row.insert(db).await.unwrap();
    }
    assert_eq!(publishing::publish_due(db).await.unwrap(), total as u64);
    let scheduled = post::Entity::find().filter(post::Column::Status.eq("scheduled")).count(db).await.unwrap();
    assert_eq!(scheduled, 0);
    let mut pages = 0;
    for n in 0..3 {
        let mut row = content::page(&writer, &format!("trang-{n}"), 1);
        row.status = Set("scheduled".into());
        row.insert(db).await.unwrap();
        pages += 1;
    }
    assert_eq!(publishing::publish_due(db).await.unwrap(), pages);
    assert_eq!(page::Entity::find().filter(page::Column::Status.eq("published")).count(db).await.unwrap(), 3);
});

db_test!(published_means_live_and_a_future_date_means_scheduled, migrate = Migrator, |db: TestDb| async move {
    let app = content::app(db).await;
    let db = app.state().db();
    let (editor, _) = signed_in(&app, "Biên", EDITOR_ROLE).await;
    let key = create(&app, &editor, "posts", json!({"title": "Bài"})).await;
    // Scheduled for later, then published now: the date becomes now, never the future one.
    run(&app, &editor, "posts", "schedule", &key, Some(json!({"at": in_hours(48)}))).await;
    run(&app, &editor, "posts", "publish", &key, None).await;
    let row = post_row(db, &key).await;
    assert!(row.published_at.unwrap() <= now());
    // Every published post or page is live, every future date is scheduled.
    let other = create(&app, &editor, "posts", json!({"title": "Khác"})).await;
    run(&app, &editor, "posts", "schedule", &other, Some(json!({"at": in_hours(1)}))).await;
    publishing::publish_due(db).await.unwrap();
    let published = post::Entity::find().filter(post::Column::Status.eq("published")).all(db).await.unwrap();
    assert!(published.iter().all(|p| p.published_at.is_some_and(|at| at <= now())), "{published:?}");
    let future = post::Entity::find().filter(post::Column::PublishedAt.gt(now())).all(db).await.unwrap();
    assert!(future.iter().all(|p| p.status == "scheduled"), "{future:?}");
    assert_eq!(future.len(), 1);
});

db_test!(post_widgets_count_what_each_role_sees, migrate = Migrator, |db: TestDb| async move {
    let app = content::app(db).await;
    let (editor, _) = signed_in(&app, "Biên", EDITOR_ROLE).await;
    let (author, _) = signed_in(&app, "An", AUTHOR_ROLE).await;
    let (contributor, _) = signed_in(&app, "Chi", CONTRIBUTOR_ROLE).await;
    let a = create(&app, &author, "posts", json!({"title": "Của An"})).await;
    run(&app, &author, "posts", "publish", &a, None).await;
    create(&app, &author, "posts", json!({"title": "Nháp của An"})).await;
    let c = create(&app, &contributor, "posts", json!({"title": "Của Chi"})).await;
    run(&app, &contributor, "posts", "submit", &c, None).await;
    let e = create(&app, &editor, "posts", json!({"title": "Của Biên"})).await;
    run(&app, &editor, "posts", "schedule", &e, Some(json!({"at": in_hours(5)}))).await;

    let value = async |client: &rustcms_testing::TestClient<'_>, widget: &str| -> String {
        let shown = client.get(&app.admin_api(&format!("dashboard/widgets/{widget}"))).await;
        shown.assert_status(StatusCode::OK);
        shown.json()["value"].as_str().unwrap().to_owned()
    };
    // The editor sees every post.
    assert_eq!(value(&editor, "published_posts").await, "1");
    assert_eq!(value(&editor, "draft_posts").await, "1");
    assert_eq!(value(&editor, "pending_posts").await, "1");
    assert_eq!(value(&editor, "scheduled_posts").await, "1");
    // An author and a contributor count their own.
    assert_eq!(value(&author, "published_posts").await, "1");
    assert_eq!(value(&author, "draft_posts").await, "1");
    assert_eq!(value(&author, "pending_posts").await, "0");
    assert_eq!(value(&contributor, "pending_posts").await, "1");
    assert_eq!(value(&contributor, "published_posts").await, "0");
    let recent = contributor.get(&app.admin_api("dashboard/widgets/recent_posts")).await;
    recent.assert_status(StatusCode::OK);
    assert_eq!(recent.json()["records"].as_array().map(Vec::len), Some(1), "{}", recent.text());
});
