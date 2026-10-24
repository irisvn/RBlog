# Deployment

RBlog runs in production as two containers that you manage with docker compose: the release image
`ghcr.io/irisvn/rblog` and MySQL 8.4. The file is [`docker/compose.production.yml`](../docker/compose.production.yml).
RBlog never updates itself: its **Version** page (Integrations → Version) checks GitHub for a newer
release every 6 hours, says so (also on the dashboard, to administrators), and shows the commands
below.

## What you need

- A Linux server with Docker Engine and the compose plugin (`docker compose version`).
- A domain for the admin (`https://admin.example.com`) and a reverse proxy in front of RBlog that
  serves it over https (Caddy, nginx, Traefik). RBlog listens on `127.0.0.1:8000` only.
- Outgoing https to `api.github.com` for the release checks (optional: without it the Version page
  says the check failed, nothing else changes).

## First install

1. A directory for RBlog, with the compose file:

   ```sh
   sudo mkdir -p /opt/rblog && cd /opt/rblog
   sudo curl -fsSLo compose.yml https://raw.githubusercontent.com/irisvn/RBlog/main/docker/compose.production.yml
   ```

2. `.env`, read by docker compose itself (the version to run and the MySQL passwords). The
   passwords go into `DATABASE_URL`, so use URL-safe ones (hex):

   ```sh
   umask 077
   cat > .env <<EOF
   RBLOG_VERSION=0.1.0
   MYSQL_PASSWORD=$(openssl rand -hex 24)
   MYSQL_ROOT_PASSWORD=$(openssl rand -hex 24)
   EOF
   ```

   Without `RBLOG_VERSION` the image is `:latest` (the newest release). Pinning a version makes
   updates and rollbacks explicit.

3. `rblog.env`, RBlog's own settings, starting with its key:

   ```sh
   touch rblog.env
   echo "APP_KEY=$(docker compose run --rm --no-deps rblog key:generate --show)" >> rblog.env
   echo "APP_URL=https://admin.example.com" >> rblog.env
   ```

   Keep `APP_KEY` safe and never change it casually: it signs sessions and encrypts secrets
   (the webhook secret). `chmod 600 .env rblog.env`.

4. Start, then create the roles and the first administrator:

   ```sh
   docker compose up -d --wait          # MySQL, then RBlog (it migrates the database first): both healthy
   docker compose exec rblog /app/rblog db:seed --force
   docker compose exec rblog /app/rblog make:user --email you@example.com --name "You" --super-admin
   ```

   `make:user` prints a generated password. Sign in at `APP_URL/admin`.

### Environment

| Variable | Where | |
|---|---|---|
| `RBLOG_VERSION` | `.env` | the release to run (`0.1.0`); absent: `latest` |
| `MYSQL_PASSWORD`, `MYSQL_ROOT_PASSWORD` | `.env` | MySQL's passwords (hex: `DATABASE_URL` is built from the first) |
| `RBLOG_PORT` | `.env` | the local port of RBlog (8000) |
| `APP_KEY` | `rblog.env` | required; `key:generate --show` |
| `APP_URL` | `rblog.env` | required; the admin's https origin (sign-in and writes are only accepted from it) |
| `RUSTCMS_HTTP__TRUSTED_PROXIES` | `rblog.env` | your reverse proxy, so that client IPs (rate limits, audit log) are right: `'["172.16.0.0/12"]'` for a proxy on the host reaching Docker's bridge |
| `RUSTCMS_STORAGE__DISK` | `rblog.env` | `local` (default: the `rblog-storage` volume) or `s3` |
| `RUSTCMS_STORAGE__S3__BUCKET`, `…__REGION`, `…__ENDPOINT` | `rblog.env` | with `s3`: a private bucket (AWS S3, Cloudflare R2, MinIO) |
| `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY` | `rblog.env` | with `s3` |

`DATABASE_URL`, `APP_ENV=production` and `APP_DEBUG=false` are set by the compose file. Any setting
of `config/*.toml` can be given as `RUSTCMS_<SECTION>__<KEY>` (lists in TOML syntax). Media disks
and moving between them: [media.md](media.md).

## Updating

The Version page shows these steps with the new version filled in. In `/opt/rblog`:

1. **Back up the database** (and, with the local disk, the `rblog-storage` volume):

   ```sh
   docker compose exec -T mysql sh -c 'exec mysqldump --single-transaction --routines --triggers -uroot -p"$MYSQL_ROOT_PASSWORD" "$MYSQL_DATABASE"' > rblog-$(date +%Y%m%d-%H%M%S).sql
   ```

2. **Choose the version** in `.env`: `RBLOG_VERSION=X.Y.Z` (or leave it out to follow `latest`).
   Read its release notes first (the Version page shows them, and the CHANGELOG says when a
   release needs more than this).

3. **Pull and restart**:

   ```sh
   docker compose pull && docker compose up -d
   ```

   The new container runs the database migrations when it starts, then `permission:sync`, then
   serves. Check `docker compose ps` (healthy), `docker compose logs rblog`, and sign in.

New permissions of a release are added by `permission:sync`, but existing roles are not changed:
grant them on the Roles page when the CHANGELOG says so.

## Rolling back

Put the previous version back in `.env` (`RBLOG_VERSION=<previous>`) and run
`docker compose pull && docker compose up -d` again.

If the newer version migrated the database, the older one finds migrations it does not know and
refuses to start (`docker compose logs rblog`). Then:

- when the release notes (CHANGELOG) say its migrations only **add** (new tables, nullable
  columns), let the older version accept them: add `RUSTCMS_DATABASE__ALLOW_NEWER_MIGRATIONS=true`
  to `rblog.env` and start again. The Version page then lists those migrations; remove the line
  once you update again;
- otherwise restore the backup made before the update:

  ```sh
  docker compose stop rblog
  docker compose exec -T mysql sh -c 'exec mysql -uroot -p"$MYSQL_ROOT_PASSWORD" "$MYSQL_DATABASE"' < rblog-….sql
  docker compose up -d
  ```

## Backups

- **Database**: the `mysqldump` command above, from cron for example, kept off the server.
- **Media** on the local disk: the `rblog-storage` volume
  (`docker run --rm -v rblog_rblog-storage:/data -v "$PWD":/backup ubuntu tar czf /backup/storage.tgz -C /data .`).
  With S3, the bucket's own versioning or replication.
- **`.env` and `rblog.env`**: `APP_KEY` is needed to read encrypted settings back.

## Release checks

Every 6 hours (schedule `release-check`, on the Schedules page) and on "Check now" (at most once a
minute), RBlog asks `https://api.github.com/repos/irisvn/RBlog/releases` anonymously, with only a
`rblog/<version>` user agent: nothing about the server is sent. The address is fixed in the code.
Only a release image (`RBLOG_VERSION` built in by `.github/workflows/release.yml`) is compared with
the releases; a development or source build shows the newest release but is never told to update.
`docker compose exec rblog /app/rblog version:check` checks from the command line.
