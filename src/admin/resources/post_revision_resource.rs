//! Revisions of a post, listed on the post's page (the `revisions` relation of `PostResource`):
//! read only, written by the server (`services::revisions`). Their visibility is the post's: the
//! scope keeps revisions of posts the user sees (not trashed; their own without
//! `posts.view_all`), and the policy checks the post again on every path
//! (`policies::revision_policy`). "Restore this version" puts the content back
//! (`revisions::restore`).

use std::sync::Arc;

use rustcms::admin::*;
use rustcms::async_trait;
use sea_orm::sea_query::Query;
use sea_orm::{ColumnTrait, QueryFilter, Select};

use crate::models::{post, post_revision};
use crate::policies::content::{me, sees_all};
use crate::policies::revision_policy::RevisionPolicy;
use crate::services::content::Kind;
use crate::services::revisions;

pub struct PostRevisionResource;

#[async_trait]
impl Resource for PostRevisionResource {
    type Entity = post_revision::Entity;
    const SLUG: &'static str = "post_revisions";

    fn meta() -> ResourceMeta {
        ResourceMeta::new("Post revision")
            .plural("Post revisions")
            .title("title")
            .icon("history")
            .group("Content")
            .navigation(false)
            // Restoring writes the post: lock it first, as every writer of the post does.
            .lock_parent("post_id", "posts")
    }

    /// Everything is set by the server: nothing is writable.
    fn fields() -> Vec<Field> {
        vec![
            Field::column(post_revision::Column::PostId)
                .label("Post")
                .guarded()
                .readable(|_| true)
                .belongs_to("posts")
                .filterable(),
            Field::column(post_revision::Column::Title).guarded().readable(|_| true).searchable(),
            Field::column(post_revision::Column::Excerpt).guarded().readable(|_| true),
            Field::column(post_revision::Column::Body).guarded().readable(|_| true).rich_text(),
            Field::column(post_revision::Column::AuthorId)
                .label("Saved by")
                .guarded()
                .readable(|_| true)
                .belongs_to("post_authors"),
            Field::column(post_revision::Column::CreatedAt).label("Saved at").guarded().readable(|_| true).sortable(),
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
            .columns(vec![
                DateColumn::make("created_at"),
                TextColumn::make("title"),
                TextColumn::make("author_id").hide_below(Breakpoint::Md),
            ])
            .default_sort("-created_at")
    }

    fn actions() -> Vec<Action<post_revision::Entity>> {
        vec![
            Action::<post_revision::Entity>::new("restore_version", |ctx, txn, revisions_, _data| {
                Box::pin(async move {
                    for revision in revisions_ {
                        revisions::restore(ctx, txn, Kind::Post, revision.post_id, revision.id)
                            .await
                            .map_err(|e| super::translated(ctx, "post_revisions", e))?;
                    }
                    Ok(())
                })
            })
            .label("Restore this version")
            .icon("rotate-ccw")
            // The one labelled action of each row of the Revisions table.
            .primary()
            .requires_confirmation(
                "Restore this version? The post's content is replaced by it; its status does not change.",
            ),
        ]
    }

    fn policy() -> Arc<dyn Policy> {
        Arc::new(RevisionPolicy(Kind::Post))
    }

    /// Revisions of the posts the user sees: not trashed, and their own without
    /// `posts.view_all`.
    fn scope(query: Select<post_revision::Entity>, ctx: &Ctx) -> Select<post_revision::Entity> {
        let mut visible = Query::select()
            .column(post::Column::Id)
            .from(post::Entity)
            .and_where(post::Column::DeletedAt.is_null())
            .to_owned();
        if !sees_all(ctx, Kind::Post) {
            visible.and_where(post::Column::AuthorId.eq(me(ctx).unwrap_or(-1)));
        }
        query.filter(post_revision::Column::PostId.in_subquery(visible))
    }
}
