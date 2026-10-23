//! The admin panel at `/admin` (like a Filament panel provider): its
//! resources (`rustcms make:resource`), pages (`make:settings`) and labels.

use rustcms::admin::Theme;
use rustcms::prelude::*;

pub fn provider() -> AdminProvider {
    AdminProvider::new()
        .brand("rblog")
        // Status tones (docs/ui-guidelines.md §1): RustCMS defaults them to grays.
        // Each keeps at least 3:1 against `--card` (5.0:1 light, 10.8 and 11.3:1 dark).
        .theme(
            Theme::new()
                .color("success", "#15803d")
                .color("warning", "#b45309")
                .dark(|dark| dark.color("success", "#4ade80").color("warning", "#fbbf24")),
        )
        // Labels in Vietnamese (`rustcms lang:missing vi` lists what is missing).
        .translations("vi", include_str!("../../lang/vi.toml"))
    // rustcms:resources
    // rustcms:pages
    // rustcms:extensions
}
