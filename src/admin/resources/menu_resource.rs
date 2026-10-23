//! Menus of the frontend, found by `key` (`primary`, `footer`). Their items are listed and
//! created on the menu's page (`MenuItemResource`). Every write records a `menu:{key}` outbox
//! event.

use rustcms::admin::*;
use rustcms::{AppResult, async_trait};
use sea_orm::DatabaseTransaction;

use super::{instant, text, translated};
use crate::models::menu;
use crate::services::content_sync::{self, Change};
use crate::services::menus;

pub struct MenuResource;

#[async_trait]
impl Resource for MenuResource {
    type Entity = menu::Entity;
    const SLUG: &'static str = "menus";

    fn meta() -> ResourceMeta {
        ResourceMeta::new("Menu").plural("Menus").title("name").icon("layers").group("Content").unique(
            "menus_key_key",
            &["key"],
            "is already taken",
        )
    }

    /// The key names the menu for the frontend (`GET /menus/{key}`): it is chosen once.
    fn fields() -> Vec<Field> {
        vec![
            Field::column(menu::Column::Key)
                .label("Location")
                .required()
                .sortable()
                .max_length(32)
                .updatable(|_| false),
            Field::column(menu::Column::Name).required().searchable().sortable().max_length(120),
        ]
    }

    fn form() -> Form {
        Form::new().columns(2).schema(components![
            SelectInput::make("key")
                .options([("primary", "Primary"), ("footer", "Footer")])
                .help("Where the frontend shows the menu. It cannot be changed later."),
            TextInput::make("name"),
        ])
    }

    fn table() -> Table {
        Table::new().columns(vec![TextColumn::make("name"), TextColumn::make("key")]).default_sort("name")
    }

    fn relations() -> Vec<Relation> {
        vec![Relation::has_many("items", "menu_items", "menu_id").label("Items")]
    }

    /// Both timestamps at one instant: `after_save` tells a create by them.
    async fn mutate_before_create(ctx: &Ctx, _txn: &DatabaseTransaction, mut data: Data) -> AppResult<Data> {
        let key = text(&data, "key").flatten().unwrap_or_default();
        let name = text(&data, "name").flatten().unwrap_or_default();
        menus::check_menu(Some(key), name).map_err(|e| translated(ctx, Self::SLUG, e))?;
        let now = instant(content_sync::now());
        data.insert("created_at".into(), now.clone());
        data.insert("updated_at".into(), now);
        Ok(data)
    }

    async fn mutate_before_update(
        ctx: &Ctx,
        _txn: &DatabaseTransaction,
        current: &menu::Model,
        mut data: Data,
    ) -> AppResult<Data> {
        let name = text(&data, "name").map_or(current.name.as_str(), Option::unwrap_or_default);
        menus::check_menu(None, name).map_err(|e| translated(ctx, Self::SLUG, e))?;
        data.insert("updated_at".into(), instant(content_sync::now()));
        Ok(data)
    }

    async fn after_save(_ctx: &Ctx, txn: &DatabaseTransaction, saved: &menu::Model) -> AppResult<()> {
        // Set by the hooks above: equal only when this write created the menu.
        let change = if saved.created_at == saved.updated_at { Change::Created } else { Change::Updated };
        content_sync::menu_changed(txn, change, saved.id, &saved.key).await
    }

    async fn after_delete(_ctx: &Ctx, txn: &DatabaseTransaction, deleted: &menu::Model) -> AppResult<()> {
        content_sync::menu_changed(txn, Change::Deleted, deleted.id, &deleted.key).await
    }
}
