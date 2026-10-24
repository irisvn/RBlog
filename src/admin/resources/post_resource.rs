//! Posts: a rich-text body, a cover, one category and tags, SEO fields, and the publishing
//! workflow. `status`, `published_at` and `author_id` are guarded: the server sets them (the
//! author is the creator; the status changes only through the workflow actions,
//! `services::publishing`). Who sees and changes what is `policies::content`; every write ends
//! in `content_sync::content_changed`.

use std::sync::Arc;

use rustcms::admin::*;
use rustcms::{AppResult, async_trait};
use sea_orm::{ColumnTrait, DatabaseTransaction, QueryFilter, Select};

use super::content::{self, STATUS_OPTIONS};
use crate::models::{post, post_tag};
use crate::policies::content::{me, sees_all};
use crate::policies::content_policy::ContentPolicy;
use crate::services::content::{Content, Kind};
use crate::services::content_sync::{self, Change};

pub struct PostResource;

#[async_trait]
impl Resource for PostResource {
    type Entity = post::Entity;
    const SLUG: &'static str = "posts";

    fn meta() -> ResourceMeta {
        ResourceMeta::new("Post")
            .plural("Posts")
            .title("title")
            .icon("file-text")
            .group("Content")
            .sort(-2)
            .soft_deletes()
            .unique("posts_slug_key", &["slug"], "is already taken")
    }

    /// Readable/writable fields. Only declared columns are ever exposed; `search_text` and
    /// `reading_minutes` are computed, never shown or written by clients.
    fn fields() -> Vec<Field> {
        vec![
            Field::column(post::Column::Title).required().searchable().sortable().max_length(255),
            Field::column(post::Column::Slug).max_length(200),
            Field::column(post::Column::Excerpt).max_length(1_000),
            Field::column(post::Column::Body).rich_text(),
            Field::column(post::Column::CoverMediaId).label("Cover image").media().accept(&["image/*"]),
            Field::column(post::Column::CategoryId).label("Category").belongs_to("categories").filterable(),
            Field::column(post::Column::Featured).filterable(),
            Field::column(post::Column::MetaTitle).label("Meta title").max_length(255),
            Field::column(post::Column::MetaDescription).label("Meta description").max_length(500),
            Field::column(post::Column::CanonicalUrl).label("Canonical URL").max_length(2048),
            Field::column(post::Column::OgMediaId).label("Share image").media().accept(&["image/*"]),
            // Set by the server only.
            Field::column(post::Column::Status).guarded().readable(|_| true).filterable(),
            Field::column(post::Column::PublishedAt).label("Publish date").guarded().readable(|_| true).sortable(),
            Field::column(post::Column::AuthorId)
                .label("Author")
                .guarded()
                .readable(|_| true)
                .belongs_to("post_authors")
                .filterable(),
            Field::column(post::Column::CreatedAt).guarded().readable(|_| true).sortable(),
            Field::column(post::Column::UpdatedAt).guarded().readable(|_| true).sortable(),
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
                SelectInput::make("category_id"),
                BelongsToManySelect::make("tags").label("Tags"),
                Toggle::make("featured").help("Shown first where the public site highlights posts"),
            ]),
            Tab::make("SEO").schema(components![
                TextInput::make("meta_title").help("For search engines. Left empty: the title"),
                Textarea::make("meta_description").help("For search engines. Left empty: the excerpt"),
                TextInput::make("canonical_url").url().help("Only when the post first appeared elsewhere"),
                FileUpload::make("og_media_id").help("Shown when the post is shared. Left empty: the cover"),
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
                TextColumn::make("category_id").hide_below(Breakpoint::Md),
                TextColumn::make("author_id").hide_below(Breakpoint::Md),
                DateColumn::make("published_at").hide_below(Breakpoint::Sm),
            ])
            .filters(vec![
                SelectFilter::make("status").options(STATUS_OPTIONS),
                SelectFilter::make("category_id"),
                SelectFilter::make("author_id"),
                TernaryFilter::make("featured"),
            ])
            // Last updated first. No `updated_at` column: the title needs the room (six columns and
            // the row's actions do not fit beside the sidebar). Under 768 px only the title, the
            // status and the publish date stay; on a phone, the title and the status.
            .default_sort("-updated_at")
    }

    fn relations() -> Vec<Relation> {
        vec![
            Relation::belongs_to_many("tags", "tags", post_tag::Column::PostId, post_tag::Column::TagId).label("Tags"),
            Relation::has_many("revisions", "post_revisions", "post_id").label("Revisions"),
        ]
    }

    fn actions() -> Vec<Action<post::Entity>> {
        let mut actions = content::workflow_actions(Kind::Post);
        actions.push(content::preview_action(Kind::Post));
        actions
    }

    fn policy() -> Arc<dyn Policy> {
        Arc::new(ContentPolicy(Kind::Post))
    }

    /// Without `posts.view_all`, a user sees only the posts they wrote, in every status.
    fn scope(query: Select<post::Entity>, ctx: &Ctx) -> Select<post::Entity> {
        if sees_all(ctx, Kind::Post) { query } else { query.filter(post::Column::AuthorId.eq(me(ctx).unwrap_or(-1))) }
    }

    async fn mutate_before_create(ctx: &Ctx, txn: &DatabaseTransaction, data: Data) -> AppResult<Data> {
        content::before_create::<post::Entity>(ctx, Self::SLUG, txn, data).await
    }

    async fn mutate_before_update(
        ctx: &Ctx,
        txn: &DatabaseTransaction,
        current: &post::Model,
        data: Data,
    ) -> AppResult<Data> {
        content::before_update::<post::Entity>(ctx, Self::SLUG, txn, &current.clone().into(), data).await
    }

    async fn after_save(ctx: &Ctx, txn: &DatabaseTransaction, saved: &post::Model) -> AppResult<()> {
        content::saved(ctx, txn, saved.clone().into()).await
    }

    async fn after_relation_change(
        ctx: &Ctx,
        txn: &DatabaseTransaction,
        record: &post::Model,
        _change: &RelationChange,
    ) -> AppResult<()> {
        let record: Content = record.clone().into();
        content_sync::content_links_changed(txn, Kind::Post, &record, content_sync::actor_id(ctx)).await
    }

    async fn after_trash(ctx: &Ctx, txn: &DatabaseTransaction, record: &post::Model) -> AppResult<()> {
        content_sync::content_changed(txn, Kind::Post, record.id, Change::Trashed, content_sync::actor_id(ctx)).await
    }

    /// Not `after_save`: a restore is its own event.
    async fn after_restore(ctx: &Ctx, txn: &DatabaseTransaction, record: &post::Model) -> AppResult<()> {
        content_sync::content_changed(txn, Kind::Post, record.id, Change::Restored, content_sync::actor_id(ctx)).await
    }

    async fn after_force_delete(_ctx: &Ctx, txn: &DatabaseTransaction, record: &post::Model) -> AppResult<()> {
        content_sync::content_deleted(txn, &record.clone().into()).await
    }
}
