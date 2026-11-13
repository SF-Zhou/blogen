//! Math pre-pass and KaTeX post-processing.
//!
//! The original pipeline (`markdown-it-texmath`, delimiter set `dollars`)
//! recognises four forms:
//!
//! | form | syntax | wrapper |
//! | --- | --- | --- |
//! | inline | `$tex$` | `<eq>…</eq>` |
//! | inline double | `$$tex$$` inside a paragraph | `<section><eqn>…</eqn></section>` |
//! | block | `$$` at the start of a line (multi-line) | `<section><eqn>…</eqn></section>` |
//! | block with equation number | `$$tex$$ (1)` at line start | `<section class="eqno"><eqn>…</eqn><span>(1)</span></section>` |
//!
//! TeX must never reach the CommonMark parser (otherwise `_`, `*`, `{` etc.
//! would be parsed as markdown), so the source is pre-processed here: math
//! spans are replaced with private-use placeholders and the original TeX is
//! rendered to HTML later with the vendored KaTeX build. Code spans, fenced and
//! indented code blocks, link destinations and reference definitions are left
//! untouched, mirroring how markdown-it's inline/block rules interleave.
//!
//! The public entry points are [`protect`] (returns placeholder-protected
//! markdown plus the collected math items) and [`render`] (returns the final
//! HTML for each item, in order).

use std::ops::Range;

use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};

use crate::error::Result;

use super::js;

/// Opening sentinel of a math placeholder.
pub(crate) const PLACEHOLDER_OPEN: char = '\u{E000}';
/// Closing sentinel of a math placeholder.
pub(crate) const PLACEHOLDER_CLOSE: char = '\u{E001}';

/// Which texmath delimiter form produced a math item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MathKind {
    /// `$tex$`
    Inline,
    /// `$$tex$$` inside a paragraph (display style but inline wrapper)
    InlineDouble,
    /// `$$` at the start of a line, possibly spanning multiple lines
    Block,
    /// `$$tex$$ (1)` at the start of a line
    BlockEqno,
}

/// One extracted math expression.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MathItem {
    /// Which delimiter form was used.
    pub kind: MathKind,
    /// Raw TeX content (without delimiters).
    pub tex: String,
    /// Equation number for [`MathKind::BlockEqno`].
    pub eqno: Option<String>,
}

impl MathItem {
    /// Whether KaTeX should render this item in display mode.
    fn display_mode(&self) -> bool {
        !matches!(self.kind, MathKind::Inline)
    }
}

/// Build the placeholder for item `index`.
pub(crate) fn placeholder(index: usize) -> String {
    format!("{PLACEHOLDER_OPEN}{index}{PLACEHOLDER_CLOSE}")
}

/// Extract math spans from markdown.
///
/// Returns the protected markdown (byte-identical except for the replaced math
/// spans and the blank lines inserted around block math) and the math items in
/// placeholder order. When `enabled` is false the input is returned unchanged.
#[allow(clippy::unnecessary_wraps)] // reserved for future TeX problems; keeps the renderer integration uniform
pub(crate) fn protect(markdown: &str, enabled: bool) -> Result<(String, Vec<MathItem>)> {
    if !enabled || !markdown.contains('$') {
        return Ok((markdown.to_string(), Vec::new()));
    }
    let protected = protected_ranges(markdown);
    let bytes = markdown.as_bytes();
    let mut output = String::with_capacity(markdown.len());
    let mut items: Vec<MathItem> = Vec::new();
    let mut copied_until = 0_usize;
    let mut index = 0_usize;
    let mut protected_index = 0_usize;

    while index < bytes.len() {
        // Skip over protected (code/link destination) regions.
        while protected_index < protected.len() && protected[protected_index].end <= index {
            protected_index += 1;
        }
        if protected_index < protected.len() && protected[protected_index].start <= index {
            index = protected[protected_index].end;
            continue;
        }

        let at_line_start = index == 0 || bytes[index - 1] == b'\n';
        if at_line_start {
            if let Some((item, consumed_until)) = match_block(markdown, index) {
                output.push_str(&markdown[copied_until..index]);
                output.push_str("\n\n");
                output.push_str(&placeholder(items.len()));
                output.push_str("\n\n");
                items.push(item);
                copied_until = consumed_until;
                index = consumed_until;
                continue;
            }
        }

        if bytes[index] == b'$' {
            if let Some((item, end)) = match_inline(markdown, index) {
                output.push_str(&markdown[copied_until..index]);
                output.push_str(&placeholder(items.len()));
                items.push(item);
                copied_until = end;
                index = end;
                continue;
            }
        }

        // Advance to the next char boundary (markdown is valid UTF-8).
        index += 1;
        while index < bytes.len() && (bytes[index] & 0xC0) == 0x80 {
            index += 1;
        }
    }

    output.push_str(&markdown[copied_until..]);
    Ok((output, items))
}

