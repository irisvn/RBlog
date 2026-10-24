//! Author profiles through the admin API: the slug made from the user's Vietnamese name and kept
//! unique, renames kept in `slug_history` and reported with old and new paths, the avatar public
//! only while the author has a published post, and who may do what.

use std::collections::BTreeSet;

use rblog::database::migrations::Migrator;
use rblog::models::{author_profile, public_media, slug_history};
use rblog::services::content_api::ensure_frontend_user;
use rblog::services::public_media::{Subject, sync};
use rustcms::axum::http::{Method, StatusCode};
use rustcms_testing::{Factory, TestDb, UserFactory, db_test};
use sea_orm::prelude::Uuid;
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set};
use serde_json::{Value, json};

use crate::support::content::{self, post, stored_media};
use crate::support::{self, holding};
use crate::taxonomy::{admin, events};

const RECORDS: &str = "resources/authors/records";

/// The profile's key (the user's id) in a create or update answer.
fn key(response: &Value) -> i64 {
    response["data"]["key"].as_str().expect("record key").parse().unwrap()
}

/// The media ids `public_media` names for a post.
async fn public_rows(db: &DatabaseConnection, post_id: i64) -> BTreeSet<Uuid> {
    public_media::Entity::find()
        .filter(public_media::Column::SubjectType.eq("post"))
        .filter(public_media::Column::SubjectId.eq(post_id))
        .all(db)
        .await
        .unwrap()
        .into_iter()
        .map(|r| r.media_id)
        .collect()
}

db_test!(an_author_slug_is_made_from_the_users_vietnamese_name, migrate = Migrator, |db: TestDb| async move {
    let app = support::app(db).await;
    let panel = admin(&app).await;
    let db = app.state().db();
    let records = app.admin_api(RECORDS);
    let ha = UserFactory::new().name("Nguyễn Thu Hà").create(db).await;
    let namesake = UserFactory::new().name("Nguyễn Thu Hà").create(db).await;

    let created = panel
        .post_json(&records, &json!({"user_id": ha.id.to_string(), "bio": "Viết về du lịch.", "website": " "}))
        .await;
    created.assert_status(StatusCode::CREATED).assert_json("/data/data/slug", json!("nguyen-thu-ha"));
    assert_eq!(key(&created.json()), ha.id);
    let stored = author_profile::Entity::find_by_id(ha.id).one(db).await.unwrap().unwrap();
    assert_eq!(stored.website, None, "a blank website is none");
    // The same name again: the next free slug.
    panel
        .post_json(&records, &json!({"user_id": namesake.id.to_string()}))
        .await
        .assert_status(StatusCode::CREATED)
        .assert_json("/data/data/slug", json!("nguyen-thu-ha-2"));

    let events = events(db, "author", ha.id).await;
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(events[0].kind, "author.created");
    assert_eq!(
        events[0].payload,
        json!({"tags": ["author:nguyen-thu-ha", "posts", "sitemap"],
               "paths": {"old": [], "new": ["/authors/nguyen-thu-ha"]}})
    );
});

db_test!(author_profiles_are_checked, migrate = Migrator, |db: TestDb| async move {
    let app = support::app(db).await;
    let panel = admin(&app).await;
    let db = app.state().db();
    let records = app.admin_api(RECORDS);
    let an = UserFactory::new().name("An").create(db).await;
    let binh = UserFactory::new().name("Bình").create(db).await;
    panel.post_json(&records, &json!({"user_id": an.id.to_string()})).await.assert_status(StatusCode::CREATED);

    // One profile per user.
    panel.post_json(&records, &json!({"user_id": an.id.to_string()})).await.assert_validation_error("user_id");
    // A typed slug is normalized; a taken one is refused on the field.
    panel
        .post_json(&records, &json!({"user_id": binh.id.to_string(), "slug": "AN"}))
        .await
        .assert_validation_error("slug");
    panel
        .post_json(&records, &json!({"user_id": binh.id.to_string(), "slug": "!!!"}))
        .await
        .assert_validation_error("slug");
    // https only, http for localhost.
    let website = |url: &str| json!({"user_id": binh.id.to_string(), "website": url});
    panel.post_json(&records, &website("http://binh.example")).await.assert_validation_error("website");
    panel.post_json(&records, &website("ftp://binh.example")).await.assert_validation_error("website");
    // At most 1000 characters of bio.
    let long_bio = json!({"user_id": binh.id.to_string(), "bio": "ệ".repeat(1001)});
    panel.post_json(&records, &long_bio).await.assert_validation_error("bio");
    let made = panel
        .post_json(
            &records,
            &json!({"user_id": binh.id.to_string(), "slug": "Bình Lê", "website": "http://localhost:3000"}),
        )
        .await;
    made.assert_status(StatusCode::CREATED).assert_json("/data/data/slug", json!("binh-le"));

    // The user never changes.
    let record = format!("{records}/{}", binh.id);
    panel
        .send(Method::PATCH, &record, Some(&json!({"user_id": an.id.to_string()})))
        .await
        .assert_status(StatusCode::UNPROCESSABLE_ENTITY);
    // Refused writes leave no event behind.
    assert_eq!(events(db, "author", binh.id).await.len(), 1);

    // Messages in the user's language.
    let carol = UserFactory::new().name("Carol").create(db).await;
    let vi = panel.languages("vi");
    vi.post_json(&records, &json!({"user_id": carol.id.to_string(), "website": "http://carol.example"}))
        .await
        .assert_json("/errors/website/0", json!("phải là địa chỉ https (http chỉ dùng cho localhost)"));
    vi.post_json(&records, &json!({"user_id": an.id.to_string()}))
        .await
        .assert_json("/errors/user_id/0", json!("đã có hồ sơ tác giả"));
});

