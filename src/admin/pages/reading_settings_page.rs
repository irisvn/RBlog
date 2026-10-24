//! The admin page of [`ReadingSettings`] (`/admin/settings/reading`): which fields the form
//! shows and how they are checked.

use rustcms::admin::{Ctx, SettingField, SettingsMeta, SettingsPage};
use rustcms::{AppResult, AppState, ValidationErrors, async_trait};
use sea_orm::{ColumnTrait, DatabaseTransaction, EntityTrait, PaginatorTrait, QueryFilter};

use crate::models::page;
use crate::services::content_sync;
use crate::settings::reading_settings::{HOMEPAGE_LATEST_POSTS, HOMEPAGE_PAGE, ReadingSettings};
use crate::settings::rules::{checked, filled, is_slug};

#[async_trait]
impl SettingsPage for ReadingSettings {
    fn meta() -> SettingsMeta {
        SettingsMeta::new("Reading").icon("home")
    }

    fn fields() -> Vec<SettingField> {
        vec![
            SettingField::select(
                "homepage_shows",
                [(HOMEPAGE_LATEST_POSTS, "Latest posts"), (HOMEPAGE_PAGE, "A page")],
            )
            .label("Homepage shows"),
            SettingField::text("homepage_page_slug")
                .label("Page slug")
                .nullable()
                .max_length(255)
                .help("The page the homepage shows, for example about"),
        ]
    }

    async fn validate(ctx: &Ctx, settings: &Self) -> Result<(), ValidationErrors> {
        let mut errors = ValidationErrors::new();
        match filled(&settings.homepage_page_slug) {
            None if settings.homepage_shows == HOMEPAGE_PAGE => {
                errors.add("homepage_page_slug", ctx.t("is required when the homepage shows a page"));
            }
            Some(slug) if !is_slug(slug) => {
                errors.add("homepage_page_slug", ctx.t("may only contain lowercase letters, digits and hyphens"));
            }
            // The page may still be a draft (it is published later), but it must exist and not be
            // in the trash.
            Some(slug) => {
                let found = page::Entity::find()
                    .filter(page::Column::Slug.eq(slug))
                    .filter(page::Column::DeletedAt.is_null())
                    .count(&ctx.conn())
                    .await;
                match found {
                    Ok(0) => errors.add("homepage_page_slug", ctx.t("is not the slug of a page")),
                    Ok(_) => {}
                    // `validate` can only answer with validation errors: never save unchecked.
                    Err(_) => errors.add_form(ctx.t("the page could not be checked, try again")),
                }
            }
            None => {}
        }
        checked(errors)
    }

    /// The public site learns of the change (`settings.updated`, tag `settings`).
    async fn after_save(
        _state: &AppState,
        _ctx: &Ctx,
        txn: &DatabaseTransaction,
        _before: &Self,
        _after: &Self,
    ) -> AppResult<()> {
        content_sync::settings_changed(txn).await
    }
}
