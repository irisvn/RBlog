# Changelog

## [Unreleased]

### Version page and production docker compose

- **Integrations → Version** (`/admin/p/version`, new permission `system.version`): what RBlog runs
  (version, commit, build channel, image, RustCMS Core, MySQL, OS and architecture, start time),
  the last release check (when, "up to date" or "RBlog X.Y.Z is available", why it failed), the
  newer release's notes as plain text, and how to update with docker compose (back up, set
  `RBLOG_VERSION`, pull and restart, roll back). RBlog never updates itself.
- Release checks ask GitHub's releases of `irisvn/RBlog` (fixed address, anonymous, a
  `rblog/<version>` user agent only, 10 s, 4 MiB at most) every 6 hours (schedule
  `release-check`) and on "Check now" (at most once a minute: 429 with `Retry-After`), and keep the
  newest non-draft, non-prerelease `vX.Y.Z` release in the new table `release_checks` (run
  `rblog migrate`; the migration only adds a table). `rblog version:check` checks from the
  command line. `config/releases.toml` (`[releases]`): `url` for tests only (refused in
  production), `timeout_secs` (1–20).
- Only release builds are compared: `build.rs` reads `RBLOG_VERSION` and `RBLOG_COMMIT`, which
  `.github/workflows/release.yml` now passes to `cargo build`. Local builds are `source`, CI builds
  `development`: both show the newest release but are never told to update.
- **Dashboard**: holders of `system.version` see the widget "New RBlog release" (empty while up to
  date). RustCMS 0.10.3 has no dashboard slot, so it is a widget over a read-only resource,
  `release_updates`; its own permissions (`release_updates.*`) grant nothing.