db_test!(a_rename_keeps_the_old_slug_and_reports_both_paths, migrate = Migrator, |db: TestDb| async move {
    let app = support::app(db).await;
    let panel = admin(&app).await;
    let db = app.state().db();
    let records = app.admin_api(RECORDS);
    let user = UserFactory::new().name("Trần Minh Quân").create(db).await;
    panel.post_json(&records, &json!({"user_id": user.id.to_string()})).await.assert_status(StatusCode::CREATED);
    let record = format!("{records}/{}", user.id);

    // Other fields keep the slug.
    panel.send(Method::PATCH, &record, Some(&json!({"bio": "Lập trình viên."}))).await.assert_status(StatusCode::OK);
    assert!(slug_history::Entity::find().all(db).await.unwrap().is_empty());
    let now = events(db, "author", user.id).await;
    assert_eq!(now.iter().map(|e| e.kind.as_str()).collect::<Vec<_>>(), ["author.created", "author.updated"]);
    assert_eq!(now[1].payload["paths"], json!({"old": [], "new": ["/authors/tran-minh-quan"]}));

    // A new slug: the old one is kept for redirects, and both are reported.
    panel
        .send(Method::PATCH, &record, Some(&json!({"slug": "Quân Trần"})))
        .await
        .assert_status(StatusCode::OK)
        .assert_json("/data/data/slug", json!("quan-tran"));
    let history = slug_history::Entity::find().all(db).await.unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!((history[0].subject_type.as_str(), history[0].subject_id), ("author", user.id));
    assert_eq!(history[0].old_slug, "tran-minh-quan");
    let last = events(db, "author", user.id).await.pop().unwrap();
    assert_eq!(last.kind, "author.updated");
    assert_eq!(
        last.payload,
        json!({"tags": ["author:quan-tran", "author:tran-minh-quan", "posts", "sitemap"],
               "paths": {"old": ["/authors/tran-minh-quan"], "new": ["/authors/quan-tran"]}})
    );

    // Deleting reports every slug it had and forgets the old ones.
    panel.send(Method::DELETE, &record, None).await.assert_status(StatusCode::NO_CONTENT);
    assert!(slug_history::Entity::find().all(db).await.unwrap().is_empty());
    let last = events(db, "author", user.id).await.pop().unwrap();
    assert_eq!(last.kind, "author.deleted");
    assert_eq!(last.payload["paths"], json!({"old": ["/authors/tran-minh-quan", "/authors/quan-tran"], "new": []}));
});

db_test!(the_avatar_is_public_only_with_a_published_post, migrate = Migrator, |db: TestDb| async move {
    let app = content::app(db).await;
    let panel = admin(&app).await;
    let db = app.state().db();
    let records = app.admin_api(RECORDS);
    let avatar = stored_media(&app, "avatar.png", "image/png", b"\x89PNG avatar").await;
    let other = stored_media(&app, "other.png", "image/png", b"\x89PNG other").await;
    let pdf = stored_media(&app, "cv.pdf", "application/pdf", b"%PDF-1.7").await;

    let an = UserFactory::new().name("An").create(db).await;
    let mut draft = post(&an, "nhap", "Nháp", 1);
    draft.status = Set("draft".into());
    let draft = draft.insert(db).await.unwrap();
    // Only images.
    panel
        .post_json(&records, &json!({"user_id": an.id.to_string(), "avatar_media_id": pdf.id.to_string()}))
        .await
        .assert_validation_error("avatar_media_id");
    panel
        .post_json(&records, &json!({"user_id": an.id.to_string(), "avatar_media_id": avatar.id.to_string()}))
        .await
        .assert_status(StatusCode::CREATED);
    // No published post: the avatar stays private.
    assert!(public_media::Entity::find().all(db).await.unwrap().is_empty());

    // A published post (written before the profile changes): its sync names the avatar.
    let live = post(&an, "bai-da-dang", "Bài đã đăng", 1).insert(db).await.unwrap();
    sync(db, Subject::Post, live.id).await.unwrap();
    assert_eq!(public_rows(db, live.id).await, BTreeSet::from([avatar.id]));
    assert!(public_rows(db, draft.id).await.is_empty());

    // Changing the avatar on the profile moves it with the post, in the same write.
    let record = format!("{records}/{}", an.id);
    panel
        .send(Method::PATCH, &record, Some(&json!({"avatar_media_id": other.id.to_string()})))
        .await
        .assert_status(StatusCode::OK);
    assert_eq!(public_rows(db, live.id).await, BTreeSet::from([other.id]));
    panel.send(Method::PATCH, &record, Some(&json!({"avatar_media_id": null}))).await.assert_status(StatusCode::OK);
    assert!(public_rows(db, live.id).await.is_empty());
    panel
        .send(Method::PATCH, &record, Some(&json!({"avatar_media_id": avatar.id.to_string()})))
        .await
        .assert_status(StatusCode::OK);
    assert_eq!(public_rows(db, live.id).await, BTreeSet::from([avatar.id]));

    // Deleting the profile makes it private again.
    panel.send(Method::DELETE, &record, None).await.assert_status(StatusCode::NO_CONTENT);
    assert!(public_rows(db, live.id).await.is_empty());
});

