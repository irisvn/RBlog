//! The Version page of the admin panel (`/admin/p/version`): what RBlog runs, whether a newer
//! release is out (checked every 6 hours, or now), its notes, and how to update with docker
//! compose. A React page (`rblog/version`) on the routes of `crate::routes::admin`
//! (`/system/version`). RBlog never updates itself.
//!
//! In the Integrations group: RustCMS draws its own System section (Users, Roles, Audit, Queue,
//! Schedules) apart from the pages' groups, so a page group named System would show a second
//! System heading.

use rustcms::admin::{PageView, PanelPage, PanelPageMeta};
use rustcms::async_trait;

use crate::services::version::PERMISSION;

pub struct VersionPage;

#[async_trait]
impl PanelPage for VersionPage {
    const SLUG: &'static str = "version";

    /// For holders of `system.version` (declared by the admin routes).
    fn meta() -> PanelPageMeta {
        PanelPageMeta::new("Version").group("Integrations").icon("info").sort(10).permission(PERMISSION)
    }

    /// Drawn by `resources/admin/src/rblog/version.tsx`; it reads everything through the routes.
    fn view() -> PageView {
        PageView::Script("rblog/version")
    }
}
