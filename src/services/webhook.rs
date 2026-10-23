//! Webhook delivery: one signed POST of a content event to the frontend (`docs/api.md`,
//! "Webhook").
//!
//! - [`sign`] and [`verify`]: the hex HMAC-SHA256 of `timestamp + "." + event_id + "." + body`.
//! - [`WebhookTarget::parse`]: HTTPS only, no userinfo, a host.
//! - [`deliver`]: resolves the host at every call, refuses the delivery if **any** address is not
//!   public ([`address_allowed`]), then connects to exactly the checked addresses (DNS rebinding
//!   cannot swap them), with TLS still verified against the hostname. No redirects, no proxy,
//!   5 s to connect and 10 s in all.
//!
//! The caller (the delivery job) chooses what to do with a [`DeliveryError`]: retry a
//! [`DeliveryError::Retryable`] with backoff, fail a [`DeliveryError::Permanent`] at once. No
//! message holds the secret, the signature or the URL's path and query: a message names the
//! host, and the receiver's error body (at most 4 KiB) is quoted with those redacted. Path
//! segments shorter than 6 bytes are not redacted, so do not put a token that short in a path.

use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use hmac::{Hmac, KeyInit, Mac};
use reqwest::dns::{Name, Resolve, Resolving};
use reqwest::{StatusCode, Url, redirect};
use sha2::Sha256;
use tokio::time::Instant;

/// Header carrying the event id.
pub const HEADER_EVENT_ID: &str = "X-RBlog-Event-Id";
/// Header carrying the unix time of the delivery, in seconds.
pub const HEADER_TIMESTAMP: &str = "X-RBlog-Timestamp";
/// Header carrying the hex HMAC-SHA256 signature.
pub const HEADER_SIGNATURE: &str = "X-RBlog-Signature";

/// Time to open the connection (TCP and TLS).
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// Time for the whole delivery: DNS, connection, request and response.
const TOTAL_TIMEOUT: Duration = Duration::from_secs(10);
/// Bytes of the response body kept, for the error message only.
const BODY_SNIPPET: usize = 4096;

type HmacSha256 = Hmac<Sha256>;

fn mac(secret: &[u8], timestamp: i64, event_id: &str, body: &[u8]) -> HmacSha256 {
    // HMAC takes a key of any length.
    let mut mac = HmacSha256::new_from_slice(secret).expect("HMAC accepts any key length");
    mac.update(timestamp.to_string().as_bytes());
    mac.update(b".");
    mac.update(event_id.as_bytes());
    mac.update(b".");
    mac.update(body);
    mac
}

/// The hex (lowercase) HMAC-SHA256 of `timestamp + "." + event_id + "." + body` under `secret`.
pub fn sign(secret: &[u8], timestamp: i64, event_id: &str, body: &[u8]) -> String {
    hex::encode(mac(secret, timestamp, event_id, body).finalize().into_bytes())
}

/// Whether `signature` (hex, either case) is [`sign`]'s output for these values, compared in
/// constant time. It checks the signature only: a receiver also refuses old timestamps and
/// event ids it has already handled.
pub fn verify(secret: &[u8], timestamp: i64, event_id: &str, body: &[u8], signature: &str) -> bool {
    let Ok(tag) = hex::decode(signature.trim()) else {
        return false;
    };
    mac(secret, timestamp, event_id, body).verify_slice(&tag).is_ok()
}

