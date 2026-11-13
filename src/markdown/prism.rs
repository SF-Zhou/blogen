//! Prism-based syntax highlighting of fenced code blocks.
//!
//! The original pipeline calls `Prism.highlight(code, Prism.languages[lang])`
//! and returns a complete `<pre class="language-…"><code …>…</code></pre>`
//! string. This module reproduces that contract: the renderer only has to
//! append the trailing newline.
//!
//! Language resolution mirrors `src/marked.js` of the legacy generator:
//!
//! * the info string is unescaped, trimmed and the first whitespace separated
//!   token is the language name, lowercased;
//! * `""` is treated as `markup`; `c++` → `cpp`, `yml` → `yaml`,
//!   `asm`/`assembly` → `nasm`;
//! * when Prism has no grammar (or highlighting throws) the code is escaped
//!   with markdown-it's `escapeHtml` and the class keeps the resolved language.

use crate::error::Result;

use super::js;

/// Extract the language name from a fence info string the same way
/// `markdown-it`'s fence renderer does (and the legacy highlighter lowercases).
pub(crate) fn info_lang(info: &str) -> String {
    let unescaped = unescape_all(info);
    let trimmed = unescaped.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    trimmed
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_lowercase()
}

/// Resolve a language name to the Prism grammar name (legacy alias table).
fn map_language(language: &str) -> String {
    match language {
        "" => "markup".to_string(),
        "c++" => "cpp".to_string(),
        "yml" => "yaml".to_string(),
        "asm" | "assembly" => "nasm".to_string(),
        other => other.to_string(),
    }
}

/// Highlight a batch of fenced code blocks.
///
/// `blocks` holds `(code, info_string)` pairs, where `code` still contains the
/// trailing newline that `markdown-it` keeps in the token content. The result
/// contains one complete `<pre class="language-X"><code class="language-X">…`
/// element per block, in order.
pub(crate) fn highlight_fences(blocks: &[(String, String)]) -> Result<Vec<String>> {
    if blocks.is_empty() {
        return Ok(Vec::new());
    }
    let resolved: Vec<(String, String)> = blocks
        .iter()
        .map(|(code, info)| (code.clone(), map_language(&info_lang(info))))
        .collect();
    let highlighted = js::highlight_code(&resolved)?;
    let mut out = Vec::with_capacity(resolved.len());
    for ((code, language), highlighted) in resolved.iter().zip(highlighted) {
        let body = highlighted.unwrap_or_else(|| escape_markdown_html(code));
        let class = escape_attr(language);
        out.push(format!(
            "<pre class=\"language-{class}\"><code class=\"language-{class}\">{body}</code></pre>"
        ));
    }
    Ok(out)
}

/// Escape text for an HTML attribute value.
fn escape_attr(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(ch),
        }
    }
    out
}

/// markdown-it's `escapeHtml`: escapes `& < > "` only.
fn escape_markdown_html(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(ch),
        }
    }
    out
}

/// markdown-it's `unescapeAll`: backslash escapes and HTML entities.
fn unescape_all(input: &str) -> String {
    if !input.contains('\\') && !input.contains('&') {
        return input.to_string();
    }
    let chars: Vec<char> = input.chars().collect();
    let mut out = String::with_capacity(input.len());
    let mut index = 0;
    while index < chars.len() {
        let ch = chars[index];
        if ch == '\\' && index + 1 < chars.len() && is_escapable(chars[index + 1]) {
            out.push(chars[index + 1]);
            index += 2;
            continue;
        }
        if ch == '&' {
            if let Some((decoded, consumed)) = decode_entity(&chars[index..]) {
                out.push(decoded);
                index += consumed;
                continue;
            }
        }
        out.push(ch);
        index += 1;
    }
    out
}

fn is_escapable(ch: char) -> bool {
    ch.is_ascii_punctuation()
}

