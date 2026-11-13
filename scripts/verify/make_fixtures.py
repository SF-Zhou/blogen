#!/usr/bin/env python3
"""Generate the ORACLE outputs for tests/fixtures/expected/ with the legacy generator.

test fixture site:  tests/fixtures/site/  (blogen.toml + posts/)
oracle outputs:     tests/fixtures/expected/

The oracle is produced by the *original* legacy blog pipeline in a throwaway
staging directory. The blog repository has been migrated to blogen (its HEAD no
longer has `src/main.js`), so the legacy generator commit is resolved as:
`--ref`/`$BLOGEN_BASELINE_REF`, else HEAD if it still has `src/main.js`, else
the pinned legacy commit `d4cb7aafad90f09cdcd85e2482eae739483743a5`.

  1. `git -C <blog> archive <legacy-ref> | tar -x -C <tmp>/blog`  (never touch the blog)
  2. symlink the legacy node_modules (read-only) so the staged copy has the
     exact pinned dependency versions. Resolution: `--node-modules` /
     `$BLOGEN_BASELINE_NODE_MODULES`, else `<blog>/node_modules`, else
     `scripts/verify/.cache/legacy-deps/node_modules` (created on demand by
     `make_baseline.sh --bootstrap-node-modules`).
  3. overwrite the staged config.json with one that points
     `posts_path` at a copy of tests/fixtures/site/posts
  4. `node src/main.js`  (this is what build.sh runs; build.sh's less/css copying
     is not needed for the oracle - article/index/rss/profile are pure Node)
  5. post-process: extract article body HTML, replace server-rendered Mermaid SVG
     figures with a placeholder, and replace the volatile RSS <lastBuildDate>

Oracle conventions (also documented in tests/fixtures/README.md):

  expected/pages/<rel>.html          full article page, exactly as the old site
                                     rendered it (mermaid figures replaced by
                                     `<figure class="mermaid"></figure>`)
  expected/bodies/<rel>.html         inner HTML of `<div class="markdown">` of the
                                     article page (the article body)
  expected/pages/index.html          full index page (card list)
  expected/cards.html                inner HTML of the index page's
                                     `<div class="markdown">`
  expected/index.json                Node index.json (semantic, ordering is exact)
  expected/rss.xml                   Node rss.xml; <lastBuildDate> is the literal
                                     placeholder __LAST_BUILD_DATE__
  expected/profile/README.md         Node profile output
  expected/mermaid/<rel>.json        list of mermaid diagram definitions for that
                                     article, in document order (the SVG itself is
                                     not stored; blogen renders Mermaid to inline SVG at
                                 build time, so tests normalise the figure subtree)
  expected/MANIFEST.json             provenance + sha256 of every oracle file

Usage:
  scripts/verify/make_fixtures.sh                # regenerate expected/
  scripts/verify/make_fixtures.sh --check        # verify expected/ is up to date
  scripts/verify/make_fixtures.sh --blog-dir DIR --fixtures-dir DIR
  scripts/verify/make_fixtures.sh --ref COMMIT --node-modules DIR

Environment: BLOGEN_BASELINE_REF, BLOGEN_BASELINE_NODE_MODULES (see above).

Exit codes: 0 ok, 1 generation/check failure, 2 usage error.
"""

import argparse
import difflib
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

# Never leave __pycache__ behind in the repository when importing compare_sites.
sys.dont_write_bytecode = True

SCRIPT_DIR = Path(__file__).resolve().parent
REPO_ROOT = SCRIPT_DIR.parent.parent
DEFAULT_FIXTURES = REPO_ROOT / "tests" / "fixtures"
DEFAULT_BLOG = REPO_ROOT.parent / "blog"
DEFAULT_NODE_MODULES = REPO_ROOT.parent / "blog" / "node_modules"
# Last legacy generator-generator blog commit (before the repository switched to blogen).
LEGACY_BASELINE_REF = "d4cb7aafad90f09cdcd85e2482eae739483743a5"
# Stable label recorded in MANIFEST.json (an absolute checkout path would make
# `--check` fail when the blog checkout lives somewhere else).
BLOG_REPO_LABEL = "SF-Zhou/blog (local checkout)"