db_test!(users_without_the_permissions_are_refused, migrate = Migrator, |db: TestDb| async move {
    let app = support::app(db).await;
    let db = app.state().db();
    let records = app.admin_api(RECORDS);
    let an = UserFactory::new().name("An").create(db).await;
    {
        let panel = admin(&app).await;
        panel.post_json(&records, &json!({"user_id": an.id.to_string()})).await.assert_status(StatusCode::CREATED);
    }
    let record = format!("{records}/{}", an.id);
    let binh = UserFactory::new().name("Bình").create(db).await;

    let nobody = UserFactory::new().create(db).await;
    let nobody = app.acting_as(&nobody).await;
    nobody.get(&records).await.assert_status(StatusCode::FORBIDDEN);
    nobody.get(&record).await.assert_status(StatusCode::FORBIDDEN);
    nobody.post_json(&records, &json!({"user_id": binh.id.to_string()})).await.assert_status(StatusCode::FORBIDDEN);
    nobody.send(Method::PATCH, &record, Some(&json!({"bio": "x"}))).await.assert_status(StatusCode::FORBIDDEN);
    nobody.send(Method::DELETE, &record, None).await.assert_status(StatusCode::FORBIDDEN);

    // Viewing is not editing.
    let viewer = holding(&app, "author-viewer", &["authors.view_any", "authors.view"]).await;
    let viewer = app.acting_as(&viewer).await;
    viewer.get(&records).await.assert_status(StatusCode::OK);
    viewer.send(Method::PATCH, &record, Some(&json!({"bio": "x"}))).await.assert_status(StatusCode::FORBIDDEN);
    viewer.send(Method::DELETE, &record, None).await.assert_status(StatusCode::FORBIDDEN);
    assert_eq!(events(db, "author", an.id).await.len(), 1, "only the admin's create");
    assert!(author_profile::Entity::find_by_id(binh.id).one(db).await.unwrap().is_none());
});

db_test!(the_users_resource_only_lists_users, migrate = Migrator, |db: TestDb| async move {
    let app = support::app(db).await;
    let db = app.state().db();
    let frontend = ensure_frontend_user(app.state()).await.unwrap();
    let an = UserFactory::new().name("An").create(db).await;
    let users = app.admin_api("resources/users/records");

    // Even a super admin cannot write users here: the Users page does that.
    let panel = admin(&app).await;
    let listed = panel.get(&users).await;
    listed.assert_status(StatusCode::OK);
    let keys: Vec<String> =
        listed.json()["data"].as_array().unwrap().iter().map(|r| r["key"].as_str().unwrap().to_owned()).collect();
    assert!(keys.contains(&an.id.to_string()), "{keys:?}");
    assert!(!keys.contains(&frontend.id.to_string()), "the Content API user is no author: {keys:?}");
    panel.post_json(&users, &json!({"name": "X"})).await.assert_status(StatusCode::FORBIDDEN);
    let record = format!("{users}/{}", an.id);
    panel.send(Method::PATCH, &record, Some(&json!({"name": "Y"}))).await.assert_status(StatusCode::FORBIDDEN);
    panel.send(Method::DELETE, &record, None).await.assert_status(StatusCode::FORBIDDEN);
    // The Content API user cannot get a profile.
    panel
        .post_json(&app.admin_api(RECORDS), &json!({"user_id": frontend.id.to_string()}))
        .await
        .assert_validation_error("user_id");

    // Listing users needs the Users page's own permission.
    let editor = holding(&app, "author-editor", &["authors.view_any", "authors.view", "authors.create"]).await;
    let editor = app.acting_as(&editor).await;
    editor.get(&users).await.assert_status(StatusCode::FORBIDDEN);
    let reader = holding(&app, "user-reader", &["users.view_any", "users.view"]).await;
    app.acting_as(&reader).await.get(&users).await.assert_status(StatusCode::OK);
});
