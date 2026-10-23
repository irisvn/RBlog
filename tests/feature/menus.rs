//! Menus and their items through the admin API: the item rules (422 on the field), items
//! created on the menu's page, and a `menu:{key}` outbox event for every write.

use rblog::database::migrations::Migrator;
use rblog::models::{category, menu_item, post};
use rblog::services::menus;
use rustcms::axum::http::{Method, StatusCode};
use rustcms_testing::{Factory, TestDb, UserFactory, db_test};
use sea_orm::{ActiveModelTrait, EntityTrait, Set};
use serde_json::{Value, json};

use crate::support;
use crate::taxonomy::{admin, events};

fn key(response: &Value) -> i64 {
    response["data"]["key"].as_str().expect("record key").parse().unwrap()
}

db_test!(menus_have_a_known_unique_key, migrate = Migrator, |db: TestDb| async move {
    let app = support::app(db).await;
    let panel = admin(&app).await;
    let records = app.admin_api("resources/menus/records");
    let created = panel.post_json(&records, &json!({"key": "primary", "name": "Main"})).await;
    created.assert_status(StatusCode::CREATED);
    let id = key(&created.json());
    panel.post_json(&records, &json!({"key": "sidebar", "name": "Side"})).await.assert_validation_error("key");
    panel.post_json(&records, &json!({"key": "primary", "name": "Again"})).await.assert_validation_error("key");
    panel.post_json(&records, &json!({"key": "footer", "name": " "})).await.assert_validation_error("name");
    // The key is chosen once; the name may change, but not to nothing.
    let record = format!("{records}/{id}");
    panel.send(Method::PATCH, &record, Some(&json!({"key": "footer"}))).await.assert_validation_error("key");
    panel.send(Method::PATCH, &record, Some(&json!({"name": ""}))).await.assert_validation_error("name");
    panel.send(Method::PATCH, &record, Some(&json!({"name": "Top"}))).await.assert_status(StatusCode::OK);

    let events = events(app.state().db(), "menu", id).await;
    assert_eq!(events.iter().map(|e| e.kind.as_str()).collect::<Vec<_>>(), ["menu.created", "menu.updated"]);
    for event in &events {
        assert_eq!(event.payload, json!({"tags": ["menu:primary"], "paths": {"old": [], "new": []}}));
    }
    panel.send(Method::DELETE, &record, None).await.assert_status(StatusCode::NO_CONTENT);
    let last = crate::taxonomy::events(app.state().db(), "menu", id).await.pop().unwrap();
    assert_eq!(last.kind, "menu.deleted");
});

