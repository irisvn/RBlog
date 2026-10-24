// Integrations → Version (`/admin/p/version`, `VersionPage`): what RBlog runs, whether a newer
// release is out (GitHub, every 6 hours or by hand at most once a minute), its notes as plain
// text, and how to update with docker compose. RBlog never updates itself. Read through
// `routes::admin` (`/system/version`); the server checks every request.
import { useEffect, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { adminKey, registerPage, useAdminQuery, type PageProps } from "@rustcms/admin-sdk";
import { ApiError } from "@rustcms/admin-sdk/host";
import { Alert, Button, CodeBlock, ErrorState, Skeleton } from "@rustcms/admin-sdk/ui";
import { dateTime, relative } from "./lib/format";
import "./lib/i18n";
import { Icon } from "./lib/icons";
import { versionStatus } from "./lib/status";
import { EmptyState, Facts, fill, Page, Section, Stack, StatusBadge } from "./lib/ui";
import "./version.css";

const PATH = "/system/version";
const DEPLOYMENT = "https://github.com/irisvn/RBlog/blob/main/docs/deployment.md";

type Channel = "release" | "development" | "source";
type State = "update" | "current" | "no_release" | "not_checked" | "not_compared";
type Status = {
  running: {
    version: string;
    commit: string | null;
    channel: Channel;
    core: string;
    mysql: string | null;
    os: string;
    arch: string;
    started_at: string;
  };
  repository: string;
  image: string;
  state: State;
  update: string | null;
  check: {
    attempted_at: string | null;
    checked_at: string | null;
    error: { code: string; status: number | null; at: string | null } | null;
    every_hours: number;
    next_manual_at: string | null;
  };
  release: { version: string; notes: string | null; url: string | null; published_at: string | null } | null;
  newer_migrations: string[];
};

const CHANNELS: Record<Channel, string> = {
  release: "Release",
  development: "Development build",
  source: "Built from source",
};

/** Why the last check failed, in words (`services::version::Failure`). */
function failure(error: NonNullable<Status["check"]["error"]>, t: (s: string) => string): string {
  switch (error.code) {
    case "timeout":
      return t("GitHub did not answer in time.");
    case "unreachable":
      return t("GitHub could not be reached. Check the server's internet access and DNS.");
    case "rate_limited":
      return t("GitHub's rate limit was reached. The next check will try again.");
    case "http":
      return fill(t("GitHub answered with an error (HTTP {status})."), { status: error.status ?? "?" });
    case "too_large":
      return t("GitHub's answer was too large to read.");
    case "malformed":
      return t("GitHub's answer could not be read.");
    case "only_previews":
      return t("GitHub's 20 newest releases are all drafts or prereleases: the newest stable release was not found.");
    default:
      return error.code;
  }
}

/** Seconds until `at` (0 once past), ticking every second while waiting. */
function useSecondsUntil(at: string | null): number {
  const [now, setNow] = useState(() => Date.now());
  const until = at ? new Date(at).getTime() : 0;
  const left = Math.max(0, Math.ceil((until - now) / 1000));
  useEffect(() => {
    if (left <= 0) return;
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
    // Restarted only when waiting starts or ends, not every second.
  }, [left > 0]);
  return left;
}

function VersionPage(props: PageProps<unknown>) {
  const t = props.t;
  const status = useAdminQuery<Status>(props, PATH);
  const queries = useQueryClient();
  const [checking, setChecking] = useState(false);
  const s = status.data;
  const wait = useSecondsUntil(s?.check.next_manual_at ?? null);

  const checkNow = async () => {
    setChecking(true);
    try {
      const out = await props.api.post<Status>(`${PATH}/check`, {});
      queries.setQueryData(adminKey(props, PATH), out);
      const error = out.check.error;
      if (error && error.at && out.check.attempted_at && error.at >= out.check.attempted_at) {
        props.toast.error(`${t("The check failed")}: ${failure(error, t)}`);
      } else if (out.state === "update" && out.update) {
        props.toast.success(fill(t("RBlog {version} is out"), { version: out.update }));
      } else {
        props.toast.success(t("Checked"));
      }
    } catch (error) {
      if (error instanceof ApiError && error.code === "session_changed") return;
      if (error instanceof ApiError && error.status === 429) {
        props.toast.error(t("Checked less than a minute ago. Try again shortly."));
        void status.refetch();
      } else {
        props.toast.error(error instanceof ApiError && error.message ? error.message : t("Something went wrong. Try again."));
      }
    } finally {
      setChecking(false);
    }
  };

  const checkButton = (
    <Button disabled={checking || !s || wait > 0} onClick={() => void checkNow()}>
      <Icon name="refresh" />
      {checking ? t("Checking…") : t("Check now")}
    </Button>
  );

  let body;
  if (status.error) {
    body = <ErrorState error={status.error} />;
  } else if (!s) {
    body = (
      <Stack>
        {[0, 1, 2].map((i) => (
          <Section key={i} title={<Skeleton className="version-skeleton-title" />}>
            <Skeleton className="version-skeleton-line" />
            <Skeleton className="version-skeleton-line" />
          </Section>
        ))}
      </Stack>
    );
  } else {
    body = (
      <Stack>
        {s.newer_migrations.length > 0 && (
          <Alert className="rblog-alert rblog-alert-warning">
            <Icon name="warning" />
            <p>
              {fill(t("A newer version of RBlog migrated this database ({names}). This version runs on it as it is: update again, or restore the backup made before the update."), {
                names: s.newer_migrations.join(", "),
              })}
            </p>
          </Alert>
        )}
        <Running {...props} status={s} />
        <Check {...props} status={s} wait={wait} />
        {s.state === "update" && s.release && <Notes {...props} release={s.release} />}
        <HowToUpdate {...props} status={s} />
      </Stack>
    );
  }

  return (
    <Page
      title={props.title}
      eyebrow={t("System")}
      heading={t("Version")}
      description={t("What RBlog runs, and whether a newer release is out. RBlog never updates itself: you update the containers.")}
      actions={checkButton}
    >
      {body}
    </Page>
  );
}

/** What runs: RBlog (version, channel, commit, image), RustCMS Core, MySQL, the system. */
function Running(props: PageProps<unknown> & { status: Status }) {
  const t = props.t;
  const { running, state } = props.status;
  const [tone, word] = versionStatus(state);
  const commit = running.commit;
  return (
    <Section title={t("Running")}>
      <Facts
        items={[
          {
            label: t("RBlog"),
            value: (
              <span className="version-inline">
                <span className="rblog-mono">{running.version}</span>
                <StatusBadge tone={tone} label={t(word)} />
              </span>
            ),
          },
          { label: t("Build"), value: t(CHANNELS[running.channel] ?? running.channel) },
          {
            label: t("Commit"),
            value: commit ? (
              <a className="rblog-link rblog-mono" href={`${props.status.repository}/commit/${commit}`} target="_blank" rel="noreferrer" title={commit}>
                {commit.slice(0, 12)}
                <Icon name="externalLink" size={12} />
              </a>
            ) : (
              <span className="rblog-muted">{t("Unknown")}</span>
            ),
          },
          {
            label: t("Image"),
            value: running.channel === "release" ? `${props.status.image}:${running.version}` : <span className="rblog-muted">{t("Not a release image")}</span>,
            mono: running.channel === "release",
          },
          { label: t("RustCMS Core"), value: running.core || "–", mono: true },
          { label: t("MySQL"), value: running.mysql ?? "–", mono: true },
          { label: t("System"), value: `${running.os} (${running.arch})`, mono: true },
          {
            label: t("Started"),
            value: <span title={dateTime(running.started_at, props.locale)}>{relative(running.started_at, props.locale)}</span>,
          },
        ]}
      />
    </Section>
  );
}

/** The last check: when, what it found, why it failed. */
function Check(props: PageProps<unknown> & { status: Status; wait: number }) {
  const t = props.t;
  const { check, state, release, running } = props.status;
  const checked = check.checked_at;
  let result;
  switch (state) {
    case "update":
      result = fill(t("RBlog {version} is available"), { version: props.status.update ?? "" });
      break;
    case "current":
      result = t("This is the newest release");
      break;
    case "no_release":
      result = t("No release of RBlog is published yet.");
      break;
    case "not_checked":
      result = t("Not checked yet. Check now, or wait for the next automatic check.");
      break;
    case "not_compared":
      result = release ? fill(t("The newest release is {version}."), { version: release.version }) : t("No release is known yet.");
      break;
  }
  return (
    <Section
      title={t("Release check")}
      description={fill(t("RBlog asks GitHub for new releases every {hours} hours, and sends nothing about this server."), { hours: check.every_hours })}
    >
      <p className="version-result">{result}</p>
      {state === "not_compared" && (
        <Alert className="rblog-alert rblog-alert-info">
          <Icon name="info" />
          <p>
            {running.channel === "development"
              ? t("This is a development build: it is not compared with releases. Install a release image to be told about updates.")
              : t("This build was made from source: it is not compared with releases. Install a release image to be told about updates.")}
          </p>
        </Alert>
      )}
      {check.error && (
        <Alert className="rblog-alert rblog-alert-warning">
          <Icon name="warning" />
          <p>
            {`${t("The last check failed")}${check.error.at ? ` (${relative(check.error.at, props.locale)})` : ""}: ${failure(check.error, t)}`}
            {checked && ` ${fill(t("What is shown is from {when}."), { when: dateTime(checked, props.locale) })}`}
          </p>
        </Alert>
      )}
      <Facts
        items={[
          {
            label: t("Last checked"),
            value: checked ? <span title={dateTime(checked, props.locale)}>{relative(checked, props.locale)}</span> : <span className="rblog-muted">{t("Never")}</span>,
          },
          {
            label: t("Newest release"),
            value: release ? (
              release.url ? (
                <a className="rblog-link rblog-mono" href={release.url} target="_blank" rel="noreferrer">
                  {release.version}
                  <Icon name="externalLink" size={12} />
                </a>
              ) : (
                <span className="rblog-mono">{release.version}</span>
              )
            ) : (
              <span className="rblog-muted">{t("Unknown")}</span>
            ),
          },
        ]}
      />
      {props.wait > 0 && <p className="rblog-small rblog-muted">{fill(t("You can check again in {seconds} s."), { seconds: props.wait })}</p>}
    </Section>
  );
}

/** The newer release's notes: text only (never HTML), line breaks kept. */
function Notes(props: PageProps<unknown> & { release: NonNullable<Status["release"]> }) {
  const t = props.t;
  const { release } = props;
  return (
    <Section
      title={fill(t("What's new in {version}"), { version: release.version })}
      description={release.published_at ? fill(t("Published {date}."), { date: dateTime(release.published_at, props.locale) }) : undefined}
      actions={
        release.url && (
          <a className="rblog-link rblog-small" href={release.url} target="_blank" rel="noreferrer">
            {t("View on GitHub")}
            <Icon name="externalLink" size={12} />
          </a>
        )
      }
    >
      {release.notes ? <div className="version-notes">{release.notes}</div> : <EmptyState title={t("This release has no notes.")} />}
    </Section>
  );
}

/** Updating with docker compose (docs/deployment.md), for the newer release when there is one. */
function HowToUpdate(props: PageProps<unknown> & { status: Status }) {
  const t = props.t;
  const { status } = props;
  // A release build moves only to a newer release (never to an older one a stale check kept);
  // a development or source build to the newest release.
  const target = status.update ?? (status.running.channel === "release" ? status.running.version : (status.release?.version ?? status.running.version));
  const previous = status.running.channel === "release" ? status.running.version : "X.Y.Z";
  const backup = `docker compose exec -T mysql sh -c 'exec mysqldump --single-transaction --routines --triggers -uroot -p"$MYSQL_ROOT_PASSWORD" "$MYSQL_DATABASE"' > rblog-$(date +%Y%m%d-%H%M%S).sql`;
  const restore = [
    "docker compose stop rblog",
    `docker compose exec -T mysql sh -c 'exec mysql -uroot -p"$MYSQL_ROOT_PASSWORD" "$MYSQL_DATABASE"' < rblog-….sql`,
    "docker compose up -d",
  ].join("\n");
  return (
    <Section
      title={t("How to update")}
      description={t("In the directory that holds compose.yml, .env and rblog.env on the server (docs/deployment.md).")}
    >
      <ol className="version-steps">
        <li>
          <p className="version-step-title">{t("Back up the database")}</p>
          <CodeBlock className="version-code" label={t("Database backup")} code={backup} />
          <p className="rblog-small rblog-muted">{t("Keep the file until the new version runs well. With the local disk, back up the storage volume too.")}</p>
        </li>
        <li>
          <p className="version-step-title">{t("Choose the version")}</p>
          <p className="rblog-muted">{fill(t("Set it in .env (or leave RBLOG_VERSION out to follow latest, {image}:latest)."), { image: status.image })}</p>
          <CodeBlock className="version-code" label=".env" code={`RBLOG_VERSION=${target}`} />
        </li>
        <li>
          <p className="version-step-title">{t("Pull the image and restart")}</p>
          <CodeBlock className="version-code" label={t("Update")} code="docker compose pull && docker compose up -d" />
          <p className="rblog-small rblog-muted">
            {t("The container runs the database migrations when it starts, then serves. Check /health and sign in again.")}
          </p>
        </li>
        <li>
          <p className="version-step-title">{t("Roll back if needed")}</p>
          <p className="rblog-muted">
            {fill(t("Put the previous version back in .env (RBLOG_VERSION={previous}) and run the same command. If the newer version migrated the database, the older one refuses to start: set RUSTCMS_DATABASE__ALLOW_NEWER_MIGRATIONS=true in rblog.env only when the release notes say its migrations only add; otherwise restore the backup."), { previous })}
          </p>
          <CodeBlock className="version-code" label={t("Restore the backup (RBlog stopped meanwhile)")} code={restore} />
        </li>
      </ol>
      <p className="rblog-small rblog-muted">
        <a className="rblog-link" href={DEPLOYMENT} target="_blank" rel="noreferrer">
          {t("Deployment guide")}
          <Icon name="externalLink" size={12} />
        </a>
      </p>
    </Section>
  );
}

registerPage("rblog/version", VersionPage);
