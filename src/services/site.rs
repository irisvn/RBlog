//! The site settings the public site may read (`GET /api/v1/content/settings`, phase P3).
//!
//! Only public values: never the `integrations` group (the webhook's address and secret).
//!
//! Images (the logo, the default share image) are `{url, width, height, alt}` at their public
//! URL, or `null`: they are public through `public_media` rows of subject `settings`, which the
//! settings pages write when they are saved. An image in the trash, or not an image any more, is
//! `null`.
//!
//! The share image of RBlog 0.1 was a text setting, `og_image_url`. It stays readable: while no
//! `og_image` is chosen, a saved `og_image_url` is `seo.og_image` as `{url, width: null,
//! height: null, alt: ""}`.

use std::collections::BTreeMap;

use rustcms::AppState;
use rustcms::settings::get;
use serde::Serialize;

use crate::http::resources::content::{ImageJson, Links};
use crate::services::public_media;

use crate::settings::general_settings::GeneralSettings;
use crate::settings::reading_settings::ReadingSettings;
use crate::settings::rules::filled;
use crate::settings::seo_settings::SeoSettings;

/// What the public site is told about the site.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PublicSettings {
    pub site_name: String,
    pub description: String,
    /// `vi` or `en`.
    pub site_language: String,
    /// Without its trailing slash.
    pub frontend_url: Option<String>,
    pub posts_per_page: i32,
    /// The site's logo.
    pub logo: Option<ImageJson>,
    pub seo: PublicSeo,
    pub homepage: Homepage,
}

/// SEO defaults and social links.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PublicSeo {
    pub meta_title: String,
    pub meta_description: String,
    /// The default share image.
    pub og_image: Option<ImageJson>,
    /// Network (`facebook`, `x`, …) → `https://` address.
    pub social_links: BTreeMap<String, String>,
}

/// What the homepage shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Homepage {
    /// `latest_posts` or `page`.
    pub shows: String,
    /// The page's slug when it `shows` a `page`.
    pub page_slug: Option<String>,
}

/// The public settings, as saved (or their defaults).
pub async fn public_settings(state: &AppState) -> anyhow::Result<PublicSettings> {
    let general: GeneralSettings = get(state).await?;
    let seo: SeoSettings = get(state).await?;
    let reading: ReadingSettings = get(state).await?;
    let images = public_media::public_images(state.db(), general.logo.into_iter().chain(seo.og_image)).await?;
    let base = state.config().app.url.trim_end_matches('/').to_owned();
    let links = Links::Public { base: &base };
    let image =
        |id: Option<sea_orm::prelude::Uuid>| id.and_then(|id| images.get(&id)).map(|m| ImageJson::new(&links, m));
    let og_image = match seo.og_image {
        Some(id) => image(Some(id)),
        // The text setting of RBlog 0.1 (see the module's documentation).
        None => filled(&seo.og_image_url).map(|url| ImageJson {
            url: url.to_owned(),
            width: None,
            height: None,
            alt: String::new(),
        }),
    };
    Ok(PublicSettings {
        logo: image(general.logo),
        frontend_url: general.frontend_url().map(str::to_owned),
        site_name: general.site_name.trim().to_owned(),
        description: general.description,
        site_language: general.site_language,
        posts_per_page: general.posts_per_page,
        seo: PublicSeo {
            meta_title: seo.meta_title,
            meta_description: seo.meta_description,
            og_image,
            social_links: seo.social_links.into_iter().map(|(k, v)| (k, v.trim().to_owned())).collect(),
        },
        homepage: Homepage { page_slug: reading.homepage_page().map(str::to_owned), shows: reading.homepage_shows },
    })
}