db_test!(menu_items_follow_the_rules, migrate = Migrator, |db: TestDb| async move {
    let app = support::app(db).await;
    let panel = admin(&app).await;
    let db = app.state().db();
    let menus_url = app.admin_api("resources/menus/records");
    let primary = key(&panel.post_json(&menus_url, &json!({"key": "primary", "name": "Main"})).await.json());
    let footer = key(&panel.post_json(&menus_url, &json!({"key": "footer", "name": "Footer"})).await.json());
    let items_of = |menu: i64| format!("{menus_url}/{menu}/relations/items");
    let items = items_of(primary);
    let news =
        category::ActiveModel { name: Set("Tin tức".into()), slug: Set("tin-tuc".into()), ..Default::default() }
            .insert(db)
            .await
            .unwrap();
    let author = UserFactory::new().create(db).await;
    let hello = post::ActiveModel {
        title: Set("Xin chào".into()),
        slug: Set("xin-chao".into()),
        author_id: Set(author.id),
        ..Default::default()
    }
    .insert(db)
    .await
    .unwrap();
    let events_before = events(db, "menu", primary).await.len();

    // Each rule, refused on its field.
    let refused = [
        (json!({"label": "A", "kind": "link", "url": "/a"}), "kind"),
        (json!({"label": "A", "kind": "url"}), "url"),
        (json!({"label": "A", "kind": "url", "url": "   "}), "url"),
        (json!({"label": "A", "kind": "url", "url": "javascript:alert(1)"}), "url"),
        (json!({"label": "A", "kind": "url", "url": "//evil.example"}), "url"),
        (json!({"label": "A", "kind": "url", "url": "ftp://example.com"}), "url"),
        (json!({"label": "A", "kind": "url", "url": "https://example.com:bad"}), "url"),
        (json!({"label": "  ", "kind": "url", "url": "/a"}), "label"),
        (json!({"label": "A", "kind": "url", "url": "/a", "target_id": news.id.to_string()}), "target_id"),
        (json!({"label": "A", "kind": "category"}), "target_id"),
        (json!({"label": "A", "kind": "category", "target_id": news.id.to_string(), "url": "/a"}), "url"),
        (json!({"label": "A", "kind": "category", "target_id": "999999"}), "target_id"),
        (json!({"label": "A", "kind": "tag", "target_id": news.id.to_string()}), "target_id"),
        (json!({"label": "A", "kind": "post", "target_id": "999999"}), "target_id"),
        (json!({"label": "A", "kind": "page", "target_id": hello.id.to_string()}), "target_id"),
    ];
    for (body, field) in refused {
        let response = panel.post_json(&items, &body).await;
        response.assert_validation_error(field);
    }
    assert!(menu_item::Entity::find().all(db).await.unwrap().is_empty());
    assert_eq!(events(db, "menu", primary).await.len(), events_before, "refused writes leave no event");

    // Valid items: a path, a URL, a category, a post.
    let home = panel.post_json(&items, &json!({"label": "Home", "kind": "url", "url": " / "})).await;
    home.assert_status(StatusCode::CREATED);
    let home = key(&home.json());
    let stored = menu_item::Entity::find_by_id(home).one(db).await.unwrap().unwrap();
    assert_eq!((stored.menu_id, stored.url.as_deref()), (primary, Some("/")));
    let news_item = panel
        .post_json(&items, &json!({"label": "News", "kind": "category", "target_id": news.id.to_string(), "url": ""}))
        .await;
    news_item.assert_status(StatusCode::CREATED);
    let news_item = key(&news_item.json());
    assert_eq!(menu_item::Entity::find_by_id(news_item).one(db).await.unwrap().unwrap().url, None);
    let child = panel
        .post_json(
            &items,
            &json!({"label": "Hello", "kind": "post", "target_id": hello.id.to_string(), "parent_id": news_item.to_string()}),
        )
        .await;
    child.assert_status(StatusCode::CREATED);
    let child = key(&child.json());
    panel
        .post_json(&items, &json!({"label": "Mail", "kind": "url", "url": "mailto:hi@example.test", "new_tab": true}))
        .await
        .assert_status(StatusCode::CREATED);

    let menu_events = events(db, "menu", primary).await;
    assert_eq!(menu_events.len(), events_before + 4);
    let last = menu_events.last().unwrap();
    assert_eq!(last.kind, "menu.updated");
    assert_eq!(last.payload, json!({"tags": ["menu:primary"], "paths": {"old": [], "new": []}}));

    // Parents: the same menu, not the item itself, one level deep.
    let other = panel.post_json(&items_of(footer), &json!({"label": "About", "kind": "url", "url": "/about"})).await;
    let other = key(&other.json());
    panel
        .post_json(&items, &json!({"label": "X", "kind": "url", "url": "/x", "parent_id": other.to_string()}))
        .await
        .assert_validation_error("parent_id");
    panel
        .post_json(&items, &json!({"label": "X", "kind": "url", "url": "/x", "parent_id": child.to_string()}))
        .await
        .assert_validation_error("parent_id");
    let item_url = |id: i64| app.admin_api(&format!("resources/menu_items/records/{id}"));
    panel
        .send(Method::PATCH, &item_url(home), Some(&json!({"parent_id": home.to_string()})))
        .await
        .assert_validation_error("parent_id");
    // An item with sub-items stays at the top.
    panel
        .send(Method::PATCH, &item_url(news_item), Some(&json!({"parent_id": home.to_string()})))
        .await
        .assert_validation_error("parent_id");
    // Updates are checked against what is stored: a URL item cannot become a category one alone.
    panel
        .send(Method::PATCH, &item_url(home), Some(&json!({"kind": "category"})))
        .await
        .assert_validation_error("target_id");
    panel
        .send(
            Method::PATCH,
            &item_url(home),
            Some(&json!({"kind": "category", "url": null, "target_id": news.id.to_string()})),
        )
        .await
        .assert_status(StatusCode::OK);
    // An item never moves to another menu.
    panel
        .send(Method::PATCH, &item_url(home), Some(&json!({"menu_id": footer.to_string()})))
        .await
        .assert_validation_error("menu_id");

    // Deleting an item is a change of its menu.
    let before = events(db, "menu", primary).await.len();
    panel.send(Method::DELETE, &item_url(child), None).await.assert_status(StatusCode::NO_CONTENT);
    let after = events(db, "menu", primary).await;
    assert_eq!(after.len(), before + 1);
    assert_eq!(after.last().unwrap().kind, "menu.updated");
});

db_test!(menus_refuse_users_without_the_permissions, migrate = Migrator, |db: TestDb| async move {
    let app = support::app(db).await;
    let menu = {
        let panel = admin(&app).await;
        key(&panel
            .post_json(&app.admin_api("resources/menus/records"), &json!({"key": "primary", "name": "Main"}))
            .await
            .json())
    };
    let user = UserFactory::new().create(app.state().db()).await;
    let editor = app.acting_as(&user).await;
    let records = app.admin_api("resources/menus/records");
    editor.get(&records).await.assert_status(StatusCode::FORBIDDEN);
    editor.post_json(&records, &json!({"key": "footer", "name": "F"})).await.assert_status(StatusCode::FORBIDDEN);
    editor
        .post_json(&format!("{records}/{menu}/relations/items"), &json!({"label": "A", "kind": "url", "url": "/"}))
        .await
        .assert_status(StatusCode::FORBIDDEN);
    editor
        .post_json(
            &app.admin_api("resources/menu_items/records"),
            &json!({"menu_id": menu.to_string(), "label": "A", "kind": "url", "url": "/"}),
        )
        .await
        .assert_status(StatusCode::FORBIDDEN);
    assert!(menu_item::Entity::find().all(app.state().db()).await.unwrap().is_empty());
});

/// The messages hooks return are translated (`lang/vi.toml`): `lang:missing` does not see them.
#[test]
fn every_hook_message_speaks_vietnamese() {
    let mut translator = rustcms::i18n::Translator::default();
    translator.add("vi", include_str!("../../lang/vi.toml")).unwrap();
    let mut messages = menus::MESSAGES.to_vec();
    messages.extend([rblog::services::taxonomy::SLUG_WITHOUT_LETTERS, "is already taken"]);
    let missing: Vec<&str> = messages.into_iter().filter(|m| translator.find("vi", None, m).is_none()).collect();
    assert!(missing.is_empty(), "not in lang/vi.toml: {missing:?}");
}
