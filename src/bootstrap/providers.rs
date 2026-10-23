//! The application's service providers, in order (like `bootstrap/providers.php`).

use rustcms::prelude::*;

use crate::providers;

pub fn register(app: &mut AppBuilder, api: &ApiRoutes) {
    // `[storage]` (config/storage.toml): before the admin panel, whose media library keeps its files there.
    app.provider(rustcms::storage::StorageProvider);
    app.provider(providers::auth_service_provider::provider(api));
    app.provider(providers::admin_panel_provider::provider());
    app.provider(providers::app_service_provider::AppServiceProvider);
}
