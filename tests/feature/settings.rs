//! The settings pages (`general`, `reading`, `seo`, `integrations`) through the admin API, and
//! the public settings the Content API will return.

use rblog::database::migrations::Migrator;
use rblog::services::site::public_settings;
use rustcms::axum::http::{Method, StatusCode};
use rustcms_testing::{Factory, TestApp, TestClient, TestDb, TestResponse, UserFactory, db_test};
use serde_json::{Value, json};

use crate::support;

const GROUPS: [&str; 4] = ["general", "reading", "seo", "integrations"];
const SECRET: &str = "whsec-0123456789-abcdef";

/// A signed-in super admin.
async fn admin(app: &TestApp) -> TestClient<'_> {
    let user = UserFactory::new().create(app.state().db()).await;
    app.make_super_admin(&user).await;
    // The page the homepage examples show.
    page(app, user.id, "gioi-thieu", false).await;
    app.acting_as(&user).await
}

/// A page `slug` by `author` (the homepage may show it), in the trash when `trashed`.
async fn page(app: &TestApp, author: i64, slug: &str, trashed: bool) {
    use sea_orm::{ActiveModelTrait, Set};
    let now: sea_orm::prelude::DateTimeUtc = std::time::SystemTime::now().into();
    rblog::models::page::ActiveModel {
        title: Set(slug.into()),
        slug: Set(slug.into()),
        author_id: Set(author),
        status: Set("draft".into()),
        reading_minutes: Set(0),
        created_at: Set(now),
        updated_at: Set(now),
        deleted_at: Set(trashed.then_some(now)),
        ..Default::default()
    }
    .insert(app.state().db())
    .await
    .unwrap();
}

/// `PUT settings/{group}` with `data`, at the version the page is at now.
async fn save(app: &TestApp, client: &TestClient<'_>, group: &str, data: Value) -> TestResponse {
    let uri = app.admin_api(&format!("settings/{group}"));
    let version = client.get(&uri).await.assert_status(StatusCode::OK).json()["version"].clone();
    client.send(Method::PUT, &uri, Some(&json!({ "version": version, "data": data }))).await
}

db_test!(super_admin_reads_and_updates_each_page, migrate = Migrator, |db: TestDb| async move {
    let app = support::app(db).await;
    let admin = admin(&app).await;

    // Never saved: the defaults.
    admin
        .get(&app.admin_api("settings/general"))
        .await
        .assert_status(StatusCode::OK)
        .assert_json("/version", json!("0"))
        .assert_json("/data/site_language", json!("vi"))
        .assert_json("/data/posts_per_page", json!(10));
    for group in GROUPS {
        admin.get(&app.admin_api(&format!("settings/{group}"))).await.assert_status(StatusCode::OK);
    }

    let pages = [
        (
            "general",
            json!({
                "site_name": "Blog của An",
                "description": "Ghi chép về Hà Nội",
                "site_language": "en",
                "frontend_url": "https://blog.example.com/",
                "posts_per_page": 20,
            }),
        ),
        ("reading", json!({ "homepage_shows": "page", "homepage_page_slug": "gioi-thieu" })),
        (
            "seo",
            json!({
                "meta_title": "Blog của An",
                "meta_description": "Ghi chép",
                "og_image_url": "https://cdn.example.com/og.png",
                "social_links": { "github": "https://github.com/an", "x": "https://x.com/an" },
            }),
        ),
        ("integrations", json!({ "webhook_url": "https://hooks.example.com/rblog", "preview_path": "/xem-truoc" })),
    ];
    for (group, data) in pages {
        let mut sent = data.clone();
        if group == "integrations" {
            sent["webhook_secret"] = json!(SECRET);
        }
        let saved = save(&app, &admin, group, sent).await;
        saved.assert_status(StatusCode::OK).assert_json("/version", json!("1"));
        let read = admin.get(&app.admin_api(&format!("settings/{group}"))).await;
        read.assert_status(StatusCode::OK).assert_json("/version", json!("1"));
        for (name, value) in data.as_object().unwrap() {
            read.assert_json(&format!("/data/{name}"), value.clone());
        }
    }

    // http is accepted for a public site on this machine.
    save(&app, &admin, "general", json!({ "frontend_url": "http://localhost:3000" }))
        .await
        .assert_status(StatusCode::OK)
        .assert_json("/data/frontend_url", json!("http://localhost:3000"));
});

