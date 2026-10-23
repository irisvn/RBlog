//! An author's public profile.

use serde::Serialize;

use super::{ImageJson, Links};
use crate::models::author_profile;

/// `{slug, name, bio, website, avatar}`: never the user's email or account data.
#[derive(Debug, Clone, Serialize)]
pub struct AuthorJson {
    pub slug: String,
    pub name: String,
    pub bio: Option<String>,
    pub website: Option<String>,
    pub avatar: Option<ImageJson>,
}

impl AuthorJson {
    /// `profile` of the user named `name`; `avatar` when it is a public image.
    pub fn new(
        links: &Links<'_>,
        profile: author_profile::Model,
        name: String,
        avatar: Option<&rustcms::admin::media::entity::Model>,
    ) -> Self {
        Self {
            slug: profile.slug,
            name,
            bio: profile.bio,
            website: profile.website,
            avatar: avatar.map(|m| ImageJson::new(links, m)),
        }
    }
}
