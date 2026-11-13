//! Small string helpers shared by the markdown pipeline.
//!
//! Everything here mirrors a specific markdown-it helper so that the rendered
//! bytes match the reference implementation:
//!
//! * [`escape_html`] is markdown-it's `escapeHtml` (`&`, `<`, `>`, `"` — note
//!   that `'` is *not* escaped);
//! * [`normalize_link`] is `mdurl.encode` after the URL parser round-trip (the
//!   hostname/punycode part of `mdurl` is not reproduced, see the module docs
//!   of [`crate::markdown`]);
//! * [`normalize_link_text`] is `mdurl.decode` with markdown-it's default
//!   exclusion set;
//! * [`unescape_all`] decodes backslash escapes and HTML entities like
//!   markdown-it's `unescapeAll`.

/// Escape text exactly like markdown-it's `escapeHtml` (`'` is left alone).
pub(crate) fn escape_html(input: &str) -> String {
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

/// Characters that `mdurl.encode` leaves untouched (its `defaultChars`).
const ENCODE_ALLOWED: &str = ";/?:@&=+$,-_.!~*'()#";

/// Percent-encode a URL like markdown-it's `normalizeLink` (`mdurl.encode`).
///
/// Existing `%XX` escape sequences are preserved; every other byte outside the
/// `mdurl` default character set is encoded as uppercase hex. Non-ASCII text is
/// encoded as UTF-8 (the reference implementation additionally punycode-encodes
/// non-ASCII hostnames; see the known-gaps note in the module documentation).
pub(crate) fn normalize_link(url: &str) -> String {
    let bytes = url.trim().as_bytes();
    let mut out = String::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte == b'%'
            && index + 2 < bytes.len()
            && bytes[index + 1].is_ascii_hexdigit()
            && bytes[index + 2].is_ascii_hexdigit()
        {
            out.push('%');
            out.push(bytes[index + 1] as char);
            out.push(bytes[index + 2] as char);
            index += 3;
            continue;
        }
        if byte.is_ascii_alphanumeric() || ENCODE_ALLOWED.as_bytes().contains(&byte) {
            out.push(byte as char);
        } else if byte < 0x80 {
            out.push('%');
            out.push(hex_digit(byte >> 4));
            out.push(hex_digit(byte & 0x0f));
        } else {
            // Encode the whole UTF-8 code point.
            let ch = url[index..].chars().next().unwrap_or('\u{FFFD}');
            let len = ch.len_utf8();
            for encoded in &bytes[index..index + len] {
                out.push('%');
                out.push(hex_digit(encoded >> 4));
                out.push(hex_digit(encoded & 0x0f));
            }
            index += len;
            continue;
        }
        index += 1;
    }
    out
}

fn hex_digit(nibble: u8) -> char {
    match nibble {
        0..=9 => (b'0' + nibble) as char,
        _ => (b'A' + nibble - 10) as char,
    }
}

/// Characters that `mdurl.decode` does not decode (its `defaultChars`).
const DECODE_EXCLUDED: &str = ";/?:@&=+$,#";

/// Decode percent escapes like markdown-it's `normalizeLinkText`.
///
/// `mdurl.parse`/`format` round-trip is the identity for the URLs the pipeline
/// can see, so only the `decode` step is reproduced. `%` is never decoded (the
/// reference passes `'%'` in the exclusion set).
pub(crate) fn normalize_link_text(url: &str) -> String {
    let mut out = String::with_capacity(url.len());
    let bytes = url.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let (Some(high), Some(low)) =
                (hex_value(bytes[index + 1]), hex_value(bytes[index + 2]))
            {
                let value = high * 16 + low;
                let ch = value as char;
                if ch.is_ascii() && DECODE_EXCLUDED.contains(ch) {
                    // Excluded characters keep their (canonical uppercase) escape.
                    out.push('%');
                    out.push(hex_digit(high));
                    out.push(hex_digit(low));
                } else {
                    out.push(ch);
                }
                index += 3;
                continue;
            }
        }
        let ch = url[index..].chars().next().unwrap_or('\u{FFFD}');
        out.push(ch);
        index += ch.len_utf8();
    }
    out
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// markdown-it's `validateLink`: refuse dangerous protocols.
pub(crate) fn validate_link(url: &str) -> bool {
    let lower = url.trim().to_ascii_lowercase();
    if lower.starts_with("vbscript:")
        || lower.starts_with("javascript:")
        || lower.starts_with("file:")
        || lower.starts_with("data:")
    {
        return lower.starts_with("data:image/gif;")
            || lower.starts_with("data:image/png;")
            || lower.starts_with("data:image/jpeg;")
            || lower.starts_with("data:image/webp;");
    }
    true
}

