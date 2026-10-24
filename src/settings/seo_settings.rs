//! The `seo` settings group: stored in the database, edited at
//! `/admin/settings/seo` (`crate::admin::pages::seo_settings_page`),
//! read with `rustcms::settings::get::<SeoSettings>(&state)`.

use std::collections::BTreeMap;

use rustcms::settings::Settings;
use sea_orm::prelude::Uuid;
use serde::{Deserialize, Serialize};

/// The networks a social link may point to (the keys of `social_links`).
pub const SOCIAL_NETWORKS: [&str; 7] = ["facebook", "x", "instagram", "youtube", "tiktok", "linkedin", "github"];

/// What search engines and social networks show when a page sets nothing itself.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SeoSettings {
    pub meta_title: String,
    pub meta_description: String,
    /// The default share image: an image of the media library.
    pub og_image: Option<Uuid>,
    /// The share image's `https://` address from before `og_image` (RBlog 0.1, a text setting).
    /// Kept as it was saved, and shown on the page so that it can be cleared: the public settings
    /// use it only while `og_image` is unset (`services::site`).
    pub og_image_url: Option<String>,
    /// Network (one of [`SOCIAL_NETWORKS`]) → `https://` address of the profile.
    pub social_links: BTreeMap<String, String>,
}

impl Settings for SeoSettings {
    const GROUP: &'static str = "seo";
}
