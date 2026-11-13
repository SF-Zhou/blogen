#!/usr/bin/env python3
"""Pixel-diff two PNG screenshots (PIL + numpy) and write a diff image.

Used by scripts/verify/screenshots.mjs. Prints a JSON object to stdout, e.g.:

  {"width_a":1100,"height_a":4200,"width_b":1100,"height_b":4200,
   "size_mismatch":false,"differing_pixels":123,"total_pixels":4620000,
   "differing_pct":0.00266,"mean_abs_diff":0.0012,"max_abs_diff":255}

Images are composited on white (alpha) and compared per channel: a pixel differs
when any channel differs by more than --tol (default 12, which ignores PNG
re-encoding / anti-aliasing noise). A size mismatch pads the smaller image with
white and is reported explicitly; padding pixels count as differing.

Usage:
  python3 pixdiff.py --a before.png --b after.png [--tol 12]
                     [--out-diff diff.png] [--json-out report.json]
"""

import argparse
import json
import sys


def load_rgb(path):
    from PIL import Image

    img = Image.open(path).convert("RGBA")
    background = Image.new("RGBA", img.size, (255, 255, 255, 255))
    return Image.alpha_composite(background, img).convert("RGB")


def pad(img, width, height):
    from PIL import Image

    if img.size == (width, height):
        return img
    canvas = Image.new("RGB", (width, height), (255, 255, 255))
    canvas.paste(img, (0, 0))
    return canvas


def main(argv=None):
    parser = argparse.ArgumentParser(description="Pixel-diff two PNG screenshots.")
    parser.add_argument("--a", required=True, help="baseline PNG")
    parser.add_argument("--b", required=True, help="candidate PNG")
    parser.add_argument("--tol", type=int, default=12,
                        help="per-channel difference tolerance 0-255 (default: 12)")
    parser.add_argument("--out-diff", help="write a diff PNG here")
    parser.add_argument("--json-out", help="also write the JSON report here")
    args = parser.parse_args(argv)

    try:
        import numpy as np
        from PIL import Image
    except ImportError as exc:  # pragma: no cover - environment issue
        print("pixdiff: PIL/numpy are required: %s" % exc, file=sys.stderr)
        return 2

    try:
        image_a = load_rgb(args.a)
        image_b = load_rgb(args.b)
    except Exception as exc:  # noqa: BLE001
        print("pixdiff: cannot read image: %s" % exc, file=sys.stderr)
        return 2

    original_a = image_a.size
    original_b = image_b.size
    width = max(image_a.width, image_b.width)
    height = max(image_a.height, image_b.height)
    size_mismatch = image_a.size != image_b.size
    image_a = pad(image_a, width, height)
    image_b = pad(image_b, width, height)

    a = np.asarray(image_a, dtype=np.int16)
    b = np.asarray(image_b, dtype=np.int16)
    delta = np.abs(a - b)
    differing = (delta > args.tol).any(axis=2)
    differing_pixels = int(differing.sum())
    total_pixels = width * height
    report = {
        "width_a": original_a[0],
        "height_a": original_a[1],
        "width_b": original_b[0],
        "height_b": original_b[1],
        "canvas_width": width,
        "canvas_height": height,
        "size_mismatch": size_mismatch,
        "tolerance": args.tol,
        "differing_pixels": differing_pixels,
        "total_pixels": total_pixels,
        "differing_pct": round(100.0 * differing_pixels / total_pixels, 6) if total_pixels else 0.0,
        "mean_abs_diff": round(float(delta.mean()), 6),
        "max_abs_diff": int(delta.max()) if total_pixels else 0,
    }
    if args.out_diff:
        # Dimmed baseline with differing pixels highlighted in red-ish tones.
        dimmed = (a * 0.28 + 255 * 0.72).astype(np.uint8)
        dimmed[differing] = (np.array([220, 0, 0], dtype=np.uint8))
        Image.fromarray(dimmed, "RGB").save(args.out_diff)

    text = json.dumps(report)
    if args.json_out:
        with open(args.json_out, "w", encoding="utf-8") as handle:
            handle.write(text + "\n")
    print(text)
    return 0


if __name__ == "__main__":
    sys.exit(main())
