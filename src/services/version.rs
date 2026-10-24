//! Integrations → Version: what RBlog runs, and whether a newer release of RBlog is out. RBlog only
//! checks and says so: the owner updates the containers himself (docs/deployment.md).
//!
//! - **What runs** ([`Build`]): the version, commit and channel `build.rs` compiled in. A
//!   `release` build (a `vX.Y.Z` tag, `.github/workflows/release.yml`) is compared with the
//!   releases; `development` (CI, `main`) and `source` (built by hand) builds are shown but
//!   **never compared**.
//! - **Checks** ([`check`]): GitHub's releases of [`REPOSITORY`] ([`RELEASES_URL`]), asked
//!   anonymously with only a `rblog/<version>` user agent, a short timeout and a bounded body.
//!   The newest non-draft, non-prerelease `vX.Y.Z` release is kept in `release_checks` (one row,
//!   shared by instances, kept across restarts) with its notes as plain text. Every 6 hours
//!   ([`SCHEDULE`]), and by hand at most once a minute ([`COOLDOWN`]). A failed check keeps the
//!   last good result and records why.
//! - The URL is fixed here and never set from the panel; `[releases] url` (config/releases.toml)
//!   points the checks at a local server in tests and development, and is refused in production.

use std::sync::{OnceLock, RwLock};
use std::time::Duration;

use rustcms::{AppError, AppResult, AppState, Config, Environment};
use sea_orm::prelude::{DateTimeUtc, Expr};
use sea_orm::{ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter, QuerySelect, Statement};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::models::release_check::{self, Column, ROW};

/// The repository whose releases are checked.
pub const REPOSITORY: &str = "irisvn/RBlog";
/// GitHub's list of its releases, newest first (20: drafts and prereleases included).
pub const RELEASES_URL: &str = "https://api.github.com/repos/irisvn/RBlog/releases?per_page=20";
/// The image a release publishes (`:X.Y.Z` and `:latest`).
pub const IMAGE: &str = "ghcr.io/irisvn/rblog";
/// The schedule that checks, and how often.
pub const SCHEDULE: &str = "release-check";
pub const PERIOD: Duration = Duration::from_secs(6 * 3600);
/// A check by hand waits this long after the last check started.
pub const COOLDOWN: Duration = Duration::from_secs(60);
/// The permission of Integrations → Version, its routes and the dashboard's release widget.
pub const PERMISSION: &str = "system.version";
/// GitHub's answer is read up to this size (20 releases with long notes fit).
pub const BODY_MAX: usize = 4 * 1024 * 1024;
/// Release notes kept, in characters.
pub const NOTES_MAX: usize = 100_000;
/// Each part of `X.Y.Z` is at most this (so that [`rank`] fits an `i64`).
const PART_MAX: u64 = 999_999;

// ---------------------------------------------------------------- What runs

/// How the binary was built (`build.rs`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Channel {
    /// A tagged release (`RBLOG_VERSION`): compared with the releases.
    Release,
    /// CI or `main` (`RBLOG_COMMIT` without a version): never compared.
    Development,
    /// Built by hand: never compared.
    Source,
}

/// What this binary is. [`Build::compiled`] in production; tests bind another one
/// (`AppBuilder::bind`) before the app boots.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Build {
    pub version: String,
    /// The full commit, when the build knew it.
    pub commit: Option<String>,
    pub channel: Channel,
}

impl Build {
    /// The facts `build.rs` compiled in.
    pub fn compiled() -> Self {
        let commit = env!("RBLOG_BUILD_COMMIT");
        Self {
            version: env!("RBLOG_BUILD_VERSION").to_owned(),
            commit: (!commit.is_empty()).then(|| commit.to_owned()),
            channel: match env!("RBLOG_BUILD_CHANNEL") {
                "release" => Channel::Release,
                "development" => Channel::Development,
                _ => Channel::Source,
            },
        }
    }

    /// Whether `latest` is an update for this build: only a release build is compared.
    pub fn is_older_than(&self, latest: &str) -> bool {
        self.channel == Channel::Release && matches!((parse(latest), parse(&self.version)), (Some(l), Some(r)) if l > r)
    }
}

/// The RustCMS Core this binary was built with (`Cargo.lock`).
pub const CORE_VERSION: &str = env!("RBLOG_CORE_VERSION");

