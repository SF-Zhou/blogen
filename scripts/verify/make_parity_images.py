#!/usr/bin/env python3
"""Compose docs/parity/ evidence images from the raw captures.

Reads the raw PNGs produced by `scripts/verify/make_parity_images.mjs` and
writes:

  docs/parity/mermaid-static-vs-baseline.png
      2x2 comparison of one Mermaid sequence diagram: reference baseline
      (server-rendered mermaid-cli SVG) vs the current blogen static rendering
      (build-time merman SVG), light and dark.
  docs/parity/index-light.jpg          home page (blogen, static build)
  docs/parity/math-code-light.jpg      article with math + highlighted code + table
  docs/parity/mermaid-dark.jpg         Mermaid article, dark mode (static SVG)

The old `mermaid-client-vs-baseline.png` (client-side JS rendering) is deleted:
it is superseded by the static comparison.

The whole docs/parity directory is kept below --max-total-bytes (default
600 KiB) by scaling the comparison panels and lowering the JPEG quality until
it fits; the chosen sizes are printed.

Usage:
  node scripts/verify/make_parity_images.mjs          # raw captures first
  python3 scripts/verify/make_parity_images.py
  python3 scripts/verify/make_parity_images.py --raw DIR --docs DIR --max-total-bytes 614400
"""

import argparse
import io
import os
import sys
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

SCRIPT_DIR = Path(__file__).resolve().parent
REPO_ROOT = SCRIPT_DIR.parent.parent
DEFAULT_RAW = SCRIPT_DIR / ".cache" / "parity-raw"
DEFAULT_DOCS = REPO_ROOT / "docs" / "parity"
DEFAULT_MAX_TOTAL = 600 * 1024

FONT_REGULAR = Path("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf")
FONT_BOLD = Path("/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf")

COLUMN_LABELS = [
    "reference baseline  (server-rendered mermaid-cli SVG)",
    "blogen static  (build-time merman SVG, no JavaScript)",
]
ROW_LABELS = ["light", "dark"]
CAPTION = (
    "Same sequence diagram, rendered to SVG before the page is served in both cases. "
    "Accepted difference: Mermaid version, theme palette and text metrics differ slightly."
)


def load_font(size, bold=False):
    path = FONT_BOLD if bold else FONT_REGULAR
    try:
        return ImageFont.truetype(str(path), size)
    except OSError:
        return ImageFont.load_default()


def text_size(draw, text, font):
    # PIL < 8 has no textbbox; textsize exists in both (deprecated in newer PIL).
    if hasattr(draw, "textbbox"):
        left, top, right, bottom = draw.textbbox((0, 0), text, font=font)
        return right - left, bottom - top
    return draw.textsize(text, font=font)


