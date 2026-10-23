//! What the Content API returns (docs/api.md), like Laravel API resources: ids as strings,
//! instants in RFC 3339 UTC. The builders batch-load what the records point to (category, tags,
//! author, images), one query per kind for a whole page.

pub mod author;
pub mod entry;
pub mod menu;
pub mod sitemap;
pub mod taxonomy;

use rustcms::admin::media::entity as media;
use sea_orm::prelude::{DateTimeUtc, Uuid};
use serde::Serialize;

/// An instant as the API writes it: RFC 3339, UTC, seconds (`2026-10-24T08:00:00Z`).
pub fn instant(at: DateTimeUtc) -> String {
    at.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// How image URLs are made, and which images may appear.
#[derive(Clone, Copy, Debug)]
pub enum Links<'a> {
    /// Published content: only public images (`public_media`), at
    /// `{APP_URL}/api/v1/media/{id}/{name}`.
    Public { base: &'a str },
    /// A preview: the subject's images (not trashed), at
    /// `{APP_URL}/api/v1/preview/media/{id}?token={token}`.
    Preview { base: &'a str, token: &'a str },
}

impl Links<'_> {
    /// The URL of `media`.
    pub fn url(&self, media: &media::Model) -> String {
        match self {
            Self::Public { base } => format!("{base}/api/v1/media/{}/{}", media.id, file_name(&media.name)),
            Self::Preview { base, token } => format!("{base}/api/v1/preview/media/{}?token={token}", media.id),
        }
    }

    /// The images among `ids` these links may show, by id.
    pub async fn images<C: sea_orm::ConnectionTrait>(
        &self,
        db: &C,
        ids: impl IntoIterator<Item = Uuid>,
    ) -> Result<std::collections::HashMap<Uuid, media::Model>, sea_orm::DbErr> {
        match self {
            Self::Public { .. } => crate::services::public_media::public_images(db, ids).await,
            Self::Preview { .. } => crate::services::public_media::images(db, ids).await,
        }
    }
}

/// `name` as the cosmetic last segment of a media URL: ASCII letters, digits, `.`, `-` and `_`,
/// anything else `-` (the server ignores it).
fn file_name(name: &str) -> String {
    let cleaned: String =
        name.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') { c } else { '-' }).collect();
    let cleaned = cleaned.trim_matches(|c| c == '.' || c == '-');
    if cleaned.is_empty() { "file".to_owned() } else { cleaned.chars().take(100).collect() }
}

/// An image: `{url, width, height, alt}`.
#[derive(Debug, Clone, Serialize)]
pub struct ImageJson {
    pub url: String,
    pub width: Option<i32>,
    pub height: Option<i32>,
    /// The media's name, until media get alt texts.
    pub alt: String,
}

impl ImageJson {
    pub fn new(links: &Links<'_>, media: &media::Model) -> Self {
        Self { url: links.url(media), width: media.width, height: media.height, alt: media.name.clone() }
    }
}

/// A category or tag a post points to: `{slug, name}`.
#[derive(Debug, Clone, Serialize)]
pub struct TermRef {
    pub slug: String,
    pub name: String,
}

/// `{"redirect": "<current slug>"}`: the slug asked for is an old one.
#[derive(Debug, Clone, Serialize)]
pub struct RedirectJson {
    pub redirect: String,
}

#[cfg(test)]
mod tests {
    use super::file_name;

    #[test]
    fn file_names_are_cosmetic_ascii() {
        assert_eq!(file_name("ảnh bìa.JPG"), "nh-b-a.JPG");
        assert_eq!(file_name("../../etc/passwd"), "etc-passwd");
        assert_eq!(file_name("..."), "file");
        assert_eq!(file_name("photo.png"), "photo.png");
    }
}
