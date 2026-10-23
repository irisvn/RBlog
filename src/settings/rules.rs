//! Shape checks shared by the settings pages: web addresses, paths and slugs.
//!
//! RustCMS 0.8.2 stores a settings value as it was sent (`SettingsPage::validate` cannot change
//! it), so a blank optional text counts as unset, and readers normalize (see [`filled`]).

use rustcms::ValidationErrors;

/// `errors` as the result of `SettingsPage::validate`.
pub fn checked(errors: ValidationErrors) -> Result<(), ValidationErrors> {
    if errors.is_empty() { Ok(()) } else { Err(errors) }
}

/// The text of an optional setting, or `None` when it is unset or blank.
pub fn filled(value: &Option<String>) -> Option<&str> {
    value.as_deref().map(str::trim).filter(|v| !v.is_empty())
}

/// Whether `value` is an absolute `https://` address with a host (and an optional port). With
/// `local_http`, `http://` is accepted for `localhost`, `127.0.0.1` and `[::1]` only.
///
/// User info (`user@`), white space and control characters are refused.
pub fn is_web_url(value: &str, local_http: bool) -> bool {
    if value.chars().any(|c| c.is_whitespace() || c.is_control()) || value.contains('\\') {
        return false;
    }
    let lower = value.to_ascii_lowercase();
    let (secure, rest) = if lower.starts_with("https://") {
        (true, &value["https://".len()..])
    } else if lower.starts_with("http://") {
        (false, &value["http://".len()..])
    } else {
        return false;
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    if authority.contains('@') {
        return false;
    }
    let Some(host) = host_of(authority) else { return false };
    secure || (local_http && is_loopback(host))
}

/// The host of `host[:port]`, if both are well formed.
fn host_of(authority: &str) -> Option<&str> {
    let (host, port) = if authority.starts_with('[') {
        // An IPv6 literal: `[::1]` or `[::1]:8080`.
        let end = authority.find(']')?;
        let inner = &authority[1..end];
        inner.parse::<std::net::Ipv6Addr>().ok()?;
        let port = match &authority[end + 1..] {
            "" => None,
            after => Some(after.strip_prefix(':')?),
        };
        (&authority[..=end], port)
    } else {
        let (host, port) = match authority.split_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (authority, None),
        };
        let labels_ok = host.len() <= 253
            && host.split('.').all(|label| {
                !label.is_empty()
                    && label.len() <= 63
                    && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
                    && !label.starts_with('-')
                    && !label.ends_with('-')
            });
        // A name ending in digits is an IPv4 address to browsers (`999.1.1.1` is none).
        let numeric = host.rsplit('.').next().is_some_and(|last| last.chars().all(|c| c.is_ascii_digit()));
        if !labels_ok || (numeric && host.parse::<std::net::Ipv4Addr>().is_err()) {
            return None;
        }
        (host, port)
    };
    match port {
        None => Some(host),
        Some(port) if port.chars().all(|c| c.is_ascii_digit()) => {
            port.parse::<u16>().ok().filter(|p| *p > 0).map(|_| host)
        }
        Some(_) => None,
    }
}

fn is_loopback(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost") || host == "127.0.0.1" || host == "[::1]"
}

/// `url` without its trailing slashes (`https://example.com/` → `https://example.com`).
pub fn without_trailing_slash(url: &str) -> &str {
    let trimmed = url.trim_end_matches('/');
    if trimmed.ends_with(':') { url } else { trimmed }
}

/// Whether `value` is a site path: starts with one `/`, without white space, control
/// characters or backslashes (`/preview`; never `//host`, which browsers read as an address).
pub fn is_site_path(value: &str) -> bool {
    value.starts_with('/')
        && !value.starts_with("//")
        && !value.chars().any(|c| c.is_whitespace() || c.is_control() || c == '\\')
}

/// Whether `value` is a slug: lowercase ASCII letters, digits and hyphens (`xin-chao-viet-nam`).
pub fn is_slug(value: &str) -> bool {
    !value.is_empty() && value.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn web_urls() {
        for ok in [
            "https://example.com",
            "https://example.com/",
            "https://blog.example.com:8443/vi?x=1#top",
            "HTTPS://Example.com",
            "https://[2001:db8::1]/",
            "https://192.168.1.10:8443",
            "https://123.example.com",
        ] {
            assert!(is_web_url(ok, false), "{ok}");
        }
        for ok in ["http://localhost:3000", "http://127.0.0.1", "http://[::1]:3000/", "http://LOCALHOST"] {
            assert!(is_web_url(ok, true), "{ok}");
            assert!(!is_web_url(ok, false), "{ok}");
        }
        for bad in [
            "",
            "example.com",
            "http://example.com",
            "http://localhost.example.com",
            "ftp://example.com",
            "https://",
            "https:///path",
            "https://user@example.com",
            "https://exa mple.com",
            "https://example.com:0",
            "https://example.com:99999",
            "https://example.com:",
            "https://-example.com",
            "https://example..com",
            "https://[::1",
            "https://[::1]x",
            "https://[abcd]/",
            "https://[1:2:3]/",
            "https://[]/",
            "https://999.1.1.1/",
            "https://1.2.3/",
            "https://example.com\\@evil.com",
            "javascript:alert(1)",
        ] {
            assert!(!is_web_url(bad, true), "{bad}");
        }
    }

    #[test]
    fn trailing_slashes() {
        assert_eq!(without_trailing_slash("https://example.com/"), "https://example.com");
        assert_eq!(without_trailing_slash("https://example.com/blog//"), "https://example.com/blog");
        assert_eq!(without_trailing_slash("https://example.com"), "https://example.com");
    }

    #[test]
    fn paths_and_slugs() {
        assert!(is_site_path("/preview"));
        assert!(is_site_path("/"));
        for bad in ["preview", "//evil.com", "/\\evil.com", "/a b", ""] {
            assert!(!is_site_path(bad), "{bad}");
        }
        assert!(is_slug("xin-chao-viet-nam"));
        assert!(is_slug("about2"));
        for bad in ["", "Xin", "xin chao", "xin_chao", "chào"] {
            assert!(!is_slug(bad), "{bad}");
        }
        assert_eq!(filled(&Some("  ".into())), None);
        assert_eq!(filled(&Some(" a ".into())), Some("a"));
        assert_eq!(filled(&None), None);
    }
}
