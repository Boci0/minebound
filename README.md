# Minebound

A roguelike duel played on two minesweeper boards.

You and your enemy each sweep your own boards, one board per element. Crystals are always visible;
mines are not. Clear every safe tile around a crystal and you claim a stack of that board's element.
Stacks are ammunition: spend them on skills, from a plain Strike to combos that need the right mix of
elements. Every safe flip in a row builds surge, up to double damage, so each turn asks the same
question: fire now, or flip one more? The enemy plays by exactly the same rules, in full view.

Between fights you pick a route through a branching map of battles, elites, encounters, shops, rests
and treasure, collecting charms that bend the rules, across three floors and three bosses.

## Get the game

You need [Git](https://git-scm.com) and [Rust](https://rustup.rs). On Windows, the Rust installer
also asks for the Visual Studio C++ build tools; say yes. Then:

```
git clone https://github.com/Boci0/minebound.git
cd minebound
cargo build --release
```

The first build takes a few minutes.

## Run

```
cargo run --release
```

`cargo run --release -- 42` replays the run with seed 42.

### Update

Close the game first, then, in the game's folder:

```
git pull
cargo build --release
```

### Desktop shortcut (Windows)

Build once with `cargo build --release`, then run this in PowerShell from the game's folder:

```powershell
$s = (New-Object -ComObject WScript.Shell).CreateShortcut("$([Environment]::GetFolderPath('Desktop'))\Minebound.lnk"); $s.TargetPath = "$PWD\target\release\minebound.exe"; $s.WorkingDirectory = "$PWD"; $s.Save()
```

The shortcut starts the game from its folder, so the music is found. It keeps working after an
update, since the update rebuilds the same file.

## Controls

- Left click a hidden tile: reveal it. Right click: flag it as a suspected mine.
- Click a board tab: swap to that element's board (free, any time in your turn).
- Drag a gem from your stacks into a skill's matching socket; the skill fires when every socket is
  full. Click a filled socket to take the gem back. Drop a gem on the enemy (or click it) to Strike.
- Click a lit skill to cast it at once. Unleash all: fire everything.
- End turn: keep your remaining stacks for later.
- Retreat (top right, outside battle): abandon the run and go back to the title.
- M: sound on or off. N: next song. Esc or the gear: volume settings.

## Rules in short

- A number on a tile is how many mines touch it. A safe tile touching no mines opens its neighbours.
- Your first flip on a fresh board is never a mine.
- The number under a crystal is how many hidden tiles still surround it. Mines around it stay hidden;
  it pays out once every safe one is revealed.
- A mine costs a little HP, gives no stack, ends your turn, wipes your surge and breaks the crystals
  next to it.
- Boards keep their state while you are on another one. A board with no crystal left to claim is
  dealt again, and all boards reset each wave.
- 6 reveals per turn (lesser enemies get fewer). There is no limit on the stacks you can hold.
- A skill deals power x stacks spent, plus its bonus, multiplied by surge (+20% per safe flip in a
  row this turn, up to +100%). Casting does not end your turn.
- After turn 7 every skill gains +2 damage per turn, so no fight lasts forever.

Every enemy bends one rule (armour, dodging your first skill, jamming your surge, stealing stacks),
and charms reward different ways of playing a turn, so no two fights or runs ask for the same plan.
Charms come in three rarities: common, rare and legendary. Your charms sit along the top of the
battle screen and flash each time they take effect. The column before each boss is all rest sites.

## Credits

Fonts: Kenney Mini by Kenney (kenney.nl), CC0; Pixel Operator by Jayvee Enaguas, CC0.
Pixel art: Tiny Dungeon by Kenney (kenney.nl), public domain (CC0). See `assets/tiny-dungeon/License.txt`.
Sound effects: RPG Audio, Impact Sounds and Interface Sounds by Kenney (kenney.nl), CC0.
Music: "Winter Lofi" by HoliznaCC0 (freemusicarchive.org/music/holiznacc0/winter-lofi), CC0.
The music is read from `assets/music` next to where the game is run; without that folder the game is simply silent.

## Tests

```
cargo test -- --nocapture
```

Includes a headless simulation that plays 900 full runs to check that every fight ends and the
heroes stay roughly balanced.
