//! Posts and pages through the admin API, as the seeded roles: the permission matrix cell by
//! cell, the publishing transitions, slugs, search text and the outbox.

use rblog::database::migrations::Migrator;
use rblog::models::{post, slug_history};
use rustcms::axum::http::{Method, StatusCode};
use rustcms_testing::{TestDb, db_test};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use serde_json::json;

use crate::support::content;
use crate::support::posts::*;

/// The slugs the Content API lists for `?q=`.
async fn found(reader: &rustcms_testing::TestClient<'_>, q: &str) -> Vec<String> {
    let listed = reader.get(&format!("/api/v1/content/posts?q={q}")).await.json();
    listed["data"].as_array().unwrap().iter().map(|p| p["slug"].as_str().unwrap().to_owned()).collect()
}

fn future(hours: i64) -> String {
    let at = rblog::services::content_sync::now() + std::time::Duration::from_secs((hours * 3600) as u64);
    at.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

db_test!(a_contributor_writes_drafts_only, migrate = Migrator, |db: TestDb| async move {
    let app = content::app(db).await;
    let (contributor, me) = signed_in(&app, "Chi", CONTRIBUTOR_ROLE).await;
    let posts = records(&app, "posts", None);

    // The server sets status, publish date and author: never the form.
    for guarded in [
        json!({"title": "Lên trang", "status": "published"}),
        json!({"title": "Lên trang", "published_at": "2026-01-01T00:00:00Z"}),
        json!({"title": "Lên trang", "author_id": "1"}),
    ] {
        let refused = contributor.post_json(&posts, &guarded).await;
        assert_eq!(refused.status, StatusCode::UNPROCESSABLE_ENTITY, "{guarded}: {}", refused.text());
    }
    let created = contributor.post_json(&posts, &json!({"title": "Xin chào Việt Nam"})).await;
    created.assert_status(StatusCode::CREATED);
    created
        .assert_json("/data/data/status", json!("draft"))
        .assert_json("/data/data/slug", json!("xin-chao-viet-nam"))
        .assert_json("/data/data/author_id", json!(me.id.to_string()))
        .assert_json("/data/data/published_at", json!(null));
    let key = key(&created);
    // Nothing to publish with.
    let refused = action(&app, &contributor, "posts", "publish", &[&key], None).await;
    refused.assert_status(StatusCode::FORBIDDEN);
    let refused = action(&app, &contributor, "posts", "schedule", &[&key], Some(json!({"at": future(2)}))).await;
    refused.assert_status(StatusCode::FORBIDDEN);
    // A draft: edited, submitted, still edited while pending.
    update(&app, &contributor, "posts", &key, json!({"excerpt": "Tóm tắt"})).await.assert_status(StatusCode::OK);
    run(&app, &contributor, "posts", "submit", &key, None).await;
    assert_eq!(post_row(app.state().db(), &key).await.status, "pending");
    update(&app, &contributor, "posts", &key, json!({"excerpt": "Tóm tắt 2"})).await.assert_status(StatusCode::OK);
    // A pending post is not submitted again.
    action(&app, &contributor, "posts", "submit", &[&key], None).await.assert_json("/denied", json!([key]));
    // Pending: not trashed (only drafts).
    contributor
        .send(Method::DELETE, &records(&app, "posts", Some(&key)), None)
        .await
        .assert_status(StatusCode::FORBIDDEN);
});

db_test!(a_contributor_cannot_change_a_published_post, migrate = Migrator, |db: TestDb| async move {
    let app = content::app(db).await;
    let db = app.state().db();
    let (contributor, _) = signed_in(&app, "Chi", CONTRIBUTOR_ROLE).await;
    let (editor, _) = signed_in(&app, "Biên", EDITOR_ROLE).await;
    let rust = content::tag(db, "rust", "Rust").await;
    let key = create(&app, &contributor, "posts", json!({"title": "Bài của Chi"})).await;
    run(&app, &contributor, "posts", "submit", &key, None).await;
    // The editor sees it (every post) and publishes it.
    editor.get(&records(&app, "posts", Some(&key))).await.assert_status(StatusCode::OK);
    run(&app, &editor, "posts", "publish", &key, None).await;
    assert_eq!(post_row(db, &key).await.status, "published");

    // Live now: the contributor may read it, never change it.
    let read = contributor.get(&records(&app, "posts", Some(&key))).await;
    read.assert_status(StatusCode::OK);
    for ability in ["update", "delete", "submit", "publish", "unpublish"] {
        assert_eq!(read.json()["data"]["abilities"][ability], json!(false), "{ability}");
    }
    update(&app, &contributor, "posts", &key, json!({"title": "Sửa"})).await.assert_status(StatusCode::FORBIDDEN);
    // Nor its tags: from the form, or the relation manager.
    update(&app, &contributor, "posts", &key, json!({"tags": {"attach": [rust.id.to_string()]}}))
        .await
        .assert_status(StatusCode::FORBIDDEN);
    let attach = app.admin_api(&format!("resources/posts/records/{key}/relations/tags/attach"));
    contributor.post_json(&attach, &json!({"keys": [rust.id.to_string()]})).await.assert_status(StatusCode::FORBIDDEN);
    contributor
        .send(Method::DELETE, &records(&app, "posts", Some(&key)), None)
        .await
        .assert_status(StatusCode::FORBIDDEN);
    action(&app, &contributor, "posts", "unpublish", &[&key], None).await.assert_status(StatusCode::FORBIDDEN);
    assert_eq!(post_row(db, &key).await.title, "Bài của Chi");
    // The editor still may.
    editor.post_json(&attach, &json!({"keys": [rust.id.to_string()]})).await.assert_status(StatusCode::OK);
});

db_test!(an_author_publishes_their_own_posts_only, migrate = Migrator, |db: TestDb| async move {
    let app = content::app(db).await;
    let db = app.state().db();
    let (an, _) = signed_in(&app, "An", AUTHOR_ROLE).await;
    let (binh, _) = signed_in(&app, "Bình", AUTHOR_ROLE).await;
    let mine = create(&app, &an, "posts", json!({"title": "Của An"})).await;
    let theirs = create(&app, &binh, "posts", json!({"title": "Của Bình"})).await;

    // Another author's post does not exist for An: not listed, not read, not written.
    let listed = an.get(&records(&app, "posts", None)).await.json();
    let keys: Vec<&str> = listed["data"].as_array().unwrap().iter().map(|r| r["key"].as_str().unwrap()).collect();
    assert_eq!(keys, [mine.as_str()]);
    an.get(&records(&app, "posts", Some(&theirs))).await.assert_status(StatusCode::NOT_FOUND);
    update(&app, &an, "posts", &theirs, json!({"title": "Sửa"})).await.assert_status(StatusCode::NOT_FOUND);
    an.send(Method::DELETE, &records(&app, "posts", Some(&theirs)), None).await.assert_status(StatusCode::NOT_FOUND);
    action(&app, &an, "posts", "publish", &[&theirs], None).await.assert_json("/missing", json!([theirs]));
    assert_eq!(post_row(db, &theirs).await.status, "draft");

    // Their own: published at once, scheduled, unpublished.
    run(&app, &an, "posts", "publish", &mine, None).await;
    let row = post_row(db, &mine).await;
    assert_eq!(row.status, "published");
    assert!(row.published_at.is_some());
    // Editing a published post of one's own is allowed with the right to publish.
    update(&app, &an, "posts", &mine, json!({"title": "Của An (sửa)"})).await.assert_status(StatusCode::OK);
    run(&app, &an, "posts", "unpublish", &mine, None).await;
    let row = post_row(db, &mine).await;
    assert_eq!((row.status.as_str(), row.published_at), ("draft", None));
    // Pages are for editors.
    an.get(&records(&app, "pages", None)).await.assert_status(StatusCode::FORBIDDEN);
    an.post_json(&records(&app, "pages", None), &json!({"title": "Giới thiệu"}))
        .await
        .assert_status(StatusCode::FORBIDDEN);
});

db_test!(transitions_apply_from_their_states_only, migrate = Migrator, |db: TestDb| async move {
    let app = content::app(db).await;
    let db = app.state().db();
    let (editor, _) = signed_in(&app, "Biên", EDITOR_ROLE).await;
    for slug in ["posts", "pages"] {
        let row = |key: String| async move {
            match slug {
                "posts" => {
                    let p = post_row(db, &key).await;
                    (p.status, p.published_at)
                }
                _ => {
                    let p = page_row(db, &key).await;
                    (p.status, p.published_at)
                }
            }
        };
        let key = create(&app, &editor, slug, json!({"title": format!("Bài {slug}")})).await;
        // A draft is neither unpublished nor (once pending) submitted again.
        action(&app, &editor, slug, "unpublish", &[&key], None).await.assert_json("/denied", json!([key]));
        run(&app, &editor, slug, "submit", &key, None).await;
        assert_eq!(row(key.clone()).await.0, "pending");
        action(&app, &editor, slug, "submit", &[&key], None).await.assert_json("/denied", json!([key]));
        // A schedule in the past is refused on the field; in the future it holds.
        let past = action(&app, &editor, slug, "schedule", &[&key], Some(json!({"at": "2020-01-01T00:00:00Z"}))).await;
        past.assert_validation_error("at");
        run(&app, &editor, slug, "schedule", &key, Some(json!({"at": future(3)}))).await;
        let (status, at) = row(key.clone()).await;
        assert_eq!(status, "scheduled");
        assert!(at.unwrap() > rblog::services::content_sync::now());
        // Rescheduled, then published now instead.
        run(&app, &editor, slug, "schedule", &key, Some(json!({"at": future(5)}))).await;
        run(&app, &editor, slug, "publish", &key, None).await;
        let (status, at) = row(key.clone()).await;
        assert_eq!(status, "published");
        assert!(at.unwrap() <= rblog::services::content_sync::now());
        // Published: not published or scheduled again, not submitted; unpublished to a draft.
        for name in ["publish", "submit"] {
            action(&app, &editor, slug, name, &[&key], None).await.assert_json("/denied", json!([key]));
        }
        action(&app, &editor, slug, "schedule", &[&key], Some(json!({"at": future(1)})))
            .await
            .assert_json("/denied", json!([key]));
        run(&app, &editor, slug, "unpublish", &key, None).await;
        assert_eq!(row(key.clone()).await, ("draft".to_owned(), None));
        // A scheduled one can be unpublished too.
        run(&app, &editor, slug, "schedule", &key, Some(json!({"at": future(1)}))).await;
        run(&app, &editor, slug, "unpublish", &key, None).await;
        assert_eq!(row(key.clone()).await, ("draft".to_owned(), None));
    }
});

db_test!(bulk_publish_publishes_what_the_user_may, migrate = Migrator, |db: TestDb| async move {
    let app = content::app(db).await;
    let db = app.state().db();
    let (an, _) = signed_in(&app, "An", AUTHOR_ROLE).await;
    let (binh, _) = signed_in(&app, "Bình", AUTHOR_ROLE).await;
    let (editor, _) = signed_in(&app, "Biên", EDITOR_ROLE).await;
    let a1 = create(&app, &an, "posts", json!({"title": "Một"})).await;
    let a2 = create(&app, &an, "posts", json!({"title": "Hai"})).await;
    let b1 = create(&app, &binh, "posts", json!({"title": "Ba"})).await;
    let done = action(&app, &an, "posts", "publish", &[&a1, &a2, &b1], None).await;
    done.assert_status(StatusCode::OK).assert_json("/missing", json!([b1]));
    let mut processed: Vec<String> =
        done.json()["processed"].as_array().unwrap().iter().map(|k| k.as_str().unwrap().to_owned()).collect();
    processed.sort();
    let mut expected = vec![a1.clone(), a2.clone()];
    expected.sort();
    assert_eq!(processed, expected);
    assert_eq!(post_row(db, &b1).await.status, "draft");
    // An editor publishes anyone's.
    run(&app, &editor, "posts", "publish", &b1, None).await;
    assert_eq!(post_row(db, &b1).await.status, "published");
});

db_test!(slugs_are_made_kept_and_refused_when_taken, migrate = Migrator, |db: TestDb| async move {
    let app = content::app(db).await;
    let db = app.state().db();
    let (editor, editor_user) = signed_in(&app, "Biên", EDITOR_ROLE).await;
    let first = create(&app, &editor, "posts", json!({"title": "Đà Nẵng mùa hè"})).await;
    assert_eq!(post_row(db, &first).await.slug, "da-nang-mua-he");
    let second = create(&app, &editor, "posts", json!({"title": "Đà Nẵng mùa hè"})).await;
    assert_eq!(post_row(db, &second).await.slug, "da-nang-mua-he-2");
    let typed = create(&app, &editor, "posts", json!({"title": "Khác", "slug": "Phở Hà Nội!"})).await;
    assert_eq!(post_row(db, &typed).await.slug, "pho-ha-noi");
    // A taken typed slug is a field error, in the user's language.
    editor
        .post_json(&records(&app, "posts", None), &json!({"title": "X", "slug": "pho-ha-noi"}))
        .await
        .assert_validation_error("slug");
    let vi = app.acting_as(&editor_user).await.languages("vi");
    vi.post_json(&records(&app, "posts", None), &json!({"title": "X", "slug": "pho-ha-noi"}))
        .await
        .assert_json("/errors/slug/0", json!("đã được dùng"));
    update(&app, &editor, "posts", &second, json!({"slug": "pho-ha-noi"})).await.assert_validation_error("slug");
    editor.post_json(&records(&app, "posts", None), &json!({"title": "   "})).await.assert_validation_error("title");
    // A rename keeps the old slug for redirects, and the event lists both paths.
    update(&app, &editor, "posts", &first, json!({"slug": "da-nang"})).await.assert_status(StatusCode::OK);
    let history = slug_history::Entity::find()
        .filter(slug_history::Column::SubjectType.eq("post"))
        .filter(slug_history::Column::SubjectId.eq(first.parse::<i64>().unwrap()))
        .all(db)
        .await
        .unwrap();
    assert_eq!(history.iter().map(|h| h.old_slug.as_str()).collect::<Vec<_>>(), ["da-nang-mua-he"]);
    let renamed = events(db, "post", &first).await.pop().unwrap();
    assert_eq!(renamed.kind, "post.updated");
    assert_eq!(renamed.payload["paths"]["old"], json!(["/posts/da-nang-mua-he", "/posts/da-nang"]));
    // A canonical URL is a web address.
    update(&app, &editor, "posts", &first, json!({"canonical_url": "javascript:alert(1)"}))
        .await
        .assert_validation_error("canonical_url");
});

db_test!(search_text_and_reading_time_follow_the_content, migrate = Migrator, |db: TestDb| async move {
    let app = content::app(db).await;
    let db = app.state().db();
    let (editor, _) = signed_in(&app, "Biên", EDITOR_ROLE).await;
    let words = "chữ ".repeat(450);
    let key = create(
        &app,
        &editor,
        "posts",
        json!({"title": "Xin chào Việt Nam", "excerpt": "Phở BÒ", "body": document(&words, &[])}),
    )
    .await;
    let row = post_row(db, &key).await;
    let search = row.search_text.unwrap();
    assert!(search.starts_with("xin chao viet nam pho bo chu chu"), "{search}");
    assert_eq!(row.reading_minutes, 3, "450 words at 200 a minute");
    // No body: no reading time; a short one: a minute.
    update(&app, &editor, "posts", &key, json!({"body": null})).await.assert_status(StatusCode::OK);
    assert_eq!(post_row(db, &key).await.reading_minutes, 0);
    update(&app, &editor, "posts", &key, json!({"body": document("Một hai", &[])})).await.assert_status(StatusCode::OK);
    assert_eq!(post_row(db, &key).await.reading_minutes, 1);

    // The Content API finds it without accents, once published.
    let reader = content::reader(&app).await;
    assert!(found(&reader, "viet").await.is_empty());
    run(&app, &editor, "posts", "publish", &key, None).await;
    assert_eq!(found(&reader, "viet").await, ["xin-chao-viet-nam"]);
    // A page too.
    let page = create(&app, &editor, "pages", json!({"title": "Giới thiệu Đà Lạt"})).await;
    assert_eq!(page_row(db, &page).await.search_text.as_deref(), Some("gioi thieu da lat"));
});

db_test!(every_write_records_one_outbox_event, migrate = Migrator, |db: TestDb| async move {
    let app = content::app(db).await;
    let db = app.state().db();
    let (editor, _) = signed_in(&app, "Biên", EDITOR_ROLE).await;
    let travel = content::category(db, "du-lich", "Du lịch").await;
    let rust = content::tag(db, "rust", "Rust").await;
    let go = content::tag(db, "go", "Go").await;

    // Created with a tag from the form: one event, which names it.
    let key = create(
        &app,
        &editor,
        "posts",
        json!({"title": "Bài", "category_id": travel.id.to_string(), "tags": {"attach": [rust.id.to_string()]}}),
    )
    .await;
    let all = events(db, "post", &key).await;
    assert_eq!(all.len(), 1, "{all:?}");
    assert_eq!(all[0].kind, "post.created");
    assert_eq!(
        all[0].payload,
        json!({"tags": [format!("post:{key}"), "posts", "category:du-lich", "tag:rust", "sitemap"],
               "paths": {"old": ["/posts/bai"], "new": []}})
    );
    // Published: its path is new.
    run(&app, &editor, "posts", "publish", &key, None).await;
    let published = events(db, "post", &key).await.pop().unwrap();
    assert_eq!(published.kind, "post.published");
    assert_eq!(published.payload["paths"], json!({"old": [], "new": ["/posts/bai"]}));
    // An edit that also changes tags: one event, with the tag it lost.
    update(
        &app,
        &editor,
        "posts",
        &key,
        json!({"title": "Bài mới", "tags": {"attach": [go.id.to_string()], "detach": [rust.id.to_string()]}}),
    )
    .await
    .assert_status(StatusCode::OK);
    let edited = events(db, "post", &key).await;
    assert_eq!(edited.len(), 3, "{edited:?}");
    let tags = edited[2].payload["tags"].as_array().unwrap().clone();
    assert!(tags.contains(&json!("tag:go")) && tags.contains(&json!("tag:rust")), "{tags:?}");
    // A link change in the relation manager: an update of the post.
    let detach = app.admin_api(&format!("resources/posts/records/{key}/relations/tags/detach"));
    editor.post_json(&detach, &json!({"keys": [go.id.to_string()]})).await.assert_status(StatusCode::OK);
    // Detaching what is not linked changes nothing: no event.
    editor.post_json(&detach, &json!({"keys": [go.id.to_string()]})).await.assert_status(StatusCode::OK);
    assert_eq!(event_kinds(db, "post", &key).await, ["post.created", "post.published", "post.updated", "post.updated"]);
    // Trash, restore, delete for good.
    let record = records(&app, "posts", Some(&key));
    editor.send(Method::DELETE, &record, None).await.assert_status(StatusCode::NO_CONTENT);
    let trashed = events(db, "post", &key).await.pop().unwrap();
    assert_eq!(trashed.kind, "post.trashed");
    assert_eq!(trashed.payload["paths"], json!({"old": ["/posts/bai"], "new": []}));
    editor.post_json(&format!("{record}/restore"), &json!({})).await.assert_status(StatusCode::OK);
    let restored = events(db, "post", &key).await.pop().unwrap();
    assert_eq!(restored.kind, "post.restored");
    assert_eq!(restored.payload["paths"], json!({"old": [], "new": ["/posts/bai"]}));
    editor.send(Method::DELETE, &record, None).await.assert_status(StatusCode::NO_CONTENT);
    editor.send(Method::DELETE, &format!("{record}/force"), None).await.assert_status(StatusCode::NO_CONTENT);
    let deleted = events(db, "post", &key).await.pop().unwrap();
    assert_eq!(deleted.kind, "post.deleted");
    assert!(deleted.payload["tags"].as_array().unwrap().contains(&json!("category:du-lich")));
    assert_eq!(deleted.payload["paths"], json!({"old": ["/posts/bai"], "new": []}));
    assert!(post::Entity::find_by_id(key.parse::<i64>().unwrap()).one(db).await.unwrap().is_none());
    // Every event has its delivery job.
    assert!(events(db, "post", &key).await.iter().all(|e| e.job_id.is_some()));
});

db_test!(events_do_not_depend_on_the_clock, migrate = Migrator, |db: TestDb| async move {
    let app = content::app(db).await;
    let db = app.state().db();
    let (editor, _) = signed_in(&app, "Biên", EDITOR_ROLE).await;
    let rust = content::tag(db, "rust", "Rust").await;
    let key = create(&app, &editor, "posts", json!({"title": "Bài"})).await;
    // An event written by an instance whose clock runs a day ahead.
    let ahead = rblog::services::content_sync::now() + std::time::Duration::from_secs(86_400);
    let mut skewed: rblog::models::content_event::ActiveModel = events(db, "post", &key).await.remove(0).into();
    skewed.created_at = sea_orm::Set(ahead);
    sea_orm::ActiveModelTrait::update(skewed, db).await.unwrap();
    // Later writes are reported all the same: a form save, a form save with tags, a link change.
    update(&app, &editor, "posts", &key, json!({"title": "Bài 2"})).await.assert_status(StatusCode::OK);
    update(&app, &editor, "posts", &key, json!({"title": "Bài 3", "tags": {"attach": [rust.id.to_string()]}}))
        .await
        .assert_status(StatusCode::OK);
    let detach = app.admin_api(&format!("resources/posts/records/{key}/relations/tags/detach"));
    editor.post_json(&detach, &json!({"keys": [rust.id.to_string()]})).await.assert_status(StatusCode::OK);
    assert_eq!(event_kinds(db, "post", &key).await, ["post.created", "post.updated", "post.updated", "post.updated"]);
});

db_test!(seeing_every_post_is_not_editing_it, migrate = Migrator, |db: TestDb| async move {
    let app = content::app(db).await;
    let db = app.state().db();
    let (an, _) = signed_in(&app, "An", AUTHOR_ROLE).await;
    let rust = content::tag(db, "rust", "Rust").await;
    let theirs = create(&app, &an, "posts", json!({"title": "Của An"})).await;
    // A reviewer: every post, the rights to edit and publish, but not others' posts.
    let reviewer = crate::support::holding(
        &app,
        "reviewer",
        &[
            "posts.view_any",
            "posts.view",
            "posts.view_all",
            "posts.update",
            "posts.delete",
            "posts.attach_tags",
            "posts.submit",
            "posts.publish",
            "posts.unpublish",
            "post_authors.view_any",
            "post_authors.view",
            "post_revisions.view_any",
            "post_revisions.view",
            "post_revisions.restore_version",
        ],
    )
    .await;
    let reviewer = app.acting_as(&reviewer).await;
    let read = reviewer.get(&records(&app, "posts", Some(&theirs))).await;
    read.assert_status(StatusCode::OK).assert_json("/data/labels/author_id", json!("An"));
    for ability in ["update", "delete", "submit", "publish", "unpublish"] {
        assert_eq!(read.json()["data"]["abilities"][ability], json!(false), "{ability}");
    }
    update(&app, &reviewer, "posts", &theirs, json!({"title": "Sửa"})).await.assert_status(StatusCode::FORBIDDEN);
    reviewer
        .send(Method::DELETE, &records(&app, "posts", Some(&theirs)), None)
        .await
        .assert_status(StatusCode::FORBIDDEN);
    for name in ["submit", "publish"] {
        action(&app, &reviewer, "posts", name, &[&theirs], None).await.assert_json("/denied", json!([theirs]));
    }
    let attach = app.admin_api(&format!("resources/posts/records/{theirs}/relations/tags/attach"));
    reviewer.post_json(&attach, &json!({"keys": [rust.id.to_string()]})).await.assert_status(StatusCode::FORBIDDEN);
    // Its history is readable, not restorable.
    let revision = crate::support::posts::first_revision(db, &theirs).await;
    reviewer.get(&records(&app, "post_revisions", Some(&revision))).await.assert_status(StatusCode::OK);
    action(&app, &reviewer, "post_revisions", "restore_version", &[&revision], None)
        .await
        .assert_json("/denied", json!([revision]));
    assert_eq!(post_row(db, &theirs).await.title, "Của An");
});

db_test!(authors_names_are_shown_to_whom_sees_their_posts, migrate = Migrator, |db: TestDb| async move {
    let app = content::app(db).await;
    let (an, an_user) = signed_in(&app, "An", AUTHOR_ROLE).await;
    let (contributor, chi) = signed_in(&app, "Chi", CONTRIBUTOR_ROLE).await;
    let (editor, _) = signed_in(&app, "Biên", EDITOR_ROLE).await;
    let an_post = create(&app, &an, "posts", json!({"title": "Của An"})).await;
    let names = async |client: &rustcms_testing::TestClient<'_>| -> Vec<String> {
        let listed = client.get(&records(&app, "post_authors", None)).await;
        listed.assert_status(StatusCode::OK);
        let mut names: Vec<String> = listed.json()["data"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["data"]["name"].as_str().unwrap().to_owned())
            .collect();
        names.sort();
        names
    };
    // Only one's own name without `posts.view_all`; never an email.
    assert_eq!(names(&contributor).await, ["Chi"]);
    let listed = contributor.get(&records(&app, "post_authors", None)).await.json();
    assert_eq!(listed["data"][0]["data"].as_object().unwrap().keys().collect::<Vec<_>>(), ["name"]);
    contributor
        .get(&records(&app, "post_authors", Some(&an_user.id.to_string())))
        .await
        .assert_status(StatusCode::NOT_FOUND);
    // An editor: whoever wrote a post, and themselves.
    assert_eq!(names(&editor).await, ["An", "Biên"]);
    // Who saved a revision, without a post of their own, too ("Saved by").
    let (other_editor, _) = signed_in(&app, "Duyên", EDITOR_ROLE).await;
    update(&app, &other_editor, "posts", &an_post, json!({"title": "Của An (sửa)"}))
        .await
        .assert_status(StatusCode::OK);
    assert_eq!(names(&editor).await, ["An", "Biên", "Duyên"]);
    // Writers never reach the Users page's list.
    contributor.get(&records(&app, "users", None)).await.assert_status(StatusCode::FORBIDDEN);
    // Nobody writes through it.
    editor
        .send(Method::PATCH, &records(&app, "post_authors", Some(&chi.id.to_string())), Some(&json!({"name": "X"})))
        .await
        .assert_status(StatusCode::FORBIDDEN);
    // Tags accept the links widget's page size.
    let tags = editor.get(&format!("{}?per_page=100", records(&app, "tags", None))).await;
    tags.assert_status(StatusCode::OK).assert_json("/meta/perPage", json!(100));
});
