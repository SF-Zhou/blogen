//! `[TOC]` support, following `markdown-it-table-of-contents` 1.2.0 with the
//! reference pipeline's options:
//!
//! ```js
//! md.use(toc, {
//!   markerPattern: /^\[TOC\]/im,
//!   includeLevel: [1, 2, 3],
//!   slugify: s => String(s).trim().toLowerCase().replace(/\s+/g, '-'),
//! })
//! ```
//!
//! The plugin is a block rule that replaces a marker line with
//! `toc_open` / `toc_body` / `toc_close` tokens; `toc_body` is rendered from
//! *all* document headings (including headings after the marker). This module
//! implements the source pre-pass that isolates marker lines as paragraphs
//! (returning placeholders for the parser), the heading collection, the exact
//! flat -> nested tree algorithm of the plugin and its HTML serialisation.

use super::tokens::{Token, TokenType};
use super::util::get_tokens_text;
use super::{parser, render};

/// Placeholder payload that marks a `[TOC]` line in the pre-processed source
/// (the surrounding U+E000/U+E001 markers are shared with math placeholders,
/// see [`crate::markdown`]).
pub(crate) const PLACEHOLDER: &str = "\u{E000}toc\u{E001}";

/// Replace `[TOC]` marker lines with isolated placeholder paragraphs.
///
/// Marker lines inside fenced code blocks are left alone; up to three leading
/// spaces are allowed, matching the block rule's `tShift`.
pub(crate) fn protect(markdown: &str) -> String {
    let mut out = String::with_capacity(markdown.len() + 16);
    let mut fence: Option<(char, usize)> = None;
    for line in markdown.split_inclusive('\n') {
        let body = line.strip_suffix('\n').unwrap_or(line);
        if let Some((marker, length)) = fence {
            out.push_str(line);
            if is_closing_fence(body, marker, length) {
                fence = None;
            }
            continue;
        }
        if let Some((marker, length)) = opening_fence(body) {
            fence = Some((marker, length));
            out.push_str(line);
            continue;
        }
        if is_toc_marker(body) {
            out.push_str("\n\n");
            out.push_str(PLACEHOLDER);
            out.push_str("\n\n");
        } else {
            out.push_str(line);
        }
    }
    out
}

/// The marker pattern `/^\[TOC\]/im` applied to the line's content (up to three
/// leading spaces, case-insensitive).
fn is_toc_marker(line: &str) -> bool {
    let trimmed = line.trim_end_matches([' ', '\t', '\r']);
    let content = trimmed.trim_start_matches(' ');
    if trimmed.len() - content.len() > 3 {
        return false;
    }
    content
        .as_bytes()
        .get(..5)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(b"[TOC]"))
}

/// Detect a CommonMark code fence opener; returns `(marker, length)`.
fn opening_fence(line: &str) -> Option<(char, usize)> {
    let content = line.trim_start_matches(' ');
    if line.len() - content.len() > 3 {
        return None;
    }
    let marker = content.chars().next()?;
    if marker != '`' && marker != '~' {
        return None;
    }
    let length = content.chars().take_while(|&ch| ch == marker).count();
    if length < 3 {
        return None;
    }
    if marker == '`' && content[length..].contains('`') {
        return None;
    }
    Some((marker, length))
}

/// Detect a code fence closer for the currently open fence.
fn is_closing_fence(line: &str, marker: char, min_length: usize) -> bool {
    let content = line.trim_start_matches(' ');
    if line.len() - content.len() > 3 {
        return false;
    }
    let length = content.chars().take_while(|&ch| ch == marker).count();
    if length < min_length {
        return false;
    }
    content[length..].trim_matches([' ', '\t', '\r']).is_empty()
}

/// Replace placeholder paragraphs with TOC tokens using the document's
/// headings (the token tree must already have heading anchors applied).
pub(crate) fn apply(tokens: &mut Vec<Token>) {
    let body = build(tokens);
    let mut out = Vec::with_capacity(tokens.len() + 3);
    let mut index = 0;
    while index < tokens.len() {
        if is_toc_paragraph(tokens, index) {
            out.push(Token::open(TokenType::TocOpen, "div"));
            let mut token = Token::new(TokenType::TocBody, "", 0, false, "");
            token.raw = Some(body.clone());
            out.push(token);
            out.push(Token::close(TokenType::TocClose, "div"));
            index += 3;
        } else {
            out.push(tokens[index].clone());
            index += 1;
        }
    }
    *tokens = out;
}

/// True when `tokens[index..index + 3]` is a paragraph containing only the TOC
/// placeholder.
fn is_toc_paragraph(tokens: &[Token], index: usize) -> bool {
    if index + 2 >= tokens.len() {
        return false;
    }
    if tokens[index].typ != TokenType::ParagraphOpen
        || tokens[index + 2].typ != TokenType::ParagraphClose
    {
        return false;
    }
    let inline = &tokens[index + 1];
    inline.typ == TokenType::Inline
        && inline.children.len() == 1
        && inline.children[0].typ == TokenType::Text
        && inline.children[0].content == PLACEHOLDER
}

