//! `rustcms db:seed --class demo`: Vietnamese demo content to try the admin with. Never called by
//! `DatabaseSeeder`, refused when `APP_ENV=production`, and safe to run again: every row is
//! found by its natural key (email, slug, menu key, menu and label) before it is created, so a
//! second run adds nothing.
//!
//! - 3 authors (users with a random password nobody knows, and their author profiles);
//! - 4 categories and 8 tags;
//! - 20 posts: 9 published at different past dates, 3 scheduled, 3 pending review, 3 drafts and
//!   2 in the trash; the first five have a cover, an image of the media library drawn by
//!   `demo_images` with its alternative text in Vietnamese;
//! - 2 pages (Giới thiệu, Liên hệ) and the `primary` and `footer` menus.
//!
//! Users are created through `RbacService` (their own transactions); the content is written in
//! one transaction with its outbox events, `public_media` rows and audit entries.

use rustcms::audit::{self, AuditContext, AuditEntry};
use rustcms::auth::secret::random_string;
use rustcms::auth::{Actor, AuthState, NewUser, RbacService};
use rustcms::config::Environment;
use rustcms::database::Seeder;
use rustcms::models::user;
use rustcms::{AppState, async_trait};
use sea_orm::prelude::DateTimeUtc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseTransaction, EntityTrait, PaginatorTrait, QueryFilter, QuerySelect, Set,
};
use serde_json::{Value, json};

use super::demo_images;
use crate::models::{author_profile, category, menu, menu_item, page, post, post_tag, tag};
use crate::services::authors::{self, NewProfile};
use crate::services::content::Kind;
use crate::services::content_sync::{self, Change};
use crate::services::menus::{self, Item};
use crate::services::slug::slugify;
use crate::services::taxonomy;

/// `(name, email, profile slug, bio, website)`.
pub const AUTHORS: [(&str, &str, &str, &str, &str); 3] = [
    (
        "Nguyễn Thu Hà",
        "thu.ha@demo.rblog.local",
        "thu-ha",
        "Biên tập viên mảng du lịch và ẩm thực, thích đi chợ sớm và ghi chép bằng bút chì.",
        "https://thuha.example.com",
    ),
    (
        "Trần Minh Quân",
        "minh.quan@demo.rblog.local",
        "minh-quan",
        "Lập trình viên backend, viết về Rust, cơ sở dữ liệu và những công cụ nhỏ mà có võ.",
        "https://minhquan.example.com",
    ),
    (
        "Lê Hoàng Yến",
        "hoang.yen@demo.rblog.local",
        "hoang-yen",
        "Viết về đời sống đô thị, sách và những thói quen tốt bắt đầu từ việc nhỏ.",
        "",
    ),
];

/// `(slug, name, description)`.
pub const CATEGORIES: [(&str, &str, &str); 4] = [
    ("du-lich", "Du lịch", "Những chuyến đi gần và xa, kèm lịch trình và kinh nghiệm thực tế."),
    ("am-thuc", "Ẩm thực", "Món ngon ba miền, công thức nấu ăn và câu chuyện quanh mâm cơm."),
    ("cong-nghe", "Công nghệ", "Lập trình, công cụ và những điều học được khi làm phần mềm."),
    ("doi-song", "Đời sống", "Thói quen, sách và nhịp sống thường ngày."),
];

/// `(slug, name)`.
pub const TAGS: [(&str, &str); 8] = [
    ("ha-noi", "Hà Nội"),
    ("sai-gon", "Sài Gòn"),
    ("da-nang", "Đà Nẵng"),
    ("pho", "Phở"),
    ("ca-phe", "Cà phê"),
    ("rust", "Rust"),
    ("lap-trinh", "Lập trình"),
    ("meo-vat", "Mẹo vặt"),
];

/// Where a demo post is in the workflow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Demo {
    /// Published this many days ago.
    Published(i64),
    /// Scheduled this many days ahead.
    Scheduled(i64),
    Pending,
    Draft,
    /// Published this many days ago, then moved to the trash.
    TrashedPublished(i64),
    /// A draft moved to the trash.
    TrashedDraft,
}