/// A webhook URL accepted by [`WebhookTarget::parse`].
#[derive(Clone, PartialEq, Eq)]
pub struct WebhookTarget {
    url: Url,
    /// The host as written in the URL: a domain (lowercase, punycode) or an IP literal.
    host: Host,
    port: u16,
    allow_insecure_local: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Host {
    Domain(String),
    Ip(IpAddr),
}

/// Why a URL is not a webhook target.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvalidTarget(&'static str);

impl fmt::Display for InvalidTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for InvalidTarget {}

impl WebhookTarget {
    /// Accepts an `https://` URL with a host (and optionally a port), without userinfo.
    ///
    /// `allow_insecure_local` (development only) also accepts `http://` and lets [`deliver`]
    /// reach loopback addresses. It never opens private or other non-public ranges.
    pub fn parse(url: &str, allow_insecure_local: bool) -> Result<Self, InvalidTarget> {
        let url = url.trim();
        // The URL standard repairs `https:///host` and `https:\\host` into a host; a webhook URL
        // must say what it means.
        if url.contains('\\') || url.chars().any(char::is_control) {
            return Err(InvalidTarget("the webhook URL is not a valid URL"));
        }
        if url.split_once("://").is_none_or(|(_, rest)| rest.is_empty() || rest.starts_with('/')) {
            return Err(InvalidTarget("the webhook URL has no host"));
        }
        let url = Url::parse(url).map_err(|_| InvalidTarget("the webhook URL is not a valid URL"))?;
        match url.scheme() {
            "https" => {}
            "http" if allow_insecure_local => {}
            _ => return Err(InvalidTarget("the webhook URL must use https")),
        }
        if !url.username().is_empty() || url.password().is_some() {
            return Err(InvalidTarget("the webhook URL must not contain a user name or password"));
        }
        let host = match url.host_str() {
            None | Some("") => return Err(InvalidTarget("the webhook URL has no host")),
            Some(host) => match host.strip_prefix('[').and_then(|h| h.strip_suffix(']')).unwrap_or(host).parse() {
                Ok(ip) => Host::Ip(ip),
                Err(_) => Host::Domain(host.to_owned()),
            },
        };
        let port = url.port_or_known_default().ok_or(InvalidTarget("the webhook URL has no port"))?;
        Ok(Self { url, host, port, allow_insecure_local })
    }

    /// The URL, which may hold a token in its path or query: never log it.
    pub fn url(&self) -> &Url {
        &self.url
    }

    /// The host, safe for messages.
    pub fn host(&self) -> &str {
        self.url.host_str().unwrap_or_default()
    }
}

impl fmt::Debug for WebhookTarget {
    // The path and query may carry a token: show the host and port only.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WebhookTarget")
            .field("scheme", &self.url.scheme())
            .field("host", &self.host())
            .field("port", &self.port)
            .field("allow_insecure_local", &self.allow_insecure_local)
            .finish()
    }
}

/// Whether a delivery may connect to `ip`: a public unicast address, or a loopback address when
/// `allow_insecure_local` is set. IPv4 embedded in IPv6 (mapped, compatible, NAT64, 6to4) is
/// judged as the IPv4 address it carries.
pub fn address_allowed(ip: IpAddr, allow_insecure_local: bool) -> bool {
    match ip {
        IpAddr::V4(v4) => v4_allowed(v4, allow_insecure_local),
        IpAddr::V6(v6) => v6_allowed(v6, allow_insecure_local),
    }
}

fn in_v4(ip: Ipv4Addr, net: [u8; 4], prefix: u32) -> bool {
    let mask = if prefix == 0 { 0 } else { u32::MAX << (32 - prefix) };
    u32::from(ip) & mask == u32::from(Ipv4Addr::from(net)) & mask
}

fn in_v6(ip: Ipv6Addr, net: [u16; 8], prefix: u32) -> bool {
    let mask = if prefix == 0 { 0 } else { u128::MAX << (128 - prefix) };
    u128::from(ip) & mask == u128::from(Ipv6Addr::from(net)) & mask
}

/// IPv4 ranges that are not public unicast (IANA special-purpose registry).
const V4_REFUSED: &[([u8; 4], u32)] = &[
    ([0, 0, 0, 0], 8),       // "this network"
    ([10, 0, 0, 0], 8),      // private
    ([100, 64, 0, 0], 10),   // shared address space (CGNAT)
    ([127, 0, 0, 0], 8),     // loopback
    ([169, 254, 0, 0], 16),  // link-local (cloud metadata)
    ([172, 16, 0, 0], 12),   // private
    ([192, 0, 0, 0], 24),    // IETF protocol assignments
    ([192, 0, 2, 0], 24),    // documentation (TEST-NET-1)
    ([192, 88, 99, 0], 24),  // 6to4 relay anycast
    ([192, 168, 0, 0], 16),  // private
    ([198, 18, 0, 0], 15),   // benchmarking
    ([198, 51, 100, 0], 24), // documentation (TEST-NET-2)
    ([203, 0, 113, 0], 24),  // documentation (TEST-NET-3)
    ([224, 0, 0, 0], 4),     // multicast
    ([240, 0, 0, 0], 4),     // reserved, including broadcast 255.255.255.255
];

