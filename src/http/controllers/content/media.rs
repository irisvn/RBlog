//! `GET /api/v1/media/{id}/{name}` (public): an image of published content.

use rustcms::admin::media::entity as media;
use rustcms::axum::http::{HeaderValue, header};
use rustcms::axum::response::{IntoResponse, Response};
use rustcms::prelude::*;
use rustcms::storage::StorageKey;
use sea_orm::prelude::Uuid;

use super::throttle_ip;
use crate::services::public_media;

/// Image requests one client address may make per minute.
const PER_MINUTE: u32 = 600;

/// The image `id` while it is public (named by `public_media`, not trashed, an image type). Any
/// other id answers the same 404, whether it exists or not. `name` is cosmetic.
pub async fn show(
    State(app): State<AppState>,
    ClientIp(ip): ClientIp,
    Path((id, _name)): Path<(String, String)>,
) -> AppResult<Response> {
    throttle_ip(&app, "content-media", ip, PER_MINUTE).await?;
    let id = Uuid::parse_str(&id).map_err(|_| AppError::NotFound)?;
    let mut found = public_media::public_images(app.db(), [id]).await?;
    let media = found.remove(&id).ok_or(AppError::NotFound)?;
    serve(&app, &media, "public, max-age=3600").await
}

/// The bytes of `media` with safe headers: its (image) type, never sniffed, sandboxed, cached as
/// `cache_control` says.
pub(crate) async fn serve(app: &AppState, media: &media::Model, cache_control: &'static str) -> AppResult<Response> {
    if !public_media::IMAGE_MIMES.contains(&media.mime.as_str()) {
        return Err(AppError::NotFound);
    }
    let storage = rustcms::storage::of(app).ok_or_else(|| AppError::internal("storage not configured"))?;
    if media.disk != storage.disk() {
        return Err(AppError::internal(format!("media on disk `{}` is not available", media.disk)));
    }
    let (body, len) = storage.open(&StorageKey::parse(&media.key)?).await?;
    let mut response = body.into_response();
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_str(&media.mime).map_err(AppError::internal)?);
    headers.insert(header::CONTENT_LENGTH, len.into());
    headers.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    headers.insert(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static("sandbox; default-src 'none'"));
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static(cache_control));
    Ok(response)
}
