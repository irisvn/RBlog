//! Rules of menus and their items, whoever writes them. Messages are English sources: the
//! caller translates them (`lang/vi.toml`).

use rustcms::{AppResult, ValidationErrors};
use sea_orm::{ColumnTrait, ConnectionTrait, EntityTrait, PaginatorTrait, QueryFilter, QuerySelect};

use crate::models::{category, menu, menu_item, page, post, tag};

/// The menus the frontend draws.
pub const KEYS: &[&str] = &["primary", "footer"];

/// What an item links to: a URL, or a post, page, category or tag (`target_id`).
pub const KINDS: &[&str] = &["url", "post", "page", "category", "tag"];

pub const KEY_INVALID: &str = "must be primary or footer";
pub const KIND_INVALID: &str = "must be url, post, page, category or tag";
pub const URL_REQUIRED: &str = "is required for a URL link";
pub const URL_NOT_ALLOWED: &str = "must be empty unless the link is a URL";
pub const URL_INVALID: &str = "must start with https://, http://, mailto: or /";
pub const TARGET_REQUIRED: &str = "is required for this link type";
pub const TARGET_NOT_ALLOWED: &str = "must be empty for a URL link";
pub const TARGET_MISSING: &str = "does not exist";
pub const PARENT_SELF: &str = "cannot be the item itself";
pub const PARENT_OTHER_MENU: &str = "must be an item of the same menu";
pub const PARENT_NESTED: &str = "must be a top-level item";
pub const PARENT_HAS_CHILDREN: &str = "cannot be set on an item that has sub-items";

/// Every message of this module, for the translation check.
pub const MESSAGES: &[&str] = &[
    KEY_INVALID,
    KIND_INVALID,
    URL_REQUIRED,
    URL_NOT_ALLOWED,
    URL_INVALID,
    TARGET_REQUIRED,
    TARGET_NOT_ALLOWED,
    TARGET_MISSING,
    PARENT_SELF,
    PARENT_OTHER_MENU,
    PARENT_NESTED,
    PARENT_HAS_CHILDREN,
];

/// The framework's own message (translated by the panel from its code).
const REQUIRED: &str = "is required";

/// A menu as it will be stored: a `key` the frontend knows, and a name that is not blank (422
/// on the field otherwise). `key` is `None` on an update: it never changes.
pub fn check_menu(key: Option<&str>, name: &str) -> AppResult<()> {
    let mut errors = ValidationErrors::new();
    if key.is_some_and(|key| !KEYS.contains(&key)) {
        errors.add("key", KEY_INVALID);
    }
    if name.trim().is_empty() {
        errors.add("name", REQUIRED);
    }
    errors.into_result()
}

/// A menu item as it will be stored.
#[derive(Clone, Debug, Default)]
pub struct Item<'a> {
    /// `None` for a new item.
    pub id: Option<i64>,
    pub label: &'a str,
    pub menu_id: i64,
    pub parent_id: Option<i64>,
    pub kind: &'a str,
    pub target_id: Option<i64>,
    /// Blank counts as empty.
    pub url: Option<&'a str>,
}

/// Checks `item` (422 with every field error at once):
/// - the label is not blank;
/// - `kind` is known, and `url` / `target_id` match it: a URL link has a valid `url`
///   ([`valid_url`]) and no target; the others have an existing target and no `url`;
/// - the parent is another item of the same menu, at the top level (menus are one level deep),
///   and an item with sub-items gets no parent.
///
/// Run it in the write's transaction, under the menu's lock (`lock_parent`), so that the items
/// it reads cannot change before the write commits.
pub async fn check_item(db: &impl ConnectionTrait, item: &Item<'_>) -> AppResult<()> {
    let mut errors = ValidationErrors::new();
    if item.label.trim().is_empty() {
        errors.add("label", REQUIRED);
    }
    let url = item.url.map(str::trim).filter(|u| !u.is_empty());
    match item.kind {
        "url" => {
            match url {
                None => errors.add("url", URL_REQUIRED),
                Some(url) if !valid_url(url) => errors.add("url", URL_INVALID),
                Some(_) => {}
            }
            if item.target_id.is_some() {
                errors.add("target_id", TARGET_NOT_ALLOWED);
            }
        }
        kind if KINDS.contains(&kind) => {
            if url.is_some() {
                errors.add("url", URL_NOT_ALLOWED);
            }
            match item.target_id {
                None => errors.add("target_id", TARGET_REQUIRED),
                Some(id) if !target_exists(db, kind, id).await? => errors.add("target_id", TARGET_MISSING),
                Some(_) => {}
            }
        }
        _ => errors.add("kind", KIND_INVALID),
    }
    if let Some(parent_id) = item.parent_id {
        if Some(parent_id) == item.id {
            errors.add("parent_id", PARENT_SELF);
        } else {
            match menu_item::Entity::find_by_id(parent_id).one(db).await? {
                Some(parent) if parent.menu_id != item.menu_id => errors.add("parent_id", PARENT_OTHER_MENU),
                Some(parent) if parent.parent_id.is_some() => errors.add("parent_id", PARENT_NESTED),
                Some(_) => {
                    if let Some(id) = item.id
                        && menu_item::Entity::find().filter(menu_item::Column::ParentId.eq(id)).count(db).await? > 0
                    {
                        errors.add("parent_id", PARENT_HAS_CHILDREN);
                    }
                }
                // The relationship check refuses it (`belongs_to`).
                None => errors.add("parent_id", PARENT_OTHER_MENU),
            }
        }
    }
    errors.into_result()
}

