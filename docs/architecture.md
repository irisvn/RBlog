# Architecture

RBlog is the admin back office of a blog and its read-only Content API: one Rust binary on
RustCMS-Core (v0.10.3) and MySQL 8.4. The public site is a separate Nuxt/Next app
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
| `src/policies/` | `content.rs`: the permission matrix for posts and pages, used by `content_policy.rs` (posts, pages), `revision_policy.rs` (their revisions, through the parent), publishing and revisions; `post_author_policy.rs` and `user_lookup_policy.rs` (the author and user lookups) |
| `src/admin/` | `resources/` (posts, pages and their revisions, categories, tags, menus and menu items, author profiles, and the read-only post-author and user lookups), `pages/` (General, Reading, SEO and Integrations settings, Content API), `slots/` (preview), `dashboard.rs` |
| `src/settings/` | General, Reading, SEO, Integrations; `rules.rs` checks URLs, paths and slugs |
| `src/jobs/` | `publish_due`, `deliver_content_event`, `redispatch_content_events` |
| `src/console/` | `media:move-disk`, `content:sync-public-media` |
| `src/database/` | migrations; seeders `DatabaseSeeder` (runs `ContentApiSeeder` and `RoleSeeder`) and `DemoSeeder` (`--class demo`, with the cover images of `demo_images.rs`) |
| `resources/admin/` | React pages (Content API keys, preview slot, status and option-label cells) and the shared components in `src/rblog/lib/` |
| `tests/feature/` | one test binary on MySQL (`db_test!`) |

## Services

| Service | Responsibility |
|---|---|
| `publishing` | the only place that changes `status` and `published_at`: submit, publish, schedule, unpublish, `publish_due`. Locks the row, checks the transition and the caller's rights. `Transition::applies` is the one rule of which states a transition leaves: the post and page resources' workflow actions use it in `visible_when` (a row offers only what applies; a run on one record it does not apply to is a 403), and the policy decides only who |
| `content` | the write path every post/page write goes through (hooks, actions, publishing, revision restore, scheduler) |
| `content_sync` | the outbox writer (`record_event`), event payloads, `public_path` (the frontend path of a subject) |
| `events` | dispatches the webhook job in the event's transaction; the poller re-dispatches lost events |
| `webhook` | signing, SSRF-safe delivery, retryable vs permanent errors |
| `revisions` | one revision per editorial change (content hash), 30 kept, restore with permission and reference checks |
| `public_media` | which media the public may download, per subject (images anywhere in a body, callouts included, through `rich_text::media_ids`; the logo and default share image, subject `settings`, written by the settings pages' `after_save`); `sync`, `sync_settings` and `backfill` |
| `preview` | preview tokens (hashed, one hour, revocable) and the frontend preview link, issued by `issue_link_in` for both the record page's preview card (`routes::admin`) and the Preview action |
| `search` | the accent-free normalizer, plain text of a rich-text document (version 1 or 2: table cells, callouts and image captions included, embeds not), the LIKE pattern for `q` |
| `slug`, `taxonomy`, `authors`, `menus` | slugs and slug history; category, tag, author and menu rules |
| `content_api` | the frontend system user, its `content-reader` role, issuing and revoking Content API keys |
| `site` | the public settings the Content API returns (logo and share image as public images; the old `og_image_url` as a fallback) |
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
  └─ queue worker: deliver_content_event          signed POST to the frontend, up to 8 attempts
  └─ every minute: redispatch_content_events      picks up events whose job was lost
  └─ every minute: publish_due                    scheduled posts that are due → published
```

Everything up to the commit happens in one transaction: if any step fails, nothing is written
and no event is sent. Delivery is at least once; the frontend deduplicates on the event id.

## Rich text

Bodies are RustCMS rich-text documents, stored as JSON. RustCMS 0.10 added version 2 (tables,
YouTube/Vimeo embeds, callouts, image captions and widths); the editor writes the lowest version
a body needs, so version 1 and 2 bodies live side by side and nothing is migrated. The Content API
renders `html` with `rich_text::to_html_with` (images as figures with their media row's width and
height, public images only, or the preview's own image URLs) and `outline` with
`rich_text::outline`; heading ids are RustCMS's bare slugs, with no prefix
(`src/http/resources/content/entry.rs`). `?include=document` returns the stored document as is.
A RustCMS older than 0.10 refuses version 2 bodies: do not roll the binary back below 0.10 once
such a body is saved.

## Rules that must keep holding

- Deny by default: every API and admin route declares its access; field rules hold on the server;
  `status`, `published_at` and `author_id` are guarded.
- "Public" means `status = published`, `published_at <= now`, not trashed (`post::public_at`).
  The Content API, sitemap, menus, public media and dashboard counts all use it.
- Secrets (webhook secret, API keys, preview tokens) never reach logs, the audit log, job
  payloads or responses (except the one-time key display, and a preview link to the user who
  asked for it). The Preview action's link is part of its outcome, which RustCMS keeps in the
  action's idempotency receipt (shown again only to that user, on a retry): the token there
  is a one-hour, single-post read, and expires with the link.
- Settings saves that change what the public site shows (General, Reading, SEO) record
  `settings.updated` in their `after_save`, in the save's transaction.
- MySQL 8.4 only, `utf8mb4_0900_bin`; migrations only add and re-run safely (CLAUDE.md §1).

## Roles

| | Administrator | Editor | Author | Contributor |
|---|---|---|---|---|
| Posts: see all / edit others' | ✓ / ✓ | ✓ / ✓ | own | own |
| Write and tag | ✓ | ✓ | own | own, draft or pending |
| Publish, schedule, unpublish | ✓ | ✓ | own | – |
| Trash, restore | ✓ | ✓ | own | own drafts |
| Delete for good (`posts.force_delete`) | ✓ | ✓ | own | – |
| Revisions: see, restore | ✓ | ✓ | own posts' | own posts' (restore while draft or pending) |
| Pages and their revisions, categories, tags, menus | ✓ | ✓ | – | – |
| Author profiles | ✓ | view | – | – |
| Upload media / see all media | ✓ / ✓ | ✓ / ✓ | ✓ / ✓ | – / ✓ |
| Settings, Content API keys, users, roles | ✓ | – | – | – |

`rblog db:seed` creates them (`RoleSeeder`, through `RbacService`): the Administrator gets every
admin permission declared at that moment, the others the lists of
`src/database/seeders/role_seeder.rs`, which are also the Roles page's presets. An existing role
is never changed. Permissions declared after a role was seeded must be granted on the Roles page.

## Tests and CI

`scripts/check.sh` runs fmt, clippy and every test on the in-memory MySQL of docker-compose
(`mysql-test`, port 3307). CI runs lint and tests in parallel, and checks the admin bundle when
`resources/admin` changes. Releases build Linux binaries and an image from a `vX.Y.Z` tag.
