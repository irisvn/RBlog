# Changelog

## [Unreleased]

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
  binaries and a Docker image from a `vX.Y.Z` tag. RustCMS-Core v0.9.0.
