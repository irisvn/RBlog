//! `GET /api/v1/content/posts[/{slug}]`: published posts.

use rustcms::axum::response::{IntoResponse, Response};
use rustcms::prelude::*;
use sea_orm::sea_query::{Expr, ExprTrait, Query as SqlQuery};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect};

use super::{Params, base_url, now, old_slug, public_links};
use crate::http::resources::content::RedirectJson;
use crate::http::resources::content::entry::{self, Detail, PostJson};
use crate::models::{author_profile, category, post, post_tag, tag};
use crate::services::search::{MAX_QUERY_CHARS, like_pattern, normalize};

/// Published posts, newest first (`published_at`, then `id`), filtered by `category`, `tag` and
/// `author` (slugs), `featured` (`true`/`false`) and `q` (accent-insensitive search).
pub async fn index(
    State(app): State<AppState>,
    page: Pagination,
    Query(query): Query<Vec<(String, String)>>,
) -> AppResult<Json<ApiPage<PostJson>>> {
    let params = Params::new(query, &["category", "tag", "author", "featured", "q"])?;
    let mut select = post::Entity::find().filter(post::public_at(now()));
    if let Some(slug) = params.get("category") {
        let ids = SqlQuery::select()
            .column(category::Column::Id)
            .from(category::Entity)
            .and_where(category::Column::Slug.eq(slug))
            .to_owned();
        select = select.filter(post::Column::CategoryId.in_subquery(ids));
    }
    if let Some(slug) = params.get("tag") {
        // EXISTS, not a join: a post is listed once whatever its tags.
        let tagged = SqlQuery::select()
            .expr(Expr::val(1))
            .from(post_tag::Entity)
            .inner_join(
                tag::Entity,
                Expr::col((tag::Entity, tag::Column::Id)).equals((post_tag::Entity, post_tag::Column::TagId)),
            )
            .and_where(Expr::col((post_tag::Entity, post_tag::Column::PostId)).equals((post::Entity, post::Column::Id)))
            .and_where(Expr::col((tag::Entity, tag::Column::Slug)).eq(slug))
            .to_owned();
        select = select.filter(Expr::exists(tagged));
    }
    if let Some(slug) = params.get("author") {
        let ids = SqlQuery::select()
            .column(author_profile::Column::UserId)
            .from(author_profile::Entity)
            .and_where(author_profile::Column::Slug.eq(slug))
            .to_owned();
        select = select.filter(post::Column::AuthorId.in_subquery(ids));
    }
    match params.get("featured") {
        None => {}
        Some("true" | "1") => select = select.filter(post::Column::Featured.eq(true)),
        Some("false" | "0") => select = select.filter(post::Column::Featured.eq(false)),
        Some(_) => return Err(ValidationErrors::field("featured", "is invalid").into()),
    }
    if let Some(q) = params.get("q") {
        match like_pattern(q) {
            Some(pattern) => select = select.filter(post::Column::SearchText.like(pattern)),
            // Only punctuation the normalization drops: nothing to search for.
            None if normalize(q).is_empty() => {}
            None => {
                let message = format!("may not be longer than {MAX_QUERY_CHARS} characters");
                return Err(ValidationErrors::field("q", message).into());
            }
        }
    }
    let select = select.order_by_desc(post::Column::PublishedAt).order_by_desc(post::Column::Id);
    let rows = ApiPage::fetch(select, app.db(), &page, |p| p).await?;
    let base = base_url(&app);
    let data = entry::posts(app.db(), &public_links(&base), &rows.data, Detail::default()).await?;
    Ok(Json(ApiPage { data, meta: rows.meta }))
}

/// One published post with its `html` (and `document` with `?include=document`). A slug the post
/// had before answers `{"redirect": "<current slug>"}`.
pub async fn show(
    State(app): State<AppState>,
    Path(slug): Path<String>,
    Query(query): Query<Vec<(String, String)>>,
) -> AppResult<Response> {
    let params = Params::new(query, &["include"])?;
    let detail = Detail { html: true, document: params.include_document()? };
    let db = app.db();
    let public = || post::Entity::find().filter(post::public_at(now()));
    let Some(found) = public().filter(post::Column::Slug.eq(slug.as_str())).one(db).await? else {
        let Some(id) = old_slug(db, "post", &slug).await? else { return Err(AppError::NotFound) };
        let current = public().select_only().column(post::Column::Slug).filter(post::Column::Id.eq(id));
        let current: String = current.into_tuple().one(db).await?.ok_or(AppError::NotFound)?;
        return Ok(Json(RedirectJson { redirect: current }).into_response());
    };
    let base = base_url(&app);
    let mut json = entry::posts(db, &public_links(&base), std::slice::from_ref(&found), detail).await?;
    Ok(Json(json.remove(0)).into_response())
}
