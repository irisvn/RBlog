//! Images of the Content API: `public_media` (sync and backfill), image URLs in `html`, the public
//! media route, and previews with their own media route.

use std::collections::BTreeSet;

use rblog::database::migrations::Migrator;
use rblog::models::{author_profile, page, post, preview_token, public_media};
use rblog::services::preview::{self, PreviewSubject};
use rblog::services::public_media::{Subject, backfill, sync};
use rustcms::admin::media::entity as media;
use rustcms::auth::secret::sha256_hex;
use rustcms::axum::http::StatusCode;
use rustcms_testing::{TestApp, TestDb, TestResponse, db_test};
use sea_orm::prelude::Uuid;
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set};
use serde_json::json;

use crate::support::content::{self, APP_URL, ago, author, body, page as new_page, post as new_post, stored_media};

/// The `public_media` rows of a subject.
async fn rows(db: &DatabaseConnection, subject: Subject, id: i64) -> BTreeSet<Uuid> {
    public_media::Entity::find()
        .filter(public_media::Column::SubjectType.eq(subject.as_str()))
        .filter(public_media::Column::SubjectId.eq(id))
        .all(db)
        .await
        .unwrap()
        .into_iter()
        .map(|r| r.media_id)
        .collect()
}

async fn png(app: &TestApp, name: &str) -> media::Model {
    stored_media(app, name, "image/png", b"\x89PNG fake image").await
}

async fn trash(db: &DatabaseConnection, media: &media::Model) {
    let mut trashed: media::ActiveModel = media.clone().into();
    trashed.deleted_at = Set(Some(ago(0)));
    trashed.update(db).await.unwrap();
}