- **Existing installs**: `permission:sync` (the image's entrypoint runs it) adds `system.version`,
  but the Administrator role made earlier does not get it: tick it on the Roles page (super admins
  have it anyway).
- **Production**: `docker/compose.production.yml` (the release image and MySQL 8.4 with
  `utf8mb4_0900_bin`, volumes for MySQL and `storage/`, a `/health` healthcheck) and
  [docs/deployment.md](docs/deployment.md) (first install, environment, updating, backups,
  rollback).

### RustCMS 0.10.3: page headers on phones

- RustCMS-Core v0.10.3 (no migration, no API change).
- On a phone, a post or page header shows only its main action and a ⋯ menu; the other actions,
  then Edit, then Delete (last, after a separator) are in that menu. Wide screens keep every
  action inline. Titles keep their room: actions wrap below them.
- View pages show the slug with its prefix (`/posts/…`, `/…`).

### RustCMS 0.10.2: an editor layout for posts and pages

- RustCMS-Core v0.10.2 (no migration; admin vocabulary 11). RBlog already sets the theme's
  success and warning colors, so the new green and amber defaults change nothing here.
- **Post and page editors** use the whole width of the window, with a main column and an aside
  that stays in view from 1024 px (below, the aside follows the main column). The tabs Content,
  SEO and Publishing are gone.
  - Main column: title, the slug right under it (shown after `/posts/`, or `/` for pages), body,
    excerpt, and SEO in a section that starts folded.
  - Aside: Publishing (status and publish date read-only, author; the workflow actions stay in
    the page header), Cover image, and for posts Organize (category, tags, featured). A new
    record has no Publishing section yet: the server sets those fields.
  - The view page lays the record out the same way.
- `scripts/ui-shots.mjs`: shots `post-edit-layout`, `post-view-layout`, `post-edit-seo-open` and
  `page-edit-layout` replace `post-edit-seo` and `post-edit-publishing`.

### RustCMS 0.10.1: row actions, preview action, media settings, alt texts

- RustCMS-Core v0.10.1 (run `rustcms migrate`: it adds `media.alt`; a 0.10.0 binary then needs
  `migrate --allow-newer`).
- **Row actions**: each post and page row offers only the transitions that apply to its status
  (`services::publishing::Transition::applies`): Publish is the row's labelled action; Submit for
  review, Schedule…, Preview and Unpublish (last, after a separator) are in its ⋯ menu, and so on
  record pages. A transition run on one record it does not apply to is now refused with 403
  (before: 200 with the record in `denied`); a selection skips those records (`denied`).
  Rescheduling a scheduled post stays possible (Schedule… is offered on drafts, pending and
  scheduled posts). The Schedule dialog's button reads "Hẹn giờ".
- **Preview action** on post and page rows and pages: opens the public site's preview in a new
  tab (a fresh one-hour link). Without a public site URL, or with an `http://localhost` one, it
  says so and makes no link. The record page's preview card stays, for the link to copy and its
  expiry; both issue links through `services::preview::issue_link_in`.
- **Revisions**: "Restore this version" is the labelled action of each row of a post's or page's
  Revisions table.
- **Narrow screens**: list columns other than the title (or name) and the status hide below 768
  or 640 px (`hide_below`).
- **Media settings**: Settings → General has a **Logo** (PNG, WebP or JPEG, 1 MB) and SEO a
  **Default share image** (an image, 5 MB), both from the media library. The old share image URL
  is kept, shown as "Old share image URL", and used only while no share image is chosen. Both
  images are public through `public_media` (subject `settings`, rebuilt by
  `content:sync-public-media`).
- **Content API (breaking)**: `GET /settings` returns `logo` and `seo.og_image` as
  `{url, width, height, alt}` or `null`; `seo.og_image_url` is gone (its value, while no image is
  chosen, is `seo.og_image.url`).
- **Alt texts**: `cover.alt`, avatars and images in `html` use the document's alternative text,
  then the media library's, then `""`; never the file name any more.
- **Outbox**: saving General, Reading or SEO settings records `settings.updated` (tag
  `settings`); Integrations records nothing.
- The demo seeder gives its first five posts covers, small images drawn by the seeder with
  Vietnamese alternative texts.
- The dashboard's resource cards follow registration order since RustCMS 0.10.1: posts and pages
  are registered first.

### RustCMS 0.10 and rich text version 2

- RustCMS-Core v0.10.0. The post and page editor gains tables, YouTube and Vimeo embeds, info and
  warning callouts, image captions and widths, several images uploaded at once, and a media
  library picker (also for covers). Version 1 bodies stay as they are.
- Content API: `html` writes images as `<figure>` with `width`, `height` and `<figcaption>`,
  headings with ids, embeds as a link in `<figure data-embed data-id>`, tables and callouts;
  single posts, pages and previews add `outline` (`[{level, text, id}]`); `?include=document`
  returns the stored document, version 1 or 2 (docs/api.md, "Rich text").
- Search and reading time count table cells, callouts and image captions; images nested in
  callouts are public exactly like the others.
- Once a version 2 body is saved, do not roll the binary back below RustCMS 0.10: older versions
  refuse those bodies.

### The first version (RustCMS 0.9.0)

The first complete version of RBlog: the admin back office of a blog and its read-only Content API
for a separate Nuxt/Next site.

- **Content**: posts and pages with a rich-text editor, cover and share images, categories, tags
  (chosen in the post form), SEO fields, Vietnamese slugs generated from the title and old slugs
  kept for redirects.
- **Publishing**: draft → pending review → scheduled → published, unpublish, bulk publish;
  scheduled posts go live every minute, catching up after downtime; preview links for drafts.
- **Revisions**: each editorial change is kept (30 per post or page) and can be restored.
- **Roles**: Administrator, Editor, Author and Contributor, with ownership and status rules.
- **Menus** (primary, footer), **author profiles**, **dashboard** counts and charts.
- **Settings** for the public site: general, reading (homepage), SEO and social links,
  integrations (webhook).
- **Content API** (`/api/v1/content`): posts, pages, categories, tags, authors, menus, settings,
  sitemap; accent-insensitive search; public images only for published content; Content API keys
  managed on the Integrations page.
- **Webhook** to the frontend on every change, signed, delivered at least once, with cache tags
  and old and new paths.
- **Media** on a local disk or S3 (AWS, R2, MinIO), and `media:move-disk` to switch an existing
  site.
- **Operations**: MySQL 8.4 only; tests on an in-memory MySQL; CI in parallel; releases build Linux
  binaries and a Docker image from a `vX.Y.Z` tag. Started on RustCMS-Core v0.8.2 and built on
  v0.9.0 (trash, restore, relation and settings hooks, date-time action fields, `media.view_all`,
  several tags chosen in the post form); the sections above upgrade it to v0.10.2.
- **Checks**: `scripts/ui-shots.mjs` screenshots every admin screen (1440, 768 and 390 px, light
  and dark, English and Vietnamese); `scripts/e2e-smoke.mjs` runs the publishing path end to end
  (roles, preview, Content API, signed webhook).