static CURRENT: RwLock<Option<Build>> = RwLock::new(None);
static STARTED: OnceLock<DateTimeUtc> = OnceLock::new();

/// At boot (`AppServiceProvider::boot`): the build this process runs as (the bound one), and
/// when it started. A resource's `scope()` has no `AppState`, so it reads [`current`].
pub fn install(build: Build) {
    STARTED.get_or_init(now);
    *CURRENT.write().unwrap_or_else(|e| e.into_inner()) = Some(build);
}

/// The build this process runs as.
pub fn current() -> Build {
    CURRENT.read().unwrap_or_else(|e| e.into_inner()).clone().unwrap_or_else(Build::compiled)
}

/// When this process booted.
pub fn started_at() -> DateTimeUtc {
    *STARTED.get_or_init(now)
}

// ---------------------------------------------------------------- Versions

/// `X.Y.Z` with an optional leading `v`: digits only, no leading zeros, no pre-release or build
/// suffix, each part at most 999 999.
pub fn parse(version: &str) -> Option<(u64, u64, u64)> {
    let mut parts = version.strip_prefix('v').unwrap_or(version).split('.');
    let (a, b, c) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() {
        return None;
    }
    let number = |s: &str| -> Option<u64> {
        let digits = !s.is_empty() && s.len() <= 6 && s.bytes().all(|b| b.is_ascii_digit());
        let canonical = s == "0" || !s.starts_with('0');
        (digits && canonical).then(|| s.parse().ok()).flatten().filter(|n| *n <= PART_MAX)
    };
    Some((number(a)?, number(b)?, number(c)?))
}

/// `major·10¹² + minor·10⁶ + patch`: the order of versions, as a number SQL can compare.
pub fn rank(version: &str) -> Option<i64> {
    let (a, b, c) = parse(version)?;
    i64::try_from(a * 1_000_000_000_000 + b * 1_000_000 + c).ok()
}

/// A release of RBlog on GitHub, as kept.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Release {
    /// `X.Y.Z`.
    pub version: String,
    /// Its notes: plain text, at most [`NOTES_MAX`] characters.
    pub notes: Option<String>,
    /// Its page, only on `https://github.com/`.
    pub url: Option<String>,
    pub published_at: Option<DateTimeUtc>,
}

/// One entry of GitHub's list (only what is used).
#[derive(Debug, Deserialize)]
pub struct GitHubRelease {
    pub tag_name: String,
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub prerelease: bool,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub html_url: Option<String>,
    #[serde(default)]
    pub published_at: Option<String>,
}

/// The newest release that is neither a draft nor a prerelease, with a `vX.Y.Z` tag.
pub fn newest(releases: Vec<GitHubRelease>) -> Option<Release> {
    let (version, found) = releases
        .into_iter()
        .filter(|r| !r.draft && !r.prerelease)
        .filter_map(|r| parse(&r.tag_name).map(|v| (v, r)))
        .max_by_key(|(v, _)| *v)?;
    let (a, b, c) = version;
    Some(Release {
        version: format!("{a}.{b}.{c}"),
        notes: found.body.map(|b| b.chars().take(NOTES_MAX).collect()).filter(|b: &String| !b.trim().is_empty()),
        url: found.html_url.filter(|u| u.starts_with("https://github.com/") && u.len() <= 512),
        published_at: found.published_at.and_then(|at| at.parse().ok()),
    })
}

// ---------------------------------------------------------------- Configuration

/// `[releases]` (config/releases.toml).
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ReleasesConfig {
    /// Tests and development only: another releases list than GitHub's (a local server).
    /// Empty: [`RELEASES_URL`]. Refused when `APP_ENV=production`.
    pub url: String,
    /// The whole check's time limit, 1–20 seconds (under the panel's 30-second request limit).
    pub timeout_secs: u64,
}

impl Default for ReleasesConfig {
    fn default() -> Self {
        Self { url: String::new(), timeout_secs: 10 }
    }
}

impl ReleasesConfig {
    /// Reads `[releases]`. A value of the wrong kind, a timeout out of range, a URL that is not
    /// `http(s)://`, or any URL with `APP_ENV=production` is an error: the app refuses to boot.
    pub fn of(config: &Config) -> anyhow::Result<Self> {
        let releases: Self = config.section("releases")?;
        if !(1..=20).contains(&releases.timeout_secs) {
            anyhow::bail!("[releases] timeout_secs must be between 1 and 20");
        }
        if !releases.url.is_empty() {
            if config.app.env == Environment::Production {
                anyhow::bail!("[releases] url is for tests and development: leave it empty when APP_ENV=production");
            }
            if !(releases.url.starts_with("http://") || releases.url.starts_with("https://")) {
                anyhow::bail!("[releases] url must be an http(s) URL");
            }
        }
        Ok(releases)
    }

