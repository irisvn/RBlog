//! The `integrations` settings group: stored in the database, edited at
//! `/admin/settings/integrations` (`crate::admin::pages::integration_settings_page`),
//! read with `rustcms::settings::get::<IntegrationSettings>(&state)`.
//!
//! Nothing here is public: `crate::services::site::public_settings` never reads this group.

use rustcms::settings::{Secret, Settings};
use serde::{Deserialize, Serialize};

use super::rules::filled;

/// Where the public site is told about changes, and how it previews drafts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntegrationSettings {
    /// An `https://` address the webhook is sent to; unset: no webhook.
    pub webhook_url: Option<String>,
    /// Signs the webhook. Kept only while `webhook_url` keeps its value.
    pub webhook_secret: Secret,
    /// The public site's preview route (`/preview`): a path starting with `/`.
    pub preview_path: String,
}

impl Default for IntegrationSettings {
    fn default() -> Self {
        Self { webhook_url: None, webhook_secret: Secret::default(), preview_path: "/preview".into() }
    }
}

impl Settings for IntegrationSettings {
    const GROUP: &'static str = "integrations";
}

impl IntegrationSettings {
    /// The webhook's address, if set.
    pub fn webhook_url(&self) -> Option<&str> {
        filled(&self.webhook_url)
    }
}
