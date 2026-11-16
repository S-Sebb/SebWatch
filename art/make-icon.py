#!/usr/bin/env python3
"""Builds SebWatch's hicolor launcher icons in art/icons/ from art/icon.png (needs Pillow).

The approved art is a full square; each size is that square with rounded corners (like SebType's PC icons),
drawn from the art at 4x and scaled down so small sizes stay crisp.
"""

from pathlib import Path

from PIL import Image, ImageDraw

root = Path(__file__).resolve().parent
SIZES = [16, 22, 24, 32, 48, 64, 128, 256, 512]

art = Image.open(root / "icon.png").convert("RGBA")
for px in SIZES:
    side = px * 4
    mask = Image.new("L", (side, side))
    ImageDraw.Draw(mask).rounded_rectangle((0, 0, side - 1, side - 1), side * 0.18, 255)
    icon = Image.new("RGBA", (side, side))
    icon.paste(art.resize((side, side), Image.LANCZOS), mask=mask)
    out = root / f"icons/hicolor/{px}x{px}/apps/sebwatch.png"
    out.parent.mkdir(parents=True, exist_ok=True)
    icon.resize((px, px), Image.LANCZOS).save(out, optimize=True)
