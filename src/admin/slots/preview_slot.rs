//! "Preview" beside a post or a page: a button that asks for a preview link of the public site
//! (`resources/admin/src/rblog/content.tsx`, `rblog/content-preview`). The link is made by
//! `POST …/previews/{posts|pages}/{id}` (`routes::admin`), which checks the record again: the
//! slot only tells the script which route to call.
//!
//! The Preview action of the same records (`admin::resources::content::preview_action`) opens a
//! fresh link in one click; this card stays for what an action cannot show: the link itself, to
//! copy and send, and when it stops working. Both issue the link through
//! `services::preview::issue_link_in`.

use rustcms::admin::{Ctx, Envelope, PageView, PanelSlot, Placement, SlotMeta};
use rustcms::{AppResult, async_trait};
use serde_json::{Value, json};

pub struct PostPreviewSlot;

#[async_trait]
impl PanelSlot for PostPreviewSlot {
    const NAME: &'static str = "post-preview";

    fn placement() -> Placement {
        Placement::RecordAside("posts")
    }

    fn meta() -> SlotMeta {
        SlotMeta::new().label("Preview").permission("posts.view")
    }

    fn view() -> PageView {
        PageView::Script("rblog/content-preview")
    }

    async fn data(_ctx: &Ctx, record: &Envelope) -> AppResult<Value> {
        Ok(json!({ "route": format!("/previews/posts/{}", record.key) }))
    }
}

pub struct PagePreviewSlot;

#[async_trait]
impl PanelSlot for PagePreviewSlot {
    const NAME: &'static str = "page-preview";

    fn placement() -> Placement {
        Placement::RecordAside("pages")
    }

    fn meta() -> SlotMeta {
        SlotMeta::new().label("Preview").permission("pages.view")
    }

    fn view() -> PageView {
        PageView::Script("rblog/content-preview")
    }

    async fn data(_ctx: &Ctx, record: &Envelope) -> AppResult<Value> {
        Ok(json!({ "route": format!("/previews/pages/{}", record.key) }))
    }
}