db_test!(settings_refuse_invalid_values, migrate = Migrator, |db: TestDb| async move {
    let app = support::app(db).await;
    let admin = admin(&app).await;
    let author = UserFactory::new().create(app.state().db()).await;
    page(&app, author.id, "da-xoa", true).await;

    let refused = [
        ("general", json!({ "frontend_url": "http://blog.example.com" }), "frontend_url"),
        ("general", json!({ "frontend_url": "blog.example.com" }), "frontend_url"),
        ("general", json!({ "posts_per_page": 0 }), "posts_per_page"),
        ("general", json!({ "posts_per_page": 101 }), "posts_per_page"),
        ("general", json!({ "site_name": "  " }), "site_name"),
        ("general", json!({ "site_language": "fr" }), "site_language"),
        ("integrations", json!({ "webhook_url": "https://hooks.example.com/rblog" }), "webhook_secret"),
        ("integrations", json!({ "webhook_url": "http://hooks.example.com", "webhook_secret": SECRET }), "webhook_url"),
        ("integrations", json!({ "preview_path": "preview" }), "preview_path"),
        ("integrations", json!({ "preview_path": "//evil.example.com" }), "preview_path"),
        ("seo", json!({ "social_links": { "myspace": "https://myspace.com/an" } }), "social_links"),
        ("seo", json!({ "social_links": { "github": "http://github.com/an" } }), "social_links"),
        ("seo", json!({ "og_image_url": "http://cdn.example.com/og.png" }), "og_image_url"),
        ("reading", json!({ "homepage_shows": "page" }), "homepage_page_slug"),
        ("reading", json!({ "homepage_shows": "page", "homepage_page_slug": "  " }), "homepage_page_slug"),
        ("reading", json!({ "homepage_shows": "page", "homepage_page_slug": "Giới thiệu" }), "homepage_page_slug"),
        // No such page, and a page in the trash.
        ("reading", json!({ "homepage_shows": "page", "homepage_page_slug": "khong-co" }), "homepage_page_slug"),
        ("reading", json!({ "homepage_shows": "page", "homepage_page_slug": "da-xoa" }), "homepage_page_slug"),
    ];
    for (group, data, field) in refused {
        save(&app, &admin, group, data).await.assert_validation_error(field);
    }
    // Nothing was saved.
    for group in GROUPS {
        admin.get(&app.admin_api(&format!("settings/{group}"))).await.assert_json("/version", json!("0"));
    }

    // The message of the panel's language, the code `custom` for an application's own text,
    // and the framework's template (translated by the panel) for a range.
    let vi = admin.languages("vi");
    save(&app, &vi, "integrations", json!({ "webhook_url": "https://hooks.example.com/rblog" }))
        .await
        .assert_status(StatusCode::UNPROCESSABLE_ENTITY)
        .assert_json("/errors/webhook_secret/0", json!("không được để trống khi đã đặt URL webhook"));
    save(&app, &vi, "general", json!({ "posts_per_page": 0 }))
        .await
        .assert_json("/issues/posts_per_page/0", json!({ "code": "between", "params": { "max": "100" } }));
});

db_test!(webhook_secret_is_never_returned, migrate = Migrator, |db: TestDb| async move {
    let app = support::app(db).await;
    let admin = admin(&app).await;
    let uri = app.admin_api("settings/integrations");

    let data = json!({ "webhook_url": "https://hooks.example.com/rblog", "webhook_secret": SECRET });
    let saved = save(&app, &admin, "integrations", data).await;
    saved.assert_status(StatusCode::OK).assert_json("/secrets/webhook_secret", json!("set"));
    assert!(!saved.text().contains(SECRET), "{}", saved.text());
    let read = admin.get(&uri).await;
    read.assert_status(StatusCode::OK).assert_json("/secrets/webhook_secret", json!("set"));
    assert!(read.json()["data"].get("webhook_secret").is_none());
    assert!(!read.text().contains(SECRET), "{}", read.text());

    // Kept while the address stays; a new address needs the secret again.
    save(&app, &admin, "integrations", json!({ "preview_path": "/preview" }))
        .await
        .assert_status(StatusCode::OK)
        .assert_json("/secrets/webhook_secret", json!("set"));
    save(&app, &admin, "integrations", json!({ "webhook_url": "https://other.example.com/hook" }))
        .await
        .assert_validation_error("webhook_secret");
    // No address, no secret needed.
    save(&app, &admin, "integrations", json!({ "webhook_url": null }))
        .await
        .assert_status(StatusCode::OK)
        .assert_json("/secrets/webhook_secret", json!("unset"));

    // Nor in the audit log.
    let audit = admin.get(&app.admin_api("audit")).await;
    audit.assert_status(StatusCode::OK);
    assert!(audit.text().contains("settings.updated"), "{}", audit.text());
    assert!(!audit.text().contains(SECRET), "{}", audit.text());
});

