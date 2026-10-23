//! Helpers shared by the test suites.

#![allow(dead_code)]

use rblog::bootstrap;
use rustcms_testing::{TestApp, TestDb};

/// The application on `db` (migrated by `db_test!`).
pub async fn app(db: TestDb) -> TestApp {
    TestApp::new(bootstrap::build, db).await
}
