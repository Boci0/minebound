# Minebound

**A roguelike duel played on minesweeper boards.**

[![Latest release](https://img.shields.io/github/v/release/Boci0/minebound?label=release)](https://github.com/Boci0/minebound/releases/latest)
[![Release build](https://github.com/Boci0/minebound/actions/workflows/release.yml/badge.svg)](https://github.com/Boci0/minebound/actions/workflows/release.yml)
![Platform: Windows](https://img.shields.io/badge/platform-Windows-0078D6)
![Built with Rust](https://img.shields.io/badge/built%20with-Rust-B7410E)
[![License: MIT](https://img.shields.io/badge/license-MIT-green)](LICENSE)

You and your enemy each sweep your own boards, one board per element. Crystals are always visible;
mines are not. Clear every safe tile around a crystal to claim a stack of that board's element, then
spend your stacks on rites that need the right mix of elements. Some rites reach into the enemy's
board, breaking its crystals or burying more mines. Every safe flip in a row builds surge, up to
double damage, and every rite you cast spends a step of it, so each turn asks the same question:
fire now, or flip one more? The enemy plays by exactly the same rules, in full view.

## Contents

- [Features](#features)
- [Installation](#installation)
- [How to play](#how-to-play)
- [Building from source](#building-from-source)
- [Development](#development)
- [Credits](#credits)
- [License](#license)

## Features

- **Four heroes**, each with their own elements, rites and play style.
- **Five elements** and 16 combo rites that need the right mix of stacks.
- **Branching runs** across three floors: battles, elites, encounters, shops, shrines, rest sites,
  treasure and a boss at the end of each floor.
- **38 pacts** in three rarities (common, rare, legendary) that bend the rules of a turn.
- **12 brands**: beat an elite to burn one onto a rite and give it an extra effect.
- **10 blessings**: win a shrine fight to take one. You hold one at a time, and each lifts a single
  stat such as damage, shield, healing or burn.
- **14 enemies**, each breaking one rule: armour, dodging, lifesteal, surge jamming, stack theft
  and more.
- **Seeded runs**: replay any run exactly by its seed.

## Installation

**Requirements:** Windows 10 or 11, 64-bit.

1. Download `Minebound-setup.exe` from the
   [latest release](https://github.com/Boci0/minebound/releases/latest).
2. Run the installer. No administrator rights are needed.
3. Launch Minebound from the Start menu, or from the desktop shortcut if you selected one.

> [!NOTE]
> The installer is not code-signed, so Windows SmartScreen may show "Windows protected your PC".
> Select **More info**, then **Run anyway**.

**Updating:** download the latest `Minebound-setup.exe` and run it. It upgrades the installed game in
place.

**Uninstalling:** open **Settings > Apps > Installed apps**, find Minebound and select
**Uninstall**.

## How to play

### Controls

| Action | Input |
| --- | --- |
| Reveal a tile | Left click |
| Flag a suspected mine | Right click |
| Switch board | Click a board tab (free, any time in your turn) |
| Socket a gem | Drag it from your stacks into a rite's matching socket; the rite fires once every socket is full |
| Pick a rite to fill | Right click outside the board steps down the rite list; then click a gem to send it there |
| Take a gem back | Click the filled socket |
| End your turn (unspent stacks are kept) | End turn, or Space |
| Explain a rule word | Hover any word that floats |
| Abandon the run | Retreat (top right, outside battle) |
| Give up a battle (ends the run) | Surrender (bottom left, in battle) |
| Sound on / off | M |
| Next song | N |
| Volume settings | Esc, or the gear icon |

### Rules

- A number on a tile is how many mines touch it. A safe tile touching no mines opens its neighbours.
- Your first flip on a fresh board is never a mine.
- The number under a crystal is how many hidden tiles still surround it; the small number in its
  corner is how many of them are mines. It pays out once every safe one is revealed.
- A mine costs a little HP, gives no stack, ends your turn, wipes your surge and breaks the crystals
  next to it.
- Boards keep their state while you are on another one. A board with no crystal left to claim is
  dealt again, and all boards reset each wave.
- You get 6 reveals per turn, and never fewer than 5 whatever the penalties (lesser enemies get
  fewer). There is no limit on the stacks you can hold.
- Stacks are spent only through rites. Gems no rite can use yet wait in your stacks.
- A rite deals power × stacks spent, plus its bonus, multiplied by surge (+20% per safe flip in a
  row this turn, up to +100%). Each rite cast spends one step of surge. Casting does not end your
  turn.
- Shield soaks damage first; half of it wears off at the start of its owner's next turn.
- After turn 12, every rite gains +2 damage per turn, so no fight lasts forever.
- Beating an elite earns a brand. Burnt onto one of your rites, it adds its effect at no extra gem
  cost. Brands do not grow when the rite is upgraded, and a rite holds one brand at a time.
- Every floor has at least one shrine. Win its fight to choose a blessing; taking a new one replaces
  the one you hold.

Your pacts sit at the top left of the battle screen and flash each time they take effect. The column
before each boss is all rest sites.

## Building from source

Minebound builds on Windows only: its audio library keeps just the Windows backend. You need
[Git](https://git-scm.com) and [Rust](https://rustup.rs); accept the Visual Studio C++ build tools
when the Rust installer offers them.

```
git clone https://github.com/Boci0/minebound.git
cd minebound
cargo run --release
```

The first build takes a few minutes. To replay a run, pass its seed: `cargo run --release -- 42`.
To update, close the game, run `git pull`, then `cargo run --release` again.

### Other platforms

The game itself (macroquad) is cross-platform; only the audio is Windows-specific. To port it,
restore your platform's backend from upstream [quad-snd 0.2.8](https://crates.io/crates/quad-snd/0.2.8)
into `vendor/quad-snd`, and add its `cfg` line and dependencies back. Each backend is a single
self-contained file; `vendor/quad-snd/README.md` lists what this copy changes. Pull requests are
welcome.

## Development

### Tests

```
cargo test -- --nocapture
```

The suite includes a headless simulation that plays 900 full runs, checking that every fight ends
and that the heroes stay roughly balanced.

### Releasing

Push a version tag. GitHub Actions builds `Minebound-setup.exe` and publishes it as a release:

```
git tag v1.0.0
git push origin v1.0.0
```

The installer is defined in `installer/minebound.iss`. Never change its `AppId`: it is how a new
installer finds and upgrades an existing installation.

## Credits

All assets are CC0 (public domain), except the character and icon glyphs (CC BY 3.0) and the fonts
(SIL Open Font License).

| Asset | Source |
| --- | --- |
| Fonts | [Pirata One](https://fonts.google.com/specimen/Pirata+One) by Rodrigo Fuenzalida and Nicolas Massi; [IBM Plex Mono](https://github.com/IBM/plex) by IBM, both under the [SIL OFL 1.1](https://openfontlicense.org) |
| Characters and icons | Glyphs from [game-icons.net](https://game-icons.net) by Lorc, Delapouite, Skoll, darkZaitzev, Kier Heyl, Faithtoken, PriorBlue and Willdabeast, under [CC BY 3.0](https://creativecommons.org/licenses/by/3.0/), baked into white silhouettes (per-icon list in `assets/sigils/Credits.txt`) |
| Window icon | Tiny Dungeon by [Kenney](https://kenney.nl) |
| Rite effects | [Particle Pack](https://kenney.nl/assets/particle-pack) by Kenney |
| Sound effects | RPG Audio, Impact Sounds, Interface Sounds and Digital SFX by [Kenney](https://kenney.nl); [Horror Hit Soundpack 1](https://opengameart.org/content/horror-hit-soundpack-1) by psychhead_ |
| Music | [Stranger Than Fiction](https://freemusicarchive.org/music/holiznacc0/stranger-than-fiction-halloween-sci-fi), [Gamer Soundtrack: Haunted Mansion](https://freemusicarchive.org/music/holiznacc0/gamer-soundtrack-haunted-mansion) and [Horseless Headman](https://freemusicarchive.org/music/holiznacc0/horseless-headman-halloween-beats) by HoliznaCC0 |

Licence files for each asset pack are in `assets/`. Audio playback uses a patched copy of
[quad-snd](https://github.com/not-fl3/quad-snd) in `vendor/quad-snd` (MIT / Apache-2.0), cut down
to its Windows backend, and drawing uses a patched copy of [macroquad](https://github.com/not-fl3/macroquad)
in `vendor/macroquad` (MIT / Apache-2.0) that sends its font atlases to the GPU once a frame.

## License

The game's code is released under the [MIT License](LICENSE). The assets keep their own licences
(all CC0), listed under [Credits](#credits).
