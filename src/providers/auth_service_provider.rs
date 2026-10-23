//! Authentication and permissions: admin sessions under `/admin`, API tokens
//! elsewhere. Declare permissions beyond each resource's own here.

use rustcms::auth::{ADMIN_GUARD, API_GUARD, PermissionArea, PermissionMeta};
use rustcms::prelude::*;

use crate::routes::admin::{INTEGRATIONS_MANAGE, INTEGRATIONS_VIEW};
use crate::services::content_api;

pub fn provider(api: &ApiRoutes, admin: &AdminRoutes) -> AuthProvider {
    AuthProvider::new()
        // The API's permissions come from its routes (`content.read`: the Content API), the
        // panel pages' from theirs (`integrations.*`: the Content API keys).
        .api_permissions(api)
        .admin_permissions(admin)
        .permission_meta(
            API_GUARD,
            content_api::PERMISSION,
            PermissionMeta::new()
                .label("Read published content")
                .description("Content API keys of the public site: published posts, pages, menus and settings"),
        )
        .permission_area("integrations", PermissionArea::new("Integrations"))
        .permission_meta(
            ADMIN_GUARD,
            INTEGRATIONS_VIEW,
            PermissionMeta::new()
                .label("View Content API keys")
                .description("See the Content API keys of the public site, never their secret")
                .area("integrations"),
        )
        .permission_meta(
            ADMIN_GUARD,
            INTEGRATIONS_MANAGE,
            PermissionMeta::new()
                .label("Manage Content API keys")
                .description("Create and revoke the keys the public site reads published content with")
                .area("integrations"),
        )
}
