//! Content API keys: API tokens limited to [`PERMISSION`], of the system user
//! [`FRONTEND_EMAIL`], whose only role is the API-guard role [`ROLE`]. A key therefore reads
//! published content and nothing else, whatever other roles exist.

use rustcms::AppState;
use rustcms::audit::{AuditContext, AuditEntry};
use rustcms::auth::entities::role;
use rustcms::auth::secret::random_string;
use rustcms::auth::tokens::{self, IssuedToken};
use rustcms::auth::{API_GUARD, Actor, AuthState, NewUser, RbacService};
use rustcms::models::user;
use rustcms::{AppError, AppResult};
use sea_orm::prelude::DateTimeUtc;
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};

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
    let frontend = match frontend_user(db).await? {
        Some(found) => found,
        None => {
            let password_hash = auth.passwords.hash(random_string(48)).await?;
            let new = NewUser { name: FRONTEND_NAME, email: FRONTEND_EMAIL, password_hash, super_admin_guard: None };
            rbac.create_user(Actor::System, new).await?
        }
    };
    rbac.assign_role(Actor::System, frontend.id, role_id).await?;
    Ok(frontend)
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
