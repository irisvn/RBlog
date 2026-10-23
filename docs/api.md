# Content API

> **Status: planned (phase P3).** This page is the contract the Nuxt/Next frontend is built
> against; it is completed with the implementation, in the same change.

RBlog is the admin and the content source; the public site is a separate Nuxt or Next app that
reads RBlog's **read-only Content API** from its server side (SSR, ISR) and is told about
changes by a **webhook**.

- Base path: `/api/v1/content`. JSON, RustCMS wire format: ids and 64-bit numbers as strings,
  instants in RFC 3339 UTC (`2026-10-24T08:00:00Z`).
- Authentication: a Content API key (`Authorization: Bearer rcms_…`), created on the admin's
  Integrations page. It only reads published content. Keep it on the server: never ship it to
  browsers (there is no CORS on purpose).
- Lists: `?page` (from 1) and `?per_page` (default 20, at most 100), answered as
  `{ "data": [...], "meta": { "page", "perPage", "total", "lastPage" } }`, newest first
  (`published_at`, then `id`).
- Errors: RustCMS's envelope `{ "message", "code", "errors" }` with 401, 403, 404, 422, 429.

## Endpoints

| Method and path | What |
|---|---|
| `GET /posts` | published posts; filters `category`, `tag`, `author` (slugs), `featured`, `q` (accent-insensitive search) |
| `GET /posts/{slug}` | one post; an old slug answers `{ "redirect": "<new slug>" }` |
| `GET /pages`, `GET /pages/{slug}` | static pages |
| `GET /categories`, `GET /categories/{slug}` | with their number of published posts |
| `GET /tags`, `GET /tags/{slug}` | with their number of published posts |
| `GET /authors/{slug}` | an author's public profile |
| `GET /menus/{key}` | `primary`, `footer`: items with resolved links (only to published content) |
| `GET /settings` | the public site settings (name, description, language, SEO defaults, social links) |
| `GET /sitemap?type=post&after=<cursor>` | `{type, path, lastmod}` in batches of 1000, for the frontend's sitemap |
| `GET /api/v1/preview/{token}` | a draft, for the frontend's preview route (short-lived token, `no-store`) |
| `GET /api/v1/media/{id}/{name}` | a public image (only images of published content), no key needed |

## A post

```json
{
  "id": "42",
  "slug": "xin-chao-viet-nam",
  "title": "Xin chào Việt Nam",
  "excerpt": "…",
  "html": "<p>…</p>",
  "cover": { "url": "https://cms.example.com/api/v1/media/…/cover.jpg", "width": 1600, "height": 900, "alt": "…" },
  "category": { "slug": "du-lich", "name": "Du lịch" },
  "tags": [{ "slug": "ha-noi", "name": "Hà Nội" }],
  "author": { "slug": "an", "name": "An" },
  "published_at": "2026-10-24T08:00:00Z",
  "updated_at": "2026-10-24T09:12:00Z",
  "reading_minutes": 4,
  "seo": { "meta_title": null, "meta_description": null, "canonical_url": null, "og_image": null }
}
```

`?include=document` adds `document`, the rich-text JSON, for frontends that render it with their
own components instead of `html`.

## Webhook

On every change that affects the public site, RBlog POSTs an event to the URL set in
Settings → Integrations, **at least once** (retried with backoff):

- headers `X-RBlog-Event-Id`, `X-RBlog-Timestamp`, and `X-RBlog-Signature` (hex HMAC-SHA256 of
  `timestamp + "." + event_id + "." + body` with the webhook secret). Check it in constant time,
  refuse old timestamps, and ignore an event id already handled;
  - `Content-Type: application/json`; the timestamp is unix time in seconds, and the signature
    is lowercase hex over the raw body bytes, so verify before parsing the JSON;
  - in Rust, `rblog::services::webhook::verify(secret, timestamp, event_id, body, signature)`
    is the reference check;
- the body lists the **cache tags** to revalidate (`post:{id}`, `posts`, `category:{slug}`,
  `tag:{slug}`, `menu:{key}`, `settings`, `sitemap`) and the old and new paths;
- the URL must be `https://` (no user name or password in it) and resolve only to public
  addresses: RBlog resolves it at every delivery and refuses loopback, private, link-local and
  other special ranges, IPv4 and IPv6;
- answer **2xx** within 10 seconds (5 to connect). Redirects are not followed and count as a
  failure. RBlog retries timeouts, connection errors, 408, 425, 429 and 5xx; any other answer
  (3xx, other 4xx) fails the delivery without retrying.

Each event is a row of RBlog's outbox (`content_events`), written in the same transaction as
the change (`src/services/content_sync.rs`). Its payload:

```json
{
  "tags": ["category:du-lich", "category:du-lich-cu", "posts", "sitemap"],
  "paths": { "old": ["/category/du-lich-cu"], "new": ["/category/du-lich"] }
}
```

- `tags`: what to revalidate, without duplicates. A renamed category or tag lists its new and
  its old slug.
- `paths.old`: paths that no longer show the content (an old slug, a deleted record);
  `paths.new`: the paths that show it now. Categories are `/category/{slug}` and tags
  `/tag/{slug}`; a menu has no path of its own (both lists empty).

| Event kind | When | Tags |
|---|---|---|
| `category.created`, `category.updated`, `category.deleted` | a category is written | `category:{slug}` (and the old slug on a rename), `posts`, `sitemap` |
| `tag.created`, `tag.updated`, `tag.deleted` | a tag is written | `tag:{slug}` (and the old slug on a rename), `posts`, `sitemap` |
| `menu.created`, `menu.updated`, `menu.deleted` | a menu or one of its items is written (an item change is `menu.updated`) | `menu:{key}` |

Keep a fallback time-based revalidation (for example 10 minutes) in case the webhook stays
unreachable.
