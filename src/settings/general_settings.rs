//! The `general` settings group: stored in the database, edited at
//! `/admin/settings/general` (`crate::admin::pages::general_settings_page`),
//! read with `rustcms::settings::get::<GeneralSettings>(&state)`.

use std::ops::RangeInclusive;

use rustcms::settings::Settings;
use sea_orm::prelude::Uuid;
use serde::{Deserialize, Serialize};

use super::rules::{filled, without_trailing_slash};

/// Fewest and most posts on a page of the public site.
pub const POSTS_PER_PAGE: RangeInclusive<i32> = 1..=100;

/// The site's identity and how the public site lists posts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneralSettings {
    pub site_name: String,
    pub description: String,
    /// `vi` or `en`: the language the public site is written in.
    pub site_language: String,
    /// Where the public site is served; read it with [`GeneralSettings::frontend_url`].
    pub frontend_url: Option<String>,
    pub posts_per_page: i32,
    /// The site's logo: an image of the media library (PNG, WebP or JPEG).
    pub logo: Option<Uuid>,
}

impl Default for GeneralSettings {
    fn default() -> Self {
        Self {
            site_name: "RBlog".into(),
            description: String::new(),
            site_language: "vi".into(),
            frontend_url: None,
            posts_per_page: 10,
            logo: None,
        }
    }
}

impl Settings for GeneralSettings {
    const GROUP: &'static str = "general";
}

impl GeneralSettings {
    /// The public site's address without its trailing slash, if set.
    pub fn frontend_url(&self) -> Option<&str> {
        filled(&self.frontend_url).map(without_trailing_slash)
    }
}