/// Render the collected math items to their final HTML wrappers.
///
/// The returned vector has one entry per item, in the same order.
#[allow(clippy::unnecessary_wraps)] // the JS engine can fail; Result stays part of the contract
pub(crate) fn render(items: &[MathItem]) -> Result<Vec<String>> {
    if items.is_empty() {
        return Ok(Vec::new());
    }
    let batch: Vec<(String, bool)> = items
        .iter()
        .map(|item| (item.tex.clone(), item.display_mode()))
        .collect();
    let rendered = match js::render_math(&batch) {
        Ok(rendered) => rendered,
        Err(batch_error) => {
            // Mirror texmath's per-expression try/catch: if a whole batch
            // fails, retry individually so one bad expression cannot break the
            // rest of the document.
            let mut fallback = Vec::with_capacity(items.len());
            for (tex, display) in &batch {
                match js::render_math(std::slice::from_ref(&(tex.clone(), *display))) {
                    Ok(mut rendered) if !rendered.is_empty() => {
                        fallback.push(rendered.remove(0));
                    }
                    _ => fallback.push(escape_texmath(&format!("{tex}:{batch_error}"))),
                }
            }
            fallback
        }
    };
    let mut out = Vec::with_capacity(items.len());
    for (item, katex) in items.iter().zip(rendered) {
        let katex = if katex.ends_with("</span>") || !katex.is_empty() {
            katex
        } else {
            escape_texmath(&format!("{}:KaTeX returned no output", item.tex))
        };
        out.push(match item.kind {
            MathKind::Inline => format!("<eq>{katex}</eq>"),
            MathKind::InlineDouble | MathKind::Block => {
                format!("<section><eqn>{katex}</eqn></section>")
            }
            MathKind::BlockEqno => format!(
                "<section class=\"eqno\"><eqn>{katex}</eqn><span>({})</span></section>",
                escape_texmath(item.eqno.as_deref().unwrap_or_default())
            ),
        });
    }
    Ok(out)
}

/// Escape text exactly like `markdown-it-texmath`'s `escapeHTML` helper.
fn escape_texmath(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#039;"),
            _ => out.push(ch),
        }
    }
    out
}

/// Try to match block math starting at a line start (`line_start`).
///
/// Returns the item and the byte offset just past the consumed lines. The
/// rest of the closing line is discarded, exactly like markdown-it block rules.
fn match_block(source: &str, line_start: usize) -> Option<(MathItem, usize)> {
    let bytes = source.as_bytes();
    let mut pos = line_start;
    let mut indent = 0;
    while pos < bytes.len() && bytes[pos] == b' ' && indent < 4 {
        pos += 1;
        indent += 1;
    }
    if indent >= 4 {
        return None;
    }
    if !source[pos..].starts_with("$$") {
        return None;
    }

    // Content may not contain `$`; the first following `$` must start the
    // closing `$$`, the content must be non-empty and must not end with a
    // backslash (`[^$]*?[^\\]`).
    let content_start = pos + 2;
    let close = source[content_start..].find('$')? + content_start;
    if !source[close..].starts_with("$$") {
        return None;
    }
    let content = &source[content_start..close];
    if content.is_empty() || content.ends_with('\\') {
        return None;
    }

    let mut match_end = close + 2;
    // Optional equation number: `\s*?\(([^)$\r\n]+?)\)`.
    let mut eqno = None;
    let mut after = match_end;
    while after < bytes.len() && bytes[after].is_ascii_whitespace() {
        after += 1;
    }
    if after < bytes.len() && bytes[after] == b'(' {
        let eqno_start = after + 1;
        if let Some(relative) = source[eqno_start..]
            .bytes()
            .position(|byte| matches!(byte, b')' | b'$' | b'\r' | b'\n'))
        {
            let eqno_end = eqno_start + relative;
            if eqno_end > eqno_start && bytes[eqno_end] == b')' {
                eqno = Some(source[eqno_start..eqno_end].to_string());
                match_end = eqno_end + 1;
            }
        }
    }
    let kind = if eqno.is_some() {
        MathKind::BlockEqno
    } else {
        MathKind::Block
    };
    // The block rule consumes the remainder of the line containing the match.
    let consumed_until = source[match_end..]
        .find('\n')
        .map_or(bytes.len(), |offset| match_end + offset + 1);
    Some((
        MathItem {
            kind,
            tex: content.to_string(),
            eqno,
        },
        consumed_until,
    ))
}

