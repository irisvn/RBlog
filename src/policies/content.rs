//! Who may do what to a post or a page (the plan's permission matrix). One function,
//! [`allows`], decides every ability: the resources' policies (`PostPolicy`, `PagePolicy`), the
//! revision policies (through the parent), `services::publishing` and `services::revisions` call
//! it, so the panel, actions and jobs follow the same rules.
//!
//! An ability needs its permission (`posts.update`, `posts.publish`, …) and, on a record:
//!
//! - **ownership**: posts of other authors need `posts.edit_others` (pages have no owners: they
//!   are for editors);
//! - **state**: without `posts.publish` (a contributor), a post can be edited, re-tagged or
//!   submitted only while it is a draft or pending, and trashed, restored or deleted only while it
//!   is a draft.
//!
//! Which states a transition leaves (`submit`: draft; `publish`, which also schedules: draft,
//! pending or scheduled; `unpublish`: published or scheduled) is not decided here but by
//! `services::publishing::Transition::applies`: the resources offer each workflow action only
//! where it applies (`Action::visible_when`, so a run on one record it does not apply to is a
//! 403), and `publishing::transition` refuses it again on the locked row.
//!
//! Visibility is the resources' `scope()`: without `posts.view_all`, a user sees only the posts
//! they wrote ([`sees_all`]).

use rustcms::admin::{Ctx, Data};
use serde_json::Value;

use crate::models::post::PostStatus;
use crate::services::content::{Content, Kind};

/// Every post, not only one's own (lists, reads, and the posts' revisions).
pub const POSTS_VIEW_ALL: &str = "posts.view_all";
/// Edit, tag, publish and delete posts of other authors.
pub const POSTS_EDIT_OTHERS: &str = "posts.edit_others";
/// Publish and schedule posts (the `publish` action's ability).
pub const POSTS_PUBLISH: &str = "posts.publish";
/// Publish and schedule pages.
pub const PAGES_PUBLISH: &str = "pages.publish";

/// What the rules read of a post or page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Facts {
    pub author_id: i64,
    pub status: Option<PostStatus>,
}

impl Facts {
    pub fn of(content: &Content) -> Self {
        Self { author_id: content.author_id, status: content.status() }
    }

    /// From a record as the engine hands it to a policy (every column, 64-bit ids as strings).
    pub fn from_record(record: &Data) -> Option<Self> {
        let author_id = match record.get("author_id")? {
            Value::String(s) => s.parse().ok()?,
            Value::Number(n) => n.as_i64()?,
            _ => return None,
        };
        let status = record.get("status").and_then(Value::as_str).and_then(PostStatus::parse);
        Some(Self { author_id, status })
    }
}

/// The signed-in user's id (`None` for a principal that is not a user).
pub fn me(ctx: &Ctx) -> Option<i64> {
    ctx.principal.id.parse().ok()
}

/// Whether the user sees every post of `kind` (pages: always, for whoever may view pages).
pub fn sees_all(ctx: &Ctx, kind: Kind) -> bool {
    match kind {
        Kind::Post => ctx.can(POSTS_VIEW_ALL),
        Kind::Page => true,
    }
}

/// Whether the user may edit what others wrote.
fn edits_others(ctx: &Ctx, kind: Kind) -> bool {
    match kind {
        Kind::Post => ctx.can(POSTS_EDIT_OTHERS),
        Kind::Page => true,
    }
}

/// The abilities a post or page resource knows. Anything else is refused.
pub const ABILITIES: &[&str] = &[
    "view_any",
    "view",
    "create",
    "update",
    "delete",
    "restore",
    "force_delete",
    "attach_tags",
    "detach_tags",
    "submit",
    "publish",
    "unpublish",
];

/// Whether the user may hold `ability` on some post or page of `kind` (the permission alone):
/// `Policy::admits`, checked before any record is looked up.
pub fn admits(ctx: &Ctx, kind: Kind, ability: &str) -> bool {
    if !ABILITIES.contains(&ability) || (kind == Kind::Page && ability.ends_with("_tags")) {
        return false;
    }
    ctx.can(&kind.permission(ability))
}

/// Whether the user may do `ability` on the post or page `record` (`None`: without a record, as
/// for `create` or a list).
pub fn allows(ctx: &Ctx, kind: Kind, ability: &str, record: Option<&Facts>) -> bool {
    if !admits(ctx, kind, ability) {
        return false;
    }
    let Some(record) = record else {
        // Without a record: the permission decides (create, lists, the form's tag input).
        return true;
    };
    let own = me(ctx) == Some(record.author_id);
    let mine_or_allowed = own || edits_others(ctx, kind);
    // Without the right to publish, a user changes only what is not live yet.
    let publishes = ctx.can(&kind.permission("publish"));
    let status = record.status;
    let editable = publishes || matches!(status, Some(PostStatus::Draft | PostStatus::Pending));
    match ability {
        "view_any" => true,
        "view" => own || sees_all(ctx, kind),
        "update" | "attach_tags" | "detach_tags" => mine_or_allowed && editable,
        "delete" | "restore" | "force_delete" => mine_or_allowed && (publishes || status == Some(PostStatus::Draft)),
        // Which states a transition leaves is `publishing::Transition::applies`, checked by the
        // actions' `visible_when` and by `publishing::transition`: here only who.
        "submit" => mine_or_allowed && editable,
        "publish" | "unpublish" => mine_or_allowed,
        "create" => true,
        _ => false,
    }
}
