//! The read-only Content API (`/api/v1/content`, docs/api.md): access, published content only,
//! order and filters, redirects, counts, menus and the sitemap.

use rblog::database::migrations::Migrator;
use rblog::database::seeders::content_api_seeder::ContentApiSeeder;
use rblog::models::{menu, menu_item, post, slug_history};
use rblog::services::content_api::{FRONTEND_EMAIL, PERMISSION, frontend_user};
use rustcms::auth::{ADMIN_GUARD, API_GUARD, AuthState};
use rustcms::axum::http::StatusCode;
use rustcms::database::Seeder;
use rustcms_testing::{Factory, TestClient, TestDb, UserFactory, db_test};
use sea_orm::{ActiveModelTrait, EntityTrait, QueryFilter, Set};
use serde_json::{Value, json};

use crate::support::content::{self, ago, author, category, page, post, tag, tag_post};

/// Every Content API route, with a path that exists in [`access_is_content_read_only`].
const ROUTES: [&str; 13] = [
    "/api/v1/content/posts",
    "/api/v1/content/posts/hello",
    "/api/v1/content/pages",
    "/api/v1/content/pages/about",
    "/api/v1/content/categories",
    "/api/v1/content/categories/news",
    "/api/v1/content/tags",
    "/api/v1/content/tags/rust",
    "/api/v1/content/authors/an",
    "/api/v1/content/menus/primary",
    "/api/v1/content/settings",
    "/api/v1/content/sitemap?type=post",
    "/api/v1/content/posts?q=hello",
];

/// The slugs of a list answer, in order.
fn slugs(list: &Value) -> Vec<String> {
    list["data"].as_array().unwrap().iter().map(|p| p["slug"].as_str().unwrap().to_owned()).collect()
}

async fn list(client: &TestClient<'_>, uri: &str) -> Vec<String> {
    let response = client.get(uri).await;
    response.assert_status(StatusCode::OK);
    slugs(&response.json())
}

db_test!(access_is_content_read_only, migrate = Migrator, |db: TestDb| async move {
    let app = content::app(db).await;
    let db = app.state().db();
    let an = author(db, "An", Some("an")).await;
    let news = category(db, "news", "News").await;
    let rust = tag(db, "rust", "Rust").await;
    let hello = post(&an, "hello", "Hello", 1).insert(db).await.unwrap();
    let mut with_category: post::ActiveModel = hello.clone().into();
    with_category.category_id = Set(Some(news.id));
    let hello = with_category.update(db).await.unwrap();
    tag_post(db, &hello, &[&rust]).await;
    page(&an, "about", 1).insert(db).await.unwrap();
    menu::ActiveModel { key: Set("primary".into()), name: Set("Primary".into()), ..Default::default() }
        .insert(db)
        .await
        .unwrap();

    // The seeder runs again safely; the frontend user has the API role only.
    ContentApiSeeder.run(app.state()).await.unwrap();
    let reader = content::reader(&app).await;
    ContentApiSeeder.run(app.state()).await.unwrap();
    let frontend = frontend_user(db).await.unwrap().expect("frontend user");
    assert_eq!(frontend.email, FRONTEND_EMAIL);
    let auth = app.state().service::<AuthState>().unwrap();
    let api = auth.resolver.grants(db, API_GUARD, frontend.id).await.unwrap();
    assert_eq!(api.permissions.iter().collect::<Vec<_>>(), [PERMISSION]);
    assert!(!api.super_admin);
    let admin = auth.resolver.grants(db, ADMIN_GUARD, frontend.id).await.unwrap();
    assert!(admin.permissions.is_empty() && !admin.super_admin, "no admin access: {admin:?}");

    // Another user's token, even with every ability: no `content.read` on the API guard.
    let stranger = UserFactory::new().create(db).await;
    let stranger = app.with_token(&stranger, &["*"]).await;
    // The frontend user's token without the ability.
    let narrowed = app.with_token(&frontend, &["posts.view_any"]).await;
    // An admin session is not an API credential.
    let editor = UserFactory::new().create(db).await;
    app.make_super_admin(&editor).await;
    let session = app.acting_as(&editor).await;
    for uri in ROUTES {
        reader.get(uri).await.assert_status(StatusCode::OK);
        app.client().get(uri).await.assert_status(StatusCode::UNAUTHORIZED);
        app.client().bearer("rcms_1_nope").get(uri).await.assert_status(StatusCode::UNAUTHORIZED);
        session.get(uri).await.assert_status(StatusCode::UNAUTHORIZED);
        stranger.get(uri).await.assert_status(StatusCode::FORBIDDEN);
        narrowed.get(uri).await.assert_status(StatusCode::FORBIDDEN);
    }

    // Settings: the public values only.
    let settings = reader.get("/api/v1/content/settings").await.json();
    assert!(settings["site_name"].is_string() && settings["seo"].is_object(), "{settings}");
    assert!(!settings.to_string().contains("webhook"), "{settings}");
});

