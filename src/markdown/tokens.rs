//! Minimal markdown-it-compatible token model.
//!
//! `pulldown-cmark` produces an event stream; [`crate::markdown::parser`]
//! converts it into the flat block-token / nested inline-token structure that
//! `markdown-it` (and therefore the reference pipeline) uses internally. The
//! renderer in [`crate::markdown::render`] mirrors `markdown-it`'s HTML
//! renderer token-by-token, which is what makes the output byte-compatible
//! (newline placement, attribute order, hidden tight-list paragraphs, ...).
//!
//! The token model deliberately follows `markdown-it`:
//!
//! * block tokens form a flat list;
//! * every block that contains inline content has exactly one [`TokenType::Inline`]
//!   token whose `children` hold the inline tokens (nested for emphasis/links);
//! * `nesting` is `1` for opening tags, `-1` for closing tags and `0` for
//!   self-contained tokens;
//! * `hidden` paragraphs are used for tight lists (rendered without `<p>`).

/// The kind of a token; the name doubles as the markdown-it token type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TokenType {
    /// Container for inline tokens (markdown-it's `inline` token).
    Inline,
    /// Plain text (escaped when rendered).
    Text,
    /// An inline code span.
    CodeInline,
    /// A self-contained fenced code block (highlighted later).
    Fence,
    /// An indented code block (never highlighted).
    CodeBlock,
    /// An image (has inline children holding the alt text).
    Image,
    /// `<br>` from two trailing spaces or a trailing backslash.
    Hardbreak,
    /// A newline inside a paragraph (`\n` in output).
    Softbreak,
    /// `<p>`.
    ParagraphOpen,
    /// `</p>`.
    ParagraphClose,
    /// `<h1>`..`<h6>` (id/tabindex attrs added by the anchor pass).
    HeadingOpen,
    /// `</h1>`..`</h6>`.
    HeadingClose,
    /// `<blockquote>`.
    BlockquoteOpen,
    /// `</blockquote>`.
    BlockquoteClose,
    /// `<ul>`.
    BulletListOpen,
    /// `</ul>`.
    BulletListClose,
    /// `<ol>` (with optional `start` attribute).
    OrderedListOpen,
    /// `</ol>`.
    OrderedListClose,
    /// `<li>`.
    ListItemOpen,
    /// `</li>`.
    ListItemClose,
    /// `<hr>`.
    Hr,
    /// `<table>`.
    TableOpen,
    /// `</table>`.
    TableClose,
    /// `<thead>`.
    TheadOpen,
    /// `</thead>`.
    TheadClose,
    /// `<tbody>`.
    TbodyOpen,
    /// `</tbody>`.
    TbodyClose,
    /// `<tr>`.
    TrOpen,
    /// `</tr>`.
    TrClose,
    /// `<th>`.
    ThOpen,
    /// `</th>`.
    ThClose,
    /// `<td>`.
    TdOpen,
    /// `</td>`.
    TdClose,
    /// `<em>`.
    EmOpen,
    /// `</em>`.
    EmClose,
    /// `<strong>`.
    StrongOpen,
    /// `</strong>`.
    StrongClose,
    /// `<s>`.
    SOpen,
    /// `</s>`.
    SClose,
    /// `<a>`.
    LinkOpen,
    /// `</a>`.
    LinkClose,
    /// `<figure tabindex="N">`.
    FigureOpen,
    /// `</figure>`.
    FigureClose,
    /// `<figcaption>`.
    FigcaptionOpen,
    /// `</figcaption>`.
    FigcaptionClose,
    /// Inline math (texmath `math_inline`): `<eq>KaTeX</eq>`.
    MathInline,
    /// Inline display math (texmath `math_inline_double`):
    /// `<section><eqn>KaTeX</eqn></section>`.
    MathSection,
    /// Block display math (texmath `math_block`).
    MathBlock,
    /// Numbered display math (texmath `math_block_eqno`).
    MathBlockEqno,
    /// Opening `<div class="table-of-contents">`.
    TocOpen,
    /// Rendered table-of-contents body (raw HTML).
    TocBody,
    /// Closing `</div>` of the table of contents.
    TocClose,
    /// A fully rendered mermaid figure (raw HTML).
    MermaidFigure,
}

/// One markdown-it-like token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Token {
    /// Token type (also markdown-it's `type`).
    pub typ: TokenType,
    /// HTML tag name for block/nesting tokens (`p`, `h1`, `a`, ...).
    pub tag: &'static str,
    /// `1` for open tags, `-1` for close tags, `0` for leaf tokens.
    pub nesting: i8,
    /// Whether the token is block level (affects newline insertion).
    pub block: bool,
    /// Hidden tokens render to nothing (tight list paragraphs).
    pub hidden: bool,
    /// `(name, value)` attribute pairs in output order.
    pub attrs: Vec<(String, String)>,
    /// Token payload: text, code, TeX source or rendered HTML.
    pub content: String,
    /// Extra data (`info` string for fences, equation number for eqno math).
    pub info: String,
    /// Inline children (only used by [`TokenType::Inline`] and images).
    pub children: Vec<Token>,
    /// Pre-rendered HTML override (fences, mermaid figures, TOC body).
    pub raw: Option<String>,
}

impl Token {
    /// Create a leaf token.
    pub fn leaf(typ: TokenType, tag: &'static str, content: impl Into<String>) -> Self {
        Self::new(typ, tag, 0, false, content.into())
    }

    /// Create a token with explicit nesting.
    pub fn new(
        typ: TokenType,
        tag: &'static str,
        nesting: i8,
        block: bool,
        content: impl Into<String>,
    ) -> Self {
        Self {
            typ,
            tag,
            nesting,
            block,
            hidden: false,
            attrs: Vec::new(),
            content: content.into(),
            info: String::new(),
            children: Vec::new(),
            raw: None,
        }
    }

    /// Create an opening block token.
    pub fn open(typ: TokenType, tag: &'static str) -> Self {
        Self::new(typ, tag, 1, true, "")
    }

    /// Create a closing block token.
    pub fn close(typ: TokenType, tag: &'static str) -> Self {
        Self::new(typ, tag, -1, true, "")
    }

    /// Create an inline opening token.
    pub fn inline_open(typ: TokenType, tag: &'static str) -> Self {
        Self::new(typ, tag, 1, false, "")
    }

    /// Create an inline closing token.
    pub fn inline_close(typ: TokenType, tag: &'static str) -> Self {
        Self::new(typ, tag, -1, false, "")
    }

    /// Append an attribute unconditionally (markdown-it's `attrPush`).
    pub fn attr_push(&mut self, name: &str, value: impl Into<String>) {
        self.attrs.push((name.to_string(), value.into()));
    }

    /// Look up an attribute value.
    pub fn attr_get(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }
}
