//! `pulldown-cmark` event stream -> markdown-it-like token tree.
//!
//! Only the parsing half lives here: the source is pre-processed by
//! [`crate::markdown::math`] (math placeholders) and
//! [`crate::markdown::toc`] (the `[TOC]` marker) before it reaches this module,
//! so the parser itself is a pure CommonMark + GFM (tables, strikethrough)
//! mapping.
//!
//! The important differences to a naive event-to-HTML mapping are:
//!
//! * raw HTML is *not* passed through (`html: false` semantics): HTML blocks
//!   become paragraphs and inline HTML becomes text, exactly like markdown-it;
//! * tight list items (pulldown omits their `Paragraph` events) get hidden
//!   paragraph tokens so the renderer can reproduce markdown-it's
//!   `<li>text</li>` output;
//! * tables are expanded to markdown-it's exact token sequence
//!   (`thead`/`tbody`/`th`/`td`, alignment styles);
//! * link destinations and image sources are normalised the way markdown-it's
//!   `normalizeLink` does (`mdurl` percent-encoding) and validated like
//!   `validateLink`.

use pulldown_cmark::TextMergeStream;
use pulldown_cmark::{
    Alignment, CodeBlockKind, Event, HeadingLevel, LinkType, Options, Parser, Tag, TagEnd,
};

use super::tokens::{Token, TokenType};
use super::util;

/// The pulldown-cmark extensions that correspond to the reference pipeline's
/// markdown-it configuration (`html: false`, GFM tables + strikethrough +
/// autolink literals; task lists and footnotes are *not* enabled because
/// markdown-it's default preset does not enable them).
fn options() -> Options {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options
}

type Events<'a> = std::iter::Peekable<TextMergeStream<'a, Parser<'a>>>;

/// Parse a (already pre-processed) markdown document into block tokens.
pub(crate) fn parse(markdown: &str) -> Vec<Token> {
    let parser = Parser::new_ext(markdown, options());
    let mut events = TextMergeStream::new(parser).peekable();
    let mut tokens = Vec::new();
    parse_blocks(&mut events, &mut tokens);
    tokens
}

/// Parse a bare inline markdown fragment (used by the table of contents'
/// `md.renderInline(text)`).
pub(crate) fn parse_inline_fragment(text: &str) -> Vec<Token> {
    if text.is_empty() {
        return Vec::new();
    }
    // markdown-it's `renderInline` never parses block constructs, but a bare
    // pulldown parse of e.g. "1. Flynn 分类法" would consume the "1. " as an
    // ordered-list marker. Prefixing a letter keeps the fragment on the
    // paragraph/inline path; the extra letter is stripped again below.
    let prefixed = format!("a{text}");
    let parser = Parser::new_ext(&prefixed, options());
    let mut events = TextMergeStream::new(parser).peekable();
    let mut children = Vec::new();
    while let Some(event) = events.next() {
        match event {
            Event::Start(Tag::Paragraph) | Event::End(TagEnd::Paragraph) => {}
            other => push_inline_event(other, &mut events, &mut children),
        }
    }
    strip_prefix_letter(&mut children);
    children
}

/// Remove the synthetic `a` prefix added by [`parse_inline_fragment`].
fn strip_prefix_letter(children: &mut Vec<Token>) {
    if let Some(first) = children.first_mut() {
        if first.typ == TokenType::Text && first.content.starts_with('a') {
            first.content.remove(0);
            if first.content.is_empty() {
                children.remove(0);
            }
        }
    }
}

/// Consume the next event, which must be an `End` tag.
#[track_caller]
fn consume_end(events: &mut Events<'_>) {
    if let Some(event) = events.next() {
        assert!(
            matches!(event, Event::End(_)),
            "expected end event, got {event:?}"
        );
    }
}

/// Parse a run of block level events into `out` until the matching end tag (or
/// the end of the stream), leaving the end event for the caller.
fn parse_blocks(events: &mut Events<'_>, out: &mut Vec<Token>) {
    while let Some(event) = events.peek() {
        if matches!(event, Event::End(_)) {
            // Leave the end event for the caller to consume.
            break;
        }
        let Some(event) = events.next() else { break };
        match event {
            Event::Start(tag) => parse_block_tag(tag, events, out),
            Event::Rule => out.push(Token::new(TokenType::Hr, "hr", 0, true, "")),
            // Stray block-level content: pulldown only emits these directly for
            // tight list items, but handle everything that has an inline
            // representation to stay robust.
            other => {
                let mut children = Vec::new();
                push_inline_event(other, events, &mut children);
                collect_tight_inline(events, &mut children);
                push_hidden_paragraph(children, out);
            }
        }
    }
}