MERMAID_FENCE_RE = re.compile(
    r"^[ \t]*```[ \t]*mermaid[ \t]*\r?\n(.*?)^[ \t]*```[ \t]*\r?$",
    re.MULTILINE | re.DOTALL,
)
MERMAID_FIGURE_RE = re.compile(
    r'<figure class="mermaid">.*?</figure>', re.DOTALL
)
MERMAID_PLACEHOLDER = '<figure class="mermaid"></figure>'
LAST_BUILD_DATE_PLACEHOLDER = "__LAST_BUILD_DATE__"
RSS_LAST_BUILD_RE = re.compile(r"<lastBuildDate>.*?</lastBuildDate>", re.DOTALL)

# Deps that determine the oracle output; recorded in MANIFEST.json.
DEP_NAMES = [
    "markdown-it", "markdown-it-anchor", "markdown-it-implicit-figures",
    "markdown-it-table-of-contents", "markdown-it-texmath", "katex", "prismjs",
    "md5", "mustache", "mermaid", "@mermaid-js/mermaid-cli",
]

NODE_CONFIG = {
    "site_name": "Fixture Blog",
    "site_url": "https://sf-zhou.github.io",
    "site_description": "blogen parity fixture site",
    "site_language": "en",
    "article_format": "md",
    "default_author": "fixture-author",
    "web_master": "fixture-master",
    "github_repo": "SF-Zhou/sf-zhou.github.io",
    "github_repo_id": "MDEwOlJlcG9zaXRvcnk4MDc5ODgwNg==",
    "google_analytics_id": "G-FIXTURE",
}


def die(message):
    print("make_fixtures: %s" % message, file=sys.stderr)
    sys.exit(2)


def find_node():
    node = shutil.which("node")
    if node:
        return node
    # nvm fallback: pick the highest installed node version.
    candidates = sorted(
        Path.home().glob(".nvm/versions/node/v*/bin/node"), reverse=True)
    for candidate in candidates:
        if candidate.is_file():
            return str(candidate)
    die("node not found on PATH and no ~/.nvm node installation found")


def read_text(path):
    with open(path, "r", encoding="utf-8") as f:
        return f.read()


def write_text(path, text):
    path.parent.mkdir(parents=True, exist_ok=True)
    with open(path, "w", encoding="utf-8", newline="") as f:
        f.write(text)


def sha256_bytes(data):
    return hashlib.sha256(data).hexdigest()


def display_path(path):
    """Path relative to the blogen repo when possible (stable in MANIFEST.json)."""
    path = Path(path).resolve()
    try:
        return path.relative_to(REPO_ROOT.resolve()).as_posix()
    except ValueError:
        return path.as_posix()


