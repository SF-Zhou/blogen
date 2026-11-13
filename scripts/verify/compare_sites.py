#!/usr/bin/env python3
"""Parity gate: compare a blogen-generated site against the reference baseline.

The baseline is produced by `scripts/verify/make_baseline.sh` from the original
legacy blog generator (git archive of HEAD -> ./build.sh) and lives in
`scripts/verify/.cache/baseline/` (public/ + profile/ + config.json + manifest).

This script compares a candidate site directory produced by blogen:

  * per-article page: metadata (title/date/author/link) must be exact; the inner
    HTML of `<div class="markdown">` is classified as
      - identical  : byte-for-byte equal
      - equivalent : equal after the allowed normalisations below
      - different  : hard failure, with a readable diff of the first lines
  * index.json : same article set/fields (ordering differences tolerated -> equivalent)
  * index.html : card title/date/author/link/tags list (same content required,
                 ordering differences -> equivalent)
  * rss.xml   : channel fields, items incl. pubDate/creator/categories
                 (lastBuildDate is ignored)
  * profile/README.md : exact (trailing-whitespace-only diff -> equivalent)
  * file inventory and local asset/link references (broken links are hard failures)

Allowed differences (classified `equivalent`, never let a real content change
slip through):
  * whitespace between block elements, runs of whitespace inside text nodes
  * attribute order / self-closing spelling / boolean attribute spelling
  * KaTeX internals: `<eq>`/`<eqn>` subtrees are compared by their original TeX
    (from `<annotation encoding="application/x-tex">`), so a KaTeX version bump
    is equivalent while changed math is still detected
  * Mermaid figure contents: the baseline's server-rendered mermaid-cli SVG and
    blogen's build-time static SVG (`merman`, inline `<svg>` in light/dark
    containers) are compared as an opaque placeholder; the number and position
    of mermaid blocks must still match
  * index.json / index.html / rss.xml item ordering (blogen sorts
    date-desc,title-asc; the old JS comparator was unstable on date ties)

Exit codes: 0 = pass (identical/equivalent only), 1 = hard mismatch, 2 = usage/
infrastructure error.

Usage:
  scripts/verify/compare_sites.py --candidate <site-dir> [options]
  scripts/verify/compare_sites.py --self-test
  scripts/verify/compare_sites.py --baseline scripts/verify/.cache/baseline \\
      --candidate /tmp/blogen-site/public

Options:
  --baseline DIR          baseline dir from make_baseline.sh (default:
                          scripts/verify/.cache/baseline)
  --candidate DIR         blogen output dir; either the site root itself (contains
                          index.html) or a build dir containing public/index.html
  --candidate-profile F   candidate profile/README.md (default: sibling profile/
                          of the candidate site)
  --json                  print the machine-readable report to stdout instead of
                          the human summary
  --report FILE           write report JSON here (default:
                          scripts/verify/.cache/reports/compare-latest.json;
                          use "" to disable)
  --verbose               list equivalent articles/order notes too
  --max-diffs N           max per-article diffs printed (default 8)
  --max-diff-lines N      max diff lines per mismatch (default 40)
  --self-test             run built-in comparer unit checks (no site needed)
"""

import argparse
import difflib
import hashlib
import html
import json
import os
import re
import sys
import urllib.parse
import xml.etree.ElementTree as ET
from html.parser import HTMLParser
from pathlib import Path

sys.dont_write_bytecode = True

SCHEMA_VERSION = 1
SCRIPT_DIR = Path(__file__).resolve().parent
DEFAULT_BASELINE = SCRIPT_DIR / ".cache" / "baseline"
DEFAULT_REPORT = SCRIPT_DIR / ".cache" / "reports" / "compare-latest.json"

VOID_ELEMENTS = {
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta",
    "param", "source", "track", "wbr",
}
RAW_TEXT_TAGS = {"script", "style"}
IGNORED_INVENTORY_PREFIXES = ("dist/",)
IGNORED_INVENTORY_FILES = (".blogen-manifest.json", ".DS_Store")
MERMAID_CLASSES = {"mermaid", "mermaid-source", "mermaid-light", "mermaid-dark"}


class CompareUsageError(Exception):
    """Bad arguments / missing site layout (exit code 2)."""


# ---------------------------------------------------------------------------
# Tolerant HTML tree
# ---------------------------------------------------------------------------
class Node:
    __slots__ = ("tag", "attrs", "children", "parent")

    def __init__(self, tag, attrs=None, parent=None):
        self.tag = tag
        self.attrs = attrs or []
        self.children = []
        self.parent = parent

    def attr(self, name):
        for k, v in self.attrs:
            if k == name:
                return v
        return None

    @property
    def classes(self):
        return (self.attr("class") or "").split()


class _TreeBuilder(HTMLParser):
    def __init__(self):
        super().__init__(convert_charrefs=True)
        self.root = Node("#document")
        self.stack = [self.root]

    def _append(self, node):
        self.stack[-1].children.append(node)

    def handle_starttag(self, tag, attrs):
        node = Node(tag.lower(), [(k.lower(), v) for k, v in attrs], self.stack[-1])
        self._append(node)
        if tag.lower() not in VOID_ELEMENTS:
            self.stack.append(node)

    def handle_startendtag(self, tag, attrs):
        node = Node(tag.lower(), [(k.lower(), v) for k, v in attrs], self.stack[-1])
        self._append(node)

    def handle_endtag(self, tag):
        tag = tag.lower()
        for i in range(len(self.stack) - 1, 0, -1):
            if self.stack[i].tag == tag:
                del self.stack[i:]
                return
        # Unmatched end tag: ignore (HTML parsers do the same).

    def handle_data(self, data):
        if data:
            self.stack[-1].children.append(data)

    def handle_comment(self, data):  # comments carry no rendered content
        pass

    def handle_decl(self, decl):
        pass

    def handle_pi(self, data):
        pass


def parse_html(text):
    builder = _TreeBuilder()
    builder.feed(text)
    builder.close()
    return builder.root


def iter_elements(node):
    for child in node.children:
        if isinstance(child, Node):
            yield child
            yield from iter_elements(child)


def find_first(node, predicate):
    for el in iter_elements(node):
        if predicate(el):
            return el
    return None


def text_content(node, skip_raw_text=True):
    parts = []
    for child in node.children:
        if isinstance(child, str):
            parts.append(child)
        else:
            if skip_raw_text and child.tag in RAW_TEXT_TAGS:
                continue
            parts.append(text_content(child, skip_raw_text))
    return "".join(parts)


def norm_text(value):
    return re.sub(r"\s+", " ", value or "").strip()


# ---------------------------------------------------------------------------
# Extract the inner HTML of a container element (default <div class="markdown">)
# ---------------------------------------------------------------------------
_CLASS_ATTR_RE = re.compile(
    r"""\bclass\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+))""", re.IGNORECASE
)


def _tag_class_tokens(tag_text):
    m = _CLASS_ATTR_RE.search(tag_text)
    if not m:
        return []
    return (m.group(1) or m.group(2) or m.group(3) or "").split()


def _find_matching_close(text, start_pos, tag):
    pattern = re.compile(r"<(/?)%s\b[^>]*>" % re.escape(tag), re.IGNORECASE)
    depth = 1
    for m in pattern.finditer(text, start_pos):
        if m.group(1):
            depth -= 1
            if depth == 0:
                return m.start()
        else:
            depth += 1
    raise ValueError("unbalanced <%s>: no matching close tag" % tag)


