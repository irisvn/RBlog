//! Admin resources, one per model (`rustcms make:resource <Model>`).

use rustcms::admin::{Ctx, Data};
use rustcms::{AppError, AppResult, ValidationErrors};
use sea_orm::DatabaseTransaction;
use serde_json::Value;

use crate::services::content_sync;
use crate::services::taxonomy::{self, Taxonomy};

pub mod author_profile_resource;
pub mod category_resource;
pub mod menu_item_resource;
pub mod menu_resource;
pub mod tag_resource;
pub mod user_resource;
// rustcms:resource-mods

/// `error` with the messages of its field and form errors translated (`ctx.t_in(slug, …)`):
/// the panel shows hook messages as written.
pub(crate) fn translated(ctx: &Ctx, slug: &str, error: AppError) -> AppError {
    match error {
        AppError::Validation(mut errors) => {
            for message in errors.errors.values_mut().flatten().chain(errors.form_errors.iter_mut()) {
                *message = ctx.t_in(slug, message);
            }
            AppError::Validation(errors)
        }
        other => other,
    }
}

/// The text of `field` in `data`: `None` when absent, `Some(None)` when `null`.
pub(crate) fn text<'a>(data: &'a Data, field: &str) -> Option<Option<&'a str>> {
    data.get(field).map(Value::as_str)
}

/// The 64-bit id of `field` in `data` (a string on the wire): `None` when absent, `Some(None)`
/// when `null`.
pub(crate) fn id(data: &Data, field: &str) -> Option<Option<i64>> {
    data.get(field).map(|v| match v {
        Value::String(s) => s.parse().ok(),
        Value::Number(n) => n.as_i64(),
        _ => None,
    })
}

/// An instant as the wire carries it (RFC 3339, UTC, microseconds).
pub(crate) fn instant(at: sea_orm::prelude::DateTimeUtc) -> Value {
    Value::String(at.format("%Y-%m-%dT%H:%M:%S%.6fZ").to_string())
}

/// `mutate_before_create` of a category or tag: the slug ([`taxonomy::slug_for_create`]) and
/// both timestamps, set to one instant (see `services::taxonomy`).
pub(crate) async fn taxonomy_before_create<E: Taxonomy>(
    ctx: &Ctx,
    resource: &str,
    txn: &DatabaseTransaction,
    mut data: Data,
) -> AppResult<Data> {
    let name = text(&data, "name").flatten().unwrap_or_default().trim().to_owned();
    if name.is_empty() {
        return Err(ValidationErrors::field("name", "is required").into());
    }
    let typed = text(&data, "slug").flatten();
    let slug = taxonomy::slug_for_create::<E>(txn, &name, typed).await.map_err(|e| translated(ctx, resource, e))?;
    let now = instant(content_sync::now());
    data.insert("slug".into(), Value::String(slug));
    data.insert("created_at".into(), now.clone());
    data.insert("updated_at".into(), now);
    Ok(data)
}

/// `mutate_before_update` of category or tag `id` (now `name` and `slug`): the slug
/// ([`taxonomy::slug_for_update`], which keeps a replaced one in `slug_history`) and
/// `updated_at`.
pub(crate) async fn taxonomy_before_update<E: Taxonomy>(
    ctx: &Ctx,
    resource: &str,
    txn: &DatabaseTransaction,
    (id, name, slug): (i64, &str, &str),
    mut data: Data,
) -> AppResult<Data> {
    let name = match text(&data, "name") {
        Some(new) => new.unwrap_or_default().trim().to_owned(),
        None => name.to_owned(),
    };
    if name.is_empty() {
        return Err(ValidationErrors::field("name", "is required").into());
    }
    let now = content_sync::now();
    let typed = text(&data, "slug");
    match taxonomy::slug_for_update::<E>(txn, id, slug, &name, typed, now)
        .await
        .map_err(|e| translated(ctx, resource, e))?
    {
        Some(new) => data.insert("slug".into(), Value::String(new)),
        // Unchanged: a typed form of the same slug is not written again.
        None => data.remove("slug"),
    };
    data.insert("updated_at".into(), instant(now));
    Ok(data)
}