/// Wrap inline children in a hidden paragraph (markdown-it's tight-list form).
fn push_hidden_paragraph(children: Vec<Token>, out: &mut Vec<Token>) {
    let mut open = Token::open(TokenType::ParagraphOpen, "p");
    open.hidden = true;
    let mut close = Token::close(TokenType::ParagraphClose, "p");
    close.hidden = true;
    let mut inline = Token::new(TokenType::Inline, "", 0, false, "");
    inline.children = children;
    out.push(open);
    out.push(inline);
    out.push(close);
}

/// True when the event starts a new block (and therefore ends a tight inline
/// run).
fn is_block_start(event: &Event<'_>) -> bool {
    matches!(
        event,
        Event::Start(
            Tag::Paragraph
                | Tag::Heading { .. }
                | Tag::BlockQuote(_)
                | Tag::CodeBlock(_)
                | Tag::HtmlBlock
                | Tag::List(_)
                | Tag::Item
                | Tag::Table(_)
                | Tag::TableHead
                | Tag::TableRow
                | Tag::TableCell
                | Tag::FootnoteDefinition(_)
                | Tag::MetadataBlock(_)
                | Tag::DefinitionList
                | Tag::DefinitionListTitle
                | Tag::DefinitionListDefinition
        )
    )
}

/// Consume inline events until the enclosing block ends.
fn collect_tight_inline(events: &mut Events<'_>, children: &mut Vec<Token>) {
    while let Some(event) = events.peek() {
        if is_block_start(event) || matches!(event, Event::End(_) | Event::Rule) {
            break;
        }
        let Some(event) = events.next() else { break };
        push_inline_event(event, events, children);
    }
}

/// Parse one block `tag` (and its content) into `out`.
fn parse_block_tag<'a>(tag: Tag<'a>, events: &mut Events<'a>, out: &mut Vec<Token>) {
    match tag {
        Tag::Paragraph => {
            let mut inline = Token::new(TokenType::Inline, "", 0, false, "");
            parse_inline_until(events, &mut inline.children);
            consume_end(events);
            out.push(Token::open(TokenType::ParagraphOpen, "p"));
            out.push(inline);
            out.push(Token::close(TokenType::ParagraphClose, "p"));
        }
        Tag::Heading { level, .. } => {
            let mut inline = Token::new(TokenType::Inline, "", 0, false, "");
            parse_inline_until(events, &mut inline.children);
            consume_end(events);
            let tag = match level {
                HeadingLevel::H1 => "h1",
                HeadingLevel::H2 => "h2",
                HeadingLevel::H3 => "h3",
                HeadingLevel::H4 => "h4",
                HeadingLevel::H5 => "h5",
                HeadingLevel::H6 => "h6",
            };
            out.push(Token::open(TokenType::HeadingOpen, tag));
            out.push(inline);
            out.push(Token::close(TokenType::HeadingClose, tag));
        }
        Tag::BlockQuote(_) => {
            out.push(Token::open(TokenType::BlockquoteOpen, "blockquote"));
            parse_blocks(events, out);
            consume_end(events);
            out.push(Token::close(TokenType::BlockquoteClose, "blockquote"));
        }
        Tag::CodeBlock(kind) => {
            let mut content = String::new();
            for event in events.by_ref() {
                match event {
                    Event::Text(text) => content.push_str(&text),
                    Event::End(TagEnd::CodeBlock) => break,
                    _ => {}
                }
            }
            match kind {
                CodeBlockKind::Indented => {
                    out.push(Token::new(TokenType::CodeBlock, "code", 0, true, content));
                }
                CodeBlockKind::Fenced(info) => {
                    let mut token = Token::new(TokenType::Fence, "code", 0, true, content);
                    token.info = info.into_string();
                    out.push(token);
                }
            }
        }
        Tag::HtmlBlock => {
            let mut content = String::new();
            for event in events.by_ref() {
                match event {
                    Event::Html(html) => content.push_str(&html),
                    Event::End(TagEnd::HtmlBlock) => break,
                    _ => {}
                }
            }
            if content.ends_with('\n') {
                content.pop();
            }
            // markdown-it with `html: false` parses HTML block lines as an
            // ordinary paragraph, so re-parse the content as inline markdown.
            let mut inline = Token::new(TokenType::Inline, "", 0, false, "");
            inline.children = parse_inline_fragment(&content);
            out.push(Token::open(TokenType::ParagraphOpen, "p"));
            out.push(inline);
            out.push(Token::close(TokenType::ParagraphClose, "p"));
        }
        Tag::List(start) => {
            let ordered = start.is_some();
            if ordered {
                let mut open = Token::open(TokenType::OrderedListOpen, "ol");
                if let Some(start) = start {
                    if start != 1 {
                        open.attr_push("start", start.to_string());
                    }
                }
                out.push(open);
            } else {
                out.push(Token::open(TokenType::BulletListOpen, "ul"));
            }
            while matches!(events.peek(), Some(Event::Start(Tag::Item))) {
                events.next();
                out.push(Token::open(TokenType::ListItemOpen, "li"));
                parse_blocks(events, out);
                consume_end(events);
                out.push(Token::close(TokenType::ListItemClose, "li"));
            }
            consume_end(events);
            if ordered {
                out.push(Token::close(TokenType::OrderedListClose, "ol"));
            } else {
                out.push(Token::close(TokenType::BulletListClose, "ul"));
            }
        }
        Tag::Table(alignments) => parse_table(&alignments, events, out),
        // Inline tags cannot legitimately appear at block level; render them as
        // text rather than dropping them.
        other => {
            let mut children = Vec::new();
            push_inline_event(Event::Start(other), events, &mut children);
            collect_tight_inline(events, &mut children);
            push_hidden_paragraph(children, out);
        }
    }
}