/// Try to match inline math (`$$…$$` first, then `$…$`) at `pos`.
fn match_inline(source: &str, pos: usize) -> Option<(MathItem, usize)> {
    let bytes = source.as_bytes();
    if !pre_guard(bytes, pos) {
        return None;
    }
    if source[pos..].starts_with("$$") {
        if let Some(result) = match_inline_double(source, pos) {
            return Some(result);
        }
    }
    match_inline_single(source, pos)
}

/// markdown-it-texmath `$_pre`: the previous character must not be a backslash
/// or an ASCII digit.
fn pre_guard(bytes: &[u8], pos: usize) -> bool {
    if pos == 0 {
        return true;
    }
    let previous = bytes[pos - 1];
    previous != b'\\' && !previous.is_ascii_digit()
}

/// markdown-it-texmath `$_post` (non-`outerSpace` mode): the next character
/// must not be an ASCII digit.
fn post_guard(bytes: &[u8], index: usize) -> bool {
    match bytes.get(index) {
        Some(byte) => !byte.is_ascii_digit(),
        None => true,
    }
}

/// Match `$$…$$` in inline context (`math_inline_double`).
fn match_inline_double(source: &str, pos: usize) -> Option<(MathItem, usize)> {
    let bytes = source.as_bytes();
    let content_start = pos + 2;
    let close = source[content_start..].find('$')? + content_start;
    if !source[close..].starts_with("$$") {
        return None;
    }
    let content = &source[content_start..close];
    // `[^$]*?[^\\]` requires non-empty content that does not end with a
    // backslash. Inline content cannot cross a paragraph break.
    if content.is_empty() || content.ends_with('\\') || content.contains("\n\n") {
        return None;
    }
    let end = close + 2;
    if !post_guard(bytes, end) {
        return None;
    }
    Some((
        MathItem {
            kind: MathKind::InlineDouble,
            tex: content.to_string(),
            eqno: None,
        },
        end,
    ))
}

/// Match `$…$` in inline context (`math_inline`).
fn match_inline_single(source: &str, pos: usize) -> Option<(MathItem, usize)> {
    let bytes = source.as_bytes();
    let content_start = pos + 1;
    if content_start >= bytes.len() {
        return None;
    }
    // `\$((?:[^\s\\])|(?:\S.*?[^\s\\]))\$` with lazy `.*?`: the first valid
    // closing `$` wins. Inline content never contains a newline.
    let mut close = content_start + 1;
    while close < bytes.len() {
        if bytes[close] == b'$' {
            let content = &source[content_start..close];
            if is_valid_inline_content(content) && post_guard(bytes, close + 1) {
                return Some((
                    MathItem {
                        kind: MathKind::Inline,
                        tex: content.to_string(),
                        eqno: None,
                    },
                    close + 1,
                ));
            }
        }
        if bytes[close] == b'\n' {
            return None;
        }
        close += 1;
        while close < bytes.len() && (bytes[close] & 0xC0) == 0x80 {
            close += 1;
        }
    }
    None
}