/// A demo post: `(title, excerpt, category slug, tag slugs, author index, state)`.
pub type DemoPost = (&'static str, &'static str, &'static str, &'static [&'static str], usize, Demo);

pub const POSTS: [DemoPost; 20] = [
    (
        "Một ngày lang thang phố cổ Hà Nội",
        "Từ gánh xôi sáng ở Hàng Bài đến tách trà đá chiều trên phố Tạ Hiện: một lịch trình đi bộ trọn ngày.",
        "du-lich",
        &["ha-noi", "meo-vat"],
        0,
        Demo::Published(2),
    ),
    (
        "Bí quyết nấu nước dùng phở bò trong veo",
        "Chần xương, nướng gừng hành và giữ lửa liu riu: ba bước để nồi nước dùng trong mà ngọt.",
        "am-thuc",
        &["pho", "ha-noi"],
        0,
        Demo::Published(5),
    ),
    (
        "Cà phê trứng: chuyện một thức uống Hà thành",
        "Ra đời trong những năm thiếu sữa, cà phê trứng nay thành món phải thử khi ghé Hà Nội.",
        "am-thuc",
        &["ca-phe", "ha-noi"],
        0,
        Demo::Published(9),
    ),
    (
        "Đà Nẵng mùa hè: lịch trình ba ngày hai đêm",
        "Biển Mỹ Khê buổi sáng, bán đảo Sơn Trà buổi chiều và chợ Cồn khi đói bụng.",
        "du-lich",
        &["da-nang"],
        0,
        Demo::Published(14),
    ),
    (
        "Viết API đầu tiên bằng Rust và Axum",
        "Từ cargo new đến một endpoint JSON có kiểm tra đầu vào, trong chưa đầy một trăm dòng.",
        "cong-nghe",
        &["rust", "lap-trinh"],
        1,
        Demo::Published(20),
    ),
    (
        "Sống chậm giữa Sài Gòn vội vã",
        "Một góc cà phê vợt, một buổi sáng không điện thoại: những cách nhỏ để thở chậm lại.",
        "doi-song",
        &["sai-gon", "ca-phe"],
        2,
        Demo::Published(27),
    ),
    (
        "Năm mẹo giữ thói quen đọc sách mỗi ngày",
        "Đọc mười trang trước khi ngủ và luôn mang theo một cuốn sách mỏng là đủ để bắt đầu.",
        "doi-song",
        &["meo-vat"],
        2,
        Demo::Published(35),
    ),
    (
        "Ăn gì ở chợ Bến Thành?",
        "Bánh canh cua, gỏi cuốn và chè ba màu: bản đồ ăn uống cho người lần đầu đến chợ.",
        "am-thuc",
        &["sai-gon"],
        0,
        Demo::Published(48),
    ),
    (
        "Rust có khó học như lời đồn?",
        "Borrow checker đáng sợ lúc đầu, nhưng là người thầy kiên nhẫn nhất mà bạn từng có.",
        "cong-nghe",
        &["rust", "lap-trinh"],
        1,
        Demo::Published(63),
    ),
    (
        "Hội An về đêm: đèn lồng và những con hẻm nhỏ",
        "Thả hoa đăng trên sông Hoài và tìm quán cao lầu khuất trong hẻm.",
        "du-lich",
        &["da-nang"],
        0,
        Demo::Scheduled(2),
    ),
    (
        "Bánh mì Sài Gòn qua từng thế hệ",
        "Từ ổ bánh mì thịt nguội đầu hẻm đến những tiệm nổi tiếng có hàng dài người chờ.",
        "am-thuc",
        &["sai-gon"],
        2,
        Demo::Scheduled(7),
    ),
    (
        "Quản lý lỗi trong Rust với thiserror và anyhow",
        "Khi nào dùng kiểu lỗi riêng, khi nào chỉ cần anyhow, và cách để thông báo lỗi dễ hiểu.",
        "cong-nghe",
        &["rust", "lap-trinh"],
        1,
        Demo::Scheduled(14),
    ),
    (
        "Kinh nghiệm leo Fansipan cho người mới",
        "Chuẩn bị giày, áo ấm và sức bền cho hai ngày leo núi qua rừng trúc.",
        "du-lich",
        &["meo-vat"],
        0,
        Demo::Pending,
    ),
    (
        "Pha cà phê phin đúng điệu tại nhà",
        "Chọn hạt, tráng phin bằng nước sôi và kiên nhẫn chờ từng giọt rơi.",
        "am-thuc",
        &["ca-phe", "meo-vat"],
        2,
        Demo::Pending,
    ),
    (
        "Làm việc từ xa: giữ nhịp sinh hoạt thế nào?",
        "Một góc làm việc riêng, giờ bắt đầu cố định và những quãng nghỉ thật sự.",
        "doi-song",
        &["meo-vat"],
        1,
        Demo::Pending,
    ),
    (
        "Bún chả Hà Nội và câu chuyện nước chấm",
        "Chả viên, chả miếng và bát nước chấm chua ngọt quyết định cả bữa trưa.",
        "am-thuc",
        &["ha-noi"],
        0,
        Demo::Draft,
    ),
    (
        "Tối ưu truy vấn MySQL cho blog nhỏ",
        "Chỉ mục đúng chỗ và EXPLAIN là hai người bạn trước khi nghĩ đến bộ nhớ đệm.",
        "cong-nghe",
        &["lap-trinh"],
        1,
        Demo::Draft,
    ),
    (
        "Góc ban công xanh trong căn hộ chung cư",
        "Vài chậu thảo mộc, một giàn tưới nhỏ giọt và ban công thành khu vườn mini.",
        "doi-song",
        &["meo-vat"],
        2,
        Demo::Draft,
    ),
    (
        "Ghi chú du lịch Sa Pa (bản cũ)",
        "Bản ghi chép cũ về Sa Pa, đã được thay bằng bài mới.",
        "du-lich",
        &[],
        0,
        Demo::TrashedPublished(40),
    ),
    (
        "Danh sách quán phở yêu thích (nháp bỏ)",
        "Một danh sách dang dở, không dùng nữa.",
        "am-thuc",
        &["pho"],
        0,
        Demo::TrashedDraft,
    ),
];

/// `(slug, title, excerpt, paragraphs)`.
pub const PAGES: [(&str, &str, &str, &[&str]); 2] = [
    (
        "gioi-thieu",
        "Giới thiệu",
        "Blog của những người thích viết về du lịch, ẩm thực, công nghệ và đời sống.",
        &[
            "Chúng tôi là một nhóm nhỏ viết về những điều mình thật sự trải nghiệm: một con phố, một món ăn, một dòng mã.",
            "Mỗi bài viết đều được biên tập cẩn thận trước khi đăng.",
        ],
    ),
    (
        "lien-he",
        "Liên hệ",
        "Gửi góp ý, đề xuất bài viết hoặc hợp tác.",
        &[
            "Bạn có thể viết cho chúng tôi qua email hello@example.com.",
            "Chúng tôi trả lời trong vòng ba ngày làm việc.",
        ],
    ),
];

/// A menu link to a category or a page, by slug: `(label, kind, slug)`.
type DemoLink = (&'static str, &'static str, &'static str);

/// `(key, name, links)`.
pub const MENUS: [(&str, &str, &[DemoLink]); 2] = [
    (
        "primary",
        "Menu chính",
        &[
            ("Du lịch", "category", "du-lich"),
            ("Ẩm thực", "category", "am-thuc"),
            ("Công nghệ", "category", "cong-nghe"),
            ("Đời sống", "category", "doi-song"),
            ("Giới thiệu", "page", "gioi-thieu"),
        ],
    ),
    (
        "footer",
        "Chân trang",
        &[("Giới thiệu", "page", "gioi-thieu"), ("Liên hệ", "page", "lien-he"), ("Du lịch", "category", "du-lich")],
    ),
];

/// The message of a run in production.
pub const REFUSED_IN_PRODUCTION: &str = "the demo seeder writes sample content; refusing in production";

pub struct DemoSeeder;

#[async_trait]
impl Seeder for DemoSeeder {
    fn name(&self) -> &'static str {
        "demo"
    }

    async fn run(&self, state: &AppState) -> anyhow::Result<()> {
        // Sample content on a live site would be published to readers: never, whatever
        // `db:seed --force` says.
        if state.config().app.env == Environment::Production {
            anyhow::bail!(REFUSED_IN_PRODUCTION);
        }
        let users = ensure_users(state).await?;
        let db = state.db();
        let txn = rustcms::db::begin_write(db).await?;
        let mut audit = Vec::new();
        seed_profiles(&txn, &users, &mut audit).await?;
        let categories = seed_categories(&txn, &mut audit).await?;
        let tags = seed_tags(&txn, &mut audit).await?;
        let storage = rustcms::storage::of(state);
        seed_posts(&txn, storage.as_deref(), &users, &categories, &tags, &mut audit).await?;
        let pages = seed_pages(&txn, users[0].id, &mut audit).await?;
        seed_menus(&txn, &categories, &pages, &mut audit).await?;
        audit::record_all(&txn, &AuditContext::system(Some("db:seed --class demo")), audit).await?;
        txn.commit().await?;
        Ok(())
    }
}

/// The demo users, created through `RbacService` when missing, with a random password nobody
/// knows (a super admin can set one on the Users page). No role: roles are given separately.
async fn ensure_users(state: &AppState) -> anyhow::Result<Vec<user::Model>> {
    let auth = state.service::<AuthState>().ok_or_else(|| anyhow::anyhow!("AuthProvider not registered"))?;
    let db = state.db();
    let rbac = RbacService::new(db, &auth.resolver);
    let mut users = Vec::new();
    for (name, email, ..) in AUTHORS {
        let found = user::Entity::find().filter(user::Column::Email.eq(email)).one(db).await?;
        let user = match found {
            Some(found) => found,
            None => {
                let password_hash = auth.passwords.hash(random_string(48)).await?;
                let new = NewUser { name, email, password_hash, super_admin_guard: None };
                rbac.create_user(Actor::System, new).await?
            }
        };
        users.push(user);
    }
    Ok(users)
}

/// A profile for each demo user without one (`services::authors`: slug, event, media).
async fn seed_profiles(
    txn: &DatabaseTransaction,
    users: &[user::Model],
    audit: &mut Vec<AuditEntry>,
) -> anyhow::Result<()> {
    for (user, (_, _, slug, bio, website)) in users.iter().zip(AUTHORS) {
        if author_profile::Entity::find_by_id(user.id).one(txn).await?.is_some() {
            continue;
        }
        let new = NewProfile {
            user_id: user.id,
            slug: Some(slug),
            bio: Some(bio),
            avatar_media_id: None,
            website: Some(website),
        };
        let profile = authors::create(txn, new).await?;
        audit.push(AuditEntry::new("created", "authors").key(profile.user_id));
    }
    Ok(())
}

/// The demo categories, by slug: `(slug, id)`.
async fn seed_categories(txn: &DatabaseTransaction, audit: &mut Vec<AuditEntry>) -> anyhow::Result<Vec<(String, i64)>> {
    let mut out = Vec::new();
    for (slug, name, description) in CATEGORIES {
        let found = category::Entity::find().filter(category::Column::Slug.eq(slug)).one(txn).await?;
        let id = match found {
            Some(found) => found.id,
            None => {
                let slug = taxonomy::slug_for_create::<category::Entity>(txn, name, Some(slug)).await?;
                let now = content_sync::now();
                let made = category::ActiveModel {
                    name: Set(name.into()),
                    slug: Set(slug),
                    description: Set(Some(description.into())),
                    created_at: Set(now),
                    updated_at: Set(now),
                    ..Default::default()
                }
                .insert(txn)
                .await?;
                taxonomy::saved::<category::Entity>(txn, made.id, &made.slug, made.created_at, made.updated_at).await?;
                audit.push(AuditEntry::new("created", "categories").key(made.id));
                made.id
            }
        };
        out.push((slug.to_owned(), id));
    }
    Ok(out)
}

/// The demo tags, by slug: `(slug, id)`.
async fn seed_tags(txn: &DatabaseTransaction, audit: &mut Vec<AuditEntry>) -> anyhow::Result<Vec<(String, i64)>> {
    let mut out = Vec::new();
    for (slug, name) in TAGS {
        let found = tag::Entity::find().filter(tag::Column::Slug.eq(slug)).one(txn).await?;
        let id = match found {
            Some(found) => found.id,
            None => {
                let slug = taxonomy::slug_for_create::<tag::Entity>(txn, name, Some(slug)).await?;
                let now = content_sync::now();
                let made = tag::ActiveModel {
                    name: Set(name.into()),
                    slug: Set(slug),
                    created_at: Set(now),
                    updated_at: Set(now),
                    ..Default::default()
                }
                .insert(txn)
                .await?;
                taxonomy::saved::<tag::Entity>(txn, made.id, &made.slug, made.created_at, made.updated_at).await?;
                audit.push(AuditEntry::new("created", "tags").key(made.id));
                made.id
            }
        };
        out.push((slug.to_owned(), id));
    }
    Ok(out)
}

/// The id of `slug` in `ids`.
fn id_of(ids: &[(String, i64)], slug: &str) -> anyhow::Result<i64> {
    ids.iter().find(|(s, _)| s == slug).map(|(_, id)| *id).ok_or_else(|| anyhow::anyhow!("no demo `{slug}`"))
}

const HOUR: i64 = 3_600;
const DAY: i64 = 24 * HOUR;

/// `days` days from now (negative: ago), at `minute` past the current hour, so that dates look
/// written by people.
fn days_from_now(days: i64, minute: i64) -> DateTimeUtc {
    let hour = content_sync::now().timestamp() / HOUR * HOUR;
    at(hour + days * DAY + minute * 60)
}

/// `instant` moved by `seconds`.
fn shifted(instant: DateTimeUtc, seconds: i64) -> DateTimeUtc {
    at(instant.timestamp() + seconds)
}

fn at(unix_seconds: i64) -> DateTimeUtc {
    DateTimeUtc::from_timestamp(unix_seconds, 0).unwrap_or_default()
}

async fn seed_posts(
    txn: &DatabaseTransaction,
    storage: Option<&dyn rustcms::storage::Storage>,
    users: &[user::Model],
    categories: &[(String, i64)],
    tags: &[(String, i64)],
    audit: &mut Vec<AuditEntry>,
) -> anyhow::Result<()> {
    for (n, (title, excerpt, category_slug, tag_slugs, author, demo)) in POSTS.into_iter().enumerate() {
        let slug = slugify(title);
        // Trashed posts keep their slug: they count as present.
        if post::Entity::find().filter(post::Column::Slug.eq(slug.as_str())).count(txn).await? > 0 {
            continue;
        }
        let body = post_body(excerpt, category_slug);
        let minute = (n as i64 * 7) % 60;
        let (status, published_at, deleted_at) = match demo {
            Demo::Published(days) => ("published", Some(days_from_now(-days, minute)), None),
            Demo::Scheduled(days) => ("scheduled", Some(days_from_now(days, minute)), None),
            Demo::Pending => ("pending", None, None),
            Demo::Draft => ("draft", None, None),
            Demo::TrashedPublished(days) => {
                ("published", Some(days_from_now(-days, minute)), Some(days_from_now(-1, minute)))
            }
            Demo::TrashedDraft => ("draft", None, Some(days_from_now(-2, minute))),
        };
        // Written a few days before it went live (or recently, if it has not); a published post
        // was last edited a few hours after it went live, the others since.
        let now = content_sync::now();
        let n = n as i64;
        let (created_at, updated_at) = match published_at {
            Some(live) if live <= now => (shifted(live, -(2 + n % 3) * DAY), shifted(live, (n % 5) * HOUR)),
            _ => {
                let created_at = days_from_now(-(4 + n % 6), minute);
                (created_at, shifted(created_at, (1 + n % 3) * DAY))
            }
        };
        // A cover for the first posts, uploaded by their author (without a media library: none).
        let cover = match (storage, demo_images::IMAGES.get(n as usize)) {
            (Some(storage), Some(image)) => Some(demo_images::ensure(txn, storage, users[author].id, *image).await?.id),
            _ => None,
        };
        let made = post::ActiveModel {
            title: Set(title.into()),
            slug: Set(slug),
            cover_media_id: Set(cover),
            excerpt: Set(Some(excerpt.into())),
            body: Set(Some(body)),
            category_id: Set(Some(id_of(categories, category_slug)?)),
            author_id: Set(users[author].id),
            status: Set(status.into()),
            published_at: Set(published_at),
            featured: Set(n == 0),
            created_at: Set(created_at),
            updated_at: Set(updated_at),
            deleted_at: Set(deleted_at),
            ..Default::default()
        }
        .insert(txn)
        .await?;
        for (position, tag_slug) in tag_slugs.iter().enumerate() {
            post_tag::ActiveModel {
                post_id: Set(made.id),
                tag_id: Set(id_of(tags, tag_slug)?),
                position: Set(position as i32),
            }
            .insert(txn)
            .await?;
        }
        // Search text, reading time, public media, the first revision and the `post.created` event.
        content_sync::content_changed(txn, Kind::Post, made.id, Change::Created, Some(users[author].id)).await?;
        audit.push(AuditEntry::new("created", "posts").key(made.id));
    }
    Ok(())
}

/// The demo pages, by slug: `(slug, id)`.
async fn seed_pages(
    txn: &DatabaseTransaction,
    author_id: i64,
    audit: &mut Vec<AuditEntry>,
) -> anyhow::Result<Vec<(String, i64)>> {
    let mut out = Vec::new();
    for (n, (slug, title, excerpt, paragraphs)) in PAGES.into_iter().enumerate() {
        let found: Option<i64> = page::Entity::find()
            .select_only()
            .column(page::Column::Id)
            .filter(page::Column::Slug.eq(slug))
            .into_tuple()
            .one(txn)
            .await?;
        let id = match found {
            Some(id) => id,
            None => {
                let body = doc(paragraphs.iter().map(|p| paragraph(p)).collect());
                let published_at = days_from_now(-90 + n as i64, 0);
                let made = page::ActiveModel {
                    title: Set(title.into()),
                    slug: Set(slug.into()),
                    excerpt: Set(Some(excerpt.into())),
                    body: Set(Some(body)),
                    author_id: Set(author_id),
                    status: Set("published".into()),
                    published_at: Set(Some(published_at)),
                    created_at: Set(published_at),
                    updated_at: Set(published_at),
                    ..Default::default()
                }
                .insert(txn)
                .await?;
                content_sync::content_changed(txn, Kind::Page, made.id, Change::Created, Some(author_id)).await?;
                audit.push(AuditEntry::new("created", "pages").key(made.id));
                made.id
            }
        };
        out.push((slug.to_owned(), id));
    }
    Ok(out)
}

async fn seed_menus(
    txn: &DatabaseTransaction,
    categories: &[(String, i64)],
    pages: &[(String, i64)],
    audit: &mut Vec<AuditEntry>,
) -> anyhow::Result<()> {
    for (key, name, links) in MENUS {
        menus::check_menu(Some(key), name)?;
        // Locked like the panel's item writes (`lock_parent`): the item checks read the menu.
        let found = menu::Entity::find().filter(menu::Column::Key.eq(key)).lock_exclusive().one(txn).await?;
        let menu = match found {
            Some(found) => found,
            None => {
                let now = content_sync::now();
                let made = menu::ActiveModel {
                    key: Set(key.into()),
                    name: Set(name.into()),
                    created_at: Set(now),
                    updated_at: Set(now),
                    ..Default::default()
                }
                .insert(txn)
                .await?;
                content_sync::menu_changed(txn, Change::Created, made.id, &made.key).await?;
                audit.push(AuditEntry::new("created", "menus").key(made.id));
                made
            }
        };
        let mut added = false;
        for (position, (label, kind, slug)) in links.iter().enumerate() {
            let exists = menu_item::Entity::find()
                .filter(menu_item::Column::MenuId.eq(menu.id))
                .filter(menu_item::Column::Label.eq(*label))
                .count(txn)
                .await?
                > 0;
            if exists {
                continue;
            }
            let target_id = match *kind {
                "page" => id_of(pages, slug)?,
                _ => id_of(categories, slug)?,
            };
            let item = Item { menu_id: menu.id, label, kind, target_id: Some(target_id), ..Default::default() };
            menus::check_item(txn, &item).await?;
            let now = content_sync::now();
            let made = menu_item::ActiveModel {
                menu_id: Set(menu.id),
                label: Set((*label).into()),
                kind: Set((*kind).into()),
                target_id: Set(Some(target_id)),
                position: Set(position as i32),
                new_tab: Set(false),
                created_at: Set(now),
                updated_at: Set(now),
                ..Default::default()
            }
            .insert(txn)
            .await?;
            audit.push(AuditEntry::new("created", "menu_items").key(made.id));
            added = true;
        }
        if added {
            content_sync::menu_changed(txn, Change::Updated, menu.id, &menu.key).await?;
        }
    }
    Ok(())
}

/// Tips under each post, by category.
fn tips(category: &str) -> [&'static str; 3] {
    match category {
        "du-lich" => [
            "Đi sớm để tránh nắng và đông người.",
            "Mang theo chai nước và một chiếc áo khoác mỏng.",
            "Hỏi người dân địa phương về quán ăn ngon gần nhất.",
        ],
        "am-thuc" => [
            "Nguyên liệu tươi quyết định một nửa món ăn.",
            "Nêm nếm từ từ, nếm lại trước khi tắt bếp.",
            "Ăn khi còn nóng, cùng người thân là ngon nhất.",
        ],
        "cong-nghe" => [
            "Bắt đầu từ ví dụ nhỏ nhất chạy được.",
            "Viết test trước khi sửa lỗi.",
            "Đọc thông báo lỗi đến hết dòng cuối.",
        ],
        _ => [
            "Bắt đầu bằng một việc nhỏ mỗi ngày.",
            "Ghi lại điều mình biết ơn trước khi ngủ.",
            "Dành thời gian không màn hình cho bản thân.",
        ],
    }
}

/// A short rich-text body (RustCMS document): the excerpt, a heading, a list of tips and a
/// closing line with a bold word.
fn post_body(excerpt: &str, category: &str) -> Value {
    let items: Vec<Value> =
        tips(category).iter().map(|tip| json!({"type": "listItem", "content": [paragraph(tip)]})).collect();
    doc(vec![
        paragraph(excerpt),
        json!({"type": "heading", "attrs": {"level": 2}, "content": [text("Ghi chú nhanh")]}),
        json!({"type": "bulletList", "content": items}),
        json!({"type": "paragraph", "content": [
            text("Bài viết "),
            {"type": "text", "text": "mẫu", "marks": [{"type": "bold"}]},
            text(" của RBlog, tạo bởi lệnh db:seed --class demo."),
        ]}),
    ])
}

fn doc(content: Vec<Value>) -> Value {
    json!({"type": "doc", "version": 1, "content": content})
}

fn paragraph(content: &str) -> Value {
    json!({"type": "paragraph", "content": [text(content)]})
}

fn text(content: &str) -> Value {
    json!({"type": "text", "text": content})
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustcms::admin::rich_text;

    #[test]
    fn bodies_are_valid_documents() {
        for (_, excerpt, category, ..) in POSTS {
            rich_text::parse(&post_body(excerpt, category)).unwrap();
        }
        for (.., paragraphs) in PAGES {
            rich_text::parse(&doc(paragraphs.iter().map(|p| paragraph(p)).collect())).unwrap();
        }
    }

    #[test]
    fn every_status_is_seeded_with_unique_slugs() {
        let slugs: std::collections::BTreeSet<String> = POSTS.iter().map(|p| slugify(p.0)).collect();
        assert_eq!(slugs.len(), POSTS.len());
        for wanted in ["published", "scheduled", "pending", "draft", "trashed"] {
            assert!(
                POSTS.iter().any(|p| format!("{:?}", p.5).to_lowercase().contains(wanted)),
                "no {wanted} demo post"
            );
        }
    }
}
