//! A newer RBlog release, for the dashboard's notice (`admin::dashboard`, widget `rblog_update`):
//! the `release_checks` row while the release it found is newer than this build. Read only, out
//! of the navigation, written by `services::version` alone.
//!
//! RustCMS 0.10.3 draws dashboard widgets over resources only (no dashboard slot or banner), so
//! the notice is a `recent` widget over this resource. Only holders of `system.version` see it
//! (`policies::release_update_policy`); development and source builds never do (`scope()`).

use std::sync::Arc;

use rustcms::admin::*;
use rustcms::async_trait;
use sea_orm::{ColumnTrait, QueryFilter, Select};

use crate::models::release_check;
use crate::policies::release_update_policy::ReleaseUpdatePolicy;
use crate::services::version::{self, Channel};

pub struct ReleaseUpdateResource;

#[async_trait]
impl Resource for ReleaseUpdateResource {
    type Entity = release_check::Entity;
    const SLUG: &'static str = "release_updates";

    fn meta() -> ResourceMeta {
        ResourceMeta::new("RBlog release")
            .plural("RBlog releases")
            .title("latest_version")
            .icon("info")
            .navigation(false)
            .audit(false)
    }

    /// Everything is set by the server: nothing is writable.
    fn fields() -> Vec<Field> {
        vec![
            Field::column(release_check::Column::LatestVersion).label("Version").guarded().readable(|_| true),
            Field::column(release_check::Column::PublishedAt).label("Published").guarded().readable(|_| true),
            Field::column(release_check::Column::Notes).label("Release notes").guarded().readable(|_| true),
            Field::column(release_check::Column::Url).label("Release page").guarded().readable(|_| true),
        ]
    }

    fn form() -> Form {
        Form::new().schema(components![
            TextInput::make("latest_version"),
            DateTimePicker::make("published_at"),
            Textarea::make("notes").help("The Version page says how to update."),
            TextInput::make("url"),
        ])
    }

    fn table() -> Table {
        Table::new().columns(vec![TextColumn::make("latest_version"), DateColumn::make("published_at")])
    }

    fn policy() -> Arc<dyn Policy> {
        Arc::new(ReleaseUpdatePolicy)
    }

    /// The row only while its release is newer than this build, and only on a release build:
    /// development and source builds are never compared.
    fn scope(query: Select<release_check::Entity>, _ctx: &Ctx) -> Select<release_check::Entity> {
        let build = version::current();
        match (build.channel, version::rank(&build.version)) {
            (Channel::Release, Some(running)) => query.filter(release_check::Column::LatestRank.gt(running)),
            // No row has a negative id: nothing.
            _ => query.filter(release_check::Column::Id.lt(0)),
        }
    }
}
