//! Markdown rendering pipeline.
//!
//! This module is intentionally self-contained: it takes markdown text and
//! returns HTML that is byte-compatible with the original pipeline
//! (markdown-it 14.3 + markdown-it-anchor + markdown-it-implicit-figures +
//! markdown-it-table-of-contents + markdown-it-texmath/KaTeX + Prism), except
//! for insignificant whitespace.
//!
//! # Pipeline
//!
//! 1. Normalise the source (CRLF -> LF, NUL -> U+FFFD).
//! 2. Code-aware source pre-passes:
//!    * `math` replaces TeX spans (`$…$`, `$$…$$`, `$$…$$ (1)`) with
//!      private-use placeholders (texmath "dollars" semantics);
//!    * `toc` isolates `[TOC]` marker lines as placeholder paragraphs.
//! 3. Parse with `pulldown-cmark` (CommonMark + GFM tables/strikethrough,
//!    `html: false`) and convert events into a markdown-it-like token tree
//!    (`parser`, `tokens`).
//! 4. Re-insert math as tokens rendered server-side by KaTeX (`split` +
//!    `math`), convert image-only paragraphs into figures (`figures`),
//!    linkify bare URLs (`linkify`), add heading anchors (`anchors`) and
//!    build the table of contents (`toc`).
//! 5. Highlight fenced code with Prism and render mermaid figures.
//! 6. Render the token tree with a byte-faithful port of markdown-it's HTML
//!    renderer (`render`).
//!
//! KaTeX and Prism are the *official* vendored browser bundles executed through
//! `quick-js` (see `js`, `vendor/js/`), so the produced HTML is identical to
//! the legacy pipeline.
//!
//! # Known gaps
//!
//! These only affect inputs the real blog does not use; the parity tests over
//! `posts/**/*.md` keep them visible:
//!
//! * `html: false` is emulated in two steps: line-leading HTML constructs are
//!   neutralised before the parse (`html_protect`) so they cannot form HTML
//!   blocks, and inline HTML events become text tokens. A raw `<a>` tag
//!   does not protect bare URLs from linkification the way markdown-it's
//!   `html_inline` bookkeeping does.
//! * Bare URLs that contain markdown emphasis characters (`_`, `*`) may be
//!   split by the CommonMark parser before linkification; markdown-it's inline
//!   linkify rule protects them.
//! * Non-ASCII host names in links are percent-encoded instead of
//!   punycode-encoded (markdown-it uses `punycode.toASCII`).
//! * Fence info strings only decode the common named HTML entities (numeric
//!   entities and the full CommonMark named-entity table are not reproduced).
//! * Block math is recognised like texmath's block rule at blank-line/block
//!   boundaries; exotic placements (e.g. directly inside a blockquote line or
//!   a list item's first line) become inline display math instead of a block.
//! * `markdown-it`'s inline linkify rule and the `text_special` escape guard
//!   are not reproduced (see `linkify`).
//!
//! # Example
//!
//! ```
//! use blogen::markdown::{MarkdownOptions, MarkdownRenderer};
//!
//! let renderer = MarkdownRenderer::new(MarkdownOptions::default());
//! let rendered = renderer.render("# Title\n\nHello *world*.").unwrap();
//! assert!(rendered.html.starts_with("<h1"));
//! assert_eq!(rendered.headings[0].text, "Title");
//! ```

use crate::config::MermaidMode;
use crate::error::{Error, Result};
use crate::model::Heading;

mod anchors;
mod figures;
mod html_protect;
mod js;
mod linkify;
mod math;
mod mermaid_static;
#[cfg(test)]
mod oracle_tests;
mod parser;
mod prism;
mod render;
mod split;
mod toc;
mod tokens;
mod util;

/// Options controlling the markdown pipeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkdownOptions {
    /// Replace a paragraph containing only `[TOC]` with a table of contents.
    pub toc: bool,
    /// Render `$...$` / `$$...$$` math with KaTeX.
    pub math: bool,
    /// Highlight fenced code blocks with Prism.
    pub highlight: bool,
    /// How to handle `mermaid` fenced code blocks.
    pub mermaid: MermaidMode,
}

impl Default for MarkdownOptions {
    fn default() -> Self {
        Self {
            toc: true,
            math: true,
            highlight: true,
            mermaid: MermaidMode::Static,
        }
    }
}