/// Parse a table into markdown-it's token sequence.
fn parse_table(alignments: &[Alignment], events: &mut Events<'_>, out: &mut Vec<Token>) {
    out.push(Token::open(TokenType::TableOpen, "table"));
    // Header row.
    if matches!(events.peek(), Some(Event::Start(Tag::TableHead))) {
        events.next();
        out.push(Token::open(TokenType::TheadOpen, "thead"));
        out.push(Token::open(TokenType::TrOpen, "tr"));
        let mut index = 0;
        while matches!(events.peek(), Some(Event::Start(Tag::TableCell))) {
            events.next();
            let mut inline = Token::new(TokenType::Inline, "", 0, false, "");
            parse_inline_until(events, &mut inline.children);
            consume_end(events);
            let mut open = Token::open(TokenType::ThOpen, "th");
            if let Some(style) = alignments.get(index).and_then(|a| alignment_style(*a)) {
                open.attr_push("style", style);
            }
            out.push(open);
            out.push(inline);
            out.push(Token::close(TokenType::ThClose, "th"));
            index += 1;
        }
        consume_end(events);
        out.push(Token::close(TokenType::TrClose, "tr"));
        out.push(Token::close(TokenType::TheadClose, "thead"));
    }
    // Body rows.
    let mut rows = Vec::new();
    while matches!(events.peek(), Some(Event::Start(Tag::TableRow))) {
        events.next();
        let mut row = vec![Token::open(TokenType::TrOpen, "tr")];
        let mut index = 0;
        while matches!(events.peek(), Some(Event::Start(Tag::TableCell))) {
            events.next();
            let mut inline = Token::new(TokenType::Inline, "", 0, false, "");
            parse_inline_until(events, &mut inline.children);
            consume_end(events);
            let mut open = Token::open(TokenType::TdOpen, "td");
            if let Some(style) = alignments.get(index).and_then(|a| alignment_style(*a)) {
                open.attr_push("style", style);
            }
            row.push(open);
            row.push(inline);
            row.push(Token::close(TokenType::TdClose, "td"));
            index += 1;
        }
        consume_end(events);
        row.push(Token::close(TokenType::TrClose, "tr"));
        rows.push(row);
    }
    if !rows.is_empty() {
        out.push(Token::open(TokenType::TbodyOpen, "tbody"));
        for row in rows {
            out.extend(row);
        }
        out.push(Token::close(TokenType::TbodyClose, "tbody"));
    }
    consume_end(events);
    out.push(Token::close(TokenType::TableClose, "table"));
}

