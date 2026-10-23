//! JSON routes of the panel's own pages, on the admin session (`AdminRoutes`, served under
//! `{admin}/api/x`, ADR 0026). Every route declares its access, checked before the handler reads
//! anything; `AuthProvider::admin_permissions` declares the permissions they name.
//!
//! - `/integrations/content-keys`: Content API keys of the Integrations page
//!   (`crate::admin::pages::content_api_page`).

use rustcms::prelude::*;

use crate::http::controllers::content_keys;

/// See and use the Integrations pages (the Content API keys' list).
pub const INTEGRATIONS_VIEW: &str = "integrations.view";
/// Make and revoke Content API keys.
pub const INTEGRATIONS_MANAGE: &str = "integrations.manage";

/// Every admin route, with the access it requires.
pub fn routes() -> AdminRoutes {
    AdminRoutes::new("/integrations")
        .get("/content-keys", AdminAccess::can(INTEGRATIONS_VIEW), content_keys::index)
        .post("/content-keys", AdminAccess::can(INTEGRATIONS_MANAGE), content_keys::store)
        .delete("/content-keys/{id}", AdminAccess::can(INTEGRATIONS_MANAGE), content_keys::destroy)
}