    /// Where the releases are asked.
    pub fn endpoint(&self) -> &str {
        if self.url.is_empty() { RELEASES_URL } else { &self.url }
    }
}

// ---------------------------------------------------------------- Fetching

/// Why a check failed (kept as [`Failure::code`] and [`Failure::status`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    /// No complete answer within the time limit.
    Timeout,
    /// No connection (DNS, refused, TLS).
    Unreachable,
    /// GitHub's rate limit (403 or 429).
    RateLimited(u16),
    /// Another status than 2xx (redirects are not followed).
    Http(u16),
    /// The answer is larger than [`BODY_MAX`].
    TooLarge,
    /// The answer is not a list of releases.
    Malformed,
    /// The list (its first page) holds only drafts and prereleases: a stable release may be on a
    /// later page, so none was confirmed.
    OnlyPreviews,
}

impl Failure {
    pub fn code(self) -> &'static str {
        match self {
            Self::Timeout => "timeout",
            Self::Unreachable => "unreachable",
            Self::RateLimited(_) => "rate_limited",
            Self::Http(_) => "http",
            Self::TooLarge => "too_large",
            Self::Malformed => "malformed",
            Self::OnlyPreviews => "only_previews",
        }
    }

    pub fn status(self) -> Option<u16> {
        match self {
            Self::RateLimited(status) | Self::Http(status) => Some(status),
            _ => None,
        }
    }
}

fn classify(error: &reqwest::Error) -> Failure {
    if error.is_timeout() { Failure::Timeout } else { Failure::Unreachable }
}

/// Asks the releases list once: the newest stable release (`None`: none on the list).
pub async fn fetch(config: &ReleasesConfig, build: &Build) -> Result<Option<Release>, Failure> {
    fetch_list(config, build).await.map(newest)
}

/// Asks the releases list once (its first page: the 20 newest, drafts and prereleases
/// included). Sends a `rblog/<version>` user agent and nothing else about this server (no
/// cookie, no token).
pub async fn fetch_list(config: &ReleasesConfig, build: &Build) -> Result<Vec<GitHubRelease>, Failure> {
    let timeout = Duration::from_secs(config.timeout_secs);
    let deadline = tokio::time::Instant::now() + timeout;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .connect_timeout(timeout.min(Duration::from_secs(5)))
        .timeout(timeout)
        .user_agent(format!("rblog/{}", build.version))
        .build()
        .map_err(|_| Failure::Unreachable)?;
    let send = client
        .get(config.endpoint())
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .send();
    let mut response = match tokio::time::timeout_at(deadline, send).await {
        Err(_) => return Err(Failure::Timeout),
        Ok(Err(e)) => return Err(classify(&e)),
        Ok(Ok(response)) => response,
    };
    let status = response.status();
    if status.as_u16() == 403 || status.as_u16() == 429 {
        return Err(Failure::RateLimited(status.as_u16()));
    }
    if !status.is_success() {
        return Err(Failure::Http(status.as_u16()));
    }
    if response.content_length().is_some_and(|n| n > BODY_MAX as u64) {
        return Err(Failure::TooLarge);
    }
    let mut body = Vec::new();
    loop {
        match tokio::time::timeout_at(deadline, response.chunk()).await {
            Err(_) => return Err(Failure::Timeout),
            Ok(Err(e)) => return Err(classify(&e)),
            Ok(Ok(None)) => break,
            Ok(Ok(Some(chunk))) => {
                if body.len() + chunk.len() > BODY_MAX {
                    return Err(Failure::TooLarge);
                }
                body.extend_from_slice(&chunk);
            }
        }
    }
    serde_json::from_slice(&body).map_err(|_| Failure::Malformed)
}

// ---------------------------------------------------------------- Checking

/// Who asked for a check.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trigger {
    /// "Check now" on Integrations → Version: at most once per [`COOLDOWN`].
    Manual,
    /// The schedule or `rblog version:check`: never refused.
    Schedule,
}

