//! Content API keys on the Integrations page (`{admin}/api/x/integrations/content-keys`): listed
//! without their secret, made (the secret answered once, `no-store`), revoked, and who may do it.

use rblog::database::migrations::Migrator;
use rblog::routes::admin::{INTEGRATIONS_MANAGE, INTEGRATIONS_VIEW, routes};
use rustcms::auth::entities::personal_access_token as token;
use rustcms::auth::{ADMIN_GUARD, Actor, AuthState, RbacService};
use rustcms::axum::http::{Method, StatusCode};
use rustcms::models::user;
use rustcms_testing::admin_routes::{Case, assert_access};
use rustcms_testing::{Factory, TestApp, TestClient, TestDb, TestResponse, UserFactory, db_test};
use sea_orm::EntityTrait;
use serde_json::{Value, json};

use crate::support::content::{self, APP_URL};

const KEYS: &str = "integrations/content-keys";

/// `{admin}/api/x/integrations/content-keys[/{rest}]`.
fn keys_uri(app: &TestApp, rest: &str) -> String {
    let base = app.admin_api(&format!("x/{KEYS}"));
    if rest.is_empty() { base } else { format!("{base}/{rest}") }
}

/// A signed-in super admin.
async fn super_admin(app: &TestApp) -> TestClient<'_> {
    let user = UserFactory::new().create(app.state().db()).await;
    app.make_super_admin(&user).await;
    app.acting_as(&user).await
}

/// A user holding only `permissions` in the admin guard (through a role).
async fn holding(app: &TestApp, name: &str, permissions: &[&str]) -> user::Model {
    let auth = app.state().service::<AuthState>().unwrap();
    let db = app.state().db();
    let rbac = RbacService::new(db, &auth.resolver);
    rbac.sync_permissions(&auth.permissions.all()).await.unwrap();
    let permissions: Vec<String> = permissions.iter().map(|p| (*p).to_owned()).collect();
    let role = rbac.create_role(Actor::System, ADMIN_GUARD, name, &permissions).await.unwrap();
    let user = UserFactory::new().create(db).await;
    rbac.assign_role(Actor::System, user.id, role.id).await.unwrap();
    user
}

fn no_store(response: &TestResponse) -> bool {
    response.header("cache-control").is_some_and(|v| v.contains("no-store"))
}

async fn create(app: &TestApp, client: &TestClient<'_>, body: Value) -> TestResponse {
    client.post_json(&keys_uri(app, ""), &body).await
}