/// Result of rendering one markdown document.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RenderedMarkdown {
    /// Rendered HTML body.
    pub html: String,
    /// Headings in document order (after slug de-duplication).
    pub headings: Vec<Heading>,
    /// Number of Mermaid diagrams encountered.
    pub mermaid_diagrams: usize,
}

/// Renders markdown to HTML.
///
/// The renderer is cheap to construct and safe to share between threads
/// (`Sync`): expensive JavaScript engines are initialized lazily per thread.
#[derive(Debug, Clone)]
pub struct MarkdownRenderer {
    options: MarkdownOptions,
}

impl MarkdownRenderer {
    /// Create a renderer with the given options.
    pub fn new(options: MarkdownOptions) -> Self {
        Self { options }
    }

    /// The options this renderer was created with.
    pub fn options(&self) -> &MarkdownOptions {
        &self.options
    }

    /// Render a markdown document to HTML.
    pub fn render(&self, markdown: &str) -> Result<RenderedMarkdown> {
        render_impl(&self.options, markdown)
    }
}

/// Render one document; the single entry point used by [`MarkdownRenderer`].
pub(crate) fn render_impl(options: &MarkdownOptions, markdown: &str) -> Result<RenderedMarkdown> {
    let source = normalize_source(markdown);
    let (source, math_items) = math::protect(&source, options.math)?;
    let source = html_protect::protect(&source);
    let source = if options.toc {
        toc::protect(&source)
    } else {
        source
    };
    let math_html = if math_items.is_empty() {
        Vec::new()
    } else {
        math::render(&math_items)?
    };

    let mut tokens = parser::parse(&source);
    split::apply(&mut tokens, &math_html, &math_scope(&math_items));
    figures::apply(&mut tokens);
    linkify::apply(&mut tokens);

    let mut headings = Vec::new();
    anchors::apply(&mut tokens, &mut headings);
    if options.toc {
        toc::apply(&mut tokens);
    }
    let mermaid_diagrams = resolve_fences(&mut tokens, options)?;

    let html = render::render(&tokens);
    Ok(RenderedMarkdown {
        html,
        headings,
        mermaid_diagrams,
    })
}

/// Extract the math item kinds for [`split::apply`].
fn math_scope(items: &[math::MathItem]) -> Vec<math::MathKind> {
    items.iter().map(|item| item.kind).collect()
}

/// markdown-it's `normalize` core rule: CRLF -> LF and NUL -> U+FFFD.
fn normalize_source(markdown: &str) -> String {
    let mut out = String::with_capacity(markdown.len());
    let mut chars = markdown.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                out.push('\n');
            }
            '\0' => out.push('\u{FFFD}'),
            _ => out.push(ch),
        }
    }
    out
}

/// Resolve all fenced code blocks: Prism highlighting, mermaid figures or the
/// plain escaped `highlight: false` path.
fn resolve_fences(tokens: &mut [tokens::Token], options: &MarkdownOptions) -> Result<usize> {
    use tokens::TokenType;

    let mut mermaid_diagrams = 0;
    let mut pending: Vec<usize> = Vec::new();
    let mut inputs: Vec<(String, String)> = Vec::new();
    let mut mermaid_index = 0;

    for (index, token) in tokens.iter_mut().enumerate() {
        if token.typ != TokenType::Fence {
            continue;
        }
        let lang = prism::info_lang(&token.info);
        if lang == "mermaid" && options.mermaid != MermaidMode::None {
            mermaid_diagrams += 1;
            let raw = match options.mermaid {
                MermaidMode::Static => {
                    let html = mermaid_static::render(&token.content, mermaid_index)?;
                    mermaid_index += 1;
                    html
                }
                MermaidMode::Placeholder => {
                    let digest = crate::hashing::md5_hex(token.content.as_bytes());
                    let id = format!("mermaid-{}-{}", &digest[..8], mermaid_index);
                    mermaid_index += 1;
                    format!(
                        "<figure class=\"mermaid\"><div class=\"mermaid-light\">{id}-light</div>\
                         <div class=\"mermaid-dark\">{id}-dark</div></figure>"
                    )
                }
                // Unreachable (the branch above excludes `None`); never panic
                // on input, so fall back to an empty payload.
                MermaidMode::None => String::new(),
            };
            token.typ = TokenType::MermaidFigure;
            token.raw = Some(raw);
            continue;
        }
        if options.highlight {
            pending.push(index);
            inputs.push((token.content.clone(), token.info.clone()));
        } else {
            token.raw = Some(plain_fence_html(token));
        }
    }

    if !pending.is_empty() {
        let highlighted = prism::highlight_fences(&inputs)?;
        if highlighted.len() != pending.len() {
            return Err(Error::Markdown(format!(
                "Prism returned {} results for {} code blocks",
                highlighted.len(),
                pending.len()
            )));
        }
        for (slot, html) in pending.into_iter().zip(highlighted) {
            tokens[slot].raw = Some(html);
        }
    }
    Ok(mermaid_diagrams)
}

