//! A menu, its links resolved to frontend paths.

use std::collections::{BTreeSet, HashMap};

use sea_orm::prelude::DateTimeUtc;
use sea_orm::{ColumnTrait, ConnectionTrait, DbErr, EntityTrait, QueryFilter, QueryOrder, QuerySelect};
use serde::Serialize;

use crate::models::{category, menu, menu_item, page, post, tag};
use crate::services::content_sync::public_path;

/// `{key, name, items}`.
#[derive(Debug, Clone, Serialize)]
pub struct MenuJson {
    pub key: String,
    pub name: String,
    pub items: Vec<MenuItemJson>,
}

/// A link: `href` is a frontend path (`content_sync::public_path`) or the item's own URL.
#[derive(Debug, Clone, Serialize)]
pub struct MenuItemJson {
    pub id: String,
    pub label: String,
    pub href: String,
    pub new_tab: bool,
    /// Items under this one (one level only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub children: Option<Vec<MenuItemJson>>,
}

/// The slugs of the targets of `items` of `kind` that the public site shows at `now`.
async fn slugs<C: ConnectionTrait>(
    db: &C,
    items: &[menu_item::Model],
    kind: &str,
    now: DateTimeUtc,
) -> Result<HashMap<i64, String>, DbErr> {
    let ids: BTreeSet<i64> = items.iter().filter(|i| i.kind == kind).filter_map(|i| i.target_id).collect();
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let rows: Vec<(i64, String)> = match kind {
        "post" => {
            post::Entity::find()
                .select_only()
                .columns([post::Column::Id, post::Column::Slug])
                .filter(post::Column::Id.is_in(ids))
                .filter(post::public_at(now))
                .into_tuple()
                .all(db)
                .await?
        }
        "page" => {
            page::Entity::find()
                .select_only()
                .columns([page::Column::Id, page::Column::Slug])
                .filter(page::Column::Id.is_in(ids))
                .filter(page::public_at(now))
                .into_tuple()
                .all(db)
                .await?
        }
        "category" => {
            category::Entity::find()
                .select_only()
                .columns([category::Column::Id, category::Column::Slug])
                .filter(category::Column::Id.is_in(ids))
                .into_tuple()
                .all(db)
                .await?
        }
        "tag" => {
            tag::Entity::find()
                .select_only()
                .columns([tag::Column::Id, tag::Column::Slug])
                .filter(tag::Column::Id.is_in(ids))
                .into_tuple()
                .all(db)
                .await?
        }
        _ => Vec::new(),
    };
    Ok(rows.into_iter().collect())
}

/// `menu` with its items in order, as a tree of one level. Items whose target is not public at
/// `now` (unpublished, scheduled, trashed or deleted), and the items under them, are left out.
pub async fn menu<C: ConnectionTrait>(db: &C, menu: menu::Model, now: DateTimeUtc) -> Result<MenuJson, DbErr> {
    let items = menu_item::Entity::find()
        .filter(menu_item::Column::MenuId.eq(menu.id))
        .order_by_asc(menu_item::Column::Position)
        .order_by_asc(menu_item::Column::Id)
        .all(db)
        .await?;
    let mut targets: HashMap<&str, HashMap<i64, String>> = HashMap::new();
    for kind in ["post", "page", "category", "tag"] {
        targets.insert(kind, slugs(db, &items, kind, now).await?);
    }
    let href = |item: &menu_item::Model| -> Option<String> {
        if item.kind == "url" {
            return item.url.clone();
        }
        let slug = targets.get(item.kind.as_str())?.get(&item.target_id?)?;
        Some(public_path(&item.kind, slug))
    };
    let mut top: Vec<MenuItemJson> = Vec::new();
    let mut index: HashMap<i64, usize> = HashMap::new();
    for item in items.iter().filter(|i| i.parent_id.is_none()) {
        if let Some(href) = href(item) {
            index.insert(item.id, top.len());
            top.push(MenuItemJson {
                id: item.id.to_string(),
                label: item.label.clone(),
                href,
                new_tab: item.new_tab,
                children: Some(Vec::new()),
            });
        }
    }
    for item in items.iter().filter(|i| i.parent_id.is_some()) {
        // A child of a hidden item (or of a child: one level only) is hidden with it.
        let Some(&at) = item.parent_id.and_then(|parent| index.get(&parent)) else { continue };
        if let Some(href) = href(item) {
            let child = MenuItemJson {
                id: item.id.to_string(),
                label: item.label.clone(),
                href,
                new_tab: item.new_tab,
                children: None,
            };
            top[at].children.get_or_insert_with(Vec::new).push(child);
        }
    }
    Ok(MenuJson { key: menu.key, name: menu.name, items: top })
}