db_test!(lists_published_posts_newest_first_with_filters, migrate = Migrator, |db: TestDb| async move {
    let app = content::app(db).await;
    let db = app.state().db();
    let an = author(db, "An", Some("an")).await;
    let binh = author(db, "Bình", Some("binh")).await;
    let news = category(db, "news", "News").await;
    let travel = category(db, "du-lich", "Du lịch").await;
    let rust = tag(db, "rust", "Rust").await;
    let web = tag(db, "web", "Web").await;

    // Two posts at the same instant: the higher id first.
    let same = ago(3);
    let mut first = post(&an, "same-1", "Same one", 0);
    first.published_at = Set(Some(same));
    first.category_id = Set(Some(news.id));
    let first = first.insert(db).await.unwrap();
    let mut second = post(&binh, "same-2", "Same two", 0);
    second.published_at = Set(Some(same));
    second.featured = Set(true);
    let second = second.insert(db).await.unwrap();
    let mut viet = post(&an, "xin-chao-viet-nam", "Xin chào Việt Nam", 1);
    viet.category_id = Set(Some(travel.id));
    let viet = viet.insert(db).await.unwrap();
    // Several tags: listed once.
    tag_post(db, &viet, &[&rust, &web]).await;
    tag_post(db, &first, &[&rust]).await;
    let old = post(&binh, "old", "Old news 100%", 48).insert(db).await.unwrap();

    // Never listed.
    for (slug, status, hours_ago, trashed) in [
        ("draft", "draft", 1, false),
        ("pending", "pending", 1, false),
        ("scheduled", "scheduled", -2, false),
        ("not-due", "published", -2, false),
        ("trashed", "published", 1, true),
        ("no-date", "published", 0, false),
    ] {
        let mut hidden = post(&an, slug, "Xin chào Việt Nam", hours_ago);
        hidden.status = Set(status.into());
        hidden.featured = Set(true);
        hidden.category_id = Set(Some(travel.id));
        if slug == "no-date" {
            hidden.published_at = Set(None);
        }
        if trashed {
            hidden.deleted_at = Set(Some(ago(0)));
        }
        let hidden = hidden.insert(db).await.unwrap();
        tag_post(db, &hidden, &[&rust]).await;
    }

    let reader = content::reader(&app).await;
    let all = reader.get("/api/v1/content/posts").await;
    all.assert_status(StatusCode::OK)
        .assert_json("/meta/total", json!(4))
        .assert_json("/meta/perPage", json!(20))
        .assert_json("/meta/lastPage", json!(1));
    assert_eq!(slugs(&all.json()), ["xin-chao-viet-nam", "same-2", "same-1", "old"]);
    let item = &all.json()["data"][0];
    assert_eq!(item["id"], json!(viet.id.to_string()));
    assert_eq!(item["category"], json!({"slug": "du-lich", "name": "Du lịch"}));
    assert_eq!(item["tags"], json!([{"slug": "rust", "name": "Rust"}, {"slug": "web", "name": "Web"}]));
    assert_eq!(item["author"], json!({"slug": "an", "name": "An"}));
    assert_eq!(item["reading_minutes"], json!(3));
    assert!(item["published_at"].as_str().unwrap().ends_with('Z'));
    assert!(item.get("html").is_none(), "lists carry no html");

    // Stable pages, equal instants included.
    let mut paged = Vec::new();
    for n in 1..=4 {
        let response = reader.get(&format!("/api/v1/content/posts?per_page=1&page={n}")).await;
        response.assert_json("/meta/lastPage", json!(4));
        paged.extend(slugs(&response.json()));
    }
    assert_eq!(paged, ["xin-chao-viet-nam", "same-2", "same-1", "old"]);
    reader.get("/api/v1/content/posts?per_page=101").await.assert_validation_error("per_page");

    // Filters.
    assert_eq!(list(&reader, "/api/v1/content/posts?category=news").await, ["same-1"]);
    assert_eq!(list(&reader, "/api/v1/content/posts?category=du-lich").await, ["xin-chao-viet-nam"]);
    assert_eq!(list(&reader, "/api/v1/content/posts?tag=rust").await, ["xin-chao-viet-nam", "same-1"]);
    assert_eq!(list(&reader, "/api/v1/content/posts?tag=web").await, ["xin-chao-viet-nam"]);
    assert!(list(&reader, "/api/v1/content/posts?tag=nope").await.is_empty());
    assert_eq!(list(&reader, "/api/v1/content/posts?author=binh").await, ["same-2", "old"]);
    assert!(list(&reader, "/api/v1/content/posts?author=nobody").await.is_empty());
    assert_eq!(list(&reader, "/api/v1/content/posts?featured=true").await, ["same-2"]);
    assert_eq!(list(&reader, "/api/v1/content/posts?featured=false").await.len(), 3);
    assert_eq!(list(&reader, "/api/v1/content/posts?q=viet").await, ["xin-chao-viet-nam"]);
    assert_eq!(list(&reader, "/api/v1/content/posts?q=Vi%E1%BB%87t").await, ["xin-chao-viet-nam"]);
    assert_eq!(list(&reader, "/api/v1/content/posts?q=CH%C3%80O%20viet").await, ["xin-chao-viet-nam"]);
    // `%` is a literal, not a wildcard.
    assert_eq!(list(&reader, "/api/v1/content/posts?q=100%25").await, ["old"]);
    assert!(list(&reader, "/api/v1/content/posts?q=1%25%25").await.is_empty());
    assert_eq!(list(&reader, "/api/v1/content/posts?tag=rust&category=news&author=an").await, ["same-1"]);
    let _ = (second, old);

    reader.get("/api/v1/content/posts?featured=maybe").await.assert_validation_error("featured");
    reader.get(&format!("/api/v1/content/posts?q={}", "a".repeat(101))).await.assert_validation_error("q");
    reader.get("/api/v1/content/posts?tag=rust&tag=web").await.assert_validation_error("tag");
});

