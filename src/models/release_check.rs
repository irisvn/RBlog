//! What the last release check found (`release_checks`, one row: [`ROW`]). Written only by
//! `services::version`; read by Integrations → Version and the dashboard's release widget
//! (`admin::resources::release_update_resource`).

use sea_orm::entity::prelude::*;

/// The id of the one row.
pub const ROW: i64 = 1;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "release_checks")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: i64,
    /// When the last check started (manual or scheduled).
    pub attempted_at: Option<DateTimeUtc>,
    /// When a check last succeeded.
    pub checked_at: Option<DateTimeUtc>,
    /// The newest stable release then (`X.Y.Z`), `None` when none is published.
    pub latest_version: Option<String>,
    /// `services::version::rank` of `latest_version`.
    pub latest_rank: Option<i64>,
    /// Its release notes, plain text (Markdown as written on GitHub), never HTML.
    pub notes: Option<String>,
    /// Its page on GitHub (`https://github.com/…` only).
    pub url: Option<String>,
    pub published_at: Option<DateTimeUtc>,
    /// Why the last attempt failed (`services::version::Failure::code`); the release above is then
    /// from `checked_at`.
    pub error: Option<String>,
    pub error_status: Option<i32>,
    pub error_at: Option<DateTimeUtc>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
