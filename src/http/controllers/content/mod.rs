//! Handlers of the read-only Content API (`/api/v1/content`, docs/api.md), the public images
//! (`/api/v1/media`) and previews (`/api/v1/preview`). Routes and their access are declared in
//! `crate::routes::api` (checked before these run).

pub mod authors;
pub mod media;
pub mod menus;
pub mod pages;
pub mod posts;
pub mod preview;
pub mod settings;
pub mod sitemap;
pub mod taxonomies;

use std::net::IpAddr;

use rustcms::auth::AuthState;
use rustcms::prelude::*;
use sea_orm::{ColumnTrait, ConnectionTrait, DbErr, EntityTrait, QueryFilter, QuerySelect};

use crate::http::resources::content::Links;
use crate::models::slug_history;

/// `APP_URL` without its trailing slash: the origin of absolute URLs (images).
pub(crate) fn base_url(app: &AppState) -> String {
    app.config().app.url.trim_end_matches('/').to_owned()
}

/// Links to public images, for published content.
pub(crate) fn public_links(base: &str) -> Links<'_> {
    Links::Public { base }
}

/// The query string's parameters this handler reads, each at most once (422 otherwise). Other
/// parameters (`page`, `per_page`, …) are left alone.
pub(crate) struct Params(Vec<(String, String)>);

impl Params {
    /// Reads `names` from `query`.
    pub(crate) fn new(query: Vec<(String, String)>, names: &[&str]) -> Result<Self, ValidationErrors> {
        let mut errors = ValidationErrors::new();
        let mut kept: Vec<(String, String)> = Vec::new();
        for (key, value) in query {
            if !names.contains(&key.as_str()) {
                continue;
            }
            if kept.iter().any(|(k, _)| *k == key) {
                if !errors.errors.contains_key(&key) {
                    errors.add(&key, "may only be given once");
                }
                continue;
            }
            kept.push((key, value));
        }
        if errors.is_empty() { Ok(Self(kept)) } else { Err(errors) }
    }

    /// The value of `name`, when given and not blank.
    pub(crate) fn get(&self, name: &str) -> Option<&str> {
        self.0.iter().find(|(k, _)| k == name).map(|(_, v)| v.trim()).filter(|v| !v.is_empty())
    }

    /// `include=document` (the only value `include` takes).
    pub(crate) fn include_document(&self) -> Result<bool, ValidationErrors> {
        match self.get("include") {
            None => Ok(false),
            Some("document") => Ok(true),
            Some(_) => Err(ValidationErrors::field("include", "is invalid")),
        }
    }
}

/// The id of the `subject_type` record that had `slug` before, if any.
pub(crate) async fn old_slug<C: ConnectionTrait>(db: &C, subject_type: &str, slug: &str) -> Result<Option<i64>, DbErr> {
    slug_history::Entity::find()
        .select_only()
        .column(slug_history::Column::SubjectId)
        .filter(slug_history::Column::SubjectType.eq(subject_type))
        .filter(slug_history::Column::OldSlug.eq(slug))
        .into_tuple()
        .one(db)
        .await
}

/// Counts one request of `ip` against `max` a minute for `bucket` (429 past it), in the limiter
/// sign-in uses (shared through Redis when configured).
pub(crate) async fn throttle_ip(app: &AppState, bucket: &str, ip: IpAddr, max: u32) -> AppResult<()> {
    let auth = app.service::<AuthState>().ok_or_else(|| AppError::internal("AuthProvider not registered"))?;
    auth.limiter.reserve(&[(&format!("{bucket}:{ip}"), max)]).await
}

/// The current time.
pub(crate) fn now() -> sea_orm::prelude::DateTimeUtc {
    crate::services::content_sync::now()
}
