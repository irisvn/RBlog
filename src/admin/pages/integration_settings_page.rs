//! The admin page of [`IntegrationSettings`] (`/admin/settings/integrations`): which fields the
//! form shows and how they are checked.

use rustcms::ValidationErrors;
use rustcms::admin::{Ctx, SettingField, SettingsMeta, SettingsPage};
use rustcms::async_trait;

use crate::settings::integration_settings::IntegrationSettings;
use crate::settings::rules::{checked, is_site_path, is_web_url};

#[async_trait]
impl SettingsPage for IntegrationSettings {
    fn meta() -> SettingsMeta {
        SettingsMeta::new("Integrations").icon("workflow")
    }

    fn fields() -> Vec<SettingField> {
        vec![
            SettingField::text("webhook_url")
                .label("Webhook URL")
                .nullable()
                .max_length(2000)
                .url()
                .help("An https address told about each change. Leave it empty for no webhook."),
            // Write-only (docs/ui-guidelines.md §5): the page shows only whether it is kept, and a
            // new webhook address without a new secret clears the old one.
            SettingField::secret("webhook_secret")
                .label("Webhook secret")
                .kept_while(&["webhook_url"])
                .help("Required with a webhook URL. Changing the URL clears it."),
            SettingField::text("preview_path")
                .label("Preview path")
                .required()
                .max_length(200)
                .help("The public site's preview route, for example /preview"),
        ]
    }

    async fn validate(ctx: &Ctx, settings: &Self) -> Result<(), ValidationErrors> {
        let mut errors = ValidationErrors::new();
        if let Some(url) = settings.webhook_url() {
            if !is_web_url(url, false) {
                errors.add("webhook_url", ctx.t("must be an https address"));
            }
            if !settings.webhook_secret.is_set() {
                errors.add("webhook_secret", ctx.t("is required when a webhook URL is set"));
            }
        }
        if !is_site_path(&settings.preview_path) {
            errors.add("preview_path", ctx.t("must be a path starting with /"));
        }
        checked(errors)
    }
}
