"""Builds sigils.png from the game-icons.net glyphs (CC BY 3.0, https://game-icons.net).
Usage: python build_sigils.py <path to the unpacked icons/ffffff/transparent/1x1 folder>
One 128x128 cell per sigil, 8 to a row, in the order of SIGILS (ui.rs relies on it: cell k is
sprite HD + k). Characters and charms are baked with a gradient in their colour and a soft glow;
map-node icons stay white, since they sit on a coloured plate. Also writes Credits.txt."""
import sys
from pathlib import Path
from PIL import Image, ImageFilter, ImageChops

EMBER, TIDE, GALE, STONE, SPARK = 0xff7d4a, 0x4fa8ff, 0x5fe0a8, 0xcbb089, 0xffd84a
COMMON, RARE, LEGEND, WHITE = 0xdcd8f4, 0x7fc0ff, 0xffc46a, None
MEDIOCRE, CURSED = 0x9a9a9a, 0xff5d5d
# (candidate names, first one found wins; colour)
SIGILS = [
    # 0-2 heroes: Ashwarden, Stormreader, Deepforger (front-on heads, so the four read as a set)
    (["dwarf-helmet"], EMBER), (["wizard-face"], TIDE), (["diving-helmet"], STONE),
    # 3-10 enemies
    (["hound"], TIDE), (["imp-laugh"], EMBER), (["spark-spirit"], GALE), (["golem-head"], STONE),
    (["leeching-worm"], TIDE), (["butterfly"], SPARK), (["thorny-vine"], GALE), (["rat"], EMBER),
    # 11-13 elites
    (["troll"], STONE), (["harpy"], GALE), (["ifrit"], EMBER),
    # 14-19 bosses
    (["ant"], EMBER), (["goblin-head"], EMBER), (["octopus"], TIDE), (["mermaid"], TIDE), (["cultist"], SPARK), (["robot-golem"], STONE),
    # 20-26 map nodes: battle, elite, encounter, shop, rest, treasure, boss
    (["crossed-swords"], WHITE), (["horned-skull"], WHITE), (["spell-book"], WHITE), (["coins-pile"], WHITE),
    (["campfire"], WHITE), (["open-treasure-chest"], WHITE), (["crowned-skull"], WHITE),
    # 27 the shopkeeper
    (["hood", "warlock-hood"], SPARK),
    # 28-65 charms, in the order of CHARMS, coloured by rarity
    (["two-coins"], COMMON), (["water-diviner-stick"], COMMON), (["rune-stone"], COMMON), (["meal"], COMMON),
    (["swap-bag"], COMMON), (["sprint"], COMMON), (["thorn-helix"], COMMON), (["toolbox"], COMMON),
    (["treasure-map"], COMMON), (["nautilus-shell"], COMMON), (["crystal-shine"], COMMON), (["burning-embers", "fire", "flame", "fire-gem"], COMMON),
    (["time-dynamite"], COMMON), (["droplets"], COMMON), (["crystal-growth"], COMMON), (["breastplate"], COMMON), (["root-tip"], COMMON),
    (["glowing-hands"], RARE), (["sound-waves"], RARE), (["fangs"], RARE), (["tripwire"], RARE), (["running-shoe"], RARE),
    (["battery-plus"], RARE), (["meditation"], RARE), (["chess-knight"], RARE), (["crystal-cluster"], RARE), (["recycle"], RARE),
    (["dynamite"], RARE), (["hourglass"], RARE), (["knapsack"], RARE), (["chain-lightning"], RARE), (["barrel"], RARE),
    (["gems"], LEGEND), (["shard-sword"], LEGEND), (["smoke-bomb"], LEGEND), (["third-eye"], LEGEND), (["echo-ripples"], LEGEND), (["battle-axe"], LEGEND),
    # 66 the fourth hero, Scaldwright (after the charms, so no older cell moves)
    (["burning-skull"], EMBER),
    # 67-81 the secret endings' enemies, five each (three normals, an elite, the boss):
    # the Drowned Archive, the Furnace Heart, the Silent Engine
    (["eel"], TIDE), (["jellyfish"], TIDE), (["fish-monster"], TIDE), (["angler-fish"], TIDE), (["evil-book"], TIDE),
    (["salamander"], EMBER), (["fluffy-flame"], EMBER), (["flame-spin"], EMBER), (["rock-golem"], EMBER), (["furnace"], EMBER),
    (["spider-bot"], SPARK), (["delivery-drone"], SPARK), (["walking-turret"], SPARK), (["battle-mech"], SPARK), (["clockwork"], SPARK),
    # 82-84 their keys: Brine Lantern, Cinder Seal, Clockwork Eye
    (["lantern-flame"], LEGEND), (["wax-seal"], LEGEND), (["brass-eye"], LEGEND),
    # 85-90 more elites, two per floor: Crystal Warden, The Burrower; Flail Sergeant, Iron Tortoise;
    # The Executioner, The Shade
    (["ice-golem"], STONE), (["mole"], STONE), (["flail"], EMBER), (["tortoise"], STONE),
    (["executioner-hood"], EMBER), (["spectre"], GALE),
    # 91-96 more bosses, two per floor: Old Tusker, Fen Crone; The Broodmother, Storm Roc;
    # The Wyrm Below, The Hive Queen
    (["boar"], STONE), (["witch-face"], TIDE), (["spider-alt"], SPARK), (["griffin-symbol"], GALE),
    (["wyvern"], EMBER), (["bee"], SPARK),
    # 97-100 pacts opened by deeds: Scavenger's Hook, War Drum, Damper Coil, Bone Pact
    (["fishing-hook"], COMMON), (["drum"], RARE), (["magnet"], RARE), (["broken-bone"], LEGEND),
    # 101 the fifth hero, Galvanist, opened by a deed
    (["gas-mask"], SPARK),
    # 102-107 mediocre pacts: Dull Buckler, Rusty Shiv, Chipped Compass, Bent Copper, Stale Crust, Damp Tinder
    (["cracked-shield"], MEDIOCRE), (["rusty-sword"], MEDIOCRE), (["compass"], MEDIOCRE),
    (["wire-coil"], MEDIOCRE), (["bread"], MEDIOCRE), (["candle-flame"], MEDIOCRE),
    # 108-113 cursed bargain pacts: Cracked Aegis, Serrated Toll, Greed's Hook, Frayed Coil, Blood Ledger, Cinder Shackle
    (["broken-shield"], CURSED), (["spiked-mace"], CURSED), (["grab"], CURSED),
    (["tesla-coil"], CURSED), (["chalice-drops"], CURSED), (["manacles"], CURSED),
    # 114-116 pure cursed burden pacts: Lead Weight, Rotting Wick, Faulty Lantern
    (["weight"], CURSED), (["candle-holder"], CURSED), (["broken-bottle"], CURSED),
]
CELL, COLS, GLYPH = 128, 8, 100