def extract_element_inner(text, tag="div", class_name="markdown"):
    """Return the raw inner HTML of the first `<tag class=...class_name...>`."""
    open_re = re.compile(r"<%s\b[^>]*>" % re.escape(tag), re.IGNORECASE)
    classes = class_name.split()
    for m in open_re.finditer(text):
        tokens = _tag_class_tokens(m.group(0))
        if all(c in tokens for c in classes):
            try:
                end = _find_matching_close(text, m.end(), tag)
            except ValueError:
                return None
            return text[m.end():end]
    return None


def extract_markdown_inner(page_html):
    return extract_element_inner(page_html, "div", "markdown")


# ---------------------------------------------------------------------------
# Canonical form
#
# canonical_tokens() turns an HTML fragment into a list of line strings that can
# be compared and diffed. It normalises attribute order, class token order,
# insignificant whitespace, and optionally collapses math/mermaid subtrees into
# semantically comparable placeholders.
# ---------------------------------------------------------------------------
_STYLE_RE = re.compile(r"\s*([:;])\s*")


def _normalize_attrs(node):
    out = []
    for name, value in node.attrs:
        if value is not None and name == "class":
            value = " ".join(sorted(value.split()))
        elif value is not None and name == "style":
            value = _STYLE_RE.sub(r"\1", value).strip().rstrip(";")
        out.append((name, value))
    out.sort(key=lambda kv: (kv[0], kv[1] or ""))
    return out


def _render_open_tag(tag, attrs):
    parts = ["<", tag]
    for name, value in attrs:
        if value is None:
            parts.append(" %s" % name)
        else:
            parts.append(' %s="%s"' % (name, html.escape(value, quote=True)))
    parts.append(">")
    return "".join(parts)


def _line_escape(value):
    return value.replace("\\", "\\\\").replace("\r", "\\r").replace("\n", "\\n")


def _find_katex_tex(node):
    for el in iter_elements(node):
        if el.tag == "annotation" and (el.attr("encoding") or "") == "application/x-tex":
            tex = norm_text(text_content(el))
            return tex
    return None


def _has_mermaid_class(node):
    return any(c in MERMAID_CLASSES for c in node.classes)


def count_math_blocks(root):
    return sum(1 for el in iter_elements(root) if el.tag in ("eq", "eqn"))


def collect_math_tex(root):
    out = []
    for el in iter_elements(root):
        if el.tag in ("eq", "eqn"):
            tex = _find_katex_tex(el)
            out.append(tex if tex is not None else norm_text(text_content(el)))
    return out


def count_mermaid_blocks(root):
    """Count top-level mermaid containers (do not double count nested divs)."""
    count = 0

    def walk(node):
        nonlocal count
        for child in node.children:
            if not isinstance(child, Node):
                continue
            if _has_mermaid_class(child):
                count += 1
                continue
            walk(child)

    walk(root)
    return count


def canonical_tokens(node, collapse_math=True, collapse_mermaid=True):
    out = []
    _canon(node, out, collapse_math, collapse_mermaid, False)
    return out


def _canon(node, out, collapse_math, collapse_mermaid, preserve_ws):
    if isinstance(node, str):
        if preserve_ws:
            out.append("#raw " + json.dumps(node))
            return
        collapsed = re.sub(r"\s+", " ", node)
        if not collapsed.strip():
            return
        out.append("#text " + json.dumps(collapsed))
        return

    tag = node.tag
    if tag in RAW_TEXT_TAGS:  # no rendered markdown content lives in script/style
        return
    if collapse_mermaid and _has_mermaid_class(node):
        out.append("<#mermaid/>")
        return
    if collapse_math and tag in ("eq", "eqn"):
        tex = _find_katex_tex(node)
        if tex is None:
            tex = norm_text(text_content(node))
        out.append("<%s>" % tag)
        out.append("#tex " + json.dumps(tex))
        out.append("</%s>" % tag)
        return

    attrs = _normalize_attrs(node)
    out.append(_render_open_tag(tag, attrs))
    child_preserve = preserve_ws or tag in ("pre", "code", "textarea")
    for child in node.children:
        _canon(child, out, collapse_math, collapse_mermaid, child_preserve)
    out.append("</%s>" % tag)


def _canon_inner(raw_inner, **kwargs):
    return canonical_tokens(parse_html(raw_inner), **kwargs)


def body_stats(raw_inner):
    root = parse_html(raw_inner)
    return {
        "math_blocks": count_math_blocks(root),
        "mermaid_blocks": count_mermaid_blocks(root),
        "math_tex": collect_math_tex(root),
    }


# ---------------------------------------------------------------------------
# Per-article body comparison
# ---------------------------------------------------------------------------
def classify_body(path, baseline_inner, candidate_inner):
    """Compare extracted markdown-div inner HTML.

    Returns dict with classification 'identical' | 'equivalent' | 'different',
    reasons/notes, stats and a unified diff for hard mismatches.
    """
    result = {
        "path": path,
        "classification": "different",
        "allowed_differences": [],
        "notes": [],
        "diff": [],
        "stats": {},
    }
    if baseline_inner is None:
        result["notes"].append("baseline page has no <div class=\"markdown\">")
        return result
    if candidate_inner is None:
        result["notes"].append("candidate page has no <div class=\"markdown\"> (theme contract)")
        return result

    b_stats = body_stats(baseline_inner)
    c_stats = body_stats(candidate_inner)
    result["stats"] = {"baseline": b_stats, "candidate": c_stats}

    if baseline_inner == candidate_inner:
        result["classification"] = "identical"
        return result

    full = canonical_tokens(parse_html(baseline_inner))
    cand_full = canonical_tokens(parse_html(candidate_inner))
    if full == cand_full:
        result["classification"] = "equivalent"
        # Identify which allowed normalisation was needed (for the report).
        no_math = dict(collapse_math=False, collapse_mermaid=True)
        no_mermaid = dict(collapse_math=True, collapse_mermaid=False)
        neither = dict(collapse_math=False, collapse_mermaid=False)
        b_attrs = _canon_inner(baseline_inner, **neither)
        c_attrs = _canon_inner(candidate_inner, **neither)
        b_math = _canon_inner(baseline_inner, **no_math)
        c_math = _canon_inner(candidate_inner, **no_math)
        b_mermaid = _canon_inner(baseline_inner, **no_mermaid)
        c_mermaid = _canon_inner(candidate_inner, **no_mermaid)
        if b_attrs == c_attrs:
            result["allowed_differences"].append("attribute-order/whitespace")
        elif b_mermaid == c_mermaid:
            result["allowed_differences"].append("math-internals")
        elif b_math == c_math:
            result["allowed_differences"].append("mermaid-contents")
        else:
            result["allowed_differences"].append("math-internals+mermaid-contents")
        if b_stats["mermaid_blocks"] != c_stats["mermaid_blocks"]:
            # Cannot happen when canonical forms are equal, but keep the guard.
            result["notes"].append("mermaid block count differs")
        return result

    # Hard difference.
    limit = 400
    diff = list(
        difflib.unified_diff(
            full[:limit],
            cand_full[:limit],
            fromfile="baseline/" + path,
            tofile="candidate/" + path,
            n=2,
            lineterm="",
        )
    )
    result["diff"] = diff
    if b_stats["math_tex"] != c_stats["math_tex"]:
        added = [t for t in c_stats["math_tex"] if t not in b_stats["math_tex"]]
        removed = [t for t in b_stats["math_tex"] if t not in c_stats["math_tex"]]
        if added:
            result["notes"].append("math added: %r" % (added[:3],))
        if removed:
            result["notes"].append("math removed: %r" % (removed[:3],))
    if b_stats["mermaid_blocks"] != c_stats["mermaid_blocks"]:
        result["notes"].append(
            "mermaid block count %d -> %d"
            % (b_stats["mermaid_blocks"], c_stats["mermaid_blocks"])
        )
    return result


