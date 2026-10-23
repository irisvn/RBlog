//! The journal of `media:move-disk` (`media_moves`): a media file copied to another disk.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "media_moves")]
pub struct Model {
    /// The `media` row whose file was copied.
    #[sea_orm(primary_key, auto_increment = false)]
    pub media_id: Uuid,
    /// The disk it was copied from (kept there until `--cleanup`).
    pub from_disk: String,
    /// The object key (the same on both disks).
    pub key: String,
    /// The disk it was copied to.
    pub to_disk: String,
    pub copied_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
