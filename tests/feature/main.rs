//! Feature tests: the application through HTTP, on MySQL 8.4 (`db_test!`,
//! see `.cargo/config.toml`). One test binary; `rustcms make:test <Name>` adds modules.

#[path = "../support/mod.rs"]
mod support;

mod content_schema;
mod health;
mod media_disk;
mod menus;
mod settings;
mod taxonomy;
mod webhook;
mod webhook_job;
// rustcms:tests