db_test!(public_media_follows_what_is_published, migrate = Migrator, |db: TestDb| async move {
    let app = content::app(db).await;
    let db = app.state().db();
    let an = author(db, "An", Some("an")).await;
    let binh = author(db, "Bình", None).await;
    let ids: Vec<Uuid> = (0..7).map(|_| Uuid::now_v7()).collect();
    let [cover, og, inline, draft_cover, page_inline, avatar, new_avatar] = ids[..] else { unreachable!() };
    let set_avatar = |id: Uuid| async move {
        let mut profile: author_profile::ActiveModel =
            author_profile::Entity::find_by_id(an.id).one(db).await.unwrap().unwrap().into();
        profile.avatar_media_id = Set(Some(id));
        profile.update(db).await.unwrap();
    };
    set_avatar(avatar).await;

    let mut live = new_post(&an, "live", "Live", 1);
    live.cover_media_id = Set(Some(cover));
    live.og_media_id = Set(Some(og));
    live.body = Set(Some(body("text", &[inline, cover])));
    let live = live.insert(db).await.unwrap();
    let mut draft = new_post(&an, "draft", "Draft", 1);
    draft.status = Set("draft".into());
    draft.cover_media_id = Set(Some(draft_cover));
    let draft = draft.insert(db).await.unwrap();
    let mut about = new_page(&an, "about", 1);
    about.body = Set(Some(body("text", &[page_inline])));
    let about = about.insert(db).await.unwrap();

    sync(db, Subject::Post, live.id).await.unwrap();
    sync(db, Subject::Post, draft.id).await.unwrap();
    sync(db, Subject::Page, about.id).await.unwrap();
    // A public post's rows name its media and its author's avatar.
    assert_eq!(rows(db, Subject::Post, live.id).await, BTreeSet::from([cover, og, inline, avatar]));
    assert_eq!(rows(db, Subject::Post, draft.id).await, BTreeSet::new());
    assert_eq!(rows(db, Subject::Page, about.id).await, BTreeSet::from([page_inline]));
    // Syncing again changes nothing.
    sync(db, Subject::Post, live.id).await.unwrap();
    assert_eq!(rows(db, Subject::Post, live.id).await, BTreeSet::from([cover, og, inline, avatar]));
    // A new avatar: syncing the author syncs their posts.
    set_avatar(new_avatar).await;
    sync(db, Subject::Author, an.id).await.unwrap();
    assert_eq!(rows(db, Subject::Post, live.id).await, BTreeSet::from([cover, og, inline, new_avatar]));
    assert_eq!(rows(db, Subject::Post, draft.id).await, BTreeSet::new());

    // Another author: the old author's avatar is no longer public through this post.
    let mut moved: post::ActiveModel = live.clone().into();
    moved.author_id = Set(binh.id);
    let moved = moved.update(db).await.unwrap();
    sync(db, Subject::Post, live.id).await.unwrap();
    assert_eq!(rows(db, Subject::Post, live.id).await, BTreeSet::from([cover, og, inline]));

    // Back to draft: nothing of it stays public.
    let mut unpublished: post::ActiveModel = moved.into();
    unpublished.status = Set("draft".into());
    let unpublished = unpublished.update(db).await.unwrap();
    sync(db, Subject::Post, live.id).await.unwrap();
    assert_eq!(rows(db, Subject::Post, live.id).await, BTreeSet::new());

    // A body change drops the images it no longer shows.
    let mut republished: post::ActiveModel = unpublished.into();
    republished.status = Set("published".into());
    republished.author_id = Set(an.id);
    republished.body = Set(Some(body("text", &[])));
    republished.og_media_id = Set(None);
    republished.update(db).await.unwrap();
    sync(db, Subject::Post, live.id).await.unwrap();
    assert_eq!(rows(db, Subject::Post, live.id).await, BTreeSet::from([cover, new_avatar]));

    // Backfill: rebuilds every post and page and drops the rows of deleted subjects.
    let stale = |media_id: Uuid, subject_type: &str, subject_id: i64| public_media::ActiveModel {
        media_id: Set(media_id),
        subject_type: Set(subject_type.into()),
        subject_id: Set(subject_id),
    };
    stale(draft_cover, "post", draft.id).insert(db).await.unwrap();
    stale(og, "post", 999_999).insert(db).await.unwrap();
    stale(og, "page", 999_999).insert(db).await.unwrap();
    stale(og, "author", an.id).insert(db).await.unwrap();
    // The rows of `live` were lost: the backfill restores them.
    public_media::Entity::delete_many()
        .filter(public_media::Column::SubjectType.eq("post"))
        .filter(public_media::Column::SubjectId.eq(live.id))
        .exec(db)
        .await
        .unwrap();
    let mut trashed: page::ActiveModel = about.into();
    trashed.deleted_at = Set(Some(ago(0)));
    let about = trashed.update(db).await.unwrap();

    let report = backfill(db).await.unwrap();
    assert_eq!(report.orphans, 3);
    assert_eq!(report.subjects, 3, "two posts and a page");
    assert_eq!(rows(db, Subject::Post, live.id).await, BTreeSet::from([cover, new_avatar]));
    assert_eq!(rows(db, Subject::Post, draft.id).await, BTreeSet::new());
    assert_eq!(rows(db, Subject::Page, about.id).await, BTreeSet::new());
    assert_eq!(public_media::Entity::find().all(db).await.unwrap().len(), 2);
});

/// Asserts the headers of a served image.
fn assert_image(response: &TestResponse, bytes: &[u8], cache: &str) {
    response.assert_status(StatusCode::OK);
    assert_eq!(&response.body[..], bytes);
    assert_eq!(response.header("content-type").as_deref(), Some("image/png"));
    assert_eq!(response.header("content-length"), Some(bytes.len().to_string()));
    assert_eq!(response.header("x-content-type-options").as_deref(), Some("nosniff"));
    assert_eq!(response.header("cache-control").as_deref(), Some(cache));
    assert_eq!(response.header("content-security-policy").as_deref(), Some("sandbox; default-src 'none'"));
}

