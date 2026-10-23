//! `preview_tokens`: a token that shows one unpublished post or page until it expires or is
//! revoked. Only its SHA-256 is stored.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "preview_tokens")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    /// SHA-256 (hex) of the token.
    #[sea_orm(unique)]
    pub token_hash: String,
    /// `post` or `page`.
    pub subject_type: String,
    pub subject_id: i64,
    /// The user who made it (the token goes with them).
    pub created_by: i64,
    pub expires_at: DateTimeUtc,
    pub revoked_at: Option<DateTimeUtc>,
    pub created_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
