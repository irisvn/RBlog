//! Dashboard widgets (`admin::dashboard`) on the demo data: their numbers, and who sees them.
//! The post widgets: tests/feature/posts.rs.

use rblog::database::migrations::Migrator;
use rblog::database::seeders::demo_seeder::DemoSeeder;
use rustcms::axum::http::StatusCode;
use rustcms::database::Seeder;
use rustcms_testing::{Factory, TestDb, UserFactory, db_test};
use serde_json::json;

use crate::support::{self, holding};
use crate::taxonomy::admin;

db_test!(the_authors_widget_counts_profiles, migrate = Migrator, |db: TestDb| async move {
    let app = support::app(db).await;
    DemoSeeder.run(app.state()).await.unwrap();
    // A user without a profile is no author.
    UserFactory::new().name("Khách").create(app.state().db()).await;
    let panel = admin(&app).await;

    let dashboard = panel.get(&app.admin_api("dashboard")).await;
    dashboard.assert_status(StatusCode::OK).assert_json("/widgets/0/name", json!("authors"));
    dashboard.assert_json("/widgets/0/kind", json!("stat")).assert_json("/widgets/0/resource", json!("authors"));
    panel
        .get(&app.admin_api("dashboard/widgets/authors"))
        .await
        .assert_status(StatusCode::OK)
        .assert_json("", json!({"kind": "stat", "value": "3"}));
    // Labels in the user's language.
    panel.languages("vi").get(&app.admin_api("dashboard")).await.assert_json("/widgets/0/label", json!("Tác giả"));
});

db_test!(a_widget_needs_its_permission_and_its_resource, migrate = Migrator, |db: TestDb| async move {
    let app = support::app(db).await;
    DemoSeeder.run(app.state()).await.unwrap();
    let data = app.admin_api("dashboard/widgets/authors");
    let listed = async |client: &rustcms_testing::TestClient<'_>| -> usize {
        let view = client.get(&app.admin_api("dashboard")).await;
        view.assert_status(StatusCode::OK);
        view.json()["widgets"].as_array().unwrap().len()
    };

    // The widget's permission alone: not shown, data refused.
    let widget_only = holding(&app, "widget-only", &["dashboard.authors"]).await;
    let widget_only = app.acting_as(&widget_only).await;
    assert_eq!(listed(&widget_only).await, 0);
    widget_only.get(&data).await.assert_status(StatusCode::FORBIDDEN);
    // The resource alone: the same.
    let resource_only = holding(&app, "resource-only", &["authors.view_any", "authors.view"]).await;
    let resource_only = app.acting_as(&resource_only).await;
    assert_eq!(listed(&resource_only).await, 0);
    resource_only.get(&data).await.assert_status(StatusCode::FORBIDDEN);
    // Both: shown, with the number.
    let both = holding(&app, "both", &["dashboard.authors", "authors.view_any", "authors.view"]).await;
    let both = app.acting_as(&both).await;
    assert_eq!(listed(&both).await, 1);
    both.get(&data).await.assert_status(StatusCode::OK).assert_json("/value", json!("3"));
    // An unknown widget.
    both.get(&app.admin_api("dashboard/widgets/nope")).await.assert_status(StatusCode::NOT_FOUND);
});
