//! HTML renderer for the markdown-it-like token tree.
//!
//! This is a faithful port of `markdown-it`'s `Renderer` (`renderer.mjs`):
//! block tokens are rendered from a flat list, inline tokens recursively, and
//! the newline placement rules (`renderToken`) are reproduced exactly. That
//! byte-level detail matters because it decides e.g. `<li>\n<p>…</p>\n</li>`
//! versus `<li><p>…</p></li>` and the newline before a nested list.
//!
//! Feature tokens (fences, math, mermaid, TOC) carry pre-rendered HTML and are
//! emitted verbatim.

use super::tokens::{Token, TokenType};
use super::util::{escape_html, render_inline_as_text};

/// Render block tokens to HTML.
pub(crate) fn render(tokens: &[Token]) -> String {
    let mut out = String::new();
    for (index, token) in tokens.iter().enumerate() {
        if token.typ == TokenType::Inline {
            render_inline(&token.children, &mut out);
        } else if let Some(rule) = rule(token) {
            out.push_str(&rule);
        } else {
            render_token(tokens, index, &mut out);
        }
    }
    out
}

/// Render a list of inline children to a string (used by the TOC renderer).
pub(crate) fn render_inline_to_string(children: &[Token]) -> String {
    let mut out = String::new();
    render_inline(children, &mut out);
    out
}

/// Render inline tokens to HTML (appended to `out`).
///
/// Containers (emphasis, links, figcaption) are stored as an opening token with
/// `children`, followed by the matching closing token as the next sibling, so
/// the loop naturally emits `<em>`, the children, then `</em>`.
pub(crate) fn render_inline(tokens: &[Token], out: &mut String) {
    for (index, token) in tokens.iter().enumerate() {
        if let Some(rule) = rule(token) {
            out.push_str(&rule);
            continue;
        }
        if token.typ == TokenType::Image {
            render_image(token, out);
            continue;
        }
        if token.typ == TokenType::Inline {
            render_inline(&token.children, out);
            continue;
        }
        render_token(tokens, index, out);
        if !token.children.is_empty() {
            render_inline(&token.children, out);
        }
    }
}

/// Custom renderer rules for tokens that are not plain tags.
fn rule(token: &Token) -> Option<String> {
    match token.typ {
        TokenType::Text => Some(escape_html(&token.content)),
        TokenType::CodeInline => {
            let mut out = String::from("<code");
            out.push_str(&render_attrs(&token.attrs));
            out.push('>');
            out.push_str(&escape_html(&token.content));
            out.push_str("</code>");
            Some(out)
        }
        TokenType::CodeBlock => {
            // Indented code blocks are never highlighted (markdown-it's
            // `code_block` rule).
            let mut out = String::from("<pre><code");
            out.push_str(&render_attrs(&token.attrs));
            out.push('>');
            out.push_str(&escape_html(&token.content));
            out.push_str("</code></pre>\n");
            Some(out)
        }
        TokenType::Fence | TokenType::MermaidFigure => {
            let mut out = token.raw.clone().unwrap_or_default();
            out.push('\n');
            Some(out)
        }
        TokenType::Hardbreak => Some("<br>\n".to_string()),
        TokenType::Softbreak => Some("\n".to_string()),
        // Math tokens already contain the complete texmath wrapper HTML
        // (`<eq>…</eq>` / `<section><eqn>…</eqn></section>`), produced by
        // `math::render`.
        TokenType::MathInline
        | TokenType::MathSection
        | TokenType::MathBlock
        | TokenType::MathBlockEqno => Some(token.content.clone()),
        TokenType::TocOpen => Some("<div class=\"table-of-contents\">".to_string()),
        TokenType::TocBody => Some(token.raw.clone().unwrap_or_default()),
        TokenType::TocClose => Some("</div>\n".to_string()),
        _ => None,
    }
}

/// Render an image token: the `alt` attribute is filled from the inline alt
/// children exactly like markdown-it's `image` rule + `renderInlineAsText`.
fn render_image(token: &Token, out: &mut String) {
    let alt = render_inline_as_text(&token.children);
    out.push_str("<img");
    out.push_str(&render_attrs_with_alt(&token.attrs, &alt));
    out.push('>');
}

/// Render attributes, overriding the value of the `alt` attribute.
fn render_attrs_with_alt(attrs: &[(String, String)], alt: &str) -> String {
    let mut out = String::new();
    for (name, value) in attrs {
        out.push(' ');
        out.push_str(&escape_html(name));
        out.push_str("=\"");
        if name == "alt" {
            out.push_str(&escape_html(alt));
        } else {
            out.push_str(&escape_html(value));
        }
        out.push('"');
    }
    out
}

/// Render attributes exactly like markdown-it's `renderAttrs` (always quoted,
/// values escaped with `escapeHtml`).
fn render_attrs(attrs: &[(String, String)]) -> String {
    let mut out = String::new();
    for (name, value) in attrs {
        out.push(' ');
        out.push_str(&escape_html(name));
        out.push_str("=\"");
        out.push_str(&escape_html(value));
        out.push('"');
    }
    out
}

/// Port of markdown-it's `Renderer.renderToken`.
fn render_token(tokens: &[Token], index: usize, out: &mut String) {
    let token = &tokens[index];
    if token.hidden {
        return;
    }
    // Insert a newline between a hidden paragraph and a subsequent opening
    // block tag.
    if token.block && token.nesting != -1 && index > 0 && tokens[index - 1].hidden {
        out.push('\n');
    }
    out.push_str(if token.nesting == -1 { "</" } else { "<" });
    out.push_str(token.tag);
    out.push_str(&render_attrs(&token.attrs));

    let mut need_lf = false;
    if token.block {
        need_lf = true;
        if token.nesting == 1 {
            if let Some(next) = tokens.get(index + 1) {
                let same_tag_close = next.nesting == -1 && next.tag == token.tag;
                if next.typ == TokenType::Inline || next.hidden || same_tag_close {
                    need_lf = false;
                }
            }
        }
    }
    out.push('>');
    if need_lf {
        out.push('\n');
    }
}