/// ASCII punctuation that a markdown backslash escape can escape
/// (markdown-it's `ESCAPED` table).
fn is_escapable(byte: u8) -> bool {
    matches!(
        byte,
        b'\\'
            | b'!'
            | b'"'
            | b'#'
            | b'$'
            | b'%'
            | b'&'
            | b'\''
            | b'('
            | b')'
            | b'*'
            | b'+'
            | b','
            | b'-'
            | b'.'
            | b'/'
            | b':'
            | b';'
            | b'<'
            | b'='
            | b'>'
            | b'?'
            | b'@'
            | b'['
            | b']'
            | b'^'
            | b'_'
            | b'`'
            | b'{'
            | b'|'
            | b'}'
            | b'~'
    )
}

/// markdown-it's `unescapeAll`: decode backslash escapes and HTML entities.
pub(crate) fn unescape_all(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.char_indices().peekable();
    while let Some((index, ch)) = chars.next() {
        if ch == '\\' {
            if let Some(&(_, next)) = chars.peek() {
                if next.is_ascii() && is_escapable(next as u8) {
                    out.push(next);
                    chars.next();
                    continue;
                }
            }
            out.push('\\');
            continue;
        }
        if ch == '&' {
            // Try to decode a numeric or named entity starting at `index`.
            if let Some((decoded, consumed)) = decode_entity(&input[index..]) {
                out.push(decoded);
                // The leading '&' was already consumed by the outer `next()`.
                for _ in 1..consumed {
                    chars.next();
                }
                continue;
            }
        }
        out.push(ch);
    }
    out
}

/// Decode one HTML entity at the start of `input`; returns the decoded
/// character and the number of *chars* consumed.
fn decode_entity(input: &str) -> Option<(char, usize)> {
    let bytes = input.as_bytes();
    if bytes.len() < 3 || bytes[0] != b'&' {
        return None;
    }
    // Find the terminating ';' within the maximum named entity length.
    let limit = bytes.len().min(33);
    let semicolon = bytes[..limit].iter().position(|&b| b == b';')?;
    let name = &input[1..semicolon];
    if name.is_empty() {
        return None;
    }
    let decoded = if let Some(rest) = name.strip_prefix('#') {
        let (radix, digits) = if let Some(hex) = rest.strip_prefix(['x', 'X']) {
            (16, hex)
        } else {
            (10, rest)
        };
        let value = u32::from_str_radix(digits, radix).ok()?;
        let ch = char::from_u32(value).unwrap_or('\u{FFFD}');
        // CommonMark replaces invalid code points with U+FFFD.
        if value == 0 || (0xD800..=0xDFFF).contains(&value) || value > 0x0010_FFFF {
            '\u{FFFD}'
        } else {
            ch
        }
    } else {
        named_entity(name)?
    };
    Some((decoded, semicolon + 1))
}

/// The handful of named entities we decode for fence info strings. The full
/// CommonMark entity table is only needed for exotic fence labels; unknown
/// entities are left as literal text, which matches markdown-it for *invalid*
/// entities and can only differ for exotic valid ones (documented gap).
fn named_entity(name: &str) -> Option<char> {
    Some(match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => '\u{a0}',
        _ => return None,
    })
}

