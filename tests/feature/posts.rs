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
    // A pending post is not submitted again: not offered, and refused on that record.
    action(&app, &contributor, "posts", "submit", &[&key], None).await.assert_status(StatusCode::FORBIDDEN);
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
        // A draft is neither unpublished nor (once pending) submitted again: the actions are
        // hidden on it (`visible_when`), so a run on it alone is a 403.
        action(&app, &editor, slug, "unpublish", &[&key], None).await.assert_status(StatusCode::FORBIDDEN);
        run(&app, &editor, slug, "submit", &key, None).await;
        assert_eq!(row(key.clone()).await.0, "pending");
        action(&app, &editor, slug, "submit", &[&key], None).await.assert_status(StatusCode::FORBIDDEN);
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
            action(&app, &editor, slug, name, &[&key], None).await.assert_status(StatusCode::FORBIDDEN);
        }
        action(&app, &editor, slug, "schedule", &[&key], Some(json!({"at": future(1)})))
            .await
            .assert_status(StatusCode::FORBIDDEN);
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

/// The workflow actions each row offers (its `abilities`), by status.
const OFFERED: [(&str, [(&str, bool); 4]); 4] = [
    ("draft", [("submit", true), ("publish", true), ("schedule", true), ("unpublish", false)]),
    ("pending", [("submit", false), ("publish", true), ("schedule", true), ("unpublish", false)]),
    ("scheduled", [("submit", false), ("publish", true), ("schedule", true), ("unpublish", true)]),
    ("published", [("submit", false), ("publish", false), ("schedule", false), ("unpublish", true)]),
];

db_test!(rows_offer_only_the_transitions_that_apply, migrate = Migrator, |db: TestDb| async move {
    let app = content::app(db).await;
    let db = app.state().db();
    let (editor, _) = signed_in(&app, "Biên", EDITOR_ROLE).await;
    for slug in ["posts", "pages"] {
        let status_of = |key: String| async move {
            match slug {
                "posts" => post_row(db, &key).await.status,
                _ => page_row(db, &key).await.status,
            }
        };
        // One record in each status.
        let mut keys = Vec::new();
        for (status, _) in OFFERED {
            let key = create(&app, &editor, slug, json!({"title": format!("{slug} {status}")})).await;
            match status {
                "pending" => run(&app, &editor, slug, "submit", &key, None).await,
                "scheduled" => run(&app, &editor, slug, "schedule", &key, Some(json!({"at": future(2)}))).await,
                "published" => run(&app, &editor, slug, "publish", &key, None).await,
                _ => {}
            }
            assert_eq!(status_of(key.clone()).await, status);
            keys.push(key);
        }
        // The list says, row by row, which actions apply (and the record page the same).
        let listed = editor.get(&records(&app, slug, None)).await.json();
        for ((status, offered), key) in OFFERED.iter().zip(&keys) {
            let row = listed["data"].as_array().unwrap().iter().find(|r| r["key"] == json!(key)).expect("listed");
            let read = editor.get(&records(&app, slug, Some(key))).await.json();
            for (action, expected) in offered {
                assert_eq!(row["abilities"][action], json!(expected), "{slug} {status} {action}");
                assert_eq!(read["data"]["abilities"][action], json!(expected), "{slug} {status} {action}");
            }
            assert_eq!(row["abilities"]["preview"], json!(true), "{slug} {status} preview");
        }
        // A transition hidden on a record is refused on it alone (403), and nothing changes.
        for ((status, offered), key) in OFFERED.iter().zip(&keys) {
            for (name, _) in offered.iter().filter(|(_, offered)| !offered) {
                let data = (*name == "schedule").then(|| json!({"at": future(4)}));
                let refused = action(&app, &editor, slug, name, &[key], data).await;
                assert_eq!(refused.status, StatusCode::FORBIDDEN, "{slug} {status} {name}: {}", refused.text());
                assert_eq!(status_of(key.clone()).await, *status, "{slug} {status} {name}");
            }
        }
        // On a selection, Publish skips the records it does not apply to.
        let done = action(&app, &editor, slug, "publish", &[&keys[0], &keys[3]], None).await;
        done.assert_status(StatusCode::OK)
            .assert_json("/processed", json!([keys[0]]))
            .assert_json("/denied", json!([keys[3]]));
    }
});

