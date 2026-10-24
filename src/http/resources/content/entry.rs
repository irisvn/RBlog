//! A post or a page, as docs/api.md shows it.

use std::collections::{BTreeSet, HashMap};

use rustcms::admin::media::entity as media;
use rustcms::admin::rich_text::{self, ImageInfo};
use rustcms::models::user;
use sea_orm::prelude::{DateTimeUtc, Uuid};
use sea_orm::{ColumnTrait, ConnectionTrait, DbErr, EntityTrait, QueryFilter, QueryOrder, QuerySelect};
use serde::Serialize;
use serde_json::Value;

use super::{ImageJson, Links, TermRef, instant, media_alt};
use crate::models::{author_profile, category, page, post, post_tag, tag};
use crate::services::public_media::body_media_ids;

/// What a single record adds to the list fields.
#[derive(Clone, Copy, Debug, Default)]
pub struct Detail {
    /// `html` and `outline`: the body rendered, and its headings (single records).
    pub html: bool,
    /// `document`, the body as stored (`?include=document`).
    pub document: bool,
}

/// The author of a post or page: `{slug, name}` (`slug` is null without a public profile).
#[derive(Debug, Clone, Serialize)]
pub struct AuthorRef {
    pub slug: Option<String>,
    pub name: String,
}

/// `seo`: the record's own SEO fields; `og_image` is the URL of its OG image.
#[derive(Debug, Clone, Serialize)]
pub struct SeoJson {
    pub meta_title: Option<String>,
    pub meta_description: Option<String>,
    pub canonical_url: Option<String>,
    pub og_image: Option<String>,
}

/// The fields posts and pages share.
#[derive(Debug, Clone, Serialize)]
pub struct EntryJson {
    pub id: String,
    pub slug: String,
    pub title: String,
    pub excerpt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub html: Option<String>,
    /// The body's headings, in order, with the ids `html` gives them (single records).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outline: Option<Vec<HeadingJson>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub document: Option<Value>,
    pub cover: Option<ImageJson>,
    pub author: AuthorRef,
    pub published_at: Option<String>,
    pub updated_at: String,
    pub reading_minutes: i32,
    pub seo: SeoJson,
}

/// A heading of the body: `{level, text, id}` (`id` is the bare slug `html` gives it).
#[derive(Debug, Clone, Serialize)]
pub struct HeadingJson {
    pub level: u8,
    pub text: String,
    pub id: String,
}

/// A post.
#[derive(Debug, Clone, Serialize)]
pub struct PostJson {
    #[serde(flatten)]
    pub entry: EntryJson,
    pub category: Option<TermRef>,
    pub tags: Vec<TermRef>,
    pub featured: bool,
}

/// A page.
#[derive(Debug, Clone, Serialize)]
pub struct PageJson {
    #[serde(flatten)]
    pub entry: EntryJson,
}

/// The columns of a post or page the shared fields come from.
struct Source<'a> {
    id: i64,
    slug: &'a str,
    title: &'a str,
    excerpt: &'a Option<String>,
    body: Option<&'a Value>,
    cover: Option<Uuid>,
    og: Option<Uuid>,
    author_id: i64,
    published_at: Option<DateTimeUtc>,
    updated_at: DateTimeUtc,
    reading_minutes: i32,
    meta_title: &'a Option<String>,
    meta_description: &'a Option<String>,
    canonical_url: &'a Option<String>,
}

impl<'a> From<&'a post::Model> for Source<'a> {
    fn from(p: &'a post::Model) -> Self {
        Self {
            id: p.id,
            slug: &p.slug,
            title: &p.title,
            excerpt: &p.excerpt,
            body: p.body.as_ref(),
            cover: p.cover_media_id,
            og: p.og_media_id,
            author_id: p.author_id,
            published_at: p.published_at,
            updated_at: p.updated_at,
            reading_minutes: p.reading_minutes,
            meta_title: &p.meta_title,
            meta_description: &p.meta_description,
            canonical_url: &p.canonical_url,
        }
    }
}

