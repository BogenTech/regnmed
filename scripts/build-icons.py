#!/usr/bin/env python3
"""Generates the PWA icons (docs/portal.md, #48).

Hand-rolled PNG writer, for the same reason the PDF and XML writers are
hand-rolled: a build step we understand end to end, with no image
library in the tree. The output is checked in like portal/app.css —
running this again produces byte-identical files.

    python3 scripts/build-icons.py

The mark is the ledger itself: a rounded square in the brand colour
with three ruled lines, the middle one shorter — a page with entries.
"""

import struct
import zlib
from pathlib import Path

OUT = Path(__file__).resolve().parent.parent / "crates/regnmed-api/portal"

# Brand: the same indigo the portal's primary uses, on a light page.
INK = (79, 70, 229)
PAPER = (255, 255, 255)


def rounded_square_mask(size, inset, radius):
    """Pixel mask for a rounded square, 4x supersampled edges."""
    mask = [[0.0] * size for _ in range(size)]
    lo, hi = inset, size - inset - 1
    for y in range(size):
        for x in range(size):
            hits = 0
            for sy in range(4):
                for sx in range(4):
                    px = x + (sx + 0.5) / 4
                    py = y + (sy + 0.5) / 4
                    # Distance to the rounded rectangle.
                    dx = max(lo + radius - px, 0, px - (hi - radius))
                    dy = max(lo + radius - py, 0, py - (hi - radius))
                    inside = lo <= px <= hi and lo <= py <= hi
                    if inside and dx * dx + dy * dy <= radius * radius + 1e-9:
                        hits += 1
                    elif inside and (dx == 0 or dy == 0):
                        hits += 1
            mask[y][x] = hits / 16
    return mask


def blend(bg, fg, alpha):
    return tuple(round(b + (f - b) * alpha) for b, f in zip(bg, fg))


def render(size, maskable):
    """maskable leaves the safe-zone padding Android crops into a circle."""
    inset = round(size * (0.18 if maskable else 0.08))
    radius = round(size * 0.16)
    mark = rounded_square_mask(size, inset, radius)
    pixels = [[PAPER] * size for _ in range(size)]
    for y in range(size):
        for x in range(size):
            if mark[y][x]:
                pixels[y][x] = blend(PAPER, INK, mark[y][x])

    # Three ruled lines across the mark: a page with entries.
    body = size - 2 * inset
    line_h = max(2, round(body * 0.09))
    widths = (0.60, 0.42, 0.60)
    for i, w in enumerate(widths):
        top = round(inset + body * (0.30 + i * 0.20))
        left = round(inset + body * 0.20)
        right = round(left + body * w)
        for y in range(top, min(top + line_h, size)):
            for x in range(left, min(right, size)):
                pixels[y][x] = PAPER
    return pixels


def write_png(path, pixels):
    size = len(pixels)
    raw = bytearray()
    for row in pixels:
        raw.append(0)  # filter type 0
        for r, g, b in row:
            raw += bytes((r, g, b))

    def chunk(tag, data):
        return (
            struct.pack(">I", len(data))
            + tag
            + data
            + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)
        )

    png = b"\x89PNG\r\n\x1a\n"
    png += chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 2, 0, 0, 0))
    png += chunk(b"IDAT", zlib.compress(bytes(raw), 9))
    png += chunk(b"IEND", b"")
    path.write_bytes(png)
    print(f"{path.name}: {len(png)} bytes")


if __name__ == "__main__":
    write_png(OUT / "icon-192.png", render(192, maskable=False))
    write_png(OUT / "icon-512.png", render(512, maskable=True))
