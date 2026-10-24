# Architecture

RBlog is the admin back office of a blog and its read-only Content API: one Rust binary on
RustCMS-Core (v0.9.0) and MySQL 8.4. The public site is a separate Nuxt/Next app
(docs/frontend-integration.html, docs/api.md). This page says where things live and how a change
travels from the admin to the public site. The database is described in docs/db-design.html.

## Layout (RustCMS ADR 0017)

| Folder | Holds |
|---|---|
| `src/bootstrap/` | the app from its providers, in order (storage before the admin panel) |
| `src/providers/` | auth (permissions, presets, labels), admin panel (theme, resources, settings, pages, dashboard), app (jobs, schedules, seeders, commands) |
| `src/routes/` | `api.rs`: the Content API, public media and preview; `admin.rs`: panel JSON routes (content keys, preview links) |
| `src/http/` | `controllers/content/*` (the Content API), `resources/content/*` (its JSON shapes), `requests/` |
| `src/models/` | SeaORM entities; `post::public_at(now)` is the one definition of "public" |
| `src/services/` | business rules (below) |
| `src/policies/` | `content.rs`: the permission matrix for posts and pages, used by the policies, publishing and revisions |
| `src/admin/` | `resources/` (posts, pages, revisions, categories, tags, menus, authors), `pages/` (settings, Content API), `slots/` (preview), `dashboard.rs` |
| `src/settings/` | General, Reading, SEO, Integrations; `rules.rs` checks URLs and slugs |
| `src/jobs/` | `publish_due`, `deliver_content_event`, `redispatch_content_events` |
| `src/console/` | `media:move-disk`, `content:sync-public-media` |
| `src/database/` | migrations, `RoleSeeder`, `ContentApiSeeder`, `DemoSeeder` |
| `resources/admin/` | React pages (Content API keys, preview slot, status and option-label cells) and the shared components in `src/rblog/lib/` |
| `tests/feature/` | one test binary on MySQL (`db_test!`) |

## Services

| Service | Responsibility |
|---|---|
| `publishing` | the only place that changes `status` and `published_at`: submit, publish, schedule, unpublish, `publish_due`. Locks the row, checks the transition and the caller's rights |
| `content` | the write path every post/page write goes through (hooks, actions, publishing, revision restore, scheduler) |
| `content_sync` | the outbox writer (`record_event`), event payloads, `public_path` (the frontend path of a subject) |
| `events` | dispatches the webhook job in the event's transaction; the poller re-dispatches lost events |
| `webhook` | signing, SSRF-safe delivery, retryable vs permanent errors |
| `revisions` | one revision per editorial change (content hash), 30 kept, restore with permission and reference checks |
| `public_media` | which media the public may download, per subject; `sync` and `backfill` |
| `preview` | preview tokens (hashed, one hour, revocable) and the frontend preview link |
| `search` | the accent-free normalizer, plain text of a rich-text document, the LIKE pattern for `q` |
| `slug`, `taxonomy`, `authors`, `menus` | slugs and slug history; category, tag, author and menu rules |
| `content_api` | the frontend system user, its `content-reader` role, issuing and revoking Content API keys |
| `site` | the public settings the Content API returns |
| `media_disk` | moving the media library between disks |

## How a change reaches the public site

```
admin form / action / scheduler
  └─ policy (src/policies/content.rs)            refuses what the role may not do
  └─ RustCMS writes the record, then the hook     after_save / after_trash / after_restore /
                                                  after_force_delete / after_relation_change
       └─ services::content (same transaction)
            ├─ search_text, reading_minutes
            ├─ slug_history (on a slug change)
            ├─ revision (if the content hash changed)
            ├─ public_media::sync                 images public only while the post is public
            └─ content_sync::record_event         outbox row in content_events
                 └─ events::enqueue               webhook job, same transaction
commit
  └─ queue worker: deliver_content_event          signed POST to the frontend, retried 8 times
  └─ every minute: redispatch_content_events      picks up events whose job was lost
  └─ every minute: publish_due                    scheduled posts that are due → published
```

Everything up to the commit happens in one transaction: if any step fails, nothing is written
and no event is sent. Delivery is at least once; the frontend deduplicates on the event id.

## Rules that must keep holding

- Deny by default: every API and admin route declares its access; field rules hold on the server;
  `status`, `published_at` and `author_id` are guarded.
- "Public" means `status = published`, `published_at <= now`, not trashed (`post::public_at`).
  The Content API, sitemap, menus, public media and dashboard counts all use it.
- Secrets (webhook secret, API keys, preview tokens) never reach logs, the audit log, job
  payloads or responses (except the one-time key display).
- MySQL 8.4 only, `utf8mb4_0900_bin`; migrations only add and re-run safely (CLAUDE.md §1).

## Roles

| | Administrator | Editor | Author | Contributor |
|---|---|---|---|---|
| Posts: see all / edit others' | ✓ / ✓ | ✓ / ✓ | own | own |
| Write and tag | ✓ | ✓ | own | own, draft or pending |
| Publish, schedule, unpublish | ✓ | ✓ | own | – |
| Trash, restore | ✓ | ✓ | own | own drafts |
| Pages, categories, tags, menus | ✓ | ✓ | – | – |
| Author profiles | ✓ | view | – | – |
| Upload media / see all media | ✓ / ✓ | ✓ / ✓ | ✓ / ✓ | – / ✓ |
| Settings, Content API keys, users | ✓ | – | – | – |

`rblog db:seed` creates them (`RoleSeeder`); an existing role is never changed. Permissions
declared after a role was seeded must be granted on the Roles page.

## Tests and CI

`scripts/check.sh` runs fmt, clippy and every test on the in-memory MySQL of docker-compose
(`mysql-test`, port 3307). CI runs lint and tests in parallel, and checks the admin bundle when
`resources/admin` changes. Releases build Linux binaries and an image from a `vX.Y.Z` tag.
