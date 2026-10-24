//! The dashboard's widgets (RustCMS guide `admin/dashboard.md`). A widget counts or lists the
//! records of a **resource**, through its list engine (scope, policy, field rules; trashed
//! records never count), so a user sees one only with its permission (`dashboard.<name>`), the
//! resource's `view_any` (and `view`, without which no record counts) and read access to every
//! field it uses. Widgets are checked at boot: a widget over a resource that is not registered
//! stops the boot.

use rustcms::admin::dashboard::{Bucket, Dashboard, Widget};
use rustcms::admin::resource::FilterOp;
use serde_json::json;

/// The widgets registered now.
///
/// | Widget | Shows | Needs |
/// |---|---|---|
/// | `rblog_update` | a newer RBlog release, while there is one (Integrations → Version) | `system.version` |
/// | `authors` | author profiles | `dashboard.authors`, `authors.view_any`, `authors.view` |
///
/// `rblog_update` is the administrators' notice of a newer release: RustCMS 0.10.3 has no
/// dashboard slot or banner, so it is a `recent` widget over `release_updates`, whose scope keeps
/// the row only while its release is newer than a release build (`admin::resources::
/// release_update_resource`). While up to date it lists nothing.
pub fn dashboard() -> Dashboard {
    Dashboard::new()
        .widget(
            Widget::recent("rblog_update", "release_updates")
                .label("New RBlog release")
                .columns(&["published_at"])
                .limit(1)
                .permission(crate::services::version::PERMISSION),
        )
        .widget(Widget::stat("authors", "authors").label("Authors"))
}

/// The post widgets, to add to [`dashboard`] (`with_post_widgets(Dashboard::new()…)`) once the
/// `posts` resource is registered; before that they stop the boot. They need `posts` to declare
/// `status` as a `.filterable()` field, `published_at` as a readable date field, and
/// `updated_at` as a readable `.sortable()` field.
///
/// | Widget | Shows | Needs |
/// |---|---|---|
/// | `published_posts` | posts with status `published` | `dashboard.published_posts`, `posts.view_any`, `posts.view`, read `status` |
/// | `draft_posts` | status `draft` | `dashboard.draft_posts`, `posts.view_any`, `posts.view`, read `status` |
/// | `pending_posts` | status `pending` (pending review) | `dashboard.pending_posts`, `posts.view_any`, `posts.view`, read `status` |
/// | `scheduled_posts` | status `scheduled` | `dashboard.scheduled_posts`, `posts.view_any`, `posts.view`, read `status` |
/// | `posts_per_week` | published posts per ISO week, last 12 weeks, by `published_at` | `dashboard.posts_per_week`, `posts.view_any`, `posts.view`, read `status`, `published_at` |
/// | `recent_posts` | the 5 posts updated last | `dashboard.recent_posts`, `posts.view_any`, `posts.view`, read `status`, `updated_at` |
///
/// Trashed posts never count. Widget filters are fixed values, so "published" here is the
/// status alone: it means "live" (`published_at <= now`, `models::post::public_at`) only while
/// every writer keeps a published post's `published_at` in the past, scheduling a future date
/// as `scheduled` instead.
pub fn with_post_widgets(dashboard: Dashboard) -> Dashboard {
    let status = |name: &str, label: &str, status: &str| {
        Widget::stat(name, "posts").label(label).filter("status", FilterOp::Eq, json!(status))
    };
    dashboard
        .widget(status("published_posts", "Published posts", "published"))
        .widget(status("draft_posts", "Drafts", "draft"))
        .widget(status("pending_posts", "Pending review", "pending"))
        .widget(status("scheduled_posts", "Scheduled", "scheduled"))
        .widget(
            Widget::chart("posts_per_week", "posts", "published_at")
                .label("Posts published per week")
                .filter("status", FilterOp::Eq, json!("published"))
                .per(Bucket::Week)
                .last(12)
                .span(2),
        )
        .widget(
            Widget::recent("recent_posts", "posts")
                .label("Recently updated posts")
                .columns(&["status", "updated_at"])
                .sort("-updated_at")
                .limit(5)
                .span(2),
        )
}
