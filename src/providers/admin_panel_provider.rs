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
        .resource::<crate::admin::resources::category_resource::CategoryResource>()
        .resource::<crate::admin::resources::tag_resource::TagResource>()
        .resource::<crate::admin::resources::menu_resource::MenuResource>()
        .resource::<crate::admin::resources::menu_item_resource::MenuItemResource>()
        // rustcms:resources
        // Integrations: the Content API keys of the public site (a React page on `routes::admin`).
        .page::<crate::admin::pages::content_api_page::ContentApiPage>()
        // rustcms:pages
        .extension_script_v2(
            "content-api",
            include_str!("../../resources/admin/js/rblog/content-api.js"),
            Requires { react: "19.3", react_query: "5.104", ui: 2 },
        )
        .extension_style("content-api", include_str!("../../resources/admin/css/rblog/content-api.css"))
    // rustcms:extensions
}
