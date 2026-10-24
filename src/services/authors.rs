//! Author profiles: the public side of a user who writes, one per user. The rules every writer
//! follows (the panel's `authors` resource, the demo seeder):
//!
//! - the slug is made from the user's name when left empty, a typed one is slugified, and it is
//!   unique; a rename keeps the old slug in `slug_history` (`services::taxonomy`, subject
//!   `author`);
//! - the website is an `https://` address (`http://` only for localhost);
//! - every write records an `author.*` outbox event, and re-syncs the `public_media` rows of the
//!   author's posts, so that the avatar is public exactly while one of their posts is
//!   (`services::public_media`).

use rustcms::models::user;
use rustcms::{AppResult, ValidationErrors};
use sea_orm::prelude::{DateTimeUtc, Uuid};
use sea_orm::{ActiveModelTrait, DatabaseTransaction, EntityTrait, Set};

use crate::models::author_profile;
use crate::services::public_media::{self, Subject};
use crate::services::taxonomy;
use crate::settings::rules::is_web_url;

/// The longest bio, in characters (the panel field's `max_length`).
pub const BIO_MAX: usize = 1000;
/// The message of a user who already has a profile (translated by the caller).
pub const ALREADY_HAS_PROFILE: &str = "already has an author profile";
/// The message of a website that is not a web address (translated by the caller).
pub const WEBSITE_INVALID: &str = "must be an https address (http only for localhost)";
/// The framework's own message for a missing record.
const USER_MISSING: &str = "does not exist";

/// The website as stored: trimmed, `None` when blank. A 422 on `website` when it is not an
/// `https://` address (`http://` for localhost only).
pub fn website(value: Option<&str>) -> AppResult<Option<String>> {
    match value.map(str::trim).filter(|v| !v.is_empty()) {
        None => Ok(None),
        Some(url) if is_web_url(url, true) => Ok(Some(url.to_owned())),
        Some(_) => Err(ValidationErrors::field("website", WEBSITE_INVALID).into()),
    }
}

/// The slug of a new profile for `user_id`: `typed`, slugified, or made from the user's name.
/// A 422 on `user_id` when the user is missing or already has a profile.
pub async fn slug_for_create(txn: &DatabaseTransaction, user_id: i64, typed: Option<&str>) -> AppResult<String> {
    let Some(user) = user::Entity::find_by_id(user_id).one(txn).await? else {
        return Err(ValidationErrors::field("user_id", USER_MISSING).into());
    };
    if author_profile::Entity::find_by_id(user_id).one(txn).await?.is_some() {
        return Err(ValidationErrors::field("user_id", ALREADY_HAS_PROFILE).into());
    }
    taxonomy::slug_for_create::<author_profile::Entity>(txn, &user.name, typed).await
}

/// The slug of `current` after an update, or `None` when it stays (see
/// [`taxonomy::slug_for_update`]; a blank one is made again from the user's current name). A
/// replaced slug is kept in `slug_history`, written at `at` (the profile's new `updated_at`).
pub async fn slug_for_update(
    txn: &DatabaseTransaction,
    current: &author_profile::Model,
    typed: Option<Option<&str>>,
    at: DateTimeUtc,
) -> AppResult<Option<String>> {
    let name = match typed {
        // Only a blank slug needs the name.
        Some(_) => user::Entity::find_by_id(current.user_id).one(txn).await?.map(|u| u.name).unwrap_or_default(),
        None => String::new(),
    };
    taxonomy::slug_for_update::<author_profile::Entity>(txn, current.user_id, &current.slug, &name, typed, at).await
}

/// After a profile is stored (created or updated): its outbox event, then its posts'
/// `public_media` rows (the avatar may have changed).
pub async fn saved(txn: &DatabaseTransaction, profile: &author_profile::Model) -> AppResult<()> {
    taxonomy::saved::<author_profile::Entity>(
        txn,
        profile.user_id,
        &profile.slug,
        profile.created_at,
        profile.updated_at,
    )
    .await?;
    public_media::sync(txn, Subject::Author, profile.user_id).await?;
    Ok(())
}

/// After a profile is deleted: its outbox event (old slugs forgotten), and its avatar no longer
/// public through the author's posts.
pub async fn deleted(txn: &DatabaseTransaction, profile: &author_profile::Model) -> AppResult<()> {
    taxonomy::deleted::<author_profile::Entity>(txn, profile.user_id, &profile.slug).await?;
    public_media::sync(txn, Subject::Author, profile.user_id).await?;
    Ok(())
}

/// A new profile, for writers other than the panel (the demo seeder).
pub struct NewProfile<'a> {
    pub user_id: i64,
    /// Blank or `None`: made from the user's name.
    pub slug: Option<&'a str>,
    pub bio: Option<&'a str>,
    pub avatar_media_id: Option<Uuid>,
    pub website: Option<&'a str>,
}

/// Creates a profile with the rules above, in `txn`.
pub async fn create(txn: &DatabaseTransaction, new: NewProfile<'_>) -> AppResult<author_profile::Model> {
    if new.bio.is_some_and(|bio| bio.chars().count() > BIO_MAX) {
        return Err(ValidationErrors::field("bio", format!("must be at most {BIO_MAX} characters")).into());
    }
    let website = website(new.website)?;
    let slug = slug_for_create(txn, new.user_id, new.slug).await?;
    let now = super::content_sync::now();
    let profile = author_profile::ActiveModel {
        user_id: Set(new.user_id),
        slug: Set(slug),
        bio: Set(new.bio.map(str::to_owned)),
        avatar_media_id: Set(new.avatar_media_id),
        website: Set(website),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(txn)
    .await?;
    saved(txn, &profile).await?;
    Ok(profile)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn websites_are_https_or_local_http() {
        assert_eq!(website(Some(" https://an.example/blog ")).unwrap(), Some("https://an.example/blog".into()));
        assert_eq!(website(Some("http://localhost:3000")).unwrap(), Some("http://localhost:3000".into()));
        assert_eq!(website(Some("  ")).unwrap(), None);
        assert_eq!(website(None).unwrap(), None);
        assert!(website(Some("http://an.example")).is_err());
        assert!(website(Some("javascript:alert(1)")).is_err());
        assert!(website(Some("https://user@an.example")).is_err());
    }
}
