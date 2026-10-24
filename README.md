# rblog

A [RustCMS](https://irisvn.github.io/RustCMS/) application. Commit `Cargo.lock`: it pins the RustCMS commit and every dependency.

```sh
docker compose up -d --wait   # MySQL 8.4: dev data on 3306 (database/user/password: rblog), tests on 3307 (in memory)
rustcms migrate          # create tables (MySQL 8.4 only, DATABASE_URL in .env)
rustcms db:seed          # run src/database/seeders (DatabaseSeeder)
rustcms serve            # http://localhost:8000/health, the admin panel at /admin
scripts/check.sh         # the gate: fmt, clippy, every test (MySQL only, cargo-nextest)
scripts/check.sh fast 'test(post)'   # while coding: only the matching tests
```

Releases: push a tag `vX.Y.Z` (matching `Cargo.toml`) on a commit whose CI is green. It builds the
Linux binaries (x86_64, arm64) and the image `ghcr.io/irisvn/rblog` (`docker/Dockerfile`).

## Demo data

```sh
rustcms db:seed --class demo   # Vietnamese demo content (src/database/seeders/demo_seeder.rs)
```

`rustcms db:seed` does not run it. It refuses when `APP_ENV=production` (even with `--force`), and
running it again adds nothing: every row is found by its email, slug, menu key or menu and label
first. It creates:

- 3 authors, `thu.ha@`, `minh.quan@` and `hoang.yen@demo.rblog.local`, with author profiles. Their
  password is random and shown nowhere, and they have no role: a super admin sets a password and
  roles on the Users page to sign in as one;
- 4 categories (Du lịch, Ẩm thực, Công nghệ, Đời sống) and 8 tags;
- 20 posts with a rich-text body: 9 published at different past dates, 3 scheduled, 3 pending
  review, 3 drafts and 2 in the trash;
- 2 pages (Giới thiệu, Liên hệ), and the `primary` and `footer` menus linking categories and pages.

Its changes are recorded in the audit log and the outbox (`docs/api.md`, "Webhook"): authors,
categories, tags, menus, posts and pages. Posts and pages go through `content_sync::content_changed`
like every other write: search text, reading time, `public_media` rows, a first revision and the
`post.created` / `page.created` event.

## Roles and publishing

`rustcms db:seed` creates four roles (`src/database/seeders/role_seeder.rs`, through `RbacService`;
an existing role is left as it is), and the Roles page offers the last three as presets:

| | Administrator | Editor | Author | Contributor |
|---|---|---|---|---|
| See every post (`posts.view_all`), edit others' (`posts.edit_others`) | ✓ | ✓ | own only | own only |
| Write and tag posts | ✓ | ✓ | own | own, while draft or pending |
| Publish, schedule, unpublish (`posts.publish`, `posts.unpublish`) | ✓ | ✓ | own | – |
| Trash and restore | ✓ | ✓ | own | own drafts |
| Pages, categories, tags, menus | ✓ | ✓ | – | – |
| Upload media / see every file (`media.view_all`) | ✓ / ✓ | ✓ / ✓ | ✓ / ✓ | – / ✓ |
| Settings, integrations, users, roles | ✓ | – | – | – |

The rules on a record live in `src/policies/content.rs`. A post's `status`, `published_at` and
`author_id` are set by the server only: the workflow actions (Submit for review, Publish, Schedule…,
Unpublish) go through `src/services/publishing.rs`, and the `publish-due` schedule (every minute)
publishes every scheduled post and page that is due, catching up after downtime. Every write ends
in `src/services/content_sync.rs`; revisions (`src/services/revisions.rs`, 30 per post or page)
are listed and restored on the post's page.

### End-to-end smoke test

`scripts/e2e-smoke.mjs` drives the whole publishing path in the installed Google Chrome
(playwright-core, nothing is downloaded), against a running app with the seeded roles and the demo
data. It is not run in CI; run it before a release or after a change to publishing, the Content API,
webhooks or preview:

```sh
(cd resources/admin && npm ci)
RUSTCMS_WEBHOOK__ALLOW_INSECURE_LOCAL=true rustcms serve      # only for this run: http://localhost receivers
RBLOG_E2E_ADMIN_EMAIL=you@example.com RBLOG_E2E_ADMIN_PASSWORD_FILE=~/.rblog-admin node scripts/e2e-smoke.mjs
```

As a super admin it makes an Editor and a Contributor (`e2e-editor@` and `e2e-contributor@example.com`,
through the Users API, so `RbacService`), points General → Public site URL and Integrations → Webhook
at a small local server the script starts, and issues a Content API key. Then the Contributor writes
a draft, opens the preview link of the preview slot (the local "frontend" asks `/api/v1/preview` for
it) and submits the draft, without being offered Publish (the server refuses it too); the Editor
publishes it; the Content API returns it with the key; and the local receiver gets a `post.published`
webhook whose signature it checks like docs/frontend-integration.html. Each step prints `ok` or `FAIL`.
Afterwards, even after a failure, it revokes the key, trashes the post, puts the settings back and
deactivates both users.

- The webhook URL must be empty before the run (the script cannot put back a secret it never saw).
- Reusing the two users means setting their password, which needs a super admin of every guard
  (`admin` and `api`). With a super admin of the admin guard only, each run makes its own pair
  (`e2e-editor+<run>@example.com`), deactivated at the end.
- `RBLOG_E2E_BASE` (default `http://localhost:8000`) and `RBLOG_E2E_PORT` (the local server's port,
  default a free one) are optional.

### Test and CI times

| | Before | After |
|---|---|---|
| Local tests (warm build) | 3.1 s (`cargo test`, 2.3 s of tests) | 2.9 s for the whole gate (fmt, clippy, tests); tests 0.4 s |
| CI (one push) | 173 s in one job (clippy 57 s, MySQL 20 s, tests 73 s; 9 tests) | about 65 s: lint 31 s and test 65 s in parallel (201 tests); admin bundle 11 s, only when `resources/admin` changes |

## Where things live

| Path | Holds | Generator |
|---|---|---|
| `src/bootstrap/` | `build()`: the app from its providers (`providers.rs` lists them) | |
| `src/providers/` | auth (permissions), admin panel (resources, pages), app (jobs, seeders, commands) | |
| `src/routes/api.rs` | every API route and its access | `make:api` |
| `src/http/` | `controllers/`, `requests/`, `resources/` (JSON) | `make:api` |
| `src/models/` | SeaORM entities and query scopes | `make:model` |
| `src/services/` | business rules shared by the panel, the API, jobs and seeders | `make:service` |
| `src/policies/` | who may do what | `make:policy` |
| `src/jobs/` | background jobs | `make:job` |
| `src/settings/` | settings stored in the database | `make:settings` |
| `src/console/` | console commands | `make:command` |
| `src/admin/` | `resources/` (one per model), `pages/` (settings pages), `slots/` (beside records), `dashboard.rs` | `make:resource`, `make:settings` |
| `src/database/` | `migrations/`, `seeders/`, `factories/` | `make:migration`, `make:seeder`, `make:factory` |
| `tests/feature/` | tests through HTTP, on MySQL 8.4 only | `make:test` |
| `config/`, `lang/`, `storage/` | configuration, translations, uploaded files | |
| `docs/` | `ui-guidelines.md` (the admin design system), `media.md` (disks, moving files), `api.md` (Content API) | |