/// markdown-it's plain `<pre><code>` output when `highlight` is disabled.
///
/// Note: unlike the highlight callback, markdown-it's renderer does *not*
/// lowercase or alias-map the language name here; it uses the first word of the
/// unescaped/trimmed info string verbatim.
fn plain_fence_html(token: &tokens::Token) -> String {
    let content = util::escape_html(&token.content);
    let info = util::unescape_all(&token.info);
    let trimmed = info.trim();
    if trimmed.is_empty() {
        format!("<pre><code>{content}</code></pre>")
    } else {
        let lang_name = trimmed.split_whitespace().next().unwrap_or_default();
        format!(
            "<pre><code class=\"language-{}\">{content}</code></pre>",
            util::escape_html(lang_name)
        )
    }
}

/// Slugify text the same way the original pipeline does:
/// `text.trim().to_lowercase()` with runs of whitespace replaced by `-`.
pub fn slugify(text: &str) -> String {
    let mut slug = String::with_capacity(text.len());
    let mut last_was_space = false;
    for ch in text.trim().chars() {
        if ch.is_whitespace() {
            last_was_space = true;
        } else {
            if last_was_space && !slug.is_empty() {
                slug.push('-');
            }
            last_was_space = false;
            for lower in ch.to_lowercase() {
                slug.push(lower);
            }
        }
    }
    slug
}