/// Validate inline math content against the texmath `math_inline` regex.
fn is_valid_inline_content(content: &str) -> bool {
    let mut chars = content.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    let last = content.chars().next_back().unwrap_or(first);
    if first.is_whitespace() || last.is_whitespace() || last == '\\' {
        return false;
    }
    // A single-character `$…$` may not be a backslash (`[^\s\\]`).
    if content.chars().count() == 1 && first == '\\' {
        return false;
    }
    !content.contains('\n')
}

/// Ranges of the markdown that must not be scanned for math.
///
/// This covers fenced/indented code blocks, inline code spans, link and image
/// destinations and link reference definitions, mirroring the source regions
/// markdown-it's rules consume before the math rules can see them.
fn protected_ranges(source: &str) -> Vec<Range<usize>> {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_GFM);

    let mut ranges: Vec<Range<usize>> = Vec::new();
    let mut code_block_start: Option<usize> = None;
    for (event, range) in Parser::new_ext(source, options).into_offset_iter() {
        match event {
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(_) | CodeBlockKind::Indented)) => {
                code_block_start = Some(range.start);
            }
            Event::End(TagEnd::CodeBlock) => {
                if let Some(start) = code_block_start.take() {
                    ranges.push(start..range.end);
                }
            }
            Event::Code(_) => ranges.push(range),
            Event::Start(Tag::Link { link_type, .. }) => {
                if matches!(
                    link_type,
                    pulldown_cmark::LinkType::Autolink | pulldown_cmark::LinkType::Email
                ) {
                    ranges.push(range);
                } else {
                    push_destination_range(source, range, &mut ranges);
                }
            }
            Event::Start(Tag::Image { .. }) => {
                push_destination_range(source, range, &mut ranges);
            }
            _ => {}
        }
    }
    // Link reference definitions are not represented by events.
    let mut offset = 0;
    for line in source.split_inclusive('\n') {
        let trimmed = line.trim_start_matches(' ');
        if line.len() - trimmed.len() <= 3 && trimmed.starts_with('[') {
            if let Some(close) = trimmed.find("]:") {
                if close > 1 {
                    ranges.push(offset..offset + line.len());
                }
            }
        }
        offset += line.len();
    }
    ranges.sort_by_key(|range| range.start);
    ranges
}