def rgb(h):
    return ((h >> 16) & 255, (h >> 8) & 255, h & 255)

def mix(a, b, t):
    return tuple(round(x + (y - x) * t) for x, y in zip(a, b))

root = Path(sys.argv[1])
atlas = Image.new("RGBA", (CELL * COLS, CELL * ((len(SIGILS) + COLS - 1) // COLS)))
credits = {}
for k, (names, colour) in enumerate(SIGILS):
    found = next((p for n in names for p in sorted(root.glob(f"*/{n}.png"))), None)
    if found is None:
        sys.exit(f"no glyph for slot {k}: {names}")
    credits.setdefault(found.parent.name, []).append(found.stem)
    mask = Image.open(found).convert("RGBA").getchannel("A").resize((GLYPH, GLYPH), Image.LANCZOS)
    cell = Image.new("RGBA", (CELL, CELL))
    at = ((CELL - GLYPH) // 2, (CELL - GLYPH) // 2)
    full = Image.new("L", (CELL, CELL))
    full.paste(mask, at)
    if colour is None:
        # white, with a soft dark shadow so it reads on any plate
        shadow = full.filter(ImageFilter.GaussianBlur(3)).point(lambda a: a * 0.6)
        cell.paste((10, 10, 25, 255), (0, 3), shadow)
        cell.paste((255, 255, 255, 255), (0, 0), full)
    else:
        c = rgb(colour)
        # a soft glow in the colour, then the glyph lit from the top
        glow = full.filter(ImageFilter.GaussianBlur(7)).point(lambda a: min(255, a * 0.9))
        cell.paste(c + (255,), (0, 0), glow)
        top, bottom = mix(c, (255, 255, 255), 0.65), mix(c, (20, 20, 40), 0.15)
        grad = Image.new("RGBA", (CELL, CELL))
        for y in range(CELL):
            t = min(1.0, max(0.0, (y - at[1]) / GLYPH))
            grad.paste(mix(top, bottom, t) + (255,), (0, y, CELL, y + 1))
        cell.paste(grad, (0, 0), full)
    atlas.paste(cell, ((k % COLS) * CELL, (k // COLS) * CELL))
out = Path(__file__).with_name("sigils.png")
atlas.save(out, optimize=True)
Path(__file__).with_name("Credits.txt").write_text(
    "Glyphs from game-icons.net, CC BY 3.0 (https://creativecommons.org/licenses/by/3.0/).\n"
    "Recoloured with a gradient and glow for Minebound.\n\n"
    + "".join(f"{a}: {', '.join(v)}\n" for a, v in sorted(credits.items())), encoding="utf-8")
print(out, atlas.size, len(SIGILS), "sigils")
