//! The policy of `PostRevisionResource` and `PageRevisionResource`: revisions are read and
//! restored through their post or page. Every ability needs its own permission
//! (`post_revisions.view`, `post_revisions.restore_version`) and the parent's:
//!
//! - `view_any`, `view`: viewing the parent (not trashed; own posts without `posts.view_all`);
//! - `restore_version`: updating the parent ([`super::content::allows`] `update`).
//!
//! Revisions are written by the server only: `create`, `update` and `delete` are always refused.

use rustcms::admin::{Ctx, Data, Policy};
use rustcms::async_trait;
use serde_json::Value;

use super::content::{self, Facts};
use crate::services::content::{self as rows, Kind};

pub struct RevisionPolicy(pub Kind);

impl RevisionPolicy {
    /// `post_revisions` or `page_revisions`.
    pub fn slug(kind: Kind) -> &'static str {
        match kind {
            Kind::Post => "post_revisions",
            Kind::Page => "page_revisions",
        }
    }

    /// `post_id` or `page_id`.
    pub fn parent_field(kind: Kind) -> &'static str {
        match kind {
            Kind::Post => "post_id",
            Kind::Page => "page_id",
        }
    }

    /// The parent's ability an ability of the revision needs.
    fn parent_ability(ability: &str) -> Option<&'static str> {
        match ability {
            "view_any" => Some("view_any"),
            "view" => Some("view"),
            "restore_version" => Some("update"),
            _ => None,
        }
    }
}

#[async_trait]
impl Policy for RevisionPolicy {
    async fn allows(&self, ctx: &Ctx, ability: &str, record: Option<&Data>) -> bool {
        let Some(parent_ability) = Self::parent_ability(ability) else { return false };
        if !self.admits(ctx, ability).await {
            return false;
        }
        let Some(record) = record else {
            // Without a record (lists, the schema): the permissions; `scope()` keeps the rows.
            return true;
        };
        let parent_id = match record.get(Self::parent_field(self.0)) {
            Some(Value::String(s)) => s.parse::<i64>().ok(),
            Some(Value::Number(n)) => n.as_i64(),
            _ => None,
        };
        let Some(parent_id) = parent_id else { return false };
        // The parent as stored now (in a write, as this write sees it).
        match rows::find(&ctx.conn(), self.0, parent_id).await {
            Ok(Some(parent)) if parent.deleted_at.is_none() => {
                content::allows(ctx, self.0, parent_ability, Some(&Facts::of(&parent)))
            }
            _ => false,
        }
    }

    async fn admits(&self, ctx: &Ctx, ability: &str) -> bool {
        let Some(parent_ability) = Self::parent_ability(ability) else { return false };
        ctx.can(&format!("{}.{ability}", Self::slug(self.0))) && content::admits(ctx, self.0, parent_ability)
    }

    /// Visibility through the parent is the resource's `scope()` too.
    fn view_depends_on_record(&self) -> bool {
        false
    }
}
