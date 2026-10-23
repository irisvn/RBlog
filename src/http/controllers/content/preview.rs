//! `GET /api/v1/preview?token=…` and `GET /api/v1/preview/media/{id}?token=…` (public: the token is
//! the capability). A token shows one post or page in any status until it expires or is revoked.
//! Every answer is `private, no-store` and sends no referrer (the token is in the URLs).
//!
//! The token travels in the query string, not the path: RustCMS's request span logs the path
//! (never the query), and the token must not reach the logs.

use std::collections::BTreeSet;

use rustcms::axum::http::{HeaderValue, header};
use rustcms::axum::response::{IntoResponse, Response};
use rustcms::prelude::*;
use sea_orm::prelude::Uuid;
use serde::Serialize;

use super::{Params, base_url, throttle_ip};
use crate::http::resources::content::Links;
use crate::http::resources::content::entry::{self, Detail};
use crate::services::preview::{self, PreviewSubject};
use crate::services::public_media::{self, body_media_ids};

/// Preview requests one client address may make per minute (JSON and images together).
const PER_MINUTE: u32 = 300;

/// The post or page, as the Content API shows it, plus `preview`.
#[derive(Serialize)]
struct PreviewJson<T: Serialize> {
    #[serde(flatten)]
    entry: T,
    preview: PreviewMeta,
}

/// What is previewed: `{type: "post" | "page", status}`.
#[derive(Serialize)]
struct PreviewMeta {
    #[serde(rename = "type")]
    kind: &'static str,
    status: String,
}

/// `response` with the headers every preview answer carries, errors included.
fn private(mut response: Response) -> Response {
    let headers = response.headers_mut();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("private, no-store"));
    headers.insert(header::REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
    response
}

/// The previewed post or page (`?include=document` adds the document). Unknown, expired and
/// revoked tokens, and trashed subjects, answer 404.
pub async fn show(
    State(app): State<AppState>,
    ClientIp(ip): ClientIp,
    Query(query): Query<Vec<(String, String)>>,
) -> Response {
    private(entry_of(&app, ip, query).await.unwrap_or_else(IntoResponse::into_response))
}

async fn entry_of(app: &AppState, ip: std::net::IpAddr, query: Vec<(String, String)>) -> AppResult<Response> {
    throttle_ip(app, "content-preview", ip, PER_MINUTE).await?;
    let params = Params::new(query, &["token", "include"])?;
    let detail = Detail { html: true, document: params.include_document()? };
    let token = params.get("token").unwrap_or_default();
    let db = app.db();
    let subject = preview::find(db, token).await?.ok_or(AppError::NotFound)?;
    let base = base_url(app);
    let links = Links::Preview { base: &base, token };
    Ok(match subject {
        PreviewSubject::Post(id) => {
            let found = preview::post(db, id).await?.ok_or(AppError::NotFound)?;
            let status = found.status.clone();
            let entry = entry::posts(db, &links, std::slice::from_ref(&found), detail).await?.remove(0);
            Json(PreviewJson { entry, preview: PreviewMeta { kind: "post", status } }).into_response()
        }
        PreviewSubject::Page(id) => {
            let found = preview::page(db, id).await?.ok_or(AppError::NotFound)?;
            let status = found.status.clone();
            let entry = entry::pages(db, &links, std::slice::from_ref(&found), detail).await?.remove(0);
            Json(PreviewJson { entry, preview: PreviewMeta { kind: "page", status } }).into_response()
        }
    })
}

/// An image of the previewed subject: its cover, its OG image or an image of its body, not
/// trashed. Any other id answers 404, even one public elsewhere.
pub async fn media(
    State(app): State<AppState>,
    ClientIp(ip): ClientIp,
    Path(id): Path<String>,
    Query(query): Query<Vec<(String, String)>>,
) -> Response {
    private(media_of(&app, ip, &id, query).await.unwrap_or_else(IntoResponse::into_response))
}

async fn media_of(app: &AppState, ip: std::net::IpAddr, id: &str, query: Vec<(String, String)>) -> AppResult<Response> {
    throttle_ip(app, "content-preview", ip, PER_MINUTE).await?;
    let params = Params::new(query, &["token"])?;
    let token = params.get("token").unwrap_or_default();
    let id = Uuid::parse_str(id).map_err(|_| AppError::NotFound)?;
    let db = app.db();
    let subject = preview::find(db, token).await?.ok_or(AppError::NotFound)?;
    let used: BTreeSet<Uuid> = match subject {
        PreviewSubject::Post(post_id) => {
            let found = preview::post(db, post_id).await?.ok_or(AppError::NotFound)?;
            found
                .cover_media_id
                .into_iter()
                .chain(found.og_media_id)
                .chain(body_media_ids(found.body.as_ref()))
                .collect()
        }
        PreviewSubject::Page(page_id) => {
            let found = preview::page(db, page_id).await?.ok_or(AppError::NotFound)?;
            found
                .cover_media_id
                .into_iter()
                .chain(found.og_media_id)
                .chain(body_media_ids(found.body.as_ref()))
                .collect()
        }
    };
    if !used.contains(&id) {
        return Err(AppError::NotFound);
    }
    let media = public_media::images(db, [id]).await?.remove(&id).ok_or(AppError::NotFound)?;
    super::media::serve(app, &media, "private, no-store").await
}