db_test!(single_records_redirect_old_slugs, migrate = Migrator, |db: TestDb| async move {
    let app = content::app(db).await;
    let db = app.state().db();
    let an = author(db, "An", None).await;
    let live = post(&an, "new-slug", "Live", 1).insert(db).await.unwrap();
    let mut draft = post(&an, "draft", "Draft", 1);
    draft.status = Set("draft".into());
    let draft = draft.insert(db).await.unwrap();
    let about = page(&an, "about-us", 1).insert(db).await.unwrap();
    let mut hidden_page = page(&an, "secret", 1);
    hidden_page.deleted_at = Set(Some(ago(0)));
    hidden_page.insert(db).await.unwrap();
    let news = category(db, "news", "News").await;
    for (kind, id, old) in [
        ("post", live.id, "old-slug"),
        ("post", draft.id, "old-draft"),
        ("page", about.id, "about"),
        ("category", news.id, "tin-tuc"),
    ] {
        slug_history::ActiveModel {
            subject_type: Set(kind.into()),
            subject_id: Set(id),
            old_slug: Set(old.into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
    }

    let reader = content::reader(&app).await;
    let shown = reader.get("/api/v1/content/posts/new-slug").await;
    shown.assert_status(StatusCode::OK).assert_json("/slug", json!("new-slug")).assert_json("/html", json!(""));
    let shown = shown.json();
    for key in ["cover", "category", "published_at", "updated_at", "seo", "author", "excerpt"] {
        assert!(shown.get(key).is_some(), "{key} in {shown}");
    }
    assert_eq!(shown["author"], json!({"slug": null, "name": "An"}));
    assert_eq!(
        shown["seo"],
        json!({"meta_title": null, "meta_description": null, "canonical_url": null, "og_image": null})
    );
    assert!(shown.get("document").is_none());
    reader
        .get("/api/v1/content/posts/new-slug?include=document")
        .await
        .assert_status(StatusCode::OK)
        .assert_json("/document", json!(null));
    reader.get("/api/v1/content/posts/new-slug?include=everything").await.assert_validation_error("include");

    reader
        .get("/api/v1/content/posts/old-slug")
        .await
        .assert_status(StatusCode::OK)
        .assert_json("", json!({"redirect": "new-slug"}));
    // The old slug of a draft does not reveal it.
    reader.get("/api/v1/content/posts/old-draft").await.assert_status(StatusCode::NOT_FOUND);
    reader.get("/api/v1/content/posts/draft").await.assert_status(StatusCode::NOT_FOUND);
    reader.get("/api/v1/content/posts/nothing").await.assert_status(StatusCode::NOT_FOUND);

    reader
        .get("/api/v1/content/pages/about-us")
        .await
        .assert_status(StatusCode::OK)
        .assert_json("/title", json!("ABOUT-US"));
    reader.get("/api/v1/content/pages/about").await.assert_json("", json!({"redirect": "about-us"}));
    reader.get("/api/v1/content/pages/secret").await.assert_status(StatusCode::NOT_FOUND);
    let pages = reader.get("/api/v1/content/pages").await;
    assert_eq!(slugs(&pages.json()), ["about-us"]);
    assert!(pages.json()["data"][0].get("tags").is_none(), "pages have no tags");

    reader.get("/api/v1/content/categories/tin-tuc").await.assert_json("", json!({"redirect": "news"}));
    reader.get("/api/v1/content/categories/nothing").await.assert_status(StatusCode::NOT_FOUND);
    reader.get("/api/v1/content/tags/nothing").await.assert_status(StatusCode::NOT_FOUND);
});

db_test!(counts_authors_and_menus_follow_published_posts, migrate = Migrator, |db: TestDb| async move {
    let app = content::app(db).await;
    let db = app.state().db();
    let an = author(db, "An", Some("an")).await;
    let drafter = author(db, "Drafter", Some("drafter")).await;
    let news = category(db, "news", "News").await;
    let empty = category(db, "empty", "Empty").await;
    let rust = tag(db, "rust", "Rust").await;
    let web = tag(db, "web", "Web").await;
    let mut live = post(&an, "live", "Live", 1);
    live.category_id = Set(Some(news.id));
    let live = live.insert(db).await.unwrap();
    tag_post(db, &live, &[&rust, &web]).await;
    let mut hidden = post(&drafter, "hidden", "Hidden", 1);
    hidden.category_id = Set(Some(news.id));
    hidden.status = Set("draft".into());
    let hidden = hidden.insert(db).await.unwrap();
    tag_post(db, &hidden, &[&rust]).await;
    let mut trashed = post(&an, "trashed", "Trashed", 1);
    trashed.category_id = Set(Some(news.id));
    trashed.deleted_at = Set(Some(ago(0)));
    let trashed = trashed.insert(db).await.unwrap();
    tag_post(db, &trashed, &[&rust]).await;
    let about = page(&an, "about", 1).insert(db).await.unwrap();
    let mut draft_page = page(&an, "draft-page", 1);
    draft_page.status = Set("pending".into());
    let draft_page = draft_page.insert(db).await.unwrap();

    let reader = content::reader(&app).await;
    let categories = reader.get("/api/v1/content/categories").await.json();
    assert_eq!(
        categories["data"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| (c["slug"].clone(), c["post_count"].clone()))
            .collect::<Vec<_>>(),
        [(json!("empty"), json!(0)), (json!("news"), json!(1))]
    );
    reader
        .get("/api/v1/content/categories/news")
        .await
        .assert_json("/post_count", json!(1))
        .assert_json("/id", json!(news.id.to_string()));
    reader.get("/api/v1/content/categories/empty").await.assert_json("/post_count", json!(0));
    reader.get("/api/v1/content/tags/rust").await.assert_json("/post_count", json!(1));
    reader.get("/api/v1/content/tags/web").await.assert_json("/post_count", json!(1));
    assert_eq!(reader.get("/api/v1/content/tags").await.json()["meta"]["total"], json!(2));
    let _ = empty;

    // Authors: only with a published post; never their email.
    let profile = reader.get("/api/v1/content/authors/an").await;
    profile.assert_status(StatusCode::OK).assert_json(
        "",
        json!({"slug": "an", "name": "An", "bio": "About An", "website": "https://example.com", "avatar": null}),
    );
    assert!(!profile.text().contains(&an.email));
    reader.get("/api/v1/content/authors/drafter").await.assert_status(StatusCode::NOT_FOUND);
    reader.get("/api/v1/content/authors/nobody").await.assert_status(StatusCode::NOT_FOUND);

    // Menus: resolved paths, hidden targets left out with their children.
    let primary = menu::ActiveModel { key: Set("primary".into()), name: Set("Primary".into()), ..Default::default() }
        .insert(db)
        .await
        .unwrap();
    let item = |label: &str, kind: &str, target: Option<i64>, url: Option<&str>, parent: Option<i64>, position: i32| {
        menu_item::ActiveModel {
            menu_id: Set(primary.id),
            parent_id: Set(parent),
            label: Set(label.into()),
            kind: Set(kind.into()),
            target_id: Set(target),
            url: Set(url.map(str::to_owned)),
            new_tab: Set(kind == "url"),
            position: Set(position),
            ..Default::default()
        }
    };
    let home = item("Home", "url", None, Some("/"), None, 0).insert(db).await.unwrap();
    item("Live", "post", Some(live.id), None, Some(home.id), 0).insert(db).await.unwrap();
    item("Hidden", "post", Some(hidden.id), None, Some(home.id), 1).insert(db).await.unwrap();
    item("News", "category", Some(news.id), None, None, 1).insert(db).await.unwrap();
    item("Rust", "tag", Some(rust.id), None, None, 2).insert(db).await.unwrap();
    item("About", "page", Some(about.id), None, None, 3).insert(db).await.unwrap();
    let gone = item("Draft page", "page", Some(draft_page.id), None, None, 4).insert(db).await.unwrap();
    item("Under a hidden item", "url", None, Some("https://example.com"), Some(gone.id), 0).insert(db).await.unwrap();
    item("Trashed", "post", Some(trashed.id), None, None, 5).insert(db).await.unwrap();
    item("Deleted", "tag", Some(999_999), None, None, 6).insert(db).await.unwrap();
    item("GitHub", "url", None, Some("https://github.com"), None, 7).insert(db).await.unwrap();

    let menu = reader.get("/api/v1/content/menus/primary").await;
    menu.assert_status(StatusCode::OK).assert_json("/key", json!("primary"));
    let items = menu.json()["items"].clone();
    let summary: Vec<(String, String)> = items
        .as_array()
        .unwrap()
        .iter()
        .map(|i| (i["label"].as_str().unwrap().to_owned(), i["href"].as_str().unwrap().to_owned()))
        .collect();
    assert_eq!(
        summary,
        [
            ("Home".to_owned(), "/".to_owned()),
            ("News".to_owned(), "/categories/news".to_owned()),
            ("Rust".to_owned(), "/tags/rust".to_owned()),
            ("About".to_owned(), "/about".to_owned()),
            ("GitHub".to_owned(), "https://github.com".to_owned()),
        ]
    );
    assert_eq!(
        items[0]["children"],
        json!([{"id": items[0]["children"][0]["id"], "label": "Live", "href": "/posts/live", "new_tab": false}])
    );
    assert_eq!(items[0]["new_tab"], json!(true));
    reader.get("/api/v1/content/menus/footer").await.assert_status(StatusCode::NOT_FOUND);
});

db_test!(sitemap_cursor_walks_every_public_row, migrate = Migrator, |db: TestDb| async move {
    let app = content::app(db).await;
    let db = app.state().db();
    let an = author(db, "An", None).await;
    let news = category(db, "news", "News").await;
    let unused = category(db, "unused", "Unused").await;
    let rust = tag(db, "rust", "Rust").await;
    let draft_only = tag(db, "draft-only", "Draft only").await;
    // 1,001 public posts and a few hidden ones, in one statement.
    let mut rows: Vec<post::ActiveModel> = (0..1001)
        .map(|n| {
            let mut p = post(&an, &format!("post-{n}"), "Post", 1);
            p.category_id = Set(Some(news.id));
            p
        })
        .collect();
    for n in 0..3 {
        let mut hidden = post(&an, &format!("hidden-{n}"), "Hidden", 1);
        hidden.status = Set("draft".into());
        hidden.category_id = Set(Some(unused.id));
        rows.insert(500 + n, hidden);
    }
    post::Entity::insert_many(rows).exec(db).await.unwrap();
    let first = post::Entity::find()
        .filter(sea_orm::ColumnTrait::eq(&post::Column::Slug, "post-0"))
        .one(db)
        .await
        .unwrap()
        .unwrap();
    tag_post(db, &first, &[&rust]).await;
    let hidden = post::Entity::find()
        .filter(sea_orm::ColumnTrait::eq(&post::Column::Slug, "hidden-0"))
        .one(db)
        .await
        .unwrap()
        .unwrap();
    tag_post(db, &hidden, &[&draft_only]).await;
    page(&an, "about", 1).insert(db).await.unwrap();

    let reader = content::reader(&app).await;
    let mut paths = Vec::new();
    let mut cursor: Option<String> = None;
    let mut batches = 0;
    loop {
        let uri = match &cursor {
            Some(after) => format!("/api/v1/content/sitemap?type=post&after={after}"),
            None => "/api/v1/content/sitemap?type=post".to_owned(),
        };
        let batch = reader.get(&uri).await;
        batch.assert_status(StatusCode::OK);
        let batch = batch.json();
        batches += 1;
        for entry in batch["data"].as_array().unwrap() {
            assert_eq!(entry["type"], json!("post"));
            assert!(entry["lastmod"].as_str().unwrap().ends_with('Z'));
            paths.push(entry["path"].as_str().unwrap().to_owned());
        }
        match batch["next"].as_str() {
            Some(next) => cursor = Some(next.to_owned()),
            None => break,
        }
    }
    assert_eq!(batches, 2);
    assert_eq!(paths.len(), 1001);
    let unique: std::collections::BTreeSet<&String> = paths.iter().collect();
    assert_eq!(unique.len(), 1001);
    assert!(paths.iter().all(|p| p.starts_with("/posts/post-")), "drafts are not listed");

    let of = |kind: &'static str| {
        let reader = &reader;
        async move {
            let batch = reader.get(&format!("/api/v1/content/sitemap?type={kind}")).await.json();
            assert_eq!(batch["next"], json!(null));
            batch["data"].as_array().unwrap().iter().map(|e| e["path"].as_str().unwrap().to_owned()).collect::<Vec<_>>()
        }
    };
    assert_eq!(of("page").await, ["/about"]);
    assert_eq!(of("category").await, ["/categories/news"]);
    assert_eq!(of("tag").await, ["/tags/rust"]);
    let _ = (unused, draft_only);

    reader.get("/api/v1/content/sitemap").await.assert_validation_error("type");
    reader.get("/api/v1/content/sitemap?type=user").await.assert_validation_error("type");
    reader.get("/api/v1/content/sitemap?type=post&after=-1").await.assert_validation_error("after");
    reader.get("/api/v1/content/sitemap?type=post&after=abc").await.assert_validation_error("after");
});
