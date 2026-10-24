//! Categories: one per post, with their own SEO fields. The slug is made from the name when
//! left empty; a typed slug is normalized, and a taken one is refused. Every write keeps the
//! old slug for redirects and records an outbox event (`services::taxonomy`).

use rustcms::admin::*;
use rustcms::{AppResult, async_trait};
use sea_orm::DatabaseTransaction;

use super::{taxonomy_before_create, taxonomy_before_update};
use crate::models::category;
use crate::services::taxonomy;

pub struct CategoryResource;

#[async_trait]
impl Resource for CategoryResource {
    type Entity = category::Entity;
    const SLUG: &'static str = "categories";

    fn meta() -> ResourceMeta {
        ResourceMeta::new("Category").plural("Categories").title("name").icon("folder").group("Content").unique(
            "categories_slug_key",
            &["slug"],
            "is already taken",
        )
    }

    /// Readable/writable fields. Only declared columns are ever exposed.
    fn fields() -> Vec<Field> {
        vec![
            Field::column(category::Column::Name).required().searchable().sortable().max_length(120),
            Field::column(category::Column::Slug).searchable().max_length(160),
            Field::column(category::Column::Description).max_length(10_000),
            Field::column(category::Column::MetaTitle).label("Meta title").max_length(255),
            Field::column(category::Column::MetaDescription).label("Meta description").max_length(500),
            Field::column(category::Column::CreatedAt).guarded().readable(|_| true).sortable(),
        ]
    }

    fn form() -> Form {
        Form::new().columns(2).schema(components![
            TextInput::make("name"),
            TextInput::make("slug").help("Left empty: made from the name"),
            Textarea::make("description").column_span(2),
            TextInput::make("meta_title").help("For search engines. Left empty: the name").column_span(2),
            Textarea::make("meta_description").help("For search engines").column_span(2),
        ])
    }

    fn table() -> Table {
        Table::new()
            .columns(vec![
                TextColumn::make("name"),
                TextColumn::make("slug").hide_below(Breakpoint::Sm),
                DateColumn::make("created_at").hide_below(Breakpoint::Md),
            ])
            .default_sort("name")
    }

    async fn mutate_before_create(ctx: &Ctx, txn: &DatabaseTransaction, data: Data) -> AppResult<Data> {
        taxonomy_before_create::<category::Entity>(ctx, Self::SLUG, txn, data).await
    }

    async fn mutate_before_update(
        ctx: &Ctx,
        txn: &DatabaseTransaction,
        current: &category::Model,
        data: Data,
    ) -> AppResult<Data> {
        let current_values = (current.id, current.name.as_str(), current.slug.as_str());
        taxonomy_before_update::<category::Entity>(ctx, Self::SLUG, txn, current_values, data).await
    }

    async fn after_save(_ctx: &Ctx, txn: &DatabaseTransaction, saved: &category::Model) -> AppResult<()> {
        taxonomy::saved::<category::Entity>(txn, saved.id, &saved.slug, saved.created_at, saved.updated_at).await
    }

    async fn after_delete(_ctx: &Ctx, txn: &DatabaseTransaction, deleted: &category::Model) -> AppResult<()> {
        taxonomy::deleted::<category::Entity>(txn, deleted.id, &deleted.slug).await
    }
}
