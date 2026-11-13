//! `markdown-it-implicit-figures` 0.12.0 support.
//!
//! A paragraph whose inline content is exactly one image, or one link wrapping
//! an image, is converted into:
//!
//! ```html
//! <figure tabindex="1"><a href="src"><img src="…" alt=""></a><figcaption>alt</figcaption></figure>
//! ```
//!
//! Options used by the reference pipeline: `{ dataType: false, figcaption: true,
//! tabindex: true, link: true }`. Notes on the exact plugin behaviour:
//!
//! * the `figure` tokens are the (possibly hidden) `paragraph_open`/`close`
//!   tokens, so figures in tight list items inherit `hidden`;
//! * `tabindex` counts from 1 per document and is appended after any existing
//!   figure attributes;
//! * the image alt children are moved into `<figcaption>` (rendered as inline
//!   markdown) and the image's `alt` attribute therefore becomes empty;
//! * a bare image is wrapped in a link to its own `src`.

use super::tokens::{Token, TokenType};

/// Convert image-only paragraphs into figures; returns the number of figures.
pub(crate) fn apply(tokens: &mut [Token]) -> usize {
    let mut tab_index = 1u64;
    let mut count = 0;
    let mut index = 1;
    while index + 1 < tokens.len() {
        if tokens[index].typ != TokenType::Inline
            || tokens[index - 1].typ != TokenType::ParagraphOpen
            || tokens[index + 1].typ != TokenType::ParagraphClose
        {
            index += 1;
            continue;
        }
        if !is_figure_content(&tokens[index].children) {
            index += 1;
            continue;
        }
        // Rewrite the paragraph tokens (keeping their `hidden` flag).
        tokens[index - 1].typ = TokenType::FigureOpen;
        tokens[index - 1].tag = "figure";
        tokens[index + 1].typ = TokenType::FigureClose;
        tokens[index + 1].tag = "figure";
        tokens[index - 1].attr_push("tabindex", tab_index.to_string());
        tab_index += 1;
        count += 1;

        let inline = &mut tokens[index];
        // Wrap a bare image in a link to its source (`link: true` option).
        if inline.children.len() == 1 {
            let src = inline.children[0]
                .attr_get("src")
                .unwrap_or_default()
                .to_string();
            let mut open = Token::inline_open(TokenType::LinkOpen, "a");
            open.attr_push("href", src);
            inline.children.insert(0, open);
            inline
                .children
                .push(Token::inline_close(TokenType::LinkClose, "a"));
        }
        // Locate the image (second child for linked images).
        let image_index =
            usize::from(inline.children.len() >= 2 && inline.children[1].typ == TokenType::Image);
        // Move the alt children into a figcaption and clear the image alt.
        let caption = std::mem::take(&mut inline.children[image_index].children);
        if !caption.is_empty() {
            inline
                .children
                .push(Token::inline_open(TokenType::FigcaptionOpen, "figcaption"));
            inline.children.extend(caption);
            inline.children.push(Token::inline_close(
                TokenType::FigcaptionClose,
                "figcaption",
            ));
        }
        index += 2;
    }
    count
}

/// `[image]` or `[link, image, link_close]` inline pattern.
fn is_figure_content(children: &[Token]) -> bool {
    match children {
        [image] => image.typ == TokenType::Image,
        [open, image, close] => {
            open.typ == TokenType::LinkOpen
                && image.typ == TokenType::Image
                && close.typ == TokenType::LinkClose
        }
        _ => false,
    }
}
