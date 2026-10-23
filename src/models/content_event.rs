//! `content_events`: the transactional outbox. A content change writes its event in its own
//! transaction; delivery (webhooks) marks it `delivered_at`.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "content_events")]
pub struct Model {
    /// Also the receiver's deduplication key.
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    /// What happened (`post.published`, …).
    pub kind: String,
    /// `post`, `page`, `category`, `tag`, `menu`, `settings`, …
    pub subject_type: String,
    pub subject_id: Option<i64>,
    /// Cache tags, old and new paths.
    pub payload: Json,
    pub created_at: DateTimeUtc,
    /// `None` while the event waits for delivery.
    pub delivered_at: Option<DateTimeUtc>,
    pub attempts: i32,
    pub last_error: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
