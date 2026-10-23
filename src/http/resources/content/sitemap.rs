//! A batch of sitemap entries.

use serde::Serialize;

/// `{type, path, lastmod}`.
#[derive(Debug, Clone, Serialize)]
pub struct SitemapEntry {
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub path: String,
    pub lastmod: String,
}

/// `{data, next}`: `next` is the cursor of the following batch (`after=`), null after the last.
#[derive(Debug, Clone, Serialize)]
pub struct SitemapJson {
    pub data: Vec<SitemapEntry>,
    pub next: Option<String>,
}
