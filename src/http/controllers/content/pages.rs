//! `GET /api/v1/content/pages[/{slug}]`: published pages.

use rustcms::axum::response::{IntoResponse, Response};
use rustcms::prelude::*;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect};

use super::{Params, base_url, now, old_slug, public_links};
use crate::http::resources::content::RedirectJson;
use crate::http::resources::content::entry::{self, Detail, PageJson};
use crate::models::page;

/// Published pages, newest first (`published_at`, then `id`).
pub async fn index(State(app): State<AppState>, page: Pagination) -> AppResult<Json<ApiPage<PageJson>>> {
    let select = page::Entity::find()
        .filter(page::public_at(now()))
        .order_by_desc(page::Column::PublishedAt)
        .order_by_desc(page::Column::Id);
    let rows = ApiPage::fetch(select, app.db(), &page, |p| p).await?;
    let base = base_url(&app);
    let data = entry::pages(app.db(), &public_links(&base), &rows.data, Detail::default()).await?;
    Ok(Json(ApiPage { data, meta: rows.meta }))
}

/// One published page with its `html` (and `document` with `?include=document`). A slug the page
/// had before answers `{"redirect": "<current slug>"}`.
pub async fn show(
    State(app): State<AppState>,
    Path(slug): Path<String>,
    Query(query): Query<Vec<(String, String)>>,
) -> AppResult<Response> {
    let params = Params::new(query, &["include"])?;
    let detail = Detail { html: true, document: params.include_document()? };
    let db = app.db();
    let public = || page::Entity::find().filter(page::public_at(now()));
    let Some(found) = public().filter(page::Column::Slug.eq(slug.as_str())).one(db).await? else {
        let Some(id) = old_slug(db, "page", &slug).await? else { return Err(AppError::NotFound) };
        let current = public().select_only().column(page::Column::Slug).filter(page::Column::Id.eq(id));
        let current: String = current.into_tuple().one(db).await?.ok_or(AppError::NotFound)?;
        return Ok(Json(RedirectJson { redirect: current }).into_response());
    };
    let base = base_url(&app);
    let mut json = entry::pages(db, &public_links(&base), std::slice::from_ref(&found), detail).await?;
    Ok(Json(json.remove(0)).into_response())
}