fn v4_allowed(ip: Ipv4Addr, allow_insecure_local: bool) -> bool {
    if ip.is_loopback() {
        return allow_insecure_local;
    }
    !V4_REFUSED.iter().any(|&(net, prefix)| in_v4(ip, net, prefix))
}

fn v6_allowed(ip: Ipv6Addr, allow_insecure_local: bool) -> bool {
    if ip.is_loopback() {
        return allow_insecure_local;
    }
    if ip.is_unspecified() {
        return false;
    }
    let bits = u128::from(ip);
    let low_v4 = Ipv4Addr::from(bits as u32);
    // ::ffff:a.b.c.d (mapped) and ::a.b.c.d (compatible, deprecated).
    if in_v6(ip, [0, 0, 0, 0, 0, 0xffff, 0, 0], 96) || in_v6(ip, [0; 8], 96) {
        return v4_allowed(low_v4, allow_insecure_local);
    }
    // NAT64: the gateway forwards to the IPv4 address it carries, never to its own loopback.
    if in_v6(ip, [0x64, 0xff9b, 0, 0, 0, 0, 0, 0], 96) {
        return v4_allowed(low_v4, false);
    }
    // 6to4 (2002:a.b.c.d::/48) likewise.
    if in_v6(ip, [0x2002, 0, 0, 0, 0, 0, 0, 0], 16) {
        return v4_allowed(Ipv4Addr::from((bits >> 80) as u32), false);
    }
    // Only global unicast (2000::/3), minus its special-purpose blocks. This refuses unique-local
    // fc00::/7, link-local fe80::/10, site-local fec0::/10, multicast ff00::/8, discard 100::/64
    // and local-use NAT64 64:ff9b:1::/48 among others.
    in_v6(ip, [0x2000, 0, 0, 0, 0, 0, 0, 0], 3)
        && !in_v6(ip, [0x2001, 0, 0, 0, 0, 0, 0, 0], 23) // IETF protocol assignments (Teredo, benchmarking, ORCHID)
        && !in_v6(ip, [0x2001, 0xdb8, 0, 0, 0, 0, 0, 0], 32) // documentation
        && !in_v6(ip, [0x3fff, 0, 0, 0, 0, 0, 0, 0], 20) // documentation
}

/// A delivery the receiver accepted (2xx).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Delivered {
    pub status: u16,
}

/// Why a delivery failed. Messages hold the host, never the full URL, the secret or the
/// signature.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeliveryError {
    /// May succeed later: a timeout, a DNS or connection error, 408, 425, 429 or 5xx.
    Retryable(String),
    /// Will not succeed by retrying: a bad URL, a refused destination, 3xx or another 4xx.
    Permanent(String),
}

impl DeliveryError {
    pub fn is_retryable(&self) -> bool {
        matches!(self, Self::Retryable(_))
    }

    pub fn message(&self) -> &str {
        match self {
            Self::Retryable(m) | Self::Permanent(m) => m,
        }
    }
}

impl fmt::Display for DeliveryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Retryable(m) => write!(f, "webhook delivery failed (retryable): {m}"),
            Self::Permanent(m) => write!(f, "webhook delivery failed: {m}"),
        }
    }
}

impl std::error::Error for DeliveryError {}

impl From<InvalidTarget> for DeliveryError {
    fn from(e: InvalidTarget) -> Self {
        Self::Permanent(e.to_string())
    }
}

/// The resolver of the delivery client: every host it may reach is pinned with
/// `resolve_to_addrs`, so any other lookup is refused instead of going to the system resolver.
struct NoResolver;

