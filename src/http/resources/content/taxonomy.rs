//! Categories and tags, with their number of published posts.

use std::collections::HashMap;

use sea_orm::prelude::DateTimeUtc;
use sea_orm::{ColumnTrait, ConnectionTrait, DbErr, EntityTrait, JoinType, QueryFilter, QuerySelect, RelationTrait};
use serde::Serialize;

use crate::models::{category, post, post_tag, tag};

/// A category.
#[derive(Debug, Clone, Serialize)]
pub struct CategoryJson {
    pub id: String,
    pub slug: String,
    pub name: String,
    pub description: Option<String>,
    pub meta_title: Option<String>,
    pub meta_description: Option<String>,
    /// Its published posts.
    pub post_count: u32,
}

/// A tag.
#[derive(Debug, Clone, Serialize)]
pub struct TagJson {
    pub id: String,
    pub slug: String,
    pub name: String,
    pub description: Option<String>,
    /// Its published posts.
    pub post_count: u32,
}

/// `rows` with their counts of posts public at `now`.
pub async fn categories<C: ConnectionTrait>(
    db: &C,
    rows: Vec<category::Model>,
    now: DateTimeUtc,
) -> Result<Vec<CategoryJson>, DbErr> {
    let counts: HashMap<i64, i64> = if rows.is_empty() {
        HashMap::new()
    } else {
        post::Entity::find()
            .select_only()
            .column(post::Column::CategoryId)
            .column_as(post::Column::Id.count(), "n")
            .filter(post::public_at(now))
            .filter(post::Column::CategoryId.is_in(rows.iter().map(|c| c.id)))
            .group_by(post::Column::CategoryId)
            .into_tuple::<(i64, i64)>()
            .all(db)
            .await?
            .into_iter()
            .collect()
    };
    Ok(rows
        .into_iter()
        .map(|c| CategoryJson {
            post_count: count(&counts, c.id),
            id: c.id.to_string(),
            slug: c.slug,
            name: c.name,
            description: c.description,
            meta_title: c.meta_title,
            meta_description: c.meta_description,
        })
        .collect())
}

/// `rows` with their counts of posts public at `now`.
pub async fn tags<C: ConnectionTrait>(db: &C, rows: Vec<tag::Model>, now: DateTimeUtc) -> Result<Vec<TagJson>, DbErr> {
    let counts: HashMap<i64, i64> = if rows.is_empty() {
        HashMap::new()
    } else {
        // One `post_tag` row per post and tag (its primary key): no post counts twice.
        post_tag::Entity::find()
            .select_only()
            .column(post_tag::Column::TagId)
            .column_as(post_tag::Column::PostId.count(), "n")
            .join(JoinType::InnerJoin, post_tag::Relation::Post.def())
            .filter(post::public_at(now))
            .filter(post_tag::Column::TagId.is_in(rows.iter().map(|t| t.id)))
            .group_by(post_tag::Column::TagId)
            .into_tuple::<(i64, i64)>()
            .all(db)
            .await?
            .into_iter()
            .collect()
    };
    Ok(rows
        .into_iter()
        .map(|t| TagJson {
            post_count: count(&counts, t.id),
            id: t.id.to_string(),
            slug: t.slug,
            name: t.name,
            description: t.description,
        })
        .collect())
}

/// The count of `id` (0 when absent), as a JSON number: counts stay far below 2^32.
fn count(counts: &HashMap<i64, i64>, id: i64) -> u32 {
    counts.get(&id).map_or(0, |n| u32::try_from(*n).unwrap_or(u32::MAX))
}
