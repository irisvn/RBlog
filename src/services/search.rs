//! Accent-insensitive search. The database compares text with `utf8mb4_0900_bin`
//! (accent-sensitive), so posts and pages keep a normalized `search_text`, and the query goes
//! through the same [`normalize`] before a `LIKE`.

use rustcms::admin::rich_text::{self, Node};
use serde_json::Value;

use super::slug::fold;

/// The longest query [`like_pattern`] accepts, in characters after [`normalize`].
pub const MAX_QUERY_CHARS: usize = 100;

/// `text` lowercase, without diacritics (the mapping of `slug::slugify`), with every run of
/// whitespace a single space and none at either end. Punctuation is kept.
pub fn normalize(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.to_lowercase().chars().filter_map(fold) {
        if c.is_whitespace() {
            if !out.is_empty() && !out.ends_with(' ') {
                out.push(' ');
            }
        } else {
            out.push(c);
        }
    }
    out.truncate(out.trim_end().len());
    out
}

/// The text of a RustCMS rich-text document, one line per block: code blocks, table cells,
/// callouts and image captions included; image alt texts, embeds and anything `Node` gains in a
/// later RustCMS left out. Empty when `document` is not a valid document (version 1 or 2).
pub fn plain_text(document: &Value) -> String {
    fn walk(nodes: &[Node], out: &mut String) {
        for node in nodes {
            match node {
                Node::Text(text, _) => out.push_str(text),
                Node::HardBreak => out.push(' '),
                Node::CodeBlock(code) => line(out, code),
                Node::Paragraph(children) | Node::Heading(_, children) => {
                    walk(children, out);
                    out.push('\n');
                }
                Node::BulletList(children)
                | Node::OrderedList(children)
                | Node::ListItem(children)
                | Node::Blockquote(children)
                | Node::Callout(_, children) => walk(children, out),
                Node::Table(table) => {
                    for cell in table.rows.iter().flatten() {
                        walk(&cell.content, out);
                    }
                }
                Node::Image(image) if !image.caption.is_empty() => line(out, &image.caption),
                _ => {}
            }
        }
    }
    fn line(out: &mut String, text: &str) {
        out.push_str(text);
        out.push('\n');
    }
    let Ok(document) = rich_text::parse(document) else {
        return String::new();
    };
    let mut out = String::new();
    walk(&document.content, &mut out);
    out.truncate(out.trim_end().len());
    out
}

/// The `LIKE` pattern matching `search_text` that contains `q`: normalized, `\`, `%` and `_`
/// escaped (`\` is MySQL's default `LIKE` escape), wrapped in `%`. `None` when `q` is empty or
/// longer than [`MAX_QUERY_CHARS`] once normalized.
pub fn like_pattern(q: &str) -> Option<String> {
    let q = normalize(q);
    if q.is_empty() || q.chars().count() > MAX_QUERY_CHARS {
        return None;
    }
    let mut pattern = String::with_capacity(q.len() + 2);
    pattern.push('%');
    for c in q.chars() {
        if matches!(c, '\\' | '%' | '_') {
            pattern.push('\\');
        }
        pattern.push(c);
    }
    pattern.push('%');
    Some(pattern)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn normalize_folds_case_diacritics_and_whitespace() {
        assert_eq!(normalize("  Xin   CHÀO\tViệt\nNam  "), "xin chao viet nam");
        assert_eq!(normalize("Đà Nẵng, 2024!"), "da nang, 2024!");
        assert_eq!(normalize("Vie\u{0302}\u{0323}t"), "viet");
        assert_eq!(normalize(" \n "), "");
    }

    #[test]
    fn plain_text_extracts_the_document_text() {
        let document = json!({"type": "doc", "version": 1, "content": [
            {"type": "heading", "attrs": {"level": 2}, "content": [{"type": "text", "text": "Tiêu đề"}]},
            {"type": "paragraph", "content": [
                {"type": "text", "text": "Xin "},
                {"type": "text", "text": "chào", "marks": [{"type": "bold"}]},
                {"type": "hardBreak"},
                {"type": "text", "text": "bạn"}
            ]},
            {"type": "bulletList", "content": [
                {"type": "listItem", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "một"}]}]},
                {"type": "listItem", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "hai"}]}]}
            ]},
            {"type": "image", "attrs": {"media": "0199d3a4-1b2c-7d3e-8f40-123456789abc", "alt": "ảnh"}},
            {"type": "codeBlock", "content": [{"type": "text", "text": "let x = 1;"}]},
            {"type": "horizontalRule"}
        ]});
        assert_eq!(plain_text(&document), "Tiêu đề\nXin chào bạn\nmột\nhai\nlet x = 1;");
        assert_eq!(normalize(&plain_text(&document)), "tieu de xin chao ban mot hai let x = 1;");
    }

    #[test]
    fn plain_text_reads_tables_callouts_and_captions() {
        let cell = |kind: &str, text: &str| json!({"type": kind, "content": [{"type": "paragraph", "content": [{"type": "text", "text": text}]}]});
        let document = json!({"type": "doc", "version": 2, "content": [
            {"type": "table", "content": [
                {"type": "tableRow", "content": [cell("tableHeader", "Gói"), cell("tableHeader", "Giá")]},
                {"type": "tableRow", "content": [cell("tableCell", "Phở bò"), cell("tableCell", "50k")]}
            ]},
            {"type": "callout", "attrs": {"tone": "warn"}, "content": [
                {"type": "paragraph", "content": [{"type": "text", "text": "Lưu ý"}]}
            ]},
            {"type": "image", "attrs": {"media": "0199d3a4-1b2c-7d3e-8f40-123456789abc", "alt": "ảnh", "caption": "Chú thích"}},
            {"type": "image", "attrs": {"media": "0199d3a4-1b2c-7d3e-8f40-123456789abd", "alt": "không chú thích"}},
            {"type": "embed", "attrs": {"provider": "youtube", "id": "dQw4w9WgXcQ"}}
        ]});
        assert_eq!(plain_text(&document), "Gói\nGiá\nPhở bò\n50k\nLưu ý\nChú thích");
    }

    #[test]
    fn plain_text_of_an_invalid_document_is_empty() {
        assert_eq!(plain_text(&json!("<p>html</p>")), "");
        assert_eq!(plain_text(&json!({"type": "doc", "version": 1, "content": []})), "");
        assert_eq!(plain_text(&Value::Null), "");
    }

    #[test]
    fn like_pattern_escapes_wildcards() {
        assert_eq!(like_pattern("  Phở  Bò "), Some("%pho bo%".to_owned()));
        assert_eq!(like_pattern("50%_off\\x"), Some("%50\\%\\_off\\\\x%".to_owned()));
        assert_eq!(like_pattern("   "), None);
        assert_eq!(like_pattern(""), None);
        assert_eq!(like_pattern(&"ệ".repeat(MAX_QUERY_CHARS)), Some(format!("%{}%", "e".repeat(MAX_QUERY_CHARS))));
        assert_eq!(like_pattern(&"a".repeat(MAX_QUERY_CHARS + 1)), None);
        // Whitespace collapses before the length counts.
        assert_eq!(like_pattern(&format!("{}     ", "a".repeat(MAX_QUERY_CHARS))).map(|p| p.len()), Some(102));
    }
}
