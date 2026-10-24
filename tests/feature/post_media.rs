//! What the public site gets from the panel's writes: images served only while their post is
//! published, and preview links made from a post's page.

use rblog::database::migrations::Migrator;
use rustcms::axum::http::{Method, StatusCode};
use rustcms_testing::{TestApp, TestClient, TestDb, db_test};
use serde_json::{Value, json};

use crate::support::content;
use crate::support::posts::*;

async fn save_settings(app: &TestApp, client: &TestClient<'_>, group: &str, data: Value) {
    let uri = app.admin_api(&format!("settings/{group}"));
    let version = client.get(&uri).await.assert_status(StatusCode::OK).json()["version"].clone();
    client
        .send(Method::PUT, &uri, Some(&json!({ "version": version, "data": data })))
        .await
        .assert_status(StatusCode::OK);
}

db_test!(images_are_public_only_while_their_post_is_published, migrate = Migrator, |db: TestDb| async move {
    let app = content::app(db).await;
    let (editor, _) = signed_in(&app, "Biên", EDITOR_ROLE).await;
    let cover = content::stored_media(&app, "bia.png", "image/png", b"\x89PNG cover").await;
    let inline = content::stored_media(&app, "trong-bai.png", "image/png", b"\x89PNG inline").await;
    let key = create(
        &app,
        &editor,
        "posts",
        json!({"title": "Ảnh", "cover_media_id": cover.id.to_string(), "body": document("x", &[inline.id.to_string()])}),
    )
    .await;
    let public = app.client();
    let status = async |id: &sea_orm::prelude::Uuid| public.get(&format!("/api/v1/media/{id}/a.png")).await.status;
    assert_eq!(status(&cover.id).await, StatusCode::NOT_FOUND);
    assert_eq!(status(&inline.id).await, StatusCode::NOT_FOUND);
    run(&app, &editor, "posts", "publish", &key, None).await;
    assert_eq!(status(&cover.id).await, StatusCode::OK);
    assert_eq!(status(&inline.id).await, StatusCode::OK);
    let reader = content::reader(&app).await;
    reader.get("/api/v1/content/posts/anh").await.assert_status(StatusCode::OK);
    // An image taken out of the body stops being public with the save.
    update(&app, &editor, "posts", &key, json!({"body": document("x", &[])})).await.assert_status(StatusCode::OK);
    assert_eq!(status(&inline.id).await, StatusCode::NOT_FOUND);
    assert_eq!(status(&cover.id).await, StatusCode::OK);
    run(&app, &editor, "posts", "unpublish", &key, None).await;
    assert_eq!(status(&cover.id).await, StatusCode::NOT_FOUND);
    reader.get("/api/v1/content/posts/anh").await.assert_status(StatusCode::NOT_FOUND);
    // Published again, then trashed.
    run(&app, &editor, "posts", "publish", &key, None).await;
    assert_eq!(status(&cover.id).await, StatusCode::OK);
    editor.send(Method::DELETE, &records(&app, "posts", Some(&key)), None).await.assert_status(StatusCode::NO_CONTENT);
    assert_eq!(status(&cover.id).await, StatusCode::NOT_FOUND);
    editor
        .post_json(&format!("{}/restore", records(&app, "posts", Some(&key))), &json!({}))
        .await
        .assert_status(StatusCode::OK);
    assert_eq!(status(&cover.id).await, StatusCode::OK);
});

db_test!(a_preview_link_opens_the_public_sites_preview, migrate = Migrator, |db: TestDb| async move {
    let app = content::app(db).await;
    let (an, _) = signed_in(&app, "An", AUTHOR_ROLE).await;
    let (binh, _) = signed_in(&app, "Bình", AUTHOR_ROLE).await;
    let admin = {
        let user = user_with_role(&app, "Quản trị", ADMINISTRATOR_ROLE).await;
        app.acting_as(&user).await
    };
    let key = create(&app, &an, "posts", json!({"title": "Nháp xem trước"})).await;
    let link = app.admin_api(&format!("x/previews/posts/{key}"));

    // No public site URL yet: no link, and no token made.
    an.post_json(&link, &json!({})).await.assert_status(StatusCode::OK).assert_json("/url", json!(null));
    save_settings(&app, &admin, "general", json!({"frontend_url": "https://blog.example.com/"})).await;
    save_settings(&app, &admin, "integrations", json!({"preview_path": "/xem-truoc"})).await;
    let made = an.post_json(&link, &json!({})).await;
    made.assert_status(StatusCode::OK);
    let url = made.json()["url"].as_str().unwrap().to_owned();
    let token = url.strip_prefix("https://blog.example.com/xem-truoc?token=").expect("the frontend's preview route");
    assert_eq!(token.len(), 43);
    // The public site reads the draft with it.
    let shown = app.client().get(&format!("/api/v1/preview?token={token}")).await;
    shown.assert_status(StatusCode::OK).assert_json("/preview/status", json!("draft"));
    // Another author's post: as if it did not exist.
    binh.post_json(&link, &json!({})).await.assert_status(StatusCode::NOT_FOUND);
    // A page needs page permissions.
    an.post_json(&app.admin_api("x/previews/pages/1"), &json!({})).await.assert_status(StatusCode::FORBIDDEN);
});