def compose_comparison(raw, panel_width, padding=16, header_height=48, row_label_width=64, gutter=16):
    panels = {}
    for label in ("baseline", "candidate"):
        for scheme in ("light", "dark"):
            path = raw / f"mermaid-{label}-{scheme}.png"
            if not path.is_file():
                raise SystemExit(f"missing raw capture: {path}")
            panels[(label, scheme)] = Image.open(path).convert("RGB")

    columns = ["baseline", "candidate"]
    scaled = {}
    for key, image in panels.items():
        scale = panel_width / image.width
        scaled[key] = image.resize(
            (panel_width, max(1, round(image.height * scale))), Image.LANCZOS)

    row_heights = [
        max(scaled[(label, scheme)].height for label in columns)
        for scheme in ("light", "dark")
    ]
    width = padding * 2 + row_label_width + panel_width * 2 + gutter
    height = (padding + header_height + row_heights[0] + gutter
              + row_heights[1] + padding + 34)
    canvas = Image.new("RGB", (width, height), (255, 255, 255))
    draw = ImageDraw.Draw(canvas)
    font_header = load_font(15, bold=True)
    font_row = load_font(16, bold=True)
    font_caption = load_font(13, bold=False)
    muted = (90, 90, 90)

    # Column headers (centred over each panel).
    for index, label in enumerate(COLUMN_LABELS):
        x = padding + row_label_width + index * (panel_width + gutter)
        text_w, text_h = text_size(draw, label, font_header)
        draw.text((x + (panel_width - text_w) // 2, padding + (header_height - text_h) // 2 - 6),
                  label, fill=(40, 40, 40), font=font_header)

    # Panel rows.
    y = padding + header_height
    column_x = [padding + row_label_width + index * (panel_width + gutter) for index in range(2)]
    for row, scheme in enumerate(("light", "dark")):
        for index, label in enumerate(columns):
            image = scaled[(label, scheme)]
            canvas.paste(image, (column_x[index], y))
            draw.rectangle(
                [column_x[index], y, column_x[index] + image.width - 1, y + image.height - 1],
                outline=(224, 224, 224),
            )
        text = ROW_LABELS[row]
        text_w, text_h = text_size(draw, text, font_row)
        draw.text((padding + (row_label_width - text_w) // 2, y + 12), text, fill=(40, 40, 40), font=font_row)
        y += row_heights[row] + gutter

    # Caption.
    caption_y = y + 6
    draw.text((padding, caption_y), CAPTION, fill=muted, font=font_caption)
    return canvas


def save_png_under_budget(image, path, max_bytes):
    """Save PNG, reducing the panel scale only if the file is too large."""
    for scale, quality in ((1.0, None), (0.9, None), (0.8, None), (0.7, None)):
        candidate = image
        if scale != 1.0:
            candidate = image.resize(
                (max(1, round(image.width * scale)), max(1, round(image.height * scale))),
                Image.LANCZOS)
        buffer = io.BytesIO()
        candidate.save(buffer, format="PNG", optimize=True)
        data = buffer.getvalue()
        if len(data) <= max_bytes:
            path.write_bytes(data)
            return len(data)
    raise SystemExit("could not fit the comparison image into the docs budget")


def save_page_jpeg(raw_path, out_path, width, quality):
    image = Image.open(raw_path).convert("RGB")
    height = max(1, round(image.height * width / image.width))
    resized = image.resize((width, height), Image.LANCZOS)
    resized.save(out_path, format="JPEG", quality=quality, optimize=True, progressive=True)
    return out_path.stat().st_size


def main(argv=None):
    parser = argparse.ArgumentParser(description="Compose docs/parity evidence images.")
    parser.add_argument("--raw", default=str(DEFAULT_RAW), help="raw captures dir")
    parser.add_argument("--docs", default=str(DEFAULT_DOCS), help="docs/parity dir")
    parser.add_argument("--max-total-bytes", type=int, default=DEFAULT_MAX_TOTAL,
                        help="docs/parity size budget in bytes (default: 600 KiB)")
    parser.add_argument("--panel-width", type=int, default=700,
                        help="comparison panel width in px (default: 700)")
    args = parser.parse_args(argv)

    raw = Path(args.raw)
    docs = Path(args.docs)
    docs.mkdir(parents=True, exist_ok=True)
    if not (raw / "mermaid-baseline-light.png").is_file():
        print("make_parity_images: raw captures missing; run "
              "`node scripts/verify/make_parity_images.mjs` first", file=sys.stderr)
        return 2

    old_client = docs / "mermaid-client-vs-baseline.png"
    if old_client.is_file():
        old_client.unlink()
        print(f"removed superseded {old_client.relative_to(REPO_ROOT)}")

    comparison_path = docs / "mermaid-static-vs-baseline.png"
    comparison = compose_comparison(raw, args.panel_width)
    comparison_bytes = save_png_under_budget(comparison, comparison_path, 260 * 1024)
    print(f"wrote {comparison_path.relative_to(REPO_ROOT)} "
          f"({comparison.width}x{comparison.height}, {comparison_bytes / 1024:.0f} KiB)")

    extra_files = [p for p in docs.iterdir() if p.is_file() and p.name != "README.md"]

    def docs_total():
        return sum(p.stat().st_size for p in docs.iterdir() if p.is_file())

    page_jobs = [
        ("page-index-light.png", "index-light.jpg"),
        ("page-math-code-light.png", "math-code-light.jpg"),
        ("page-mermaid-dark.png", "mermaid-dark.jpg"),
    ]
    written = {comparison_path.name: comparison_bytes}
    for quality in (84, 80, 76, 72, 68, 64, 60):
        for raw_name, out_name in page_jobs:
            out_path = docs / out_name
            written[out_name] = save_page_jpeg(raw / raw_name, out_path, 700, quality)
        if docs_total() <= args.max_total_bytes:
            break
    total = docs_total()
    print(f"wrote {len(page_jobs)} page screenshot(s) at 700 px wide (JPEG quality {quality})")
    for name, size in written.items():
        print(f"  {name}: {size / 1024:.0f} KiB")
    print(f"docs/parity total (all files): {total / 1024:.0f} KiB "
          f"(budget {args.max_total_bytes / 1024:.0f} KiB)")
    if total > args.max_total_bytes:
        print("make_parity_images: docs/parity exceeds the size budget", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
