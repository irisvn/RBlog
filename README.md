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
| `src/admin/` | `resources/` (one per model), `pages/` (settings pages) | `make:resource`, `make:settings` |
| `src/database/` | `migrations/`, `seeders/`, `factories/` | `make:migration`, `make:seeder`, `make:factory` |
| `tests/feature/` | tests through HTTP, on MySQL 8.4 only | `make:test` |
| `config/`, `lang/`, `storage/` | configuration, translations, uploaded files | |
| `docs/` | `ui-guidelines.md` (the admin design system), `media.md` (disks, moving files), `api.md` (Content API) | |
