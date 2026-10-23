//! The admin page of [`SeoSettings`] (`/admin/settings/seo`): which fields the form shows and
//! how they are checked.

use rustcms::ValidationErrors;
use rustcms::admin::{Ctx, SettingField, SettingsMeta, SettingsPage};
use rustcms::async_trait;

use crate::settings::rules::{checked, filled, is_web_url};
use crate::settings::seo_settings::{SOCIAL_NETWORKS, SeoSettings};

// TODO(C1): emit the `settings` outbox event after a save, once RustCMS 0.9 gives
// `SettingsPage` an after-save hook (0.8.2 has none).
#[async_trait]
impl SettingsPage for SeoSettings {
    fn meta() -> SettingsMeta {
        SettingsMeta::new("SEO and social").icon("globe")
    }

    fn fields() -> Vec<SettingField> {
        vec![
            SettingField::text("meta_title")
                .label("Default meta title")
                .max_length(70)
                .help("Used when a page sets no meta title"),
            SettingField::textarea("meta_description")
                .label("Default meta description")
                .max_length(160)
                .help("Used when a page sets no meta description"),
            SettingField::text("og_image_url")
                .label("Default share image URL")
                .nullable()
                .max_length(2000)
                .url()
                .help("An https address of the image shown when a page without its own is shared"),
            SettingField::key_value("social_links")
                .label("Social links")
                .max_items(SOCIAL_NETWORKS.len())
                .max_length(2000)
                .help("Keys: facebook, x, instagram, youtube, tiktok, linkedin, github. Values: https addresses."),
        ]
    }

    async fn validate(ctx: &Ctx, settings: &Self) -> Result<(), ValidationErrors> {
        let mut errors = ValidationErrors::new();
        if let Some(url) = filled(&settings.og_image_url)
            && !is_web_url(url, false)
        {
            errors.add("og_image_url", ctx.t("must be an https address"));
        }
        if settings.social_links.keys().any(|key| !SOCIAL_NETWORKS.contains(&key.as_str())) {
            errors.add(
                "social_links",
                ctx.t("may only use the keys facebook, x, instagram, youtube, tiktok, linkedin and github"),
            );
        }
        if settings.social_links.values().any(|url| !is_web_url(url.trim(), false)) {
            errors.add("social_links", ctx.t("must have https addresses as values"));
        }
        checked(errors)
    }
}
