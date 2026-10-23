//! HTTP handlers of the application (the admin panel's are built in):
//! `controllers` (handlers), `requests` (validated input), `resources`
//! (records as JSON). Routes are declared in `crate::routes`.

pub mod controllers;
pub mod requests;
pub mod resources;
// rustcms:http-mods
