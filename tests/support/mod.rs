//! Helpers shared by the test suites.

#![allow(dead_code)]

use rblog::bootstrap;
use rustcms::auth::{ADMIN_GUARD, Actor, AuthState, RbacService};
use rustcms::models::user;
use rustcms_testing::{Factory, TestApp, TestDb, UserFactory};

/// The application on `db` (migrated by `db_test!`).
pub async fn app(db: TestDb) -> TestApp {
    TestApp::new(bootstrap::build, db).await
}

/// A user holding only `permissions` in the admin guard, through the role `role`.
pub async fn holding(app: &TestApp, role: &str, permissions: &[&str]) -> user::Model {
    let auth = app.state().service::<AuthState>().unwrap();
    let db = app.state().db();
    let rbac = RbacService::new(db, &auth.resolver);
    rbac.sync_permissions(&auth.permissions.all()).await.unwrap();
    let permissions: Vec<String> = permissions.iter().map(|p| (*p).to_owned()).collect();
    let role = rbac.create_role(Actor::System, ADMIN_GUARD, role, &permissions).await.unwrap();
    let user = UserFactory::new().create(db).await;
    rbac.assign_role(Actor::System, user.id, role.id).await.unwrap();
    user
}

pub mod content;
pub mod posts;