/// JavaScript's `\s` class (used by the texmath inline guards and block rule).
pub(crate) fn is_js_space(ch: char) -> bool {
    matches!(
        ch,
        '\t' | '\n' | '\u{0B}' | '\u{0C}' | '\r' | ' ' | '\u{A0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200A}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202F}'
                | '\u{205F}'
                | '\u{3000}'
                | '\u{FEFF}'
    )
}

/// markdown-it's `getTokensText` used by markdown-it-anchor and
/// markdown-it-table-of-contents: concatenate `text` and `code_inline` token
/// contents (inline containers are flattened; image alt text is *not*
/// included because markdown-it's flat token list keeps it in the image
/// token's own children).
pub(crate) fn get_tokens_text(tokens: &[super::tokens::Token]) -> String {
    let mut out = String::new();
    collect_text(tokens, &mut out);
    out
}

/// Recursive helper for [`get_tokens_text`].
fn collect_text(tokens: &[super::tokens::Token], out: &mut String) {
    use super::tokens::TokenType;
    for token in tokens {
        match token.typ {
            TokenType::Text | TokenType::CodeInline => out.push_str(&token.content),
            TokenType::Image => {}
            _ => collect_text(&token.children, out),
        }
    }
}

/// markdown-it's `renderInlineAsText` for image alt attributes: keep text,
/// recurse into images, turn breaks into newlines and skip everything else.
pub(crate) fn render_inline_as_text(tokens: &[super::tokens::Token]) -> String {
    use super::tokens::TokenType;
    let mut out = String::new();
    for token in tokens {
        match token.typ {
            TokenType::Text => out.push_str(&token.content),
            TokenType::Image => out.push_str(&render_inline_as_text(&token.children)),
            TokenType::Softbreak | TokenType::Hardbreak => out.push('\n'),
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_like_markdown_it() {
        assert_eq!(escape_html("a&b<c>d\"e'f"), "a&amp;b&lt;c&gt;d&quot;e'f");
    }

    #[test]
    fn normalizes_links_like_mdurl() {
        assert_eq!(
            normalize_link("https://a.b/c?d=e&f=g#h"),
            "https://a.b/c?d=e&f=g#h"
        );
        assert_eq!(normalize_link("a b"), "a%20b");
        assert_eq!(
            normalize_link("https://例え.jp"),
            "https://%E4%BE%8B%E3%81%88.jp"
        );
        assert_eq!(normalize_link("/x(%20y)"), "/x(%20y)");
        assert_eq!(normalize_link("https://a.b/%2Fkeep"), "https://a.b/%2Fkeep");
    }

    #[test]
    fn decodes_link_text() {
        assert_eq!(normalize_link_text("https://a.b/c%20d"), "https://a.b/c d");
        assert_eq!(normalize_link_text("https://a.b"), "https://a.b");
        assert_eq!(normalize_link_text("/a%2Fb%20c"), "/a%2Fb c");
    }

    #[test]
    fn validates_links() {
        assert!(validate_link("https://a.b"));
        assert!(!validate_link("javascript:alert(1)"));
        assert!(validate_link("data:image/png;base64,AAA"));
        assert!(!validate_link("data:text/html,AAA"));
    }

    #[test]
    fn unescapes_fence_info() {
        assert_eq!(unescape_all("c\\+\\+"), "c++");
        assert_eq!(unescape_all(" rust "), " rust ");
        assert_eq!(unescape_all("a&amp;b"), "a&b");
        assert_eq!(unescape_all("&#65;"), "A");
        assert_eq!(unescape_all("no\\escape"), "no\\escape");
    }

    #[test]
    fn js_whitespace() {
        assert!(is_js_space(' '));
        assert!(is_js_space('\u{3000}'));
        assert!(is_js_space('\u{FEFF}'));
        assert!(!is_js_space('a'));
    }
}