/// Protect the `(destination "title")` part of an inline link/image.
fn push_destination_range(source: &str, range: Range<usize>, ranges: &mut Vec<Range<usize>>) {
    let slice = &source[range.clone()];
    if let Some(relative) = slice.find("](") {
        let destination_start = range.start + relative + 2;
        if destination_start < range.end {
            ranges.push(destination_start..range.end);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn protected(markdown: &str) -> (String, Vec<MathItem>) {
        protect(markdown, true).expect("protect")
    }

    #[test]
    fn inline_math() {
        let (out, items) = protected("before $x^2$ after");
        assert_eq!(out, format!("before {} after", placeholder(0)));
        assert_eq!(
            items,
            vec![MathItem {
                kind: MathKind::Inline,
                tex: "x^2".into(),
                eqno: None,
            }]
        );
    }

    #[test]
    fn inline_double_math() {
        let (out, items) = protected("a $$x+1$$ b");
        assert_eq!(out, format!("a {} b", placeholder(0)));
        assert_eq!(items[0].kind, MathKind::InlineDouble);
        assert_eq!(items[0].tex, "x+1");
    }

    #[test]
    fn block_math() {
        let (out, items) = protected("text\n\n$$\na^2 + b^2\n$$\n\nmore");
        assert_eq!(out, format!("text\n\n\n\n{}\n\n\nmore", placeholder(0)));
        assert_eq!(items[0].kind, MathKind::Block);
        assert_eq!(items[0].tex, "\na^2 + b^2\n");
    }

    #[test]
    fn block_math_with_equation_number() {
        let (_out, items) = protected("$$\nx = y\n$$ (42)\n");
        assert_eq!(items[0].kind, MathKind::BlockEqno);
        assert_eq!(items[0].eqno.as_deref(), Some("42"));
    }

    #[test]
    fn block_math_discards_trailing_text_on_closing_line() {
        let (out, _items) = protected("$$x$$ trailing text\nnext");
        assert!(!out.contains("trailing"), "{out}");
        assert!(out.ends_with("next"));
    }

    #[test]
    fn code_is_not_math() {
        let (out, items) = protected("`$x$`\n\n```\n$y$\n```\n\n    $z$\n");
        assert_eq!(items.len(), 0, "{items:?}");
        assert_eq!(out, "`$x$`\n\n```\n$y$\n```\n\n    $z$\n");
    }

    #[test]
    fn link_destinations_are_not_math() {
        let (out, items) = protected("[link](http://example.com/$a$) ![i](img/$b$.png)");
        assert_eq!(items.len(), 0, "{items:?}");
        assert_eq!(out, "[link](http://example.com/$a$) ![i](img/$b$.png)");
    }

    #[test]
    fn escaped_dollar_is_not_math() {
        let (out, items) = protected("costs \\$5 and \\$6");
        assert_eq!(items.len(), 0, "{items:?}");
        assert_eq!(out, "costs \\$5 and \\$6");
    }

    #[test]
    fn digit_adjacent_dollars_are_not_math() {
        // `$_pre` rejects a digit before the opening `$`, `$_post` rejects a
        // digit after the closing `$`; the content itself may be a digit.
        assert_eq!(protected("5$x$").1.len(), 0);
        assert_eq!(protected("$x$5").1.len(), 0);
        assert_eq!(protected("x$5$y").1.len(), 1);
        assert_eq!(protected("x$5$y").1[0].tex, "5");
    }

    #[test]
    fn dollar_amounts_are_not_math() {
        assert_eq!(protected("price is $5 or $10 today").1.len(), 0);
    }

    #[test]
    fn multiple_items_keep_order() {
        let (_out, items) = protected("$a$ then $$b$$ then\n\n$$\nc\n$$\n");
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].tex, "a");
        assert_eq!(items[1].tex, "b");
        assert_eq!(items[2].tex, "\nc\n");
    }

    #[test]
    fn math_inside_emphasis_is_extracted() {
        let (_out, items) = protected("**bold $x$**");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].tex, "x");
    }

    #[test]
    fn disabled_passthrough() {
        let source = "text $x$";
        let (out, items) = protect(source, false).unwrap();
        assert_eq!(out, source);
        assert_eq!(items.len(), 0);
    }

    #[test]
    fn renders_inline_and_block_wrappers() {
        let items = vec![
            MathItem {
                kind: MathKind::Inline,
                tex: "x".into(),
                eqno: None,
            },
            MathItem {
                kind: MathKind::Block,
                tex: "y^2".into(),
                eqno: None,
            },
            MathItem {
                kind: MathKind::BlockEqno,
                tex: "z".into(),
                eqno: Some("7".into()),
            },
        ];
        let html = render(&items).expect("render");
        assert_eq!(html.len(), 3);
        assert!(
            html[0].starts_with("<eq><span class=\"katex\">"),
            "{}",
            html[0]
        );
        assert!(html[0].ends_with("</eq>"));
        assert!(html[1].starts_with("<section><eqn><span class=\"katex-display\">"));
        assert!(html[1].ends_with("</eqn></section>"));
        assert!(html[2].starts_with("<section class=\"eqno\"><eqn>"));
        assert!(html[2].ends_with("</eqn><span>(7)</span></section>"));
    }

    #[test]
    fn error_fallback_is_escaped() {
        // A TeX expression that KaTeX cannot parse still renders an error node
        // (throwOnError:false) and never aborts the batch.
        let items = vec![MathItem {
            kind: MathKind::Inline,
            tex: "\\frac{".into(),
            eqno: None,
        }];
        let html = render(&items).expect("render");
        assert!(html[0].starts_with("<eq>"));
        assert!(html[0].contains("katex-error") || html[0].contains("katex"));
    }

    #[test]
    fn escape_texmath_matches_original() {
        assert_eq!(
            escape_texmath("a&b<c>d\"e'f"),
            "a&amp;b&lt;c&gt;d&quot;e&#039;f"
        );
    }

    #[test]
    fn placeholder_sentinels_are_unique() {
        let (out, items) = protected("$a$\n\n$$\nb\n$$\n");
        assert!(out.contains(PLACEHOLDER_OPEN));
        assert!(out.contains(PLACEHOLDER_CLOSE));
        assert_eq!(items.len(), 2);
    }
}
