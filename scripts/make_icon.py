"""Draw the Serika Code app icon: a violet squircle with a terminal chevron and cursor.

    python -I scripts/make_icon.py

Writes qt/serika-code/icons/app-logo.svg (used in the app), PNGs from 16 to 1024 px and a
multi-size .ico into qt/serika-code/app/. Rerun it after changing the geometry below; the SVG
and the bitmaps share the same numbers.
"""

from pathlib import Path

from PIL import Image, ImageChops, ImageDraw, ImageFilter

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "qt" / "serika-code" / "app"
SVG = ROOT / "qt" / "serika-code" / "icons" / "app-logo.svg"

# Geometry on a 1024 canvas.
SIZE = 1024
INSET = 64                      # margin around the tile, as Windows and macOS icons expect
TOP = (167, 139, 250)           # #a78bfa
MID = (124, 58, 237)            # #7c3aed
BOTTOM = (76, 29, 149)          # #4c1d95
CHEVRON = [(318, 352), (512, 512), (318, 672)]
STROKE = 104
CURSOR = (572, 620, 762, 708)   # left, top, right, bottom
CURSOR_COLOR = (233, 225, 255)


def squircle(size: int, inset: int, exponent: float = 5.0, steps: int = 720) -> list[tuple[float, float]]:
    import math

    half = (size - 2 * inset) / 2
    cx = cy = size / 2
    points = []
    for i in range(steps):
        t = 2 * math.pi * i / steps
        c, s = math.cos(t), math.sin(t)
        x = cx + half * math.copysign(abs(c) ** (2 / exponent), c)
        y = cy + half * math.copysign(abs(s) ** (2 / exponent), s)
        points.append((x, y))
    return points


def gradient(size: int) -> Image.Image:
    """Diagonal three-stop gradient, light at the top left."""
    img = Image.new("RGB", (size, size))
    px = img.load()
    for y in range(size):
        for x in range(size):
            t = (x * 0.35 + y * 0.65) / size
            if t < 0.5:
                a, b, k = TOP, MID, t / 0.5
            else:
                a, b, k = MID, BOTTOM, (t - 0.5) / 0.5
            px[x, y] = tuple(round(a[i] + (b[i] - a[i]) * k) for i in range(3))
    return img


def draw_master() -> Image.Image:
    scale = 2  # supersample, then shrink for smooth edges
    big = SIZE * scale
    shape = [(x * scale, y * scale) for x, y in squircle(SIZE, INSET)]

    mask = Image.new("L", (big, big), 0)
    ImageDraw.Draw(mask).polygon(shape, fill=255)

    tile = gradient(big // 4).resize((big, big), Image.BICUBIC).convert("RGBA")
    # Soft highlight along the top edge.
    glow = Image.new("L", (big, big), 0)
    ImageDraw.Draw(glow).ellipse((-big * 0.2, -big * 0.75, big * 1.2, big * 0.42), fill=48)
    glow = glow.filter(ImageFilter.GaussianBlur(big * 0.09))
    white = Image.new("RGBA", (big, big), (255, 255, 255, 0))
    white.putalpha(ImageChops.multiply(glow, mask))
    tile = Image.alpha_composite(tile, white)

    marks = Image.new("RGBA", (big, big), (0, 0, 0, 0))
    draw = ImageDraw.Draw(marks)
    pts = [(x * scale, y * scale) for x, y in CHEVRON]
    width = STROKE * scale
    draw.line(pts, fill=(255, 255, 255, 255), width=width, joint="curve")
    r = width / 2
    for x, y in (pts[0], pts[-1]):
        draw.ellipse((x - r, y - r, x + r, y + r), fill=(255, 255, 255, 255))
    l, t, rr, b = (v * scale for v in CURSOR)
    draw.rounded_rectangle((l, t, rr, b), radius=(b - t) / 2, fill=CURSOR_COLOR + (255,))

    # A faint drop shadow under the marks keeps them crisp on the light corner.
    shadow = marks.split()[3].filter(ImageFilter.GaussianBlur(big * 0.012))
    shade = Image.new("RGBA", (big, big), (40, 10, 90, 0))
    shade.putalpha(shadow.point(lambda v: v * 0.35))
    tile = Image.alpha_composite(tile, shade)
    tile = Image.alpha_composite(tile, marks)

    tile.putalpha(mask)
    return tile.resize((SIZE, SIZE), Image.LANCZOS)


def write_svg() -> None:
    path = " ".join(
        ("M" if i == 0 else "L") + f"{x:.1f} {y:.1f}" for i, (x, y) in enumerate(squircle(SIZE, INSET, steps=180))
    ) + " Z"
    chevron = " ".join(("M" if i == 0 else "L") + f"{x} {y}" for i, (x, y) in enumerate(CHEVRON))
    l, t, r, b = CURSOR
    hexes = ["#%02x%02x%02x" % c for c in (TOP, MID, BOTTOM)]
    SVG.write_text(
        f"""<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {SIZE} {SIZE}">
<defs><linearGradient id="g" x1="0.2" y1="0" x2="0.8" y2="1">
<stop offset="0" stop-color="{hexes[0]}"/><stop offset="0.5" stop-color="{hexes[1]}"/><stop offset="1" stop-color="{hexes[2]}"/>
</linearGradient></defs>
<path d="{path}" fill="url(#g)"/>
<path d="{chevron}" fill="none" stroke="#ffffff" stroke-width="{STROKE}" stroke-linecap="round" stroke-linejoin="round"/>
<rect x="{l}" y="{t}" width="{r - l}" height="{b - t}" rx="{(b - t) / 2}" fill="#e9e1ff"/>
</svg>
""",
        encoding="utf-8",
    )


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    master = draw_master()
    sizes = [16, 20, 24, 32, 40, 48, 64, 128, 256, 512, 1024]
    for size in sizes:
        img = master.resize((size, size), Image.LANCZOS)
        if size <= 32:
            # Small sizes: a touch of sharpening so the chevron stays readable.
            img = img.filter(ImageFilter.UnsharpMask(radius=0.6, percent=60, threshold=0))
        img.save(OUT / f"serika-code-{size}.png")
    master.save(OUT / "serika-code.ico", sizes=[(s, s) for s in (16, 20, 24, 32, 40, 48, 64, 128, 256)])
    write_svg()
    print(f"wrote {OUT} and {SVG}")


if __name__ == "__main__":
    main()
