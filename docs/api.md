# Content API

> **Status: implemented (phase P3), read side.** This page is the contract the Nuxt/Next frontend
> is built against.

RBlog is the admin and the content source; the public site is a separate Nuxt or Next app that
reads RBlog's **read-only Content API** from its server side (SSR, ISR) and is told about
changes by a **webhook**.

- Base path: `/api/v1/content`. JSON, RustCMS wire format: ids and 64-bit numbers as strings,
  instants in RFC 3339 UTC to the second (`2026-10-24T08:00:00Z`).
- Authentication: a Content API key (`Authorization: Bearer rcms_…`), created on the admin's
  Integrations → Content API page (`/admin/p/content-api`). It only reads published content. Keep
  it on the server: never ship it to browsers (there is no CORS on purpose).
  - On that page, `integrations.view` lists the keys (never their secret) and
    `integrations.manage` creates them (a name of at most 80 characters, no expiry or 30, 90 or 365
    days; the secret is shown once) and revokes them at once. Both are audited (`token.created`,
    `token.revoked`). The page calls `{admin}/api/x/integrations/content-keys` (`GET`, `POST`,
    `DELETE /{id}`, `src/routes/admin.rs`).
  - A key is an API token of the system user `frontend@rblog.local` (API-guard role
    `content-reader`, made by `rblog db:seed`), limited to the one permission `content.read`.
  - No key, or an invalid, expired or revoked one: **401**. A token without `content.read`
    (another user's, or one issued with other abilities): **403**.
- Published means `status = published`, `published_at` reached, not in the trash. Nothing else
  is ever answered, not even through an old slug.
- Lists: `?page` (from 1) and `?per_page` (default 20, at most 100), answered as
  `{ "data": [...], "meta": { "page", "perPage", "total", "lastPage" } }`. Posts and pages come
  newest first (`published_at`, then `id`, both descending: stable across pages); categories
  and tags by name.
- Query parameters a route reads may be given once (twice: 422 on that parameter); others are
  ignored.
- Paths: RBlog names each public page of the frontend the same way everywhere (menus, sitemap,
  webhook): `/posts/{slug}`, `/{slug}` for a page, `/categories/{slug}`, `/tags/{slug}`,
  `/authors/{slug}`. Map them to your routes if they differ.
- Errors: RustCMS's envelope `{ "message", "code", "errors" }` with 401, 403, 404, 422, 429.

## Endpoints

| Method and path | What |
|---|---|
| `GET /posts` | published posts; filters below |
| `GET /posts/{slug}` | one post, with `html`; an old slug answers `{ "redirect": "<new slug>" }` |
| `GET /pages`, `GET /pages/{slug}` | static pages: a post without `category`, `tags`, `featured`; old slugs redirect the same way |
| `GET /categories`, `GET /categories/{slug}` | `{id, slug, name, description, meta_title, meta_description, post_count}`; old slugs redirect |
| `GET /tags`, `GET /tags/{slug}` | `{id, slug, name, description, post_count}`; old slugs redirect |
| `GET /authors/{slug}` | `{slug, name, bio, website, avatar}`; 404 for an author without a published post |
| `GET /menus/{key}` | `primary`, `footer`: `{key, name, items}`, links resolved (only to published content) |
| `GET /settings` | the public site settings (name, description, language, frontend URL, posts per page, SEO defaults, social links, homepage) |
| `GET /sitemap?type=post&after=<cursor>` | `{type, path, lastmod}` in batches of 1000, for the frontend's sitemap |
| `GET /api/v1/preview?token=<token>` | a draft, for the frontend's preview route (short-lived token, `no-store`), no key needed |
| `GET /api/v1/preview/media/{id}?token=<token>` | an image of that draft |
| `GET /api/v1/media/{id}/{name}` | a public image (only images of published content), no key needed |

`post_count` counts published posts only. `post_count`, `reading_minutes`, `width` and `height`
are JSON numbers.

### Filters of `GET /posts`

- `category`, `tag`, `author`: slugs (an unknown slug gives an empty list). They combine.
- `featured`: `true` or `false` (also `1`, `0`); anything else is 422.
- `q`: accent- and case-insensitive search in the title, excerpt and body text (`viet` finds
  "Việt"); `%` and `_` are literal. At most 100 characters (422 beyond).

### Redirects

A slug that is not current but that a published post (page, category, tag) had before answers
**200** `{ "redirect": "<current slug>" }`: redirect (301) to the current path. An old slug of
content that is not published is a plain 404.

### Lists and single records

List items (`GET /posts`, `GET /pages`) carry every field of a single record except `html` and
`document`: fetch the record for its body.

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
  "featured": false,
  "published_at": "2026-10-24T08:00:00Z",
  "updated_at": "2026-10-24T09:12:00Z",
  "reading_minutes": 4,
  "seo": { "meta_title": null, "meta_description": null, "canonical_url": null, "og_image": null }
}
```

- `html` is the body rendered by RustCMS (every text escaped). An image that is not public
  (see below) is left out of it, and `cover` or `seo.og_image` is then `null`.
- Image URLs are absolute, on `APP_URL`. The last segment is the file name, cosmetic only.
- `cover.alt` is the media's file name until media get alt texts.
- `seo.og_image` is the URL of the OG image (a string), or `null`.
- `author.slug` is `null` when the author has no public profile; `category` is `null` without
  one; `tags` keep the order set in the admin.

`?include=document` adds `document`, the rich-text JSON, for frontends that render it with their
own components instead of `html` (any other `include` value is 422). Its images are media ids:
show those whose URL appears in `html` or `cover`.

## Menus

```json
{ "key": "primary", "name": "Primary", "items": [
  { "id": "1", "label": "Home", "href": "/", "new_tab": false, "children": [
    { "id": "2", "label": "Xin chào", "href": "/posts/xin-chao-viet-nam", "new_tab": false }
  ] },
  { "id": "3", "label": "GitHub", "href": "https://github.com/…", "new_tab": true, "children": [] }
] }
```

Items come in their admin order, one level deep. A link to a post, page, category or tag has its
path (see Paths); a URL link keeps its URL. Items whose target is not published (draft,
scheduled, trashed, deleted) are left out, with the items under them.

## Sitemap

`GET /sitemap?type=post|page|category|tag[&after=<cursor>]` answers
`{ "data": [{ "type", "path", "lastmod" }], "next": "<cursor>" | null }`: at most 1000 entries,
then `next` until the last batch. Pass `next` back as `after`; treat it as opaque. Posts and
pages are the published ones; categories and tags those with a published post. `lastmod` is the
record's last change.

## Images

`GET /api/v1/media/{id}/{name}` needs no key: it is the URL in `html` and `cover`.

- It serves an image (PNG, JPEG, GIF, WebP) used by published content (cover, OG image, body,
  or the avatar of an author with a published post), unless the image is in the trash. Anything
  else is the same 404, whether it exists or not.
- `Cache-Control: public, max-age=3600` (an image that stops being public stays at most an hour
  in caches; never `immutable`), `X-Content-Type-Options: nosniff`, a sandboxing CSP.
- Rate limited per client address (600 a minute, then 429 with `Retry-After`). Put a CDN in
  front of it for real traffic.

## Preview

A preview link from the admin carries a token (43 letters and digits). Its frontend route calls
`GET /api/v1/preview?token=<token>` from the server, without a key: the token is the access. It
is a query parameter, not a path segment, so that it stays out of RBlog's request log (which
records paths only).

- The answer is the post or page as above, in whatever status, plus
  `"preview": { "type": "post" | "page", "status": "draft" | "pending" | "scheduled" | "published" }`.
  `published_at` may be `null`.
- Its image URLs are `/api/v1/preview/media/{id}?token=<token>`: they serve only the cover, the OG
  image and the body images of that one post or page (not trashed), while the token works.
- A token lasts one hour and can be revoked. Expired, revoked and unknown tokens, and a trashed
  post or page, answer 404.
- Every answer (errors included) is `Cache-Control: private, no-store` with
  `Referrer-Policy: no-referrer`: never cache it, never put it behind a shared cache. Rate
  limited per client address (300 a minute for both routes together).

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
  `tag:{slug}`, `menu:{key}`, `settings`, `sitemap`) and the old and new paths:

  ```json
  {
    "id": "0192b0a4-7c1e-7000-8000-000000000001",
    "kind": "category.updated",
    "subject": { "type": "category", "id": "12" },
    "tags": ["category:du-lich", "category:du-lich-cu", "posts", "sitemap"],
    "paths": { "old": ["/categories/du-lich-cu"], "new": ["/categories/du-lich"] },
    "created_at": "2026-10-24T08:15:30.123456Z"
  }
  ```

  - `id` is the event id, the same as `X-RBlog-Event-Id`; deduplicate on it.
  - `kind` is `{subject}.{change}` (`post.published`, `category.updated`, …). Handle unknown
    kinds by revalidating `tags` and `paths` anyway.
  - `subject.id` is a string (64-bit ids travel as strings), or `null` for a subject without
    an id (`settings`).
  - `paths.old` are public paths that no longer show this content (an old slug, a deleted
    record): purge or redirect them. `paths.new` are the paths that show it now. Both are
    always arrays, possibly empty.
  - `created_at` is when the change was recorded (UTC, RFC 3339, microseconds). Deliveries can
    arrive out of order and more than once: revalidating is the safe answer to any of them.
  - New fields may be added; existing ones keep their name and meaning.
- an event is retried for about an hour (8 attempts, 30 s apart at first, doubling). One the
  receiver refuses for good (3xx, other 4xx), or that still fails after the last attempt, is not
  sent again unless an operator retries its job on the panel's Queue page;
- while no webhook URL is set, events are marked handled without being sent: setting a URL
  later does not replay the backlog;
- the URL must be `https://` (no user name or password in it) and resolve only to public
  addresses: RBlog resolves it at every delivery and refuses loopback, private, link-local and
  other special ranges, IPv4 and IPv6. For a receiver on the developer's machine,
  `allow_insecure_local = true` in `config/webhook.toml` also allows `http://` and loopback
  addresses (never private ranges); RBlog refuses to start with it when `APP_ENV=production`.
  Settings → Integrations still asks for an `https://` URL;
