//! The admin page of [`SeoSettings`] (`/admin/settings/seo`): which fields the form shows and
//! how they are checked.

use rustcms::admin::{Ctx, SettingField, SettingsMeta, SettingsPage};
use rustcms::{AppResult, AppState, ValidationErrors, async_trait};
use sea_orm::DatabaseTransaction;

use crate::services::content_sync;
use crate::services::public_media::{self, IMAGE_MIMES, SettingsImages};
use crate::settings::rules::{checked, filled, is_web_url};
use crate::settings::seo_settings::{SOCIAL_NETWORKS, SeoSettings};

/// The share image's largest size.
pub const OG_IMAGE_MAX_BYTES: u64 = 5_000_000;

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
            SettingField::media("og_image")
                .label("Default share image")
                .accept(&IMAGE_MIMES)
                .max_size(OG_IMAGE_MAX_BYTES)
                .help("Shown when a page without its own image is shared. 1200 × 630 px suits most networks."),
            // The text setting of RBlog 0.1, kept so that a saved address keeps working (and can
            // be cleared): `services::site` reads it only while `og_image` is unset.
            SettingField::text("og_image_url")
                .label("Old share image URL")
                .nullable()
                .max_length(2000)
                .url()
                .help("Used only while no default share image is chosen above. Clear it once one is."),
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

    /// The share image is public while it is set (`public_media`), and the public site learns of
    /// the change (`settings.updated`).
    async fn after_save(
        _state: &AppState,
        _ctx: &Ctx,
        txn: &DatabaseTransaction,
        _before: &Self,
        after: &Self,
    ) -> AppResult<()> {
        public_media::sync_settings(txn, SettingsImages::Seo, after.og_image).await?;
        content_sync::settings_changed(txn).await
    }
}
