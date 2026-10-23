//! `GET /api/v1/content/sitemap?type=post|page|category|tag&after=<cursor>`: the public paths, in
//! batches of [`BATCH`], for the frontend's sitemap.

use rustcms::prelude::*;
use sea_orm::prelude::DateTimeUtc;
use sea_orm::sea_query::{Expr, ExprTrait, Query as SqlQuery};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect};

use super::{Params, now};
use crate::http::resources::content::instant;
use crate::http::resources::content::sitemap::{SitemapEntry, SitemapJson};
use crate::models::{category, page, post, post_tag, tag};
use crate::services::content_sync::public_path;

/// Entries per answer.
pub const BATCH: u64 = 1000;

/// Published posts or pages, or the categories or tags with a published post, by id, from the
/// cursor `after` on (keyset pagination: a batch never skips or repeats a row while others
/// change). `next` is the cursor of the following batch, null after the last one.
pub async fn index(
    State(app): State<AppState>,
    Query(query): Query<Vec<(String, String)>>,
) -> AppResult<Json<SitemapJson>> {
    let params = Params::new(query, &["type", "after"])?;
    let kind: &'static str = match params.get("type") {
        None => return Err(ValidationErrors::field("type", "is required").into()),
        Some("post") => "post",
        Some("page") => "page",
        Some("category") => "category",
        Some("tag") => "tag",
        Some(_) => return Err(ValidationErrors::field("type", "is invalid").into()),
    };
    // The cursor is the last id of the previous batch; clients treat it as opaque.
    let after = match params.get("after") {
        None => 0,
        Some(cursor) => match cursor.parse::<i64>() {
            Ok(id) if id >= 0 => id,
            _ => return Err(ValidationErrors::field("after", "is invalid").into()),
        },
    };
    let db = app.db();
    let now = now();
    let limit = BATCH + 1;
    let mut rows: Vec<(i64, String, DateTimeUtc)> = match kind {
        "post" => {
            post::Entity::find()
                .select_only()
                .columns([post::Column::Id, post::Column::Slug, post::Column::UpdatedAt])
                .filter(post::public_at(now))
                .filter(post::Column::Id.gt(after))
                .order_by_asc(post::Column::Id)
                .limit(limit)
                .into_tuple()
                .all(db)
                .await?
        }
        "page" => {
            page::Entity::find()
                .select_only()
                .columns([page::Column::Id, page::Column::Slug, page::Column::UpdatedAt])
                .filter(page::public_at(now))
                .filter(page::Column::Id.gt(after))
                .order_by_asc(page::Column::Id)
                .limit(limit)
                .into_tuple()
                .all(db)
                .await?
        }
        "category" => {
            let used = SqlQuery::select()
                .column(post::Column::CategoryId)
                .from(post::Entity)
                .cond_where(post::public_at(now))
                .and_where(post::Column::CategoryId.is_not_null())
                .to_owned();
            category::Entity::find()
                .select_only()
                .columns([category::Column::Id, category::Column::Slug, category::Column::UpdatedAt])
                .filter(category::Column::Id.in_subquery(used))
                .filter(category::Column::Id.gt(after))
                .order_by_asc(category::Column::Id)
                .limit(limit)
                .into_tuple()
                .all(db)
                .await?
        }
        _ => {
            let used = SqlQuery::select()
                .expr(Expr::val(1))
                .from(post_tag::Entity)
                .inner_join(
                    post::Entity,
                    Expr::col((post::Entity, post::Column::Id)).equals((post_tag::Entity, post_tag::Column::PostId)),
                )
                .and_where(
                    Expr::col((post_tag::Entity, post_tag::Column::TagId)).equals((tag::Entity, tag::Column::Id)),
                )
                .cond_where(post::public_at(now))
                .to_owned();
            tag::Entity::find()
                .select_only()
                .columns([tag::Column::Id, tag::Column::Slug, tag::Column::UpdatedAt])
                .filter(Expr::exists(used))
                .filter(tag::Column::Id.gt(after))
                .order_by_asc(tag::Column::Id)
                .limit(limit)
                .into_tuple()
                .all(db)
                .await?
        }
    };
    let next = if rows.len() as u64 > BATCH {
        rows.truncate(BATCH as usize);
        rows.last().map(|(id, _, _)| id.to_string())
    } else {
        None
    };
    let data = rows
        .into_iter()
        .map(|(_, slug, lastmod)| SitemapEntry { kind, path: public_path(kind, &slug), lastmod: instant(lastmod) })
        .collect();
    Ok(Json(SitemapJson { data, next }))
}
