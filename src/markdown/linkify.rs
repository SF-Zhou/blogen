//! `linkify: true` support: bare URLs, `www.`/domain links and e-mail
//! addresses in text tokens.
//!
//! The reference pipeline enables markdown-it's `linkify` option, which uses
//! [`linkify-it`](https://github.com/markdown-it/linkify-it) (`fuzzyLink` and
//! `fuzzyEmail` on, `fuzzyIP` off) and markdown-it's core `linkify` rule. This
//! module re-implements the relevant subset in Rust:
//!
//! * most of linkify-it's `match()`: explicit `http:`/`https:`/`ftp:`/`//`/
//!   `mailto:` schemas, fuzzy links (`example.com/path`) and fuzzy e-mails
//!   (`user@example.com`), with linkify-it's trailing-punctuation rules;
//! * markdown-it's token splitting and `normalizeLink`/`normalizeLinkText`
//!   handling of `href` and display text.
//!
//! # Known gaps
//!
//! * Unicode general categories are approximated (punctuation ranges cover
//!   ASCII, general punctuation, CJK and fullwidth forms) so that path/host
//!   boundaries behave like linkify-it for typical text;
//! * linkify-it's "text_special" guard after a backslash escape is not
//!   reproduced;
//! * markdown-it's *inline* linkify rule (which protects a URL from markdown
//!   markup before emphasis parsing) is not reproduced: a bare URL containing
//!   characters that CommonMark interprets as emphasis (`_`, `*`) can be
//!   parsed slightly differently.

use super::tokens::{Token, TokenType};
use super::util::{normalize_link, normalize_link_text, validate_link};

/// Apply linkification to all block-level inline token lists.
pub(crate) fn apply(tokens: &mut [Token]) {
    for token in tokens.iter_mut() {
        if token.typ == TokenType::Inline {
            linkify_children(&mut token.children);
        }
    }
}

/// Linkify a flat inline token list, skipping link contents and image alt
/// text (markdown-it's core rule only scans text tokens at "link level 0";
/// inline containers are flat in the token model, just like markdown-it).
fn linkify_children(children: &mut Vec<Token>) {
    let mut index = 0;
    let mut link_depth = 0usize;
    while index < children.len() {
        match children[index].typ {
            TokenType::LinkOpen => {
                link_depth += 1;
                index += 1;
            }
            TokenType::LinkClose => {
                link_depth = link_depth.saturating_sub(1);
                index += 1;
            }
            TokenType::Text if link_depth == 0 => {
                let text = std::mem::take(&mut children[index].content);
                let replacement = split_text(&text);
                if replacement.len() > 1 {
                    let new_len = replacement.len();
                    children.splice(index..=index, replacement);
                    index += new_len;
                } else {
                    children[index].content = text;
                    index += 1;
                }
            }
            _ => index += 1,
        }
    }
}

/// Split a text token into text/link tokens using linkify-it + markdown-it's
/// core linkify rule.
fn split_text(text: &str) -> Vec<Token> {
    let matches = find_links(text);
    if matches.is_empty() {
        return vec![Token::leaf(TokenType::Text, "", text.to_string())];
    }
    let mut out = Vec::new();
    let mut last = 0;
    for link in matches {
        let href = normalize_link(&link.url);
        if !validate_link(&href) {
            continue;
        }
        if link.start > last {
            out.push(Token::leaf(
                TokenType::Text,
                "",
                text[last..link.start].to_string(),
            ));
        }
        let mut open = Token::inline_open(TokenType::LinkOpen, "a");
        open.attr_push("href", href);
        out.push(open);
        out.push(Token::leaf(TokenType::Text, "", link.display));
        out.push(Token::inline_close(TokenType::LinkClose, "a"));
        last = link.end;
    }
    if last < text.len() {
        out.push(Token::leaf(TokenType::Text, "", text[last..].to_string()));
    }
    out
}

