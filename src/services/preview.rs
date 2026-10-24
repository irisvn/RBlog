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

/// A preview link: the public site's preview route with a fresh token.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreviewLink {
    pub url: String,
    pub expires_at: DateTimeUtc,
}

/// A preview link for post or page `key`, made by the user of `ctx`:
///
/// - the record is read as the panel reads it for this user (`Ctx::record`: the resource's
///   admission, scope and `view` policy; 403 or 404 otherwise, a trashed one is 404);
/// - `None` while Settings → General has no public site URL (no token is made);
/// - otherwise a token ([`issue`], audited), and
///   `{frontend_url}{preview_path}?token={token}` (Settings → Integrations' preview path).
pub async fn issue_link(
    app: &rustcms::AppState,
    ctx: &rustcms::admin::Ctx,
    kind: crate::services::content::Kind,
    key: &str,
) -> rustcms::AppResult<Option<PreviewLink>> {
    use crate::services::content::Kind;
    use crate::settings::general_settings::GeneralSettings;
    use crate::settings::integration_settings::IntegrationSettings;

    ctx.record(kind.resource(), key).await?;
    let id: i64 = key.parse().map_err(|_| rustcms::AppError::NotFound)?;
    let general = rustcms::settings::get::<GeneralSettings>(app).await?;
    let Some(frontend) = general.frontend_url() else { return Ok(None) };
    let integrations = rustcms::settings::get::<IntegrationSettings>(app).await?;
    let me: i64 = ctx.principal.id.parse().map_err(|_| rustcms::AppError::Forbidden)?;
    let subject = match kind {
        Kind::Post => PreviewSubject::Post(id),
        Kind::Page => PreviewSubject::Page(id),
    };
    let txn = rustcms::db::begin_write(app.db()).await?;
    let token = issue(&txn, subject, me).await?;
    rustcms::audit::record(
        &txn,
        &ctx.audit_context,
        rustcms::audit::AuditEntry::new("preview_link.created", kind.resource()).key(id),
    )
    .await?;
    txn.commit().await?;
    Ok(Some(PreviewLink {
        url: preview_url(frontend, &integrations.preview_path, &token),
        expires_at: DateTimeUtc::from(SystemTime::now() + LIFETIME),
    }))
}

/// `{frontend}{path}` with `token=` added to its query: after a query the path already has
/// (`/preview?mode=draft` → `…&token=`), before its fragment (`/preview#top` → `?token=…#top`).
/// The token is alphanumeric: it needs no escaping.
pub fn preview_url(frontend: &str, path: &str, token: &str) -> String {
    // The site's address names a site: a query or fragment it carries would swallow the route.
    let frontend = frontend.split(['?', '#']).next().unwrap_or_default().trim_end_matches('/');
    let (path, fragment) = match path.split_once('#') {
        Some((path, fragment)) => (path, Some(fragment)),
        None => (path, None),
    };
    let joiner = match path.split_once('?') {
        Some((_, "")) => "",
        Some((_, query)) if query.ends_with('&') => "",
        Some(_) => "&",
        None => "?",
    };
    let mut url = format!("{frontend}{path}{joiner}token={token}");
    if let Some(fragment) = fragment {
        url.push('#');
        url.push_str(fragment);
    }
    url
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

#[cfg(test)]
mod tests {
    use super::preview_url;

    #[test]
    fn the_token_joins_the_query_before_the_fragment() {
        let site = "https://blog.example.com";
        assert_eq!(preview_url(site, "/preview", "T"), "https://blog.example.com/preview?token=T");
        assert_eq!(
            preview_url(site, "/preview?mode=draft", "T"),
            "https://blog.example.com/preview?mode=draft&token=T"
        );
        assert_eq!(preview_url(site, "/preview?", "T"), "https://blog.example.com/preview?token=T");
        assert_eq!(preview_url(site, "/preview?a=1&", "T"), "https://blog.example.com/preview?a=1&token=T");
        assert_eq!(preview_url(site, "/preview#top", "T"), "https://blog.example.com/preview?token=T#top");
        assert_eq!(preview_url(site, "/p?x=1#top", "T"), "https://blog.example.com/p?x=1&token=T#top");
        assert_eq!(
            preview_url("https://blog.example.com/#top", "/preview", "T"),
            "https://blog.example.com/preview?token=T"
        );
        assert_eq!(
            preview_url("https://blog.example.com/?a=1", "/preview", "T"),
            "https://blog.example.com/preview?token=T"
        );
    }
}
