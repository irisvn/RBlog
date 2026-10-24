//! The policy of `PostResource` and `PageResource`: [`super::content::allows`] on the record the
//! engine hands over.

use rustcms::admin::{Ctx, Data, Policy};
use rustcms::async_trait;

use super::content::{Facts, admits, allows};
use crate::services::content::Kind;

pub struct ContentPolicy(pub Kind);

#[async_trait]
impl Policy for ContentPolicy {
    async fn allows(&self, ctx: &Ctx, ability: &str, record: Option<&Data>) -> bool {
        match record {
            None => allows(ctx, self.0, ability, None),
            // A record without an author or status is not one of ours: refused.
            Some(record) => Facts::from_record(record).is_some_and(|facts| allows(ctx, self.0, ability, Some(&facts))),
        }
    }

    /// The permission alone, before any record is looked up: a user without it is refused
    /// whether or not the key exists.
    async fn admits(&self, ctx: &Ctx, ability: &str) -> bool {
        admits(ctx, self.0, ability)
    }

    /// `view` on a record is the resource's `scope()` (own posts without `posts.view_all`).
    fn view_depends_on_record(&self) -> bool {
        false
    }
}
