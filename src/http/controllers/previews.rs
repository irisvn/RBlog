//! Preview links of the post and page pages (the `rblog/content-preview` slot): a token for one
//! post or page, and the public site's preview URL that carries it. Routes and their access are
//! in `crate::routes::admin`; the rules live in `crate::services::preview::issue_link`.

use rustcms::admin::AdminCtx;
use rustcms::prelude::*;
use serde_json::{Value, json};

use crate::services::content::Kind;
use crate::services::preview;

/// `POST …/previews/posts/{id}`: a preview link of a post the user may view.
pub async fn post(
    State(app): State<AppState>,
    AdminCtx(ctx): AdminCtx,
    Path(id): Path<String>,
) -> AppResult<Json<Value>> {
    link(&app, &ctx, Kind::Post, &id).await
}

/// `POST …/previews/pages/{id}`: a preview link of a page the user may view.
pub async fn page(
    State(app): State<AppState>,
    AdminCtx(ctx): AdminCtx,
    Path(id): Path<String>,
) -> AppResult<Json<Value>> {
    link(&app, &ctx, Kind::Page, &id).await
}

/// `{"url": "https://site/preview?token=…", "expires_at": …}`, or `{"url": null}` when the public
/// site's URL is not set (no token is made then).
async fn link(app: &AppState, ctx: &rustcms::admin::Ctx, kind: Kind, id: &str) -> AppResult<Json<Value>> {
    let link = preview::issue_link(app, ctx, kind, id).await?;
    Ok(Json(match link {
        Some(link) => json!({ "url": link.url, "expires_at": link.expires_at }),
        None => json!({ "url": null }),
    }))
}