# ---------------------------------------------------------------------------
# Page metadata (title / h1 / date / author / author link)
# ---------------------------------------------------------------------------
def _author_url(node):
    cur = node
    while cur is not None:
        if cur.tag == "a" and cur.attr("href"):
            return cur.attr("href")
        cur = cur.parent
    return None


def _inside(node, ancestor):
    cur = node
    while cur is not None:
        if cur is ancestor:
            return True
        cur = cur.parent
    return False


def page_metadata(page_html):
    root = parse_html(page_html)
    # Page chrome (title/date/author) lives outside the article/index body.
    # Ignore date/author elements that belong to cards inside <div class="markdown">.
    markdown_node = find_first(
        root, lambda n: n.tag == "div" and "markdown" in n.classes)

    def chrome(el):
        return markdown_node is None or not _inside(el, markdown_node)

    title_el = find_first(root, lambda n: n.tag == "title")
    h1_el = find_first(root, lambda n: n.tag == "h1")
    date_el = find_first(
        root, lambda n: "date" in n.classes and chrome(n))
    author_el = find_first(
        root, lambda n: "author" in n.classes and chrome(n))
    return {
        "title": norm_text(text_content(title_el)) if title_el is not None else None,
        "h1": norm_text(text_content(h1_el)) if h1_el is not None else None,
        "date": norm_text(text_content(date_el)) if date_el is not None else None,
        "author": norm_text(text_content(author_el)) if author_el is not None else None,
        "author_url": _author_url(author_el) if author_el is not None else None,
    }


def compare_page_metadata(base_html, cand_html):
    base = page_metadata(base_html)
    cand = page_metadata(cand_html)
    mismatches = []
    for field in ("title", "h1", "date", "author", "author_url"):
        if base.get(field) != cand.get(field):
            mismatches.append({"field": field, "baseline": base.get(field), "candidate": cand.get(field)})
    return mismatches


# ---------------------------------------------------------------------------
# index.html cards
# ---------------------------------------------------------------------------
def _nearest_ancestor_with_class(node, class_name, stop=None):
    cur = node.parent
    while cur is not None and cur is not stop:
        if class_name in cur.classes:
            return cur
        cur = cur.parent
    return None


def _card_titles(card):
    link = find_first(card, lambda n: "article_title" in n.classes)
    if link is None:
        link = find_first(card, lambda n: n.tag == "a" and n.attr("href"))
    title = norm_text(text_content(link)) if link is not None else None
    href = link.attr("href") if link is not None else None
    return title, href


def _card_tags(card):
    tags = []
    tags_box = find_first(card, lambda n: "tags" in n.classes)
    if tags_box is None:
        return tags
    for el in iter_elements(tags_box):
        if "tag" not in el.classes or el is tags_box:
            continue
        link = find_first(el, lambda n: n.tag == "a")
        if link is not None:
            tags.append({"text": norm_text(text_content(link)), "href": link.attr("href")})
        else:
            tags.append({"text": norm_text(text_content(el)), "href": None})
    return tags


def _card_info(card):
    title, href = _card_titles(card)
    date_el = find_first(card, lambda n: "date" in n.classes)
    author_el = find_first(card, lambda n: "author" in n.classes)
    # The author element in the old card template is wrapped in <a href>.
    author_url = None
    if author_el is not None:
        cur = author_el.parent
        while cur is not None and cur is not card:
            if cur.tag == "a" and cur.attr("href"):
                author_url = cur.attr("href")
                break
            cur = cur.parent
    return {
        "title": title,
        "url_path": href,
        "date": norm_text(text_content(date_el)) if date_el is not None else None,
        "author": norm_text(text_content(author_el)) if author_el is not None else None,
        "author_url": author_url,
        "tags": _card_tags(card),
    }


def extract_cards(page_html):
    inner = extract_markdown_inner(page_html)
    if inner is None:
        return None
    root = parse_html(inner)
    cards = []
    for card in iter_elements(root):
        if card.tag == "div" and "card" in card.classes:
            if _nearest_ancestor_with_class(card, "card") is not None:
                continue  # nested decoration inside a card
            cards.append(_card_info(card))
    return cards


def compare_cards(base_cards, cand_cards):
    """Return (classification, notes, diff_lines)."""
    if base_cards is None or cand_cards is None:
        return (
            "different",
            ["missing index card container (<div class=\"markdown\"> with div.card)"],
            [],
        )
    lines_b = [json.dumps(c, sort_keys=True) for c in base_cards]
    lines_c = [json.dumps(c, sort_keys=True) for c in cand_cards]
    if lines_b == lines_c:
        return "identical", [], []
    if sorted(lines_b) == sorted(lines_c):
        return "equivalent", ["card ordering differs (allowed)"], []
    diff = list(
        difflib.unified_diff(
            lines_b, lines_c, fromfile="baseline/index.html cards",
            tofile="candidate/index.html cards", n=2, lineterm=""))
    return "different", [], diff


# ---------------------------------------------------------------------------
# index.json
# ---------------------------------------------------------------------------
INDEX_JSON_FIELDS = ("title", "date", "author", "author_url", "tags", "filename", "url_path")


def compare_index_json(base_data, cand_data):
    """Compare parsed index.json lists. Returns dict."""
    result = {
        "section": "index.json",
        "classification": "different",
        "notes": [],
        "diff": [],
        "counts": {},
    }
    if not isinstance(base_data, list) or not isinstance(cand_data, list):
        result["notes"].append("index.json is not a JSON list")
        return result
    result["counts"] = {"baseline": len(base_data), "candidate": len(cand_data)}
    by_path_b = {}
    by_path_c = {}
    for i, entry in enumerate(base_data):
        if not isinstance(entry, dict):
            result["notes"].append("baseline entry %d is not an object" % i)
            return result
        by_path_b[str(entry.get("url_path"))] = entry
    for i, entry in enumerate(cand_data):
        if not isinstance(entry, dict):
            result["notes"].append("candidate entry %d is not an object" % i)
            return result
        by_path_c[str(entry.get("url_path"))] = entry

    missing = sorted(set(by_path_b) - set(by_path_c))
    extra = sorted(set(by_path_c) - set(by_path_b))
    field_mismatches = []
    for path in sorted(set(by_path_b) & set(by_path_c)):
        b, c = by_path_b[path], by_path_c[path]
        for field in INDEX_JSON_FIELDS:
            if b.get(field) != c.get(field):
                field_mismatches.append(
                    {"url_path": path, "field": field,
                     "baseline": b.get(field), "candidate": c.get(field)})
        b_extra = sorted(set(b) - set(INDEX_JSON_FIELDS))
        c_extra = sorted(set(c) - set(INDEX_JSON_FIELDS))
        if b_extra != c_extra:
            field_mismatches.append(
                {"url_path": path, "field": "<extra keys>",
                 "baseline": b_extra, "candidate": c_extra})

    result["missing"] = missing
    result["extra"] = extra
    result["field_mismatches"] = field_mismatches

    if missing or extra or field_mismatches:
        lines_b = [
            json.dumps({f: e.get(f) for f in INDEX_JSON_FIELDS}, sort_keys=True)
            for e in base_data
        ]
        lines_c = [
            json.dumps({f: e.get(f) for f in INDEX_JSON_FIELDS}, sort_keys=True)
            for e in cand_data
        ]
        result["diff"] = list(
            difflib.unified_diff(
                lines_b, lines_c, fromfile="baseline/index.json",
                tofile="candidate/index.json", n=1, lineterm=""))
        if missing:
            result["notes"].append("missing articles: %s" % ", ".join(missing[:8]))
        if extra:
            result["notes"].append("unexpected articles: %s" % ", ".join(extra[:8]))
        if field_mismatches:
            result["notes"].append("%d field mismatch(es)" % len(field_mismatches))
        return result

    # Same articles and fields; check ordering.
    base_order = [str(e.get("url_path")) for e in base_data]
    cand_order = [str(e.get("url_path")) for e in cand_data]
    if base_order == cand_order:
        result["classification"] = "identical"
    else:
        result["classification"] = "equivalent"
        result["notes"].append("article ordering differs (allowed)")
    return result


