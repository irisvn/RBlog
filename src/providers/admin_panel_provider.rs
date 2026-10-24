//! The admin panel at `/admin` (like a Filament panel provider): its
//! resources (`rustcms make:resource`), pages (`make:settings`) and labels.

use rustcms::admin::{Requires, Theme};
use rustcms::prelude::*;

use crate::settings::general_settings::GeneralSettings;
use crate::settings::integration_settings::IntegrationSettings;
use crate::settings::reading_settings::ReadingSettings;
use crate::settings::seo_settings::SeoSettings;

pub fn provider() -> AdminProvider {
    AdminProvider::new()
        .brand("rblog")
        // Status tones (docs/ui-guidelines.md §1): RustCMS defaults them to grays.
        // Each keeps at least 3:1 against `--card` (5.0:1 light, 10.8 and 11.3:1 dark).
        .theme(
            Theme::new()
                .color("success", "#15803d")
                .color("warning", "#b45309")
                .dark(|dark| dark.color("success", "#4ade80").color("warning", "#fbbf24")),
        )
        // The media library (`[storage]`): uploads from forms and the editor.
        .media()
        // Labels in Vietnamese (`rustcms lang:missing vi` lists what is missing).
        .translations("vi", include_str!("../../lang/vi.toml"))
        // Settings, in the navigation's order: the site, its homepage, search and social
        // networks, then what connects it to the public site.
        .settings::<GeneralSettings>()
        .settings::<ReadingSettings>()
        .settings::<SeoSettings>()
        .settings::<IntegrationSettings>()
        // Posts and pages come first: in the navigation (`sort`) and, since RustCMS 0.10.1, on the
        // dashboard's resource cards, which follow the order of registration. Revisions are
        // shown on their pages.
        .resource::<crate::admin::resources::post_resource::PostResource>()
        .resource::<crate::admin::resources::page_resource::PageResource>()
        .resource::<crate::admin::resources::category_resource::CategoryResource>()
        .resource::<crate::admin::resources::tag_resource::TagResource>()
        .resource::<crate::admin::resources::menu_resource::MenuResource>()
        .resource::<crate::admin::resources::menu_item_resource::MenuItemResource>()
        .resource::<crate::admin::resources::author_profile_resource::AuthorProfileResource>()
        // The target of `belongs_to("users")` (read only, no navigation entry).
        .resource::<crate::admin::resources::user_resource::UserResource>()
        // The target of posts' and pages' `author_id`: names only (read only, no navigation entry).
        .resource::<crate::admin::resources::post_author_resource::PostAuthorResource>()
        .resource::<crate::admin::resources::post_revision_resource::PostRevisionResource>()
        .resource::<crate::admin::resources::page_revision_resource::PageRevisionResource>()
        // The dashboard's notice of a newer RBlog release (read only, no navigation entry).
        .resource::<crate::admin::resources::release_update_resource::ReleaseUpdateResource>()
        // rustcms:resources
        // Widgets above the resource cards (`admin::dashboard`).
        .dashboard(crate::admin::dashboard::with_post_widgets(crate::admin::dashboard::dashboard()))
        // Integrations: the Content API keys of the public site (a React page on `routes::admin`).
        .page::<crate::admin::pages::content_api_page::ContentApiPage>()
        // Integrations → Version: what runs and whether a newer release is out (`routes::admin`).
        .page::<crate::admin::pages::version_page::VersionPage>()
        // rustcms:pages
        .extension_script_v2(
            "content-api",
            include_str!("../../resources/admin/js/rblog/content-api.js"),
            Requires { react: "19.3", react_query: "5.104", ui: 2 },
        )
        .extension_style("content-api", include_str!("../../resources/admin/css/rblog/content-api.css"))
        // Posts and pages: the status cell of their lists and the preview slot of their pages.
        .slot::<crate::admin::slots::preview_slot::PostPreviewSlot>()
        .slot::<crate::admin::slots::preview_slot::PagePreviewSlot>()
        .extension_script_v2(
            "content",
            include_str!("../../resources/admin/js/rblog/content.js"),
            Requires { react: "19.3", react_query: "5.104", ui: 2 },
        )
        .extension_style("content", include_str!("../../resources/admin/css/rblog/content.css"))
        .extension_script_v2(
            "version",
            include_str!("../../resources/admin/js/rblog/version.js"),
            Requires { react: "19.3", react_query: "5.104", ui: 2 },
        )
        .extension_style("version", include_str!("../../resources/admin/css/rblog/version.css"))
    // rustcms:extensions
}
