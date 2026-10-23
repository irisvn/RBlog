//! rblog: a rustcms application. Where things live:
//! `bootstrap` assembles the app from `providers`; `routes` declares the API,
//! served by `http`; `models`, `services` and `policies` hold the domain;
//! `admin` the panel; `console` the commands; `database` migrations, seeders and factories.

pub mod admin;
pub mod bootstrap;
pub mod console;
pub mod database;
pub mod http;
pub mod models;
pub mod policies;
pub mod providers;
pub mod routes;
pub mod services;
pub mod settings;
// rustcms:mods