# ---------------------------------------------------------------------------
# rss.xml
# ---------------------------------------------------------------------------
DC_NS = "{http://purl.org/dc/elements/1.1/}"
ATOM_NS = "{http://www.w3.org/2005/Atom}"


def parse_rss_text(text):
    root = ET.fromstring(text)
    channel = root.find("channel")
    if channel is None:
        raise ValueError("rss.xml has no <channel>")

    def txt(el, path):
        found = el.find(path)
        return None if found is None else (found.text or "").strip()

    def attr(el, path, name):
        found = el.find(path)
        return None if found is None else found.attrib.get(name)

    data = {
        "root_tag": root.tag,
        "channel": {
            "title": txt(channel, "title"),
            "link": txt(channel, "link"),
            "description": txt(channel, "description"),
            "language": txt(channel, "language"),
            "self_link": {
                "href": attr(channel, ATOM_NS + "link", "href"),
                "rel": attr(channel, ATOM_NS + "link", "rel"),
                "type": attr(channel, ATOM_NS + "link", "type"),
            },
        },
        "items": [],
    }
    for item in channel.findall("item"):
        guid_el = item.find("guid")
        data["items"].append({
            "title": txt(item, "title"),
            "link": txt(item, "link"),
            "guid": None if guid_el is None else (guid_el.text or "").strip(),
            "guid_isPermaLink": None if guid_el is None else guid_el.attrib.get("isPermaLink"),
            "pubDate": txt(item, "pubDate"),
            "creator": txt(item, DC_NS + "creator"),
            "categories": [norm_text(c.text or "") for c in item.findall("category")],
        })
    return data


def compare_rss_text(base_text, cand_text):
    result = {
        "section": "rss.xml",
        "classification": "different",
        "notes": [],
        "diff": [],
        "counts": {},
    }
    try:
        base = parse_rss_text(base_text)
        cand = parse_rss_text(cand_text)
    except Exception as exc:  # noqa: BLE001 - report any parse failure as a gate failure
        result["notes"].append("rss parse error: %s" % exc)
        return result

    if base["root_tag"] != cand["root_tag"]:
        result["notes"].append("root element %r != %r" % (base["root_tag"], cand["root_tag"]))
        return result

    channel_mismatches = []
    for field in ("title", "link", "description", "language"):
        if base["channel"][field] != cand["channel"][field]:
            channel_mismatches.append(
                {"field": field, "baseline": base["channel"][field],
                 "candidate": cand["channel"][field]})
    if base["channel"]["self_link"] != cand["channel"]["self_link"]:
        channel_mismatches.append(
            {"field": "atom:link", "baseline": base["channel"]["self_link"],
             "candidate": cand["channel"]["self_link"]})
    result["channel_mismatches"] = channel_mismatches
    result["counts"] = {"baseline": len(base["items"]), "candidate": len(cand["items"])}
    result["notes"].append("lastBuildDate ignored")

    items_b = base["items"]
    items_c = cand["items"]
    lines_b = [json.dumps(i, sort_keys=True) for i in items_b]
    lines_c = [json.dumps(i, sort_keys=True) for i in items_c]
    if channel_mismatches:
        # Channel metadata (title/link/description/language/atom:link) must be exact.
        result["notes"].append("%d channel field mismatch(es)" % len(channel_mismatches))
        if items_b != items_c:
            result["diff"] = list(
                difflib.unified_diff(
                    lines_b, lines_c, fromfile="baseline/rss.xml items",
                    tofile="candidate/rss.xml items", n=1, lineterm=""))
        return result
    if items_b != items_c:
        result["diff"] = list(
            difflib.unified_diff(
                lines_b, lines_c, fromfile="baseline/rss.xml items",
                tofile="candidate/rss.xml items", n=1, lineterm=""))
        if sorted(lines_b) == sorted(lines_c):
            result["classification"] = "equivalent"
            result["notes"].append("item ordering differs (allowed)")
        else:
            result["notes"].append("item content differs")
        return result

    result["classification"] = "identical"
    return result


# ---------------------------------------------------------------------------
# Profile README
# ---------------------------------------------------------------------------
def compare_profile(base_text, cand_text):
    result = {"section": "profile/README.md", "classification": "different",
              "notes": [], "diff": []}
    if base_text is None:
        result["notes"].append("baseline profile missing")
        return result
    if cand_text is None:
        result["notes"].append("candidate profile missing")
        return result
    if base_text == cand_text:
        result["classification"] = "identical"
        return result
    if base_text.rstrip() == cand_text.rstrip():
        result["classification"] = "equivalent"
        result["notes"].append("trailing whitespace differs")
        return result
    result["diff"] = list(
        difflib.unified_diff(
            base_text.splitlines(), cand_text.splitlines(),
            fromfile="baseline/profile/README.md",
            tofile="candidate/profile/README.md", n=2, lineterm=""))
    return result


# ---------------------------------------------------------------------------
# Inventory and local links
# ---------------------------------------------------------------------------
def walk_files(root):
    root = Path(root)
    files = set()
    for dirpath, dirnames, filenames in os.walk(root):
        dirnames[:] = sorted(d for d in dirnames if d not in (".git", "node_modules"))
        for name in filenames:
            full = Path(dirpath) / name
            if full.is_symlink() or not full.is_file():
                continue
            files.add(full.relative_to(root).as_posix())
    return files