/// A check by hand within [`COOLDOWN`] of the last one: again in this many seconds (1–60).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TooSoon(pub u64);

/// Checks now and keeps what it found: the release, or why it failed. Instances are serialized
/// by the row's lock while claiming the check; a slower, older check never overwrites a newer
/// one's result (its `attempted_at` is the token of the write).
pub async fn check(app: &AppState, trigger: Trigger) -> AppResult<Result<Result<Option<Release>, Failure>, TooSoon>> {
    let config = ReleasesConfig::of(app.config()).map_err(|e| AppError::internal(format!("{e:#}")))?;
    let build = current();
    let token = match claim(app, trigger).await? {
        Ok(token) => token,
        Err(too_soon) => return Ok(Err(too_soon)),
    };
    // A first page of only drafts and prereleases does not mean there is no release (it may be on
    // a later page): nothing is confirmed, the last result is kept. Only an empty list says none.
    let found = fetch_list(&config, &build).await.and_then(|list| {
        let listed = !list.is_empty();
        match newest(list) {
            None if listed => Err(Failure::OnlyPreviews),
            found => Ok(found),
        }
    });
    let txn = rustcms::db::begin_write(app.db()).await?;
    let mut update = release_check::Entity::update_many();
    update = match &found {
        Ok(release) => update
            .col_expr(Column::CheckedAt, Expr::value(now()))
            .col_expr(Column::LatestVersion, Expr::value(release.as_ref().map(|r| r.version.clone())))
            .col_expr(Column::LatestRank, Expr::value(release.as_ref().and_then(|r| rank(&r.version))))
            .col_expr(Column::Notes, Expr::value(release.as_ref().and_then(|r| r.notes.clone())))
            .col_expr(Column::Url, Expr::value(release.as_ref().and_then(|r| r.url.clone())))
            .col_expr(Column::PublishedAt, Expr::value(release.as_ref().and_then(|r| r.published_at)))
            .col_expr(Column::Error, Expr::value(Option::<String>::None))
            .col_expr(Column::ErrorStatus, Expr::value(Option::<i32>::None))
            .col_expr(Column::ErrorAt, Expr::value(Option::<DateTimeUtc>::None)),
        Err(failure) => update
            .col_expr(Column::Error, Expr::value(failure.code()))
            .col_expr(Column::ErrorStatus, Expr::value(failure.status().map(i32::from)))
            .col_expr(Column::ErrorAt, Expr::value(now())),
    };
    update.filter(Column::Id.eq(ROW)).filter(Column::AttemptedAt.eq(token)).exec(&txn).await?;
    txn.commit().await?;
    Ok(Ok(found))
}

/// Records that a check starts now, unless `trigger` is manual and the last one started less
/// than [`COOLDOWN`] ago. The token is this check's `attempted_at`.
async fn claim(app: &AppState, trigger: Trigger) -> AppResult<Result<DateTimeUtc, TooSoon>> {
    // The migration made the row; a database restored without it gets it back, on its own
    // (an `INSERT IGNORE` in the claim's transaction would take a shared lock first, and two
    // claims upgrading it to an exclusive one would deadlock).
    if release_check::Entity::find_by_id(ROW).one(app.db()).await?.is_none() {
        let db = app.db();
        db.execute_raw(Statement::from_string(
            db.get_database_backend(),
            format!("INSERT IGNORE INTO release_checks (id) VALUES ({ROW})"),
        ))
        .await?;
    }
    // The row's exclusive lock is the transaction's first and only lock: claims queue on it
    // (RustCMS's `xact_lock` names on MySQL are preseeded framework rows, none of them ours).
    let txn = rustcms::db::begin_write(app.db()).await?;
    let row = release_check::Entity::find_by_id(ROW)
        .lock_exclusive()
        .one(&txn)
        .await?
        .ok_or_else(|| AppError::internal("release_checks has no row"))?;
    let mut token = now();
    if let Some(last) = row.attempted_at {
        let since = token - last;
        let cooldown = chrono_duration(COOLDOWN);
        if trigger == Trigger::Manual && since < cooldown {
            let wait = ((cooldown - since).num_milliseconds() + 999) / 1000;
            let wait = wait.clamp(1, COOLDOWN.as_secs() as i64);
            return Ok(Err(TooSoon(wait as u64)));
        }
        // A token is never reused, even when clocks disagree.
        if token <= last {
            token = last + chrono_duration(Duration::from_micros(1));
        }
    }
    release_check::Entity::update_many()
        .col_expr(Column::AttemptedAt, Expr::value(token))
        .filter(Column::Id.eq(ROW))
        .exec(&txn)
        .await?;
    txn.commit().await?;
    Ok(Ok(token))
}

