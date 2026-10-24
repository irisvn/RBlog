//! Rich text version 2 (RustCMS 0.10) through the panel and the Content API: figures with sizes
//! and captions, embeds, tables and callouts in `html`, `outline`, search inside tables and
//! callouts, and the public images nested in a callout. Version 1 documents still render.

use rblog::database::migrations::Migrator;
use rblog::models::post;
use rblog::services::preview::{self, PreviewSubject};
use rustcms::axum::http::StatusCode;
use rustcms_testing::{TestDb, db_test};
use sea_orm::{ActiveModelTrait, Set};
use serde_json::{Value, json};

use crate::support::content::{self, APP_URL, stored_media};
use crate::support::posts::*;

fn text(text: &str) -> Value {
    json!({"type": "text", "text": text})
}

fn paragraph(t: &str) -> Value {
    json!({"type": "paragraph", "content": [text(t)]})
}

fn heading(level: u8, t: &str) -> Value {
    json!({"type": "heading", "attrs": {"level": level}, "content": [text(t)]})
}

fn cell(kind: &str, t: &str) -> Value {
    json!({"type": kind, "content": [paragraph(t)]})
}

/// A version 2 document: headings, a table, an embed, a callout holding a list with an image
/// (`nested`) and a captioned wide image (`figure`).
fn v2_body(figure: &str, nested: &str) -> Value {
    json!({"type": "doc", "version": 2, "content": [
        heading(2, "Giới thiệu"),
        paragraph("Mở đầu"),
        {"type": "image", "attrs": {"media": figure, "alt": "Bát phở", "caption": "Phở Hà Nội", "width": "wide"}},
        heading(3, "Bảng giá"),
        {"type": "table", "content": [
            {"type": "tableRow", "content": [cell("tableHeader", "Món"), cell("tableHeader", "Giá")]},
            {"type": "tableRow", "content": [cell("tableCell", "Phở đặc biệt"), cell("tableCell", "50k")]}
        ]},
        {"type": "embed", "attrs": {"provider": "youtube", "id": "dQw4w9WgXcQ"}},
        {"type": "callout", "attrs": {"tone": "info"}, "content": [
            paragraph("Mẹo nấu nước dùng"),
            {"type": "bulletList", "content": [{"type": "listItem", "content": [
                paragraph("Hành nướng"),
                {"type": "image", "attrs": {"media": nested, "alt": "Hành"}}
            ]}]}
        ]},
        heading(2, "Giới thiệu")
    ]})
}

/// The slugs the Content API lists for `?q=` (percent-encoded here).
async fn found(reader: &rustcms_testing::TestClient<'_>, q: &str) -> Vec<String> {
    let q: String = q
        .bytes()
        .map(|b| if b.is_ascii_alphanumeric() { char::from(b).to_string() } else { format!("%{b:02X}") })
        .collect();
    let listed = reader.get(&format!("/api/v1/content/posts?q={q}")).await.json();
    listed["data"].as_array().unwrap().iter().map(|p| p["slug"].as_str().unwrap().to_owned()).collect()
}

