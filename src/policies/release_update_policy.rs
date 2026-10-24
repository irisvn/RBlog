//! The policy of `ReleaseUpdateResource` (the dashboard's notice of a newer RBlog release): read
//! only, for holders of `system.version`, the permission of Integrations → Version. The resource's
//! own permissions (`release_updates.*`, declared by RustCMS for every resource) grant nothing.

use rustcms::admin::{Ctx, Data, Policy};
use rustcms::async_trait;

use crate::services::version::PERMISSION;

pub struct ReleaseUpdatePolicy;

#[async_trait]
impl Policy for ReleaseUpdatePolicy {
    async fn allows(&self, ctx: &Ctx, ability: &str, _record: Option<&Data>) -> bool {
        self.admits(ctx, ability).await
    }

    /// Only reading; the row is written by `services::version` alone.
    async fn admits(&self, ctx: &Ctx, ability: &str) -> bool {
        matches!(ability, "view_any" | "view") && ctx.can(PERMISSION)
    }

    fn view_depends_on_record(&self) -> bool {
        false
    }
}
