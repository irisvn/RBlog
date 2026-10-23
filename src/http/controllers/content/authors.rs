//! `GET /api/v1/content/authors/{slug}`: the public profile of an author with a published post.

use rustcms::models::user;
use rustcms::prelude::*;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QuerySelect};

use super::{base_url, now, public_links};
use crate::http::resources::content::author::AuthorJson;
use crate::models::{author_profile, post};

/// The profile `slug`. Authors without a published post are not public: 404, like an unknown slug.
pub async fn show(State(app): State<AppState>, Path(slug): Path<String>) -> AppResult<Json<AuthorJson>> {
    let db = app.db();
    let profile = author_profile::Entity::find()
        .filter(author_profile::Column::Slug.eq(slug.as_str()))
        .one(db)
        .await?
        .ok_or(AppError::NotFound)?;
    let published = post::Entity::find()
        .select_only()
        .column(post::Column::Id)
        .filter(post::public_at(now()))
        .filter(post::Column::AuthorId.eq(profile.user_id))
        .into_tuple::<i64>()
        .one(db)
        .await?;
    if published.is_none() {
        return Err(AppError::NotFound);
    }
    let name: String = user::Entity::find_by_id(profile.user_id)
        .select_only()
        .column(user::Column::Name)
        .into_tuple()
        .one(db)
        .await?
        .ok_or(AppError::NotFound)?;
    let base = base_url(&app);
    let links = public_links(&base);
    let avatar_id = profile.avatar_media_id;
    let images = links.images(db, avatar_id).await?;
    let avatar = avatar_id.and_then(|id| images.get(&id));
    Ok(Json(AuthorJson::new(&links, profile, name, avatar)))
}
