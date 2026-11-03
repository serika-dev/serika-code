"""Draw the pictures for the Windows installer from the app icon.

    python -I scripts/make_installer_art.py <out-dir>

Writes wizard-large-100.bmp / -200.bmp (the side panel on the first and last page) and
wizard-small-100.bmp / -200.bmp (the corner of every other page). Inno Setup picks the size
that fits the screen's scaling. The bitmaps are made at build time so the repository only
keeps the icon they come from.
"""

import sys
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter, ImageFont

ROOT = Path(__file__).resolve().parent.parent
ICON = ROOT / "qt" / "serika-code" / "app" / "serika-code-512.png"
FONT = ROOT / "qt" / "serika-code" / "fonts" / "Outfit.ttf"

TOP = (24, 20, 38)       # night violet
BOTTOM = (10, 10, 14)    # the app's dark background
GLOW = (124, 58, 237)    # #7c3aed, the accent


def large(scale: int) -> Image.Image:
    w, h = 164 * scale, 314 * scale
    img = Image.new("RGB", (w, h))
    draw = ImageDraw.Draw(img)
    for y in range(h):
        t = y / (h - 1)
        draw.line([(0, y), (w, y)], fill=tuple(round(a + (b - a) * t) for a, b in zip(TOP, BOTTOM)))

    # A soft accent glow behind the icon.
    glow = Image.new("L", (w, h), 0)
    gd = ImageDraw.Draw(glow)
    cx, cy, r = w // 2, int(h * 0.36), int(w * 0.42)
    gd.ellipse([cx - r, cy - r, cx + r, cy + r], fill=150)
    glow = glow.filter(ImageFilter.GaussianBlur(28 * scale))
    img = Image.composite(Image.new("RGB", (w, h), GLOW), img, glow)

    icon = Image.open(ICON).convert("RGBA")
    size = int(w * 0.5)
    icon = icon.resize((size, size), Image.LANCZOS)
    img.paste(icon, (cx - size // 2, cy - size // 2), icon)

    # The name under it
    draw = ImageDraw.Draw(img)
    font = ImageFont.truetype(str(FONT), 19 * scale)
    try:
        font.set_variation_by_axes([600])
    except Exception:
        pass
    text = "Serika Code"
    tw = draw.textlength(text, font=font)
    draw.text(((w - tw) / 2, cy + size // 2 + 18 * scale), text, font=font, fill=(244, 244, 247))
    return img


def small(scale: int) -> Image.Image:
    s = 55 * scale
    img = Image.new("RGB", (s, s), (255, 255, 255))
    icon = Image.open(ICON).convert("RGBA").resize((s, s), Image.LANCZOS)
    img.paste(icon, (0, 0), icon)
    return img


def main() -> None:
    out = Path(sys.argv[1] if len(sys.argv) > 1 else ROOT / "dist" / "installer-art")
    out.mkdir(parents=True, exist_ok=True)
    for scale, name in ((1, "100"), (2, "200")):
        large(scale).save(out / f"wizard-large-{name}.bmp")
        small(scale).save(out / f"wizard-small-{name}.bmp")
    print(out)


if __name__ == "__main__":
    main()
