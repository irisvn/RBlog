//! `GET /api/v1/content/categories[/{slug}]` and `GET /api/v1/content/tags[/{slug}]`, with their
//! number of published posts.

use rustcms::axum::response::{IntoResponse, Response};
use rustcms::prelude::*;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect};

use super::{now, old_slug};
use crate::http::resources::content::RedirectJson;
use crate::http::resources::content::taxonomy::{self, CategoryJson, TagJson};
use crate::models::{category, tag};

/// Every category, by name.
pub async fn categories(State(app): State<AppState>, page: Pagination) -> AppResult<Json<ApiPage<CategoryJson>>> {
    let select = category::Entity::find().order_by_asc(category::Column::Name).order_by_asc(category::Column::Id);
    let rows = ApiPage::fetch(select, app.db(), &page, |c| c).await?;
    let data = taxonomy::categories(app.db(), rows.data, now()).await?;
    Ok(Json(ApiPage { data, meta: rows.meta }))
}

/// One category; an old slug answers `{"redirect": "<current slug>"}`.
pub async fn category(State(app): State<AppState>, Path(slug): Path<String>) -> AppResult<Response> {
    let db = app.db();
    let Some(found) = category::Entity::find().filter(category::Column::Slug.eq(slug.as_str())).one(db).await? else {
        let Some(id) = old_slug(db, "category", &slug).await? else { return Err(AppError::NotFound) };
        let current = category::Entity::find_by_id(id).select_only().column(category::Column::Slug);
        let current: String = current.into_tuple().one(db).await?.ok_or(AppError::NotFound)?;
        return Ok(Json(RedirectJson { redirect: current }).into_response());
    };
    let mut json = taxonomy::categories(db, vec![found], now()).await?;
    Ok(Json(json.remove(0)).into_response())
}

/// Every tag, by name.
pub async fn tags(State(app): State<AppState>, page: Pagination) -> AppResult<Json<ApiPage<TagJson>>> {
    let select = tag::Entity::find().order_by_asc(tag::Column::Name).order_by_asc(tag::Column::Id);
    let rows = ApiPage::fetch(select, app.db(), &page, |t| t).await?;
    let data = taxonomy::tags(app.db(), rows.data, now()).await?;
    Ok(Json(ApiPage { data, meta: rows.meta }))
}

/// One tag; an old slug answers `{"redirect": "<current slug>"}`.
pub async fn tag(State(app): State<AppState>, Path(slug): Path<String>) -> AppResult<Response> {
    let db = app.db();
    let Some(found) = tag::Entity::find().filter(tag::Column::Slug.eq(slug.as_str())).one(db).await? else {
        let Some(id) = old_slug(db, "tag", &slug).await? else { return Err(AppError::NotFound) };
        let current = tag::Entity::find_by_id(id).select_only().column(tag::Column::Slug);
        let current: String = current.into_tuple().one(db).await?.ok_or(AppError::NotFound)?;
        return Ok(Json(RedirectJson { redirect: current }).into_response());
    };
    let mut json = taxonomy::tags(db, vec![found], now()).await?;
    Ok(Json(json.remove(0)).into_response())
}
