//! `db:seed --class demo`: the demo content (every post status, consistent links), a second run
//! that adds nothing, and a refusal in production.

use rblog::database::migrations::Migrator;
use rblog::database::seeders::DatabaseSeeder;
use rblog::database::seeders::demo_seeder::{AUTHORS, DemoSeeder, REFUSED_IN_PRODUCTION};
use rblog::models::{
    author_profile, category, content_event, menu, menu_item, page, post, post_tag, public_media, slug_history, tag,
};
use rblog::services::content_sync;
use rustcms::admin::rich_text;
use rustcms::database::Seeder;
use rustcms::models::audit_log;
use rustcms::models::user;
use rustcms_testing::{TestApp, TestDb, db_test, test_config};
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter};

use crate::support;

/// Rows of every table the seeder writes.
async fn counts(db: &DatabaseConnection) -> Vec<(&'static str, u64)> {
    vec![
        ("users", user::Entity::find().count(db).await.unwrap()),
        ("author_profiles", author_profile::Entity::find().count(db).await.unwrap()),
        ("categories", category::Entity::find().count(db).await.unwrap()),
        ("tags", tag::Entity::find().count(db).await.unwrap()),
        ("posts", post::Entity::find().count(db).await.unwrap()),
        ("post_tag", post_tag::Entity::find().count(db).await.unwrap()),
        ("pages", page::Entity::find().count(db).await.unwrap()),
        ("menus", menu::Entity::find().count(db).await.unwrap()),
        ("menu_items", menu_item::Entity::find().count(db).await.unwrap()),
        ("slug_history", slug_history::Entity::find().count(db).await.unwrap()),
        ("content_events", content_event::Entity::find().count(db).await.unwrap()),
        ("public_media", public_media::Entity::find().count(db).await.unwrap()),
        ("media", rustcms::admin::media::entity::Entity::find().count(db).await.unwrap()),
        ("audit_logs", audit_log::Entity::find().count(db).await.unwrap()),
    ]
}

/// Posts of `status`, live (`trashed` false) or in the trash.
async fn posts(db: &DatabaseConnection, status: &str, trashed: bool) -> Vec<post::Model> {
    let query = post::Entity::find().filter(post::Column::Status.eq(status));
    let query = if trashed {
        query.filter(post::Column::DeletedAt.is_not_null())
    } else {
        query.filter(post::Column::DeletedAt.is_null())
    };
    query.all(db).await.unwrap()
}