db_test!(published_images_are_public_and_others_are_not, migrate = Migrator, |db: TestDb| async move {
    let app = content::app(db).await;
    let db = app.state().db();
    let an = author(db, "An", Some("an")).await;
    let photo = png(&app, "ảnh bìa.png").await;
    let avatar = png(&app, "avatar.png").await;
    let mut profile: author_profile::ActiveModel =
        author_profile::Entity::find_by_id(an.id).one(db).await.unwrap().unwrap().into();
    profile.avatar_media_id = Set(Some(avatar.id));
    profile.update(db).await.unwrap();
    let inline = png(&app, "inline.png").await;
    let trashed = png(&app, "trashed.png").await;
    let late = png(&app, "late.png").await;
    let draft_only = png(&app, "draft.png").await;
    let pdf = stored_media(&app, "doc.pdf", "application/pdf", b"%PDF-1.4").await;

    let mut live = new_post(&an, "live", "Live", 1);
    live.cover_media_id = Set(Some(photo.id));
    live.og_media_id = Set(Some(pdf.id));
    live.body = Set(Some(body("Xin chào", &[inline.id, trashed.id])));
    let live = live.insert(db).await.unwrap();
    let mut draft = new_post(&an, "draft", "Draft", 1);
    draft.status = Set("draft".into());
    draft.cover_media_id = Set(Some(draft_only.id));
    let draft = draft.insert(db).await.unwrap();
    sync(db, Subject::Post, live.id).await.unwrap();
    sync(db, Subject::Post, draft.id).await.unwrap();
    trash(db, &trashed).await;
    // An image added after the last sync is not public yet.
    let mut edited: post::ActiveModel = live.into();
    edited.body = Set(Some(body("Xin chào", &[inline.id, trashed.id, late.id])));
    edited.update(db).await.unwrap();

    let reader = content::reader(&app).await;
    let shown = reader.get("/api/v1/content/posts/live").await;
    shown.assert_status(StatusCode::OK);
    let shown = shown.json();
    let inline_url = format!("{APP_URL}/api/v1/media/{}/inline.png", inline.id);
    assert_eq!(shown["html"], json!(format!("<p>Xin chào</p><img src=\"{inline_url}\" alt=\"hình\">")));
    let photo_url = format!("{APP_URL}/api/v1/media/{}/nh-b-a.png", photo.id);
    assert_eq!(shown["cover"], json!({"url": photo_url, "width": 1600, "height": 900, "alt": "ảnh bìa.png"}));
    assert_eq!(shown["seo"]["og_image"], json!(null), "a PDF is not an image");
    let listed = reader.get("/api/v1/content/posts").await.json();
    assert_eq!(listed["data"][0]["cover"]["url"], json!(photo_url));
    reader
        .get("/api/v1/content/posts/live?include=document")
        .await
        .assert_json("/document/content/1/attrs/media", json!(inline.id.to_string()));

    let avatar_url = format!("{APP_URL}/api/v1/media/{}/avatar.png", avatar.id);
    reader.get("/api/v1/content/authors/an").await.assert_json("/avatar/url", json!(avatar_url));

    // Public: no key needed, the name is cosmetic.
    let client = app.client();
    assert_image(
        &client.get(&format!("/api/v1/media/{}/a.png", avatar.id)).await,
        b"\x89PNG fake image",
        "public, max-age=3600",
    );
    assert_image(
        &client.get(&format!("/api/v1/media/{}/anything.jpg", photo.id)).await,
        b"\x89PNG fake image",
        "public, max-age=3600",
    );
    assert_image(
        &client.get(&format!("/api/v1/media/{}/x", inline.id)).await,
        b"\x89PNG fake image",
        "public, max-age=3600",
    );

    // Not public, trashed, not an image or unknown: the same 404.
    let missing = client.get(&format!("/api/v1/media/{}/x.png", Uuid::now_v7())).await;
    missing.assert_status(StatusCode::NOT_FOUND);
    for id in
        [draft_only.id.to_string(), trashed.id.to_string(), pdf.id.to_string(), late.id.to_string(), "nope".into()]
    {
        let refused = client.get(&format!("/api/v1/media/{id}/x.png")).await;
        refused.assert_status(StatusCode::NOT_FOUND);
        assert_eq!(refused.body, missing.body, "{id}");
        assert_eq!(refused.header("content-type"), missing.header("content-type"));
    }
});

