//! Integrations → Version (`/admin/p/version`): what RBlog runs and whether a newer release is out.
//! Routes and their access are in `crate::routes::admin`; the rules live in
//! `crate::services::version`.

use rustcms::prelude::*;
use serde_json::Value;

use crate::services::version::{self, TooSoon, Trigger};

/// `GET …/system/version`: what runs, the last check and the newest release.
pub async fn show(State(app): State<AppState>) -> AppResult<Json<Value>> {
    Ok(Json(version::status(&app).await?))
}

/// `POST …/system/version/check`: checks GitHub now, then answers as `show`. 429 (with
/// `Retry-After`) within a minute of the last check, whoever started it.
pub async fn check(State(app): State<AppState>) -> AppResult<Json<Value>> {
    if let Err(TooSoon(seconds)) = version::check(&app, Trigger::Manual).await? {
        return Err(AppError::TooManyRequests { retry_after: seconds });
    }
    Ok(Json(version::status(&app).await?))
}
