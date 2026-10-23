# rblog

A [RustCMS](https://irisvn.github.io/RustCMS/) application. Commit `Cargo.lock`: it pins the RustCMS commit and every dependency.

```sh
docker compose up -d --wait   # MySQL 8.4 on localhost:3306 (database/user/password: rblog)
rustcms migrate          # create tables (MySQL 8.4 only, DATABASE_URL in .env)
rustcms db:seed          # run src/database/seeders (DatabaseSeeder)
rustcms serve            # http://localhost:8000/health, the admin panel at /admin
cargo test               # on MySQL only (.cargo/config.toml); fails if MySQL is not running
```

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
| `docs/` | `ui-guidelines.md`: the admin design system | |