db_test!(the_demo_content_covers_every_status, migrate = Migrator, |db: TestDb| async move {
    let app = support::content::app(db).await;
    DemoSeeder.run(app.state()).await.unwrap();
    let db = app.state().db();
    let now = content_sync::now();

    for (name, email, ..) in AUTHORS {
        let user = user::Entity::find().filter(user::Column::Email.eq(email)).one(db).await.unwrap().unwrap();
        assert_eq!(user.name, name);
        assert!(author_profile::Entity::find_by_id(user.id).one(db).await.unwrap().is_some(), "{email}");
    }
    assert_eq!(category::Entity::find().count(db).await.unwrap(), 4);
    assert_eq!(tag::Entity::find().count(db).await.unwrap(), 8);
    assert_eq!(post::Entity::find().count(db).await.unwrap(), 20);

    // Published at different past dates; live means published, due and not trashed.
    let published = posts(db, "published", false).await;
    assert_eq!(published.len(), 9);
    assert!(published.iter().all(|p| p.is_public_at(now)));
    let dates: std::collections::BTreeSet<_> = published.iter().map(|p| p.published_at.unwrap().date_naive()).collect();
    assert_eq!(dates.len(), 9, "different days");
    assert_eq!(post::Entity::find().filter(post::public_at(now)).count(db).await.unwrap(), 9);
    // Scheduled in the future; pending and drafts not dated.
    let scheduled = posts(db, "scheduled", false).await;
    assert_eq!(scheduled.len(), 3);
    assert!(scheduled.iter().all(|p| p.published_at.is_some_and(|at| at > now)));
    for status in ["pending", "draft"] {
        let found = posts(db, status, false).await;
        assert_eq!(found.len(), 3, "{status}");
        assert!(found.iter().all(|p| p.published_at.is_none()), "{status}");
    }
    // Two in the trash, one of them once published: neither is live.
    let trashed = post::Entity::find().filter(post::Column::DeletedAt.is_not_null()).all(db).await.unwrap();
    assert_eq!(trashed.len(), 2);
    assert!(trashed.iter().all(|p| !p.is_public_at(now)));
    assert_eq!(posts(db, "published", true).await.len(), 1);

    // Every post: a category, a valid rich-text body, search text; edited after it was written.
    for p in post::Entity::find().all(db).await.unwrap() {
        assert!(p.category_id.is_some(), "{}", p.slug);
        rich_text::parse(p.body.as_ref().unwrap()).unwrap();
        assert!(p.search_text.as_deref().is_some_and(|s| s.is_ascii()), "{}", p.slug);
        assert!(p.updated_at >= p.created_at, "{}", p.slug);
        assert!(p.updated_at <= now, "{}", p.slug);
    }

    // Pages and menus linking categories and pages that exist.
    let pages = page::Entity::find().all(db).await.unwrap();
    let mut slugs: Vec<&str> = pages.iter().map(|p| p.slug.as_str()).collect();
    slugs.sort_unstable();
    assert_eq!(slugs, ["gioi-thieu", "lien-he"]);
    assert!(pages.iter().all(|p| p.is_public_at(now)));
    for key in ["primary", "footer"] {
        let menu = menu::Entity::find().filter(menu::Column::Key.eq(key)).one(db).await.unwrap().unwrap();
        let items = menu_item::Entity::find().filter(menu_item::Column::MenuId.eq(menu.id)).all(db).await.unwrap();
        assert!(items.len() >= 3, "{key}");
        for item in items {
            let target = item.target_id.unwrap();
            let exists = match item.kind.as_str() {
                "category" => category::Entity::find_by_id(target).one(db).await.unwrap().is_some(),
                "page" => page::Entity::find_by_id(target).one(db).await.unwrap().is_some(),
                other => panic!("unexpected link {other}"),
            };
            assert!(exists, "{key}: {}", item.label);
        }
    }

    // The first posts have covers: images of the library, with their alternative text, public
    // while their post is (the Content API's `cover.alt`).
    use rblog::database::seeders::demo_images::IMAGES;
    use rustcms::admin::media::entity as media;
    let library = media::Entity::find().all(db).await.unwrap();
    assert_eq!(library.len(), IMAGES.len());
    for (name, alt, ..) in IMAGES {
        let image = library.iter().find(|m| m.name == name).expect(name);
        assert_eq!(image.alt.as_deref(), Some(alt), "{name}");
        assert_eq!((image.mime.as_str(), image.width, image.height), ("image/png", Some(320), Some(180)), "{name}");
    }
    let audited = audit_log::Entity::find().all(db).await.unwrap();
    let media_created = audited.iter().filter(|e| e.event == "created" && e.subject_type == "media").count();
    assert_eq!(media_created, IMAGES.len(), "each demo image is audited");
    let covered = post::Entity::find().filter(post::Column::CoverMediaId.is_not_null()).all(db).await.unwrap();
    assert_eq!(covered.len(), IMAGES.len());
    let reader = support::content::reader(&app).await;
    let first = reader.get("/api/v1/content/posts/mot-ngay-lang-thang-pho-co-ha-noi").await;
    first.assert_status(rustcms::axum::http::StatusCode::OK);
    assert_eq!(first.json()["cover"]["alt"], serde_json::json!(IMAGES[0].1));
    let file = app
        .client()
        .get(first.json()["cover"]["url"].as_str().unwrap().trim_start_matches(support::content::APP_URL))
        .await;
    file.assert_status(rustcms::axum::http::StatusCode::OK);
    assert_eq!(&file.body[..8], b"\x89PNG\r\n\x1a\n");

    // The outbox heard about what it covers on main: authors, categories, tags, menus.
    let kinds: Vec<String> = content_event::Entity::find().all(db).await.unwrap().into_iter().map(|e| e.kind).collect();
    for (kind, expected) in
        [("author.created", 3), ("category.created", 4), ("tag.created", 8), ("menu.created", 2), ("menu.updated", 2)]
    {
        assert_eq!(kinds.iter().filter(|k| *k == kind).count(), expected, "{kind}: {kinds:?}");
    }
});

db_test!(seeding_twice_adds_nothing, migrate = Migrator, |db: TestDb| async move {
    let app = support::content::app(db).await;
    let db = app.state().db();
    // Not part of `db:seed`.
    DatabaseSeeder.run(app.state()).await.unwrap();
    assert_eq!(post::Entity::find().count(db).await.unwrap(), 0);

    DemoSeeder.run(app.state()).await.unwrap();
    let before = counts(db).await;
    DemoSeeder.run(app.state()).await.unwrap();
    assert_eq!(counts(db).await, before);
});

db_test!(the_demo_seeder_refuses_in_production, migrate = Migrator, |db: TestDb| async move {
    let config = test_config(db.url()).with("app.env", "production").unwrap();
    let app = TestApp::with_config(rblog::bootstrap::build, config, db).await;
    let before = counts(app.state().db()).await;
    let refused = DemoSeeder.run(app.state()).await.unwrap_err();
    assert!(format!("{refused:#}").contains(REFUSED_IN_PRODUCTION), "{refused:#}");
    assert_eq!(counts(app.state().db()).await, before, "nothing written");
});
