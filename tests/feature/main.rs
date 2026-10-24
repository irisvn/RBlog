//! Feature tests: the application through HTTP, on MySQL 8.4 (`db_test!`,
//! see `.cargo/config.toml`). One test binary; `rustcms make:test <Name>` adds modules.

#[path = "../support/mod.rs"]
mod support;

mod authors;
mod content_api;
mod content_media;
mod content_schema;
mod dashboard;
mod demo_seeder;
mod health;
mod integrations;
mod media_disk;
mod menus;
mod post_media;
mod posts;
mod publishing;
mod revisions;
mod rich_text;
mod settings;
mod taxonomy;
mod webhook;
mod webhook_job;
// rustcms:tests
