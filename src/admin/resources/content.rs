//! What `PostResource` and `PageResource` share: the hooks of their writes and their workflow
//! actions. Every write ends in `content_sync::content_changed` (search text, public media,
//! revision, outbox event); every change of status goes through `services::publishing`.

use rustcms::admin::*;
use rustcms::{AppResult, ValidationErrors};
use sea_orm::prelude::DateTimeUtc;
use sea_orm::{DatabaseTransaction, EntityTrait};
use serde_json::Value;

use super::{instant, text, translated};
use crate::models::post::PostStatus;
use crate::services::content::{Content, Kind};
use crate::services::content_sync::{self, Change};
use crate::services::publishing::{self, Transition};
use crate::services::taxonomy::{self, Taxonomy};

/// The status words of the workflow (docs/ui-guidelines.md §4), for filters and the form.
pub const STATUS_OPTIONS: [(&str, &str); 4] =
    [("draft", "Draft"), ("pending", "Pending review"), ("scheduled", "Scheduled"), ("published", "Published")];

/// The message of a canonical URL that is not a web address (translated here).
pub const CANONICAL_NOT_WEB: &str = "must be an http or https address";

/// `mutate_before_create`: the title, the slug (made from the title when left empty), the author
/// (the creator), a draft with no publish date, and both timestamps at one instant (see
/// `content_sync::content_changed`).
pub async fn before_create<E: Taxonomy>(
    ctx: &Ctx,
    resource: &str,
    txn: &DatabaseTransaction,
    mut data: Data,
) -> AppResult<Data> {
    let title = text(&data, "title").flatten().unwrap_or_default().trim().to_owned();
    if title.is_empty() {
        return Err(translated(ctx, resource, ValidationErrors::field("title", "is required").into()));
    }
    let typed = text(&data, "slug").flatten();
    let slug = taxonomy::slug_for_create::<E>(txn, &title, typed).await.map_err(|e| translated(ctx, resource, e))?;
    check_canonical(ctx, resource, &mut data)?;
    let me = content_sync::actor_id(ctx).ok_or(rustcms::AppError::Forbidden)?;
    let at = content_sync::now();
    content_sync::mark_form_write(txn, kind_of::<E>(), None, at).await?;
    let now = instant(at);
    data.insert("title".into(), Value::String(title));
    data.insert("slug".into(), Value::String(slug));
    data.insert("author_id".into(), Value::String(me.to_string()));
    data.insert("status".into(), Value::String(PostStatus::Draft.as_str().into()));
    data.insert("published_at".into(), Value::Null);
    data.insert("created_at".into(), now.clone());
    data.insert("updated_at".into(), now);
    Ok(data)
}

/// `mutate_before_update`: the title, the slug (a replaced one kept in `slug_history`), and
/// `updated_at`. Status, publish date and author never change here.
pub async fn before_update<E: Taxonomy>(
    ctx: &Ctx,
    resource: &str,
    txn: &DatabaseTransaction,
    current: &Content,
    mut data: Data,
) -> AppResult<Data> {
    let title = match text(&data, "title") {
        Some(new) => new.unwrap_or_default().trim().to_owned(),
        None => current.title.clone(),
    };
    if title.is_empty() {
        return Err(translated(ctx, resource, ValidationErrors::field("title", "is required").into()));
    }
    if data.contains_key("title") {
        data.insert("title".into(), Value::String(title.clone()));
    }
    check_canonical(ctx, resource, &mut data)?;
    let now = content_sync::now();
    let typed = text(&data, "slug");
    match taxonomy::slug_for_update::<E>(txn, current.id, &current.slug, &title, typed, now)
        .await
        .map_err(|e| translated(ctx, resource, e))?
    {
        Some(new) => data.insert("slug".into(), Value::String(new)),
        None => data.remove("slug"),
    };
    data.insert("updated_at".into(), instant(now));
    content_sync::mark_form_write(txn, kind_of::<E>(), Some(current.id), now).await?;
    Ok(data)
}

/// Posts or pages.
fn kind_of<E: Taxonomy>() -> Kind {
    if E::SUBJECT == "page" { Kind::Page } else { Kind::Post }
}

