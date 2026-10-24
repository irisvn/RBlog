//! `rustcms db:seed --class role` (also called by `DatabaseSeeder`): the four roles of the plan's
//! permission matrix, in the admin guard, through `RbacService`. The same lists are the Roles
//! page's presets (`providers/auth_service_provider.rs`).
//!
//! | | Administrator | Editor | Author | Contributor |
//! |---|---|---|---|---|
//! | Every post (see, edit, publish) | ✓ | ✓ | own | own |
//! | Publish, schedule, unpublish | ✓ | ✓ | own | – |
//! | Edit and tag | ✓ | ✓ | own | own, draft or pending |
//! | Trash, restore | ✓ | ✓ | own | own drafts |
//! | Pages, categories, tags, menus | ✓ | ✓ | – | – |
//! | Upload media / see all media | ✓ / ✓ | ✓ / ✓ | ✓ / ✓ | – / ✓ |
//! | Settings, integrations, users, roles | ✓ | – | – | – |
//!
//! The rules on a record (own posts, statuses) are `policies::content`; these lists only grant
//! the permissions.

use rustcms::auth::entities::role;
use rustcms::auth::{ADMIN_GUARD, Actor, AuthState, RbacService};
use rustcms::database::Seeder;
use rustcms::{AppState, async_trait};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

/// Reading the categories, tags and authors a post form chooses from, and the media library's
/// images: what every writer needs.
const READ_SHARED: &[&str] = &[
    "categories.view_any",
    "categories.view",
    "tags.view_any",
    "tags.view",
    "post_authors.view_any",
    "post_authors.view",
    "media.view_any",
    "media.view",
    "media.view_all",
];

/// The post widgets of the dashboard (`admin::dashboard::with_post_widgets`): they count what
/// the user sees (their own posts without `posts.view_all`).
const POST_WIDGETS: &[&str] = &[
    "dashboard.published_posts",
    "dashboard.draft_posts",
    "dashboard.pending_posts",
    "dashboard.scheduled_posts",
    "dashboard.posts_per_week",
    "dashboard.recent_posts",
];

/// Contributors: their own posts while draft or pending, submitted for review; no publishing, no
/// uploads (they use the library's images).
pub const CONTRIBUTOR: &[&str] = &[
    "posts.view_any",
    "posts.view",
    "posts.create",
    "posts.update",
    "posts.delete",
    "posts.restore",
    "posts.attach_tags",
    "posts.detach_tags",
    "posts.submit",
    "post_revisions.view_any",
    "post_revisions.view",
    "post_revisions.restore_version",
];

/// Authors: their own posts, published by themselves; uploads.
pub const AUTHOR: &[&str] = &[
    "posts.view_any",
    "posts.view",
    "posts.create",
    "posts.update",
    "posts.delete",
    "posts.restore",
    "posts.force_delete",
    "posts.attach_tags",
    "posts.detach_tags",
    "posts.submit",
    "posts.publish",
    "posts.unpublish",
    "post_revisions.view_any",
    "post_revisions.view",
    "post_revisions.restore_version",
    "media.create",
    "media.delete",
    "media.restore",
];

/// Editors: every post and page, publishing, categories, tags, menus; uploads.
pub const EDITOR: &[&str] = &[
    "posts.view_any",
    "posts.view",
    "posts.create",
    "posts.update",
    "posts.delete",
    "posts.restore",
    "posts.force_delete",
    "posts.attach_tags",
    "posts.detach_tags",
    "posts.submit",
    "posts.publish",
    "posts.unpublish",
    "posts.view_all",
    "posts.edit_others",
    "post_revisions.view_any",
    "post_revisions.view",
    "post_revisions.restore_version",
    "pages.view_any",
    "pages.view",
    "pages.create",
    "pages.update",
    "pages.delete",
    "pages.restore",
    "pages.force_delete",
    "pages.submit",
    "pages.publish",
    "pages.unpublish",
    "page_revisions.view_any",
    "page_revisions.view",
    "page_revisions.restore_version",
    "categories.create",
    "categories.update",
    "categories.delete",
    "tags.create",
    "tags.update",
    "tags.delete",
    "menus.view_any",
    "menus.view",
    "menus.create",
    "menus.update",
    "menus.delete",
    "menu_items.view_any",
    "menu_items.view",
    "menu_items.create",
    "menu_items.update",
    "menu_items.delete",
    "media.create",
    "media.delete",
    "media.restore",
    // Author profiles: read (making one needs the Users page, an administrator's).
    "authors.view_any",
    "authors.view",
    "dashboard.authors",
];

/// The role names, in the matrix's order.
pub const ADMINISTRATOR_ROLE: &str = "Administrator";
pub const EDITOR_ROLE: &str = "Editor";
pub const AUTHOR_ROLE: &str = "Author";
pub const CONTRIBUTOR_ROLE: &str = "Contributor";

/// The permissions of a role (`READ_SHARED` included).
pub fn permissions_of(list: &[&str]) -> Vec<String> {
    let mut all: Vec<String> = READ_SHARED.iter().chain(POST_WIDGETS).chain(list).map(|p| (*p).to_owned()).collect();
    all.sort();
    all.dedup();
    all
}

pub struct RoleSeeder;

#[async_trait]
impl Seeder for RoleSeeder {
    fn name(&self) -> &'static str {
        "role"
    }

    /// Runs again safely: the declared permissions are synced, and a role is created only when
    /// missing, so changes made on the Roles page are kept. The Administrator gets every admin
    /// permission declared when it is created.
    async fn run(&self, state: &AppState) -> anyhow::Result<()> {
        let auth = state.service::<AuthState>().ok_or_else(|| anyhow::anyhow!("AuthProvider is not registered"))?;
        let rbac = RbacService::new(state.db(), &auth.resolver);
        rbac.sync_permissions(&auth.permissions.all()).await?;
        let everything: Vec<String> = auth
            .permissions
            .all()
            .into_keys()
            .filter(|(guard, _)| guard == ADMIN_GUARD)
            .map(|(_, name)| name)
            .collect();
        let roles = [
            (ADMINISTRATOR_ROLE, everything),
            (EDITOR_ROLE, permissions_of(EDITOR)),
            (AUTHOR_ROLE, permissions_of(AUTHOR)),
            (CONTRIBUTOR_ROLE, permissions_of(CONTRIBUTOR)),
        ];
        for (name, permissions) in roles {
            if find(state, name).await?.is_none() {
                rbac.create_role(Actor::System, ADMIN_GUARD, name, &permissions).await?;
            }
        }
        Ok(())
    }
}

/// The admin-guard role `name`.
pub async fn find(state: &AppState, name: &str) -> anyhow::Result<Option<role::Model>> {
    Ok(role::Entity::find()
        .filter(role::Column::Name.eq(name))
        .filter(role::Column::GuardName.eq(ADMIN_GUARD))
        .one(state.db())
        .await?)
}
