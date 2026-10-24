//! Pages (About, Contact, …): posts without category, tags or `featured`, with the same
//! rich-text body, SEO fields, publishing workflow and trash. `status`, `published_at` and
//! `author_id` are guarded, set by the server. Pages are for editors (`pages.*`); the rules are
//! `policies::content`, and every write ends in `content_sync::content_changed`.

use std::sync::Arc;

use rustcms::admin::*;
use rustcms::{AppResult, async_trait};
use sea_orm::DatabaseTransaction;

use super::content::{self, STATUS_OPTIONS};
use crate::models::page;
use crate::policies::content_policy::ContentPolicy;
use crate::services::content::Kind;
use crate::services::content_sync::{self, Change};

pub struct PageResource;

#[async_trait]
impl Resource for PageResource {
    type Entity = page::Entity;
    const SLUG: &'static str = "pages";

    fn meta() -> ResourceMeta {
        ResourceMeta::new("Page")
            .plural("Pages")
            .title("title")
            .icon("file")
            .group("Content")
            .sort(-1)
            .soft_deletes()
            .unique("pages_slug_key", &["slug"], "is already taken")
    }

    /// Readable/writable fields. Only declared columns are ever exposed; `search_text` and
    /// `reading_minutes` are computed, never shown or written by clients.
    fn fields() -> Vec<Field> {
        vec![
            Field::column(page::Column::Title).required().searchable().sortable().max_length(255),
            Field::column(page::Column::Slug).max_length(200),
            Field::column(page::Column::Excerpt).max_length(1_000),
            Field::column(page::Column::Body).rich_text(),
            Field::column(page::Column::CoverMediaId).label("Cover image").media().accept(&["image/*"]),
            Field::column(page::Column::MetaTitle).label("Meta title").max_length(255),
            Field::column(page::Column::MetaDescription).label("Meta description").max_length(500),
            Field::column(page::Column::CanonicalUrl).label("Canonical URL").max_length(2048),
            Field::column(page::Column::OgMediaId).label("Share image").media().accept(&["image/*"]),
            // Set by the server only.
            Field::column(page::Column::Status).guarded().readable(|_| true).filterable(),
            Field::column(page::Column::PublishedAt).label("Publish date").guarded().readable(|_| true).sortable(),
            Field::column(page::Column::AuthorId)
                .label("Author")
                .guarded()
                .readable(|_| true)
                .belongs_to("post_authors")
                .filterable(),
            Field::column(page::Column::CreatedAt).guarded().readable(|_| true).sortable(),
            Field::column(page::Column::UpdatedAt).guarded().readable(|_| true).sortable(),
        ]
    }

    fn form() -> Form {
        Form::new().schema(components![Tabs::make().schema(components![
            Tab::make("Content").schema(components![
                TextInput::make("title"),
                TextInput::make("slug").help("Left empty: made from the title"),
                Textarea::make("excerpt").help("A short summary for lists and search results"),
                RichEditor::make("body"),
                FileUpload::make("cover_media_id"),
            ]),
            Tab::make("SEO").schema(components![
                TextInput::make("meta_title").help("For search engines. Left empty: the title"),
                Textarea::make("meta_description").help("For search engines. Left empty: the excerpt"),
                TextInput::make("canonical_url").url().help("Only when the page first appeared elsewhere"),
                FileUpload::make("og_media_id").help("Shown when the page is shared. Left empty: the cover"),
            ]),
            Tab::make("Publishing").schema(components![
                SelectInput::make("status")
                    .options(STATUS_OPTIONS)
                    .help("Changed with Submit for review, Publish, Schedule and Unpublish"),
                DateTimePicker::make("published_at").help("Times are in UTC (Vietnam is UTC+7)"),
                SelectInput::make("author_id"),
            ]),
        ])])
    }

    fn table() -> Table {
        Table::new()
            .columns(vec![
                TextColumn::make("title"),
                TextColumn::make("status").cell("rblog/content-status"),
                TextColumn::make("author_id"),
                DateColumn::make("published_at"),
                DateColumn::make("updated_at"),
            ])
            .filters(vec![SelectFilter::make("status").options(STATUS_OPTIONS), SelectFilter::make("author_id")])
            .default_sort("-updated_at")
    }

    fn relations() -> Vec<Relation> {
        vec![Relation::has_many("revisions", "page_revisions", "page_id").label("Revisions")]
    }

    fn actions() -> Vec<Action<page::Entity>> {
        content::workflow_actions(Kind::Page)
    }

    fn policy() -> Arc<dyn Policy> {
        Arc::new(ContentPolicy(Kind::Page))
    }

    async fn mutate_before_create(ctx: &Ctx, txn: &DatabaseTransaction, data: Data) -> AppResult<Data> {
        content::before_create::<page::Entity>(ctx, Self::SLUG, txn, data).await
    }

    async fn mutate_before_update(
        ctx: &Ctx,
        txn: &DatabaseTransaction,
        current: &page::Model,
        data: Data,
    ) -> AppResult<Data> {
        content::before_update::<page::Entity>(ctx, Self::SLUG, txn, &current.clone().into(), data).await
    }

    async fn after_save(ctx: &Ctx, txn: &DatabaseTransaction, saved: &page::Model) -> AppResult<()> {
        content::saved(ctx, txn, saved.clone().into()).await
    }

    async fn after_trash(ctx: &Ctx, txn: &DatabaseTransaction, record: &page::Model) -> AppResult<()> {
        content_sync::content_changed(txn, Kind::Page, record.id, Change::Trashed, content_sync::actor_id(ctx)).await
    }

    /// Not `after_save`: a restore is its own event.
    async fn after_restore(ctx: &Ctx, txn: &DatabaseTransaction, record: &page::Model) -> AppResult<()> {
        content_sync::content_changed(txn, Kind::Page, record.id, Change::Restored, content_sync::actor_id(ctx)).await
    }

    async fn after_force_delete(_ctx: &Ctx, txn: &DatabaseTransaction, record: &page::Model) -> AppResult<()> {
        content_sync::content_deleted(txn, &record.clone().into()).await
    }
}
