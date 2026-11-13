//! Raw-HTML neutralisation for `html: false` semantics.
//!
//! markdown-it is configured with `html: false`, so raw HTML is never passed
//! through: an HTML block is just a paragraph whose text happens to look like
//! markup, and inline tags are ordinary text. `pulldown-cmark` always parses
//! HTML (there is no option to disable it), which matters structurally because
//! CommonMark HTML blocks can *interrupt* a paragraph — something markdown-it
//! with `html: false` never does.
//!
//! This pre-pass removes that structural difference. A `<` that would start an
//! HTML block (first non-space character of a line, indentation < 4) is
//! rewritten to the `&lt;` entity, which the inline parser decodes back to a
//! literal `<` text character and which the renderer escapes again. The bytes
//! therefore match markdown-it's output, while HTML constructs no longer
//! interrupt paragraphs. Real CommonMark autolinks (`<https://…>`,
//! `<user@example.com>`) are left untouched so they still become links.
//!
//! Inline HTML that is not at the start of a line is already handled by the
//! parser (inline HTML events become text tokens).

/// Rewrite line-leading HTML constructs so that they cannot form HTML blocks.
pub(crate) fn protect(markdown: &str) -> String {
    if !markdown.contains('<') {
        return markdown.to_string();
    }
    let mut out = String::with_capacity(markdown.len());
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
        let indent = body.len() - body.trim_start_matches(' ').len();
        let content = &body[indent..];
        if indent <= 3 && content.starts_with('<') && !is_autolink(content) {
            out.push_str(&body[..indent]);
            out.push_str("&lt;");
            out.push_str(&body[indent + 1..]);
            if line.ends_with('\n') {
                out.push('\n');
            }
        } else {
            out.push_str(line);
        }
    }
    out
}

/// Whether `content` (starting with `<`) is a CommonMark autolink.
fn is_autolink(content: &str) -> bool {
    let Some(close) = content.find('>') else {
        return false;
    };
    let inner = &content[1..close];
    if inner.is_empty() || inner.contains('<') {
        return false;
    }
    is_uri_autolink(inner) || is_email_autolink(inner)
}

/// markdown-it's `AUTOLINK_RE`: `^[a-zA-Z][a-zA-Z0-9+.-]{1,31}:[^<>\x00-\x20]*$`.
fn is_uri_autolink(inner: &str) -> bool {
    let bytes = inner.as_bytes();
    if bytes.len() < 3 || !bytes[0].is_ascii_alphabetic() {
        return false;
    }
    let mut index = 1;
    while index < bytes.len()
        && (bytes[index].is_ascii_alphanumeric() || matches!(bytes[index], b'+' | b'.' | b'-'))
    {
        index += 1;
        if index > 32 {
            return false;
        }
    }
    if bytes.get(index) != Some(&b':') {
        return false;
    }
    inner[index + 1..]
        .bytes()
        .all(|byte| !matches!(byte, b'<' | b'>') && byte > 0x20)
}

/// markdown-it's `EMAIL_RE` (simplified but faithful for ASCII addresses).
fn is_email_autolink(inner: &str) -> bool {
    let Some((local, domain)) = inner.split_once('@') else {
        return false;
    };
    if local.is_empty() || domain.is_empty() || local.len() > 64 {
        return false;
    }
    let local_ok = local.bytes().all(|byte| {
        byte.is_ascii_alphanumeric()
            || matches!(
                byte,
                b'.' | b'!'
                    | b'#'
                    | b'$'
                    | b'%'
                    | b'&'
                    | b'\''
                    | b'*'
                    | b'+'
                    | b'/'
                    | b'='
                    | b'?'
                    | b'^'
                    | b'_'
                    | b'`'
                    | b'{'
                    | b'|'
                    | b'}'
                    | b'~'
                    | b'-'
            )
    });
    if !local_ok {
        return false;
    }
    let mut parts = domain.split('.');
    let mut count = 0;
    for part in parts.by_ref() {
        if part.is_empty() || part.len() > 63 {
            return false;
        }
        if !part
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            return false;
        }
        count += 1;
    }
    count >= 1
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neutralizes_line_leading_html_only() {
        let source = "<div>\ntext\n</div>\n\ninline <b>x</b> stays\n";
        let protected = protect(source);
        assert_eq!(
            protected,
            "&lt;div>\ntext\n&lt;/div>\n\ninline <b>x</b> stays\n"
        );
    }

    #[test]
    fn keeps_autolinks_and_code() {
        let source = "<https://a.b/c>\n\n```\n<div>\n```\n";
        assert_eq!(protect(source), source);
    }

    #[test]
    fn keeps_indented_code() {
        let source = "    <div>\n";
        assert_eq!(protect(source), source);
    }
}