/// Percent-encode a string the same way JavaScript's `encodeURIComponent` does.
pub fn encode_uri_component(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'!'
            | b'~'
            | b'*'
            | b'\''
            | b'('
            | b')' => out.push(byte as char),
            _ => {
                out.push('%');
                out.push(
                    char::from_digit(u32::from(byte >> 4), 16)
                        .unwrap_or('0')
                        .to_ascii_uppercase(),
                );
                out.push(
                    char::from_digit(u32::from(byte & 0x0f), 16)
                        .unwrap_or('0')
                        .to_ascii_uppercase(),
                );
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_matches_markdown_it_anchor() {
        assert_eq!(slugify("1. Flynn 分类法"), "1.-flynn-分类法");
        assert_eq!(slugify("  Hello   World  "), "hello-world");
        assert_eq!(
            slugify("RuaPC：安全的 RDMA 零拷贝"),
            "ruapc：安全的-rdma-零拷贝"
        );
    }

    #[test]
    fn encode_uri_component_matches_js() {
        assert_eq!(
            encode_uri_component("abc-DEF_1.2!~*'()"),
            "abc-DEF_1.2!~*'()"
        );
        assert_eq!(
            encode_uri_component("1.-Flynn 分类法"),
            "1.-Flynn%20%E5%88%86%E7%B1%BB%E6%B3%95"
        );
        assert_eq!(encode_uri_component("a/b?c=d&e"), "a%2Fb%3Fc%3Dd%26e");
    }

    fn render_with(markdown: &str, options: MarkdownOptions) -> RenderedMarkdown {
        MarkdownRenderer::new(options)
            .render(markdown)
            .expect("markdown render")
    }

    #[test]
    fn headings_are_collected_in_document_order() {
        let rendered = render_with(
            "# One\n\n## One\n\n### One\n\n#### Four\n\n## `code_x` and *em*\n",
            MarkdownOptions::default(),
        );
        let summary: Vec<(u8, &str, &str)> = rendered
            .headings
            .iter()
            .map(|heading| (heading.level, heading.id.as_str(), heading.text.as_str()))
            .collect();
        assert_eq!(
            summary,
            vec![
                (1, "one", "One"),
                (2, "one-1", "One"),
                (3, "one-2", "One"),
                (4, "four", "Four"),
                (2, "code_x-and-em", "code_x and em"),
            ]
        );
    }

    #[test]
    fn mermaid_static_mode_emits_two_svg_figures() {
        let rendered = render_with(
            "Before.\n\n```mermaid\ngraph TD\n  A --> B\n```\n\nAfter.\n",
            MarkdownOptions::default(),
        );
        assert_eq!(rendered.mermaid_diagrams, 1);
        assert!(rendered
            .html
            .starts_with("<p>Before.</p>\n<figure class=\"mermaid\" data-mermaid-digest=\""));
        assert!(rendered
            .html
            .contains("<div class=\"mermaid-light\"><svg id=\"mermaid-"));
        assert!(rendered
            .html
            .contains("<div class=\"mermaid-dark\"><svg id=\"mermaid-"));
        assert!(rendered.html.ends_with("</figure>\n<p>After.</p>\n"));
        assert_eq!(rendered.html.matches("<svg").count(), 2);
        assert!(!rendered.html.contains("mermaid-source"));
        assert!(!rendered.html.contains("mermaid-placeholder"));
    }

    #[test]
    fn mermaid_placeholder_mode_matches_reference_oracle() {
        // Oracle: `node scripts/reference/render.mjs post.md --mermaid placeholder`
        let rendered = render_with(
            "Before.\n\n```mermaid\ngraph TD\n  A --> B\n```\n\nAfter.\n",
            MarkdownOptions {
                mermaid: MermaidMode::Placeholder,
                ..MarkdownOptions::default()
            },
        );
        assert_eq!(rendered.mermaid_diagrams, 1);
        assert_eq!(
            rendered.html,
            "<p>Before.</p>\n<figure class=\"mermaid\"><div class=\"mermaid-light\">\
             mermaid-0dc68518-0-light</div><div class=\"mermaid-dark\">\
             mermaid-0dc68518-0-dark</div></figure>\n<p>After.</p>\n"
        );
    }

    #[test]
    fn mermaid_disabled_mode_is_a_plain_code_block() {
        let rendered = render_with(
            "```mermaid\ngraph TD\n  A --> B\n```\n",
            MarkdownOptions {
                mermaid: MermaidMode::None,
                ..MarkdownOptions::default()
            },
        );
        assert_eq!(rendered.mermaid_diagrams, 0);
        assert_eq!(
            rendered.html,
            "<pre class=\"language-mermaid\"><code class=\"language-mermaid\">\
             graph TD\n  A --&gt; B\n</code></pre>\n"
        );
    }

    #[test]
    fn math_disabled_leaves_dollar_signs_as_text() {
        let rendered = render_with(
            "Inline $x_1$ and block:\n\n$$\ny = 2\n$$\n",
            MarkdownOptions {
                math: false,
                ..MarkdownOptions::default()
            },
        );
        assert_eq!(
            rendered.html,
            "<p>Inline $x_1$ and block:</p>\n<p>$$\ny = 2\n$$</p>\n"
        );
    }

    #[test]
    fn highlight_disabled_uses_plain_code_blocks() {
        let rendered = render_with(
            "```rust\nlet x = 1;\n```\n\n```\nplain\n```\n\n```Rust\nfn main() {}\n```\n",
            MarkdownOptions {
                highlight: false,
                ..MarkdownOptions::default()
            },
        );
        assert_eq!(
            rendered.html,
            "<pre><code class=\"language-rust\">let x = 1;\n</code></pre>\n\
             <pre><code>plain\n</code></pre>\n\
             <pre><code class=\"language-Rust\">fn main() {}\n</code></pre>\n"
        );
    }

    #[test]
    fn toc_disabled_leaves_the_marker_as_text() {
        let rendered = render_with(
            "Intro.\n\n[TOC]\n\n## Heading\n",
            MarkdownOptions {
                toc: false,
                ..MarkdownOptions::default()
            },
        );
        assert_eq!(rendered.html, "<p>Intro.</p>\n<p>[TOC]</p>\n<h2 id=\"heading\" tabindex=\"-1\"><a href=\"#heading\">Heading</a></h2>\n");
    }

    #[test]
    fn task_lists_render_literally_no_extension() {
        let rendered = render_with("- [x] done\n- [ ] todo\n", MarkdownOptions::default());
        assert_eq!(
            rendered.html,
            "<ul>\n<li>[x] done</li>\n<li>[ ] todo</li>\n</ul>\n"
        );
    }

    #[test]
    fn normalizes_line_endings() {
        assert_eq!(normalize_source("a\r\nb\rc\0d"), "a\nb\nc\u{FFFD}d");
    }
}

#[cfg(test)]
mod parity {
    use std::env;
    use std::path::{Path, PathBuf};

    use super::{MarkdownOptions, MarkdownRenderer, MermaidMode};

    /// Normalise "insignificant" HTML differences for the parity metric:
    /// collapse whitespace runs, drop spaces that only separate tags, and
    /// normalise `&gt;` to `>` (both render identically in text context).
    fn normalize(html: &str) -> String {
        let mut out = String::with_capacity(html.len());
        let mut in_tag = false;
        for ch in html.chars() {
            match ch {
                '<' => {
                    while out.ends_with(' ') {
                        out.pop();
                    }
                    in_tag = true;
                    out.push('<');
                }
                '>' => {
                    in_tag = false;
                    out.push('>');
                }
                _ if in_tag => out.push(ch),
                _ if ch.is_whitespace() => {
                    if !out.ends_with('>') && !out.ends_with(' ') {
                        out.push(' ');
                    }
                }
                _ => out.push(ch),
            }
        }
        let mut out = out.trim().to_string();
        // Mermaid blocks cannot be compared directly: the Node oracle leaves
        // `<pre class="mermaid-placeholder">N</pre>` markers while blogen
        // renders a figure. Replace both by a fixed marker (the mermaid
        // rendering itself is covered by dedicated unit tests).
        out = canonicalize_mermaid(&out);
        let _ = &mut out;
        out
    }

    /// Replace reference placeholders and rendered mermaid figures by a marker.
    fn canonicalize_mermaid(html: &str) -> String {
        let mut out = String::with_capacity(html.len());
        let mut rest = html;
        loop {
            let placeholder = rest.find("<pre class=\"mermaid-placeholder\">");
            let figure = rest.find("<figure class=\"mermaid\">");
            let (start, end_marker) = match (placeholder, figure) {
                (Some(p), Some(f)) => {
                    if p <= f {
                        (p, "</pre>")
                    } else {
                        (f, "</figure>")
                    }
                }
                (Some(p), None) => (p, "</pre>"),
                (None, Some(f)) => (f, "</figure>"),
                (None, None) => {
                    out.push_str(rest);
                    break;
                }
            };
            out.push_str(&rest[..start]);
            out.push_str("«mermaid»");
            let tail = &rest[start..];
            if let Some(offset) = tail.find(end_marker) {
                rest = &tail[offset + end_marker.len()..];
            } else {
                out.push_str(tail);
                break;
            }
        }
        out
    }

    fn first_difference(expected: &str, actual: &str) -> String {
        let expected_bytes = expected.as_bytes();
        let actual_bytes = actual.as_bytes();
        let common = expected_bytes.len().min(actual_bytes.len());
        let mut index = 0;
        while index < common && expected_bytes[index] == actual_bytes[index] {
            index += 1;
        }
        let window = |text: &str| {
            let mut start = index.saturating_sub(120);
            while start > 0 && !text.is_char_boundary(start) {
                start -= 1;
            }
            let mut end = (index + 160).min(text.len());
            while end < text.len() && !text.is_char_boundary(end) {
                end += 1;
            }
            text[start..end].replace('\n', "\\n")
        };
        format!(
            "first difference at byte {index}
expected: …{}…
actual:   …{}…",
            window(expected),
            window(actual)
        )
    }

    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|ext| ext == "md") {
                out.push(path);
            }
        }
        out.sort();
    }

    /// Parity test against the reference renderer.
    ///
    /// Generate the oracle and run the comparison with:
    ///
    /// ```bash
    /// cd scripts/reference && npm install
    /// node render.mjs --corpus /path/to/blog/posts --out-dir /tmp/reference-html
    ///
    /// BLOGEN_POSTS_DIR=/path/to/blog/posts \
    /// BLOGEN_REFERENCE_DIR=/tmp/reference-html \
    /// cargo test --lib markdown::parity -- --nocapture
    /// ```
    ///
    /// Mermaid blocks are compared through placeholders/digests only: the
    /// corpus is rendered with [`MermaidMode::Placeholder`], and the oracle's
    /// `<pre class="mermaid-placeholder">N</pre>` markers are canonicalized to
    /// `«mermaid»`. Static SVG rendering is covered by dedicated unit tests and
    /// by the fixture parity tests in `tests/parity.rs`.
    ///
    /// The reference HTML for each `posts/foo/bar.md` must live at
    /// `<reference>/foo/bar.html`. Mismatching documents are printed (first 40
    /// by default, `BLOGEN_PARITY_LIMIT`) and the test fails below
    /// `BLOGEN_PARITY_THRESHOLD` (default `95`). Set `BLOGEN_PARITY_SHOW_EXACT=1`
    /// to print documents that only differ in insignificant whitespace.
    #[test]
    fn corpus_parity() {
        let (Ok(posts_dir), Ok(reference_dir)) = (
            env::var("BLOGEN_POSTS_DIR"),
            env::var("BLOGEN_REFERENCE_DIR"),
        ) else {
            eprintln!(
                "markdown parity test skipped: set BLOGEN_POSTS_DIR and BLOGEN_REFERENCE_DIR"
            );
            return;
        };
        let posts_dir = PathBuf::from(posts_dir);
        let reference_dir = PathBuf::from(reference_dir);
        let mut files = Vec::new();
        walk(&posts_dir, &mut files);
        // Mermaid rendering is verified separately; compare markdown semantics
        // with the deterministic placeholder mode so this corpus stays cheap.
        let options = MarkdownOptions {
            mermaid: MermaidMode::Placeholder,
            ..MarkdownOptions::default()
        };
        let canonical = canonicalize_mermaid;
        let renderer = MarkdownRenderer::new(options);
        let mut exact = 0usize;
        let mut normalized = 0usize;
        let mut mismatches = Vec::new();
        let mut near_misses = Vec::new();
        for file in &files {
            let post_rel = file.strip_prefix(&posts_dir).expect("walked file");
            let reference_path = reference_dir.join(post_rel).with_extension("html");
            let Ok(reference) = std::fs::read_to_string(&reference_path) else {
                mismatches.push(format!("{}: missing reference HTML", post_rel.display()));
                continue;
            };
            let markdown = std::fs::read_to_string(file).expect("read markdown");
            let output = match renderer.render(&markdown) {
                Ok(html) => html,
                Err(err) => {
                    mismatches.push(format!("{}: render error: {err}", post_rel.display()));
                    continue;
                }
            };
            if canonical(&output.html) == canonical(&reference) {
                exact += 1;
            }
            if normalize(&output.html) == normalize(&reference)
                || canonical(&output.html) == canonical(&reference)
            {
                normalized += 1;
                if output.html != reference {
                    near_misses.push(format!(
                        "{}:\n{}",
                        post_rel.display(),
                        first_difference(&reference, &output.html)
                    ));
                }
            } else {
                mismatches.push(format!(
                    "{}:\n{}",
                    post_rel.display(),
                    first_difference(&normalize(&reference), &normalize(&output.html))
                ));
            }
        }
        let total = files.len();
        let limit: usize = env::var("BLOGEN_PARITY_LIMIT")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(10);
        for mismatch in mismatches.iter().take(limit) {
            eprintln!("---- parity mismatch ----\n{mismatch}\n");
        }
        if env::var("BLOGEN_PARITY_SHOW_EXACT").is_ok() {
            for near_miss in near_misses.iter().take(limit.max(10)) {
                eprintln!("---- normalized-identical, not byte-identical ----\n{near_miss}\n");
            }
        }
        let normalized_percentage = if total == 0 {
            0.0
        } else {
            let basis_points = normalized
                .saturating_mul(10_000)
                .checked_div(total)
                .unwrap_or(0);
            f64::from(u32::try_from(basis_points).unwrap_or(0)) / 100.0
        };
        eprintln!(
            "markdown parity: {normalized}/{total} normalized-identical ({normalized_percentage:.2}%), \
             {exact} byte-identical (mermaid placeholder mode), {} mismatches",
            mismatches.len()
        );
        let threshold = env::var("BLOGEN_PARITY_THRESHOLD")
            .ok()
            .and_then(|value| value.parse::<f64>().ok())
            .unwrap_or(95.0);
        assert!(
            normalized_percentage >= threshold,
            "markdown parity {normalized_percentage:.2}% below threshold {threshold:.2}% \
             ({} mismatching documents)",
            mismatches.len()
        );
    }
}
