//! Revisions of posts and pages: written by the server when the content changes, capped at 30,
//! read and restored through their post or page only.

use rblog::database::migrations::Migrator;
use rblog::models::{post_revision, post_tag};
use rblog::services::revisions::KEEP;
use rustcms::axum::http::{Method, StatusCode};
use rustcms_testing::{TestDb, db_test};
use sea_orm::{ColumnTrait, ConnectionTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder};
use serde_json::{Value, json};

use crate::support::content;
use crate::support::posts::*;

async fn revisions_of(db: &impl ConnectionTrait, key: &str) -> Vec<post_revision::Model> {
    post_revision::Entity::find()
        .filter(post_revision::Column::PostId.eq(key.parse::<i64>().unwrap()))
        .order_by_asc(post_revision::Column::Id)
        .all(db)
        .await
        .unwrap()
}

/// The keys a list answer holds.
fn keys_of(listed: &Value) -> Vec<String> {
    listed["data"].as_array().unwrap().iter().map(|r| r["key"].as_str().unwrap().to_owned()).collect()
}

db_test!(a_revision_is_written_only_when_the_content_changes, migrate = Migrator, |db: TestDb| async move {
    let app = content::app(db).await;
    let db = app.state().db();
    let (editor, _) = signed_in(&app, "Biên", EDITOR_ROLE).await;
    let rust = content::tag(db, "rust", "Rust").await;
    let key = create(&app, &editor, "posts", json!({"title": "Bài", "body": document("Một", &[])})).await;
    assert_eq!(revisions_of(db, &key).await.len(), 1);
    // The same values again, and a change of status: nothing new.
    update(&app, &editor, "posts", &key, json!({"title": "Bài", "body": document("Một", &[])}))
        .await
        .assert_status(StatusCode::OK);
    run(&app, &editor, "posts", "publish", &key, None).await;
    run(&app, &editor, "posts", "unpublish", &key, None).await;
    assert_eq!(revisions_of(db, &key).await.len(), 1);
    // Content: each change is one.
    update(&app, &editor, "posts", &key, json!({"excerpt": "Tóm tắt"})).await.assert_status(StatusCode::OK);
    update(&app, &editor, "posts", &key, json!({"meta_title": "SEO"})).await.assert_status(StatusCode::OK);
    let attach = app.admin_api(&format!("resources/posts/records/{key}/relations/tags/attach"));
    editor.post_json(&attach, &json!({"keys": [rust.id.to_string()]})).await.assert_status(StatusCode::OK);
    let all = revisions_of(db, &key).await;
    assert_eq!(all.len(), 4);
    assert_eq!(all[3].snapshot["tag_ids"], json!([rust.id.to_string()]));
    assert_eq!(all[2].snapshot["meta_title"], json!("SEO"));
    assert!(all.iter().all(|r| r.author_id.is_some()));
});

db_test!(a_post_keeps_its_newest_thirty_revisions, migrate = Migrator, |db: TestDb| async move {
    let app = content::app(db).await;
    let db = app.state().db();
    let (editor, _) = signed_in(&app, "Biên", EDITOR_ROLE).await;
    let key = create(&app, &editor, "posts", json!({"title": "Bản 0"})).await;
    let first = revisions_of(db, &key).await[0].id;
    for n in 1..=35 {
        update(&app, &editor, "posts", &key, json!({"title": format!("Bản {n}")})).await.assert_status(StatusCode::OK);
    }
    let kept = revisions_of(db, &key).await;
    assert_eq!(kept.len() as u64, KEEP);
    assert!(kept.iter().all(|r| r.id != first));
    assert_eq!(kept.last().unwrap().title, "Bản 35");
    assert_eq!(kept[0].title, "Bản 6");
    // Restoring the oldest kept one: done, and reported as done (it is not pruned meanwhile).
    let oldest = kept[0].id.to_string();
    run(&app, &editor, "post_revisions", "restore_version", &oldest, None).await;
    assert_eq!(post_row(db, &key).await.title, "Bản 6");
    assert!(revisions_of(db, &key).await.iter().any(|r| r.id.to_string() == oldest));
});