db_test!(public_routes_are_rate_limited_per_address, migrate = Migrator, |db: TestDb| async move {
    let app = content::app(db).await;
    let first = app.client().from("203.0.113.7:4000".parse().unwrap());
    let second = app.client().from("203.0.113.8:4000".parse().unwrap());
    // Unknown ids: refused after the count, without a query.
    for _ in 0..600 {
        first.get("/api/v1/media/nope/x.png").await.assert_status(StatusCode::NOT_FOUND);
    }
    first.get("/api/v1/media/nope/x.png").await.assert_status(StatusCode::TOO_MANY_REQUESTS);
    second.get("/api/v1/media/nope/x.png").await.assert_status(StatusCode::NOT_FOUND);
    for _ in 0..300 {
        second.get("/api/v1/preview?token=short").await.assert_status(StatusCode::NOT_FOUND);
    }
    let limited = second.get("/api/v1/preview?token=short").await;
    assert_private(&limited, StatusCode::TOO_MANY_REQUESTS);
    assert!(limited.header("retry-after").is_some());
});

/// A preview answer: private, no referrer.
fn assert_private(response: &TestResponse, status: StatusCode) {
    response.assert_status(status);
    assert_eq!(response.header("cache-control").as_deref(), Some("private, no-store"));
    assert_eq!(response.header("referrer-policy").as_deref(), Some("no-referrer"));
}