/// Decode one HTML entity starting at `chars[0] == '&'`.
///
/// Returns the decoded character and the number of consumed characters.
fn decode_entity(chars: &[char]) -> Option<(char, usize)> {
    let end = chars.iter().position(|&ch| ch == ';')?;
    if !(2..=9).contains(&end) {
        return None;
    }
    let name: String = chars[1..end].iter().collect();
    let decoded = if let Some(hex) = name.strip_prefix("#x").or_else(|| name.strip_prefix("#X")) {
        u32::from_str_radix(hex, 16).ok().and_then(char::from_u32)?
    } else if let Some(decimal) = name.strip_prefix('#') {
        decimal.parse::<u32>().ok().and_then(char::from_u32)?
    } else {
        match name.as_str() {
            "amp" => '&',
            "lt" => '<',
            "gt" => '>',
            "quot" => '"',
            "apos" => '\'',
            "nbsp" => '\u{a0}',
            _ => return None,
        }
    };
    Some((decoded, end + 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn highlight(markdown_code: &str, info: &str) -> String {
        let result =
            highlight_fences(&[(markdown_code.to_string(), info.to_string())]).expect("highlight");
        result.into_iter().next().unwrap()
    }

    #[test]
    fn info_language_extraction() {
        assert_eq!(info_lang("rust"), "rust");
        assert_eq!(info_lang(" Rust "), "rust");
        assert_eq!(info_lang("rust title=example"), "rust");
        assert_eq!(info_lang(""), "");
        assert_eq!(info_lang("c++"), "c++");
        assert_eq!(info_lang("C++"), "c++");
    }

    #[test]
    fn rust_highlighting_matches_oracle() {
        assert_eq!(
            highlight("let x = 1;\n", "rust"),
            "<pre class=\"language-rust\"><code class=\"language-rust\"><span class=\"token keyword\">let</span> x <span class=\"token operator\">=</span> <span class=\"token number\">1</span><span class=\"token punctuation\">;</span>\n</code></pre>"
        );
    }

    #[test]
    fn cpp_alias_matches_oracle() {
        assert_eq!(
            highlight("int a = 0;\n", "c++"),
            "<pre class=\"language-cpp\"><code class=\"language-cpp\"><span class=\"token keyword\">int</span> a <span class=\"token operator\">=</span> <span class=\"token number\">0</span><span class=\"token punctuation\">;</span>\n</code></pre>"
        );
    }

    #[test]
    fn unknown_language_falls_back_to_escaped_text() {
        assert_eq!(
            highlight("select 1;\n", "sql"),
            "<pre class=\"language-sql\"><code class=\"language-sql\">select 1;\n</code></pre>"
        );
        assert_eq!(
            highlight("a < b && c > d \"q\"\n", "haskell"),
            "<pre class=\"language-haskell\"><code class=\"language-haskell\">a &lt; b &amp;&amp; c &gt; d &quot;q&quot;\n</code></pre>"
        );
    }

    #[test]
    fn empty_info_uses_markup_grammar() {
        assert_eq!(
            highlight("plain <text> & stuff\n", ""),
            "<pre class=\"language-markup\"><code class=\"language-markup\">plain <span class=\"token tag\"><span class=\"token tag\"><span class=\"token punctuation\">&lt;</span>text</span><span class=\"token punctuation\">></span></span> &amp; stuff\n</code></pre>"
        );
    }

    #[test]
    fn aliases_match_oracle() {
        assert_eq!(
            highlight("a: 1\n", "yml"),
            "<pre class=\"language-yaml\"><code class=\"language-yaml\"><span class=\"token key atrule\">a</span><span class=\"token punctuation\">:</span> <span class=\"token number\">1</span>\n</code></pre>"
        );
        assert_eq!(
            highlight("mov eax, 1\n", "asm"),
            "<pre class=\"language-nasm\"><code class=\"language-nasm\">mov <span class=\"token register variable\">eax</span>, <span class=\"token number\">1</span>\n</code></pre>"
        );
    }

    #[test]
    fn info_extra_words_are_ignored() {
        assert_eq!(
            highlight("fn main() {}\n", "rust title=x"),
            "<pre class=\"language-rust\"><code class=\"language-rust\"><span class=\"token keyword\">fn</span> <span class=\"token function-definition function\">main</span><span class=\"token punctuation\">(</span><span class=\"token punctuation\">)</span> <span class=\"token punctuation\">{</span><span class=\"token punctuation\">}</span>\n</code></pre>"
        );
    }

    #[test]
    fn unescape_all_handles_escapes_and_entities() {
        assert_eq!(unescape_all("c\\+\\+"), "c++");
        assert_eq!(unescape_all("c&#43;&#43;"), "c++");
        assert_eq!(unescape_all("a&amp;b"), "a&b");
        assert_eq!(unescape_all("\\&amp;"), "&amp;");
    }

    #[test]
    fn empty_batch() {
        assert_eq!(highlight_fences(&[]).unwrap().len(), 0);
    }
}
