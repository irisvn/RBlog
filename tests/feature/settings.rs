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

/// The kinds of the outbox events about the settings, oldest first.
async fn settings_events(app: &TestApp) -> Vec<(String, Value)> {
    use rblog::models::content_event;
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder};
    content_event::Entity::find()
        .filter(content_event::Column::SubjectType.eq("settings"))
        .order_by_asc(content_event::Column::Id)
        .all(app.state().db())
        .await
        .unwrap()
        .into_iter()
        .map(|e| (e.kind, e.payload))
        .collect()
}

db_test!(saving_public_settings_tells_the_public_site, migrate = Migrator, |db: TestDb| async move {
    let app = support::app(db).await;
    let admin = admin(&app).await;
    let changes = [
        ("general", json!({ "site_name": "Blog của An" })),
        ("reading", json!({ "homepage_shows": "page", "homepage_page_slug": "gioi-thieu" })),
        ("seo", json!({ "meta_title": "Blog của An" })),
    ];
    let payload = json!({ "tags": ["settings"], "paths": { "old": [], "new": [] } });
    for (count, (group, data)) in changes.into_iter().enumerate() {
        save(&app, &admin, group, data.clone()).await.assert_status(StatusCode::OK);
        let events = settings_events(&app).await;
        assert_eq!(events.len(), count + 1, "{group}");
        assert_eq!(events[count], ("settings.updated".to_owned(), payload.clone()), "{group}");
        // A save that changes nothing records nothing.
        save(&app, &admin, group, data).await.assert_status(StatusCode::OK);
        assert_eq!(settings_events(&app).await.len(), count + 1, "{group} unchanged");
    }
    // Integrations are not public: no event.
    save(&app, &admin, "integrations", json!({ "preview_path": "/xem-truoc" })).await.assert_status(StatusCode::OK);
    assert_eq!(settings_events(&app).await.len(), 3);
});

db_test!(the_logo_and_share_image_are_public_media, migrate = Migrator, |db: TestDb| async move {
    use crate::support::content::{self, APP_URL};
    use rblog::models::public_media;
    use rblog::services::public_media::backfill;
    use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};

    let app = content::app(db).await;
    let db = app.state().db();
    let admin = admin(&app).await;
    let logo = content::stored_media(&app, "logo.png", "image/png", b"\x89PNG logo").await;
    let og = content::stored_media(&app, "chia se.jpg", "image/jpeg", b"\xff\xd8\xff og").await;
    let pdf = content::stored_media(&app, "menu.pdf", "application/pdf", b"%PDF-1.7").await;
    let mut described: rustcms::admin::media::entity::ActiveModel = og.clone().into();
    described.alt = Set(Some("Hồ Gươm lúc bình minh".into()));
    let og = described.update(db).await.unwrap();
    let public = app.client();
    let served = async |id: &sea_orm::prelude::Uuid| public.get(&format!("/api/v1/media/{id}/a.png")).await.status;
    assert_eq!(served(&logo.id).await, StatusCode::NOT_FOUND);

    // A PDF is neither a logo nor a share image.
    save(&app, &admin, "general", json!({ "logo": pdf.id.to_string() })).await.assert_validation_error("logo");
    save(&app, &admin, "seo", json!({ "og_image": pdf.id.to_string() })).await.assert_validation_error("og_image");

    // Before any image is chosen, an address saved by RBlog 0.1 is the share image.
    save(&app, &admin, "seo", json!({ "og_image_url": "https://cdn.example.com/og.png" }))
        .await
        .assert_status(StatusCode::OK);
    let settings = serde_json::to_value(public_settings(app.state()).await.unwrap()).unwrap();
    assert_eq!(settings["logo"], json!(null));
    assert_eq!(
        settings["seo"]["og_image"],
        json!({ "url": "https://cdn.example.com/og.png", "width": null, "height": null, "alt": "" })
    );

    save(&app, &admin, "general", json!({ "logo": logo.id.to_string() })).await.assert_status(StatusCode::OK);
    save(&app, &admin, "seo", json!({ "og_image": og.id.to_string() })).await.assert_status(StatusCode::OK);
    let settings = serde_json::to_value(public_settings(app.state()).await.unwrap()).unwrap();
    assert_eq!(
        settings["logo"],
        json!({ "url": format!("{APP_URL}/api/v1/media/{}/logo.png", logo.id), "width": 1600, "height": 900, "alt": "" })
    );
    // The chosen image wins over the old address, with its alternative text.
    assert_eq!(
        settings["seo"]["og_image"],
        json!({
            "url": format!("{APP_URL}/api/v1/media/{}/chia-se.jpg", og.id),
            "width": 1600,
            "height": 900,
            "alt": "Hồ Gươm lúc bình minh",
        })
    );
    assert!(settings["seo"].get("og_image_url").is_none(), "{settings}");
    // Served publicly, also through the Content API.
    assert_eq!(served(&logo.id).await, StatusCode::OK);
    assert_eq!(served(&og.id).await, StatusCode::OK);
    let reader = content::reader(&app).await;
    let api = reader.get("/api/v1/content/settings").await;
    api.assert_status(StatusCode::OK).assert_json("/logo/url", settings["logo"]["url"].clone());

    // A replaced or cleared image is private again.
    let other = content::stored_media(&app, "logo-2.webp", "image/webp", b"RIFF....WEBP").await;
    save(&app, &admin, "general", json!({ "logo": other.id.to_string() })).await.assert_status(StatusCode::OK);
    assert_eq!(served(&logo.id).await, StatusCode::NOT_FOUND);
    save(&app, &admin, "general", json!({ "logo": null })).await.assert_status(StatusCode::OK);
    assert_eq!(served(&other.id).await, StatusCode::NOT_FOUND);
    assert_eq!(serde_json::to_value(public_settings(app.state()).await.unwrap()).unwrap()["logo"], json!(null));

    // The backfill rebuilds the rows of the settings too.
    public_media::Entity::delete_many().exec(db).await.unwrap();
    assert_eq!(served(&og.id).await, StatusCode::NOT_FOUND);
    backfill(db, app.state().keyring()).await.unwrap();
    assert_eq!(served(&og.id).await, StatusCode::OK);
    let rows =
        public_media::Entity::find().filter(public_media::Column::SubjectType.eq("settings")).all(db).await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!((rows[0].media_id, rows[0].subject_id), (og.id, 2));
});