impl Resolve for NoResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let message = format!("`{}` was not checked by the webhook policy", name.as_str());
        Box::pin(async move { Err(message.into()) })
    }
}

/// The addresses of `target` that a delivery may connect to, all of them checked.
async fn checked_addrs(target: &WebhookTarget) -> Result<Vec<SocketAddr>, DeliveryError> {
    let host = target.host();
    let addrs: Vec<SocketAddr> = match &target.host {
        Host::Ip(ip) => vec![SocketAddr::new(*ip, target.port)],
        Host::Domain(domain) => tokio::net::lookup_host((domain.as_str(), target.port))
            .await
            .map_err(|e| DeliveryError::Retryable(format!("cannot resolve `{host}`: {e}")))?
            .collect(),
    };
    if addrs.is_empty() {
        return Err(DeliveryError::Retryable(format!("`{host}` has no address")));
    }
    if let Some(bad) = addrs.iter().find(|a| !address_allowed(a.ip(), target.allow_insecure_local)) {
        return Err(DeliveryError::Permanent(format!(
            "`{host}` resolves to {}, which is not a public address",
            bad.ip()
        )));
    }
    Ok(addrs)
}

/// POSTs `body` (JSON) to `target`, signed with `secret` (see the module docs).
pub async fn deliver(
    target: &WebhookTarget,
    secret: &[u8],
    event_id: &str,
    body: &[u8],
) -> Result<Delivered, DeliveryError> {
    let deadline = Instant::now() + TOTAL_TIMEOUT;
    let host = target.host();
    // Header values must be visible ASCII; event ids are UUIDs.
    if event_id.is_empty() || !event_id.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(DeliveryError::Permanent("the event id is not a valid header value".into()));
    }
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or_default();
    let signature = sign(secret, timestamp, event_id, body);

    let send = async {
        let addrs = checked_addrs(target).await?;
        let mut builder = reqwest::Client::builder()
            .redirect(redirect::Policy::none())
            .no_proxy()
            .https_only(!target.allow_insecure_local)
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(TOTAL_TIMEOUT)
            .dns_resolver(Arc::new(NoResolver));
        if let Host::Domain(domain) = &target.host {
            builder = builder.resolve_to_addrs(domain, &addrs);
        }
        let client = builder
            .build()
            .map_err(|e| DeliveryError::Permanent(format!("cannot build the HTTP client: {}", describe(e))))?;
        client
            .post(target.url.clone())
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .header(HEADER_EVENT_ID, event_id)
            .header(HEADER_TIMESTAMP, timestamp.to_string())
            .header(HEADER_SIGNATURE, &signature)
            .body(body.to_vec())
            .send()
            .await
            .map_err(|e| {
                let kind = if e.is_timeout() { "timed out" } else { "failed" };
                DeliveryError::Retryable(format!("the request to `{host}` {kind}: {}", describe(e)))
            })
    };
    let response = match tokio::time::timeout_at(deadline, send).await {
        Ok(response) => response?,
        Err(_) => return Err(DeliveryError::Retryable(format!("`{host}` did not answer within 10 s"))),
    };

    let status = response.status();
    if status.is_success() {
        return Ok(Delivered { status: status.as_u16() });
    }
    // The status decides; the body only explains, as far as it arrives before the deadline.
    let snippet = snippet(response, deadline, &redactions(target, secret, &signature)).await;
    let message = if status.is_redirection() {
        format!("`{host}` answered {status} (redirects are not followed){snippet}")
    } else {
        format!("`{host}` answered {status}{snippet}")
    };
    Err(if retryable_status(status) { DeliveryError::Retryable(message) } else { DeliveryError::Permanent(message) })
}

fn retryable_status(status: StatusCode) -> bool {
    matches!(status.as_u16(), 408 | 425 | 429) || status.is_server_error()
}

/// A reqwest error and its causes, without the URL.
fn describe(e: reqwest::Error) -> String {
    let e = e.without_url();
    let mut text = e.to_string();
    let mut source = std::error::Error::source(&e);
    while let Some(cause) = source {
        text.push_str(": ");
        text.push_str(&cause.to_string());
        source = cause.source();
    }
    text
}

