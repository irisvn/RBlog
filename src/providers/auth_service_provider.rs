//! Authentication and permissions: admin sessions under `/admin`, API tokens
//! elsewhere. Declare permissions beyond each resource's own here.

use rustcms::auth::{ADMIN_GUARD, API_GUARD, PermissionArea, PermissionMeta, PermissionPreset};
use rustcms::prelude::*;

use crate::database::seeders::role_seeder::{AUTHOR, CONTRIBUTOR, EDITOR, permissions_of};
use crate::policies::content::{PAGES_PUBLISH, POSTS_EDIT_OTHERS, POSTS_PUBLISH, POSTS_VIEW_ALL};
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
        // Posts and pages (`policies::content`): the workflow actions' abilities are declared by
        // their resources (`posts.submit`, `posts.publish`, `posts.unpublish`, …); these two
        // widen what a user reaches beyond their own posts.
        .permission(ADMIN_GUARD, POSTS_VIEW_ALL, "posts")
        .permission(ADMIN_GUARD, POSTS_EDIT_OTHERS, "posts")
        .permission_area("publishing", PermissionArea::new("Publishing").sort(10))
        .permission_meta(
            ADMIN_GUARD,
            POSTS_VIEW_ALL,
            PermissionMeta::new()
                .label("See every post")
                .description("Every author's posts and their history, not only one's own")
                .area("publishing"),
        )
        .permission_meta(
            ADMIN_GUARD,
            POSTS_EDIT_OTHERS,
            PermissionMeta::new()
                .label("Edit others' posts")
                .description("Edit, tag, publish and delete posts of other authors (with the matching permission)")
                .area("publishing"),
        )
        .permission_meta(
            ADMIN_GUARD,
            POSTS_PUBLISH,
            PermissionMeta::new()
                .label("Publish posts")
                .description("Publish and schedule posts; without it, posts can only be edited while draft or pending"),
        )
        .permission_meta(
            ADMIN_GUARD,
            "posts.unpublish",
            PermissionMeta::new()
                .label("Unpublish posts")
                .description("Take published or scheduled posts back to drafts"),
        )
        .permission_meta(
            ADMIN_GUARD,
            "posts.submit",
            PermissionMeta::new().label("Submit posts for review").description("Move a draft to pending review"),
        )
        .permission_meta(
            ADMIN_GUARD,
            PAGES_PUBLISH,
            PermissionMeta::new()
                .label("Publish pages")
                .description("Publish and schedule pages; without it, pages can only be edited while draft or pending"),
        )
        .permission_meta(
            ADMIN_GUARD,
            "pages.unpublish",
            PermissionMeta::new()
                .label("Unpublish pages")
                .description("Take published or scheduled pages back to drafts"),
        )
        .permission_meta(
            ADMIN_GUARD,
            "pages.submit",
            PermissionMeta::new().label("Submit pages for review").description("Move a draft to pending review"),
        )
        .permission_meta(
            ADMIN_GUARD,
            "post_revisions.restore_version",
            PermissionMeta::new()
                .label("Restore post revisions")
                .description("Put an earlier version back, on posts one may edit"),
        )
        .permission_meta(
            ADMIN_GUARD,
            "page_revisions.restore_version",
            PermissionMeta::new()
                .label("Restore page revisions")
                .description("Put an earlier version back, on pages one may edit"),
        )
        // Starting selections of the Roles page: the roles `RoleSeeder` creates.
        .permission_preset(
            "editor",
            PermissionPreset::new("Editor")
                .description("Every post and page, publishing, categories, tags, menus and media")
                .permissions(permissions_of(EDITOR)),
        )
        .permission_preset(
            "author",
            PermissionPreset::new("Author")
                .description("Writes and publishes their own posts, uploads media")
                .permissions(permissions_of(AUTHOR)),
        )
        .permission_preset(
            "contributor",
            PermissionPreset::new("Contributor")
                .description("Writes their own drafts and submits them for review")
                .permissions(permissions_of(CONTRIBUTOR)),
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
