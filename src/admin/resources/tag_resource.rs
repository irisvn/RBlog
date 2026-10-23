//! Tags: many per post. The slug is made from the name when left empty; a typed slug is
//! normalized, and a taken one is refused. Every write keeps the old slug for redirects and
//! records an outbox event (`services::taxonomy`).

use rustcms::admin::*;
use rustcms::{AppResult, async_trait};
use sea_orm::DatabaseTransaction;

use super::{taxonomy_before_create, taxonomy_before_update};
use crate::models::tag;
use crate::services::taxonomy;

pub struct TagResource;

#[async_trait]
impl Resource for TagResource {
    type Entity = tag::Entity;
    const SLUG: &'static str = "tags";

    fn meta() -> ResourceMeta {
        ResourceMeta::new("Tag").plural("Tags").title("name").icon("tag").group("Content").unique(
            "tags_slug_key",
            &["slug"],
            "is already taken",
        )
    }

    /// Readable/writable fields. Only declared columns are ever exposed.
    fn fields() -> Vec<Field> {
        vec![
            Field::column(tag::Column::Name).required().searchable().sortable().max_length(120),
            Field::column(tag::Column::Slug).searchable().max_length(160),
            Field::column(tag::Column::Description).max_length(10_000),
            Field::column(tag::Column::CreatedAt).guarded().readable(|_| true).sortable(),
        ]
    }

    fn form() -> Form {
        Form::new().columns(2).schema(components![
            TextInput::make("name"),
            TextInput::make("slug").help("Left empty: made from the name"),
            Textarea::make("description").column_span(2),
        ])
    }

    fn table() -> Table {
        Table::new()
            .columns(vec![TextColumn::make("name"), TextColumn::make("slug"), DateColumn::make("created_at")])
            .default_sort("name")
    }

    async fn mutate_before_create(ctx: &Ctx, txn: &DatabaseTransaction, data: Data) -> AppResult<Data> {
        taxonomy_before_create::<tag::Entity>(ctx, Self::SLUG, txn, data).await
    }

    async fn mutate_before_update(
        ctx: &Ctx,
        txn: &DatabaseTransaction,
        current: &tag::Model,
        data: Data,
    ) -> AppResult<Data> {
        let current_values = (current.id, current.name.as_str(), current.slug.as_str());
        taxonomy_before_update::<tag::Entity>(ctx, Self::SLUG, txn, current_values, data).await
    }

    async fn after_save(_ctx: &Ctx, txn: &DatabaseTransaction, saved: &tag::Model) -> AppResult<()> {
        taxonomy::saved::<tag::Entity>(txn, saved.id, &saved.slug, saved.created_at, saved.updated_at).await
    }

    async fn after_delete(_ctx: &Ctx, txn: &DatabaseTransaction, deleted: &tag::Model) -> AppResult<()> {
        taxonomy::deleted::<tag::Entity>(txn, deleted.id, &deleted.slug).await
    }
}