/// A canonical URL is an `http(s)://` address; a blank one is none.
fn check_canonical(ctx: &Ctx, resource: &str, data: &mut Data) -> AppResult<()> {
    if let Some(url) = text(data, "canonical_url").flatten().map(str::trim) {
        if url.is_empty() {
            data.insert("canonical_url".into(), Value::Null);
        } else if !is_web_address(url) {
            return Err(translated(ctx, resource, ValidationErrors::field("canonical_url", CANONICAL_NOT_WEB).into()));
        } else {
            data.insert("canonical_url".into(), Value::String(url.to_owned()));
        }
    }
    Ok(())
}

/// An `http(s)://` address with a host: dot-separated labels of letters, digits and `-`, and an
/// optional `:port` (no user name, no whitespace anywhere).
pub fn is_web_address(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://").or_else(|| url.strip_prefix("http://")) else {
        return false;
    };
    if url.contains(char::is_whitespace) {
        return false;
    }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) => (host, Some(port)),
        None => (authority, None),
    };
    let host_ok = !host.is_empty()
        && host.split('.').all(|label| !label.is_empty() && label.chars().all(|c| c.is_alphanumeric() || c == '-'));
    let port_ok = port.is_none_or(|p| !p.is_empty() && p.len() <= 5 && p.bytes().all(|b| b.is_ascii_digit()));
    host_ok && port_ok
}

/// `after_save`: a create (equal timestamps, set by [`before_create`]) or an update.
pub async fn saved(ctx: &Ctx, txn: &DatabaseTransaction, content: Content) -> AppResult<()> {
    content_sync::clear_form_write(txn).await?;
    let change = if content.created_at == content.updated_at { Change::Created } else { Change::Updated };
    content_sync::content_changed(txn, content.kind, content.id, change, content_sync::actor_id(ctx)).await
}

/// The workflow actions of a post or page resource: Submit for review, Publish (also on a
/// selection), Schedule…, Unpublish. Each is offered on the records its ability allows (the
/// policy checks the permission, the author and the status) and runs `services::publishing`.
pub fn workflow_actions<E>(kind: Kind) -> Vec<Action<E>>
where
    E: EntityTrait,
    E::Model: Into<Content>,
{
    let resource = kind.resource();
    let run = move |name: &'static str, transition: fn(&Data) -> Result<Transition, ValidationErrors>| {
        Action::<E>::new(name, move |ctx, txn, records, data| {
            Box::pin(async move {
                let transition = transition(&data).map_err(|e| translated(ctx, resource, e.into()))?;
                for record in records {
                    let content: Content = record.into();
                    publishing::transition(ctx, txn, kind, content.id, transition)
                        .await
                        .map_err(|e| translated(ctx, resource, e))?;
                }
                Ok(())
            })
        })
    };
    vec![
        run("submit", |_| Ok(Transition::Submit)).label("Submit for review").icon("send"),
        run("publish", |_| Ok(Transition::Publish))
            .label("Publish")
            .icon("globe")
            .bulk()
            .requires_confirmation("Publish now? It goes live on the public site."),
        run("schedule", schedule_at)
            .ability("publish")
            .label("Schedule…")
            .icon("calendar-clock")
            .form(vec![ActionField::datetime("at").label("Publish at").help("Times are in UTC").required()]),
        run("unpublish", |_| Ok(Transition::Unpublish))
            .label("Unpublish")
            .icon("eye-off")
            .requires_confirmation("Unpublish? It is removed from the public site and goes back to drafts."),
    ]
}

/// The `at` of the Schedule dialog (canonical UTC RFC 3339).
fn schedule_at(data: &Data) -> Result<Transition, ValidationErrors> {
    data.get("at")
        .and_then(Value::as_str)
        .and_then(chrono_parse)
        .map(Transition::Schedule)
        .ok_or_else(|| ValidationErrors::field("at", "is required"))
}

fn chrono_parse(at: &str) -> Option<DateTimeUtc> {
    sea_orm::prelude::DateTimeWithTimeZone::parse_from_rfc3339(at).ok().map(DateTimeUtc::from)
}

#[cfg(test)]
mod tests {
    use super::is_web_address;

    #[test]
    fn a_web_address_has_a_host() {
        for good in [
            "https://example.com",
            "http://blog.example.com/a?b=1#c",
            "https://xn--b-0ga.vn:8443/x",
            "https://đà-nẵng.vn",
        ] {
            assert!(is_web_address(good), "{good}");
        }
        for bad in [
            "https://",
            "https:///path",
            "http://a b.com",
            "javascript:alert(1)",
            "https://user@host.com",
            "https://a..b",
            "https://a.com:",
            "ftp://a.com",
        ] {
            assert!(!is_web_address(bad), "{bad}");
        }
    }
}
