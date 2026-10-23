//! Authentication and permissions: admin sessions under `/admin`, API tokens
//! elsewhere. Declare permissions beyond each resource's own here.

use rustcms::auth::{API_GUARD, PermissionMeta};
use rustcms::prelude::*;

use crate::services::content_api;

pub fn provider(api: &ApiRoutes) -> AuthProvider {
    // The API's permissions come from its routes (`content.read`: the Content API).
    AuthProvider::new().api_permissions(api).permission_meta(
        API_GUARD,
        content_api::PERMISSION,
        PermissionMeta::new()
            .label("Read published content")
            .description("Content API keys of the public site: published posts, pages, menus and settings"),
    )
}