/// A link the frontend may follow: `https://…`, `http://…` (with a host), `mailto:…`, or a
/// path of this site (`/…`, not `//host`). No whitespace or control characters anywhere.
pub fn valid_url(url: &str) -> bool {
    if url.is_empty() || url.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return false;
    }
    if let Some(path) = url.strip_prefix('/') {
        return !path.starts_with('/') && !path.starts_with('\\');
    }
    let lower = url.to_ascii_lowercase();
    if let Some(rest) = lower.strip_prefix("https://").or_else(|| lower.strip_prefix("http://")) {
        let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
        return !rest.contains('\\') && valid_authority(authority);
    }
    lower.strip_prefix("mailto:").is_some_and(|rest| !rest.is_empty())
}

/// `host` or `host:port` of a web link: a domain name (letters, digits and inner `-`, labels
/// joined by `.`), or an IPv6 address in brackets; a port of 1 to 65535. No user information.
fn valid_authority(authority: &str) -> bool {
    // The host, and what follows it (`""` or `:port`).
    let (host_ok, rest) = match authority.strip_prefix('[') {
        Some(v6) => match v6.split_once(']') {
            Some((address, rest)) => (address.parse::<std::net::Ipv6Addr>().is_ok(), rest),
            None => return false,
        },
        None => {
            let end = authority.find(':').unwrap_or(authority.len());
            (valid_domain(&authority[..end]), &authority[end..])
        }
    };
    let port_ok = match rest.strip_prefix(':') {
        Some(port) => {
            !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) && port.parse::<u16>().is_ok_and(|p| p > 0)
        }
        None => rest.is_empty(),
    };
    host_ok && port_ok
}

/// A domain name (or IPv4 address): labels of letters (any script), digits and inner `-`,
/// joined by single dots, one trailing dot allowed.
fn valid_domain(host: &str) -> bool {
    let host = host.strip_suffix('.').unwrap_or(host);
    !host.is_empty()
        && host.split('.').all(|label| {
            !label.is_empty()
                && label.chars().count() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label.chars().all(|c| c.is_alphanumeric() || c == '-')
        })
}

async fn target_exists(db: &impl ConnectionTrait, kind: &str, id: i64) -> AppResult<bool> {
    let found = match kind {
        "post" => {
            post::Entity::find_by_id(id).select_only().column(post::Column::Id).into_tuple::<i64>().one(db).await?
        }
        "page" => {
            page::Entity::find_by_id(id).select_only().column(page::Column::Id).into_tuple::<i64>().one(db).await?
        }
        "category" => {
            category::Entity::find_by_id(id)
                .select_only()
                .column(category::Column::Id)
                .into_tuple::<i64>()
                .one(db)
                .await?
        }
        "tag" => tag::Entity::find_by_id(id).select_only().column(tag::Column::Id).into_tuple::<i64>().one(db).await?,
        _ => None,
    };
    Ok(found.is_some())
}

/// The key of menu `id`, read in the write's transaction (for its outbox event).
pub async fn key_of(db: &impl ConnectionTrait, id: i64) -> AppResult<Option<String>> {
    Ok(menu::Entity::find_by_id(id).select_only().column(menu::Column::Key).into_tuple::<String>().one(db).await?)
}

#[cfg(test)]
mod tests {
    use super::valid_url;

    #[test]
    fn urls_are_web_links_mail_or_site_paths() {
        for ok in [
            "https://example.com",
            "http://example.com/a?b=1",
            "HTTPS://Example.com",
            "https://example.com:8443/x#top",
            "https://example.com?q=1",
            "https://ví-dụ.vn/",
            "http://127.0.0.1:8080",
            "https://[2001:db8::1]:443/a",
            "https://[::1]",
            "mailto:a@b.c",
            "/",
            "/blog/x",
        ] {
            assert!(valid_url(ok), "{ok}");
        }
        for bad in [
            "",
            "javascript:alert(1)",
            "ftp://example.com",
            "//evil.example",
            "/\\evil.example",
            "https://",
            "https:///path",
            "mailto:",
            "example.com",
            "/a b",
            "https://example.com/\n",
            "data:text/html,x",
            "https://[",
            "https://[::1",
            "https://[nope]",
            "https://[::1]x",
            "https://example.com:bad",
            "https://example.com:",
            "https://example.com:0",
            "https://example.com:65536",
            "https://user@example.com",
            "https://exa_mple.com",
            "https://-example.com",
            "https://example..com",
            "https://example.com\\@evil",
        ] {
            assert!(!valid_url(bad), "{bad:?}");
        }
    }
}
