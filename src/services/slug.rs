//! Slugs: ASCII, lowercase, words joined by `-` (`xin-chao-viet-nam`). The database compares
//! text with `utf8mb4_0900_bin` (accent-sensitive), so slugs carry no diacritics at all.

use sea_orm::{ColumnTrait, ConnectionTrait, DbErr, EntityTrait, QueryFilter, QuerySelect};

use crate::models::{author_profile, category, page, post, tag};

/// The longest slug `slugify` makes (the `posts.slug` and `pages.slug` column width).
pub const MAX_LEN: usize = 200;

/// The base letter of a lowercase Vietnamese (or common Latin) letter with diacritics, the
/// character itself otherwise, and `None` for a combining mark (decomposed text: `o` + U+0323),
/// which is dropped.
pub(crate) fn fold(c: char) -> Option<char> {
    const GROUPS: &[(&str, char)] = &[
        ("àáạảãâầấậẩẫăằắặẳẵäåā", 'a'),
        ("èéẹẻẽêềếệểễëē", 'e'),
        ("ìíịỉĩïîī", 'i'),
        ("òóọỏõôồốộổỗơờớợởỡöøō", 'o'),
        ("ùúụủũưừứựửữüûū", 'u'),
        ("ỳýỵỷỹÿ", 'y'),
        ("đ", 'd'),
        ("ç", 'c'),
        ("ñ", 'n'),
    ];
    if ('\u{0300}'..='\u{036f}').contains(&c) {
        return None;
    }
    Some(GROUPS.iter().find(|(set, _)| set.contains(c)).map_or(c, |(_, plain)| *plain))
}

/// `title` as a slug: diacritics removed (`đ` → `d`), ASCII letters and digits kept, every other
/// run of characters a single `-`, no `-` at either end, at most [`MAX_LEN`] characters. Empty
/// when `title` has no letter or digit.
pub fn slugify(title: &str) -> String {
    let mut out = String::new();
    for c in title.to_lowercase().chars().filter_map(fold) {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    // ASCII only, so bytes are characters.
    out.truncate(MAX_LEN);
    out.trim_end_matches('-').to_owned()
}

/// A table with a unique slug column.
pub trait Slugged: EntityTrait {
    /// The slug column.
    const SLUG: Self::Column;
    /// The primary key (an `i64`).
    const ID: Self::Column;
    /// The slug column's width.
    const MAX_LEN: usize;
}

impl Slugged for post::Entity {
    const SLUG: post::Column = post::Column::Slug;
    const ID: post::Column = post::Column::Id;
    const MAX_LEN: usize = 200;
}

impl Slugged for page::Entity {
    const SLUG: page::Column = page::Column::Slug;
    const ID: page::Column = page::Column::Id;
    const MAX_LEN: usize = 200;
}

impl Slugged for category::Entity {
    const SLUG: category::Column = category::Column::Slug;
    const ID: category::Column = category::Column::Id;
    const MAX_LEN: usize = 160;
}

impl Slugged for tag::Entity {
    const SLUG: tag::Column = tag::Column::Slug;
    const ID: tag::Column = tag::Column::Id;
    const MAX_LEN: usize = 160;
}

impl Slugged for author_profile::Entity {
    const SLUG: author_profile::Column = author_profile::Column::Slug;
    const ID: author_profile::Column = author_profile::Column::UserId;
    const MAX_LEN: usize = 160;
}

/// A slug for `base` (slugified; `untitled` when that is empty) that no row of `E` has, trashed
/// rows included, except the row `except_id` (the one being saved): `base`, then `base-2`,
/// `base-3`, … Shortened to fit the column.
///
/// Call it inside the write's transaction. Two writers can still pick the same slug at once:
/// the unique index refuses the second.
pub async fn unique_slug<E: Slugged>(
    db: &impl ConnectionTrait,
    _table: E,
    base: &str,
    except_id: Option<i64>,
) -> Result<String, DbErr> {
    let base = match slugify(base) {
        slug if slug.is_empty() => "untitled".to_owned(),
        slug => slug,
    };
    for n in 1u64.. {
        let candidate = if n == 1 { fit(&base, "", E::MAX_LEN) } else { fit(&base, &format!("-{n}"), E::MAX_LEN) };
        let mut taken = E::find().select_only().column(E::ID).filter(E::SLUG.eq(candidate.as_str()));
        if let Some(id) = except_id {
            taken = taken.filter(E::ID.ne(id));
        }
        if taken.into_tuple::<i64>().one(db).await?.is_none() {
            return Ok(candidate);
        }
    }
    unreachable!("u64 suffixes do not run out")
}

/// `base` shortened so that `base` + `suffix` fits `max` characters (ASCII), without a `-` left
/// before the suffix.
fn fit(base: &str, suffix: &str, max: usize) -> String {
    let base = &base[..base.len().min(max - suffix.len())];
    format!("{}{suffix}", base.trim_end_matches('-'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_removes_vietnamese_diacritics() {
        assert_eq!(slugify("Xin chào Việt Nam"), "xin-chao-viet-nam");
        assert_eq!(slugify("Đường đến trường ĐẸP"), "duong-den-truong-dep");
        assert_eq!(slugify("Phở bò & bún chả!"), "pho-bo-bun-cha");
        // Decomposed text: base letter plus combining marks.
        assert_eq!(slugify("Vie\u{0302}\u{0323}t"), "viet");
        assert_eq!(slugify("Café crème, ñandú"), "cafe-creme-nandu");
    }

    #[test]
    fn slugify_joins_and_trims_separators() {
        assert_eq!(slugify("  --Rust   2024 -- "), "rust-2024");
        assert_eq!(slugify("a__b..c"), "a-b-c");
        assert_eq!(slugify("日本語"), "");
        assert_eq!(slugify(""), "");
    }

    #[test]
    fn slugify_stops_at_max_len_without_a_trailing_dash() {
        let slug = slugify(&"ab ".repeat(150));
        assert!(slug.len() <= MAX_LEN, "{}", slug.len());
        assert!(!slug.ends_with('-'));
        // Cut exactly on a separator.
        let slug = slugify(&format!("{} b", "a".repeat(MAX_LEN)));
        assert_eq!(slug, "a".repeat(MAX_LEN));
        let slug = slugify(&format!("{} bcd", "a".repeat(MAX_LEN - 1)));
        assert_eq!(slug, "a".repeat(MAX_LEN - 1));
    }

    #[test]
    fn fit_keeps_the_suffix_within_the_column() {
        assert_eq!(fit("hello", "-2", 200), "hello-2");
        assert_eq!(fit("abcde", "-12", 6), "abc-12");
        assert_eq!(fit("ab-cd", "-2", 5), "ab-2");
    }
}
