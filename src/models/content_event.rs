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
    /// Delivery attempts made (POSTs tried) across every job of the event, at most 8 in a series.
    /// An operator retry of a job that gave up starts a new series at 0.
    pub attempts: i32,
    /// Why the last attempt failed (redacted, at most 500 characters); `None` after a success.
    pub last_error: Option<String>,
    /// The delivery job last dispatched for it (`crate::services::events::enqueue`).
    pub job_id: Option<Uuid>,
    /// When delivery stopped without success (a permanent refusal, or the last retry).
    pub gave_up_at: Option<DateTimeUtc>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