def import_compare_module():
    import importlib.util

    spec = importlib.util.spec_from_file_location(
        "blogen_compare_sites", SCRIPT_DIR / "compare_sites.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def git_output(blog_dir, *args):
    return subprocess.run(
        ["git", "-C", str(blog_dir)] + list(args),
        capture_output=True, text=True)


def resolve_legacy_ref(blog_dir, explicit_ref):
    """Pick the legacy generator commit and return (ref, commit, source)."""
    def exists(ref):
        return git_output(blog_dir, "rev-parse", "--verify", "--quiet",
                          ref + "^{commit}").returncode == 0

    def has_legacy_generator(ref):
        return git_output(blog_dir, "cat-file", "-e", ref + ":src/main.js").returncode == 0

    if explicit_ref:
        if not exists(explicit_ref):
            die("legacy baseline ref '%s' not found in %s.\n"
                "The blog HEAD was migrated to blogen; pass --ref <commit> or set\n"
                "BLOGEN_BASELINE_REF to a commit containing src/main.js (pinned\n"
                "default: %s), or fetch the full blog history."
                % (explicit_ref, blog_dir, LEGACY_BASELINE_REF))
        if not has_legacy_generator(explicit_ref):
            die("ref '%s' has no src/main.js, so it is not a legacy legacy generator\n"
                "generator commit. Use %s or another commit containing src/main.js."
                % (explicit_ref, LEGACY_BASELINE_REF))
        commit = git_output(blog_dir, "rev-parse", explicit_ref + "^{commit}").stdout.strip()
        return explicit_ref, commit, "explicit"

    if has_legacy_generator("HEAD"):
        commit = git_output(blog_dir, "rev-parse", "HEAD").stdout.strip()
        return "HEAD", commit, "HEAD"

    if exists(LEGACY_BASELINE_REF):
        if not has_legacy_generator(LEGACY_BASELINE_REF):
            die("pinned legacy commit %s has no src/main.js; set BLOGEN_BASELINE_REF."
                % LEGACY_BASELINE_REF)
        return LEGACY_BASELINE_REF, LEGACY_BASELINE_REF, "pinned-legacy-fallback"

    die("HEAD no longer contains the legacy generator (src/main.js) and the\n"
        "pinned legacy commit %s is not present in %s.\n"
        "Pass --ref <commit> or set BLOGEN_BASELINE_REF to a legacy generator\n"
        "commit (one containing src/main.js), or fetch the full blog history."
        % (LEGACY_BASELINE_REF, blog_dir))


def resolve_node_modules(explicit):
    candidates = []
    if explicit:
        candidates.append(Path(explicit))
    candidates.append(DEFAULT_NODE_MODULES)
    candidates.append(SCRIPT_DIR / ".cache" / "legacy-deps" / "node_modules")
    for candidate in candidates:
        if (candidate / ".bin" / "lessc").is_file():
            return candidate.resolve()
    die("legacy node_modules not found (tried: %s).\n"
        "The oracle build runs the original pipeline, so it needs the\n"
        "legacy dependency tree. Restore it once with:\n"
        "  scripts/verify/make_baseline.sh --bootstrap-node-modules\n"
        "(installs it into scripts/verify/.cache/legacy-deps/node_modules; needs network),\n"
        "or pass --node-modules / BLOGEN_BASELINE_NODE_MODULES."
        % ", ".join(str(c) for c in candidates))


def stack_archive(blog_dir, dest, ref):
    dest.mkdir(parents=True, exist_ok=True)
    archive = subprocess.run(
        ["git", "-C", str(blog_dir), "archive", ref],
        check=True, capture_output=True)
    subprocess.run(["tar", "-x", "-C", str(dest)], input=archive.stdout, check=True)


def dep_version(node_modules, name):
    manifest = node_modules / name / "package.json"
    if not manifest.is_file():
        return None
    return json.loads(read_text(manifest)).get("version")


def find_source_markdown(posts_root, stem):
    """Map an output path stem to its source markdown file.

    The source file name is `<name>.md`, optionally prefixed with the metadata
    bracket `[date tags author]` that the old generator strips from the output.
    """
    directory = posts_root / Path(stem).parent
    name = Path(stem).name
    if not directory.is_dir():
        return None
    pattern = re.compile(r"^(?:\[[^\]]*\])?%s\.md$" % re.escape(name))
    matches = [path for path in sorted(directory.iterdir())
               if path.is_file() and pattern.match(path.name)]
    if len(matches) > 1:
        raise RuntimeError(
            "ambiguous markdown source for %s: %s"
            % (stem, ", ".join(m.name for m in matches)))
    return matches[0] if matches else None


def extract_mermaid_definitions(source_text):
    return [m.group(1).rstrip("\n") for m in MERMAID_FENCE_RE.finditer(source_text)]


def build_expected(tmp, fixtures_dir, blog_dir, node, ref, ref_commit, node_modules):
    """Build the oracle in `tmp`, returning {relpath: bytes} of files to store."""
    staged_blog = tmp / "blog"
    archive_root = tmp / "archive"
    stack_archive(blog_dir, archive_root, ref)
    # The archive root contains src/, dist/, posts/, config.json, package.json, ...
    shutil.copytree(archive_root / "src", staged_blog / "src")
    staged_blog.mkdir(parents=True, exist_ok=True)
    os.symlink(node_modules, staged_blog / "node_modules")

    posts_copy = tmp / "posts"
    shutil.copytree(fixtures_dir / "site" / "posts", posts_copy)
    out_dir = tmp / "out"
    profile_dir = tmp / "profile"

    config = dict(NODE_CONFIG)
    config.update({
        "posts_path": str(posts_copy),
        "output_path": str(out_dir),
        "profile_path": str(profile_dir),
    })
    write_text(staged_blog / "config.json", json.dumps(config, indent=2) + "\n")

    # Run the exact same entry point build.sh runs. The old generator logs a
    # prism "no grammar" error for the unknown-language fixture fence; keep it
    # out of the output unless generation fails.
    subprocess.run([node, "src/main.js"], cwd=str(staged_blog), check=True,
                   capture_output=True)

    compare = import_compare_module()
    files = {}  # relpath -> bytes

    # index.json / rss.xml / profile
    files["index.json"] = (out_dir / "index.json").read_bytes()
    rss = read_text(out_dir / "rss.xml")
    rss = RSS_LAST_BUILD_RE.sub(
        "<lastBuildDate>%s</lastBuildDate>" % LAST_BUILD_DATE_PLACEHOLDER, rss)
    files["rss.xml"] = rss.encode()
    files["profile/README.md"] = (profile_dir / "README.md").read_bytes()

    # index.html + cards
    index_html = read_text(out_dir / "index.html")
    if "<figure" in index_html and "mermaid" in index_html:
        raise RuntimeError("index page should not contain mermaid figures")
    files["pages/index.html"] = index_html.encode()
    cards = compare.extract_markdown_inner(index_html)
    if cards is None:
        raise RuntimeError("index.html has no <div class=\"markdown\">")
    files["cards.html"] = cards.encode()

    # article pages (hidden posts are .htm)
    page_count = 0
    for path in sorted(out_dir.rglob("*")):
        if not path.is_file() or path.suffix not in (".html", ".htm"):
            continue
        rel = path.relative_to(out_dir).as_posix()
        if rel == "index.html":
            continue
        page = read_text(path)
        body = compare.extract_markdown_inner(page)
        if body is None:
            raise RuntimeError("%s has no <div class=\"markdown\">" % rel)
        # Replace server-rendered Mermaid SVG with a stable placeholder.
        page_norm = MERMAID_FIGURE_RE.sub(MERMAID_PLACEHOLDER, page)
        body_norm = MERMAID_FIGURE_RE.sub(MERMAID_PLACEHOLDER, body)
        files["pages/" + rel] = page_norm.encode()
        files["bodies/" + rel] = body_norm.encode()
        page_count += 1

        # Mermaid definitions come from the source markdown, in document order.
        stem = rel[: -len(".html")] if rel.endswith(".html") else rel[: -len(".htm")]
        source_md = find_source_markdown(posts_copy, stem)
        if source_md is not None:
            definitions = extract_mermaid_definitions(read_text(source_md))
            placeholder_count = body_norm.count(MERMAID_PLACEHOLDER)
            if definitions:
                if len(definitions) != placeholder_count:
                    raise RuntimeError(
                        "%s: %d mermaid fence(s) but %d placeholder(s)"
                        % (rel, len(definitions), placeholder_count))
                files["mermaid/" + rel + ".json"] = (
                    json.dumps(definitions, indent=2, ensure_ascii=False) + "\n"
                ).encode()
            elif placeholder_count:
                raise RuntimeError(
                    "%s: mermaid placeholder without a source fence" % rel)

    # -- MANIFEST.json ------------------------------------------------------
    commit = ref_commit
    commit_subject = subprocess.run(
        ["git", "-C", str(blog_dir), "log", "-1", "--pretty=%s", commit],
        check=True, capture_output=True, text=True).stdout.strip()
    node_version = subprocess.run(
        [node, "--version"], check=True, capture_output=True, text=True).stdout.strip()

    file_hashes = {}
    for rel in sorted(files):
        file_hashes[rel] = sha256_bytes(files[rel])

    manifest = {
        "schema_version": 1,
        "purpose": "oracle outputs for blogen integration tests (tests/fixtures)",
        "generated_by": "scripts/verify/make_fixtures.sh",
        # Only the resolved commit is recorded (not the ref/source used to find
        # it), so MANIFEST.json and `--check` are stable whether the legacy
        # generator is found at HEAD or via the pinned post-migration fallback.
        "source": {
            "repo": BLOG_REPO_LABEL,
            "commit": commit,
            "commit_subject": commit_subject,
            "pinned_legacy_ref": LEGACY_BASELINE_REF,
            "pipeline": "legacy blog/src/main.js",
            "command": "node src/main.js (staged git archive %s + node_modules symlink)" % commit,
        },
        "generator": {
            "node": node_version,
            "deps": {name: dep_version(node_modules, name) for name in DEP_NAMES},
            "legacy_deps_sha256": hashlib.sha256(
                (SCRIPT_DIR / "legacy-deps.json").read_bytes()).hexdigest()
            if (SCRIPT_DIR / "legacy-deps.json").is_file() else None,
            "legacy_package_lock_sha256": hashlib.sha256(
                (SCRIPT_DIR / "legacy-package-lock.json").read_bytes()).hexdigest()
            if (SCRIPT_DIR / "legacy-package-lock.json").is_file() else None,
        },
        "input_site": {
            "config": display_path(fixtures_dir / "site" / "blogen.toml"),
            "posts": sorted(
                path.relative_to(fixtures_dir / "site" / "posts").as_posix()
                for path in (fixtures_dir / "site" / "posts").rglob("*")
                if path.is_file()),
            "node_config": {k: NODE_CONFIG[k] for k in sorted(NODE_CONFIG)},
        },
        "conventions": {
            "pages": "full article page HTML as rendered by the old site",
            "bodies": "inner HTML of <div class=\"markdown\"> in pages/",
            "mermaid_placeholder": MERMAID_PLACEHOLDER,
            "mermaid_definitions": "mermaid/<rel>.json lists the fence contents in document order",
            "rss_last_build_date": LAST_BUILD_DATE_PLACEHOLDER,
            "ordering": "index.json/index.html/rss.xml are ordered by date desc, then title",
        },
        "stats": {
            "article_pages": page_count,
            "expected_files": len(files) + 1,
            "expected_bytes": sum(len(v) for v in files.values()),
        },
        "files": file_hashes,
    }
    files["MANIFEST.json"] = (json.dumps(manifest, indent=2) + "\n").encode()
    return files, page_count


def write_tree(dest, files):
    if dest.exists():
        shutil.rmtree(dest)
    dest.mkdir(parents=True)
    for rel, data in sorted(files.items()):
        path = dest / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)