def sha256_file(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def is_article_page(rel):
    if rel == "index.html":
        return False
    if not (rel.endswith(".html") or rel.endswith(".htm")):
        return False
    if any(rel.startswith(p) for p in IGNORED_INVENTORY_PREFIXES):
        return False
    if rel.startswith("images/"):
        return False
    return True


def _is_ignored_inventory(rel):
    if any(rel.startswith(p) for p in IGNORED_INVENTORY_PREFIXES):
        return True
    if rel in IGNORED_INVENTORY_FILES:
        return True
    return False


_REF_ATTR_RE = re.compile(r"""\b(href|src)\s*=\s*(?:"([^"]*)"|'([^']*)')""", re.IGNORECASE)


def collect_local_refs(href, page_rel):
    """Yield (raw_value, target_relpath_or_None) for local asset references."""
    out = []
    for m in _REF_ATTR_RE.finditer(href):
        value = (m.group(2) if m.group(2) is not None else m.group(3) or "").strip()
        if not value:
            continue
        lowered = value.lower()
        if lowered.startswith(("http:", "https:", "//", "mailto:", "tel:", "data:", "javascript:")):
            continue
        if value.startswith("#"):
            continue
        raw = value
        value = value.split("#", 1)[0].split("?", 1)[0]
        if not value:
            continue
        value = urllib.parse.unquote(value)
        if "://" in value:
            continue  # percent-encoded absolute URL: external, not a site asset
        if value.startswith("/"):
            target = value.lstrip("/")
        else:
            target = os.path.normpath(os.path.join(os.path.dirname(page_rel), value))
            target = target.replace(os.sep, "/")
            if target.startswith("../"):
                continue  # outside the site root: not our concern
        out.append((raw, target))
    return out


def check_candidate_links(site_root, page_htmls):
    """Return (broken_refs, checked_count, page_count)."""
    root = Path(site_root)
    broken = []
    checked = 0
    for rel, text in page_htmls:
        for raw, target in collect_local_refs(text, rel):
            checked += 1
            candidate_path = root / target
            if candidate_path.is_file() or candidate_path.is_dir():
                continue
            if (root / target / "index.html").is_file():
                continue
            broken.append({"page": rel, "ref": raw, "target": target})
    return broken, checked, len(page_htmls)


# ---------------------------------------------------------------------------
# Top-level comparison
# ---------------------------------------------------------------------------
def resolve_baseline(path):
    base = Path(path).resolve()
    public = base / "public"
    if not public.is_dir():
        raise CompareUsageError(
            "baseline %s has no public/ directory (run make_baseline.sh)" % base)
    profile = base / "profile" / "README.md"
    manifest = base / "manifest.json"
    manifest_data = None
    if manifest.is_file():
        with open(manifest) as f:
            manifest_data = json.load(f)
    return {"dir": str(base), "public": public, "profile": profile,
            "manifest": manifest_data}


def resolve_candidate(path, profile_arg=None):
    cand = Path(path).resolve()
    if (cand / "index.html").is_file():
        site_root = cand
        build_dir = cand.parent
    elif (cand / "public" / "index.html").is_file():
        site_root = cand / "public"
        build_dir = cand
    else:
        raise CompareUsageError(
            "candidate %s is neither a site root (index.html) nor a build dir "
            "containing public/index.html" % cand)
    if profile_arg:
        profile = Path(profile_arg).resolve()
    else:
        profile = None
        for candidate_profile in (site_root.parent / "profile" / "README.md",
                                  build_dir / "profile" / "README.md",
                                  site_root / "profile" / "README.md"):
            if candidate_profile.is_file():
                profile = candidate_profile
                break
    return {"dir": str(cand), "site_root": site_root, "build_dir": build_dir,
            "profile": profile}


def read_text(path):
    with open(path, "r", encoding="utf-8", errors="replace") as f:
        return f.read()


def compare_sites(args):
    baseline = resolve_baseline(args.baseline)
    candidate = resolve_candidate(args.candidate, args.candidate_profile)
    base_root = baseline["public"]
    cand_root = candidate["site_root"]

    report = {
        "schema_version": SCHEMA_VERSION,
        "baseline": {
            "dir": baseline["dir"],
            "public": str(base_root),
            "manifest": baseline["manifest"],
        },
        "candidate": {
            "dir": candidate["dir"],
            "site_root": str(cand_root),
            "profile": str(candidate["profile"]) if candidate["profile"] else None,
        },
        "sections": {},
        "hard_failures": [],
        "warnings": [],
        "summary": {},
    }

    base_files = walk_files(base_root)
    cand_files = walk_files(cand_root)

    # -- article inventory --------------------------------------------------
    base_articles = {rel for rel in base_files if is_article_page(rel)}
    cand_articles = {rel for rel in cand_files if is_article_page(rel)}
    missing_articles = sorted(base_articles - cand_articles)
    extra_articles = sorted(cand_articles - base_articles)

    # -- per-article comparison --------------------------------------------
    article_results = []
    for rel in sorted(base_articles & cand_articles):
        base_html = read_text(base_root / rel)
        cand_html = read_text(cand_root / rel)
        body = classify_body(
            rel,
            extract_markdown_inner(base_html),
            extract_markdown_inner(cand_html),
        )
        body["metadata_mismatches"] = compare_page_metadata(base_html, cand_html)
        article_results.append(body)

    counts = {"total": len(article_results), "identical": 0, "equivalent": 0, "different": 0}
    for res in article_results:
        counts[res["classification"]] += 1
    for res in article_results:
        if res["classification"] == "different" or res["metadata_mismatches"]:
            label = res["path"]
            if res["metadata_mismatches"]:
                labels = ", ".join(m["field"] for m in res["metadata_mismatches"])
                report["hard_failures"].append("article %s: metadata differs (%s)" % (label, labels))
            if res["classification"] == "different":
                report["hard_failures"].append("article %s: body HTML differs" % label)
    for rel in missing_articles:
        report["hard_failures"].append("missing article page: %s" % rel)
    for rel in extra_articles:
        report["hard_failures"].append("unexpected article page: %s" % rel)

    report["sections"]["articles"] = {
        "counts": counts,
        "missing": missing_articles,
        "extra": extra_articles,
        "results": article_results,
    }

    # -- index.json ---------------------------------------------------------
    base_index_json = base_root / "index.json"
    cand_index_json = cand_root / "index.json"
    if not cand_index_json.is_file():
        report["hard_failures"].append("missing candidate index.json")
        report["sections"]["index_json"] = {
            "section": "index.json", "classification": "missing", "notes": [], "diff": []}
    else:
        try:
            base_data = json.loads(read_text(base_index_json))
            cand_data = json.loads(read_text(cand_index_json))
        except json.JSONDecodeError as exc:
            report["hard_failures"].append("index.json is not valid JSON: %s" % exc)
            base_data, cand_data = None, None
        index_result = compare_index_json(base_data, cand_data) if base_data is not None else {
            "section": "index.json", "classification": "different", "notes": ["parse failure"], "diff": []}
        report["sections"]["index_json"] = index_result
        if index_result["classification"] == "different":
            report["hard_failures"].append("index.json content differs")
        elif index_result["classification"] == "equivalent":
            report["warnings"].append("index.json: " + "; ".join(index_result["notes"]))

    # -- index.html cards ---------------------------------------------------
    base_index_html = base_root / "index.html"
    cand_index_html = cand_root / "index.html"
    if not cand_index_html.is_file():
        report["hard_failures"].append("missing candidate index.html")
        report["sections"]["index_html"] = {
            "section": "index.html", "classification": "missing", "notes": [], "diff": []}
    else:
        base_text = read_text(base_index_html)
        cand_text = read_text(cand_index_html)
        cards_cls, cards_notes, cards_diff = compare_cards(
            extract_cards(base_text), extract_cards(cand_text))
        meta_mismatches = compare_page_metadata(base_text, cand_text)
        section = {
            "section": "index.html",
            "classification": cards_cls,
            "notes": cards_notes,
            "diff": cards_diff,
            "metadata_mismatches": meta_mismatches,
        }
        report["sections"]["index_html"] = section
        if cards_cls == "different":
            report["hard_failures"].append("index.html card list differs")
        elif cards_cls == "equivalent":
            report["warnings"].append("index.html: card ordering differs (allowed)")
        if meta_mismatches:
            report["hard_failures"].append("index.html: page metadata differs")

    # -- rss.xml ------------------------------------------------------------
    base_rss = base_root / "rss.xml"
    cand_rss = cand_root / "rss.xml"
    if not cand_rss.is_file():
        report["hard_failures"].append("missing candidate rss.xml")
        report["sections"]["rss"] = {
            "section": "rss.xml", "classification": "missing", "notes": [], "diff": []}
    else:
        rss_result = compare_rss_text(read_text(base_rss), read_text(cand_rss))
        report["sections"]["rss"] = rss_result
        if rss_result["classification"] == "different":
            report["hard_failures"].append("rss.xml content differs")
        elif rss_result["classification"] == "equivalent":
            report["warnings"].append("rss.xml: item ordering differs (allowed)")
        else:
            report["warnings"].append("rss.xml: lastBuildDate ignored")

    # -- profile/README.md --------------------------------------------------
    base_profile = baseline["profile"]
    cand_profile = candidate["profile"]
    base_profile_text = read_text(base_profile) if base_profile.is_file() else None
    cand_profile_text = read_text(cand_profile) if cand_profile and cand_profile.is_file() else None
    profile_result = compare_profile(base_profile_text, cand_profile_text)
    report["sections"]["profile"] = profile_result
    if profile_result["classification"] == "different":
        report["hard_failures"].append("profile/README.md differs")
    elif profile_result["classification"] == "equivalent":
        report["warnings"].append("profile/README.md: trailing whitespace differs (allowed)")

    # -- inventory ----------------------------------------------------------
    base_inventory = {rel for rel in base_files if not _is_ignored_inventory(rel)}
    cand_inventory = {rel for rel in cand_files if not _is_ignored_inventory(rel)}
    missing_other = sorted(base_inventory - cand_inventory - base_articles)
    extra_other = sorted(cand_inventory - base_inventory - cand_articles)
    common = sorted(base_inventory & cand_inventory)
    image_mismatches = []
    for rel in common:
        if not rel.startswith("images/"):
            continue
        if sha256_file(base_root / rel) != sha256_file(cand_root / rel):
            image_mismatches.append(rel)
    for rel in image_mismatches:
        report["hard_failures"].append("image content differs: %s" % rel)
    if missing_other:
        report["warnings"].append(
            "baseline files not generated by candidate: %s%s"
            % (", ".join(missing_other[:8]), " ..." if len(missing_other) > 8 else ""))
    if extra_other:
        report["warnings"].append(
            "candidate extra files (informational): %s%s"
            % (", ".join(extra_other[:8]), " ..." if len(extra_other) > 8 else ""))
    report["sections"]["inventory"] = {
        "baseline_files": len(base_files),
        "candidate_files": len(cand_files),
        "article_pages": {"baseline": len(base_articles), "candidate": len(cand_articles)},
        "missing_articles": missing_articles,
        "extra_articles": extra_articles,
        "missing_non_article": missing_other,
        "extra_non_article": extra_other,
        "image_content_mismatches": image_mismatches,
    }

    # -- local asset references (broken links are hard failures) -----------
    page_htmls = []
    for rel in sorted(cand_articles):
        page_htmls.append((rel, read_text(cand_root / rel)))
    if cand_index_html.is_file():
        page_htmls.append(("index.html", read_text(cand_index_html)))
    broken, checked, page_count = check_candidate_links(cand_root, page_htmls)
    for item in broken[:20]:
        report["hard_failures"].append(
            "broken local reference in %s: %s" % (item["page"], item["ref"]))
    if len(broken) > 20:
        report["hard_failures"].append("... and %d more broken local references" % (len(broken) - 20))
    report["sections"]["links"] = {
        "checked_references": checked,
        "pages": page_count,
        "broken": broken,
    }

    # -- summary ------------------------------------------------------------
    articles_ok = counts["different"] == 0 and not missing_articles and not extra_articles
    sections_ok = all(
        report["sections"].get(name, {}).get("classification") in ("identical", "equivalent")
        for name in ("index_json", "index_html", "rss", "profile")
    )
    report["summary"] = {
        "result": "PASS" if not report["hard_failures"] else "FAIL",
        "articles": counts,
        "articles_ok": articles_ok,
        "sections_ok": sections_ok,
        "hard_failure_count": len(report["hard_failures"]),
    }
    return report


# ---------------------------------------------------------------------------
# Report rendering
# ---------------------------------------------------------------------------
def _short_dir(path):
    try:
        return str(Path(path))
    except Exception:  # noqa: BLE001
        return str(path)


def print_human_report(report, args):
    print("blogen parity compare")
    print("  baseline : %s" % _short_dir(report["baseline"]["public"]))
    manifest = report["baseline"].get("manifest") or {}
    if manifest:
        src = manifest.get("source", {})
        gen = manifest.get("generator", {})
        print("             reference baseline: commit %s, %s, runtime %s"
              % (str(src.get("commit", "?"))[:12], gen.get("command", "?"),
                 gen.get("node", "?")))
    print("  candidate: %s" % _short_dir(report["candidate"]["site_root"]))
    print()

    sections = report["sections"]
    articles = sections.get("articles", {})
    counts = articles.get("counts", {})
    mark = "OK " if (counts.get("different", 1) == 0
                     and not articles.get("missing") and not articles.get("extra")) else "FAIL"
    print("[%s] articles: %d compared -> %d identical, %d equivalent, %d different"
          % (mark, counts.get("total", 0), counts.get("identical", 0),
             counts.get("equivalent", 0), counts.get("different", 0)))
    for rel in articles.get("missing", [])[:10]:
        print("       - missing: %s" % rel)
    for rel in articles.get("extra", [])[:10]:
        print("       - extra:   %s" % rel)
    shown_diffs = 0
    for res in articles.get("results", []):
        if res["classification"] == "different" or res["metadata_mismatches"]:
            shown_diffs += 1
            if shown_diffs > args.max_diffs:
                continue
            print("       - DIFFERENT %s" % res["path"])
            for mismatch in res["metadata_mismatches"]:
                print("           metadata %s: baseline=%r candidate=%r"
                      % (mismatch["field"], mismatch["baseline"], mismatch["candidate"]))
            for note in res.get("notes", []):
                print("           note: %s" % note)
            diff = res.get("diff", [])
            for line in diff[: args.max_diff_lines]:
                print("           " + line)
            if len(diff) > args.max_diff_lines:
                print("           ... (%d more diff lines)" % (len(diff) - args.max_diff_lines))
    if shown_diffs > args.max_diffs:
        print("       ... (%d more mismatching articles not shown)" % (shown_diffs - args.max_diffs))
    if args.verbose:
        for res in articles.get("results", []):
            if res["classification"] == "equivalent":
                print("       ~ equivalent %s (%s)"
                      % (res["path"], ", ".join(res.get("allowed_differences") or ["normalized"])))

    def section_line(name, label, detail=""):
        sec = sections.get(name)
        if not sec:
            return
        cls = sec.get("classification", "different")
        mark = {"identical": "OK ", "equivalent": "OK~", "missing": "FAIL"}.get(cls, "FAIL")
        suffix = detail
        notes = sec.get("notes") or []
        if notes:
            note_text = "; ".join(notes[:3])
            suffix = (suffix + "; " if suffix else "") + note_text
        print("[%s] %s%s" % (mark, label, (": " + suffix) if suffix else ""))
        if cls == "different":
            for mismatch in (sec.get("channel_mismatches") or sec.get("metadata_mismatches") or [])[:6]:
                print("       field %s: baseline=%r candidate=%r"
                      % (mismatch.get("field"), mismatch.get("baseline"),
                         mismatch.get("candidate")))
            diff = sec.get("diff") or []
            for line in diff[: args.max_diff_lines]:
                print("       " + line)
            if len(diff) > args.max_diff_lines:
                print("       ... (%d more diff lines)" % (len(diff) - args.max_diff_lines))

    section_line("index_json", "index.json",
                 "%s articles" % (sections.get("index_json", {}).get("counts", {}).get("candidate", "?")))
    section_line("index_html", "index.html cards")
    section_line("rss", "rss.xml")
    section_line("profile", "profile/README.md")

    inv = sections.get("inventory", {})
    inv_bad = bool(inv.get("missing_articles") or inv.get("extra_articles")
                   or inv.get("image_content_mismatches"))
    print("[%s] inventory: baseline %d files, candidate %d files"
          % ("FAIL" if inv_bad else "OK ", inv.get("baseline_files", 0),
             inv.get("candidate_files", 0)))
    if inv.get("missing_non_article"):
        print("       info: not regenerated (%d): %s"
              % (len(inv["missing_non_article"]), ", ".join(inv["missing_non_article"][:6])))
    if inv.get("extra_non_article"):
        print("       info: candidate-only files (%d): %s"
              % (len(inv["extra_non_article"]), ", ".join(inv["extra_non_article"][:6])))
    for rel in inv.get("image_content_mismatches", [])[:6]:
        print("       image content differs: %s" % rel)

    links = sections.get("links", {})
    print("[%s] links: checked %d local reference(s) in %d page(s), %d broken"
          % ("FAIL" if links.get("broken") else "OK ", links.get("checked_references", 0),
             links.get("pages", 0), len(links.get("broken", []))))
    for item in (links.get("broken") or [])[:6]:
        print("       broken: %s -> %s (from %s)"
              % (item["ref"], item["target"], item["page"]))

    if report["warnings"]:
        print("warnings:")
        for warning in report["warnings"][:20]:
            print("  - %s" % warning)
    print()
    summary = report["summary"]
    print("RESULT: %s (%d hard mismatch(es))"
          % (summary.get("result"), summary.get("hard_failure_count", 0)))
    if summary.get("result") != "PASS":
        for failure in report["hard_failures"][:30]:
            print("  ! %s" % failure)


# ---------------------------------------------------------------------------
# Self-test
# ---------------------------------------------------------------------------
SIMPLE_KATEX_B = (
    '<eq><span class="katex"><span class="katex-mathml"><math>'
    '<annotation encoding="application/x-tex">E = mc^2</annotation></math></span>'
    '<span class="katex-html"><span class="mord">E</span><span class="mrel">=</span>'
    '<span class="mord">m</span><span class="mord">c</span><span class="mord">2</span>'
    '</span></span></eq>'
)
SIMPLE_KATEX_C = (
    '<eq><span class="katex"><span class="katex-mathml"><math>'
    '<annotation encoding="application/x-tex">E = mc^2</annotation></math></span>'
    '<span class="katex-html" aria-hidden="true"><span class="katex-base">'
    '<span class="mord mathnormal">E</span><span class="mrel">=</span>'
    '<span class="mord mathnormal">m</span><span class="mord mathnormal">c</span>'
    '<span class="mord">2</span></span></span></span></eq>'
)
MERMAID_B = (
    '<figure class="mermaid"><div class="mermaid-light">'
    '<svg id="mermaid-abc-0-light"><g class="nodes"/></svg></div>'
    '<div class="mermaid-dark"><svg id="mermaid-abc-0-dark"/></div></figure>'
)
MERMAID_C = (
    '<figure class="mermaid"><pre class="mermaid-source">graph TD;\n  A--&gt;B;</pre></figure>'
)


def run_self_test():
    failures = []
    checks = 0

    def check(name, condition, detail=""):
        nonlocal checks
        checks += 1
        if not condition:
            failures.append("%s%s" % (name, (": " + detail) if detail else ""))

    # Extraction.
    check("extract inner",
          extract_markdown_inner('<div class="markdown"><p>x</p></div>') == "<p>x</p>")
    check("extract class with other tokens",
          extract_markdown_inner('<div id="a" class="f markdown g"><p>x</p></div>') == "<p>x</p>")
    check("extract nested divs",
          extract_markdown_inner('<div class="markdown"><div><p>x</p></div></div>') == "<div><p>x</p></div>")
    check("extract missing", extract_markdown_inner("<p>no container</p>") is None)

    # identical.
    res = classify_body("t", "<p>Hello</p>", "<p>Hello</p>")
    check("identical", res["classification"] == "identical", res["classification"])
    # whitespace between blocks + attribute order -> equivalent.
    res = classify_body(
        "t",
        '<div class="a b">\n  <p>Hello</p>\n  <img src="x.png" alt="">\n</div>',
        '<div class="b a"><p>Hello</p><img alt="" src="x.png"></div>',
    )
    check("attr/whitespace equivalent", res["classification"] == "equivalent",
          "%s %r" % (res["classification"], res["allowed_differences"]))
    # whitespace runs inside text nodes -> equivalent.
    res = classify_body("t", "<p>a  b</p>", "<p>a b</p>")
    check("text whitespace equivalent", res["classification"] == "equivalent", res["classification"])
    # KaTeX internals -> equivalent.
    res = classify_body("t", "<p>%s</p>" % SIMPLE_KATEX_B, "<p>%s</p>" % SIMPLE_KATEX_C)
    check("katex equivalent", res["classification"] == "equivalent",
          "%s %r" % (res["classification"], res["allowed_differences"]))
    # Mermaid server vs client -> equivalent.
    res = classify_body("t", "<p>%s</p>" % MERMAID_B, "<p>%s</p>" % MERMAID_C)
    check("mermaid equivalent", res["classification"] == "equivalent",
          "%s %r" % (res["classification"], res["allowed_differences"]))
    # Text change -> different.
    res = classify_body("t", "<p>Hello</p>", "<p>Hello!</p>")
    check("text change different", res["classification"] == "different")
    # Heading id change -> different.
    res = classify_body("t", '<h2 id="a"><a href="#a">A</a></h2>',
                        '<h2 id="b"><a href="#b">A</a></h2>')
    check("heading id different", res["classification"] == "different")
    # Math content change -> different (KaTeX structure alone is allowed).
    katex_other = SIMPLE_KATEX_C.replace("E = mc^2", "E = mc^3")
    res = classify_body("t", "<p>%s</p>" % SIMPLE_KATEX_B, "<p>%s</p>" % katex_other)
    check("math change different", res["classification"] == "different")
    # Missing block -> different.
    res = classify_body("t", "<p>a</p><p>b</p>", "<p>a</p>")
    check("missing block different", res["classification"] == "different")
    # Mermaid count change -> different.
    res = classify_body("t", "<p>%s</p>" % MERMAID_B, "<p></p>")
    check("missing mermaid different", res["classification"] == "different")
    # Elements preserved inside code (whitespace significant).
    res = classify_body("t", '<pre class="language-x"><code>a\n  b</code></pre>',
                        '<pre class="language-x"><code>a b</code></pre>')
    check("code whitespace significant", res["classification"] == "different")

    # Page metadata.
    page_b = ('<html><head><title>T | S</title></head><body><h1> T </h1>'
              '<div class="date">2024.01.02</div><a href="https://github.com/x">'
              '<div class="author"> x </div></a></body></html>')
    page_c = ('<html><head><title>T | S</title></head><body><h1>T</h1>'
              '<p class="date">2024.01.02</p><a href="https://github.com/x">'
              '<span class="author">x</span></a></body></html>')
    check("metadata tolerant", compare_page_metadata(page_b, page_c) == [])
    page_c_bad = page_c.replace("2024.01.02", "2024.01.03")
    check("metadata date mismatch",
          [m["field"] for m in compare_page_metadata(page_b, page_c_bad)] == ["date"])

    # index.json.
    entry = {"title": "A", "date": "2024.01.01", "author": "x", "author_url": "",
             "tags": ["Rust"], "filename": "a", "url_path": "/a/a.html"}
    entry2 = dict(entry, title="B", url_path="/b/b.html", filename="b")
    res = compare_index_json([entry, entry2], [entry2, entry])
    check("index.json order equivalent", res["classification"] == "equivalent",
          res["classification"])
    res = compare_index_json([entry], [dict(entry, title="Changed")])
    check("index.json field different", res["classification"] == "different")
    res = compare_index_json([entry, entry2], [entry])
    check("index.json missing article", "missing" in res["notes"][0], str(res["notes"]))

    # rss.
    rss_t = ("<rss><channel><title>S</title><link>U</link><description>D</description>"
             "<language>en</language><lastBuildDate>X</lastBuildDate>"
             "<atom:link href='U/rss.xml' rel='self' type='application/rss+xml' xmlns:atom='http://www.w3.org/2005/Atom'/>"
             "<item><title>A</title><link>U/a</link><guid isPermaLink='true'>U/a</guid>"
             "<pubDate>Mon, 01 Jan 2024 00:00:00 GMT</pubDate>"
             "<dc:creator xmlns:dc='http://purl.org/dc/elements/1.1/'>x</dc:creator>"
             "<category>Rust</category></item></channel></rss>")
    rss_t2 = rss_t.replace("X", "Y")
    res = compare_rss_text(rss_t, rss_t2)
    check("rss lastBuildDate ignored", res["classification"] == "identical",
          res["classification"])
    res = compare_rss_text(rss_t, rss_t.replace("Mon, 01 Jan", "Tue, 02 Jan"))
    check("rss pubDate different", res["classification"] == "different")
    item_b = ("<item><title>B</title><link>U/b</link>"
              "<guid isPermaLink='true'>U/b</guid>"
              "<pubDate>Tue, 02 Jan 2024 00:00:00 GMT</pubDate></item>")
    two_items = rss_t.replace("</channel>", item_b + "</channel>")
    res = compare_rss_text(rss_t, two_items)
    check("rss item added different", res["classification"] == "different")
    item_a = re.search(r"<item>.*?</item>", rss_t, re.S).group(0)
    two_items_reversed = two_items.replace(item_a + item_b, item_b + item_a)
    res = compare_rss_text(two_items, two_items_reversed)
    check("rss order-only equivalent", res["classification"] == "equivalent",
          res["classification"])
    # A channel metadata change is a hard failure even if only item order differs.
    channel_changed = two_items_reversed.replace(">D<", ">D2<")
    res = compare_rss_text(two_items, channel_changed)
    check("rss channel mismatch different", res["classification"] == "different",
          res["classification"])

    # profile.
    check("profile identical", compare_profile("a\nb\n", "a\nb\n")["classification"] == "identical")
    check("profile trailing ws equivalent",
          compare_profile("a\nb\n", "a\nb  \n")["classification"] == "equivalent")
    check("profile different",
          compare_profile("a\nb\n", "a\nc\n")["classification"] == "different")

    # cards.
    card_html = ('<div class="markdown"><div class="card"><div class="abstract">'
                 '<a href="/a/a.html" class="article_title"><p>A</p></a>'
                 '<div class="article_info"><div class="article_base">'
                 '<p class="date">2024.01.01</p><a href="https://github.com/x">'
                 '<p class="author"> x </p></a></div><div class="tags">'
                 '<p class="tag"><a href="/#/Rust">Rust</a></p></div></div></div></div></div>')
    cards = extract_cards(card_html)
    check("cards extracted", len(cards) == 1 and cards[0]["url_path"] == "/a/a.html",
          repr(cards))
    check("cards identical", compare_cards(cards, extract_cards(card_html))[0] == "identical")
    other = card_html.replace("<p>A</p>", "<p>A2</p>")
    check("cards content different", compare_cards(cards, extract_cards(other))[0] == "different")

    if failures:
        print("self-test: FAIL (%d/%d checks failed)" % (len(failures), checks))
        for failure in failures:
            print("  ! " + failure)
        return 1
    print("self-test: PASS (%d checks)" % checks)
    return 0


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------
def build_parser():
    parser = argparse.ArgumentParser(
        description="Compare a blogen site against the reference baseline.",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog=__doc__.split("Usage:")[0].strip(),
    )
    parser.add_argument("--baseline", default=str(DEFAULT_BASELINE),
                        help="baseline directory from make_baseline.sh")
    parser.add_argument("--candidate", help="blogen output directory")
    parser.add_argument("--candidate-profile", help="candidate profile/README.md")
    parser.add_argument("--json", action="store_true", help="print JSON report to stdout")
    parser.add_argument("--report", default=str(DEFAULT_REPORT),
                        help='write JSON report here ("" disables; default: %(default)s)')
    parser.add_argument("--verbose", action="store_true", help="list equivalent articles too")
    parser.add_argument("--max-diffs", type=int, default=8,
                        help="max mismatching articles with diffs printed (default: %(default)s)")
    parser.add_argument("--max-diff-lines", type=int, default=40,
                        help="max diff lines per mismatch (default: %(default)s)")
    parser.add_argument("--self-test", action="store_true",
                        help="run built-in comparer unit checks and exit")
    return parser


def main(argv=None):
    args = build_parser().parse_args(argv)
    if args.self_test:
        return run_self_test()
    if not args.candidate:
        print("compare_sites: --candidate is required (or use --self-test)", file=sys.stderr)
        return 2
    try:
        report = compare_sites(args)
    except CompareUsageError as exc:
        print("compare_sites: %s" % exc, file=sys.stderr)
        return 2
    except Exception as exc:  # noqa: BLE001 - infrastructure errors -> exit 2
        print("compare_sites: error: %s" % exc, file=sys.stderr)
        return 2

    if args.report:
        report_path = Path(args.report)
        report_path.parent.mkdir(parents=True, exist_ok=True)
        with open(report_path, "w", encoding="utf-8") as f:
            json.dump(report, f, indent=2, ensure_ascii=False)
            f.write("\n")

    if args.json:
        print(json.dumps(report, indent=2, ensure_ascii=False))
    else:
        print_human_report(report, args)
    return 0 if report["summary"]["result"] == "PASS" else 1


if __name__ == "__main__":
    sys.exit(main())