/// markdown-it's alignment CSS for `<th>`/`<td>`.
fn alignment_style(alignment: Alignment) -> Option<&'static str> {
    match alignment {
        Alignment::None => None,
        Alignment::Left => Some("text-align:left"),
        Alignment::Center => Some("text-align:center"),
        Alignment::Right => Some("text-align:right"),
    }
}

/// Parse inline content into `children` until the enclosing element ends.
fn parse_inline_until(events: &mut Events<'_>, children: &mut Vec<Token>) {
    while let Some(event) = events.peek() {
        if matches!(event, Event::End(_)) {
            break;
        }
        let Some(event) = events.next() else { break };
        push_inline_event(event, events, children);
    }
}

/// Parse the content of an inline container (emphasis, link, ...).
fn parse_inline_container(events: &mut Events<'_>) -> Vec<Token> {
    let mut children = Vec::new();
    parse_inline_until(events, &mut children);
    consume_end(events);
    children
}

/// Append a container's children surrounded by its open/close tokens.
fn push_container(children: &mut Vec<Token>, open: Token, inner: Vec<Token>, close: Token) {
    children.push(open);
    children.extend(inner);
    children.push(close);
}

/// Convert one inline-level event into tokens.
fn push_inline_event(event: Event<'_>, events: &mut Events<'_>, children: &mut Vec<Token>) {
    match event {
        Event::Text(text) => {
            children.push(Token::leaf(TokenType::Text, "", text.into_string()));
        }
        Event::Code(code) => {
            children.push(Token::leaf(
                TokenType::CodeInline,
                "code",
                code.into_string(),
            ));
        }
        Event::SoftBreak => children.push(Token::new(TokenType::Softbreak, "br", 0, false, "")),
        Event::HardBreak => children.push(Token::new(TokenType::Hardbreak, "br", 0, false, "")),
        Event::InlineHtml(html) | Event::Html(html) => {
            children.push(Token::leaf(TokenType::Text, "", html.into_string()));
        }
        Event::Start(Tag::Emphasis) => {
            let inner = parse_inline_container(events);
            push_container(
                children,
                Token::inline_open(TokenType::EmOpen, "em"),
                inner,
                Token::inline_close(TokenType::EmClose, "em"),
            );
        }
        Event::Start(Tag::Strong) => {
            let inner = parse_inline_container(events);
            push_container(
                children,
                Token::inline_open(TokenType::StrongOpen, "strong"),
                inner,
                Token::inline_close(TokenType::StrongClose, "strong"),
            );
        }
        Event::Start(Tag::Strikethrough) => {
            let inner = parse_inline_container(events);
            push_container(
                children,
                Token::inline_open(TokenType::SOpen, "s"),
                inner,
                Token::inline_close(TokenType::SClose, "s"),
            );
        }
        Event::Start(Tag::Link {
            link_type,
            dest_url,
            title,
            ..
        }) => {
            let href = match link_type {
                LinkType::Email => util::normalize_link(&format!("mailto:{dest_url}")),
                _ => util::normalize_link(&dest_url),
            };
            let inner = parse_inline_container(events);
            let valid = util::validate_link(&href);
            let mut open = Token::inline_open(TokenType::LinkOpen, "a");
            open.attr_push("href", if valid { href } else { String::new() });
            if !title.is_empty() {
                open.attr_push("title", title.into_string());
            }
            let content = if valid {
                match link_type {
                    LinkType::Autolink | LinkType::Email => vec![Token::leaf(
                        TokenType::Text,
                        "",
                        util::normalize_link_text(&dest_url),
                    )],
                    _ => inner,
                }
            } else {
                inner
            };
            push_container(
                children,
                open,
                content,
                Token::inline_close(TokenType::LinkClose, "a"),
            );
        }
        Event::Start(Tag::Image {
            dest_url, title, ..
        }) => {
            let inner = parse_inline_container(events);
            let href = util::normalize_link(&dest_url);
            let mut image = Token::new(TokenType::Image, "img", 0, false, "");
            image.attr_push(
                "src",
                if util::validate_link(&href) {
                    href
                } else {
                    String::new()
                },
            );
            image.attr_push("alt", "");
            if !title.is_empty() {
                image.attr_push("title", title.into_string());
            }
            image.children = inner;
            children.push(image);
        }
        // Any other event has no inline representation; ignore it.
        _ => {}
    }
}
