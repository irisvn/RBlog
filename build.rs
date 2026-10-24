//! What the binary says of itself (Integrations → Version, `services::version`):
//!
//! - `RBLOG_BUILD_VERSION`: the crate's version. `RBLOG_VERSION` (a release's `X.Y.Z`, set by
//!   `.github/workflows/release.yml`) must equal it.
//! - `RBLOG_BUILD_COMMIT`: `RBLOG_COMMIT` (the release's commit), or empty.
//! - `RBLOG_BUILD_CHANNEL`: `release` with `RBLOG_VERSION` (a tagged build), `development` with
//!   `RBLOG_COMMIT` or on CI (`CI`) without it, else `source` (built by hand).
//! - `RBLOG_CORE_VERSION`: the RustCMS Core it was built with (`Cargo.lock`).

fn main() {
    println!("cargo:rerun-if-env-changed=RBLOG_VERSION");
    println!("cargo:rerun-if-env-changed=RBLOG_COMMIT");
    println!("cargo:rerun-if-env-changed=CI");
    println!("cargo:rerun-if-changed=Cargo.lock");

    let crate_version = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
    let set = |name: &str| std::env::var(name).ok().map(|v| v.trim().to_owned()).filter(|v| !v.is_empty());
    let release = set("RBLOG_VERSION").map(|v| v.trim_start_matches('v').to_owned());
    if let Some(release) = &release {
        assert!(
            *release == crate_version,
            "RBLOG_VERSION is {release} but Cargo.toml says {crate_version}: tag the version Cargo.toml has"
        );
    }
    let commit: String =
        set("RBLOG_COMMIT").map(|c| c.chars().filter(char::is_ascii_hexdigit).take(40).collect()).unwrap_or_default();
    let channel = if release.is_some() {
        "release"
    } else if !commit.is_empty() || set("CI").is_some() {
        "development"
    } else {
        "source"
    };

    // The `rustcms` package of Cargo.lock: `[[package]]`, `name = "rustcms"`, `version = "…"`.
    let lock = std::fs::read_to_string("Cargo.lock").unwrap_or_default();
    let core = lock
        .split("[[package]]")
        .find(|p| p.lines().any(|l| l.trim() == "name = \"rustcms\""))
        .and_then(|p| {
            p.lines().find_map(|l| l.trim().strip_prefix("version = \"")).map(|v| v.trim_end_matches('"').to_owned())
        })
        .unwrap_or_default();

    println!("cargo:rustc-env=RBLOG_BUILD_VERSION={crate_version}");
    println!("cargo:rustc-env=RBLOG_BUILD_COMMIT={commit}");
    println!("cargo:rustc-env=RBLOG_BUILD_CHANNEL={channel}");
    println!("cargo:rustc-env=RBLOG_CORE_VERSION={core}");
}