db_test!(preview_tokens_show_one_subject_and_its_images, migrate = Migrator, |db: TestDb| async move {
    let app = content::app(db).await;
    let db = app.state().db();
    let an = author(db, "An", Some("an")).await;
    let cover = png(&app, "cover.png").await;
    let inline = png(&app, "inline.png").await;
    let trashed = png(&app, "trashed.png").await;
    let only_b = png(&app, "only-b.png").await;
    let pdf = stored_media(&app, "doc.pdf", "application/pdf", b"%PDF-1.4").await;

    let mut a = new_post(&an, "post-a", "Bài A", 0);
    a.status = Set("draft".into());
    a.published_at = Set(None);
    a.cover_media_id = Set(Some(cover.id));
    a.og_media_id = Set(Some(pdf.id));
    a.body = Set(Some(body("Bản nháp", &[inline.id, trashed.id])));
    let a = a.insert(db).await.unwrap();
    let mut b = new_post(&an, "post-b", "Bài B", 0);
    b.status = Set("pending".into());
    b.body = Set(Some(body("B", &[only_b.id])));
    let b = b.insert(db).await.unwrap();
    let mut gone = new_post(&an, "gone", "Gone", 0);
    gone.deleted_at = Set(Some(ago(0)));
    let gone = gone.insert(db).await.unwrap();
    let mut about = new_page(&an, "about", 0);
    about.status = Set("scheduled".into());
    let about = about.insert(db).await.unwrap();
    trash(db, &trashed).await;

    let token = preview::issue(db, PreviewSubject::Post(a.id), an.id).await.unwrap();
    assert!(token.len() >= 43 && token.bytes().all(|c| c.is_ascii_alphanumeric()));
    // Only the hash is stored.
    let stored = preview_token::Entity::find().one(db).await.unwrap().unwrap();
    assert_eq!(stored.token_hash, sha256_hex(&token));
    assert!(stored.expires_at > ago(0) && stored.expires_at <= ago(-1));

    let client = app.client();
    let shown = client.get(&format!("/api/v1/preview?token={token}")).await;
    assert_private(&shown, StatusCode::OK);
    let shown = shown.json();
    assert_eq!(shown["title"], json!("Bài A"));
    assert_eq!(shown["preview"], json!({"type": "post", "status": "draft"}));
    assert_eq!(shown["published_at"], json!(null));
    let media_url = |id: Uuid| format!("{APP_URL}/api/v1/preview/media/{id}?token={token}");
    assert_eq!(shown["cover"]["url"], json!(media_url(cover.id)));
    assert_eq!(shown["html"], json!(format!("<p>Bản nháp</p><img src=\"{}\" alt=\"hình\">", media_url(inline.id))));
    assert_eq!(shown["seo"]["og_image"], json!(null));
    assert_eq!(shown["author"], json!({"slug": "an", "name": "An"}));

    for id in [cover.id, inline.id] {
        let image = client.get(&format!("/api/v1/preview/media/{id}?token={token}")).await;
        assert_image(&image, b"\x89PNG fake image", "private, no-store");
        assert_eq!(image.header("referrer-policy").as_deref(), Some("no-referrer"));
        // Still a draft: not public.
        client.get(&format!("/api/v1/media/{id}/x.png")).await.assert_status(StatusCode::NOT_FOUND);
    }
    // Post A's token cannot fetch post B's image, a trashed one or a non-image.
    for id in [only_b.id.to_string(), trashed.id.to_string(), pdf.id.to_string(), "nope".into()] {
        assert_private(&client.get(&format!("/api/v1/preview/media/{id}?token={token}")).await, StatusCode::NOT_FOUND);
    }
    let token_b = preview::issue(db, PreviewSubject::Post(b.id), an.id).await.unwrap();
    assert_image(
        &client.get(&format!("/api/v1/preview/media/{}?token={token_b}", only_b.id)).await,
        b"\x89PNG fake image",
        "private, no-store",
    );
    client
        .get(&format!("/api/v1/preview/media/{}?token={token_b}", cover.id))
        .await
        .assert_status(StatusCode::NOT_FOUND);

    // Pages too.
    let page_token = preview::issue(db, PreviewSubject::Page(about.id), an.id).await.unwrap();
    client
        .get(&format!("/api/v1/preview?token={page_token}"))
        .await
        .assert_status(StatusCode::OK)
        .assert_json("/preview", json!({"type": "page", "status": "scheduled"}))
        .assert_json("/slug", json!("about"));

    // Revoked, expired, unknown, malformed tokens and trashed subjects: 404.
    let revoked = preview::issue(db, PreviewSubject::Post(a.id), an.id).await.unwrap();
    client.get(&format!("/api/v1/preview?token={revoked}")).await.assert_status(StatusCode::OK);
    let row = preview_token::Entity::find()
        .filter(preview_token::Column::TokenHash.eq(sha256_hex(&revoked)))
        .one(db)
        .await
        .unwrap()
        .unwrap();
    preview::revoke(db, row.id).await.unwrap();
    let expired = preview::issue(db, PreviewSubject::Post(a.id), an.id).await.unwrap();
    preview_token::Entity::update_many()
        .col_expr(preview_token::Column::ExpiresAt, sea_orm::sea_query::Expr::value(ago(1)))
        .filter(preview_token::Column::TokenHash.eq(sha256_hex(&expired)))
        .exec(db)
        .await
        .unwrap();
    let trashed_subject = preview::issue(db, PreviewSubject::Post(gone.id), an.id).await.unwrap();
    let unknown = "A".repeat(43);
    for bad in [revoked.as_str(), expired.as_str(), trashed_subject.as_str(), unknown.as_str(), "short"] {
        assert_private(&client.get(&format!("/api/v1/preview?token={bad}")).await, StatusCode::NOT_FOUND);
        let image = client.get(&format!("/api/v1/preview/media/{}?token={bad}", cover.id)).await;
        assert_private(&image, StatusCode::NOT_FOUND);
    }
    assert_private(&client.get("/api/v1/preview").await, StatusCode::NOT_FOUND);
    assert_private(&client.get(&format!("/api/v1/preview/media/{}", cover.id)).await, StatusCode::NOT_FOUND);
});
