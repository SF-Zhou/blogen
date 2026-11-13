//! Heading anchors, following `markdown-it-anchor` 9.2.1 with the custom
//! `renderPermalink` used by the reference pipeline.
//!
//! For every heading markdown-it-anchor:
//!
//! 1. computes the heading text from `text`/`code_inline` token contents,
//! 2. slugifies it (`trim().to_lowercase()` with whitespace runs replaced by
//!    `-`),
//! 3. de-duplicates slugs by appending `-1`, `-2`, ... (the suffix counter is
//!    independent per base slug and keeps counting while slugs collide),
//! 4. sets `id="<slug>"` and `tabindex="-1"` attributes,
//! 5. runs the blog's custom permalink renderer, which wraps *all* heading
//!    content in `<a href="#<percent-encoded-slug>">…</a>`.

use std::collections::HashSet;

use crate::model::Heading;

use super::tokens::{Token, TokenType};
use super::util::get_tokens_text;
use super::{encode_uri_component, slugify};

/// Add anchors to all headings and collect them for the [`Heading`] list.
pub(crate) fn apply(tokens: &mut [Token], headings: &mut Vec<Heading>) {
    let mut used: HashSet<String> = HashSet::new();
    let mut index = 0;
    while index < tokens.len() {
        if tokens[index].typ != TokenType::HeadingOpen {
            index += 1;
            continue;
        }
        let level = heading_level(tokens[index].tag);
        let text = match tokens.get(index + 1) {
            Some(inline) if inline.typ == TokenType::Inline => get_tokens_text(&inline.children),
            _ => String::new(),
        };
        let slug = unique_slug(&slugify(&text), &mut used);
        tokens[index].attr_push("id", slug.clone());
        tokens[index].attr_push("tabindex", "-1");
        let href = format!("#{}", encode_uri_component(&slug));
        if let Some(inline) = tokens.get_mut(index + 1) {
            if inline.typ == TokenType::Inline {
                let mut open = Token::inline_open(TokenType::LinkOpen, "a");
                open.attr_push("href", href);
                inline.children.insert(0, open);
                inline
                    .children
                    .push(Token::inline_close(TokenType::LinkClose, "a"));
            }
        }
        headings.push(Heading {
            level,
            id: slug,
            text: text.trim().to_string(),
        });
        index += 2;
    }
}

/// First free slug: `base`, then `base-1`, `base-2`, ...
fn unique_slug(base: &str, used: &mut HashSet<String>) -> String {
    if used.insert(base.to_string()) {
        return base.to_string();
    }
    let mut suffix = 1u64;
    loop {
        let candidate = format!("{base}-{suffix}");
        if used.insert(candidate.clone()) {
            return candidate;
        }
        suffix += 1;
    }
}

/// Numeric level of an `h1`..`h6` tag.
pub(crate) fn heading_level(tag: &str) -> u8 {
    tag.as_bytes()
        .get(1)
        .and_then(|byte| byte.checked_sub(b'0'))
        .filter(|level| (1..=6).contains(level))
        .unwrap_or(1)
}