db_test!(the_lists_place_their_actions_and_columns, migrate = Migrator, |db: TestDb| async move {
    let app = content::app(db).await;
    let (editor, me) = signed_in(&app, "Biên", EDITOR_ROLE).await;
    for slug in ["posts", "pages"] {
        let schema = editor.get(&app.admin_api(&format!("resources/{slug}/schema"))).await;
        schema.assert_status(StatusCode::OK);
        let schema = schema.json();
        let actions = schema["table"]["actions"].as_array().unwrap();
        let names: Vec<&str> = actions.iter().map(|a| a["name"].as_str().unwrap()).collect();
        assert_eq!(names, ["publish", "submit", "schedule", "unpublish", "preview"], "{slug}");
        let find = |name: &str| actions.iter().find(|a| a["name"] == json!(name)).unwrap().clone();
        // Publish is the row's labelled action; the others are in its menu, Unpublish last.
        assert_eq!(find("publish")["primary"], json!(true), "{slug}");
        assert_eq!(find("publish")["bulk"], json!(true), "{slug}");
        for name in ["submit", "schedule", "unpublish", "preview"] {
            assert_eq!(find(name).get("primary"), None, "{slug} {name}");
        }
        assert_eq!(find("unpublish")["color"], json!("danger"), "{slug}");
        assert_eq!(find("schedule")["submitLabel"], json!("Schedule"), "{slug}");
        // The title keeps the room on a phone.
        let columns = schema["table"]["columns"].as_array().unwrap();
        let hidden: Vec<(&str, Option<&str>)> =
            columns.iter().map(|c| (c["name"].as_str().unwrap(), c["hideBelow"].as_str())).collect();
        let expected: &[(&str, Option<&str>)] = match slug {
            "posts" => &[
                ("title", None),
                ("status", None),
                ("category_id", Some("md")),
                ("author_id", Some("md")),
                ("published_at", Some("sm")),
            ],
            _ => &[
                ("title", None),
                ("status", None),
                ("author_id", Some("md")),
                ("published_at", Some("sm")),
                ("updated_at", Some("lg")),
            ],
        };
        assert_eq!(hidden, expected, "{slug}");
    }
    // In Vietnamese, Schedule's dialog button reads "Hẹn giờ".
    let vi = app.acting_as(&me).await.languages("vi");
    let schema = vi.get(&app.admin_api("resources/posts/schema")).await.json();
    let actions = schema["table"]["actions"].as_array().unwrap();
    let schedule = actions.iter().find(|a| a["name"] == json!("schedule")).unwrap();
    assert_eq!(schedule["submitLabel"], json!("Hẹn giờ"));
});

/// The input or layout of a form schema named `name` (an input) or labelled `name` (a layout).
fn component<'a>(components: &'a serde_json::Value, name: &str) -> &'a serde_json::Value {
    components
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == json!(name) || c["label"] == json!(name))
        .unwrap_or_else(|| panic!("no {name} in {components}"))
}

/// The names (inputs) and labels (layouts) of a list of components, in order.
fn names(components: &serde_json::Value) -> Vec<&str> {
    components.as_array().unwrap().iter().map(|c| c["name"].as_str().or(c["label"].as_str()).unwrap_or("?")).collect()
}

