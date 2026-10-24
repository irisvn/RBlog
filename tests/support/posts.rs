//! Posts and pages through the admin API, as the seeded roles (`RoleSeeder`).

use rblog::database::seeders::role_seeder::{self, RoleSeeder};
use rblog::models::{content_event, page, post};
use rustcms::auth::{Actor, AuthState, RbacService};
use rustcms::axum::http::{Method, StatusCode};
use rustcms::database::Seeder;
use rustcms::models::user;
use rustcms_testing::{Factory, TestApp, TestClient, TestResponse, UserFactory};
use sea_orm::{ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter, QueryOrder};
use serde_json::{Value, json};

pub use role_seeder::{ADMINISTRATOR_ROLE, AUTHOR_ROLE, CONTRIBUTOR_ROLE, EDITOR_ROLE};

/// A new user named `name` with the seeded role `role` (the roles are seeded when missing).
pub async fn user_with_role(app: &TestApp, name: &str, role: &str) -> user::Model {
    RoleSeeder.run(app.state()).await.unwrap();
    let db = app.state().db();
    let auth = app.state().service::<AuthState>().unwrap();
    let role = role_seeder::find(app.state(), role).await.unwrap().expect("seeded role");
    let user = UserFactory::new().name(name).create(db).await;
    RbacService::new(db, &auth.resolver).assign_role(Actor::System, user.id, role.id).await.unwrap();
    user
}

/// A signed-in client of a new user with `role`, and the user.
pub async fn signed_in<'a>(app: &'a TestApp, name: &str, role: &str) -> (TestClient<'a>, user::Model) {
    let user = user_with_role(app, name, role).await;
    (app.acting_as(&user).await, user)
}

/// `{admin}/api/resources/{slug}/records[/{key}]`.
pub fn records(app: &TestApp, slug: &str, key: Option<&str>) -> String {
    match key {
        Some(key) => app.admin_api(&format!("resources/{slug}/records/{key}")),
        None => app.admin_api(&format!("resources/{slug}/records")),
    }
}

/// The key of a created or read record.
pub fn key(response: &TestResponse) -> String {
    response.json()["data"]["key"].as_str().expect("record key").to_owned()
}

/// Creates a post (201 expected) and returns its key.
pub async fn create(app: &TestApp, client: &TestClient<'_>, slug: &str, body: Value) -> String {
    let created = client.post_json(&records(app, slug, None), &body).await;
    created.assert_status(StatusCode::CREATED);
    key(&created)
}

pub async fn update(app: &TestApp, client: &TestClient<'_>, slug: &str, key: &str, body: Value) -> TestResponse {
    client.send(Method::PATCH, &records(app, slug, Some(key)), Some(&body)).await
}

/// Runs action `name` on `keys` (with `data` when the action has a form).
pub async fn action(
    app: &TestApp,
    client: &TestClient<'_>,
    slug: &str,
    name: &str,
    keys: &[&str],
    data: Option<Value>,
) -> TestResponse {
    let mut body = json!({ "keys": keys });
    if let Some(data) = data {
        body["data"] = data;
    }
    client.post_json(&app.admin_api(&format!("resources/{slug}/actions/{name}")), &body).await
}

/// Runs action `name` on one record and expects it processed.
pub async fn run(app: &TestApp, client: &TestClient<'_>, slug: &str, name: &str, key: &str, data: Option<Value>) {
    let ran = action(app, client, slug, name, &[key], data).await;
    ran.assert_status(StatusCode::OK).assert_json("/processed", json!([key]));
}

pub async fn post_row(db: &impl ConnectionTrait, key: &str) -> post::Model {
    post::Entity::find_by_id(key.parse::<i64>().unwrap()).one(db).await.unwrap().expect("post")
}

pub async fn page_row(db: &impl ConnectionTrait, key: &str) -> page::Model {
    page::Entity::find_by_id(key.parse::<i64>().unwrap()).one(db).await.unwrap().expect("page")
}

/// The kinds of the outbox events of `subject_type` `key`, oldest first.
pub async fn event_kinds(db: &impl ConnectionTrait, subject_type: &str, key: &str) -> Vec<String> {
    events(db, subject_type, key).await.into_iter().map(|e| e.kind).collect()
}

pub async fn events(db: &impl ConnectionTrait, subject_type: &str, key: &str) -> Vec<content_event::Model> {
    content_event::Entity::find()
        .filter(content_event::Column::SubjectType.eq(subject_type))
        .filter(content_event::Column::SubjectId.eq(key.parse::<i64>().unwrap()))
        .order_by_asc(content_event::Column::Id)
        .all(db)
        .await
        .unwrap()
}

/// The key of the oldest revision of post `key`.
pub async fn first_revision(db: &impl ConnectionTrait, key: &str) -> String {
    use rblog::models::post_revision;
    post_revision::Entity::find()
        .filter(post_revision::Column::PostId.eq(key.parse::<i64>().unwrap()))
        .order_by_asc(post_revision::Column::Id)
        .one(db)
        .await
        .unwrap()
        .expect("a revision")
        .id
        .to_string()
}

/// A rich-text document: a paragraph of `text`, then an image of each of `images`.
pub fn document(text: &str, images: &[String]) -> Value {
    let mut content = vec![json!({"type": "paragraph", "content": [{"type": "text", "text": text}]})];
    content.extend(images.iter().map(|id| json!({"type": "image", "attrs": {"media": id, "alt": "hình"}})));
    json!({"type": "doc", "version": 1, "content": content})
}
