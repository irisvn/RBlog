//! JSON routes of the panel's own pages, on the admin session (`AdminRoutes`, served under
//! `{admin}/api/x`, ADR 0026). Every route declares its access, checked before the handler reads
//! anything; `AuthProvider::admin_permissions` declares the permissions they name.
//!
//! - `/integrations/content-keys`: Content API keys of the Integrations page
//!   (`crate::admin::pages::content_api_page`).
//! - `/previews/{posts|pages}/{id}`: preview links of the post and page pages (the
//!   `rblog/content-preview` slot). The route needs `posts.view` / `pages.view`; the record itself
//!   is then read as the panel reads it for the user (scope and policy).

use rustcms::prelude::*;

use crate::http::controllers::{content_keys, previews};

/// See and use the Integrations pages (the Content API keys' list).
pub const INTEGRATIONS_VIEW: &str = "integrations.view";
/// Make and revoke Content API keys.
pub const INTEGRATIONS_MANAGE: &str = "integrations.manage";

/// Every admin route, with the access it requires.
pub fn routes() -> AdminRoutes {
    AdminRoutes::scope()
        .nest(
            "/integrations",
            AdminRoutes::scope()
                .get("/content-keys", AdminAccess::can(INTEGRATIONS_VIEW), content_keys::index)
                .post("/content-keys", AdminAccess::can(INTEGRATIONS_MANAGE), content_keys::store)
                .delete("/content-keys/{id}", AdminAccess::can(INTEGRATIONS_MANAGE), content_keys::destroy),
        )
        .nest(
            "/previews",
            AdminRoutes::scope().post("/posts/{id}", AdminAccess::can("posts.view"), previews::post).post(
                "/pages/{id}",
                AdminAccess::can("pages.view"),
                previews::page,
            ),
        )
}
