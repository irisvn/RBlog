//! Content for the Content API tests, written straight through the entities (the admin write
//! paths come later), and a client holding a Content API key.

use std::path::PathBuf;

use rblog::bootstrap;
use rblog::models::{author_profile, category, page, post, post_tag, tag};
use rblog::services::content_api::{ensure_frontend_user, issue_content_key};
use rblog::services::search::normalize;
use rustcms::admin::media::entity as media;
use rustcms::audit::AuditContext;
use rustcms::models::user;
use rustcms::storage::StorageKey;
use rustcms_testing::{Factory, TestApp, TestClient, TestDb, UserFactory, test_config};
use sea_orm::prelude::{DateTimeUtc, Uuid};
use sea_orm::{ActiveModelTrait, ConnectionTrait, DatabaseConnection, Set};
use serde_json::{Value, json};

/// `APP_URL` in these tests.
pub const APP_URL: &str = "https://cms.test";

/// The application with its media in a fresh temporary directory and `APP_URL` set.
pub async fn app(db: TestDb) -> TestApp {
    let root = std::env::temp_dir().join(format!("rblog-content-{}", Uuid::now_v7()));
    let config = test_config(db.url())
        .with("storage.root", root.to_string_lossy().as_ref())
        .and_then(|c| c.with("app.url", format!("{APP_URL}/")))
        .expect("test configuration");
    TestApp::with_config(bootstrap::build, config, db).await
}

/// A client with a fresh Content API key.
pub async fn reader(app: &TestApp) -> TestClient<'_> {
    ensure_frontend_user(app.state()).await.unwrap();
    let issued = issue_content_key(app.state().db(), "test", None, &AuditContext::system(Some("test"))).await.unwrap();
    app.client().bearer(&issued.plain_text)
}

/// `hours` ago.
pub fn ago(hours: i64) -> DateTimeUtc {
    let now = rblog::services::content_sync::now();
    DateTimeUtc::from_timestamp_micros(now.timestamp_micros() - hours * 3_600_000_000).unwrap()
}

/// A user named `name`, with the author profile `slug` when given.
pub async fn author(db: &DatabaseConnection, name: &str, slug: Option<&str>) -> user::Model {
    let user = UserFactory::new().name(name).create(db).await;
    if let Some(slug) = slug {
        author_profile::ActiveModel {
            user_id: Set(user.id),
            slug: Set(slug.into()),
            bio: Set(Some(format!("About {name}"))),
            website: Set(Some("https://example.com".into())),
            avatar_media_id: Set(None),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
    }
    user
}

pub async fn category(db: &impl ConnectionTrait, slug: &str, name: &str) -> category::Model {
    category::ActiveModel { name: Set(name.into()), slug: Set(slug.into()), ..Default::default() }
        .insert(db)
        .await
        .unwrap()
}

pub async fn tag(db: &impl ConnectionTrait, slug: &str, name: &str) -> tag::Model {
    tag::ActiveModel { name: Set(name.into()), slug: Set(slug.into()), ..Default::default() }.insert(db).await.unwrap()
}

/// A post published `hours_ago` hours ago (tweak the fields before inserting).
pub fn post(author: &user::Model, slug: &str, title: &str, hours_ago: i64) -> post::ActiveModel {
    post::ActiveModel {
        title: Set(title.into()),
        slug: Set(slug.into()),
        author_id: Set(author.id),
        status: Set("published".into()),
        published_at: Set(Some(ago(hours_ago))),
        search_text: Set(Some(normalize(title))),
        reading_minutes: Set(3),
        ..Default::default()
    }
}

/// A page published `hours_ago` hours ago.
pub fn page(author: &user::Model, slug: &str, hours_ago: i64) -> page::ActiveModel {
    page::ActiveModel {
        title: Set(slug.to_uppercase()),
        slug: Set(slug.into()),
        author_id: Set(author.id),
        status: Set("published".into()),
        published_at: Set(Some(ago(hours_ago))),
        reading_minutes: Set(1),
        ..Default::default()
    }
}

/// Tags `post` with `tags`, in order.
pub async fn tag_post(db: &impl ConnectionTrait, post: &post::Model, tags: &[&tag::Model]) {
    for (position, tag) in tags.iter().enumerate() {
        post_tag::ActiveModel { post_id: Set(post.id), tag_id: Set(tag.id), position: Set(position as i32) }
            .insert(db)
            .await
            .unwrap();
    }
}

/// A rich-text body: a paragraph, then an image of each of `images`.
pub fn body(text: &str, images: &[Uuid]) -> Value {
    let mut content = vec![json!({"type": "paragraph", "content": [{"type": "text", "text": text}]})];
    content.extend(images.iter().map(|id| json!({"type": "image", "attrs": {"media": id.to_string(), "alt": "hình"}})));
    json!({"type": "doc", "version": 1, "content": content})
}

/// A media row of type `mime` named `name` whose file holds `content`, in the app's storage.
pub async fn stored_media(app: &TestApp, name: &str, mime: &str, content: &[u8]) -> media::Model {
    let storage = rustcms::storage::of(app.state()).unwrap();
    let ext = match mime {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "application/pdf" => "pdf",
        _ => "bin",
    };
    let key = StorageKey::generate(ext).unwrap();
    let file: PathBuf = std::env::temp_dir().join(format!("rblog-upload-{}", Uuid::now_v7()));
    std::fs::write(&file, content).unwrap();
    storage.put_file(&key, &file, mime).await.unwrap();
    std::fs::remove_file(&file).unwrap();
    media::ActiveModel {
        id: Set(Uuid::now_v7()),
        disk: Set(storage.disk().into()),
        key: Set(key.as_str().into()),
        name: Set(name.into()),
        mime: Set(mime.into()),
        size: Set(content.len() as i64),
        sha256: Set("0".repeat(64)),
        width: Set(Some(1600)),
        height: Set(Some(900)),
        owner_id: Set(1),
        created_at: Set(ago(0)),
        deleted_at: Set(None),
        alt: Set(None),
    }
    .insert(app.state().db())
    .await
    .unwrap()
}
