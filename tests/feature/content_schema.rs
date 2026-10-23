//! The content tables: unique slugs, foreign keys, migrations that refuse to lose data, and
//! `services::slug::unique_slug`.

use rblog::database::migrations::Migrator;
use rblog::models::{category, page, post, post_revision, post_tag, tag};
use rblog::services::slug::unique_slug;
use rustcms::migration::MigratorTrait;
use rustcms::models::user;
use rustcms_testing::{Factory, TestDb, UserFactory, db_test};
use sea_orm::{ActiveModelTrait, ConnectionTrait, EntityTrait, ModelTrait, PaginatorTrait, Set};
use serde_json::json;

async fn category(db: &impl ConnectionTrait, slug: &str) -> category::Model {
    category::ActiveModel { name: Set(slug.chars().take(120).collect()), slug: Set(slug.into()), ..Default::default() }
        .insert(db)
        .await
        .unwrap()
}

async fn tag(db: &impl ConnectionTrait, slug: &str) -> tag::Model {
    tag::ActiveModel { name: Set(slug.to_uppercase()), slug: Set(slug.into()), ..Default::default() }
        .insert(db)
        .await
        .unwrap()
}

fn post(author: &user::Model, slug: &str, category_id: Option<i64>) -> post::ActiveModel {
    post::ActiveModel {
        title: Set(slug.to_uppercase()),
        slug: Set(slug.into()),
        author_id: Set(author.id),
        category_id: Set(category_id),
        ..Default::default()
    }
}

db_test!(slugs_are_unique_and_posts_default_to_draft, migrate = Migrator, |db: TestDb| async move {
    let db = db.conn();
    let author = UserFactory::new().create(db).await;
    category(db, "tin-tuc").await;
    let duplicate =
        category::ActiveModel { name: Set("Other".into()), slug: Set("tin-tuc".into()), ..Default::default() }
            .insert(db)
            .await;
    assert!(duplicate.is_err(), "a second category with the same slug");

    let first = post(&author, "xin-chao", None).insert(db).await.unwrap();
    assert_eq!(first.status, "draft");
    assert_eq!(first.status(), Some(post::PostStatus::Draft));
    assert!(!first.featured);
    assert_eq!(first.reading_minutes, 0);
    assert_eq!(first.body, None);
    // A trashed post keeps its slug.
    let mut trashed: post::ActiveModel = first.into();
    trashed.deleted_at = Set(Some(chrono_now()));
    trashed.update(db).await.unwrap();
    assert!(post(&author, "xin-chao", None).insert(db).await.is_err(), "the slug of a trashed post is taken");

    // Pages have their own slugs.
    page::ActiveModel {
        title: Set("Xin chào".into()),
        slug: Set("xin-chao".into()),
        author_id: Set(author.id),
        ..Default::default()
    }
    .insert(db)
    .await
    .unwrap();
});

db_test!(deleting_a_category_leaves_its_posts_without_one, migrate = Migrator, |db: TestDb| async move {
    let db = db.conn();
    let author = UserFactory::new().create(db).await;
    let news = category(db, "news").await;
    let written = post(&author, "hello", Some(news.id)).insert(db).await.unwrap();
    assert_eq!(written.find_related(category::Entity).one(db).await.unwrap().map(|c| c.id), Some(news.id));

    news.delete(db).await.unwrap();
    let written = post::Entity::find_by_id(written.id).one(db).await.unwrap().unwrap();
    assert_eq!(written.category_id, None);
});

db_test!(deleting_a_post_deletes_its_tags_and_revisions, migrate = Migrator, |db: TestDb| async move {
    let db = db.conn();
    let author = UserFactory::new().create(db).await;
    let (rust, mysql) = (tag(db, "rust").await, tag(db, "mysql").await);
    let written = post(&author, "hello", None).insert(db).await.unwrap();
    let other = post(&author, "other", None).insert(db).await.unwrap();
    for (post_id, tag_id, position) in [(written.id, rust.id, 1), (written.id, mysql.id, 0), (other.id, rust.id, 0)] {
        post_tag::ActiveModel { post_id: Set(post_id), tag_id: Set(tag_id), position: Set(position) }
            .insert(db)
            .await
            .unwrap();
    }
    post_revision::ActiveModel {
        post_id: Set(written.id),
        title: Set("hello".into()),
        body: Set(None),
        snapshot: Set(json!({"tags": [rust.id, mysql.id]})),
        content_hash: Set("0".repeat(64)),
        author_id: Set(Some(author.id)),
        ..Default::default()
    }
    .insert(db)
    .await
    .unwrap();
    let tags: Vec<String> =
        written.find_related(tag::Entity).all(db).await.unwrap().into_iter().map(|t| t.slug).collect();
    assert_eq!(tags.len(), 2);
    assert_eq!(rust.find_related(post::Entity).count(db).await.unwrap(), 2);

    written.delete(db).await.unwrap();
    assert_eq!(post_tag::Entity::find().count(db).await.unwrap(), 1, "only the other post's tag is left");
    assert_eq!(post_revision::Entity::find().count(db).await.unwrap(), 0);
    assert_eq!(tag::Entity::find().count(db).await.unwrap(), 2, "tags stay");
});

db_test!(an_author_with_posts_cannot_be_deleted, migrate = Migrator, |db: TestDb| async move {
    let db = db.conn();
    let author = UserFactory::new().create(db).await;
    post(&author, "hello", None).insert(db).await.unwrap();
    assert!(author.delete(db).await.is_err(), "posts.author_id is RESTRICT");
});

db_test!(unique_slug_skips_taken_slugs, migrate = Migrator, |db: TestDb| async move {
    let db = db.conn();
    let author = UserFactory::new().create(db).await;
    assert_eq!(unique_slug(db, post::Entity, "Xin chào", None).await.unwrap(), "xin-chao");

    let first = post(&author, "xin-chao", None).insert(db).await.unwrap();
    let mut second = post(&author, "xin-chao-2", None);
    second.deleted_at = Set(Some(chrono_now()));
    second.insert(db).await.unwrap();
    // Taken by a live and a trashed post.
    assert_eq!(unique_slug(db, post::Entity, "Xin chào", None).await.unwrap(), "xin-chao-3");
    // A post keeps its own slug.
    assert_eq!(unique_slug(db, post::Entity, "Xin chào", Some(first.id)).await.unwrap(), "xin-chao");
    // Other tables do not count.
    assert_eq!(unique_slug(db, category::Entity, "Xin chào", None).await.unwrap(), "xin-chao");
    assert_eq!(unique_slug(db, tag::Entity, "!!!", None).await.unwrap(), "untitled");

    // The suffix fits the column (160 for categories).
    let long = "a".repeat(200);
    let taken = unique_slug(db, category::Entity, &long, None).await.unwrap();
    assert_eq!(taken, "a".repeat(160));
    category(db, &taken).await;
    assert_eq!(unique_slug(db, category::Entity, &long, None).await.unwrap(), format!("{}-2", "a".repeat(158)));
});

db_test!(rolling_back_refuses_to_lose_content, migrate = Migrator, |db: TestDb| async move {
    let db = db.conn();
    category(db, "news").await;
    // Newest first: the empty tables go, the taxonomy (holding a category) refuses.
    let refused = Migrator::down(db, None).await;
    assert!(refused.is_err(), "{refused:?}");
    assert_eq!(category::Entity::find().count(db).await.unwrap(), 1);
});

fn chrono_now() -> sea_orm::prelude::DateTimeUtc {
    std::time::SystemTime::now().into()
}
