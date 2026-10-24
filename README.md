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
