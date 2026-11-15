"""Bakes assets/sigils/sigils-mono.png from sigils.png: white silhouettes, no colour, no glow.

Run from the repo root after build_sigils.py: python assets/sigils/mono_sigils.py
"""
from PIL import Image

img = Image.open("assets/sigils/sigils.png").convert("RGBA")
# keep only the solid glyph (the glow is the faint alpha), with a short ramp so edges stay smooth
alpha = img.split()[3].point(lambda v: 0 if v < 120 else 255 if v > 180 else (v - 120) * 255 // 60)
out = Image.new("RGBA", img.size, (255, 255, 255, 0))
out.putalpha(alpha)
out.save("assets/sigils/sigils-mono.png", optimize=True)
assert out.size == img.size
print("ok", out.size)
