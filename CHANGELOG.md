# Changelog

## [Unreleased]

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
  binaries and a Docker image from a `vX.Y.Z` tag. RustCMS-Core v0.10.0.
- **Checks**: `scripts/ui-shots.mjs` screenshots every admin screen (1440, 768 and 390 px, light
  and dark, English and Vietnamese); `scripts/e2e-smoke.mjs` runs the publishing path end to end
  (roles, preview, Content API, signed webhook).
