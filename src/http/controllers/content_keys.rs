//! Content API keys on the Integrations page (`/admin/p/content-api`): listed, made (the secret
//! answered once) and revoked. Routes and their access are in `crate::routes::admin`; the rules
//! live in `crate::services::content_api`.

use rustcms::audit::{AuditActor, AuditContext, RequestMeta};
use rustcms::auth::entities::personal_access_token as token;
use rustcms::auth::tokens;
use rustcms::axum::http::header::CACHE_CONTROL;
use rustcms::axum::response::{IntoResponse, Response};
use rustcms::prelude::*;
use sea_orm::prelude::DateTimeUtc;
use serde::Serialize;

use super::content::base_url;
use crate::http::requests::content_key_request::NewContentKey;
use crate::services::content_api::{
    content_keys, frontend_user, frontend_user_or_create, issue_content_key, revoke_content_key,
};

/// A key as listed: never its secret (only its hash is stored).
#[derive(Debug, Serialize)]
pub struct ContentKeyJson {
    pub id: String,
    pub name: String,
    pub created_at: DateTimeUtc,
    pub last_used_at: Option<DateTimeUtc>,
    pub expires_at: Option<DateTimeUtc>,
}

impl From<token::Model> for ContentKeyJson {
    fn from(row: token::Model) -> Self {
        Self {
            id: row.id.to_string(),
            name: row.name,
            created_at: row.created_at,
            last_used_at: row.last_used_at,
            expires_at: row.expires_at,
        }
    }
}

/// A page of keys, and the Content API's base URL (`APP_URL/api/v1/content`) for the usage notes.
#[derive(Debug, Serialize)]
pub struct ContentKeysJson {
    #[serde(flatten)]
    pub page: ApiPage<ContentKeyJson>,
    pub base_url: String,
}

/// `GET …/integrations/content-keys?page=&per_page=`: the live keys, newest first.
pub async fn index(State(app): State<AppState>, page: Pagination) -> AppResult<Json<ContentKeysJson>> {
    let db = app.db();
    // Before the first key the frontend user may not exist yet: nothing to list (no user has id 0).
    let frontend_id = frontend_user(db).await?.map_or(0, |f| f.id);
    let page = ApiPage::fetch(content_keys(frontend_id), db, &page, ContentKeyJson::from).await?;
    Ok(Json(ContentKeysJson { page, base_url: format!("{}/api/v1/content", base_url(&app)) }))
}

/// `POST …/integrations/content-keys`: a new key. Its secret is in this answer only, which no
/// cache keeps (`Cache-Control: private, no-store`, as the panel answers anyway).
pub async fn store(
    State(app): State<AppState>,
    auth: Auth,
    request: RequestMeta,
    Validated(input): Validated<NewContentKey>,
) -> AppResult<Response> {
    let db = app.db();
    frontend_user_or_create(&app).await?;
    let expires_at = match input.expires_in_days {
        Some(days) => {
            Some(tokens::expiry_in_days(days).ok_or_else(|| ValidationErrors::field("expires_in_days", "is invalid"))?)
        }
        None => None,
    };
    let audit = AuditContext::new(AuditActor::user(auth.principal()), request);
    let issued = issue_content_key(db, &input.name, expires_at, &audit).await?;
    let body = json!({ "token": issued.plain_text, "key": ContentKeyJson::from(issued.token) });
    Ok((StatusCode::CREATED, [(CACHE_CONTROL, "private, no-store")], Json(body)).into_response())
}

/// `DELETE …/integrations/content-keys/{id}`: revokes a key at once. Anything but a live Content
/// API key (another user's token, a revoked or unknown one) is 404.
pub async fn destroy(
    State(app): State<AppState>,
    auth: Auth,
    request: RequestMeta,
    Path(id): Path<i64>,
) -> AppResult<StatusCode> {
    let audit = AuditContext::new(AuditActor::user(auth.principal()), request);
    revoke_content_key(app.db(), id, &audit).await?;
    Ok(StatusCode::NO_CONTENT)
}
