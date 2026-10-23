//! A new Content API key, from the Integrations page (`POST {admin}/api/x/integrations/content-keys`).

use rustcms::prelude::*;
use serde::Deserialize;

/// The longest key name, in characters.
pub const MAX_NAME: usize = 80;
/// The lifetimes a key may be given, in days (`None`: until revoked).
pub const LIFETIMES: [u32; 3] = [30, 90, 365];

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewContentKey {
    /// What the key is for ("Production site"), trimmed.
    pub name: String,
    /// One of [`LIFETIMES`], or `null` for a key that does not expire.
    #[serde(default)]
    pub expires_in_days: Option<u32>,
}

impl Validate for NewContentKey {
    fn validate(&mut self) -> Result<(), ValidationErrors> {
        let mut errors = ValidationErrors::new();
        self.name = self.name.trim().to_owned();
        if self.name.is_empty() {
            errors.add("name", "is required");
        } else if self.name.chars().count() > MAX_NAME {
            errors.add("name", format!("may not be longer than {MAX_NAME} characters"));
        } else if self.name.chars().any(char::is_control) {
            errors.add("name", "is invalid");
        }
        if self.expires_in_days.is_some_and(|days| !LIFETIMES.contains(&days)) {
            errors.add("expires_in_days", "is invalid");
        }
        if errors.is_empty() { Ok(()) } else { Err(errors) }
    }
}
