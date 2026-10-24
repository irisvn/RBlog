//! Revisions of a page, listed on the page's page (the `revisions` relation of `PageResource`):
//! read only, written by the server (`services::revisions`). The scope keeps revisions of pages
//! that are not trashed, and the policy checks the page again on every path
//! (`policies::revision_policy`). "Restore this version" puts the content back
//! (`revisions::restore`).

use std::sync::Arc;

use rustcms::admin::*;
use rustcms::async_trait;
use sea_orm::sea_query::Query;
use sea_orm::{ColumnTrait, QueryFilter, Select};

use crate::models::{page, page_revision};
use crate::policies::revision_policy::RevisionPolicy;
use crate::services::content::Kind;
use crate::services::revisions;

pub struct PageRevisionResource;

#[async_trait]
impl Resource for PageRevisionResource {
    type Entity = page_revision::Entity;
    const SLUG: &'static str = "page_revisions";

    fn meta() -> ResourceMeta {
        ResourceMeta::new("Page revision")
            .plural("Page revisions")
            .title("title")
            .icon("history")
            .group("Content")
            .navigation(false)
            // Restoring writes the page: lock it first, as every writer of the page does.
            .lock_parent("page_id", "pages")
    }

    /// Everything is set by the server: nothing is writable.
    fn fields() -> Vec<Field> {
        vec![
            Field::column(page_revision::Column::PageId)
                .label("Page")
                .guarded()
                .readable(|_| true)
                .belongs_to("pages")
                .filterable(),
            Field::column(page_revision::Column::Title).guarded().readable(|_| true).searchable(),
            Field::column(page_revision::Column::Excerpt).guarded().readable(|_| true),
            Field::column(page_revision::Column::Body).guarded().readable(|_| true).rich_text(),
            Field::column(page_revision::Column::AuthorId)
                .label("Saved by")
                .guarded()
                .readable(|_| true)
                .belongs_to("post_authors"),
            Field::column(page_revision::Column::CreatedAt).label("Saved at").guarded().readable(|_| true).sortable(),
        ]
    }

    fn form() -> Form {
        Form::new().schema(components![
            TextInput::make("title"),
            Textarea::make("excerpt"),
            RichEditor::make("body"),
            SelectInput::make("author_id"),
            DateTimePicker::make("created_at"),
        ])
    }

    fn table() -> Table {
        Table::new()
            .columns(vec![DateColumn::make("created_at"), TextColumn::make("title"), TextColumn::make("author_id")])
            .default_sort("-created_at")
    }

    fn actions() -> Vec<Action<page_revision::Entity>> {
        vec![
            Action::<page_revision::Entity>::new("restore_version", |ctx, txn, revisions_, _data| {
                Box::pin(async move {
                    for revision in revisions_ {
                        revisions::restore(ctx, txn, Kind::Page, revision.page_id, revision.id)
                            .await
                            .map_err(|e| super::translated(ctx, "page_revisions", e))?;
                    }
                    Ok(())
                })
            })
            .label("Restore this version")
            .icon("rotate-ccw")
            .requires_confirmation(
                "Restore this version? The page's content is replaced by it; its status does not change.",
            ),
        ]
    }

    fn policy() -> Arc<dyn Policy> {
        Arc::new(RevisionPolicy(Kind::Page))
    }

    /// Revisions of pages that are not trashed.
    fn scope(query: Select<page_revision::Entity>, _ctx: &Ctx) -> Select<page_revision::Entity> {
        let visible = Query::select()
            .column(page::Column::Id)
            .from(page::Entity)
            .and_where(page::Column::DeletedAt.is_null())
            .to_owned();
        query.filter(page_revision::Column::PageId.in_subquery(visible))
    }
}