impl<'a> From<&'a page::Model> for Source<'a> {
    fn from(p: &'a page::Model) -> Self {
        Self {
            id: p.id,
            slug: &p.slug,
            title: &p.title,
            excerpt: &p.excerpt,
            body: p.body.as_ref(),
            cover: p.cover_media_id,
            og: p.og_media_id,
            author_id: p.author_id,
            published_at: p.published_at,
            updated_at: p.updated_at,
            reading_minutes: p.reading_minutes,
            meta_title: &p.meta_title,
            meta_description: &p.meta_description,
            canonical_url: &p.canonical_url,
        }
    }
}

/// What the shared fields need, loaded once for a list.
struct Shared {
    authors: HashMap<i64, AuthorRef>,
    images: HashMap<Uuid, media::Model>,
}

impl Shared {
    async fn load<C: ConnectionTrait>(
        db: &C,
        links: &Links<'_>,
        sources: &[Source<'_>],
        detail: Detail,
    ) -> Result<Self, DbErr> {
        let author_ids: BTreeSet<i64> = sources.iter().map(|s| s.author_id).collect();
        let mut image_ids: BTreeSet<Uuid> = sources.iter().flat_map(|s| s.cover.into_iter().chain(s.og)).collect();
        if detail.html {
            image_ids.extend(sources.iter().flat_map(|s| body_media_ids(s.body)));
        }
        Ok(Self { authors: authors(db, author_ids).await?, images: links.images(db, image_ids).await? })
    }

    fn entry(&self, links: &Links<'_>, s: &Source<'_>, detail: Detail) -> EntryJson {
        let image = |id: Option<Uuid>| id.and_then(|id| self.images.get(&id));
        // Images without their own alternative text get the media's (`html` only: `document`
        // stays as stored). Filled in after parsing, so the stored document's limits are the
        // only ones it meets.
        let document =
            detail.html.then(|| s.body.and_then(|body| rich_text::parse(body).ok())).flatten().map(|mut doc| {
                fill_media_alts(&mut doc.content, &self.images);
                doc
            });
        let html = detail.html.then(|| document.as_ref().map(|doc| self.html(links, doc)).unwrap_or_default());
        let outline = detail.html.then(|| {
            document
                .as_ref()
                .map(|doc| {
                    rich_text::outline(doc)
                        .into_iter()
                        .map(|h| HeadingJson { level: h.level, text: h.text, id: h.id })
                        .collect()
                })
                .unwrap_or_default()
        });
        EntryJson {
            id: s.id.to_string(),
            slug: s.slug.to_owned(),
            title: s.title.to_owned(),
            excerpt: s.excerpt.clone(),
            html,
            outline,
            document: if detail.document { Some(s.body.cloned().unwrap_or(Value::Null)) } else { None },
            cover: image(s.cover).map(|m| ImageJson::new(links, m)),
            author: self
                .authors
                .get(&s.author_id)
                .cloned()
                .unwrap_or_else(|| AuthorRef { slug: None, name: String::new() }),
            published_at: s.published_at.map(instant),
            updated_at: instant(s.updated_at),
            reading_minutes: s.reading_minutes,
            seo: SeoJson {
                meta_title: s.meta_title.clone(),
                meta_description: s.meta_description.clone(),
                canonical_url: s.canonical_url.clone(),
                og_image: image(s.og).map(|m| links.url(m)),
            },
        }
    }
}

impl Shared {
    /// `doc` as HTML: images these links may show as figures with their URL and size (so
    /// browsers reserve the space), the others left out.
    fn html(&self, links: &Links<'_>, doc: &rich_text::Document) -> String {
        let images: HashMap<Uuid, ImageInfo> = rich_text::media_ids(doc)
            .into_iter()
            .filter_map(|id| self.images.get(&id))
            .map(|m| {
                let info = ImageInfo::new(links.url(m));
                let size =
                    m.width.zip(m.height).and_then(|(w, h)| Some((u32::try_from(w).ok()?, u32::try_from(h).ok()?)));
                (
                    m.id,
                    match size {
                        Some((w, h)) if w > 0 && h > 0 => info.size(w, h),
                        _ => info,
                    },
                )
            })
            .collect();
        rich_text::to_html_with(doc, &images)
    }
}

/// Gives each image of `nodes` that has no alternative text of its own (blank) its media's
/// ([`media_alt`]) when `images` holds that media, or `""`: the document's text first, then the
/// library's, then none (never the file name). Images sit in blocks, quotes, lists and callouts
/// (table cells hold text only).
fn fill_media_alts(nodes: &mut [rich_text::Node], images: &HashMap<Uuid, media::Model>) {
    use rich_text::Node;
    for node in nodes {
        match node {
            Node::Image(image) if image.alt.trim().is_empty() => {
                image.alt = images.get(&image.media).map(media_alt).unwrap_or_default().to_owned();
            }
            Node::Paragraph(children)
            | Node::Heading(_, children)
            | Node::BulletList(children)
            | Node::OrderedList(children)
            | Node::ListItem(children)
            | Node::Blockquote(children)
            | Node::Callout(_, children) => fill_media_alts(children, images),
            _ => {}
        }
    }
}

/// Name and profile slug of each user in `ids`.
async fn authors<C: ConnectionTrait>(db: &C, ids: BTreeSet<i64>) -> Result<HashMap<i64, AuthorRef>, DbErr> {
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let names: Vec<(i64, String)> = user::Entity::find()
        .select_only()
        .column(user::Column::Id)
        .column(user::Column::Name)
        .filter(user::Column::Id.is_in(ids.iter().copied()))
        .into_tuple()
        .all(db)
        .await?;
    let slugs: HashMap<i64, String> = author_profile::Entity::find()
        .select_only()
        .column(author_profile::Column::UserId)
        .column(author_profile::Column::Slug)
        .filter(author_profile::Column::UserId.is_in(ids))
        .into_tuple::<(i64, String)>()
        .all(db)
        .await?
        .into_iter()
        .collect();
    Ok(names.into_iter().map(|(id, name)| (id, AuthorRef { slug: slugs.get(&id).cloned(), name })).collect())
}

/// `rows` as JSON, in their order.
pub async fn posts<C: ConnectionTrait>(
    db: &C,
    links: &Links<'_>,
    rows: &[post::Model],
    detail: Detail,
) -> Result<Vec<PostJson>, DbErr> {
    let sources: Vec<Source<'_>> = rows.iter().map(Source::from).collect();
    let shared = Shared::load(db, links, &sources, detail).await?;
    let category_ids: BTreeSet<i64> = rows.iter().filter_map(|p| p.category_id).collect();
    let categories: HashMap<i64, TermRef> = if category_ids.is_empty() {
        HashMap::new()
    } else {
        category::Entity::find()
            .filter(category::Column::Id.is_in(category_ids))
            .all(db)
            .await?
            .into_iter()
            .map(|c| (c.id, TermRef { slug: c.slug, name: c.name }))
            .collect()
    };
    let mut tags: HashMap<i64, Vec<TermRef>> = HashMap::new();
    if !rows.is_empty() {
        let links_and_tags = post_tag::Entity::find()
            .find_also_related(tag::Entity)
            .filter(post_tag::Column::PostId.is_in(rows.iter().map(|p| p.id)))
            .order_by_asc(post_tag::Column::PostId)
            .order_by_asc(post_tag::Column::Position)
            .order_by_asc(post_tag::Column::TagId)
            .all(db)
            .await?;
        for (link, found) in links_and_tags {
            if let Some(t) = found {
                tags.entry(link.post_id).or_default().push(TermRef { slug: t.slug, name: t.name });
            }
        }
    }
    Ok(rows
        .iter()
        .zip(&sources)
        .map(|(p, s)| PostJson {
            entry: shared.entry(links, s, detail),
            category: p.category_id.and_then(|id| categories.get(&id).cloned()),
            tags: tags.remove(&p.id).unwrap_or_default(),
            featured: p.featured,
        })
        .collect())
}

/// `rows` as JSON, in their order.
pub async fn pages<C: ConnectionTrait>(
    db: &C,
    links: &Links<'_>,
    rows: &[page::Model],
    detail: Detail,
) -> Result<Vec<PageJson>, DbErr> {
    let sources: Vec<Source<'_>> = rows.iter().map(Source::from).collect();
    let shared = Shared::load(db, links, &sources, detail).await?;
    Ok(sources.iter().map(|s| PageJson { entry: shared.entry(links, s, detail) }).collect())
}
