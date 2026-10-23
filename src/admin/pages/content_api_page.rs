//! The Content API page of the admin panel (`/admin/p/content-api`): the keys the public site
//! reads published content with (docs/api.md). A React page (`rblog/content-api`) on the routes
//! of `crate::routes::admin` (`/integrations/content-keys`).

use rustcms::admin::{PageView, PanelPage, PanelPageMeta};
use rustcms::async_trait;

use crate::routes::admin::INTEGRATIONS_VIEW;

pub struct ContentApiPage;

#[async_trait]
impl PanelPage for ContentApiPage {
    const SLUG: &'static str = "content-api";

    /// For holders of `integrations.view` (declared by the admin routes); making and revoking keys
    /// needs `integrations.manage`, checked by each route.
    fn meta() -> PanelPageMeta {
        PanelPageMeta::new("Content API").group("Integrations").icon("key-round").permission(INTEGRATIONS_VIEW)
    }

    /// Drawn by `resources/admin/src/rblog/content-api.tsx`; it reads everything through the routes.
    fn view() -> PageView {
        PageView::Script("rblog/content-api")
    }
}
