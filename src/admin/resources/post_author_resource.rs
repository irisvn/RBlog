//! The names of the users who write posts and pages: the target of their `author_id`
//! (`belongs_to("post_authors")`), so lists and forms show "Lan" and lists filter by author. Read
//! only, out of the navigation.
//!
//! Not the `users` lookup resource: that one needs `users.view_any` / `users.view`, the Users
//! page's permissions, and shows emails. Every writer of a post needs its author to be a visible
//! record of the target (the reference check of a create), and contributors must not see the
//! Users page. This one shows the name only, with its own permissions
//! (`post_authors.view_any`, `post_authors.view`). Who appears: with `posts.view_all`, every user
//! who wrote a post or a page or saved a revision, and oneself; otherwise only oneself.

use std::sync::Arc;

use rustcms::admin::*;
use rustcms::async_trait;
use rustcms::models::user;
use sea_orm::sea_query::Query;
use sea_orm::{ColumnTrait, Condition, QueryFilter, Select};

use crate::models::{page, page_revision, post, post_revision};
use crate::policies::content::{me, sees_all};
use crate::policies::post_author_policy::PostAuthorPolicy;
use crate::services::content::Kind;

pub struct PostAuthorResource;

#[async_trait]
impl Resource for PostAuthorResource {
    type Entity = user::Entity;
    const SLUG: &'static str = "post_authors";

    fn meta() -> ResourceMeta {
        ResourceMeta::new("Author").plural("Authors").title("name").icon("user").navigation(false).audit(false)
    }

    /// The name only: never the email or anything else of `users`.
    fn fields() -> Vec<Field> {
        vec![Field::column(user::Column::Name).guarded().readable(|_| true).searchable().sortable()]
    }

    fn table() -> Table {
        Table::new().columns(vec![TextColumn::make("name")]).default_sort("name")
    }

    fn policy() -> Arc<dyn Policy> {
        Arc::new(PostAuthorPolicy)
    }

    fn scope(query: Select<user::Entity>, ctx: &Ctx) -> Select<user::Entity> {
        let me = me(ctx).unwrap_or(-1);
        if !sees_all(ctx, Kind::Post) {
            return query.filter(user::Column::Id.eq(me));
        }
        let posts = Query::select().column(post::Column::AuthorId).from(post::Entity).to_owned();
        let pages = Query::select().column(page::Column::AuthorId).from(page::Entity).to_owned();
        // Who saved a revision ("Saved by"), even without a post of their own.
        let post_savers = Query::select()
            .column(post_revision::Column::AuthorId)
            .from(post_revision::Entity)
            .and_where(post_revision::Column::AuthorId.is_not_null())
            .to_owned();
        let page_savers = Query::select()
            .column(page_revision::Column::AuthorId)
            .from(page_revision::Entity)
            .and_where(page_revision::Column::AuthorId.is_not_null())
            .to_owned();
        query.filter(
            Condition::any()
                .add(user::Column::Id.eq(me))
                .add(user::Column::Id.in_subquery(posts))
                .add(user::Column::Id.in_subquery(pages))
                .add(user::Column::Id.in_subquery(post_savers))
                .add(user::Column::Id.in_subquery(page_savers)),
        )
    }
}
