#!/usr/bin/env python3
"""Regenerate the Fabric mod icon (mod/src/main/resources/assets/arsex/icon.png).

Hand-authored pixel drawing code, not an AI image — the same rule the
prototype background follows. Strictly greyscale: every colour has R==G==B.

A katana seen edge-on, running corner to corner: long blade to the upper
right, wrapped tsuka (handle) to the lower left, separated by a square tsuba
(guard). The single accent is the registration tick the launcher UI uses to
mark "owned" items.

Run from anywhere; paths resolve from the repo root:

    py tools/make-mod-icon.py        (Windows)
    python3 tools/make-mod-icon.py   (elsewhere)
"""

import os

from PIL import Image, ImageDraw

# Greyscale palette only — mono-lint's rule, applied to images.
INK = (10, 10, 10, 255)          # near-black ground
BLADE = (245, 245, 245, 255)     # #F5F5F5 cutting edge
BLADE_SHADE = (140, 140, 140, 255)  # #8C8C8C mune (spine) shading
GUARD = (200, 200, 200, 255)
HANDLE = (38, 38, 38, 255)
WRAP = (120, 120, 120, 255)
TICK = (245, 245, 245, 255)

SS = 8          # supersample factor for crisp antialiased diagonals
SIZE = 128      # final icon size (Fabric recommends 128)


def draw_katana(size: int) -> Image.Image:
    img = Image.new("RGBA", (size, size), INK)
    d = ImageDraw.Draw(img)

    s = size / 128.0  # everything below is authored against a 128 grid

    # --- geometry -----------------------------------------------------------
    # The sword runs from (18,110) at the tsuka end to (118,10) at the kissaki.
    # It is a thick diagonal band; the tsuba sits ~30% along it.
    x0, y0 = 18 * s, 110 * s
    x1, y1 = 118 * s, 10 * s
    band = 10 * s          # blade width
    guard_at = 0.30        # fraction along the sword where the tsuba sits
    gx = x0 + (x1 - x0) * guard_at
    gy = y0 + (y1 - y0) * guard_at

    def along(f: float, off: float):
        """Point f along the sword axis, offset perpendicular by off."""
        dx, dy = x1 - x0, y1 - y0
        length = (dx * dx + dy * dy) ** 0.5
        nx, ny = -dy / length, dx / length  # unit normal
        return (x0 + dx * f + nx * off, y0 + dy * f + ny * off)

    # --- handle (tsuka): dark band with cross wraps -------------------------
    d.polygon(
        [along(0.0, -band / 2), along(guard_at, -band / 2),
         along(guard_at, band / 2), along(0.0, band / 2)],
        fill=HANDLE,
    )
    for f in (0.06, 0.13, 0.20):
        d.polygon(
            [along(f, -band / 2), along(f + 0.025, -band / 2 + 3 * s),
             along(f + 0.05, band / 2), along(f + 0.025, band / 2 - 3 * s)],
            fill=WRAP,
        )

    # --- tsuba (guard): square plate, axis-aligned --------------------------
    g = 9 * s
    d.rectangle([gx - g, gy - g, gx + g, gy + g], fill=GUARD)
    d.rectangle([gx - 4 * s, gy - 4 * s, gx + 4 * s, gy + 4 * s], fill=INK)

    # --- blade ---------------------------------------------------------------
    # Main band in the lighter grey, then a #F5F5F5 edge along the upper side
    # (the ha / cutting edge faces up-right), and a kissaki tip.
    d.polygon(
        [along(guard_at, -band / 2), along(1.0, -band / 2),
         along(1.0, band / 2), along(guard_at, band / 2)],
        fill=BLADE_SHADE,
    )
    d.polygon(
        [along(guard_at, -band / 2 - 2 * s), along(1.0, -band / 2 - 2 * s),
         along(1.0, -band / 2), along(guard_at, -band / 2)],
        fill=BLADE,
    )
    # Bosahi-like hairline down the centre of the blade.
    for f in [i / 40 for i in range(40)]:
        a, b = along(guard_at + f * (1 - guard_at) * 0.98, 0.5 * s), \
               along(guard_at + (f + 0.024) * (1 - guard_at) * 0.98, 0.5 * s)
        d.line([a, b], fill=INK, width=max(1, int(1.2 * s)))

    # --- registration tick, top-left corner (launcher "owned" mark) ----------
    d.rectangle([8 * s, 8 * s, 16 * s, 9 * s], fill=TICK)
    d.rectangle([8 * s, 8 * s, 9 * s, 16 * s], fill=TICK)

    return img


def main() -> None:
    here = os.path.dirname(os.path.abspath(__file__))
    out = os.path.join(here, "..", "mod", "src", "main", "resources",
                       "assets", "arsex", "icon.png")
    out = os.path.normpath(out)
    os.makedirs(os.path.dirname(out), exist_ok=True)
    img = draw_katana(SIZE * SS)
    img = img.resize((SIZE, SIZE), Image.LANCZOS)
    img.save(out, "PNG", optimize=True)

    # Guard: the icon must stay greyscale. Refuse to write anything else.
    for px in img.getdata():
        r, g, b = px[0], px[1], px[2]
        assert r == g == b, f"colour pixel {px} leaked into the mod icon"
    print(f"wrote {out} ({SIZE}x{SIZE}, greyscale-verified)")


if __name__ == "__main__":
    main()
