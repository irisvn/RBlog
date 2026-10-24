//! Author profiles: the public side of a user who writes, one per user (the profile's key is
//! the user's id, chosen on create and never changed). The rules live in `services::authors`:
//! the slug (made from the user's name when left empty, unique, old ones kept for redirects),
//! the website, the outbox event and the avatar's `public_media` rows.
//!
//! Default permissions (`authors.*`). Letting authors edit their own profile is a later role
//! rule (a policy over `user_id`), not built yet.

use rustcms::admin::*;
use rustcms::{AppResult, async_trait};
use sea_orm::DatabaseTransaction;
use serde_json::Value;

use super::{id, instant, text, translated};
use crate::models::author_profile;
use crate::services::{authors, content_sync};

pub struct AuthorProfileResource;

#[async_trait]
impl Resource for AuthorProfileResource {
    type Entity = author_profile::Entity;
    const SLUG: &'static str = "authors";

    fn meta() -> ResourceMeta {
        ResourceMeta::new("Author")
            .plural("Authors")
            .title("slug")
            .icon("user-pen")
            .group("Content")
            .key(KeyStrategy::Natural)
            .unique("author_profiles_slug_key", &["slug"], "is already taken")
            // Two profiles for one user at once: the second insert hits the primary key.
            .unique("PRIMARY", &["user_id"], authors::ALREADY_HAS_PROFILE)
    }

    /// Readable/writable fields. Only declared columns are ever exposed.
    fn fields() -> Vec<Field> {
        vec![
            Field::column(author_profile::Column::UserId)
                .label("User")
                .required()
                .belongs_to("users")
                .updatable(|_| false),
            Field::column(author_profile::Column::Slug).searchable().sortable().max_length(160),
            Field::column(author_profile::Column::Bio).max_length(authors::BIO_MAX),
            Field::column(author_profile::Column::AvatarMediaId).label("Avatar").media().accept(&["image/*"]),
            Field::column(author_profile::Column::Website).max_length(2048),
            Field::column(author_profile::Column::CreatedAt).guarded().readable(|_| true).sortable(),
        ]
    }

    fn form() -> Form {
        Form::new().columns(2).schema(components![
            SelectInput::make("user_id").help("A user without an author profile. It cannot be changed later."),
            TextInput::make("slug").help("Left empty: made from the user's name"),
            Textarea::make("bio").help("At most 1000 characters").column_span(2),
            FileUpload::make("avatar_media_id"),
            TextInput::make("website")
                .help("An https address, for example https://example.com (http only for localhost)"),
        ])
    }

    fn table() -> Table {
        Table::new()
            .columns(vec![
                TextColumn::make("user_id"),
                TextColumn::make("slug"),
                TextColumn::make("website"),
                DateColumn::make("created_at"),
            ])
            .default_sort("slug")
    }

    async fn mutate_before_create(ctx: &Ctx, txn: &DatabaseTransaction, mut data: Data) -> AppResult<Data> {
        normalize_website(&mut data).map_err(|e| translated(ctx, Self::SLUG, e))?;
        let user_id = id(&data, "user_id").flatten().unwrap_or_default();
        let typed = text(&data, "slug").flatten();
        let slug = authors::slug_for_create(txn, user_id, typed).await.map_err(|e| translated(ctx, Self::SLUG, e))?;
        let now = instant(content_sync::now());
        data.insert("slug".into(), Value::String(slug));
        data.insert("created_at".into(), now.clone());
        data.insert("updated_at".into(), now);
        Ok(data)
    }

    async fn mutate_before_update(
        ctx: &Ctx,
        txn: &DatabaseTransaction,
        current: &author_profile::Model,
        mut data: Data,
    ) -> AppResult<Data> {
        normalize_website(&mut data).map_err(|e| translated(ctx, Self::SLUG, e))?;
        let now = content_sync::now();
        match authors::slug_for_update(txn, current, text(&data, "slug"), now)
            .await
            .map_err(|e| translated(ctx, Self::SLUG, e))?
        {
            Some(new) => data.insert("slug".into(), Value::String(new)),
            // Unchanged: a typed form of the same slug is not written again.
            None => data.remove("slug"),
        };
        data.insert("updated_at".into(), instant(now));
        Ok(data)
    }

    async fn after_save(_ctx: &Ctx, txn: &DatabaseTransaction, saved: &author_profile::Model) -> AppResult<()> {
        authors::saved(txn, saved).await
    }

    async fn after_delete(_ctx: &Ctx, txn: &DatabaseTransaction, deleted: &author_profile::Model) -> AppResult<()> {
        authors::deleted(txn, deleted).await
    }
}

/// A sent `website` stored as [`authors::website`] makes it (trimmed, blank as none), or a 422.
fn normalize_website(data: &mut Data) -> AppResult<()> {
    if let Some(sent) = text(data, "website") {
        let value = authors::website(sent)?.map_or(Value::Null, Value::String);
        data.insert("website".into(), value);
    }
    Ok(())
}
