"""Builds particles.png from Kenney's Particle Pack (CC0, https://kenney.nl/assets/particle-pack).
Usage: python build_fx.py <path to the pack's "PNG (Transparent)" folder>
One 128x128 cell per particle, 8 to a row. Prints the Rust constants for ui.rs: each effect is
a few particles drawn together, growing, turning and fading (see `anims` in ui.rs)."""
import sys
from pathlib import Path
from PIL import Image

EFFECTS = {
    "FX_EMBER": ["scorch_02", "fire_01", "star_09"],
    "FX_TIDE": ["circle_03", "light_02", "circle_05"],
    "FX_GALE": ["twirl_01", "twirl_02", "twirl_03"],
    "FX_STONE": ["dirt_02", "smoke_03", "circle_02"],
    "FX_SPARK": ["spark_02", "spark_01", "star_06"],
    "FX_HEAL": ["magic_02", "star_06", "circle_05"],
    "FX_FLAME": ["muzzle_02", "fire_02", "flame_05"],
    "FX_SLASH": ["slash_02", "scratch_01"],
}
CELL, COLS = 128, 8
root = Path(sys.argv[1])
names = list(dict.fromkeys(n for v in EFFECTS.values() for n in v))
atlas = Image.new("RGBA", (CELL * COLS, CELL * ((len(names) + COLS - 1) // COLS)))
for i, n in enumerate(names):
    atlas.paste(Image.open(root / f"{n}.png").convert("RGBA").resize((CELL, CELL), Image.LANCZOS), ((i % COLS) * CELL, (i // COLS) * CELL))
atlas.save(Path(__file__).with_name("particles.png"), optimize=True)
for k, v in EFFECTS.items():
    cells = ", ".join(f"({names.index(n) % COLS * CELL}.0, {names.index(n) // COLS * CELL}.0, {CELL}.0)" for n in v)
    print(f"const {k}: &[(f32, f32, f32)] = &[{cells}];")
