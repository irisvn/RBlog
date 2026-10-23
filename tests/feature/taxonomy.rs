//! Categories and tags through the admin API: slugs made from Vietnamese names, taken slugs
//! refused, renames kept in `slug_history`, and an outbox event for every write.

use rblog::database::migrations::Migrator;
use rblog::models::{category, content_event, slug_history, tag};
use rustcms::axum::http::{Method, StatusCode};
use rustcms_testing::{Factory, TestApp, TestClient, TestDb, UserFactory, db_test};
use sea_orm::{ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter, QueryOrder};
use serde_json::{Value, json};

use crate::support;

/// A signed-in super admin.
pub async fn admin(app: &TestApp) -> TestClient<'_> {
    let user =
        UserFactory::new().email("admin@example.test").password("secret password").create(app.state().db()).await;
    app.make_super_admin(&user).await;
    app.login_as("admin@example.test", "secret password").await
}

/// The outbox events of `subject_type` `id`, oldest first.
pub async fn events(db: &impl ConnectionTrait, subject_type: &str, id: i64) -> Vec<content_event::Model> {
    content_event::Entity::find()
        .filter(content_event::Column::SubjectType.eq(subject_type))
        .filter(content_event::Column::SubjectId.eq(id))
        .order_by_asc(content_event::Column::CreatedAt)
        .order_by_asc(content_event::Column::Id)
        .all(db)
        .await
        .unwrap()
}

fn key(response: &Value) -> i64 {
    response["data"]["key"].as_str().expect("record key").parse().unwrap()
}

db_test!(a_category_slug_is_made_from_its_vietnamese_name, migrate = Migrator, |db: TestDb| async move {
    let app = support::app(db).await;
    let panel = admin(&app).await;
    let records = app.admin_api("resources/categories/records");

    let created = panel.post_json(&records, &json!({"name": "Du lịch Đà Nẵng"})).await;
    created.assert_status(StatusCode::CREATED).assert_json("/data/data/slug", json!("du-lich-da-nang"));
    let id = key(&created.json());
    // The same name again: the next free slug.
    panel
        .post_json(&records, &json!({"name": "Du lịch Đà Nẵng"}))
        .await
        .assert_status(StatusCode::CREATED)
        .assert_json("/data/data/slug", json!("du-lich-da-nang-2"));
    // A blank slug is made too; a typed one is normalized.
    panel
        .post_json(&records, &json!({"name": "Ẩm thực", "slug": "  "}))
        .await
        .assert_status(StatusCode::CREATED)
        .assert_json("/data/data/slug", json!("am-thuc"));
    panel
        .post_json(&records, &json!({"name": "Phở", "slug": "Phở Hà Nội!"}))
        .await
        .assert_status(StatusCode::CREATED)
        .assert_json("/data/data/slug", json!("pho-ha-noi"));

    let events = events(app.state().db(), "category", id).await;
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(events[0].kind, "category.created");
    assert_eq!(events[0].delivered_at, None);
    assert_eq!(
        events[0].payload,
        json!({"tags": ["category:du-lich-da-nang", "posts", "sitemap"],
               "paths": {"old": [], "new": ["/category/du-lich-da-nang"]}})
    );
});

db_test!(a_taken_typed_slug_is_refused_on_the_field, migrate = Migrator, |db: TestDb| async move {
    let app = support::app(db).await;
    let panel = admin(&app).await;
    for resource in ["categories", "tags"] {
        let records = app.admin_api(&format!("resources/{resource}/records"));
        panel.post_json(&records, &json!({"name": "Rust"})).await.assert_status(StatusCode::CREATED);
        panel.post_json(&records, &json!({"name": "Other", "slug": "RUST"})).await.assert_validation_error("slug");
        // Nothing left of it once normalized.
        panel.post_json(&records, &json!({"name": "Other", "slug": "!!!"})).await.assert_validation_error("slug");
        // A blank name is no name.
        panel.post_json(&records, &json!({"name": "  "})).await.assert_validation_error("name");
        // Renaming onto a taken slug.
        let other = panel.post_json(&records, &json!({"name": "Go"})).await;
        let other = key(&other.json());
        panel
            .send(Method::PATCH, &format!("{records}/{other}"), Some(&json!({"slug": "rust"})))
            .await
            .assert_validation_error("slug");
    }
    // The message speaks the user's language.
    let records = app.admin_api("resources/categories/records");
    let panel = panel.languages("vi");
    let refused = panel.post_json(&records, &json!({"name": "Other", "slug": "rust"})).await;
    refused.assert_json("/errors/slug/0", json!("đã được dùng"));
    // Refused writes leave no event behind.
    let all = content_event::Entity::find().all(app.state().db()).await.unwrap();
    assert_eq!(all.iter().filter(|e| e.kind.ends_with(".created")).count(), 4, "{all:?}");
});