db_test!(posts_and_pages_are_edited_in_an_editor_layout, migrate = Migrator, |db: TestDb| async move {
    let app = content::app(db).await;
    let (editor, me) = signed_in(&app, "Biên", EDITOR_ROLE).await;
    let mut post = String::new();
    for (slug, prefix, aside) in [
        ("posts", "/posts/", &["Publishing", "Cover image", "Organize"][..]),
        ("pages", "/", &["Publishing", "Cover image"][..]),
    ] {
        let created = editor.post_json(&records(&app, slug, None), &json!({"title": "Phở Hà Nội"})).await;
        created.assert_status(StatusCode::CREATED);
        let record = key(&created);
        if slug == "posts" {
            post = record.clone();
        }
        for operation in ["create", "edit", "view"] {
            let query = match operation {
                "create" => "operation=create".to_owned(),
                _ => format!("operation={operation}&key={record}"),
            };
            let schema = editor.get(&app.admin_api(&format!("resources/{slug}/schema?{query}"))).await;
            schema.assert_status(StatusCode::OK);
            let form = &schema.json()["form"];
            // The whole width, one split: the writing in the main column, the details beside it.
            assert_eq!(form["width"], json!("full"), "{slug} {operation}");
            let top = form["components"].as_array().unwrap();
            assert_eq!(top.len(), 1, "{slug} {operation}: {top:?}");
            assert_eq!(top[0]["type"], json!("split"), "{slug} {operation}");
            let panes = top[0]["components"].as_array().unwrap();
            let ids: Vec<&str> = panes.iter().map(|p| p["id"].as_str().unwrap()).collect();
            assert_eq!(ids, ["main", "aside"], "{slug} {operation}");
            let (main, side) = (&panes[0]["components"], &panes[1]["components"]);
            // The slug right under the title, with its path; SEO folded at the end.
            assert_eq!(names(main), ["title", "slug", "body", "excerpt", "SEO"], "{slug} {operation}");
            assert_eq!(component(main, "slug")["prefix"], json!(prefix), "{slug} {operation}");
            let seo = component(main, "SEO");
            assert_eq!((seo["type"].clone(), seo["collapsed"].clone()), (json!("section"), json!(true)));
            assert_eq!(names(&seo["components"]), ["meta_title", "meta_description", "canonical_url", "og_media_id"]);
            // Compact sections in the aside; no tabs anywhere. A new record has no status, date
            // or author yet (the server sets them), so its form has no Publishing section.
            let aside = if operation == "create" { &aside[1..] } else { aside };
            assert_eq!(names(side), aside, "{slug} {operation}");
            for section in side.as_array().unwrap() {
                assert_eq!(section["compact"], json!(true), "{slug} {operation}");
            }
            if operation != "create" {
                let publishing = component(side, "Publishing");
                assert_eq!(names(&publishing["components"]), ["status", "published_at", "author_id"]);
                // Read-only: only the workflow actions in the page's header change them.
                for field in ["status", "published_at", "author_id"] {
                    let input = component(&publishing["components"], field);
                    assert!(operation == "view" || input["disabled"] == json!(true), "{slug} {field}: {input}");
                }
            }
            if slug == "posts" {
                // The view page lists the tags in their relation table, not in the form.
                let organize: &[&str] = if operation == "view" {
                    &["category_id", "featured"]
                } else {
                    &["category_id", "tags", "featured"]
                };
                assert_eq!(names(&component(side, "Organize")["components"]), organize, "{operation}");
            }
            assert!(!form.to_string().contains("\"tabs\""), "{slug} {operation}");
        }
    }
    // In Vietnamese: the section titles and the slug's help.
    let vi = app.acting_as(&me).await.languages("vi");
    let form =
        vi.get(&app.admin_api(&format!("resources/posts/schema?operation=view&key={post}"))).await.json()["form"]
            .clone();
    let panes = &form["components"][0]["components"];
    assert_eq!(names(&panes[1]["components"]), ["Xuất bản", "Ảnh bìa", "Phân loại"]);
    assert_eq!(component(&panes[0]["components"], "slug")["help"], json!("Để trống: tạo từ tiêu đề"));
    assert_eq!(names(&panes[0]["components"])[4], "SEO");
});