db_test!(settings_need_their_permissions, migrate = Migrator, |db: TestDb| async move {
    let app = support::app(db).await;
    let user = UserFactory::new().create(app.state().db()).await;
    let editor = app.acting_as(&user).await;
    for group in GROUPS {
        let uri = app.admin_api(&format!("settings/{group}"));
        editor.get(&uri).await.assert_status(StatusCode::FORBIDDEN);
        editor
            .send(Method::PUT, &uri, Some(&json!({ "version": "0", "data": { "preview_path": "/x" } })))
            .await
            .assert_status(StatusCode::FORBIDDEN);
    }
    let panel = editor.get(&app.admin_api("panel")).await;
    panel.assert_status(StatusCode::OK).assert_json("/settings", json!([]));
});

db_test!(public_settings_leave_out_integrations, migrate = Migrator, |db: TestDb| async move {
    let app = support::app(db).await;
    let admin = admin(&app).await;

    // Defaults before anything is saved.
    let defaults = public_settings(app.state()).await.unwrap();
    assert_eq!(defaults.site_language, "vi");
    assert_eq!(defaults.posts_per_page, 10);
    assert_eq!(defaults.homepage.shows, "latest_posts");
    assert_eq!(defaults.homepage.page_slug, None);

    let general = json!({ "site_name": "Blog của An", "frontend_url": "https://blog.example.com/" });
    save(&app, &admin, "general", general).await.assert_status(StatusCode::OK);
    let seo = json!({ "social_links": { "github": "https://github.com/an" } });
    save(&app, &admin, "seo", seo).await.assert_status(StatusCode::OK);
    let reading = json!({ "homepage_shows": "page", "homepage_page_slug": "gioi-thieu" });
    save(&app, &admin, "reading", reading).await.assert_status(StatusCode::OK);
    let webhook = json!({ "webhook_url": "https://hooks.example.com/rblog", "webhook_secret": SECRET });
    save(&app, &admin, "integrations", webhook).await.assert_status(StatusCode::OK);

    let settings = public_settings(app.state()).await.unwrap();
    assert_eq!(settings.site_name, "Blog của An");
    assert_eq!(settings.frontend_url.as_deref(), Some("https://blog.example.com"));
    assert_eq!(settings.seo.social_links["github"], "https://github.com/an");
    assert_eq!(settings.homepage.page_slug.as_deref(), Some("gioi-thieu"));

    let wire = serde_json::to_string(&settings).unwrap();
    for hidden in [SECRET, "hooks.example.com", "webhook", "preview"] {
        assert!(!wire.contains(hidden), "`{hidden}` in {wire}");
    }
});

db_test!(a_local_webhook_needs_the_development_flag, migrate = Migrator, |db: TestDb| async move {
    let local = json!({ "webhook_url": "http://localhost:3000/api/revalidate", "webhook_secret": SECRET });
    let config = rustcms_testing::test_config(db.url()).with("webhook.allow_insecure_local", true).unwrap();
    let app = TestApp::with_config(rblog::bootstrap::build, config, db).await;
    let admin = admin(&app).await;
    save(&app, &admin, "integrations", local).await.assert_status(StatusCode::OK);
});

db_test!(a_local_webhook_is_refused_without_the_flag, migrate = Migrator, |db: TestDb| async move {
    let app = support::app(db).await;
    let admin = admin(&app).await;
    let local = json!({ "webhook_url": "http://localhost:3000/api/revalidate", "webhook_secret": SECRET });
    save(&app, &admin, "integrations", local).await.assert_status(StatusCode::UNPROCESSABLE_ENTITY);
});