db_test!(a_rename_keeps_the_old_slug_and_reports_both, migrate = Migrator, |db: TestDb| async move {
    let app = support::app(db).await;
    let panel = admin(&app).await;
    let db = app.state().db();
    let records = app.admin_api("resources/categories/records");
    let created = panel.post_json(&records, &json!({"name": "Du lịch"})).await;
    let id = key(&created.json());
    let record = format!("{records}/{id}");

    // A new name keeps the slug.
    panel.send(Method::PATCH, &record, Some(&json!({"name": "Du lịch Việt Nam"}))).await.assert_status(StatusCode::OK);
    assert!(slug_history::Entity::find().all(db).await.unwrap().is_empty());
    let events_now = events(db, "category", id).await;
    assert_eq!(events_now.len(), 2);
    assert_eq!(events_now[1].kind, "category.updated");
    assert_eq!(events_now[1].payload["tags"], json!(["category:du-lich", "posts", "sitemap"]));

    // A cleared slug is made again from the name: a rename.
    panel
        .send(Method::PATCH, &record, Some(&json!({"slug": ""})))
        .await
        .assert_status(StatusCode::OK)
        .assert_json("/data/data/slug", json!("du-lich-viet-nam"));
    let history = slug_history::Entity::find().all(db).await.unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!((history[0].subject_type.as_str(), history[0].subject_id), ("category", id));
    assert_eq!(history[0].old_slug, "du-lich");
    let events_now = events(db, "category", id).await;
    assert_eq!(events_now.len(), 3);
    assert_eq!(events_now[2].kind, "category.updated");
    assert_eq!(
        events_now[2].payload,
        json!({"tags": ["category:du-lich-viet-nam", "category:du-lich", "posts", "sitemap"],
               "paths": {"old": ["/category/du-lich"], "new": ["/category/du-lich-viet-nam"]}})
    );

    // Back to the first slug: it answers again, and the second one redirects.
    panel.send(Method::PATCH, &record, Some(&json!({"slug": "du-lich"}))).await.assert_status(StatusCode::OK);
    let history = slug_history::Entity::find().all(db).await.unwrap();
    assert_eq!(history.iter().map(|h| h.old_slug.as_str()).collect::<Vec<_>>(), ["du-lich-viet-nam"]);
    let last = events(db, "category", id).await.pop().unwrap();
    assert_eq!(last.payload["tags"], json!(["category:du-lich", "category:du-lich-viet-nam", "posts", "sitemap"]));

    // Tags work the same way.
    let tags = app.admin_api("resources/tags/records");
    let created = panel.post_json(&tags, &json!({"name": "Hà Nội"})).await;
    let tag_id = key(&created.json());
    panel
        .send(Method::PATCH, &format!("{tags}/{tag_id}"), Some(&json!({"slug": "thu-do"})))
        .await
        .assert_status(StatusCode::OK);
    let stored = tag::Entity::find_by_id(tag_id).one(db).await.unwrap().unwrap();
    assert_eq!(stored.slug, "thu-do");
    assert!(stored.updated_at > stored.created_at);
    let tag_events = events(db, "tag", tag_id).await;
    assert_eq!(tag_events.iter().map(|e| e.kind.as_str()).collect::<Vec<_>>(), ["tag.created", "tag.updated"]);
    assert_eq!(tag_events[1].payload["tags"], json!(["tag:thu-do", "tag:ha-noi", "posts", "sitemap"]));
    assert_eq!(tag_events[1].payload["paths"], json!({"old": ["/tag/ha-noi"], "new": ["/tag/thu-do"]}));
});

db_test!(deleting_records_an_event_and_forgets_old_slugs, migrate = Migrator, |db: TestDb| async move {
    let app = support::app(db).await;
    let panel = admin(&app).await;
    let db = app.state().db();
    let records = app.admin_api("resources/categories/records");
    let id = key(&panel.post_json(&records, &json!({"name": "Cũ"})).await.json());
    let record = format!("{records}/{id}");
    panel.send(Method::PATCH, &record, Some(&json!({"slug": "moi"}))).await.assert_status(StatusCode::OK);
    assert_eq!(slug_history::Entity::find().all(db).await.unwrap().len(), 1);

    panel.send(Method::DELETE, &record, None).await.assert_status(StatusCode::NO_CONTENT);
    assert!(category::Entity::find_by_id(id).one(db).await.unwrap().is_none());
    assert!(slug_history::Entity::find().all(db).await.unwrap().is_empty());
    let last = events(db, "category", id).await.pop().unwrap();
    assert_eq!(last.kind, "category.deleted");
    assert_eq!(
        last.payload,
        json!({"tags": ["category:moi", "posts", "sitemap"], "paths": {"old": ["/category/moi"], "new": []}})
    );

    let tags = app.admin_api("resources/tags/records");
    let tag_id = key(&panel.post_json(&tags, &json!({"name": "Rust"})).await.json());
    panel.send(Method::DELETE, &format!("{tags}/{tag_id}"), None).await.assert_status(StatusCode::NO_CONTENT);
    assert_eq!(events(db, "tag", tag_id).await.pop().unwrap().kind, "tag.deleted");
});

db_test!(users_without_the_permissions_are_refused, migrate = Migrator, |db: TestDb| async move {
    let app = support::app(db).await;
    let id = {
        let panel = admin(&app).await;
        key(&panel.post_json(&app.admin_api("resources/categories/records"), &json!({"name": "Tin tức"})).await.json())
    };
    let user = UserFactory::new().create(app.state().db()).await;
    let editor = app.acting_as(&user).await;
    for resource in ["categories", "tags"] {
        let records = app.admin_api(&format!("resources/{resource}/records"));
        editor.get(&records).await.assert_status(StatusCode::FORBIDDEN);
        editor.post_json(&records, &json!({"name": "X"})).await.assert_status(StatusCode::FORBIDDEN);
    }
    let record = app.admin_api(&format!("resources/categories/records/{id}"));
    editor.send(Method::PATCH, &record, Some(&json!({"name": "Y"}))).await.assert_status(StatusCode::FORBIDDEN);
    editor.send(Method::DELETE, &record, None).await.assert_status(StatusCode::FORBIDDEN);
    assert_eq!(events(app.state().db(), "category", id).await.len(), 1, "only the admin's create");
});
