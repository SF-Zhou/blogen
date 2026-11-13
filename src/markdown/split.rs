//! Placeholder splitting.
//!
//! [`crate::markdown::math`] replaces TeX spans in the source with private-use
//! placeholders (`U+E000` + decimal index + `U+E001`) before the CommonMark
//! parse. This pass walks the parsed token tree and turns those placeholders
//! back into math tokens, and unwraps paragraphs that consist only of a
//! block-level math item (texmath emits a block-level token there, not a
//! paragraph).

use super::math::{MathKind, PLACEHOLDER_CLOSE, PLACEHOLDER_OPEN};
use super::tokens::{Token, TokenType};

/// Replace math placeholders inside text tokens and unwrap block math
/// paragraphs. `html` holds the final KaTeX wrapper HTML per math item.
pub(crate) fn apply(tokens: &mut Vec<Token>, html: &[String], kinds: &[MathKind]) {
    if kinds.is_empty() {
        return;
    }
    for token in tokens.iter_mut() {
        if token.typ == TokenType::Inline {
            process_children(&mut token.children, html, kinds);
        }
    }
    unwrap_block_math(tokens);
}

/// Recursively split placeholders in a list of inline children.
fn process_children(children: &mut Vec<Token>, html: &[String], kinds: &[MathKind]) {
    let mut out = Vec::with_capacity(children.len());
    for mut token in children.drain(..) {
        let needs_split = token.typ == TokenType::Text && token.content.contains(PLACEHOLDER_OPEN);
        if needs_split {
            out.extend(split_text(&token.content, html, kinds));
        } else {
            if !token.children.is_empty() {
                process_children(&mut token.children, html, kinds);
            }
            out.push(token);
        }
    }
    *children = out;
}

/// Split one text payload into text/math tokens.
fn split_text(content: &str, html: &[String], kinds: &[MathKind]) -> Vec<Token> {
    let mut out = Vec::new();
    let mut rest_start = 0;
    let mut search = 0;
    while let Some(open_offset) = content[search..].find(PLACEHOLDER_OPEN) {
        let open = search + open_offset;
        let Some(close_offset) = content[open..].find(PLACEHOLDER_CLOSE) else {
            break;
        };
        let close = open + close_offset;
        let payload = &content[open + PLACEHOLDER_OPEN.len_utf8()..close];
        let index = payload.parse::<usize>().ok();
        match index {
            Some(index) if index < html.len() && index < kinds.len() => {
                if open > rest_start {
                    out.push(Token::leaf(
                        TokenType::Text,
                        "",
                        content[rest_start..open].to_string(),
                    ));
                }
                out.push(math_token(kinds[index], &html[index]));
                rest_start = close + PLACEHOLDER_CLOSE.len_utf8();
                search = rest_start;
            }
            _ => {
                // Not a math placeholder (e.g. the TOC marker): keep scanning.
                search = close + PLACEHOLDER_CLOSE.len_utf8();
            }
        }
    }
    if rest_start < content.len() {
        out.push(Token::leaf(
            TokenType::Text,
            "",
            content[rest_start..].to_string(),
        ));
    }
    out
}

/// Build a math token (content is the final wrapper HTML).
fn math_token(kind: MathKind, html: &str) -> Token {
    let (typ, tag) = match kind {
        MathKind::Inline => (TokenType::MathInline, "math"),
        MathKind::InlineDouble => (TokenType::MathSection, "math"),
        MathKind::Block => (TokenType::MathBlock, "math"),
        MathKind::BlockEqno => (TokenType::MathBlockEqno, "math"),
    };
    let mut token = Token::new(typ, tag, 0, false, html.to_string());
    token.block = matches!(kind, MathKind::Block | MathKind::BlockEqno);
    token
}

/// A paragraph consisting only of one block-level math token becomes a block
/// math token (texmath pushes a block token, not a paragraph).
fn unwrap_block_math(tokens: &mut Vec<Token>) {
    let mut out = Vec::with_capacity(tokens.len());
    let mut index = 0;
    while index < tokens.len() {
        let unwrap = index + 2 < tokens.len()
            && tokens[index].typ == TokenType::ParagraphOpen
            && tokens[index + 2].typ == TokenType::ParagraphClose
            && tokens[index + 1].typ == TokenType::Inline
            && tokens[index + 1].children.len() == 1
            && matches!(
                tokens[index + 1].children[0].typ,
                TokenType::MathBlock | TokenType::MathBlockEqno
            );
        if unwrap {
            let math = tokens[index + 1].children[0].clone();
            out.push(math);
            index += 3;
        } else {
            out.push(tokens[index].clone());
            index += 1;
        }
    }
    *tokens = out;
}
