//! Content API keys: API tokens limited to [`PERMISSION`], of the system user
//! [`FRONTEND_EMAIL`], whose only role is the API-guard role [`ROLE`]. A key therefore reads
//! published content and nothing else, whatever other roles exist.

use rustcms::AppState;
use rustcms::audit::{AuditContext, AuditEntry};
use rustcms::auth::entities::{personal_access_token as token, role};
use rustcms::auth::secret::random_string;
use rustcms::auth::tokens::{self, IssuedToken};
use rustcms::auth::{API_GUARD, Actor, AuthState, NewUser, RbacService};
use rustcms::models::user;
use rustcms::{AppError, AppResult};
use sea_orm::prelude::DateTimeUtc;
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder, QuerySelect, Select};

/// What a Content API key may do (every `/api/v1/content` route requires it).
pub const PERMISSION: &str = "content.read";
/// The API-guard role holding only [`PERMISSION`].
pub const ROLE: &str = "content-reader";
/// The system user Content API keys belong to. It has no admin role and a random password
/// nobody knows, so it cannot sign in to the admin.
pub const FRONTEND_EMAIL: &str = "frontend@rblog.local";
const FRONTEND_NAME: &str = "Frontend (Content API)";

/// Creates the [`ROLE`] role and the [`FRONTEND_EMAIL`] user holding it, through
/// `RbacService`, unless they exist. Runs again safely. Returns the user.
pub async fn ensure_frontend_user(state: &AppState) -> anyhow::Result<user::Model> {
    let auth = state.service::<AuthState>().ok_or_else(|| anyhow::anyhow!("AuthProvider not registered"))?;
    let db = state.db();
    let rbac = RbacService::new(db, &auth.resolver);
    // `content.read` is declared by the API routes; make sure the table has it.
    rbac.sync_permissions(&auth.permissions.all()).await?;
    let role_id = match find_role(db).await? {
        Some(found) => found.id,
        None => rbac.create_role(Actor::System, API_GUARD, ROLE, &[PERMISSION.to_owned()]).await?.id,
    };
    match frontend_user(db).await? {
        // Made before with its role; assigning it again repairs a user that lost it.
        Some(found) => {
            rbac.assign_role(Actor::System, found.id, role_id).await?;
            Ok(found)
        }
        // The user and its role in one transaction: never a frontend user without the role, whose
        // keys would all be refused.
        None => {
            let password_hash = auth.passwords.hash(random_string(48)).await?;
            let new = NewUser { name: FRONTEND_NAME, email: FRONTEND_EMAIL, password_hash, super_admin_guard: None };
            Ok(rbac.create_user_with_roles(Actor::System, new, API_GUARD, &[role_id]).await?)
        }
    }
}

/// The [`FRONTEND_EMAIL`] user, made by [`ensure_frontend_user`] when it is missing (the first key
/// made on a database that was not seeded). Two first keys made at once both try to make it: the
/// one whose role or user clashes with the other's runs it again, which then finds them.
pub async fn frontend_user_or_create(state: &AppState) -> AppResult<user::Model> {
    if let Some(found) = frontend_user(state.db()).await? {
        return Ok(found);
    }
    let mut failure = None;
    for _ in 0..3 {
        match ensure_frontend_user(state).await {
            Ok(made) => return Ok(made),
            Err(error) => failure = Some(error),
        }
    }
    Err(AppError::internal(failure.map_or_else(|| "the frontend user could not be made".into(), |e| e.to_string())))
}

/// The API-guard role [`ROLE`].
async fn find_role(db: &DatabaseConnection) -> AppResult<Option<role::Model>> {
    Ok(role::Entity::find()
        .filter(role::Column::Name.eq(ROLE))
        .filter(role::Column::GuardName.eq(API_GUARD))
        .one(db)
        .await?)
}

/// The [`FRONTEND_EMAIL`] user.
pub async fn frontend_user(db: &DatabaseConnection) -> AppResult<Option<user::Model>> {
    Ok(user::Entity::find().filter(user::Column::Email.eq(FRONTEND_EMAIL)).one(db).await?)
}

/// Issues a Content API key named `name` for the frontend user (see [`ensure_frontend_user`]),
/// limited to exactly `["content.read"]`, until `expires_at` (`None`: until revoked). Recorded in
/// the audit log as `token.created` (never the secret). `plain_text` is shown once.
pub async fn issue_content_key(
    db: &DatabaseConnection,
    name: &str,
    expires_at: Option<DateTimeUtc>,
    audit: &AuditContext,
) -> AppResult<IssuedToken> {
    let frontend = frontend_user(db)
        .await?
        .ok_or_else(|| AppError::internal("the frontend user is missing: run `rblog db:seed --class content_api`"))?;
    let txn = rustcms::db::begin_write(db).await?;
    let issued = tokens::issue(&txn, frontend.id, name, &[PERMISSION.to_owned()], expires_at).await?;
    let entry = AuditEntry::new("token.created", "user").key(frontend.id).meta(serde_json::json!({
        "name": issued.token.name,
        "abilities": [PERMISSION],
        "expires_at": issued.token.expires_at,
        "token_id": issued.token.id.to_string(),
    }));
    rustcms::audit::record(&txn, audit, entry).await?;
    txn.commit().await?;
    Ok(issued)
}

/// Live (not revoked) Content API keys, newest first: what the Integrations page lists. Expired
/// keys stay listed until revoked. Never the secret: only its hash is stored.
pub fn content_keys(frontend_id: i64) -> Select<token::Entity> {
    token::Entity::find()
        .filter(token::Column::UserId.eq(frontend_id))
        .filter(token::Column::RevokedAt.is_null())
        .order_by_desc(token::Column::CreatedAt)
        .order_by_desc(token::Column::Id)
}

/// Revokes the Content API key `token_id`, recorded in the audit log as `token.revoked` in the
/// same transaction. A key that is not a live key of the frontend user (another user's token,
/// one already revoked, or none) is [`AppError::NotFound`].
pub async fn revoke_content_key(db: &DatabaseConnection, token_id: i64, audit: &AuditContext) -> AppResult<()> {
    let frontend = frontend_user(db).await?.ok_or(AppError::NotFound)?;
    let txn = rustcms::db::begin_write(db).await?;
    let key = token::Entity::find_by_id(token_id)
        .filter(token::Column::UserId.eq(frontend.id))
        .filter(token::Column::RevokedAt.is_null())
        .lock_exclusive()
        .one(&txn)
        .await?
        .ok_or(AppError::NotFound)?;
    tokens::revoke(&txn, key.id).await?;
    let entry = AuditEntry::new("token.revoked", "user").key(frontend.id).meta(serde_json::json!({
        "name": key.name,
        "token_id": key.id.to_string(),
    }));
    rustcms::audit::record(&txn, audit, entry).await?;
    txn.commit().await?;
    Ok(())
}