/// One detected link, mirroring linkify-it's `Match` after normalisation.
struct FoundLink {
    start: usize,
    end: usize,
    /// Href before markdown-it's `normalizeLink`.
    url: String,
    /// Display text before markdown-it's `normalizeLinkText`.
    display: String,
}

/// A raw candidate from one of linkify-it's three scans.
#[derive(Clone, Copy)]
struct Candidate {
    start: usize,
    end: usize,
    kind: CandidateKind,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CandidateKind {
    /// Explicit schema (`http:`, `https:`, `ftp:`, `//`, `mailto:`).
    Schemed,
    /// Fuzzy link without schema (`example.com`).
    FuzzyLink,
    /// Fuzzy e-mail without `mailto:`.
    FuzzyEmail,
}

/// linkify-it's `match()`: collect candidates from the three scans, then merge
/// them by earliest start (longer wins on ties), skipping overlaps.
fn find_links(text: &str) -> Vec<FoundLink> {
    let bytes = text.as_bytes();
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut schemed = Vec::new();
    let mut fuzzy_links = Vec::new();
    let mut fuzzy_emails = Vec::new();
    for (offset, ch) in &chars {
        let pos = *offset;
        let _ = ch;
        // Explicit schemas.
        if schema_boundary(text, pos) {
            if let Some(end) = match_schema(text, bytes, pos) {
                schemed.push(Candidate {
                    start: pos,
                    end,
                    kind: CandidateKind::Schemed,
                });
            }
        }
        // Fuzzy links.
        if fuzzy_link_boundary(text, pos) {
            if let Some(end) = match_fuzzy_link(text, bytes, pos) {
                fuzzy_links.push(Candidate {
                    start: pos,
                    end,
                    kind: CandidateKind::FuzzyLink,
                });
            }
        }
        // Fuzzy e-mails.
        if fuzzy_email_boundary(text, pos) {
            if let Some(end) = match_fuzzy_email(text, bytes, pos) {
                fuzzy_emails.push(Candidate {
                    start: pos,
                    end,
                    kind: CandidateKind::FuzzyEmail,
                });
            }
        }
    }
    let mut found = Vec::new();
    let (mut i, mut j, mut k) = (0, 0, 0);
    let mut last_end = 0;
    loop {
        let candidates = [
            schemed.get(i).copied(),
            fuzzy_emails.get(j).copied(),
            fuzzy_links.get(k).copied(),
        ];
        let chosen = candidates
            .into_iter()
            .flatten()
            .min_by(|a, b| a.start.cmp(&b.start).then_with(|| b.end.cmp(&a.end)));
        let Some(chosen) = chosen else { break };
        // Advance the scan the candidate came from.
        if schemed
            .get(i)
            .is_some_and(|c| c.start == chosen.start && c.end == chosen.end)
        {
            i += 1;
        }
        if fuzzy_emails
            .get(j)
            .is_some_and(|c| c.start == chosen.start && c.end == chosen.end)
        {
            j += 1;
        }
        if fuzzy_links
            .get(k)
            .is_some_and(|c| c.start == chosen.start && c.end == chosen.end)
        {
            k += 1;
        }
        if chosen.start < last_end {
            continue;
        }
        let raw = &text[chosen.start..chosen.end];
        let (url, display) = match chosen.kind {
            CandidateKind::Schemed => (raw.to_string(), normalize_link_text(raw)),
            CandidateKind::FuzzyLink => {
                let url = format!("http://{raw}");
                let display = normalize_link_text(&url);
                (url, display["http://".len()..].to_string())
            }
            CandidateKind::FuzzyEmail => {
                let url = format!("mailto:{raw}");
                let display = normalize_link_text(&url);
                (url, display["mailto:".len()..].to_string())
            }
        };
        found.push(FoundLink {
            start: chosen.start,
            end: chosen.end,
            url,
            display,
        });
        last_end = chosen.end;
    }
    found
}

/// Character before `pos` (ASCII/char aware).
fn prev_char(text: &str, pos: usize) -> Option<char> {
    text[..pos].chars().next_back()
}

/// linkify-it schema boundary: start of text, or a preceding `>`, `<`, `｜` or
/// ZPCc character that is not `_`.
fn schema_boundary(text: &str, pos: usize) -> bool {
    match prev_char(text, pos) {
        None => true,
        Some(ch) => ch != '_' && (is_text_separator(ch) || is_zpcc(ch)),
    }
}

/// linkify-it fuzzy-link boundary.
fn fuzzy_link_boundary(text: &str, pos: usize) -> bool {
    match prev_char(text, pos) {
        None => true,
        Some(ch) => {
            if matches!(ch, '.' | ':' | '/' | '-' | '_' | '@') {
                return false;
            }
            matches!(
                ch,
                '$' | '+' | '<' | '=' | '>' | '^' | '`' | '|' | '\u{FF5C}'
            ) || is_zpcc(ch)
        }
    }
}

/// linkify-it fuzzy-e-mail boundary: start, or `>`, `<`, `｜`, `"`, `(` or ZCc.
fn fuzzy_email_boundary(text: &str, pos: usize) -> bool {
    match prev_char(text, pos) {
        None => true,
        Some(ch) => matches!(ch, '>' | '<' | '\u{FF5C}' | '"' | '(') || is_zcc(ch),
    }
}

/// Match one of linkify-it's explicit schemas at `pos`.
fn match_schema(text: &str, bytes: &[u8], pos: usize) -> Option<usize> {
    // Schemas are tried by the schema_search regex; the regex alternation order
    // is the compiled schema object order (http:, https:, ftp:, //, mailto:),
    // but a leftmost-longest candidate list is what linkify-it builds anyway.
    const HTTP: &str = "http:";
    const HTTPS: &str = "https:";
    const FTP: &str = "ftp:";
    if starts_with_ci(bytes, pos, HTTP) {
        if let Some(end) = match_http_url(text, bytes, pos + HTTP.len()) {
            return Some(end);
        }
    }
    if starts_with_ci(bytes, pos, HTTPS) {
        if let Some(end) = match_http_url(text, bytes, pos + HTTPS.len()) {
            return Some(end);
        }
    }
    if starts_with_ci(bytes, pos, FTP) {
        if let Some(end) = match_http_url(text, bytes, pos + FTP.len()) {
            return Some(end);
        }
    }
    if starts_with_ci(bytes, pos, "//") {
        if let Some(end) = match_protocol_relative(text, bytes, pos + 2) {
            return Some(end);
        }
    }
    if starts_with_ci(bytes, pos, "mailto:") {
        if let Some(end) = match_mailto(text, bytes, pos + "mailto:".len()) {
            return Some(end);
        }
    }
    None
}

/// `//auth host:port path` after the schema.
fn match_http_url(text: &str, bytes: &[u8], pos: usize) -> Option<usize> {
    if !starts_with_ci(bytes, pos, "//") {
        return None;
    }
    match_host_url(text, bytes, pos + 2)
}

/// linkify-it's `//` (protocol-neutral) schema validator.
fn match_protocol_relative(text: &str, bytes: &[u8], pos: usize) -> Option<usize> {
    // "should not be `://` & `///`, that protects from errors in protocol name"
    if pos >= 3 {
        let before = bytes.get(pos - 3).copied()?;
        if before == b':' || before == b'/' {
            return None;
        }
    }
    // Don't allow single-level domains: localhost or a dotted domain.
    if starts_with_ci(bytes, pos, "localhost") {
        let end = pos + "localhost".len();
        for port_end in match_port_candidates(text, bytes, end) {
            if host_terminator(text, bytes, port_end) {
                return Some(match_path(text, port_end));
            }
        }
        return None;
    }
    // Require at least `domain.domain`.
    let (host_end, labels) = parse_domain_labels(text, pos)?;
    if labels < 2 {
        return None;
    }
    for port_end in match_port_candidates(text, bytes, host_end) {
        if host_terminator(text, bytes, port_end) {
            return Some(match_path(text, port_end));
        }
    }
    None
}

/// `http(s)`/ftp style: optional auth, host, port, terminator, path.
fn match_host_url(text: &str, bytes: &[u8], pos: usize) -> Option<usize> {
    let mut pos = match_auth(bytes, pos);
    let (host_end, _labels) = parse_domain_labels(text, pos)?;
    pos = host_end;
    let mut result = None;
    for port_end in match_port_candidates(text, bytes, pos) {
        if host_terminator(text, bytes, port_end) {
            result = Some(match_path(text, port_end));
            break;
        }
    }
    result
}

/// `mailto:` schema validator (optionally with auth-like `user@host`).
fn match_mailto(text: &str, bytes: &[u8], pos: usize) -> Option<usize> {
    let name_end = parse_email_name(text, bytes, pos)?;
    if bytes.get(name_end) != Some(&b'@') {
        return None;
    }
    let (host_end, labels) = parse_domain_labels(text, name_end + 1)?;
    let _ = labels;
    if !host_terminator(text, bytes, host_end) {
        return None;
    }
    Some(host_end)
}

/// Fuzzy e-mail `name@tld-host`.
fn match_fuzzy_email(text: &str, bytes: &[u8], pos: usize) -> Option<usize> {
    let name_end = parse_email_name(text, bytes, pos)?;
    if bytes.get(name_end) != Some(&b'@') {
        return None;
    }
    match_fuzzy_host(text, bytes, name_end + 1)
}

/// Fuzzy link `host:port path` where host is an IP or a TLD-suffixed domain.
fn match_fuzzy_link(text: &str, bytes: &[u8], pos: usize) -> Option<usize> {
    let first = text[pos..].chars().next()?;
    if matches!(
        first,
        '$' | '+' | '<' | '=' | '>' | '^' | '`' | '|' | '\u{FF5C}'
    ) {
        return None;
    }
    match_fuzzy_host(text, bytes, pos)
}

/// linkify-it's `tpl_host_port_fuzzy_strict` + `src_path`.
fn match_fuzzy_host(text: &str, bytes: &[u8], pos: usize) -> Option<usize> {
    // IP address alternative first, then `(domain.)+tld`.
    if let Some(ip_end) = match_ip4(text, pos) {
        for port_end in match_port_candidates(text, bytes, ip_end) {
            if host_terminator(text, bytes, port_end) {
                return Some(match_path(text, port_end));
            }
        }
    }
    let (labels_end, labels) = parse_domain_labels(text, pos)?;
    if labels < 2 {
        return None;
    }
    // Try candidate TLDs from the longest label suffix to the shortest.
    let mut label_ranges = Vec::new();
    let mut scan = pos;
    while scan < labels_end {
        let end = match_label_end(text, scan)?;
        label_ranges.push((scan, end));
        scan = end;
        if bytes.get(scan) == Some(&b'.') {
            scan += 1;
        } else {
            break;
        }
    }
    for split in (1..label_ranges.len()).rev() {
        let (_, tld_end) = label_ranges[split];
        let tld = &text[label_ranges[split].0..tld_end];
        if !is_tld(tld) {
            continue;
        }
        for port_end in match_port_candidates(text, bytes, tld_end) {
            if host_terminator(text, bytes, port_end) {
                return Some(match_path(text, port_end));
            }
        }
    }
    None
}

/// Parse `(domain\.)*domain` starting at `pos`; returns the end offset and the
/// number of labels consumed.
fn parse_domain_labels(text: &str, pos: usize) -> Option<(usize, usize)> {
    let mut scan = pos;
    let mut labels = 0;
    loop {
        let end = match_label_end(text, scan)?;
        labels += 1;
        scan = end;
        if text.as_bytes().get(scan) == Some(&b'.') {
            let next = scan + 1;
            if match_label_end(text, next).is_none() {
                return Some((end, labels));
            }
            scan = next;
        } else {
            return Some((end, labels));
        }
    }
}

/// End of one domain label at `pos` (`src_domain`).
fn match_label_end(text: &str, pos: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    if pos >= bytes.len() {
        return None;
    }
    // `xn--[a-z0-9-]{1,59}`
    if starts_with_ci(bytes, pos, "xn--") {
        let mut scan = pos + 4;
        let mut count = 0;
        while scan < bytes.len() && (bytes[scan].is_ascii_alphanumeric() || bytes[scan] == b'-') {
            scan += 1;
            count += 1;
            if count >= 59 {
                break;
            }
        }
        if count >= 1 {
            return Some(scan);
        }
        return None;
    }
    // `pseudo_letter` or `pseudo_letter (pseudo_letter | -){0,61} pseudo_letter`
    let first = text[pos..].chars().next()?;
    if !is_pseudo_letter(first) {
        return None;
    }
    let mut end = pos + first.len_utf8();
    let mut inner = 0;
    let mut last_valid = end;
    while inner < 61 {
        let Some(ch) = text[end..].chars().next() else {
            break;
        };
        if ch == '-' || is_pseudo_letter(ch) {
            end += ch.len_utf8();
            inner += 1;
            if is_pseudo_letter(ch) {
                last_valid = end;
            }
        } else {
            break;
        }
    }
    // Greedy match must end on a pseudo-letter; the regex only has the
    // single-char and the (>=2 chars, dash-allowed) forms.
    Some(last_valid)
}

/// `src_auth`: optional `user:pass@` (max 50 chars, no whitespace/@/slash/
/// brackets/parens).
fn match_auth(bytes: &[u8], pos: usize) -> usize {
    let mut scan = pos;
    let mut count = 0;
    while scan < bytes.len() && count < 50 {
        let byte = bytes[scan];
        let ch = byte as char;
        if byte >= 0x80 {
            break;
        }
        if ch.is_ascii_whitespace()
            || ch.is_control()
            || matches!(ch, '@' | '/' | '[' | ']' | '(' | ')')
        {
            break;
        }
        scan += 1;
        count += 1;
    }
    if count >= 1 && bytes.get(scan) == Some(&b'@') {
        scan + 1
    } else {
        pos
    }
}

/// `src_email_name`: `class1 class2{0,63}`.
fn parse_email_name(text: &str, bytes: &[u8], pos: usize) -> Option<usize> {
    let mut scan = pos;
    let mut count = 0;
    while scan < bytes.len() {
        let ch = bytes[scan] as char;
        let allowed = if count == 0 {
            ch.is_ascii_alphanumeric()
                || matches!(
                    ch,
                    '-' | ';' | ':' | '&' | '=' | '+' | '$' | ',' | '.' | '_'
                )
        } else {
            ch.is_ascii_alphanumeric()
                || matches!(
                    ch,
                    '-' | ';' | ':' | '&' | '=' | '+' | '$' | ',' | '.' | '_' | '"'
                )
        };
        if !allowed || count >= 64 {
            break;
        }
        scan += 1;
        count += 1;
    }
    let _ = text;
    if count == 0 {
        None
    } else {
        Some(scan)
    }
}

/// linkify-it's host terminator lookahead.
fn host_terminator(text: &str, bytes: &[u8], pos: usize) -> bool {
    let Some(ch) = text[pos..].chars().next() else {
        return true;
    };
    if !(is_text_separator(ch) || is_zpcc(ch)) {
        return false;
    }
    match ch {
        '-' | '_' => false,
        ':' => {
            let mut iter = text[pos + 1..].chars();
            !matches!(iter.next(), Some(next) if next.is_ascii_digit())
        }
        '.' => match text[pos + 1..].chars().next() {
            None => true,
            Some(next) => is_zpcc(next),
        },
        _ => {
            let _ = bytes;
            true
        }
    }
}

/// `src_port`: all candidate end offsets (longest first) of an optional port.
fn match_port_candidates(text: &str, bytes: &[u8], pos: usize) -> Vec<usize> {
    if bytes.get(pos) != Some(&b':') {
        return vec![pos];
    }
    let mut digit_end = pos + 1;
    while digit_end < bytes.len() && bytes[digit_end].is_ascii_digit() {
        digit_end += 1;
    }
    let digits = digit_end - (pos + 1);
    if digits == 0 {
        return vec![pos];
    }
    let mut candidates = Vec::new();
    let max_len = digits.min(5);
    for len in (1..=max_len).rev() {
        let slice = &text[pos + 1..pos + 1 + len];
        if is_port(slice) {
            candidates.push(pos + 1 + len);
        }
    }
    candidates.push(pos);
    candidates
}

/// The port sub-pattern `6(...)|[1-5]?\d{1,4}` (approximated for 6xxxx).
fn is_port(digits: &str) -> bool {
    let bytes = digits.as_bytes();
    if bytes.is_empty() || !bytes.iter().all(u8::is_ascii_digit) {
        return false;
    }
    match bytes.len() {
        1..=4 => true,
        5 => match bytes[0] {
            b'1'..=b'5' => true,
            b'6' => digits.parse::<u32>().is_ok_and(|value| value <= 65_535),
            _ => false,
        },
        _ => false,
    }
}

/// `src_path`: optional `[/?#]` followed by path atoms, or a lone `/`.
fn match_path(text: &str, pos: usize) -> usize {
    let Some(first) = text[pos..].chars().next() else {
        return pos;
    };
    if !matches!(first, '/' | '?' | '#') {
        return pos;
    }
    let mut scan = pos + first.len_utf8();
    let atoms_start = scan;
    while let Some(next) = match_path_atom(text, scan) {
        scan = next;
    }
    if scan > atoms_start {
        scan
    } else if first == '/' {
        pos + 1
    } else {
        pos
    }
}

/// Try to match one path atom at `pos`; returns the new offset.
fn match_path_atom(text: &str, pos: usize) -> Option<usize> {
    let ch = text[pos..].chars().next()?;
    if is_zcc(ch) || is_text_separator(ch) {
        return None;
    }
    let next = text[pos + ch.len_utf8()..].chars().next();
    match ch {
        '[' => match_group(text, pos, '[', ']'),
        '(' => match_group(text, pos, '(', ')'),
        '{' => match_group(text, pos, '{', '}'),
        '"' => match_quoted(text, pos, '"'),
        '\'' => match_quoted(text, pos, '\'').or_else(|| match next {
            Some(next) if is_pseudo_letter(next) || next == '-' => Some(pos + 1),
            _ => None,
        }),
        '.' => {
            if next == Some('.') {
                let mut scan = pos;
                while text[scan..].starts_with('.') {
                    scan += 1;
                }
                let after = text[scan..].chars().next()?;
                if scan - pos >= 2
                    && (after.is_ascii_alphanumeric() || matches!(after, '%' | '/' | '&'))
                {
                    return Some(scan + after.len_utf8());
                }
                None
            } else if matches!(next, Some('.')) || next.is_none_or(is_zcc) {
                None
            } else {
                Some(pos + 1)
            }
        }
        '-' => {
            let mut scan = pos;
            while text[scan..].starts_with('-') {
                scan += 1;
            }
            Some(scan)
        }
        ',' | ';' => {
            if next.is_none_or(is_zcc) {
                None
            } else {
                Some(pos + 1)
            }
        }
        '!' => {
            if next.is_none_or(|next| is_zcc(next) || next == '!') {
                None
            } else {
                let mut scan = pos;
                while text[scan..].starts_with('!') {
                    scan += 1;
                }
                Some(scan)
            }
        }
        '?' => {
            if next.is_none_or(|next| is_zcc(next) || next == '?') {
                None
            } else {
                Some(pos + 1)
            }
        }
        // Closing brackets without a matching opener are explicitly excluded
        // by linkify-it's path atom class.
        ')' | ']' | '}' => None,
        _ => Some(pos + ch.len_utf8()),
    }
}

/// `\[(?:(?!ZCc|\]).)*\]` style balanced group.
fn match_group(text: &str, pos: usize, open: char, close: char) -> Option<usize> {
    let mut scan = pos + open.len_utf8();
    while let Some(ch) = text[scan..].chars().next() {
        if ch == close {
            return Some(scan + ch.len_utf8());
        }
        if is_zcc(ch) {
            return None;
        }
        scan += ch.len_utf8();
    }
    None
}

/// `"(?:(?!ZCc|").)+"` style quoted section.
fn match_quoted(text: &str, pos: usize, quote: char) -> Option<usize> {
    let mut scan = pos + quote.len_utf8();
    let mut count = 0;
    while let Some(ch) = text[scan..].chars().next() {
        if ch == quote {
            return if count > 0 {
                Some(scan + ch.len_utf8())
            } else {
                None
            };
        }
        if is_zcc(ch) {
            return None;
        }
        scan += ch.len_utf8();
        count += 1;
    }
    None
}

/// `src_ip4` at `pos`.
fn match_ip4(text: &str, pos: usize) -> Option<usize> {
    let mut scan = pos;
    for part in 0..4 {
        let start = scan;
        while scan < text.len() && text.as_bytes()[scan].is_ascii_digit() {
            scan += 1;
        }
        let digits = &text[start..scan];
        if digits.is_empty() || digits.len() > 3 {
            return None;
        }
        let value: u32 = digits.parse().ok()?;
        if value > 255 || (digits.len() > 1 && digits.starts_with('0')) {
            return None;
        }
        if part < 3 {
            if text.as_bytes().get(scan) != Some(&b'.') {
                return None;
            }
            scan += 1;
        }
    }
    Some(scan)
}

/// Whether a label is one of linkify-it's TLDs (approximation: any two-letter
/// label, the default gTLD list, and `xn--` labels).
fn is_tld(label: &str) -> bool {
    let lower = label.to_ascii_lowercase();
    if lower.len() == 2 && lower.bytes().all(|byte| byte.is_ascii_alphabetic()) {
        return true;
    }
    if lower.starts_with("xn--") && lower.len() > 4 {
        return true;
    }
    matches!(
        lower.as_str(),
        "biz"
            | "com"
            | "edu"
            | "gov"
            | "net"
            | "org"
            | "pro"
            | "web"
            | "xxx"
            | "aero"
            | "asia"
            | "coop"
            | "info"
            | "museum"
            | "name"
            | "shop"
            | "рф"
    )
}

fn starts_with_ci(bytes: &[u8], pos: usize, needle: &str) -> bool {
    let needle = needle.as_bytes();
    bytes.len() >= pos + needle.len() && bytes[pos..pos + needle.len()].eq_ignore_ascii_case(needle)
}

/// linkify-it's `src_ZCC` (whitespace + control).
fn is_zcc(ch: char) -> bool {
    super::util::is_js_space(ch) || ch.is_control()
}

/// Approximation of the Unicode punctuation category (see module docs).
fn is_punct(ch: char) -> bool {
    if ch.is_ascii() {
        return ch.is_ascii_punctuation();
    }
    let code = ch as u32;
    matches!(
        code,
        0x00A1..=0x00BF
            | 0x00D7
            | 0x00F7
            | 0x2000..=0x206F
            | 0x2E00..=0x2E7F
            | 0x3001..=0x303F
            | 0xFE10..=0xFE19
            | 0xFE30..=0xFE4F
            | 0xFF01..=0xFF0F
            | 0xFF1A..=0xFF20
            | 0xFF3B..=0xFF40
            | 0xFF5B..=0xFF65
            | 0xFFE0..=0xFFE6
    )
}

/// linkify-it's `src_ZPCc`.
fn is_zpcc(ch: char) -> bool {
    is_zcc(ch) || is_punct(ch)
}

/// linkify-it's `text_separators` (`[><｜]`).
fn is_text_separator(ch: char) -> bool {
    matches!(ch, '>' | '<' | '\u{FF5C}')
}

/// linkify-it's `src_pseudo_letter` (anything but ZPCc and text separators).
fn is_pseudo_letter(ch: char) -> bool {
    !(is_zpcc(ch) || is_text_separator(ch))
}
