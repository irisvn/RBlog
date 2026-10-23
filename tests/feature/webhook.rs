//! Webhook delivery: signature, destination policy (SSRF) and the POST itself, against a
//! local axum server. No database.

use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rblog::services::webhook::{self, DeliveryError, WebhookTarget, address_allowed, sign, verify};
use rustcms::axum::Router;
use rustcms::axum::body::Bytes;
use rustcms::axum::extract::Path;
use rustcms::axum::http::{HeaderMap, StatusCode, Uri, header};
use rustcms::axum::response::IntoResponse;
use rustcms::axum::routing::post;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const SECRET: &[u8] = b"whsec-test-secret";
const EVENT: &str = "0192b0a4-7c1e-7000-8000-000000000001";
const BODY: &[u8] = br#"{"tags":["posts","post:1"]}"#;

#[test]
fn sign_and_verify_round_trip() {
    let signature = sign(SECRET, 1_700_000_000, EVENT, BODY);
    assert_eq!(signature.len(), 64);
    assert!(signature.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()));
    assert!(verify(SECRET, 1_700_000_000, EVENT, BODY, &signature));
    assert!(verify(SECRET, 1_700_000_000, EVENT, BODY, &signature.to_ascii_uppercase()));

    assert!(!verify(SECRET, 1_700_000_000, EVENT, br#"{"tags":["posts"]}"#, &signature), "tampered body");
    assert!(!verify(SECRET, 1_700_000_001, EVENT, BODY, &signature), "tampered timestamp");
    assert!(!verify(SECRET, 1_700_000_000, "0192b0a4-7c1e-7000-8000-000000000002", BODY, &signature), "tampered id");
    assert!(!verify(b"another secret", 1_700_000_000, EVENT, BODY, &signature), "other secret");
    assert!(!verify(SECRET, 1_700_000_000, EVENT, BODY, &signature[..62]), "truncated");
    assert!(!verify(SECRET, 1_700_000_000, EVENT, BODY, "not hex"));
    assert!(!verify(SECRET, 1_700_000_000, EVENT, BODY, ""));
}

/// The signed string is `timestamp.event_id.body`: a known HMAC-SHA256 vector computed with
/// `printf '1700000000.evt.{}' | openssl dgst -sha256 -hmac key`.
#[test]
fn signature_matches_the_documented_format() {
    assert_eq!(
        sign(b"key", 1_700_000_000, "evt", b"{}"),
        "3537a30cfeab2d3ea37cc70304a4e1a90535db49098b62afd68912fbb2af475c"
    );
}

#[test]
fn parse_refuses_insecure_or_ambiguous_urls() {
    for (url, why) in [
        ("http://hooks.example.com/rblog", "http"),
        ("ftp://hooks.example.com/rblog", "other scheme"),
        ("https://user@hooks.example.com/rblog", "user name"),
        ("https://user:pass@hooks.example.com/rblog", "userinfo"),
        ("https://:pass@hooks.example.com/rblog", "password"),
        ("https://", "no host"),
        ("https:///rblog", "no host"),
        ("https:////hooks.example.com/rblog", "no host"),
        ("https:\\\\hooks.example.com/rblog", "backslashes"),
        ("mailto:ops@example.com", "no host"),
        ("hooks.example.com/rblog", "not absolute"),
        ("", "empty"),
    ] {
        assert!(WebhookTarget::parse(url, false).is_err(), "{url} ({why}) must be refused");
    }
    // The flag permits http, but still no userinfo.
    assert!(WebhookTarget::parse("http://localhost:8080/hook", true).is_ok());
    assert!(WebhookTarget::parse("http://user:pass@localhost:8080/hook", true).is_err());
    assert!(WebhookTarget::parse("ftp://localhost/hook", true).is_err());

    let target = WebhookTarget::parse("https://Hooks.Example.com:8443/rblog?token=t", false).unwrap();
    assert_eq!(target.host(), "hooks.example.com");
    assert!(!format!("{target:?}").contains("token"), "Debug hides the path and query");
}

#[test]
fn classifier_refuses_every_non_public_range() {
    let refused = [
        // IPv4
        "0.0.0.0",
        "0.1.2.3",
        "10.0.0.1",
        "10.255.255.255",
        "100.64.0.1",
        "100.127.255.255",
        "127.0.0.1",
        "127.1.2.3",
        "169.254.169.254",
        "172.16.0.1",
        "172.31.255.255",
        "192.0.0.8",
        "192.0.2.1",
        "192.88.99.1",
        "192.168.1.1",
        "198.18.0.1",
        "198.19.255.255",
        "198.51.100.1",
        "203.0.113.1",
        "224.0.0.1",
        "239.255.255.250",
        "240.0.0.1",
        "255.255.255.255",
        // IPv6
        "::",
        "::1",
        "fc00::1",
        "fd12:3456::1",
        "fe80::1",
        "febf::1",
        "fec0::1",
        "ff02::1",
        "ff0e::1",
        "2001:db8::1",
        "2001:db8:ffff::1",
        "3fff::1",
        "100::1",
        "2001::1",
        "2001:2::1",
        "64:ff9b:1::1",
        // IPv4 carried in IPv6
        "::ffff:127.0.0.1",
        "::ffff:10.0.0.1",
        "::ffff:169.254.169.254",
        "::ffff:192.168.0.1",
        "::ffff:0.0.0.0",
        "::127.0.0.1",
        "::10.0.0.1",
        "64:ff9b::127.0.0.1",
        "64:ff9b::a9fe:a9fe",
        "64:ff9b::192.168.0.1",
        "2002:7f00:1::1",
        "2002:c0a8:101::1",
    ];
    for ip in refused {
        let ip: IpAddr = ip.parse().unwrap();
        assert!(!address_allowed(ip, false), "{ip} must be refused");
    }

    let public = [
        "8.8.8.8",
        "1.1.1.1",
        "93.184.215.14",
        "172.32.0.1",
        "100.128.0.1",
        "2606:4700:4700::1111",
        "2a00:1450:4001::200e",
        "::ffff:8.8.8.8",
        "64:ff9b::8.8.8.8",
        "2002:808:808::1",
    ];
    for ip in public {
        let ip: IpAddr = ip.parse().unwrap();
        assert!(address_allowed(ip, false), "{ip} is public");
    }

    // The development flag opens loopback, and nothing else.
    for ip in ["127.0.0.1", "127.0.0.53", "::1", "::ffff:127.0.0.1"] {
        assert!(address_allowed(ip.parse().unwrap(), true), "{ip} with the flag");
    }
    for ip in ["10.0.0.1", "169.254.169.254", "fd00::1", "fe80::1", "0.0.0.0", "::", "64:ff9b::127.0.0.1"] {
        assert!(!address_allowed(ip.parse().unwrap(), true), "{ip} stays refused with the flag");
    }
}

/// What the local receiver got.
#[derive(Clone, Default)]
struct Seen(Arc<Mutex<Vec<(String, HeaderMap, Bytes)>>>);

impl Seen {
    fn paths(&self) -> Vec<String> {
        self.0.lock().unwrap().iter().map(|(p, _, _)| p.clone()).collect()
    }
}

/// A receiver on 127.0.0.1: `/hook` 204, `/moved` 302 to `/hook`, `/busy` 503, `/slow-down`
/// 429, `/bad` 400 echoing the request's URI, signature and the secret.
async fn receiver() -> (SocketAddr, Seen) {
    let seen = Seen::default();
    let record = |path: &'static str, seen: Seen| {
        move |uri: Uri, headers: HeaderMap, body: Bytes| async move {
            seen.0.lock().unwrap().push((path.to_owned(), headers.clone(), body));
            match path {
                "/hook" => StatusCode::NO_CONTENT.into_response(),
                "/moved" => (StatusCode::FOUND, [(header::LOCATION, "/hook")]).into_response(),
                "/busy" => (StatusCode::SERVICE_UNAVAILABLE, "try later").into_response(),
                "/slow-down" => StatusCode::TOO_MANY_REQUESTS.into_response(),
                _ => {
                    let echoed = headers.get(webhook::HEADER_SIGNATURE).unwrap().to_str().unwrap().to_owned();
                    (
                        StatusCode::BAD_REQUEST,
                        format!("bad signature {echoed} for {uri} with key {}", std::str::from_utf8(SECRET).unwrap()),
                    )
                        .into_response()
                }
            }
        }
    };
    let app = Router::new()
        .route("/hook", post(record("/hook", seen.clone())))
        .route("/moved", post(record("/moved", seen.clone())))
        .route("/busy", post(record("/busy", seen.clone())))
        .route("/slow-down", post(record("/slow-down", seen.clone())))
        .route("/bad", post(record("/bad", seen.clone())))
        // Echoes its path segment decoded, as a framework's 404 page might.
        .route(
            "/tokens/{token}",
            post(|Path(token): Path<String>| async move { (StatusCode::NOT_FOUND, format!("unknown token {token}")) }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { rustcms::axum::serve(listener, app).await.unwrap() });
    (addr, seen)
}

fn local(addr: SocketAddr, host: &str, path: &str) -> WebhookTarget {
    WebhookTarget::parse(&format!("http://{host}:{}{path}", addr.port()), true).unwrap()
}

#[tokio::test]
async fn delivers_a_signed_post_to_a_local_receiver_with_the_flag() {
    let (addr, seen) = receiver().await;
    // An IP literal, and a name resolved then pinned (`localhost`).
    for host in ["127.0.0.1", "localhost"] {
        let delivered = webhook::deliver(&local(addr, host, "/hook"), SECRET, EVENT, BODY).await.unwrap();
        assert_eq!(delivered.status, 204);
    }

    let received = seen.0.lock().unwrap().clone();
    assert_eq!(received.len(), 2);
    for (path, headers, body) in received {
        assert_eq!(path, "/hook");
        assert_eq!(&body[..], BODY);
        assert_eq!(headers.get(header::CONTENT_TYPE).unwrap(), "application/json");
        assert_eq!(headers.get(webhook::HEADER_EVENT_ID).unwrap(), EVENT);
        let timestamp: i64 = headers.get(webhook::HEADER_TIMESTAMP).unwrap().to_str().unwrap().parse().unwrap();
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
        assert!((now - timestamp).abs() <= 5, "timestamp is the unix time in seconds");
        let signature = headers.get(webhook::HEADER_SIGNATURE).unwrap().to_str().unwrap();
        assert!(verify(SECRET, timestamp, EVENT, &body, signature));
    }
}

#[tokio::test]
async fn a_redirect_is_permanent_and_not_followed() {
    let (addr, seen) = receiver().await;
    let error = webhook::deliver(&local(addr, "127.0.0.1", "/moved"), SECRET, EVENT, BODY).await.unwrap_err();
    assert!(matches!(error, DeliveryError::Permanent(_)), "{error}");
    assert!(error.message().contains("302"), "{error}");
    assert_eq!(seen.paths(), ["/moved"], "the redirect to /hook is not followed");
}

#[tokio::test]
async fn statuses_are_classified() {
    let (addr, _) = receiver().await;
    let busy = webhook::deliver(&local(addr, "127.0.0.1", "/busy"), SECRET, EVENT, BODY).await.unwrap_err();
    assert!(busy.is_retryable(), "{busy}");
    assert!(busy.message().contains("503") && busy.message().contains("try later"), "{busy}");

    let throttled = webhook::deliver(&local(addr, "127.0.0.1", "/slow-down"), SECRET, EVENT, BODY).await.unwrap_err();
    assert!(throttled.is_retryable(), "{throttled}");

    let bad = webhook::deliver(&local(addr, "127.0.0.1", "/bad?token=url-token-1234"), SECRET, EVENT, BODY)
        .await
        .unwrap_err();
    assert!(matches!(bad, DeliveryError::Permanent(_)), "{bad}");
    assert!(bad.message().contains("400") && bad.message().contains("bad signature"), "{bad}");
    // The receiver echoed the URI and the signature: the message hides both, and never holds
    // the secret.
    assert!(bad.message().contains("[redacted]"), "{bad}");
    assert!(!bad.message().contains("url-token-1234"), "{bad}");
    assert!(!bad.to_string().contains(std::str::from_utf8(SECRET).unwrap()), "{bad}");
    let hex = bad.message().split_whitespace().any(|w| w.len() == 64 && w.bytes().all(|b| b.is_ascii_hexdigit()));
    assert!(!hex, "no signature in {bad}");

    // A decoded control character does not hide the token from redaction.
    let tab = webhook::deliver(&local(addr, "127.0.0.1", "/tokens/abc%09secret-token"), SECRET, EVENT, BODY)
        .await
        .unwrap_err();
    assert!(tab.message().contains("unknown token [redacted]"), "{tab}");
    assert!(!tab.message().contains("secret-token"), "{tab}");

    // A short secret is redacted too (here a prefix of what `/bad` echoes).
    let short = webhook::deliver(&local(addr, "127.0.0.1", "/bad"), b"whsec", EVENT, BODY).await.unwrap_err();
    assert!(!short.message().contains("whsec"), "{short}");

    let unknown = webhook::deliver(&local(addr, "127.0.0.1", "/tokens/abc%2Fsecret-token"), SECRET, EVENT, BODY)
        .await
        .unwrap_err();
    assert!(matches!(unknown, DeliveryError::Permanent(_)), "{unknown}");
    assert!(unknown.message().contains("unknown token [redacted]"), "{unknown}");
    assert!(!unknown.message().contains("secret-token"), "{unknown}");
}

#[tokio::test]
async fn an_unreachable_receiver_is_retryable() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    let error = webhook::deliver(&local(addr, "127.0.0.1", "/hook"), SECRET, EVENT, BODY).await.unwrap_err();
    assert!(error.is_retryable(), "{error}");
}