/// What a receiver may echo back and a message must not hold: the secret, the signature, and
/// the parts of the URL that can carry a token (path, path segments, query and its values),
/// longest first.
fn redactions(target: &WebhookTarget, secret: &[u8], signature: &str) -> Vec<String> {
    let url = &target.url;
    // Path segments shorter than this (`/`, `v1`, `hook`) are not secrets, and redacting them
    // would garble the message. Everything else is redacted whatever its length.
    const PATH_MIN: usize = 6;
    let mut path = vec![url.path().to_owned()];
    path.extend(url.path_segments().into_iter().flatten().map(str::to_owned));
    let mut out =
        vec![String::from_utf8_lossy(secret).into_owned(), signature.to_owned(), signature.to_ascii_uppercase()];
    if let Some(query) = url.query() {
        out.push(query.to_owned());
        out.extend(query.split('&').filter_map(|pair| pair.split_once('=')).map(|(_, v)| v.to_owned()));
    }
    out.extend(url.query_pairs().map(|(_, v)| v.into_owned()));
    // A receiver may echo the URL decoded (`abc%2Fsecret` as `abc/secret`).
    let decoded: Vec<String> = out[3..].iter().map(|s| percent_decode(s)).collect();
    out.extend(decoded);
    path.extend(path.clone().iter().map(|s| percent_decode(s)));
    out.extend(path.into_iter().filter(|s| s.len() >= PATH_MIN));
    out.retain(|s| !s.is_empty());
    out.sort_by_key(|s| std::cmp::Reverse(s.len()));
    out.dedup();
    out
}

/// `%XX` escapes decoded (invalid UTF-8 replaced); anything else kept as is.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = bytes.get(i + 1..i + 3).filter(|h| h.iter().all(u8::is_ascii_hexdigit));
        let hex = hex.and_then(|h| u8::from_str_radix(std::str::from_utf8(h).ok()?, 16).ok());
        match (bytes[i], hex) {
            (b'%', Some(byte)) => {
                out.push(byte);
                i += 3;
            }
            (byte, _) => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Where `text` ends in a proper prefix of `piece` at least 3 bytes long, the prefix's start.
fn partial_tail(text: &str, piece: &str) -> Option<usize> {
    (3..piece.len())
        .rev()
        .filter(|&k| piece.is_char_boundary(k))
        .find(|&k| text.ends_with(&piece[..k]))
        .map(|k| text.len() - k)
}

/// At most [`BODY_SNIPPET`] bytes of the response body as `": …"`, printable, with every
/// `redactions` piece replaced (a receiver may echo the request back). Reading stops at the
/// deadline, keeping what arrived.
async fn snippet(mut response: reqwest::Response, deadline: Instant, redactions: &[String]) -> String {
    // Read the longest piece's length past the limit, so that one cut by the limit is still
    // redacted.
    let limit = BODY_SNIPPET + redactions.first().map_or(0, String::len);
    let mut bytes = Vec::new();
    while bytes.len() < limit {
        match tokio::time::timeout_at(deadline, response.chunk()).await {
            Ok(Ok(Some(chunk))) => bytes.extend_from_slice(&chunk[..chunk.len().min(limit - bytes.len())]),
            _ => break,
        }
    }
    // Redact first, on the text as received, then make it printable.
    let mut text = String::from_utf8_lossy(&bytes).into_owned();
    for piece in redactions {
        text = text.replace(piece.as_str(), "[redacted]");
    }
    // A body that stopped early (deadline, error) may end in the start of a piece.
    if let Some(cut) = redactions.iter().filter_map(|piece| partial_tail(&text, piece)).min() {
        text.truncate(cut);
        text.push_str("[redacted]");
    }
    let text: String = text.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    let mut end = text.len().min(BODY_SNIPPET);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let text = text[..end].trim();
    if text.is_empty() { String::new() } else { format!(": {text}") }
}
