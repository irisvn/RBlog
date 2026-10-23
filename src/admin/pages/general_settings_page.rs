//! The admin page of [`GeneralSettings`] (`/admin/settings/general`): which fields the form
//! shows and how they are checked.

use rustcms::ValidationErrors;
use rustcms::admin::{Ctx, SettingField, SettingsMeta, SettingsPage};
use rustcms::async_trait;

use crate::settings::general_settings::{GeneralSettings, POSTS_PER_PAGE};
use crate::settings::rules::{checked, filled, is_web_url};

// TODO(C1): emit the `settings` outbox event after a save, once RustCMS 0.9 gives
// `SettingsPage` an after-save hook (0.8.2 has none).
#[async_trait]
impl SettingsPage for GeneralSettings {
    fn meta() -> SettingsMeta {
        SettingsMeta::new("General").icon("settings")
    }

    fn fields() -> Vec<SettingField> {
        vec![
            SettingField::text("site_name").label("Site name").required().max_length(120),
            SettingField::textarea("description").label("Description").max_length(500),
            SettingField::select("site_language", [("vi", "Vietnamese"), ("en", "English")])
                .label("Site language")
                .help("The language the public site is written in"),
            SettingField::text("frontend_url")
                .label("Public site URL")
                .nullable()
                .max_length(2000)
                .url()
                .help("An https address, for example https://example.com (http only for localhost)"),
            SettingField::integer("posts_per_page").label("Posts per page").help("Between 1 and 100"),
        ]
    }

    async fn validate(ctx: &Ctx, settings: &Self) -> Result<(), ValidationErrors> {
        let mut errors = ValidationErrors::new();
        if settings.site_name.trim().is_empty() {
            errors.add("site_name", "is required");
        }
        if let Some(url) = filled(&settings.frontend_url)
            && !is_web_url(url, true)
        {
            errors.add("frontend_url", ctx.t("must be an https address (http only for localhost)"));
        }
        if !POSTS_PER_PAGE.contains(&settings.posts_per_page) {
            errors.add("posts_per_page", format!("must be between 1 and {}", POSTS_PER_PAGE.end()));
        }
        checked(errors)
    }
}