- answer **2xx** within 10 seconds (5 to connect). Redirects are not followed and count as a
  failure. RBlog retries timeouts, connection errors, 408, 425, 429 and 5xx; any other answer
  (3xx, other 4xx) fails the delivery without retrying.

Each event is a row of RBlog's outbox (`content_events`), written in the same transaction as
the change (`src/services/content_sync.rs`). Its payload:

```json
{
  "tags": ["category:du-lich", "category:du-lich-cu", "posts", "sitemap"],
  "paths": { "old": ["/categories/du-lich-cu"], "new": ["/categories/du-lich"] }
}
```

- `tags`: what to revalidate, without duplicates. A renamed category or tag lists its new and
  its old slug.
- `paths.old`: paths that no longer show the content (an old slug, a deleted record);
  `paths.new`: the paths that show it now. Categories are `/categories/{slug}` and tags
  `/tags/{slug}`; a menu has no path of its own (both lists empty).

| Event kind | When | Tags |
|---|---|---|
| `category.created`, `category.updated`, `category.deleted` | a category is written | `category:{slug}` (and the old slug on a rename), `posts`, `sitemap` |
| `tag.created`, `tag.updated`, `tag.deleted` | a tag is written | `tag:{slug}` (and the old slug on a rename), `posts`, `sitemap` |
| `menu.created`, `menu.updated`, `menu.deleted` | a menu or one of its items is written (an item change is `menu.updated`) | `menu:{key}` |

Keep a fallback time-based revalidation (for example 10 minutes) in case the webhook stays
unreachable.
