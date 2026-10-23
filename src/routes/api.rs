//! The JSON API under `/api/v1`, authenticated with API tokens
//! (`Authorization: Bearer rcms_…`, see `rustcms token:create`). Every route
//! declares its access; `rustcms make:api <Model>` adds controller routes here.
//!
//! - `/content/*`: the read-only Content API of the public site (docs/api.md), for Content API
//!   keys (`content.read`, `services::content_api`).
//! - `/media/{id}/{name}`: images of published content, public.
//! - `/preview[/media/{id}]?token=…`: public, the token being the capability (in the query
//!   string, which the request log leaves out).

use rustcms::prelude::*;

use crate::http::controllers::content;
use crate::services::content_api::PERMISSION as CONTENT_READ;

/// Every API route, with the access it requires.
pub fn routes() -> ApiRoutes {
    ApiRoutes::new("/api/v1")
        .get("/ping", Access::public(), ping)
        .get("/content/posts", Access::can(CONTENT_READ), content::posts::index)
        .get("/content/posts/{slug}", Access::can(CONTENT_READ), content::posts::show)
        .get("/content/pages", Access::can(CONTENT_READ), content::pages::index)
        .get("/content/pages/{slug}", Access::can(CONTENT_READ), content::pages::show)
        .get("/content/categories", Access::can(CONTENT_READ), content::taxonomies::categories)
        .get("/content/categories/{slug}", Access::can(CONTENT_READ), content::taxonomies::category)
        .get("/content/tags", Access::can(CONTENT_READ), content::taxonomies::tags)
        .get("/content/tags/{slug}", Access::can(CONTENT_READ), content::taxonomies::tag)
        .get("/content/authors/{slug}", Access::can(CONTENT_READ), content::authors::show)
        .get("/content/menus/{key}", Access::can(CONTENT_READ), content::menus::show)
        .get("/content/settings", Access::can(CONTENT_READ), content::settings::show)
        .get("/content/sitemap", Access::can(CONTENT_READ), content::sitemap::index)
        .get("/media/{id}/{name}", Access::public(), content::media::show)
        .get("/preview", Access::public(), content::preview::show)
        .get("/preview/media/{id}", Access::public(), content::preview::media)
    // rustcms:api-routes
}

async fn ping() -> Json<serde_json::Value> {
    Json(json!({ "pong": true }))
}