db_test!(version_2_posts_render_and_search_their_new_nodes, migrate = Migrator, |db: TestDb| async move {
    let app = content::app(db).await;
    let db = app.state().db();
    let (editor, _) = signed_in(&app, "Biên", EDITOR_ROLE).await;
    let figure = stored_media(&app, "pho.png", "image/png", b"\x89PNG figure").await;
    let nested = stored_media(&app, "hanh.png", "image/png", b"\x89PNG nested").await;
    let late = stored_media(&app, "late.png", "image/png", b"\x89PNG late").await;
    let body = v2_body(&figure.id.to_string(), &nested.id.to_string());
    let key = create(&app, &editor, "posts", json!({"title": "Phở", "slug": "pho", "body": body})).await;

    // Search and reading time count table cells, callouts and captions (not embeds).
    let row = post_row(db, &key).await;
    let search = row.search_text.clone().unwrap();
    for part in ["mon gia pho dac biet 50k", "meo nau nuoc dung hanh nuong", "pho ha noi"] {
        assert!(search.contains(part), "{part} in {search}");
    }
    assert!(!search.contains("dqw4w9wgxcq"), "{search}");

    // A draft: its preview shows the nested image at the token's URL; nothing is public.
    let token = preview::issue(db, PreviewSubject::Post(row.id), row.author_id).await.unwrap();
    let client = app.client();
    let previewed = client.get(&format!("/api/v1/preview?token={token}")).await;
    previewed.assert_status(StatusCode::OK);
    let preview_url = format!("{APP_URL}/api/v1/preview/media/{}?token={token}", nested.id);
    let html = previewed.json()["html"].as_str().unwrap().to_owned();
    assert!(
        html.contains(&format!("<img src=\"{preview_url}\" width=\"1600\" height=\"900\" alt=\"Hành\">")),
        "{html}"
    );
    previewed.assert_json("/outline/1", json!({"level": 3, "text": "Bảng giá", "id": "bang-gia"}));
    let nested_image = client.get(&format!("/api/v1/preview/media/{}?token={token}", nested.id)).await;
    nested_image.assert_status(StatusCode::OK);
    let public_nested = format!("/api/v1/media/{}/hanh.png", nested.id);
    client.get(&public_nested).await.assert_status(StatusCode::NOT_FOUND);

    let reader = content::reader(&app).await;
    assert!(found(&reader, "dac biet").await.is_empty());
    run(&app, &editor, "posts", "publish", &key, None).await;
    // Text inside a table cell and inside a callout is found.
    assert_eq!(found(&reader, "phở đặc biệt").await, ["pho"]);
    assert_eq!(found(&reader, "nuoc dung").await, ["pho"]);
    assert_eq!(found(&reader, "ha noi").await, ["pho"], "captions are searched");

    // `html`: a figure with its size and caption, the table, the embed and the callout.
    let shown = reader.get("/api/v1/content/posts/pho").await;
    shown.assert_status(StatusCode::OK);
    let shown = shown.json();
    let html = shown["html"].as_str().unwrap();
    let url = |m: &rustcms::admin::media::entity::Model| format!("{APP_URL}/api/v1/media/{}/{}", m.id, m.name);
    let expected = [
        "<h2 id=\"gioi-thieu\">Giới thiệu</h2><p>Mở đầu</p>".to_owned(),
        format!(
            "<figure data-width=\"wide\"><img src=\"{}\" width=\"1600\" height=\"900\" alt=\"Bát phở\">\
             <figcaption>Phở Hà Nội</figcaption></figure>",
            url(&figure)
        ),
        "<h3 id=\"bang-gia\">Bảng giá</h3>".to_owned(),
        "<table><thead><tr><th><p>Món</p></th><th><p>Giá</p></th></tr></thead>\
         <tbody><tr><td><p>Phở đặc biệt</p></td><td><p>50k</p></td></tr></tbody></table>"
            .to_owned(),
        "<figure data-embed=\"youtube\" data-id=\"dQw4w9WgXcQ\">\
         <a href=\"https://www.youtube.com/watch?v=dQw4w9WgXcQ\" rel=\"noopener noreferrer\">\
         https://www.youtube.com/watch?v=dQw4w9WgXcQ</a></figure>"
            .to_owned(),
        format!(
            "<aside class=\"callout\" data-tone=\"info\"><p>Mẹo nấu nước dùng</p><ul><li><p>Hành nướng</p>\
             <figure><img src=\"{}\" width=\"1600\" height=\"900\" alt=\"Hành\"></figure></li></ul></aside>",
            url(&nested)
        ),
        "<h2 id=\"gioi-thieu-2\">Giới thiệu</h2>".to_owned(),
    ];
    assert_eq!(html, expected.concat());
    assert_eq!(
        shown["outline"],
        json!([
            {"level": 2, "text": "Giới thiệu", "id": "gioi-thieu"},
            {"level": 3, "text": "Bảng giá", "id": "bang-gia"},
            {"level": 2, "text": "Giới thiệu", "id": "gioi-thieu-2"}
        ])
    );
    // Lists carry neither `html` nor `outline`; `?include=document` is the stored document.
    let listed = reader.get("/api/v1/content/posts").await.json();
    assert!(listed["data"][0].get("outline").is_none() && listed["data"][0].get("html").is_none());
    reader.get("/api/v1/content/posts/pho?include=document").await.assert_json("/document", body.clone());

    // The image nested in the callout is public while the post is.
    client.get(&public_nested).await.assert_status(StatusCode::OK);
    // An image the public rows do not name yet (written without a sync) is left out of `html`.
    let mut with_late = body.clone();
    with_late["content"][6]["content"][1]["content"][0]["content"]
        .as_array_mut()
        .unwrap()
        .push(json!({"type": "image", "attrs": {"media": late.id.to_string(), "alt": "Muộn"}}));
    let mut edited: post::ActiveModel = post_row(db, &key).await.into();
    edited.body = Set(Some(with_late));
    edited.update(db).await.unwrap();
    let html = reader.get("/api/v1/content/posts/pho").await.json()["html"].as_str().unwrap().to_owned();
    assert!(!html.contains(&late.id.to_string()) && html.contains(&nested.id.to_string()), "{html}");

    run(&app, &editor, "posts", "unpublish", &key, None).await;
    client.get(&public_nested).await.assert_status(StatusCode::NOT_FOUND);
    client.get(&format!("/api/v1/media/{}/pho.png", figure.id)).await.assert_status(StatusCode::NOT_FOUND);
});

db_test!(version_1_posts_still_render, migrate = Migrator, |db: TestDb| async move {
    let app = content::app(db).await;
    let (editor, _) = signed_in(&app, "Biên", EDITOR_ROLE).await;
    let photo = stored_media(&app, "anh.png", "image/png", b"\x89PNG v1").await;
    let body = json!({"type": "doc", "version": 1, "content": [
        heading(2, "Đà Nẵng"),
        paragraph("Biển"),
        {"type": "image", "attrs": {"media": photo.id.to_string(), "alt": "Biển"}}
    ]});
    let key = create(&app, &editor, "posts", json!({"title": "Biển", "slug": "bien", "body": body})).await;
    run(&app, &editor, "posts", "publish", &key, None).await;

    let reader = content::reader(&app).await;
    let shown = reader.get("/api/v1/content/posts/bien?include=document").await;
    shown.assert_status(StatusCode::OK);
    let url = format!("{APP_URL}/api/v1/media/{}/anh.png", photo.id);
    shown
        .assert_json(
            "/html",
            json!(format!(
                "<h2 id=\"da-nang\">Đà Nẵng</h2><p>Biển</p>\
                 <figure><img src=\"{url}\" width=\"1600\" height=\"900\" alt=\"Biển\"></figure>"
            )),
        )
        .assert_json("/outline", json!([{"level": 2, "text": "Đà Nẵng", "id": "da-nang"}]))
        .assert_json("/document", body);
});
