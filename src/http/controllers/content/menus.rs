//! `GET /api/v1/content/menus/{key}`: a menu with links to public content only.

use rustcms::prelude::*;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

use super::now;
use crate::http::resources::content::menu::{self, MenuJson};
use crate::models::menu as menus;

/// The menu `key` (`primary`, `footer`, …).
pub async fn show(State(app): State<AppState>, Path(key): Path<String>) -> AppResult<Json<MenuJson>> {
    let db = app.db();
    let found =
        menus::Entity::find().filter(menus::Column::Key.eq(key.as_str())).one(db).await?.ok_or(AppError::NotFound)?;
    Ok(Json(menu::menu(db, found, now()).await?))
}
