//! Users as the target of `belongs_to("users")` fields (an author profile's user). RustCMS 0.8.2
//! has no users resource of its own (its Users page is a system page), and a `belongs_to` needs
//! one. This one only lists and shows the name and email (`UserLookupPolicy`), without a
//! navigation entry: users are still created and changed on the Users page.

use std::sync::Arc;

use rustcms::admin::*;
use rustcms::models::user;
use sea_orm::{ColumnTrait, QueryFilter, Select};

use crate::policies::user_lookup_policy::UserLookupPolicy;
use crate::services::content_api::FRONTEND_EMAIL;

pub struct UserResource;

impl Resource for UserResource {
    type Entity = user::Entity;
    const SLUG: &'static str = "users";

    fn meta() -> ResourceMeta {
        ResourceMeta::new("User").plural("Users").title("name").icon("users").navigation(false).audit(false)
    }

    /// Read only: nothing here is ever written.
    fn fields() -> Vec<Field> {
        vec![
            Field::column(user::Column::Name).guarded().readable(|_| true).searchable().sortable(),
            Field::column(user::Column::Email).guarded().readable(|_| true).searchable(),
        ]
    }

    fn table() -> Table {
        Table::new().columns(vec![TextColumn::make("name"), TextColumn::make("email")]).default_sort("name")
    }

    fn policy() -> Arc<dyn Policy> {
        Arc::new(UserLookupPolicy)
    }

    /// The Content API's system user writes nothing: it is never an author.
    fn scope(query: Select<user::Entity>, _ctx: &Ctx) -> Select<user::Entity> {
        query.filter(user::Column::Email.ne(FRONTEND_EMAIL))
    }
}
