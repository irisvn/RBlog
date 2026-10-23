//! The `reading` settings group: stored in the database, edited at
//! `/admin/settings/reading` (`crate::admin::pages::reading_settings_page`),
//! read with `rustcms::settings::get::<ReadingSettings>(&state)`.

use rustcms::settings::Settings;
use serde::{Deserialize, Serialize};

use super::rules::filled;

/// `homepage_shows`: the latest posts.
pub const HOMEPAGE_LATEST_POSTS: &str = "latest_posts";
/// `homepage_shows`: the page `homepage_page_slug`.
pub const HOMEPAGE_PAGE: &str = "page";

/// What the public site's homepage shows.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadingSettings {
    /// [`HOMEPAGE_LATEST_POSTS`] or [`HOMEPAGE_PAGE`].
    pub homepage_shows: String,
    /// The page shown with [`HOMEPAGE_PAGE`]; read it with [`ReadingSettings::homepage_page`].
    pub homepage_page_slug: Option<String>,
}

impl Default for ReadingSettings {
    fn default() -> Self {
        Self { homepage_shows: HOMEPAGE_LATEST_POSTS.into(), homepage_page_slug: None }
    }
}

impl Settings for ReadingSettings {
    const GROUP: &'static str = "reading";
}

impl ReadingSettings {
    /// The slug of the page the homepage shows, or `None` when it shows the latest posts.
    pub fn homepage_page(&self) -> Option<&str> {
        if self.homepage_shows == HOMEPAGE_PAGE { filled(&self.homepage_page_slug) } else { None }
    }
}
