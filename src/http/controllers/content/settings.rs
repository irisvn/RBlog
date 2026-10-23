//! `GET /api/v1/content/settings`: the public site settings (`services::site`).

use rustcms::prelude::*;

use crate::services::site::{PublicSettings, public_settings};

/// Name, description, language, frontend URL, SEO defaults, social links and homepage.
pub async fn show(State(app): State<AppState>) -> AppResult<Json<PublicSettings>> {
    Ok(Json(public_settings(&app).await.map_err(AppError::internal)?))
}