/// The scheduled check's summary on the Schedules page (no secrets: there are none).
pub fn summary(found: &Result<Option<Release>, Failure>) -> String {
    match found {
        Ok(Some(release)) if current().is_older_than(&release.version) => {
            format!("RBlog {} is out", release.version)
        }
        Ok(Some(release)) => format!("the newest release is {}", release.version),
        Ok(None) => "no release published yet".to_owned(),
        Err(failure) => match failure.status() {
            Some(status) => format!("failed: {} (HTTP {status})", failure.code()),
            None => format!("failed: {}", failure.code()),
        },
    }
}

// ---------------------------------------------------------------- Reading

/// The kept check (an empty one before the first).
pub async fn last<C: ConnectionTrait>(db: &C) -> AppResult<release_check::Model> {
    Ok(release_check::Entity::find_by_id(ROW).one(db).await?.unwrap_or(release_check::Model {
        id: ROW,
        attempted_at: None,
        checked_at: None,
        latest_version: None,
        latest_rank: None,
        notes: None,
        url: None,
        published_at: None,
        error: None,
        error_status: None,
        error_at: None,
    }))
}

/// Where this build stands: `update` (a newer release), `current`, `no_release` (none
/// published), `not_checked` (no successful check yet), `not_compared` (development or source).
pub fn state(build: &Build, kept: &release_check::Model) -> &'static str {
    if build.channel != Channel::Release {
        "not_compared"
    } else if kept.checked_at.is_none() {
        "not_checked"
    } else {
        match kept.latest_version.as_deref() {
            None => "no_release",
            Some(latest) if build.is_older_than(latest) => "update",
            Some(_) => "current",
        }
    }
}

/// Integrations → Version (`GET …/system/version`): what runs, the last check, the newest release
/// (its notes as text), and the migrations a newer version applied to this database.
pub async fn status(app: &AppState) -> AppResult<Value> {
    let db = app.db();
    let build = current();
    let kept = last(db).await?;
    let mysql = db
        .query_one_raw(Statement::from_string(db.get_database_backend(), "SELECT VERSION() AS v"))
        .await?
        .and_then(|row| row.try_get::<String>("", "v").ok());
    let state = state(&build, &kept);
    let next_manual = kept.attempted_at.map(|at| at + chrono_duration(COOLDOWN)).filter(|at| *at > now());
    let newer_migrations =
        rustcms::db::unknown_migrations::<crate::database::migrations::Migrator>(db).await.unwrap_or_default();
    Ok(json!({
        "running": {
            "version": build.version,
            "commit": build.commit,
            "channel": build.channel,
            "core": CORE_VERSION,
            "mysql": mysql,
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
            "started_at": started_at(),
        },
        "repository": format!("https://github.com/{REPOSITORY}"),
        "image": IMAGE,
        "state": state,
        "update": (state == "update").then(|| kept.latest_version.clone()).flatten(),
        "check": {
            "attempted_at": kept.attempted_at,
            "checked_at": kept.checked_at,
            "error": kept.error.as_ref().map(|code| json!({
                "code": code,
                "status": kept.error_status,
                "at": kept.error_at,
            })),
            "every_hours": PERIOD.as_secs() / 3600,
            "next_manual_at": next_manual,
        },
        "release": kept.latest_version.as_ref().map(|version| json!({
            "version": version,
            "notes": kept.notes,
            "url": kept.url,
            "published_at": kept.published_at,
        })),
        "newer_migrations": newer_migrations,
    }))
}

/// Now, to the microsecond (what `DATETIME(6)` keeps, so a token reads back equal).
pub fn now() -> DateTimeUtc {
    let at = DateTimeUtc::from(std::time::SystemTime::now());
    DateTimeUtc::from_timestamp_micros(at.timestamp_micros()).unwrap_or(at)
}

fn chrono_duration(d: Duration) -> sea_orm::sea_query::prelude::chrono::TimeDelta {
    sea_orm::sea_query::prelude::chrono::TimeDelta::from_std(d).unwrap_or_default()
}
