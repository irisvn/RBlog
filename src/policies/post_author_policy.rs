//! The policy of `PostAuthorResource`: the names of the users who write posts and pages, read
//! only (`post_authors.view_any`, `post_authors.view`). Who appears is its `scope()`.

use rustcms::admin::{Ctx, Data, Policy};
use rustcms::async_trait;

pub struct PostAuthorPolicy;

#[async_trait]
impl Policy for PostAuthorPolicy {
    async fn allows(&self, ctx: &Ctx, ability: &str, _record: Option<&Data>) -> bool {
        self.admits(ctx, ability).await
    }

    /// Only reading: users are managed on the panel's Users page.
    async fn admits(&self, ctx: &Ctx, ability: &str) -> bool {
        matches!(ability, "view_any" | "view") && ctx.can(&format!("post_authors.{ability}"))
    }

    fn view_depends_on_record(&self) -> bool {
        false
    }
}
