//! Who may see users through the `users` lookup resource (`admin::resources::user_resource`).

use rustcms::admin::*;
use rustcms::async_trait;

/// The `users` resource only lists and shows users, for `belongs_to("users")` fields (the
/// author profile's user): `users.view_any` and `users.view`, the built-in permissions of the
/// panel's Users page. Every write is refused: users are managed on that page, through
/// `RbacService`, never through this resource.
pub struct UserLookupPolicy;

/// The abilities this resource grants (with the user's matching `users.*` permission).
const READ: [&str; 2] = ["view_any", "view"];

#[async_trait]
impl Policy for UserLookupPolicy {
    async fn allows(&self, ctx: &Ctx, ability: &str, _record: Option<&Data>) -> bool {
        READ.contains(&ability) && ctx.can(&format!("users.{ability}"))
    }

    async fn admits(&self, ctx: &Ctx, ability: &str) -> bool {
        self.allows(ctx, ability, None).await
    }

    /// Visibility is the scope's alone (`UserResource::scope`).
    fn view_depends_on_record(&self) -> bool {
        false
    }
}