db_test!(super_admin_lists_creates_and_revokes, migrate = Migrator, |db: TestDb| async move {
    let app = content::app(db).await;
    let admin = super_admin(&app).await;

    // Before any key (and before the frontend user exists): an empty list, and the base URL.
    let empty = admin.get(&keys_uri(&app, "")).await;
    empty.assert_status(StatusCode::OK);
    assert!(no_store(&empty));
    empty.assert_json("/data", json!([])).assert_json("/meta/total", json!(0));
    empty.assert_json("/base_url", json!(format!("{APP_URL}/api/v1/content")));

    // Made: the secret once, never cached.
    let made = create(&app, &admin, json!({ "name": "  Production site ", "expires_in_days": 90 })).await;
    made.assert_status(StatusCode::CREATED);
    assert!(no_store(&made));
    let made = made.json();
    let secret = made["token"].as_str().unwrap().to_owned();
    let id = made["key"]["id"].as_str().expect("the id travels as a string").to_owned();
    assert!(secret.starts_with(&format!("rcms_{id}_")), "{secret}");
    assert_eq!(made["key"]["name"], "Production site");
    let expires: sea_orm::prelude::DateTimeUtc = serde_json::from_value(made["key"]["expires_at"].clone()).unwrap();
    let days = (expires - chrono_now()).num_days();
    assert!((89..=90).contains(&days), "expires in {days} days");
    // A key without expiry.
    let forever = create(&app, &admin, json!({ "name": "Staging", "expires_in_days": null })).await;
    forever.assert_status(StatusCode::CREATED).assert_json("/key/expires_at", Value::Null);

    // Listed, newest first, never with the secret (nor its hash).
    let list = admin.get(&keys_uri(&app, "")).await;
    list.assert_status(StatusCode::OK);
    assert!(no_store(&list));
    let body = list.json();
    let names: Vec<&str> = body["data"].as_array().unwrap().iter().map(|k| k["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["Staging", "Production site"]);
    assert_eq!(body["meta"]["total"], 2);
    let secret_part = secret.rsplit('_').next().unwrap();
    assert!(!list.text().contains(secret_part), "{}", list.text());
    assert!(!list.text().contains("token_hash") && !list.text().contains("\"token\""), "{}", list.text());
    let listed = &body["data"][1];
    let mut fields: Vec<&str> = listed.as_object().unwrap().keys().map(String::as_str).collect();
    fields.sort_unstable();
    assert_eq!(fields, ["created_at", "expires_at", "id", "last_used_at", "name"]);
    assert_eq!(listed["last_used_at"], Value::Null);

    // The key reads the Content API, and its use shows in the list.
    let reader = app.client().bearer(&secret);
    reader.get("/api/v1/content/settings").await.assert_status(StatusCode::OK);
    let used = admin.get(&keys_uri(&app, "")).await.json();
    assert!(used["data"][1]["last_used_at"].is_string(), "{used}");

    // Revoked: 401 at once, gone from the list; revoking it again is 404.
    admin.send(Method::DELETE, &keys_uri(&app, &id), None).await.assert_status(StatusCode::NO_CONTENT);
    reader.get("/api/v1/content/settings").await.assert_status(StatusCode::UNAUTHORIZED);
    let after = admin.get(&keys_uri(&app, "")).await.json();
    assert_eq!(after["meta"]["total"], 1);
    assert_eq!(after["data"][0]["name"], "Staging");
    admin.send(Method::DELETE, &keys_uri(&app, &id), None).await.assert_status(StatusCode::NOT_FOUND);
    // The other key still works.
    let staging = forever.json()["token"].as_str().unwrap().to_owned();
    app.client().bearer(&staging).get("/api/v1/content/settings").await.assert_status(StatusCode::OK);

    // Both changes are in the audit log, without the secret.
    let audit = admin.get(&app.admin_api("audit")).await;
    audit.assert_status(StatusCode::OK);
    assert!(audit.text().contains("token.created") && audit.text().contains("token.revoked"), "{}", audit.text());
    assert!(!audit.text().contains(secret_part), "{}", audit.text());

    // The page itself opens.
    admin.get(&app.admin_api("pages/content-api")).await.assert_status(StatusCode::OK);
});

/// Now, as the server's clock gives it.
fn chrono_now() -> sea_orm::prelude::DateTimeUtc {
    rblog::services::content_sync::now()
}

db_test!(revoking_another_users_token_is_not_found, migrate = Migrator, |db: TestDb| async move {
    let app = content::app(db).await;
    let admin = super_admin(&app).await;
    // The frontend user exists (a key was made), yet another user's token is not a Content API key.
    create(&app, &admin, json!({ "name": "Site" })).await.assert_status(StatusCode::CREATED);
    let stranger = UserFactory::new().create(app.state().db()).await;
    let issued =
        rustcms::auth::tokens::issue(app.state().db(), stranger.id, "cli", &["*".to_owned()], None).await.unwrap();
    let uri = keys_uri(&app, &issued.token.id.to_string());
    admin.send(Method::DELETE, &uri, None).await.assert_status(StatusCode::NOT_FOUND);
    let kept = token::Entity::find_by_id(issued.token.id).one(app.state().db()).await.unwrap().unwrap();
    assert_eq!(kept.revoked_at, None, "another user's token is left alone");
    // Nor an id that does not exist.
    admin.send(Method::DELETE, &keys_uri(&app, "999999999"), None).await.assert_status(StatusCode::NOT_FOUND);
});

db_test!(only_integrations_permissions_give_access, migrate = Migrator, |db: TestDb| async move {
    let app = content::app(db).await;
    let anonymous = app.client();
    anonymous.fetch_csrf().await;
    let nobody = holding(&app, "writer", &["categories.view_any"]).await;
    let nobody = app.acting_as(&nobody).await;
    let viewer = holding(&app, "integrations-viewer", &[INTEGRATIONS_VIEW]).await;
    let viewer = app.acting_as(&viewer).await;
    let manager = holding(&app, "integrations-manager", &[INTEGRATIONS_VIEW, INTEGRATIONS_MANAGE]).await;
    let manager = app.acting_as(&manager).await;
    let admin = super_admin(&app).await;
    assert_access(
        &app,
        &routes(),
        &[
            Case::new(Method::GET, &format!("/{KEYS}"))
                .expect("anonymous", &anonymous, StatusCode::UNAUTHORIZED)
                .expect("nobody", &nobody, StatusCode::FORBIDDEN)
                .expect("viewer", &viewer, StatusCode::OK)
                .expect("manager", &manager, StatusCode::OK)
                .expect("super admin", &admin, StatusCode::OK),
            // Allowed callers get 422: `{}` is not a valid body.
            Case::new(Method::POST, &format!("/{KEYS}"))
                .expect("anonymous", &anonymous, StatusCode::UNAUTHORIZED)
                .expect("nobody", &nobody, StatusCode::FORBIDDEN)
                .expect("viewer", &viewer, StatusCode::FORBIDDEN)
                .expect("manager", &manager, StatusCode::UNPROCESSABLE_ENTITY)
                .expect("super admin", &admin, StatusCode::UNPROCESSABLE_ENTITY),
            // Allowed callers get 404: no such key.
            Case::new(Method::DELETE, &format!("/{KEYS}/1"))
                .expect("anonymous", &anonymous, StatusCode::UNAUTHORIZED)
                .expect("nobody", &nobody, StatusCode::FORBIDDEN)
                .expect("viewer", &viewer, StatusCode::FORBIDDEN)
                .expect("manager", &manager, StatusCode::NOT_FOUND)
                .expect("super admin", &admin, StatusCode::NOT_FOUND),
            // Preview links (tests/feature/posts.rs covers them in full): `posts.view` /
            // `pages.view`, then the record; allowed callers get 404, no such post or page.
            Case::new(Method::POST, "/previews/posts/1")
                .expect("anonymous", &anonymous, StatusCode::UNAUTHORIZED)
                .expect("nobody", &nobody, StatusCode::FORBIDDEN)
                .expect("viewer", &viewer, StatusCode::FORBIDDEN)
                .expect("super admin", &admin, StatusCode::NOT_FOUND),
            Case::new(Method::POST, "/previews/pages/1")
                .expect("anonymous", &anonymous, StatusCode::UNAUTHORIZED)
                .expect("nobody", &nobody, StatusCode::FORBIDDEN)
                .expect("manager", &manager, StatusCode::FORBIDDEN)
                .expect("super admin", &admin, StatusCode::NOT_FOUND),
        ],
    )
    .await;

    // A real key, which the refused callers cannot revoke either.
    let made = create(&app, &manager, json!({ "name": "Site" })).await;
    made.assert_status(StatusCode::CREATED);
    let id = made.json()["key"]["id"].as_str().unwrap().to_owned();
    for refused in [&nobody, &viewer] {
        refused.send(Method::DELETE, &keys_uri(&app, &id), None).await.assert_status(StatusCode::FORBIDDEN);
        create(&app, refused, json!({ "name": "Mine" })).await.assert_status(StatusCode::FORBIDDEN);
    }
    nobody.get(&keys_uri(&app, "")).await.assert_status(StatusCode::FORBIDDEN);
    viewer.get(&keys_uri(&app, "")).await.assert_status(StatusCode::OK).assert_json("/meta/total", json!(1));

    // The page follows `integrations.view`.
    let page = app.admin_api("pages/content-api");
    nobody.get(&page).await.assert_status(StatusCode::FORBIDDEN);
    viewer.get(&page).await.assert_status(StatusCode::OK);
});

db_test!(a_key_needs_a_valid_name_and_expiry, migrate = Migrator, |db: TestDb| async move {
    let app = content::app(db).await;
    let admin = super_admin(&app).await;
    for body in [
        json!({ "name": "" }),
        json!({ "name": "   " }),
        json!({ "name": "x".repeat(81) }),
        json!({ "name": "line\nbreak" }),
        json!({ "expires_in_days": 30 }),
    ] {
        create(&app, &admin, body.clone()).await.assert_status(StatusCode::UNPROCESSABLE_ENTITY);
    }
    create(&app, &admin, json!({ "name": "" })).await.assert_validation_error("name");
    create(&app, &admin, json!({ "name": "x".repeat(81) })).await.assert_validation_error("name");
    for days in [0, 7, 60, 366, 3650] {
        let refused = create(&app, &admin, json!({ "name": "Site", "expires_in_days": days })).await;
        refused.assert_status(StatusCode::UNPROCESSABLE_ENTITY).assert_validation_error("expires_in_days");
    }
    // Unknown fields (abilities are always exactly `content.read`).
    let wider = create(&app, &admin, json!({ "name": "Site", "abilities": ["*"] })).await;
    wider.assert_status(StatusCode::UNPROCESSABLE_ENTITY);
    // Nothing was made.
    admin.get(&keys_uri(&app, "")).await.assert_json("/meta/total", json!(0));

    // 80 characters, counted as characters (Vietnamese is multi-byte), and each lifetime.
    let long = "Trang chủ tiếng Việt ".repeat(4)[..].chars().take(80).collect::<String>();
    assert_eq!(long.chars().count(), 80);
    create(&app, &admin, json!({ "name": long })).await.assert_status(StatusCode::CREATED);
    for days in [30, 90, 365] {
        create(&app, &admin, json!({ "name": "Site", "expires_in_days": days }))
            .await
            .assert_status(StatusCode::CREATED);
    }
});

db_test!(first_keys_made_at_once_both_succeed, migrate = Migrator, |db: TestDb| async move {
    let app = content::app(db).await;
    let admin = super_admin(&app).await;
    let other = super_admin(&app).await;
    // No frontend user yet: both requests make it, and neither fails on the other's.
    let (a, b) = tokio::join!(
        create(&app, &admin, json!({ "name": "First" })),
        create(&app, &other, json!({ "name": "Second" })),
    );
    a.assert_status(StatusCode::CREATED);
    b.assert_status(StatusCode::CREATED);
    admin.get(&keys_uri(&app, "")).await.assert_json("/meta/total", json!(2));
    for made in [a, b] {
        let secret = made.json()["token"].as_str().unwrap().to_owned();
        app.client().bearer(&secret).get("/api/v1/content/settings").await.assert_status(StatusCode::OK);
    }
});