/// One heading collected for the table of contents.
struct Headline {
    level: i64,
    text: String,
    anchor: Option<String>,
}

/// A node of the nested TOC tree (arena-based to mirror the plugin's parent
/// pointers).
struct Node {
    level: i64,
    text: String,
    anchor: Option<String>,
    children: Vec<usize>,
    parent: Option<usize>,
}

/// Collect headings with levels 1-3, exactly like the plugin's
/// `findHeadlineElements` (text is the trimmed concatenation of text and
/// code_inline contents, anchor is the heading's existing `id`).
fn collect_headlines(tokens: &[Token]) -> Vec<Headline> {
    let mut headlines = Vec::new();
    let mut index = 0;
    while index < tokens.len() {
        if tokens[index].typ == TokenType::HeadingOpen {
            let level = i64::from(super::anchors::heading_level(tokens[index].tag));
            if (1..=3).contains(&level) {
                let inline = tokens.get(index + 1);
                let text = inline
                    .filter(|token| token.typ == TokenType::Inline)
                    .map(|token| get_tokens_text(&token.children))
                    .unwrap_or_default();
                let anchor = tokens[index].attr_get("id").map(str::to_string);
                headlines.push(Headline {
                    level,
                    text: text.trim().to_string(),
                    anchor,
                });
            }
            index += 2;
        } else {
            index += 1;
        }
    }
    headlines
}

/// Port of the plugin's `flatHeadlineItemsToNestedTree`.
fn build_tree(headlines: &[Headline]) -> Vec<Node> {
    let min_level = headlines
        .iter()
        .map(|headline| headline.level)
        .min()
        .unwrap_or(1);
    let mut nodes = vec![Node {
        level: min_level - 1,
        text: String::new(),
        anchor: None,
        children: Vec::new(),
        parent: None,
    }];
    let mut current_root = 0usize;
    let mut prev = 0usize;
    for headline in headlines {
        match headline.level.cmp(&nodes[prev].level) {
            std::cmp::Ordering::Greater => {
                for _ in 0..(headline.level - nodes[prev].level) {
                    current_root = prev;
                    prev = add_node(&mut nodes, headline.level, "", None, current_root);
                }
                nodes[prev].text.clone_from(&headline.text);
                nodes[prev].anchor.clone_from(&headline.anchor);
            }
            std::cmp::Ordering::Equal => {
                prev = add_node(
                    &mut nodes,
                    headline.level,
                    &headline.text,
                    headline.anchor.clone(),
                    current_root,
                );
            }
            std::cmp::Ordering::Less => {
                for _ in 0..(nodes[prev].level - headline.level) {
                    if let Some(parent) = nodes[current_root].parent {
                        current_root = parent;
                    }
                }
                prev = add_node(
                    &mut nodes,
                    headline.level,
                    &headline.text,
                    headline.anchor.clone(),
                    current_root,
                );
            }
        }
    }
    nodes
}

/// Add a list item below `parent` and return its arena index.
fn add_node(
    nodes: &mut Vec<Node>,
    level: i64,
    text: &str,
    anchor: Option<String>,
    parent: usize,
) -> usize {
    let index = nodes.len();
    nodes.push(Node {
        level,
        text: text.to_string(),
        anchor,
        children: Vec::new(),
        parent: Some(parent),
    });
    nodes[parent].children.push(index);
    index
}

/// Render the TOC tree to `<ul>…</ul>` exactly like `tocItemToHtml`.
fn render_tree(nodes: &[Node], index: usize) -> String {
    let mut out = String::from("<ul>");
    for &child in &nodes[index].children {
        let node = &nodes[child];
        out.push_str("<li>");
        let text = if node.text.is_empty() {
            None
        } else {
            Some(render_inline_text(&node.text))
        };
        match &node.anchor {
            Some(anchor) => {
                out.push_str("<a href=\"#");
                out.push_str(anchor);
                out.push_str("\">");
                if let Some(text) = &text {
                    out.push_str(text);
                }
                out.push_str("</a>");
            }
            None => {
                if let Some(text) = &text {
                    out.push_str(text);
                }
            }
        }
        if !node.children.is_empty() {
            out.push_str(&render_tree(nodes, child));
        }
        out.push_str("</li>");
    }
    out.push_str("</ul>");
    out
}

/// `md.renderInline(text)`: parse the text as inline markdown and render it.
fn render_inline_text(text: &str) -> String {
    let children = parser::parse_inline_fragment(text);
    render::render_inline_to_string(&children)
}

/// Build the TOC HTML for the current heading set.
fn build(tokens: &[Token]) -> String {
    let headlines = collect_headlines(tokens);
    if headlines.is_empty() {
        return "<ul></ul>".to_string();
    }
    let nodes = build_tree(&headlines);
    render_tree(&nodes, 0)
}