def tree_diff(expected_dir, generated):
    problems = []
    existing = {}
    for path in expected_dir.rglob("*"):
        if path.is_file():
            existing[path.relative_to(expected_dir).as_posix()] = path.read_bytes()
    for rel in sorted(set(existing) | set(generated)):
        if rel not in generated:
            problems.append("unexpected committed file: %s" % rel)
        elif rel not in existing:
            problems.append("missing committed file: %s" % rel)
        elif existing[rel] != generated[rel]:
            problem = "content differs: %s" % rel
            if rel.endswith((".json", ".md", ".txt", ".xml", ".html", ".htm")):
                diff = list(
                    difflib.unified_diff(
                        existing[rel].decode("utf-8", "replace").splitlines(),
                        generated[rel].decode("utf-8", "replace").splitlines(),
                        fromfile="committed/" + rel,
                        tofile="regenerated/" + rel,
                        n=1,
                        lineterm="",
                    )
                )
                if diff:
                    problem += "\n" + "\n".join("      " + line for line in diff[:24])
                    if len(diff) > 24:
                        problem += "\n      ... (%d more diff lines)" % (len(diff) - 24)
            problems.append(problem)
    return problems


def main(argv=None):
    parser = argparse.ArgumentParser(
        description="Generate tests/fixtures/expected with the original pipeline.")
    parser.add_argument("--blog-dir", default=str(DEFAULT_BLOG),
                        help="read-only legacy blog checkout (default: %(default)s)")
    parser.add_argument("--fixtures-dir", default=str(DEFAULT_FIXTURES),
                        help="fixtures dir (default: %(default)s)")
    parser.add_argument("--ref", default=os.environ.get("BLOGEN_BASELINE_REF"),
                        help="legacy generator commit/ref "
                             "(default: HEAD if it has src/main.js, else pinned %s)"
                             % LEGACY_BASELINE_REF)
    parser.add_argument("--node-modules", default=os.environ.get("BLOGEN_BASELINE_NODE_MODULES"),
                        help="legacy node_modules directory (default: blog/node_modules, "
                             "else scripts/verify/.cache/legacy-deps/node_modules)")
    parser.add_argument("--check", action="store_true",
                        help="regenerate in memory and verify tests/fixtures/expected is up to date")
    parser.add_argument("--keep-tmp", action="store_true",
                        help="keep the staging directory for debugging")
    args = parser.parse_args(argv)

    blog_dir = Path(args.blog_dir).resolve()
    fixtures_dir = Path(args.fixtures_dir).resolve()
    if not (blog_dir / ".git").is_dir():
        die("%s is not a git repository; the oracle must come from a git archive" % blog_dir)
    if not (fixtures_dir / "site" / "posts").is_dir():
        die("%s/site/posts missing" % fixtures_dir)

    ref, ref_commit, ref_source = resolve_legacy_ref(blog_dir, args.ref)
    node_modules = resolve_node_modules(args.node_modules)
    node = find_node()
    tmp = Path(tempfile.mkdtemp(prefix="blogen-fixtures-"))
    try:
        files, page_count = build_expected(
            tmp, fixtures_dir, blog_dir, node, ref, ref_commit, node_modules)
    except subprocess.CalledProcessError as exc:
        print("make_fixtures: Node generator failed: %s" % exc, file=sys.stderr)
        if exc.stderr:
            print(exc.stderr.decode(errors="replace"), file=sys.stderr)
        return 1
    except Exception as exc:  # noqa: BLE001 - any oracle build error fails generation
        print("make_fixtures: oracle generation failed: %s" % exc, file=sys.stderr)
        return 1
    finally:
        if not args.keep_tmp:
            shutil.rmtree(tmp, ignore_errors=True)

    expected_dir = fixtures_dir / "expected"
    total_bytes = sum(len(v) for v in files.values())

    if args.check:
        problems = tree_diff(expected_dir, files)
        if problems:
            print("make_fixtures --check: FAIL (%d problem(s))" % len(problems))
            for problem in problems[:40]:
                print("  ! %s" % problem)
            if len(problems) > 40:
                print("  ... (%d more)" % (len(problems) - 40))
            return 1
        print("make_fixtures --check: OK (%d files, %d article page(s), %.1f KiB up to date)"
              % (len(files), page_count, total_bytes / 1024.0))
        return 0

    write_tree(expected_dir, files)
    print("make_fixtures: wrote %s" % expected_dir)
    print("  article pages: %d (incl. hidden .htm posts)" % page_count)
    print("  files:         %d, %.1f KiB" % (len(files), total_bytes / 1024.0))
    print("  oracle:        %s" % json.loads(files["MANIFEST.json"])["source"]["commit"][:12])
    if total_bytes > 300 * 1024:
        print("make_fixtures: WARNING: fixtures exceed the 300 KiB budget", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