db_test!(a_restore_needs_the_category_and_tags_it_brings_back, migrate = Migrator, |db: TestDb| async move {
    let app = content::app(db).await;
    let db = app.state().db();
    let (editor, _) = signed_in(&app, "Biên", EDITOR_ROLE).await;
    let travel = content::category(db, "du-lich", "Du lịch").await;
    let key = create(&app, &editor, "posts", json!({"title": "Bài", "category_id": travel.id.to_string()})).await;
    let with_category = revisions_of(db, &key).await[0].id.to_string();
    update(&app, &editor, "posts", &key, json!({"category_id": null})).await.assert_status(StatusCode::OK);
    // Someone who may edit every post, but not see categories.
    let blind = crate::support::holding(
        &app,
        "no-categories",
        &[
            "posts.view_any",
            "posts.view",
            "posts.view_all",
            "posts.edit_others",
            "posts.update",
            "posts.publish",
            "post_authors.view_any",
            "post_authors.view",
            "post_revisions.view_any",
            "post_revisions.view",
            "post_revisions.restore_version",
        ],
    )
    .await;
    let blind = app.acting_as(&blind).await;
    let refused = action(&app, &blind, "post_revisions", "restore_version", &[&with_category], None).await;
    refused.assert_status(StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(post_row(db, &key).await.category_id, None);
    // The editor may.
    run(&app, &editor, "post_revisions", "restore_version", &with_category, None).await;
    assert_eq!(post_row(db, &key).await.category_id, Some(travel.id));
});

db_test!(restoring_a_version_keeps_the_status, migrate = Migrator, |db: TestDb| async move {
    let app = content::app(db).await;
    let db = app.state().db();
    let (author, _) = signed_in(&app, "An", AUTHOR_ROLE).await;
    let rust = content::tag(db, "rust", "Rust").await;
    let key = create(
        &app,
        &author,
        "posts",
        json!({"title": "Bản đầu", "body": document("Lần một", &[]), "tags": {"attach": [rust.id.to_string()]}}),
    )
    .await;
    let original = revisions_of(db, &key).await[0].clone();
    run(&app, &author, "posts", "publish", &key, None).await;
    update(
        &app,
        &author,
        "posts",
        &key,
        json!({"title": "Bản sau", "body": document("Lần hai", &[]), "tags": {"detach": [rust.id.to_string()]}}),
    )
    .await
    .assert_status(StatusCode::OK);
    // On the post's page: its revisions, newest first.
    let listed = author.get(&app.admin_api(&format!("resources/posts/records/{key}/relations/revisions"))).await;
    listed.assert_status(StatusCode::OK);
    assert_eq!(keys_of(&listed.json()).len(), 2);

    run(&app, &author, "post_revisions", "restore_version", &original.id.to_string(), None).await;
    let row = post_row(db, &key).await;
    assert_eq!(row.title, "Bản đầu");
    assert_eq!(row.body, original.body);
    assert_eq!(row.status, "published", "restoring never changes the status");
    let tags = post_tag::Entity::find().filter(post_tag::Column::PostId.eq(row.id)).count(db).await.unwrap();
    assert_eq!(tags, 1);
    // Restoring is a change: a new revision, and an event.
    assert_eq!(revisions_of(db, &key).await.len(), 3);
    assert_eq!(event_kinds(db, "post", &key).await.last().unwrap(), "post.updated");
});

db_test!(revisions_are_seen_and_restored_through_their_post, migrate = Migrator, |db: TestDb| async move {
    let app = content::app(db).await;
    let db = app.state().db();
    let (an, _) = signed_in(&app, "An", AUTHOR_ROLE).await;
    let (binh, _) = signed_in(&app, "Bình", AUTHOR_ROLE).await;
    let (contributor, _) = signed_in(&app, "Chi", CONTRIBUTOR_ROLE).await;
    let (editor, _) = signed_in(&app, "Biên", EDITOR_ROLE).await;
    let theirs = create(&app, &binh, "posts", json!({"title": "Nháp của Bình"})).await;
    let revision = revisions_of(db, &theirs).await[0].id.to_string();

    // Another author's draft history: not listed, not read, not restored, on any path.
    let relation = app.admin_api(&format!("resources/posts/records/{theirs}/relations/revisions"));
    an.get(&relation).await.assert_status(StatusCode::NOT_FOUND);
    let listed = an.get(&records(&app, "post_revisions", None)).await;
    listed.assert_status(StatusCode::OK);
    assert!(keys_of(&listed.json()).is_empty());
    an.get(&records(&app, "post_revisions", Some(&revision))).await.assert_status(StatusCode::NOT_FOUND);
    action(&app, &an, "post_revisions", "restore_version", &[&revision], None)
        .await
        .assert_json("/missing", json!([revision]));
    // The editor sees every post's history.
    editor.get(&relation).await.assert_status(StatusCode::OK);
    assert_eq!(
        keys_of(&editor.get(&records(&app, "post_revisions", None)).await.json()),
        std::slice::from_ref(&revision)
    );
    // Nobody writes revisions.
    editor
        .post_json(&records(&app, "post_revisions", None), &json!({"title": "x", "post_id": theirs}))
        .await
        .assert_status(StatusCode::FORBIDDEN);
    editor
        .send(Method::PATCH, &records(&app, "post_revisions", Some(&revision)), Some(&json!({"title": "x"})))
        .await
        .assert_status(StatusCode::FORBIDDEN);
    editor
        .send(Method::DELETE, &records(&app, "post_revisions", Some(&revision)), None)
        .await
        .assert_status(StatusCode::FORBIDDEN);
    // A trashed post's history is gone with it.
    binh.send(Method::DELETE, &records(&app, "posts", Some(&theirs)), None).await.assert_status(StatusCode::NO_CONTENT);
    editor.get(&records(&app, "post_revisions", Some(&revision))).await.assert_status(StatusCode::NOT_FOUND);

    // A contributor's own post, once published: its history is read, never restored.
    let own = create(&app, &contributor, "posts", json!({"title": "Một"})).await;
    update(&app, &contributor, "posts", &own, json!({"title": "Hai"})).await.assert_status(StatusCode::OK);
    let first = revisions_of(db, &own).await[0].id.to_string();
    run(&app, &editor, "posts", "publish", &own, None).await;
    contributor.get(&records(&app, "post_revisions", Some(&first))).await.assert_status(StatusCode::OK);
    action(&app, &contributor, "post_revisions", "restore_version", &[&first], None)
        .await
        .assert_json("/denied", json!([first]));
    assert_eq!(post_row(db, &own).await.title, "Hai");
    // While it was a draft, they could.
    run(&app, &editor, "posts", "unpublish", &own, None).await;
    run(&app, &contributor, "post_revisions", "restore_version", &first, None).await;
    assert_eq!(post_row(db, &own).await.title, "Một");
});

db_test!(a_version_whose_image_is_gone_is_not_restored, migrate = Migrator, |db: TestDb| async move {
    let app = content::app(db).await;
    let db = app.state().db();
    let (editor, _) = signed_in(&app, "Biên", EDITOR_ROLE).await;
    let image = content::stored_media(&app, "anh.png", "image/png", b"\x89PNG fake").await;
    let key =
        create(&app, &editor, "posts", json!({"title": "Có ảnh", "body": document("x", &[image.id.to_string()])}))
            .await;
    let with_image = revisions_of(db, &key).await[0].id.to_string();
    update(&app, &editor, "posts", &key, json!({"body": document("y", &[])})).await.assert_status(StatusCode::OK);
    // The image goes to the trash.
    let mut trashed: rustcms::admin::media::entity::ActiveModel = image.into();
    trashed.deleted_at = sea_orm::Set(Some(rblog::services::content_sync::now()));
    sea_orm::ActiveModelTrait::update(trashed, db).await.unwrap();
    let refused = action(&app, &editor, "post_revisions", "restore_version", &[&with_image], None).await;
    refused.assert_status(StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(post_row(db, &key).await.body, Some(document("y", &[])));
});

db_test!(page_revisions_follow_their_page, migrate = Migrator, |db: TestDb| async move {
    let app = content::app(db).await;
    let (editor, _) = signed_in(&app, "Biên", EDITOR_ROLE).await;
    let (author, _) = signed_in(&app, "An", AUTHOR_ROLE).await;
    let key = create(&app, &editor, "pages", json!({"title": "Giới thiệu"})).await;
    update(&app, &editor, "pages", &key, json!({"title": "Về chúng tôi"})).await.assert_status(StatusCode::OK);
    let listed = editor.get(&app.admin_api(&format!("resources/pages/records/{key}/relations/revisions"))).await;
    listed.assert_status(StatusCode::OK);
    let keys = keys_of(&listed.json());
    assert_eq!(keys.len(), 2);
    let oldest = keys.last().unwrap().clone();
    run(&app, &editor, "page_revisions", "restore_version", &oldest, None).await;
    assert_eq!(page_row(app.state().db(), &key).await.title, "Giới thiệu");
    // Without page permissions: refused before anything is read.
    author.get(&records(&app, "page_revisions", None)).await.assert_status(StatusCode::FORBIDDEN);
    action(&app, &author, "page_revisions", "restore_version", &[&oldest], None)
        .await
        .assert_status(StatusCode::FORBIDDEN);
});

db_test!(the_revisions_table_restores_a_version_from_its_row, migrate = Migrator, |db: TestDb| async move {
    let app = content::app(db).await;
    let db = app.state().db();
    let (editor, _) = signed_in(&app, "Biên", EDITOR_ROLE).await;
    let (contributor, _) = signed_in(&app, "Chi", CONTRIBUTOR_ROLE).await;
    for (slug, revisions) in [("posts", "post_revisions"), ("pages", "page_revisions")] {
        let key = create(&app, &editor, slug, json!({"title": "Bản đầu"})).await;
        update(&app, &editor, slug, &key, json!({"title": "Bản sau"})).await.assert_status(StatusCode::OK);
        // The post's Revisions table: its rows offer Restore, the related resource's action,
        // labelled (primary) on each row.
        let relation = app.admin_api(&format!("resources/{slug}/records/{key}/relations/revisions"));
        let listed = editor.get(&relation).await;
        listed.assert_status(StatusCode::OK);
        let listed = listed.json();
        let rows = listed["data"].as_array().unwrap();
        assert_eq!(rows.len(), 2, "{slug}");
        assert!(rows.iter().all(|r| r["abilities"]["restore_version"] == json!(true)), "{slug}: {listed}");
        let schema = editor.get(&app.admin_api(&format!("resources/{revisions}/schema?operation=view"))).await;
        let schema = schema.json();
        let restore = &schema["table"]["actions"][0];
        assert_eq!((&restore["name"], &restore["primary"]), (&json!("restore_version"), &json!(true)), "{slug}");
        // Run from the row as the panel does: the related resource's action route.
        let oldest = rows.iter().find(|r| r["data"]["title"] == json!("Bản đầu")).unwrap()["key"].as_str().unwrap();
        run(&app, &editor, revisions, "restore_version", oldest, None).await;
        let title = match slug {
            "posts" => post_row(db, &key).await.title,
            _ => page_row(db, &key).await.title,
        };
        assert_eq!(title, "Bản đầu", "{slug}");
    }
    // Another author's post, and so its Revisions table, does not exist for a contributor.
    let theirs = create(&app, &editor, "posts", json!({"title": "Của Biên"})).await;
    let relation = app.admin_api(&format!("resources/posts/records/{theirs}/relations/revisions"));
    contributor.get(&relation).await.assert_status(StatusCode::NOT_FOUND);
});
