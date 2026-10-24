//! The admin page of [`GeneralSettings`] (`/admin/settings/general`): which fields the form
//! shows and how they are checked.

use rustcms::admin::{Ctx, SettingField, SettingsMeta, SettingsPage};
use rustcms::{AppResult, AppState, ValidationErrors, async_trait};
use sea_orm::DatabaseTransaction;

use crate::services::content_sync;
use crate::services::public_media::{self, SettingsImages};
use crate::settings::general_settings::{GeneralSettings, POSTS_PER_PAGE};
use crate::settings::rules::{checked, filled, is_web_url};

/// The logo's accepted types and size.
pub const LOGO_TYPES: [&str; 3] = ["image/png", "image/webp", "image/jpeg"];
pub const LOGO_MAX_BYTES: u64 = 1_000_000;

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
            SettingField::media("logo")
                .label("Logo")
                .accept(&LOGO_TYPES)
                .max_size(LOGO_MAX_BYTES)
                .help("PNG, WebP or JPEG, at most 1 MB. The public site shows it in its header."),
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

    /// The logo is public while it is set (`public_media`), and the public site learns of the
    /// change (`settings.updated`).
    async fn after_save(
        _state: &AppState,
        _ctx: &Ctx,
        txn: &DatabaseTransaction,
        _before: &Self,
        after: &Self,
    ) -> AppResult<()> {
        public_media::sync_settings(txn, SettingsImages::General, after.logo).await?;
        content_sync::settings_changed(txn).await
    }
}