#[tokio::test]
async fn without_the_flag_localhost_is_refused_before_any_connection() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    // http itself is refused without the flag.
    assert!(WebhookTarget::parse(&format!("http://127.0.0.1:{port}/hook"), false).is_err());

    for url in [
        format!("https://127.0.0.1:{port}/hook"),
        format!("https://localhost:{port}/hook"),
        format!("https://[::ffff:127.0.0.1]:{port}/hook"),
    ] {
        let target = WebhookTarget::parse(&url, false).unwrap();
        let error = webhook::deliver(&target, SECRET, EVENT, BODY).await.unwrap_err();
        assert!(matches!(error, DeliveryError::Permanent(_)), "{url}: {error}");
        assert!(error.message().contains("not a public address"), "{url}: {error}");
    }
    let accepted = tokio::time::timeout(Duration::from_millis(200), listener.accept()).await;
    assert!(accepted.is_err(), "no connection was opened");
}

/// A 400 whose body never finishes stays Permanent: the status decides, and reading the body
/// for the message stops at the 10 s deadline.
#[tokio::test]
async fn a_stalled_error_body_keeps_the_status_classification() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        let mut buf = [0u8; 1024];
        while !request.ends_with(BODY) {
            let n = socket.read(&mut buf).await.unwrap();
            assert!(n > 0, "the request ended early");
            request.extend_from_slice(&buf[..n]);
        }
        // The body stops inside the echoed URL token.
        let head = b"HTTP/1.1 400 Bad Request\r\nContent-Length: 1000\r\n\r\nstill sending for url-token-12";
        socket.write_all(head).await.unwrap();
        // Hold the connection open, the body unfinished.
        tokio::time::sleep(Duration::from_secs(30)).await;
        drop(socket);
    });

    let started = std::time::Instant::now();
    let target = local(addr, "127.0.0.1", "/hook?token=url-token-1234");
    let error = webhook::deliver(&target, SECRET, EVENT, BODY).await.unwrap_err();
    assert!(matches!(error, DeliveryError::Permanent(_)), "{error}");
    assert!(error.message().contains("400") && error.message().contains("still sending for [redacted]"), "{error}");
    assert!(!error.message().contains("url-token"), "a cut token is redacted too: {error}");
    assert!(started.elapsed() < Duration::from_secs(12), "bounded by the deadline");
}
