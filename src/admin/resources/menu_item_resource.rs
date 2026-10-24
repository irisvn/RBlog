//! The links of a menu, listed and created on the menu's page (the `items` relation of
//! `MenuResource`), so they have no navigation entry. Writes lock the menu first, then check
//! the item (`services::menus::check_item`) and record a `menu:{key}` outbox event.

use rustcms::admin::*;
use rustcms::{AppResult, async_trait};
use sea_orm::DatabaseTransaction;
use serde_json::Value;

use super::{id, instant, text, translated};
use crate::models::menu_item;
use crate::services::content_sync::{self, Change};
use crate::services::menus::{self, Item};

pub struct MenuItemResource;

/// The link types (`services::menus::KINDS`) as the form and the filter offer them.
const KIND_OPTIONS: [(&str, &str); 5] =
    [("url", "URL"), ("post", "Post"), ("page", "Page"), ("category", "Category"), ("tag", "Tag")];

#[async_trait]
impl Resource for MenuItemResource {
    type Entity = menu_item::Entity;
    const SLUG: &'static str = "menu_items";

    fn meta() -> ResourceMeta {
        ResourceMeta::new("Menu item")
            .plural("Menu items")
            .title("label")
            .icon("link")
            .group("Content")
            .navigation(false)
            // The item rules read the menu's other items: writes under one menu take turns.
            .lock_parent("menu_id", "menus")
    }

    /// Readable/writable fields. Only declared columns are ever exposed.
    fn fields() -> Vec<Field> {
        vec![
            // Set on create (by the menu's page); an item never moves to another menu.
            Field::column(menu_item::Column::MenuId)
                .label("Menu")
                .required()
                .belongs_to("menus")
                .filterable()
                .updatable(|_| false),
            Field::column(menu_item::Column::ParentId).label("Parent item").belongs_to("menu_items").filterable(),
            Field::column(menu_item::Column::Label).required().searchable().sortable().max_length(120),
            Field::column(menu_item::Column::Kind).label("Link type").required().filterable().max_length(16),
            Field::column(menu_item::Column::TargetId).label("Target ID"),
            Field::column(menu_item::Column::Url).label("URL").max_length(2048),
            Field::column(menu_item::Column::NewTab).label("Open in a new tab"),
            Field::column(menu_item::Column::Position).sortable(),
        ]
    }

    fn form() -> Form {
        Form::new().columns(2).schema(components![
            // Hidden when the item is created on its menu's page (the relation sets it).
            SelectInput::make("menu_id"),
            TextInput::make("label"),
            SelectInput::make("parent_id").help("Left empty: a top-level item. Menus are one level deep."),
            SelectInput::make("kind").options(KIND_OPTIONS),
            NumberInput::make("target_id").help("The ID of the linked post, page, category or tag"),
            TextInput::make("url").help("https://…, http://…, mailto:… or a path such as /about"),
            NumberInput::make("position").help("Smaller numbers come first"),
            Toggle::make("new_tab"),
        ])
    }

    fn table() -> Table {
        Table::new()
            .columns(vec![
                TextColumn::make("label"),
                // The option's label, not the stored value (docs/ui-guidelines.md §4).
                TextColumn::make("kind").cell("rblog/option-label").props(option_labels(&KIND_OPTIONS)),
                TextColumn::make("parent_id").hide_below(Breakpoint::Md),
                TextColumn::make("position").hide_below(Breakpoint::Sm),
            ])
            .filters(vec![SelectFilter::make("kind").options(KIND_OPTIONS), SelectFilter::make("parent_id")])
            .default_sort("position")
    }

    async fn mutate_before_create(ctx: &Ctx, txn: &DatabaseTransaction, mut data: Data) -> AppResult<Data> {
        clear_blank_url(&mut data);
        let item = Item {
            id: None,
            label: text(&data, "label").flatten().unwrap_or_default(),
            menu_id: id(&data, "menu_id").flatten().unwrap_or_default(),
            parent_id: id(&data, "parent_id").flatten(),
            kind: text(&data, "kind").flatten().unwrap_or_default(),
            target_id: id(&data, "target_id").flatten(),
            url: text(&data, "url").flatten(),
        };
        menus::check_item(txn, &item).await.map_err(|e| translated(ctx, Self::SLUG, e))?;
        let now = instant(content_sync::now());
        data.insert("created_at".into(), now.clone());
        data.insert("updated_at".into(), now);
        Ok(data)
    }

    async fn mutate_before_update(
        ctx: &Ctx,
        txn: &DatabaseTransaction,
        current: &menu_item::Model,
        mut data: Data,
    ) -> AppResult<Data> {
        clear_blank_url(&mut data);
        // The item as it will be stored: what is sent, over what is stored.
        let item = Item {
            id: Some(current.id),
            label: text(&data, "label").map_or(current.label.as_str(), Option::unwrap_or_default),
            menu_id: current.menu_id,
            parent_id: id(&data, "parent_id").unwrap_or(current.parent_id),
            kind: text(&data, "kind").map_or(current.kind.as_str(), Option::unwrap_or_default),
            target_id: id(&data, "target_id").unwrap_or(current.target_id),
            url: text(&data, "url").unwrap_or(current.url.as_deref()),
        };
        menus::check_item(txn, &item).await.map_err(|e| translated(ctx, Self::SLUG, e))?;
        data.insert("updated_at".into(), instant(content_sync::now()));
        Ok(data)
    }

    async fn after_save(_ctx: &Ctx, txn: &DatabaseTransaction, saved: &menu_item::Model) -> AppResult<()> {
        menu_event(txn, saved.menu_id).await
    }

    async fn after_delete(_ctx: &Ctx, txn: &DatabaseTransaction, deleted: &menu_item::Model) -> AppResult<()> {
        menu_event(txn, deleted.menu_id).await
    }
}

/// The props of the `rblog/option-label` cell: each value's label (translated by the cell).
fn option_labels(options: &[(&str, &str)]) -> Value {
    Value::Object(options.iter().map(|(value, label)| ((*value).to_owned(), Value::from(*label))).collect())
}

/// A sent `url` is stored trimmed, and a blank one as no URL.
fn clear_blank_url(data: &mut Data) {
    if let Some(url) = text(data, "url").flatten().map(str::trim) {
        let url = if url.is_empty() { Value::Null } else { Value::String(url.to_owned()) };
        data.insert("url".into(), url);
    }
}

/// An item change is an update of its menu.
async fn menu_event(txn: &DatabaseTransaction, menu_id: i64) -> AppResult<()> {
    match menus::key_of(txn, menu_id).await? {
        Some(key) => content_sync::menu_changed(txn, Change::Updated, menu_id, &key).await,
        // Deleted with its menu: the menu's own event covers it.
        None => Ok(()),
    }
}
