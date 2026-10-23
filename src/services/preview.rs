//! Preview tokens: a random token shows one post or page, in any status, through
//! `GET /api/v1/preview/{token}` until it expires (one hour) or is revoked. The token is the
//! capability: only its SHA-256 is stored, and only who may view the subject issues one (the
//! admin action checks that before calling [`issue`]).

use std::time::{Duration, SystemTime};

use rustcms::auth::secret::{random_string, sha256_hex};
use sea_orm::prelude::DateTimeUtc;
use sea_orm::{ActiveModelTrait, ColumnTrait, ConnectionTrait, DbErr, EntityTrait, QueryFilter, Set};

use crate::models::{page, post, preview_token};

/// How long a token works.
pub const LIFETIME: Duration = Duration::from_secs(3600);
/// Length of a token: 43 alphanumeric characters, about 256 bits.
pub const TOKEN_LEN: usize = 43;

/// What a preview token shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreviewSubject {
    Post(i64),
    Page(i64),
}

impl PreviewSubject {
    /// The `preview_tokens.subject_type` value.
    pub fn kind(self) -> &'static str {
        match self {
            Self::Post(_) => "post",
            Self::Page(_) => "page",
        }
    }

    /// The post's or page's id.
    pub fn id(self) -> i64 {
        match self {
            Self::Post(id) | Self::Page(id) => id,
        }
    }

    fn parse(kind: &str, id: i64) -> Option<Self> {
        match kind {
            "post" => Some(Self::Post(id)),
            "page" => Some(Self::Page(id)),
            _ => None,
        }
    }
}

/// Issues a token for `subject`, made by user `created_by`, valid for [`LIFETIME`]. Returns the
/// token, which is never stored: show it once.
pub async fn issue<C: ConnectionTrait>(txn: &C, subject: PreviewSubject, created_by: i64) -> Result<String, DbErr> {
    let token = random_string(TOKEN_LEN);
    let now = SystemTime::now();
    preview_token::ActiveModel {
        token_hash: Set(sha256_hex(&token)),
        subject_type: Set(subject.kind().to_owned()),
        subject_id: Set(subject.id()),
        created_by: Set(created_by),
        expires_at: Set(DateTimeUtc::from(now + LIFETIME)),
        revoked_at: Set(None),
        created_at: Set(DateTimeUtc::from(now)),
        ..Default::default()
    }
    .insert(txn)
    .await?;
    Ok(token)
}

/// Revokes the token `token_id` (no-op when it is already revoked or does not exist).
pub async fn revoke<C: ConnectionTrait>(txn: &C, token_id: i64) -> Result<(), DbErr> {
    preview_token::Entity::update_many()
        .col_expr(
            preview_token::Column::RevokedAt,
            sea_orm::sea_query::Expr::value(DateTimeUtc::from(SystemTime::now())),
        )
        .filter(preview_token::Column::Id.eq(token_id))
        .filter(preview_token::Column::RevokedAt.is_null())
        .exec(txn)
        .await?;
    Ok(())
}

/// The subject `token` shows now, while the token is neither expired nor revoked.
pub async fn find<C: ConnectionTrait>(db: &C, token: &str) -> Result<Option<PreviewSubject>, DbErr> {
    // Tokens we issue are alphanumeric of a fixed length: anything else is not one.
    if token.len() != TOKEN_LEN || !token.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Ok(None);
    }
    let now = DateTimeUtc::from(SystemTime::now());
    let found = preview_token::Entity::find()
        .filter(preview_token::Column::TokenHash.eq(sha256_hex(token)))
        .filter(preview_token::Column::RevokedAt.is_null())
        .filter(preview_token::Column::ExpiresAt.gt(now))
        .one(db)
        .await?;
    Ok(found.and_then(|t| PreviewSubject::parse(&t.subject_type, t.subject_id)))
}

/// The post a preview shows, unless it is trashed.
pub async fn post<C: ConnectionTrait>(db: &C, id: i64) -> Result<Option<post::Model>, DbErr> {
    post::Entity::find_by_id(id).filter(post::Column::DeletedAt.is_null()).one(db).await
}

/// The page a preview shows, unless it is trashed.
pub async fn page<C: ConnectionTrait>(db: &C, id: i64) -> Result<Option<page::Model>, DbErr> {
    page::Entity::find_by_id(id).filter(page::Column::DeletedAt.is_null()).one(db).await
}
