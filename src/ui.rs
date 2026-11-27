//! Immediate-mode screens: each function draws one screen and reports what was clicked.

use crate::{
    battle::{Battle, Event, Fighter, CAVE_DMG, CLAIM_GEMS, PRESSURE_TURN},
    board::{Board, CAVE, N, W},
    data::*,
    run::*,
    save::{is_open, Save, Unlock, CLEARED, DEEDS},
    sfx::{self, Sfx},
};
use macroquad::audio::{play_sound, PlaySoundParams, Sound};
use macroquad::rand::gen_range;
use macroquad::prelude::*;
// named imports win over the prelude's glob: debug builds can be driven from a file
use crate::ctl::{is_key_pressed, is_mouse_button_down, is_mouse_button_pressed};

const fn rgb(hex: u32) -> Color {
    Color::new(((hex >> 16) & 255) as f32 / 255.0, ((hex >> 8) & 255) as f32 / 255.0, (hex & 255) as f32 / 255.0, 1.0)
}

// palette: black and white only. Meaning comes from shape, weight and brightness, never hue;
// the old colour names stay so every screen keeps its roles (GOLD_C = something precious, BAD = harm)
pub const BG: Color = rgb(0x0a0a0a);
const OUT: Color = rgb(0x000000);
const CARD: Color = rgb(0x0e0e0e);
const CARD_HI: Color = rgb(0x1e1e1e);
const WELL: Color = rgb(0x151515);
/// hairline outlines and dividers
const FAINT: Color = rgb(0x363636);
const INK: Color = rgb(0xf2f2f2);
const MUTED: Color = rgb(0x8a8a8a);
const ACCENT: Color = rgb(0xc4c4c4);
const GOLD_C: Color = rgb(0xffffff);
const GOOD: Color = rgb(0xf2f2f2);
const BAD: Color = rgb(0xd6d6d6);
const HEX: Color = rgb(0xa0a0a0);

pub const VIEW: Vec2 = vec2(1280.0, 720.0);

/// Elements differ only by their alchemical sign (`gem`); a dimmer one would read as disabled.
fn el_color(_: Element) -> Color {
    INK
}

fn mix(a: Color, b: Color, t: f32) -> Color {
    Color::new(a.r + (b.r - a.r) * t, a.g + (b.g - a.g) * t, a.b + (b.b - a.b) * t, 1.0)
}

fn fade(c: Color, a: f32) -> Color {
    Color::new(c.r, c.g, c.b, a.clamp(0.0, 1.0))
}

/// One square of debris, spark or crystal shard.
#[derive(Clone, Copy)]
struct Part {
    pos: Vec2,
    vel: Vec2,
    born: f64,
    life: f32,
    size: f32,
    color: Color,
    /// downward pull; negative makes it rise
    grav: f32,
    /// if set, the particle flies here instead of falling
    home: Option<Vec2>,
}

/// A skill in flight.
#[derive(Clone, Copy)]
struct Shot {
    from: usize,
    t0: f64,
    /// (index in `COMBOS`, level)
    skill: (usize, usize),
    /// the effect of the rite's first brand, if it has one (for the visuals)
    brand: Option<Effect>,
    /// false for a rite that deals no damage: it rises from the caster and settles back on them
    hits: bool,
    /// dodged: it flies past and fizzles, with no impact, shake or effects
    miss: bool,
    /// mastered, and its mastery takes effect: it gets its own sign and shout
    master: bool,
}

/// Shot timing, in units of `SHOT_TIME`: upgraded skills charge up first, then fire one bolt per level.
fn charge(lvl: usize) -> f32 {
    if lvl >= 2 { 2.0 } else { 0.0 }
}

fn volley(lvl: usize, k: usize) -> f32 {
    charge(lvl) + k as f32 * 0.6
}

/// When the last bolt of `sh` hits.
fn lands_at(sh: &Shot) -> f64 {
    let lvl = sh.skill.1;
    sh.t0 + ((volley(lvl, lvl - 1) + 1.0) * SHOT_TIME) as f64
}

/// An expanding geometric ring or ripple (circle, diamond, hex, square, or exact rect border).
#[derive(Clone, Copy)]
struct Ring {
    at: Vec2,
    size: Vec2,
    r0: f32,
    r1: f32,
    /// 0 = circle, 4 = diamond, 6 = hexagon, 254 = rect border echo (`size` + `rad`), 255 = axis-aligned square
    sides: u8,
    rot: f32,
    width: f32,
    color: Color,
    t0: f64,
    dur: f32,
}

/// An alchemical gem glyph flying in an arc from a mined crystal to a stack chip.
#[derive(Clone, Copy)]
struct FlyingGem {
    element: Element,
    from: Vec2,
    to: Vec2,
    t0: f64,
    dur: f32,
}

/// Visual actions queued to fire at the exact moment a bolt lands.
#[derive(Clone, Copy)]
enum DelayedFx {
    /// defender sidesteps a miss or graze
    Dodge { side: usize, strength: f32 },
    /// shield soaks damage: hex deflection ripple
    ShieldHit { side: usize },
    /// shield completely shattered by a hit
    ShieldBreak { side: usize },
    /// enemy loaded aura shatters outward on break
    BreakBurst,
    /// brittle shield shatters back onto the attacker
    BrittleBack { from: usize },
    /// pierce slashes straight through a shield
    PierceSlash { side: usize },
    /// elemental reaction burst at impact
    Reaction { from: usize, element: Element },
    /// thorns fire sharp sparks back onto the attacker
    ThornsBack { from: usize },
    /// graze glancing sparks
    GrazeSparks { side: usize },
    /// burn tick flame burst
    BurnTick { side: usize },
}

/// Per-side array of `N` board-tile slots (`N = 36 > 32`, so `Default` is implemented by hand).
#[derive(Clone, Copy)]
struct TileGrid<T>([[T; N]; 2]);

impl<T: Copy + Default> Default for TileGrid<T> {
    fn default() -> Self {
        Self([[T::default(); N]; 2])
    }
}

impl<T> std::ops::Index<usize> for TileGrid<T> {
    type Output = [T; N];
    fn index(&self, s: usize) -> &Self::Output {
        &self.0[s]
    }
}

impl<T> std::ops::IndexMut<usize> for TileGrid<T> {
    fn index_mut(&mut self, s: usize) -> &mut Self::Output {
        &mut self.0[s]
    }
}

/// Purely visual memory: what the screen showed last frame, so changes can be animated.
#[derive(Default)]
struct Fx {
    live: bool,
    wave: usize,
    hp: [i32; 2],
    /// shield as last seen, so a hit the shield soaks still shows a number
    shield: [i32; 2],
    /// trailing HP bar that drains after the real one
    ghost: [f32; 2],
    hit: [f64; 2],
    stacks: [usize; 2],
    stack_t: [[f64; 16]; 2],
    /// when each revealed tile or claimed crystal first appeared: (side, board, tile)
    pops: std::collections::HashMap<(usize, usize, usize), f64>,
    /// floating numbers: (where, text, colour, start, size); a start in the future waits for its hit
    floats: Vec<(Vec2, String, Color, f64, f32)>,
    shots: Vec<Shot>,
    fade: f64,
    /// game time when the current screen opened, for staggered entrances
    screen_t: f64,
    /// the win or lose jingle has played for this battle
    ended: bool,
    parts: Vec<Part>,
    /// expanding crisp geometric shockwaves and ripples
    rings: Vec<Ring>,
    /// flying alchemical gems arcing from mined crystals to gem stack chips
    flying_gems: Vec<FlyingGem>,
    /// delayed visual bursts tied to bolt landing times
    pending_fx: Vec<(f64, DelayedFx)>,
    /// whole-screen shake: (start, strength in pixels)
    quake: (f64, f32),
    /// whole-screen colour flash: (colour, start)
    flash: Option<(Color, f64)>,
    /// names swept across the board, in order: (text, colour, side it enters from, start, skill level; 0 for a turn).
    /// A start in the future waits its turn; the latest one started is shown.
    banner: Vec<(&'static str, Color, usize, f64, usize)>,
    /// game time until which the current action is still playing out; the next one waits for it
    busy: f64,
    /// when your turn's banner is in and your clicks count again
    turn_at: f64,
    /// sounds waiting for their moment: (game time, sound)
    later: Vec<(f64, Sfx)>,
    /// ground just caved in: when it goes off
    boom: Option<f64>,
    /// a killing blow on its way: (game time it lands, side that falls)
    fell: Option<(f64, usize)>,
    /// the enemy a wave change replaced, shown until its replacement walks in
    fallen: Option<Fighter>,
    /// when the replacement's name and bar take over from the fallen enemy's (game time)
    takeover: Option<f64>,
    /// HP each bar shows, and the change it is waiting to show: (game time, HP)
    bar: [i32; 2],
    /// HP bar values still waiting for the hit that set them to land, in order
    due: [Vec<(f64, i32)>; 2],
    /// visually displayed shield per side, and pending shield transitions waiting for casts/hits to land
    vis_shield: [i32; 2],
    due_shield: [Vec<(f64, i32)>; 2],
    /// white hit-flash and green heal-flash timers on each HP bar
    bar_flash: [f64; 2],
    heal_flash: [f64; 2],
    /// shield lock-in flash timestamp per side
    shield_gain: [f64; 2],
    /// a felled enemy's sigil flaring out: (sprite, start)
    vanish: Option<(usize, f64)>,
    /// whose turn the screen last showed
    turn: Option<usize>,
    /// a quick zoom-in on a heavy hit: start
    punch: f64,
    /// a hit of `HUGE` or more on its way: (game time it lands, side hit)
    huge: Option<(f64, usize)>,
    /// impact frames for that hit once it lands: (real time they started, side hit)
    impact: Option<(f64, usize)>,
    /// the shop ware being looked at
    pick: Option<usize>,
    /// when the selected shop ware last changed
    sel_t: f64,
    /// animated purse counter in the shop
    shop_shards: f32,
    shop_shards_init: bool,
    /// shopkeeper hop timestamp on buy or restock
    keeper_hop: f64,
    /// timestamp of last shop restock for ware pop-in
    restock_t: f64,
    /// the shopkeeper's last reaction and when it was said
    line: Option<(&'static str, f64)>,
    /// the line being typed out and when it started
    said: (&'static str, f64),
    /// when each of your charms last took effect, by charm id
    charm_t: std::collections::HashMap<usize, f64>,
    /// gems you have slotted into each skill, by skill name; it fires when every socket is full
    sockets: std::collections::HashMap<&'static str, Vec<Element>>,
    /// the gem being dragged, by index in your stacks
    drag: Option<usize>,
    /// where that gem was picked up: let go on the same spot, it was a click
    drag_from: Vec2,
    /// when the next wave's enemy arrived (game time); it walks in and you wait for it
    intro: Option<f64>,
    /// when Wave 1 started (game time), for opening entrance motion
    battle_t: f64,
    /// when the battle ended (real time); the result panel waits for the kill to play out
    ended_at: f64,
    /// sheet animations playing: (frames, centre, size, start, duration, tint)
    anims: Vec<(&'static [(f32, f32, f32)], Vec2, f32, f64, f32, Color)>,
    /// your board as last drawn: (which one, tiles turned face up, when it last came in anew)
    board_seen: (usize, usize, f64),
    /// per-side board crack tracking for jolt/ease feedback: (cur_board, crack_per_tile)
    tile_board: [usize; 2],
    tile_crack: TileGrid<u8>,
    /// per-tile micro-animation: (start_time, kind: +1 cracked, -1 eased, 2 mined, dir_x, dir_y)
    tile_fx: TileGrid<(f64, i8, f32, f32)>,
    /// board swap slide direction (-1 or +1) and timestamp
    board_dir: f32,
    /// board redeal celebration timestamp
    redeal_t: f64,
    /// stack chip arrival pop timestamp per element
    chip_pop: [f64; 5],
    /// rite/basic cast button recoil: (name, timestamp)
    cast_kick: Option<(&'static str, f64)>,
    /// enemy board highlight cursor: (tile index, timestamp)
    enemy_cursor: Option<(usize, f64)>,
    /// shop sold stamp bounce timestamps per shelf slot
    sold_t: [f64; 4],
    /// settings volume tick pop: (row 0=music/1=sound, timestamp)
    vol_pop: (usize, f64),
    /// smoothly sliding x position of active board tab underline
    tab_x: f32,
    /// surge tracking for pip bounce & multiplier pop
    surge_seen: [i32; 2],
    surge_t: [[f64; 16]; 2],
    surge_pop: [f64; 2],
    /// picks tracking for counter pulse
    picks_seen: [i32; 2],
    picks_pop: [f64; 2],
    /// smooth enemy break meter fill and progress flash
    break_vis: f32,
    break_flash: f64,
    /// fighter sidestep dodge animation: (start_time, strength)
    dodge: [(f64, f32); 2],
    /// shield soak deflection flash timestamp per fighter
    shield_hit: [f64; 2],
    /// enemy load / break timestamps for aura pulse & shatter
    load_t: f64,
    break_t: f64,
    /// smooth hover interpolation (0..=1) keyed by widget position hash
    hover_t: std::collections::HashMap<u64, f32>,
    /// sub-step transition timestamp (2-step brand/learn screens, event result reveal)
    sub_t: f64,
    /// world map bearer swap bounce
    bearer_t: f64,
    last_bearer: usize,
    /// forge ore / purchase bounce
    forge_t: f64,
}

thread_local! {
    /// [display face for titles and numbers, body face for everything small]
    static FONT: std::cell::RefCell<[Option<Font>; 2]> = const { std::cell::RefCell::new([None, None]) };
    /// where the camera is looking this frame, for snapping text to whole screen pixels
    static CAM: std::cell::Cell<Vec2> = const { std::cell::Cell::new(vec2(640.0, 360.0)) };
    /// the "abandon this run?" question is up
    static RETREAT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// [Tiny Dungeon sheet, 32px tile atlas, effects sheet]
    static ART: std::cell::RefCell<[Option<Texture2D>; 3]> = const { std::cell::RefCell::new([None, None, None]) };
    static SCALE: std::cell::Cell<f32> = const { std::cell::Cell::new(1.0) };
    static FX: std::cell::RefCell<Fx> = std::cell::RefCell::new(Fx::default());
    static SOUNDS: std::cell::RefCell<Vec<Vec<Sound>>> = const { std::cell::RefCell::new(Vec::new()) };
    static QUIET: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// (music, sound effects) volume, each 0..=10
    static VOLUME: std::cell::Cell<(i32, i32)> = const { std::cell::Cell::new((6, 8)) };
    static SETTINGS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// the rules sheet is up over the first battle
    static RULES: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// the rules sheet, reopened from settings over any screen
    static HOWTO: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// the deeds sheet is up over the map of The Below
    static DEEDS_ON: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static FORGE_ON: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// a sheet over the map is being drawn: its own words may show tooltips though it is modal
    static SHEET_TIPS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// game time and this frame's step: real time, slowed for a kill and stopped for a hit-stop
    static CLOCK: std::cell::Cell<(f64, f32)> = const { std::cell::Cell::new((0.0, 0.0)) };
    /// real times until which (slow motion, hit-stop) last
    static WARP: std::cell::Cell<(f64, f64)> = const { std::cell::Cell::new((0.0, 0.0)) };
    /// how muffled the audio is right now, 0..=1
    static MUFFLE: std::cell::Cell<f32> = const { std::cell::Cell::new(0.0) };
    /// real time when the active modal panel opened, for crisp unfold animation
    static MODAL_T: std::cell::Cell<f64> = const { std::cell::Cell::new(0.0) };
    static MODAL_WAS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

pub fn volume() -> (i32, i32) {
    VOLUME.with(|v| v.get())
}

pub fn set_volume(v: (i32, i32)) {
    VOLUME.with(|x| x.set(v));
}

/// The game clock. Every animation runs on it, so slow motion slows everything at once.
fn now() -> f64 {
    CLOCK.with(|c| c.get().0)
}

/// Seconds of game time this frame.
fn dt() -> f32 {
    CLOCK.with(|c| c.get().1)
}

/// Slows the game (and muffles the sound) for `secs` of real time.
fn slowmo(secs: f64) {
    let t = get_time();
    trace(|| format!("anim slow motion {secs:.2}s"));
    WARP.with(|w| w.set((t + secs, w.get().1)));
}

/// Freezes the game for a blink, to sell a heavy hit.
fn hitstop(secs: f64) {
    let t = get_time();
    trace(|| format!("anim hitstop {secs:.2}s"));
    WARP.with(|w| w.set((w.get().0, w.get().1.max(t + secs))));
}

fn punch() {
    fx(|fx| fx.punch = now());
}

/// Music loudness 0.0..=1.0 as the player set it; 0 while muted.
pub fn music_volume() -> f32 {
    if quiet() {
        0.0
    } else {
        VOLUME.with(|v| v.get().0) as f32 / 10.0
    }
}

/// The takes of every sound, in the order of `sfx::SOUNDS`.
pub fn set_sounds(sounds: Vec<Vec<Sound>>) {
    SOUNDS.with(|s| *s.borrow_mut() = sounds);
}

/// True while the player has sound switched off (M).
pub fn quiet() -> bool {
    QUIET.with(|q| q.get())
}

/// Reads or changes the visual memory.
fn fx<R>(f: impl FnOnce(&mut Fx) -> R) -> R {
    FX.with(|fx| f(&mut fx.borrow_mut()))
}

/// Throws `n` squares outward from `at`.
fn burst(at: Vec2, n: usize, speed: f32, size: f32, color: Color) {
    let now = now();
    fx(|fx| {
        for _ in 0..n {
            let vel = Vec2::from_angle(gen_range(0.0, std::f32::consts::TAU)) * gen_range(0.25, 1.0) * speed;
            fx.parts.push(Part { pos: at, vel, born: now, life: gen_range(0.35, 0.9), size: size * gen_range(0.5, 1.0), color, grav: 520.0, home: None });
        }
    });
}

/// Sends `n` shards from one spot to another, like a claimed crystal flying to your stacks.
fn stream(from: Vec2, to: Vec2, n: usize, color: Color) {
    let now = now();
    fx(|fx| {
        for _ in 0..n {
            let vel = Vec2::from_angle(gen_range(0.0, std::f32::consts::TAU)) * gen_range(80.0, 260.0);
            fx.parts.push(Part { pos: from, vel, born: now, life: gen_range(0.45, 0.7), size: gen_range(5.0, 9.0), color, grav: 0.0, home: Some(to) });
        }
    });
}

fn quake(strength: f32) {
    quake_at(now(), strength);
}

/// A shake that starts at `t`, which may be later: as a bolt lands.
fn quake_at(t: f64, strength: f32) {
    // the user found full strength overkill: every shake plays at a third, and a weaker one never cuts a stronger short
    let s = strength / 3.0;
    fx(|fx| {
        if t - fx.quake.0 > 0.15 || s >= fx.quake.1 {
            fx.quake = (t, s);
        }
    });
}

fn flash(color: Color) {
    fx(|fx| fx.flash = Some((color, now())));
}

/// Particles that float up from around `at`, like flames or healing motes.
fn rise(at: Vec2, n: usize, spread: f32, color: Color) {
    let now = now();
    fx(|fx| {
        for _ in 0..n {
            let pos = at + vec2(gen_range(-spread, spread), gen_range(-spread, spread) * 0.5);
            let vel = vec2(gen_range(-20.0, 20.0), gen_range(-150.0, -60.0));
            fx.parts.push(Part { pos, vel, born: now, life: gen_range(0.5, 1.0), size: gen_range(4.0, 9.0), color, grav: -60.0, home: None });
        }
    });
}

/// Moves and draws every live particle.
fn particles() {
    let (now, dt) = (now(), dt());
    fx(|fx| {
        fx.parts.retain(|p| ((now - p.born) as f32) < p.life);
        for p in fx.parts.iter_mut() {
            let age = (now - p.born) as f32 / p.life;
            match p.home {
                Some(home) => {
                    // scatter first, then get pulled in harder and harder
                    p.pos += p.vel * dt * (1.0 - age);
                    p.pos = p.pos.lerp(home, (dt * (2.0 + 22.0 * age * age)).min(1.0));
                }
                None => {
                    p.vel.y += p.grav * dt;
                    p.pos += p.vel * dt;
                }
            }
            let s = p.size * (1.0 - age * 0.6);
            // a round mote with a faint halo, fading out as it dies so a big burst never reads as a solid blob
            let a = p.color.a * (1.0 - age * age);
            draw_circle(p.pos.x, p.pos.y, s * 0.9, fade(p.color, 0.18 * a));
            draw_circle(p.pos.x, p.pos.y, s * 0.5, fade(p.color, a));
        }
    });
}

fn play(s: Sfx) {
    if QUIET.with(|q| q.get()) {
        return;
    }
    SOUNDS.with(|all| {
        if let Some(takes) = all.borrow().get(s as usize).filter(|t| !t.is_empty()) {
            let level = VOLUME.with(|v| v.get().1) as f32 / 10.0;
            play_sound(&takes[gen_range(0, takes.len())], PlaySoundParams { looped: false, volume: sfx::SOUNDS[s as usize].0 * level });
        }
    });
}

/// The display face (titles, names, big numbers) and the body face (rules text). Either may be
/// missing; the built-in font stands in. Text drawn at its exact size lands on whole pixels and
/// looks the same either way; smooth sampling keeps text drawn a few percent off its size clean.
pub fn set_fonts(display: Option<Font>, body: Option<Font>) {
    FONT.with(|f| {
        *f.borrow_mut() = [display, body].map(|font| {
            font.map(|mut font| {
                font.set_filter(FilterMode::Linear);
                font
            })
        })
    });
}

/// The Tiny Dungeon sprite sheet (12 tiles a row, 16px each), the 32px tile atlas (8 tiles a
/// row) and the effects sheet (16px cells).
pub fn set_art(sheet: Texture2D, sigils: Texture2D, effects: Texture2D) {
    // the pixel sheet stays crisp; the sigils and particles are smooth art and scale smoothly
    sheet.set_filter(FilterMode::Nearest);
    let all = [sheet, sigils, effects];
    ART.with(|a| *a.borrow_mut() = all.map(Some));
}

/// Draws `source` from art sheet `which` stretched over `r`.
fn art(which: usize, source: Rect, r: Rect, tint: Color) {
    ART.with(|a| {
        if let Some(sheet) = a.borrow()[which].as_ref() {
            draw_texture_ex(sheet, r.x, r.y, tint, DrawTextureParams { dest_size: Some(vec2(r.w, r.h)), source: Some(source), ..Default::default() });
        }
    });
}

// Effects: the particles (x, y, side in particles.png) drawn together, growing, turning and fading.
const FX_EMBER: &[(f32, f32, f32)] = &[(0.0, 0.0, 128.0), (128.0, 0.0, 128.0), (256.0, 0.0, 128.0)];
const FX_TIDE: &[(f32, f32, f32)] = &[(384.0, 0.0, 128.0), (512.0, 0.0, 128.0), (640.0, 0.0, 128.0)];
const FX_GALE: &[(f32, f32, f32)] = &[(768.0, 0.0, 128.0), (896.0, 0.0, 128.0), (0.0, 128.0, 128.0)];
const FX_STONE: &[(f32, f32, f32)] = &[(128.0, 128.0, 128.0), (256.0, 128.0, 128.0), (384.0, 128.0, 128.0)];
const FX_SPARK: &[(f32, f32, f32)] = &[(512.0, 128.0, 128.0), (640.0, 128.0, 128.0), (768.0, 128.0, 128.0)];
const FX_HEAL: &[(f32, f32, f32)] = &[(896.0, 128.0, 128.0), (768.0, 128.0, 128.0), (640.0, 0.0, 128.0)];
const FX_FLAME: &[(f32, f32, f32)] = &[(0.0, 256.0, 128.0), (128.0, 256.0, 128.0), (256.0, 256.0, 128.0)];
const FX_SLASH: &[(f32, f32, f32)] = &[(384.0, 256.0, 128.0), (512.0, 256.0, 128.0)];

fn fx_impact(e: Element) -> &'static [(f32, f32, f32)] {
    match e {
        Element::Ember => FX_EMBER,
        Element::Tide => FX_TIDE,
        Element::Gale => FX_GALE,
        Element::Stone => FX_STONE,
        Element::Spark => FX_SPARK,
    }
}

/// Plays an effect once at `at`, about `size` across, over `dur` seconds.
fn anim(frames: &'static [(f32, f32, f32)], at: Vec2, size: f32, dur: f32, tint: Color) {
    fx(|fx| fx.anims.push((frames, at, size, now(), dur, tint)));
}

/// Draws every playing sheet animation.
fn anims() {
    let now = now();
    fx(|fx| {
        fx.anims.retain(|a| ((now - a.3) as f32) < a.4);
        ART.with(|a| {
            let Some(sheet) = a.borrow()[2].clone() else { return };
            for &(layers, at, size, t0, dur, tint) in &fx.anims {
                let p = ((now - t0) as f32 / dur).clamp(0.0, 1.0);
                let grow = 1.0 - (1.0 - p) * (1.0 - p);
                // each layer swells out, turns its own way, and fades; a quick fade in avoids a pop
                for (i, &(x, y, side)) in layers.iter().enumerate() {
                    let li = i as f32;
                    let s = size * (0.5 + 0.7 * grow) * (1.0 - 0.15 * li);
                    let a = tint.a * (p * 12.0).min(1.0) * (1.0 - p).powf(1.4) * (1.0 - 0.2 * li);
                    let turn = li * 1.1 + p * if i % 2 == 0 { 1.4 } else { -1.4 };
                    draw_texture_ex(&sheet, at.x - s / 2.0, at.y - s / 2.0, fade(tint, a), DrawTextureParams {
                        dest_size: Some(vec2(s, s)),
                        source: Some(Rect::new(x, y, side, side)),
                        rotation: turn,
                        ..Default::default()
                    });
                }
            }
        });
    });
}

/// Spawns an expanding geometric ring at `at` starting right now.
fn ring(at: Vec2, r0: f32, r1: f32, sides: u8, rot: f32, width: f32, color: Color, dur: f32) {
    ring_at(now(), at, r0, r1, sides, rot, width, color, dur);
}

/// Spawns an expanding rectangular border echo emanating directly from `r`'s outer edge.
fn rect_ring(r: Rect, grow: f32, width: f32, color: Color, dur: f32) {
    let t0 = now();
    fx(|fx| fx.rings.push(Ring { at: r.center(), size: vec2(r.w, r.h), r0: 0.0, r1: grow, sides: 254, rot: 0.0, width, color, t0, dur }));
}

/// Spawns an expanding geometric ring starting at `t0`.
fn ring_at(t0: f64, at: Vec2, r0: f32, r1: f32, sides: u8, rot: f32, width: f32, color: Color, dur: f32) {
    fx(|fx| fx.rings.push(Ring { at, size: Vec2::ZERO, r0, r1, sides, rot, width, color, t0, dur }));
}

/// Draws every live expanding geometric ring.
fn draw_rings() {
    let now = now();
    fx(|fx| {
        fx.rings.retain(|r| (now - r.t0) < r.dur as f64);
        for r in &fx.rings {
            let el = (now - r.t0) as f32;
            if el < 0.0 {
                continue;
            }
            let p = (el / r.dur).clamp(0.0, 1.0);
            let e = 1.0 - (1.0 - p).powi(3);
            let rad = r.r0 + (r.r1 - r.r0) * e;
            let a = r.color.a * (1.0 - p).powf(1.3);
            let w = (r.width * (1.0 - p * 0.65)).max(1.0);
            let col = fade(r.color, a);
            match r.sides {
                0 => draw_circle_lines(r.at.x, r.at.y, rad, w, col),
                254 => {
                    let rw = r.size.x + rad * 2.0;
                    let rh = r.size.y + rad * 2.0;
                    draw_rectangle_lines(r.at.x - rw * 0.5, r.at.y - rh * 0.5, rw, rh, w, col);
                }
                255 => draw_rectangle_lines(r.at.x - rad, r.at.y - rad, rad * 2.0, rad * 2.0, w, col),
                n => draw_poly_lines(r.at.x, r.at.y, n, rad, r.rot + p * 18.0, w, col),
            }
        }
    });
}

/// Cubic ease-out from 0 to 1.
fn ease_out3(p: f32) -> f32 {
    let q = 1.0 - p.clamp(0.0, 1.0);
    1.0 - q * q * q
}

/// Staggered entrance progress (0..=1) for item `k` since `t0`.
fn stagger(t0: f64, k: usize, step: f32, dur: f32) -> f32 {
    let el = (now() - t0) as f32 - k as f32 * step;
    ease_out3((el / dur).clamp(0.0, 1.0))
}

/// Smoothly interpolates a 0..=1 hover factor for a widget identified by `r`.
fn hover_lerp(r: Rect, hot: bool) -> f32 {
    let key = ((r.x.round() as i32 as u64) << 40) ^ ((r.y.round() as i32 as u64) << 20) ^ ((r.w.round() as i32 as u64) << 8) ^ (r.h.round() as i32 as u64);
    let step = (get_frame_time().min(0.05) * 14.0).min(1.0);
    fx(|fx| {
        if fx.hover_t.len() > 256 {
            fx.hover_t.retain(|_, v| *v > 0.01);
        }
        let v = fx.hover_t.entry(key).or_insert(0.0);
        let target = if hot { 1.0 } else { 0.0 };
        *v += (target - *v) * step;
        if (*v - target).abs() < 0.005 {
            *v = target;
        }
        *v
    })
}

/// Crisp monochrome corner-tick brackets framing a hovered card or button (no glow, no accent stripe).
fn corner_ticks(r: Rect, k: f32, c: Color) {
    if k <= 0.02 {
        return;
    }
    let len = 8.0 * k;
    let out = 3.0 + 2.0 * (1.0 - k);
    let col = fade(c, k * 0.9);
    let (x0, y0, x1, y1) = (r.x - out, r.y - out, r.right() + out, r.bottom() + out);
    for (cx, cy, dx, dy) in [(x0, y0, 1.0, 1.0), (x1, y0, -1.0, 1.0), (x0, y1, 1.0, -1.0), (x1, y1, -1.0, -1.0)] {
        draw_line(cx, cy, cx + dx * len, cy, 1.5, col);
        draw_line(cx, cy, cx, cy + dy * len, 1.5, col);
    }
}

/// Current modal unfold factor (0..=1), easing smoothly over 0.18s when any sheet opens.
fn modal_open_p() -> f32 {
    let el = (get_time() - MODAL_T.with(|t| t.get())) as f32;
    ease_out3((el / 0.18).clamp(0.0, 1.0))
}

/// Records the exact instant a modal sheet opens so Frame 1 starts smoothly at 0.
fn start_modal_anim() {
    MODAL_T.with(|t| t.set(get_time()));
    MODAL_WAS.with(|m| m.set(true));
}

/// Draws flying alchemical gems arcing from mined crystals into stack chips, popping the chip on arrival.
fn draw_flying_gems() {
    let now = now();
    let mut landed = vec![];
    fx(|fx| {
        let mut keep = vec![];
        for g in fx.flying_gems.drain(..) {
            let p = ((now - g.t0) as f32 / g.dur).clamp(0.0, 1.0);
            if p >= 1.0 {
                landed.push((g.element, g.to));
                continue;
            }
            keep.push(g);
            // curved upward parabolic arc with accelerating pull into the chip
            let e = p * p * (3.0 - 2.0 * p);
            let lift = (p * std::f32::consts::PI).sin() * 58.0;
            let pos = g.from.lerp(g.to, e) - vec2(0.0, lift);
            let col = mix(INK, hue(g.element), 0.55);
            // crisp comet ghost trail
            for k in 1..=3 {
                let pb = (p - k as f32 * 0.06).max(0.0);
                let eb = pb * pb * (3.0 - 2.0 * pb);
                let bp = g.from.lerp(g.to, eb) - vec2(0.0, (pb * std::f32::consts::PI).sin() * 58.0);
                draw_circle(bp.x, bp.y, 4.5 - k as f32, fade(col, 0.45 - k as f32 * 0.12));
            }
            let r = 11.0 + (p * std::f32::consts::PI).sin() * 4.0;
            sign(g.element, pos, r, col);
        }
        fx.flying_gems = keep;
    });
    for (e, to) in landed {
        let col = mix(INK, hue(e), 0.55);
        fx(|fx| {
            fx.chip_pop[e as usize] = now;
            fx.floats.push((to - vec2(0.0, 16.0), "+1".into(), col, now, 18.0));
        });
        ring(to, 12.0, 34.0, 4, 0.0, 2.0, col, 0.22);
        burst(to, 8, 120.0, 4.0, col);
    }
}

/// Subtle drifting monochrome dust motes across dark backgrounds (title, world, shrine).
fn ambient_motes(count: usize, seed: f32, alpha: f32) {
    let t = now() as f32;
    for k in 0..count {
        let f = k as f32;
        let phase = (t * (0.04 + (f * 0.017) % 0.04) + hash01(k as u64 + seed as u64, 17)).fract();
        let x = hash01(k as u64 + seed as u64, 3) * VIEW.x + (t * 0.5 + f * 1.9).sin() * 14.0;
        let y = VIEW.y * (1.0 - phase);
        let a = alpha * (phase * std::f32::consts::PI).sin();
        let s = 1.2 + (k % 3) as f32 * 0.7;
        draw_rectangle(x, y, s, s, fade(INK, a));
    }
}

/// Call when the screen changes: forgets the old screen's effects and fades the new one in.
pub fn transition() {
    trace(|| "screen changes: effects cleared".into());
    let t = now();
    fx(|fx| *fx = Fx { fade: t, screen_t: t, ..Default::default() });
}

pub fn end_frame() {
    draw_flying_gems();
    draw_rings();
    draw_tip();
    let el = (now() - fx(|fx| fx.fade)) as f32;
    if el < 0.22 {
        let p = ease_out3((el / 0.22).clamp(0.0, 1.0));
        let h = VIEW.y * 0.5 * (1.0 - p);
        overlay();
        // sleek parting ink curtains instead of a sluggish grey fade
        draw_rectangle(-2000.0, -2000.0, 5280.0, 2000.0 + h, BG);
        draw_rectangle(-2000.0, VIEW.y - h, 5280.0, 2000.0 + h, BG);
        if p < 0.92 {
            draw_line(-2000.0, h, 3280.0, h, 1.5, fade(FAINT, 1.0 - p));
            draw_line(-2000.0, VIEW.y - h, 3280.0, VIEW.y - h, 1.5, fade(FAINT, 1.0 - p));
        }
    }
}

/// Bounce curve for something that appeared at `t0`: starts small, overshoots, settles at 1.
fn bounce(t0: f64) -> f32 {
    let p = (((now() - t0) / 0.22) as f32).clamp(0.0, 1.0);
    0.7 + 0.3 * p + 0.14 * (p * std::f32::consts::PI).sin()
}

/// When the thing identified by `key` was first drawn.
fn born(key: (usize, usize, usize)) -> f64 {
    let now = now();
    fx(|fx| *fx.pops.entry(key).or_insert(now))
}

fn unpop(key: (usize, usize, usize)) {
    fx(|fx| fx.pops.remove(&key));
}

fn scaled(r: Rect, k: f32) -> Rect {
    let c = r.center();
    Rect::new(c.x - r.w * k / 2.0, c.y - r.h * k / 2.0, r.w * k, r.h * k)
}

fn scale() -> f32 {
    SCALE.with(|s| s.get())
}

/// Everything is laid out on a fixed 1280x720 canvas, scaled and centred to fit the window.
pub fn begin_frame() {
    // the clock: slow motion eases back to full speed over its last 0.3 s
    let (real, real_dt) = (get_time(), get_frame_time().min(0.1));
    if get_frame_time() > 0.05 {
        trace(|| format!("slow frame {:.3}s", get_frame_time()));
    }
    let (slow, stop) = WARP.with(|w| w.get());
    let speed = if real < stop {
        0.0
    } else if real < slow {
        let left = (slow - real) as f32;
        if left > 0.3 { 0.2 } else { 0.2 + 0.8 * (1.0 - left / 0.3) }
    } else {
        1.0
    };
    CLOCK.with(|c| c.set((c.get().0 + (real_dt * speed) as f64, real_dt * speed)));
    // muffled during slow motion and behind a panel
    let want: f32 = if real < slow {
        1.0
    } else if modal() {
        0.45
    } else {
        0.0
    };
    let m = MUFFLE.with(|m| {
        let v = m.get() + (want - m.get()) * (real_dt * 10.0).min(1.0);
        m.set(v);
        v
    });
    quad_snd::set_muffle(m);
    let (w, h) = (screen_width(), screen_height());
    let k = (w / VIEW.x).min(h / VIEW.y).max(0.1);
    SCALE.with(|s| s.set(k));
    if is_key_pressed(KeyCode::M) {
        QUIET.with(|q| q.set(!q.get()));
    }
    // a heavy hit shakes the whole view for a moment
    let (t0, strength) = fx(|fx| fx.quake);
    let since = (now() - t0) as f32;
    let jolt = if (0.0..0.3).contains(&since) {
        vec2((since * 71.0).sin(), (since * 93.0).cos()) * strength * (1.0 - since / 0.3)
    } else {
        Vec2::ZERO
    };
    let since = (now() - fx(|fx| fx.punch)) as f32;
    let z = if (0.0..0.28).contains(&since) { 1.0 + 0.035 * (1.0 - since / 0.28).powi(2) } else { 1.0 };
    CAM.with(|c| c.set(VIEW / 2.0 + jolt));
    set_camera(&Camera2D { target: VIEW / 2.0 + jolt, zoom: vec2(2.0 * k / w, 2.0 * k / h) * z, ..Default::default() });
    // track modal open edge so sheets unfold smoothly over 0.14 s
    let is_modal = modal();
    if is_modal && !MODAL_WAS.with(|m| m.get()) {
        MODAL_T.with(|t| t.set(real));
    }
    MODAL_WAS.with(|m| m.set(is_modal));
    // a fresh frame: nothing drawn yet to clash with
    DRAWN.with(|v| v.borrow_mut().clear());
    DRAWN_LAYER.with(|l| l.set(0));
    draw_rectangle(-2000.0, -2000.0, 5280.0, 4720.0, BG);
}

/// A line in the dev log (see `devlog`): input, events and animation timing, to find bugs by.
fn trace(line: impl FnOnce() -> String) {
    crate::devlog::log(line);
}

fn mouse() -> Vec2 {
    if let Some(p) = crate::ctl::mouse() {
        return p;
    }
    let window = vec2(screen_width(), screen_height());
    (Vec2::from(mouse_position()) - window / 2.0) / scale() + VIEW / 2.0
}

/// True while a panel (settings, retreat) is open over the screen.
fn modal() -> bool {
    SETTINGS.with(|s| s.get()) || RETREAT.with(|r| r.get()) || RULES.with(|r| r.get()) || HOWTO.with(|h| h.get()) || DEEDS_ON.with(|d| d.get()) || FORGE_ON.with(|f| f.get())
}

/// A left click meant for the screen. While a panel is open it swallows every click.
fn clicked() -> bool {
    !modal() && is_mouse_button_pressed(MouseButton::Left)
}

/// Pirata One sits small for its nominal size; IBM Plex Mono runs wide. These keep the layouts as designed.
const DISPLAY_K: f32 = 1.15;
const BODY_K: f32 = 0.82;
/// Text this size and up is set in the gothic display face; smaller text in the mono body face.
const DISPLAY_FROM: f32 = 24.0;

/// Which face, the font size to ask for, and the scale back to the canvas, for text of design `size`.
/// The glyphs are rasterised at the window's real pixel size, so text stays crisp at any scale.
fn face(size: f32) -> (usize, u16, f32) {
    face_in(if size >= DISPLAY_FROM { 0 } else { 1 }, size)
}

/// `face`, in font `which` whatever the size: text that grows or shrinks keeps one look.
fn face_in(which: usize, size: f32) -> (usize, u16, f32) {
    let k = scale();
    let want = size * if which == 0 { DISPLAY_K } else { BODY_K };
    // ask in logical pixels; macroquad multiplies by the screen's dpi itself
    let px = (want * k).max(1.0);
    // Every new size rasterises a new set of glyphs into the font's atlas, which grows (it is
    // sent to the GPU whole, once a frame: see vendor/macroquad). So text that
    // grows or shrinks, and all display text, rounds to one of a few sizes (about 9% apart) and
    // scales the rest; small body text stays pixel exact, to stay sharp.
    let ladder = size.fract() != 0.0 || which == 0;
    let base = if ladder { (px.log2() * 8.0).round() / 8.0 } else { px.round().log2() };
    let base = 2f32.powf(base).round().max(1.0);
    (which, base as u16, px / base / k)
}

/// Moves a canvas point onto the nearest whole physical pixel, so glyph edges do not straddle two.
fn snap(p: Vec2) -> Vec2 {
    let (k, dpi, cam) = (scale(), screen_dpi_scale(), CAM.with(|c| c.get()));
    let half = vec2(screen_width(), screen_height()) / 2.0;
    (((p - cam) * k + half) * dpi).round() / dpi / k - half / k + cam
}

fn text(s: &str, x: f32, y: f32, size: f32, c: Color) {
    if crate::ctl::on() && c.a > 0.05 && !s.trim().is_empty() {
        let d = measure(s, size);
        drawn(Rect::new(x + 1.0, y - d.offset_y + 1.0, d.width - 2.0, d.height - 2.0), || s.to_string());
    }
    text_in(face(size), s, x, y, c);
}

/// Text in an already chosen face (see `face_in`).
fn text_in((which, font_size, font_scale): (usize, u16, f32), s: &str, x: f32, y: f32, c: Color) {
    let at = snap(vec2(x, y));
    FONT.with(|f| {
        let fonts = f.borrow();
        let params = TextParams { font: fonts[which].as_ref(), font_size, font_scale, color: c, ..Default::default() };
        draw_text_ex(s, at.x, at.y, params);
    });
}

fn measure(s: &str, size: f32) -> TextDimensions {
    let (which, font_size, font_scale) = face(size);
    FONT.with(|f| measure_text(s, f.borrow()[which].as_ref(), font_size, font_scale))
}

fn centered(s: &str, cx: f32, y: f32, size: f32, c: Color) {
    text(s, cx - measure(s, size).width / 2.0, y, size, c);
}

/// Centred heading text. (It once had a drop shadow; the flat look has none.)
fn stamped(s: &str, cx: f32, y: f32, size: f32, c: Color) {
    centered(s, cx, y, size, c);
}

fn right(s: &str, x: f32, y: f32, size: f32, c: Color) {
    text(s, x - measure(s, size).width, y, size, c);
}

/// `wrapped`, at the biggest size up to `size` whose lines fit in height `h`.
fn wrapped_fit(s: &str, x: f32, y: f32, w: f32, size: f32, h: f32, c: Color) -> f32 {
    let mut sz = size;
    while sz > 12.0 && wrap(s, w, sz).len() as f32 * sz * 1.25 > h {
        sz -= 1.0;
    }
    wrapped(s, x, y, w, sz, c)
}

/// Word-wraps into width `w`; returns the y of the line after the last one.
fn wrapped(s: &str, x: f32, mut y: f32, w: f32, size: f32, c: Color) -> f32 {
    for line in wrap(s, w, size) {
        rich(&line, x, y, size, c);
        y += size * 1.25;
    }
    y
}

/// One block of a `Column`: an optional label over one or more paragraphs.
struct Block {
    /// how long it holds its room when the column runs short: the lowest gives way first
    keep: u8,
    head: Option<String>,
    paras: Vec<String>,
    /// said instead of `paras` when room runs short; the full text then shows on hover
    short: Option<String>,
    color: Color,
}

#[derive(Clone, Copy, PartialEq)]
enum Fit {
    Full,
    Short,
    Folded,
}

/// Text laid out from what it says rather than at fixed rows. Blocks stack from `y` (a baseline);
/// when they would run past `bottom` (the lowest baseline allowed), the block with the lowest `keep`
/// first falls back to its `short` form, then folds into one "More" line at the end that shows it
/// on hover. So a longer text never lands on whatever sits below the column.
struct Column {
    x: f32,
    y: f32,
    w: f32,
    bottom: f32,
    size: f32,
    blocks: Vec<Block>,
}

const HEAD_GAP: f32 = 20.0;
const PARA_GAP: f32 = 7.0;
const BLOCK_GAP: f32 = 16.0;

impl Column {
    fn new(x: f32, y: f32, w: f32, bottom: f32, size: f32) -> Column {
        Column { x, y, w, bottom, size, blocks: vec![] }
    }

    /// Adds a block; `.short()` right after gives it a short form.
    fn block<S: Into<String>>(&mut self, keep: u8, head: Option<&str>, paras: impl IntoIterator<Item = S>, color: Color) -> &mut Self {
        let paras: Vec<String> = paras.into_iter().map(Into::into).collect();
        if head.is_some() || !paras.is_empty() {
            self.blocks.push(Block { keep, head: head.map(str::to_string), paras, short: None, color });
        }
        self
    }

    fn short(&mut self, s: impl Into<String>) -> &mut Self {
        if let Some(b) = self.blocks.last_mut() {
            b.short = Some(s.into());
        }
        self
    }

    fn pitch(&self) -> f32 {
        self.size * 1.25
    }

    /// The paragraphs a block shows at `fit`.
    fn said<'a>(b: &'a Block, fit: Fit) -> Vec<&'a str> {
        match (fit, &b.short) {
            (Fit::Short, Some(s)) => vec![s.as_str()],
            _ => b.paras.iter().map(String::as_str).collect(),
        }
    }

    /// From one block's first baseline to the next block's.
    fn height(&self, b: &Block, fit: Fit) -> f32 {
        let paras = Column::said(b, fit);
        let lines: usize = paras.iter().map(|p| wrap(p, self.w, self.size).len()).sum();
        let head = if b.head.is_some() { if paras.is_empty() { self.pitch() } else { HEAD_GAP } } else { 0.0 };
        head + lines as f32 * self.pitch() + paras.len().saturating_sub(1) as f32 * PARA_GAP + BLOCK_GAP
    }

    /// The room every block takes in full, from the first baseline to below the last line.
    fn needed(&self) -> f32 {
        self.blocks.iter().map(|b| self.height(b, Fit::Full)).sum::<f32>() - BLOCK_GAP
    }

    /// How each block is shown so that the column ends by `bottom`.
    fn fits(&self) -> Vec<Fit> {
        let mut fit = vec![Fit::Full; self.blocks.len()];
        loop {
            let folded = fit.contains(&Fit::Folded);
            let used: f32 = self.blocks.iter().zip(&fit).filter(|(_, f)| **f != Fit::Folded).map(|(b, &f)| self.height(b, f)).sum();
            let last = self.y + used - BLOCK_GAP - self.pitch() + if folded { self.pitch() } else { 0.0 };
            if last <= self.bottom {
                return fit;
            }
            // the least kept block still shown gives way; on a tie the lower one goes first
            let Some(k) = (0..self.blocks.len()).filter(|&k| fit[k] != Fit::Folded).min_by_key(|&k| (self.blocks[k].keep, std::cmp::Reverse(k))) else {
                return fit;
            };
            fit[k] = if fit[k] == Fit::Full && self.blocks[k].short.is_some() { Fit::Short } else { Fit::Folded };
        }
    }

    /// Draws the column; returns the baseline after it.
    fn draw(&self) -> f32 {
        let fit = self.fits();
        let mut y = self.y;
        let mut folded = vec![];
        for (b, &f) in self.blocks.iter().zip(&fit) {
            if f == Fit::Folded {
                folded.push(b);
                continue;
            }
            let top = y;
            if let Some(h) = &b.head {
                label(h, self.x, y);
                y += if b.paras.is_empty() { self.pitch() } else { HEAD_GAP };
            }
            for p in Column::said(b, f) {
                y = wrapped(p, self.x, y, self.w, self.size, b.color) + PARA_GAP;
            }
            if f == Fit::Short && Rect::new(self.x, top - self.size, self.w, y - top).contains(mouse()) {
                tip_text(b.head.as_deref().unwrap_or("In full"), b.paras.join("\n"));
            }
            y += BLOCK_GAP - PARA_GAP;
        }
        if !folded.is_empty() {
            let more = "More...";
            let hot = Rect::new(self.x, y - self.size, measure(more, self.size).width + 8.0, self.size * 1.3).contains(mouse());
            text(more, self.x, y, self.size, if hot { INK } else { MUTED });
            if hot {
                let body = folded.iter().map(|b| match &b.head {
                    Some(h) => format!("{}: {}", h, b.paras.join(" ")),
                    None => b.paras.join(" "),
                });
                tip_text("More", body.collect::<Vec<_>>().join("\n"));
            }
            y += self.pitch();
        }
        y
    }
}

/// Debug builds driven by `ctl` only: where text, signs and sprites landed this frame, so a
/// screenshot comes with a list of the ones that overlap.
struct Drawn {
    r: Rect,
    layer: u32,
    group: u32,
    what: String,
}

thread_local! {
    static DRAWN: std::cell::RefCell<Vec<Drawn>> = const { std::cell::RefCell::new(vec![]) };
    static DRAWN_LAYER: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    static DRAWN_GROUP: std::cell::Cell<(u32, bool)> = const { std::cell::Cell::new((0, false)) };
}

fn drawn(r: Rect, what: impl FnOnce() -> String) {
    if !crate::ctl::on() || r.w < 1.0 || r.h < 1.0 {
        return;
    }
    let layer = DRAWN_LAYER.with(|l| l.get());
    let group = DRAWN_GROUP.with(|g| {
        let (id, held) = g.get();
        if !held {
            g.set((id + 1, false));
        }
        id
    });
    DRAWN.with(|v| v.borrow_mut().push(Drawn { r, layer, group, what: what() }));
}

/// Everything drawn inside `f` counts as one piece (a line drawn word by word, or letter by letter).
fn one_piece<T>(f: impl FnOnce() -> T) -> T {
    let outer = DRAWN_GROUP.with(|g| {
        let (id, held) = g.get();
        if !held {
            g.set((id, true));
        }
        held
    });
    let out = f();
    if !outer {
        DRAWN_GROUP.with(|g| g.set((g.get().0 + 1, false)));
    }
    out
}

/// What is drawn after this sits over what came before (a dimmed panel, a tooltip): no clash.
fn overlay() {
    DRAWN_LAYER.with(|l| l.set(l.get() + 1));
}

/// The pieces drawn this frame that overlap, one line each, for `ctl` to save beside a screenshot.
pub fn overlaps() -> Vec<String> {
    DRAWN.with(|v| {
        let v = v.borrow();
        let mut out = vec![];
        for (i, a) in v.iter().enumerate() {
            for b in &v[i + 1..] {
                if a.layer != b.layer || a.group == b.group {
                    continue;
                }
                let (w, h) = (a.r.right().min(b.r.right()) - a.r.x.max(b.r.x), a.r.bottom().min(b.r.bottom()) - a.r.y.max(b.r.y));
                if w > 1.5 && h > 1.5 {
                    let line = format!("{:?} at ({:.0}, {:.0}) x {:?} at ({:.0}, {:.0})", a.what, a.r.x, a.r.y, b.what, b.r.x, b.r.y);
                    if !out.contains(&line) {
                        out.push(line);
                    }
                }
            }
        }
        out
    })
}

// Colour, used sparingly against the grey: rules keywords, and the few names that move.
const BLOOD: Color = rgb(0xff5d5d);
const GOLD: Color = rgb(0xf0c75e);
/// a mastered rite's name: dark red, still readable on the near-black panels
const MASTERED: Color = rgb(0xc0302f);

/// The colour an upgraded rite's name takes: gold, then dark red once mastered.
fn level_color(lvl: usize) -> Color {
    if lvl >= MAX_LEVEL { MASTERED } else { GOLD }
}

/// The colour a rules word is printed in, if it has one (punctuation and case ignored), and how
/// it moves: effects that linger (burn, weaken, detonate) and rarities are alive on the page.
fn keyword(word: &str) -> Option<(Color, Option<Motion>)> {
    let w = word.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase();
    let still = |c| Some((c, None));
    match w.as_str() {
        "burn" | "burns" | "burning" => Some((rgb(0xff8a4c), Some(Motion::Wave))),
        "detonate" => Some((rgb(0xff8a4c), Some(Motion::Tremble))),
        "weaken" | "halved" => Some((rgb(0xd2b48c), Some(Motion::Tremble))),
        "legendary" => Some((GOLD, Some(Motion::Rainbow))),
        "rare" => Some((rgb(0x7fc0ff), Some(Motion::Wave))),
        "ember" => still(rgb(0xff8a4c)),
        "shield" | "armor" | "tide" => still(rgb(0x6fb6ff)),
        "heal" | "heals" | "healing" | "hp" => still(rgb(0x7bd88f)),
        "damage" | "cave-in" | "cave-ins" | "fault" | "faults" => still(BLOOD),
        "shard" | "shards" => still(GOLD),
        "surge" | "spark" => still(rgb(0xffe066)),
        "reveal" | "pick" | "picks" | "gale" => still(rgb(0x5fe0c8)),
        "ignite" => Some((rgb(0xff8a4c), Some(Motion::Wave))),
        "douse" => Some((rgb(0x6fb6ff), Some(Motion::Wave))),
        "gust" => Some((rgb(0x5fe0c8), Some(Motion::Wave))),
        "crush" | "stone" => still(rgb(0xc9a77c)),
        "arc" => Some((rgb(0xffe066), Some(Motion::Tremble))),
        "break" | "broken" => Some((BLOOD, Some(Motion::Tremble))),
        "brittle" => Some((rgb(0x6fb6ff), Some(Motion::Tremble))),
        "marked" => Some((INK, Some(Motion::Wave))),
        _ => None,
    }
}

/// Rules words that explain themselves: (heading, explanation). They float on the page, so a
/// player can tell they are worth hovering.
fn glossary(word: &str) -> Option<(&'static str, &'static str)> {
    let w = word.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase();
    Some(match w.as_str() {
        "burn" | "burns" | "burning" => ("Burn", "Loses 1 HP per burn as its turn starts, through shield. Then the burn halves."),
        "detonate" => ("Detonate", "All burn on the foe goes off: 3 damage each. Then it is gone."),
        "weak" | "weaken" | "halved" => ("Weak", "Its rites deal half damage for the turns shown."),
        "shield" => ("Shield", "Soaks damage before HP. Half wears off as its owner's turn starts. Burn goes through."),
        "armor" => ("Armor", "Every hit on it deals this much less, down to 1."),
        "evade" => ("Evade", "The first rite at it each turn misses. The gems are still spent."),
        "grazes" | "graze" => ("Graze", "The first rite at it each turn deals half damage."),
        "surge" => ("Surge", "+10% rite damage per step, up to 10. Mining adds a step; each rite spends one."),
        "overpressure" => ("Overpressure", "From turn 13, every rite deals 2 more each turn."),
        "power" => ("Power", "Damage for each gem a rite spends, before its bonus and surge."),
        "duds" => ("Duds", "Cave-ins that do you no harm this wave."),
        "pick" | "picks" => ("Picks", "Each mines a crystal or digs a vent. Never fewer than 3."),
        "cave-in" | "cave-ins" => ("Cave-in", "A tile at 3 cracks falls: 10 damage to its board's owner, through shield. A crystal there is lost."),
        "vent" | "vents" => ("Vent", "A pick on a tile without a crystal: it and the tiles touching it lose a crack."),
        "fault" | "faults" => ("Fault", "A tile dealt with 2 cracks: one more and it caves in."),
        "crystal" | "crystals" => ("Crystal", "Mine it for a gem. The tiles touching it crack."),
        "rare" => ("Rare", "Offered less often than Common. 45 shards in a shop."),
        "legendary" => ("Legendary", "Offered rarely. 65 shards in a shop."),
        "mark" => ("Mark", "Elements of the last rite that hit it. A new element reacts."),
        "ignite" => ("Ignite", "Ember reacts: +3 burn on the target."),
        "douse" => ("Douse", "Tide reacts: +4 shield for the caster."),
        "gust" => ("Gust", "Gale reacts: +1 pick for the caster."),
        "crush" => ("Crush", "Stone reacts: +4 damage, before surge."),
        "arc" => ("Arc", "Spark reacts: +2 surge for the caster."),
        "brittle" => ("Brittle", "Blocks the next rite, then shatters: the attacker takes that much damage."),
        "break" | "broken" => ("Break", "Deal this much rite damage in your turn and its loaded rites crack: they still fire, for half."),
        _ => return None,
    })
}

thread_local!(static TIP: std::cell::RefCell<Option<(String, String)>> = const { std::cell::RefCell::new(None) });

thread_local!(static MARK_TIP: std::cell::RefCell<Option<Vec<Element>>> = const { std::cell::RefCell::new(None) });

thread_local!(static STEPS: std::cell::RefCell<Vec<(String, String)>> = const { std::cell::RefCell::new(vec![]) });

/// Asks for a rite's damage steps to be shown by the mouse this frame, ending in `total`.
fn steps_tip(mut steps: Vec<(String, String)>, total: i32) {
    if !modal() {
        steps.push(("damage".into(), total.to_string()));
        STEPS.with(|c| *c.borrow_mut() = steps);
    }
}

/// Asks for `tip` to be shown by the mouse this frame, over everything else.
fn tip(t: (&'static str, &'static str)) {
    tip_text(t.0, t.1.to_string());
}

/// A tooltip of any text; a newline in `body` starts a new paragraph.
fn tip_text(head: &str, body: String) {
    if !modal() || (SHEET_TIPS.with(|t| t.get()) && !SETTINGS.with(|s| s.get())) {
        TIP.with(|c| *c.borrow_mut() = Some((head.to_string(), body)));
    }
}

/// Lines of `s` that fit in width `w`.
fn wrap(s: &str, w: f32, size: f32) -> Vec<String> {
    let mut lines = vec![String::new()];
    for word in s.split(' ') {
        let line = lines.last_mut().expect("starts with one");
        let longer = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
        if !line.is_empty() && measure(&longer, size).width > w {
            lines.push(word.to_string());
        } else {
            *line = longer;
        }
    }
    lines
}

/// The tooltip asked for this frame, in a plate beside the mouse and kept on screen.
fn draw_tip() {
    overlay();
    let steps = STEPS.with(|c| std::mem::take(&mut *c.borrow_mut()));
    if !steps.is_empty() {
        // how the number is made, one step a line, the total last
        let h = 44.0 + steps.len() as f32 * 21.0;
        let m = mouse();
        let r = Rect::new((m.x + 16.0).min(VIEW.x - 330.0), if m.y + 20.0 + h > VIEW.y { m.y - 12.0 - h } else { m.y + 20.0 }, 318.0, h);
        plate(r, CARD);
        text("How it adds up", r.x + 14.0, r.y + 24.0, 16.0, INK);
        let last = steps.len() - 1;
        for (k, (what, v)) in steps.iter().enumerate() {
            let (y, c) = (r.y + 48.0 + k as f32 * 21.0, if k == last { INK } else { MUTED });
            if k == last {
                draw_line(r.x + 14.0, y - 16.0, r.x + r.w - 14.0, y - 16.0, 1.0, FAINT);
            }
            text(what, r.x + 14.0, y, 15.0, c);
            right(v, r.x + r.w - 14.0, y, 15.0, c);
        }
    }
    let tip = TIP.with(|c| c.borrow_mut().take());
    if let Some(mark) = MARK_TIP.with(|c| c.borrow_mut().take()) {
        // every element by its sign: the ones in the mark set off nothing, the rest react
        let all = [Element::Ember, Element::Tide, Element::Gale, Element::Stone, Element::Spark];
        let what = |e: Element| if mark.contains(&e) { "in the mark: nothing" } else { reaction_text(e) };
        let head = "Hit last by these. A rite with any other reacts:";
        // as wide as its widest line, so nothing spills out of the box
        let w = all.iter().map(|&e| 94.0 + measure(what(e), 15.0).width).fold(measure(head, 15.0).width, f32::max) + 28.0;
        let h = 84.0 + all.len() as f32 * 26.0;
        let m = mouse();
        let r = Rect::new((m.x + 16.0).min(VIEW.x - 12.0 - w), if m.y + 20.0 + h > VIEW.y { m.y - 12.0 - h } else { m.y + 20.0 }, w, h);
        plate(r, CARD);
        text("Marked", r.x + 14.0, r.y + 24.0, 16.0, INK);
        text(head, r.x + 14.0, r.y + 48.0, 15.0, MUTED);
        for (k, &e) in all.iter().enumerate() {
            let y = r.y + 80.0 + k as f32 * 26.0;
            let marked = mark.contains(&e);
            sign(e, vec2(r.x + 24.0, y - 5.0), 8.0, if marked { FAINT } else { INK });
            text(element_tip(e).0, r.x + 44.0, y, 15.0, if marked { FAINT } else { INK });
            text(what(e), r.x + 108.0, y, 15.0, if marked { FAINT } else { MUTED });
        }
        return;
    }
    let Some((head, body)) = tip else { return };
    let lines: Vec<String> = body.split('\n').flat_map(|p| wrap(p, 290.0, 15.0)).collect();
    let h = 44.0 + lines.len() as f32 * 19.0;
    let m = mouse();
    let r = Rect::new((m.x + 16.0).min(VIEW.x - 330.0), if m.y + 20.0 + h > VIEW.y { m.y - 12.0 - h } else { m.y + 20.0 }, 318.0, h);
    plate(r, CARD);
    text(&head, r.x + 14.0, r.y + 24.0, 16.0, INK);
    for (k, line) in lines.iter().enumerate() {
        text(line, r.x + 14.0, r.y + 46.0 + k as f32 * 19.0, 15.0, MUTED);
    }
}

/// One line of text with its keywords in colour (and the line's own alpha). Words with a
/// tooltip carry a dotted underline; under the mouse they come alive and explain themselves.
fn rich(s: &str, x: f32, y: f32, size: f32, c: Color) {
    one_piece(|| rich_words(s, x, y, size, c));
}

fn rich_words(s: &str, x: f32, y: f32, size: f32, c: Color) {
    let space = measure("a a", size).width - measure("aa", size).width;
    let mut at = x;
    for word in s.split(' ') {
        let w = measure(word, size).width;
        let explained = glossary(word);
        let (col, motion) = match keyword(word) {
            Some((k, m)) => (Color { a: c.a, ..k }, m),
            None => (c, None),
        };
        // still on the page, so a screen of rules text does not crawl; it moves only when looked at
        let hot = Rect::new(at, y - size, w, size * 1.3).contains(mouse());
        match motion.or(explained.map(|_| Motion::Wave)).filter(|_| hot) {
            Some(m) => lively(word, at, y, size, col, m),
            None => text(word, at, y, size, col),
        }
        if let Some(t) = explained {
            if hot {
                tip(t);
            } else {
                // under the word itself, not the punctuation round it
                let core = word.trim_matches(|c: char| !c.is_alphanumeric());
                let x0 = at + measure(&word[..word.find(core).unwrap_or(0)], size).width;
                for k in 0..(measure(core, size).width / 4.0) as i32 {
                    draw_rectangle((x0 + k as f32 * 4.0).round(), (y + 3.0).round(), 2.0, 1.0, fade(col, col.a * 0.55));
                }
            }
        }
        at += w + space;
    }
}

fn rich_centered(s: &str, cx: f32, y: f32, size: f32, c: Color) {
    rich(s, cx - measure(s, size).width / 2.0, y, size, c);
}

/// How a line's letters move on their own.
#[derive(Clone, Copy, PartialEq)]
enum Motion {
    /// a slow wave running through the letters
    Wave,
    /// each letter shivers in place, for things to fear
    Tremble,
    /// the wave, with the letters running through the rainbow
    Rainbow,
}

/// Text from `x` whose letters move by `m`, a few percent of its size.
fn lively(s: &str, x: f32, y: f32, size: f32, c: Color, m: Motion) {
    one_piece(|| lively_letters(s, x, y, size, c, m));
}

fn lively_letters(s: &str, x: f32, y: f32, size: f32, c: Color, m: Motion) {
    let t = now() as f32;
    // a line's width is the sum of its letters' advances, so step along rather than re-measure
    let (mut at, mut buf) = (x, [0u8; 4]);
    for (i, ch) in s.chars().enumerate() {
        let (f, ch) = (i as f32, &*ch.encode_utf8(&mut buf));
        let (dx, dy) = if m == Motion::Tremble {
            let k = (t * 12.0) as usize;
            ((noise(k, i) - 0.5) * size * 0.07, (noise(k + 31, i) - 0.5) * size * 0.07)
        } else {
            (0.0, (t * 2.4 - f * 0.5).sin() * size * 0.06)
        };
        let col = if m == Motion::Rainbow {
            Color { a: c.a, ..macroquad::color::hsl_to_rgb((t * 0.12 + f * 0.05).fract(), 0.75, 0.7) }
        } else {
            c
        };
        text(ch, at + dx, y + dy, size, col);
        at += measure(ch, size).width;
    }
}

fn lively_centered(s: &str, cx: f32, y: f32, size: f32, c: Color, m: Motion) {
    lively(s, cx - measure(s, size).width / 2.0, y, size, c, m);
}

/// A pact's name: Legendary ones shimmer through the rainbow.
fn pact_name(id: usize, x: f32, y: f32, size: f32, c: Color) {
    if CHARMS[id].rarity == Rarity::Legendary {
        lively(CHARMS[id].name, x, y, size, c, Motion::Rainbow);
    } else {
        text(CHARMS[id].name, x, y, size, c);
    }
}

/// A filled rectangle. Corners are square throughout the black-and-white look.
fn block(r: Rect, c: Color) {
    draw_rectangle(r.x, r.y, r.w, r.h, c);
}

/// Kept for its many callers; corners are square now, so `radius` is ignored.
fn rounded(r: Rect, _radius: f32, c: Color) {
    block(r, c);
}

fn grow(r: Rect, d: f32) -> Rect {
    Rect::new(r.x - d, r.y - d, r.w + 2.0 * d, r.h + 2.0 * d)
}

/// A 1 px outline just inside `r`.
fn outline(r: Rect, c: Color) {
    draw_rectangle_lines(r.x, r.y, r.w, r.h, 2.0, c);
}

/// A panel: a flat fill with a hairline edge.
fn plate(r: Rect, c: Color) {
    block(r, c);
    outline(r, FAINT);
}

/// A clickable card with smooth hover lift and crisp corner brackets. Returns true on the frame it is clicked.
fn card(r: Rect, on: bool) -> bool {
    let hot = on && r.contains(mouse());
    let h = hover_lerp(r, hot);
    let pressed = hot && is_mouse_button_down(MouseButton::Left) && !modal();
    let dy = -3.0 * h + if pressed { 2.0 } else { 0.0 };
    let draw_r = Rect::new(r.x, r.y + dy, r.w, r.h);
    plate(draw_r, mix(CARD, CARD_HI, h));
    if h > 0.05 {
        outline(draw_r, mix(FAINT, INK, h));
        corner_ticks(draw_r, h, INK);
    }
    if hot && clicked() {
        play(Sfx::Click);
        rect_ring(draw_r, 12.0, 1.8, fade(INK, 0.5), 0.2);
    }
    hot && clicked()
}

/// A primary button is solid white with black text; the others are an outline. Disabled ones go grey.
fn button(r: Rect, label: &str, on: bool, primary: bool) -> bool {
    let hot = on && r.contains(mouse()) && !modal();
    let down = hot && is_mouse_button_down(MouseButton::Left);
    button_face(r, label, on, primary, hot, down);
    if hot && clicked() {
        play(Sfx::Click);
        rect_ring(r, 9.0, 1.5, fade(INK, 0.45), 0.16);
    }
    hot && clicked()
}

/// How a button looks: shared by ordinary buttons and the ones on modal panels.
fn button_face(r: Rect, label: &str, on: bool, primary: bool, hot: bool, down: bool) {
    let h = hover_lerp(r, hot);
    let dy = if down { 1.5 } else { -h };
    let r = Rect::new(r.x, r.y + dy, r.w, r.h);
    let (fill, edge, ink) = match (on, primary) {
        (false, _) => (BG, FAINT, MUTED),
        (true, true) => (mix(INK, rgb(0xffffff), h), mix(INK, rgb(0xffffff), h), OUT),
        (true, false) => (mix(BG, CARD_HI, h), INK, INK),
    };
    block(r, fill);
    outline(r, edge);
    if on && h > 0.05 {
        corner_ticks(r, h, if primary { rgb(0xffffff) } else { INK });
    }
    let d = measure(label, 18.0);
    text(label, r.x + (r.w - d.width) / 2.0, r.y + r.h / 2.0 + 6.0, 18.0, ink);
}

/// Draws tile `idx` of the sprite sheet as a `size` square.
fn sprite(idx: usize, x: f32, y: f32, size: f32, tint: Color) {
    sprite_rect(idx, Rect::new(x, y, size, size), tint);
}

/// Draws sprite `idx` stretched over `r`: a Tiny Dungeon tile, or from `HD` up a character.
fn sprite_rect(idx: usize, r: Rect, tint: Color) {
    if tint.a > 0.05 {
        drawn(grow(r, -2.0), || format!("sprite {idx}"));
    }
    // sample a hair inside the tile: at fractional positions (the idle bob, window scaling,
    // anti-aliased edges) the exact border would pick up a row of the neighbouring tile
    const INSET: f32 = 0.1;
    if idx >= HD {
        let k = idx - HD;
        let source = Rect::new((k % 8) as f32 * 128.0, (k / 8) as f32 * 128.0, 128.0, 128.0);
        // the sigils are still, so characters (the first 20, the shopkeeper, the endings' enemies) breathe:
        // a slow squash from the feet up. Map and charm icons hold still.
        let alive = k < 20 || k == 27 || (67..82).contains(&k);
        let b = if alive { 0.025 * (1.0 + (now() as f32 * 2.4 + k as f32 * 1.3).sin()) } else { 0.0 };
        // the atlas is white; any colour tint from a call site becomes its brightness, so nothing turns colourful
        let v = tint.r.max(tint.g).max(tint.b);
        let tint = Color::new(v, v, v, tint.a);
        return art(1, source, Rect::new(r.x - r.w * b / 2.0, r.y + r.h * b, r.w * (1.0 + b), r.h * (1.0 - b)), tint);
    }
    let source = Rect::new((idx % 12) as f32 * 16.0 + INSET, (idx / 12) as f32 * 16.0 + INSET, 16.0 - 2.0 * INSET, 16.0 - 2.0 * INSET);
    art(0, source, r, tint);
}

/// Each element's alchemical sign as strokes, in a 20 x 20 box centred on (10, 10):
/// fire a triangle, water one upside down, air and earth the same with a bar, spark a bolt.
/// Fire and water carry a dot at their heart and the bars reach well past the triangle, so each
/// pair still tells apart at chip size.
fn sign_strokes(e: Element) -> Vec<Vec<(f32, f32)>> {
    let up = vec![(10.0, 3.0), (18.0, 17.0), (2.0, 17.0), (10.0, 3.0)];
    let down = vec![(10.0, 17.0), (18.0, 3.0), (2.0, 3.0), (10.0, 17.0)];
    match e {
        Element::Ember => vec![up, vec![(10.0, 11.5), (10.0, 13.0)]],
        Element::Tide => vec![down, vec![(10.0, 7.0), (10.0, 8.5)]],
        Element::Gale => vec![up, vec![(0.5, 12.0), (19.5, 12.0)]],
        Element::Stone => vec![down, vec![(0.5, 8.0), (19.5, 8.0)]],
        Element::Spark => vec![vec![(12.0, 2.0), (5.0, 11.0), (10.0, 11.0), (8.0, 18.0), (15.0, 9.0), (10.0, 9.0), (12.0, 2.0)]],
    }
}

fn count(v: &[Element], e: Element) -> usize {
    v.iter().filter(|&&x| x == e).count()
}

/// The gems in `slotted` beyond what `cost` asks for: the ones paying a toll.
fn paid_toll(cost: &[Element], slotted: &[Element]) -> Vec<Element> {
    let mut rest = slotted.to_vec();
    for e in cost {
        if let Some(k) = rest.iter().position(|x| x == e) {
            rest.remove(k);
        }
    }
    rest
}

/// The toll's own mark, unlike any element's sign: a ring cut by a bar. Any gem fills its socket.
fn toll_sign(p: Vec2, r: f32, c: Color) {
    let k = r / 9.0;
    let w = (1.8 * k).max(1.5);
    draw_circle_lines(p.x, p.y, 7.0 * k, w, c);
    draw_line(p.x - 9.5 * k, p.y, p.x + 9.5 * k, p.y, w, c);
}

/// A gold shard, the currency.
fn shard(p: Vec2, r: f32) {
    draw_poly(p.x, p.y, 4, r + 2.0, 0.0, OUT);
    draw_poly(p.x, p.y, 4, r, 0.0, GOLD_C);
    draw_poly(p.x - r * 0.3, p.y - r * 0.3, 4, r * 0.35, 0.0, INK);
}

/// An element drawn as its alchemical sign, `r` from the centre to the box edge.
fn gem(e: Element, p: Vec2, r: f32) {
    sign(e, p, r, el_color(e));
}

/// An element's hue, the one its rules word is printed in. Signs take only a hint of it (see `sign`).
fn hue(e: Element) -> Color {
    match e {
        Element::Ember => rgb(0xff8a4c),
        Element::Tide => rgb(0x6fb6ff),
        Element::Gale => rgb(0x5fe0c8),
        Element::Stone => rgb(0xc9a77c),
        Element::Spark => rgb(0xffe066),
    }
}

/// The same sign in colour `c`, for greyed-out and highlighted ones.
fn sign(e: Element, p: Vec2, r: f32, c: Color) {
    let k = r / 9.0;
    if c.a > 0.05 {
        // the strokes' own extent, inside the 20 x 20 box (Gale's and Stone's bars stop at its edges, so signs in a row never touch)
        let pts: Vec<(f32, f32)> = sign_strokes(e).concat();
        let (x0, x1) = pts.iter().fold((f32::MAX, f32::MIN), |(a, b), p| (a.min(p.0), b.max(p.0)));
        let (y0, y1) = pts.iter().fold((f32::MAX, f32::MIN), |(a, b), p| (a.min(p.1), b.max(p.1)));
        drawn(Rect::new(p.x + (x0 - 10.0) * k, p.y + (y0 - 10.0) * k, (x1 - x0) * k, (y1 - y0) * k), || format!("{e:?} sign"));
    }
    // a lit sign is white with a hint of its element, so the screen stays black and white
    let c = if c == INK { mix(INK, hue(e), 0.3) } else { c };
    let w = (1.8 * k).max(1.5);
    let at = |(x, y): (f32, f32)| p + vec2(x - 10.0, y - 10.0) * k;
    for line in sign_strokes(e) {
        for pair in line.windows(2) {
            let (a, b) = (at(pair[0]), at(pair[1]));
            draw_line(a.x, a.y, b.x, b.y, w, c);
            // round joints, so corners have no notches
            draw_circle(b.x, b.y, w / 2.0, c);
        }
    }
}

pub fn status(run: &Run) {
    plate(Rect::new(24.0, 14.0, 1200.0, 44.0), CARD);
    sprite(HEROES[run.hero].sprite, 36.0, 20.0, 32.0, WHITE);
    text(HEROES[run.hero].name, 78.0, 43.0, 22.0, INK);
    // hero level and compact EXP progress bar
    let lv_r = Rect::new(224.0, 18.0, 74.0, 36.0);
    text(&format!("Lv {}", run.hero_lvl), lv_r.x, 38.0, 17.0, GOLD_C);
    let need = run.xp_next().max(1);
    let frac = (run.hero_xp as f32 / need as f32).clamp(0.0, 1.0);
    let xp_bar = Rect::new(lv_r.x, 45.0, 62.0, 5.0);
    block(xp_bar, WELL);
    if frac > 0.0 {
        block(Rect::new(xp_bar.x, xp_bar.y, xp_bar.w * frac, xp_bar.h), GOLD);
    }
    outline(xp_bar, FAINT);
    if !modal() && lv_r.contains(mouse()) {
        let mut tip = format!("EXP {}/{need} · Trait Rank {}", run.hero_xp, run.trait_rank + 1);
        if run.trait_rank > 0 {
            tip += &format!("\nDeepened x{}: {}", run.trait_rank, HEROES[run.hero].trait_up_desc);
        }
        tip_text(&format!("{} · Level {}", HEROES[run.hero].name, run.hero_lvl), tip);
    }
    let mut x = 312.0;
    for (label, value, c) in [
        ("HP", format!("{}/{}", run.hp, run.max_hp()), GOOD),
        ("Shards", run.shards.to_string(), GOLD_C),
        ("Floor", format!("{}/{}", run.floor + 1, run.floors()), INK),
        ("Pacts", run.charms.len().to_string(), ACCENT),
    ] {
        text(label, x, 43.0, 17.0, MUTED);
        let w = measure(label, 17.0).width;
        text(&value, x + w + 7.0, 43.0, 19.0, c);
        x += 148.0;
    }
    // sets two or more pacts deep, and active Crucible relic if carried, before the Retreat button
    let mut right = 1066.0;
    if let Some(relic) = run.crucible {
        let tag = "Crucible";
        let tw = measure(tag, 15.0).width + 14.0;
        let cr = Rect::new(right - tw, 24.0, tw, 24.0);
        block(cr, WELL);
        outline(cr, GOLD);
        centered(tag, cr.center().x, cr.y + 17.0, 15.0, GOLD);
        if !modal() && cr.contains(mouse()) {
            let body = format!(
                "+ {}\n+ {}\n- {}",
                CRUCIBLE_BOONS[relic.boons[0]].desc, CRUCIBLE_BOONS[relic.boons[1]].desc, CRUCIBLE_CURSES[relic.curse].desc
            );
            tip_text(&relic.name(), body);
        }
        right = cr.x - 14.0;
    }
    for (k, set) in SETS.iter().enumerate().rev() {
        let n = run.set_count(k);
        if n >= 2 {
            let s = format!("{} {}/{SET_AT}", set.name, n.min(SET_AT));
            if right - measure(&s, 15.0).width < 888.0 {
                break;
            }
            right -= measure(&s, 15.0).width;
            text(&s, right, 42.0, 15.0, if n >= SET_AT { GOLD_C } else { MUTED });
            right -= 16.0;
        }
    }
}


/// Pact `id`'s set, how many of it `held` has, and what the set does at `SET_AT`.
fn set_line(held: &[usize], id: usize) -> (String, bool) {
    let set = CHARMS[id].set;
    let n = held.iter().filter(|&&c| CHARMS[c].set == set).count();
    (format!("{} {}/{SET_AT}: {}", set.def().name, n.min(SET_AT), set.def().bonus), n >= SET_AT)
}

/// `line` at `size`, shrunk until it fits in `room`.
fn fit_text(line: &str, x: f32, y: f32, room: f32, size: f32, c: Color) {
    text(line, x, y, size * (room / measure(line, size).width).min(1.0), c);
}

fn rarity_color(r: Rarity) -> Color {
    match r {
        Rarity::Common => MUTED,
        Rarity::Rare => rgb(0x7fc0ff),
        Rarity::Legendary => GOLD,
    }
}

/// A pact's picture in a thin frame; it lights up while it takes effect.
fn charm_icon(id: usize, r: Rect, lit: f32) {
    let c = &CHARMS[id];
    block(r, mix(BG, INK, lit * 0.3));
    outline(r, if lit > 0.0 { INK } else { FAINT });
    let pad = r.w * 0.12;
    sprite(c.icon.0, r.x + pad, r.y + pad, r.w - 2.0 * pad, rgb(c.icon.1));
}

/// How many slots the battle's charm bar has; past that, the last one becomes "+N".
const BAR_SLOTS: usize = 10;

/// Where charm `k` of `n` sits in the battle's charm bar. Charms with no room share the "+N" slot.
fn charm_slot(k: usize, n: usize) -> Rect {
    let k = if n > BAR_SLOTS { k.min(BAR_SLOTS - 1) } else { k };
    Rect::new(56.0 + k as f32 * 38.0, 56.0, 32.0, 32.0)
}

/// The "+N" slot standing in for pacts there is no room for. Hovering it shows every pact's icon.
fn more_slot(r: Rect, hidden: usize, all: &[usize]) {
    let hot = r.contains(mouse()) && !modal();
    block(r, if hot { CARD_HI } else { BG });
    outline(r, if hot { INK } else { FAINT });
    centered(&format!("+{hidden}"), r.center().x, r.center().y + 5.0, 15.0, INK);
    if !hot {
        return;
    }
    let (cols, s, pad) = (8.min(all.len()), 34.0, 12.0);
    let rows = all.len().div_ceil(cols);
    let (w, h) = (cols as f32 * s + 2.0 * pad - 4.0, rows as f32 * s + 2.0 * pad - 4.0);
    // below the slot, or above it when there is no room underneath
    let y = if r.bottom() + 12.0 + h > VIEW.y - 8.0 { r.y - 12.0 - h } else { r.bottom() + 12.0 };
    let panel = Rect::new(r.x.min(VIEW.x - w - 20.0), y, w, h);
    plate(panel, CARD);
    for (k, &id) in all.iter().enumerate() {
        charm_icon(id, Rect::new(panel.x + pad + (k % cols) as f32 * s, panel.y + pad + (k / cols) as f32 * s, 30.0, 30.0), 0.0);
    }
}

/// Your charms along the top of a battle. Each one jumps and glows when it takes effect;
/// hovering one shows what it does.
fn charm_bar(charms: &[usize]) {
    let now = now();
    let times = fx(|fx| fx.charm_t.clone());
    let mut tip = None;
    let shown = if charms.len() > BAR_SLOTS { BAR_SLOTS - 1 } else { charms.len() };
    for (k, &id) in charms.iter().enumerate().take(shown) {
        let since = times.get(&id).map_or(9.0, |t| (now - t) as f32);
        let lit = (1.0 - since / 0.7).max(0.0);
        let r = charm_slot(k, charms.len());
        let jump = if since < 0.35 { (since / 0.35 * std::f32::consts::PI).sin() * 7.0 } else { 0.0 };
        let r = Rect::new(r.x, r.y - jump, r.w, r.h);
        charm_icon(id, r, lit);
        if r.contains(mouse()) {
            tip = Some((id, r));
        }
    }
    if shown < charms.len() {
        more_slot(charm_slot(shown, charms.len()), charms.len() - shown, charms);
    }
    if let Some((id, r)) = tip {
        let c = &CHARMS[id];
        let panel = Rect::new(r.x.min(VIEW.x - 330.0), r.bottom() + 12.0, 310.0, 108.0);
        plate(panel, CARD);
        pact_name(id, panel.x + 14.0, panel.y + 24.0, 16.0, INK);
        rich(c.rarity.name(), panel.right() - 14.0 - measure(c.rarity.name(), 16.0).width, panel.y + 24.0, 16.0, rarity_color(c.rarity));
        wrapped(c.desc, panel.x + 14.0, panel.y + 48.0, panel.w - 28.0, 16.0, MUTED);
        let (line, awake) = set_line(charms, id);
        fit_text(&line, panel.x + 14.0, panel.bottom() - 12.0, panel.w - 28.0, 14.0, if awake { GOLD_C } else { MUTED });
    }
}

/// What rite `c` of `f` does besides damage that the screen would not otherwise say, as (on the
/// foe, word) pairs: power, thorns, surge, picks and gems.
fn callouts(f: &Fighter, c: &Combo) -> Vec<(bool, String)> {
    f.effects(c)
        .filter_map(|(e, l)| match e {
            Effect::Stoke(n) => Some((false, format!("+{n} POWER"))),
            Effect::Spikes(n) => Some((false, format!("+{} THORNS", at_level(n, l)))),
            Effect::Surge(v) => Some((false, format!("+{} SURGE", at_level(v, l)))),
            Effect::Blind(n) => Some((true, format!("-{n} PICKS"))),
            Effect::Drain(n) => Some((true, format!("-{} GEMS", at_level(n as i32, l)))),
            Effect::Armor(n) => Some((false, format!("+{} ARMOR", at_level(n, l)))),
            Effect::Regen(n) => Some((false, format!("+{} REGEN", at_level(n, l)))),
            Effect::Gems(n) => Some((false, format!("+{n} GEMS"))),
            Effect::Defuse(n) => Some((true, format!("-{} LOADED", at_level(n as i32, l)))),
            _ => None,
        })
        .collect()
}

/// What the title and the overworld ask the game to do next.
pub enum Go {
    Play,
    Back,
    /// today's daily descent
    Daily,
    /// the gallery (debug builds)
    Gallery,
}

/// What the gallery asks for: a song, a boss fight (true: the first ending's), or the title again.
pub enum Shown {
    Track(usize),
    Fight(bool),
    Back,
}

/// Debug builds: every song to play by name, and ways into the fights with the big animations.
/// `tracks`: (file, seconds, mood 0-2, place: floors 0-2, endings 3-5, 6 menus), as main.rs lists them.
pub fn gallery(tracks: &[(&str, f64, u8, u8)], playing: Option<usize>) -> Option<Shown> {
    centered("GALLERY", 640.0, 62.0, 44.0, INK);
    let places = ["Floor 1", "Floor 2", "Floor 3", "Archive", "Furnace", "Engine", "Menus"];
    for (k, head) in ["CALM", "FIGHT", "BOSS"].iter().enumerate() {
        label(head, 150.0 + k as f32 * 276.0, 104.0);
    }
    let mut go = None;
    let mut menus = 0;
    for (i, &(name, ..)) in tracks.iter().enumerate() {
        let (mood, place) = (tracks[i].2 as usize, tracks[i].3 as usize);
        // menu tracks are all calm: they fill their row left to right
        let col = if place == 6 { (menus, menus += 1).0 } else { mood };
        let y = 116.0 + place as f32 * 70.0;
        if col == 0 {
            label(places[place], 40.0, y + 24.0);
        }
        let secs = tracks[i].1 as u32;
        if button(Rect::new(150.0 + col as f32 * 276.0, y, 268.0, 36.0), name, true, playing == Some(i)) {
            go = Some(Shown::Track(i));
        }
        centered(&format!("{}:{:02}", secs / 60, secs % 60), 150.0 + col as f32 * 276.0 + 134.0, y + 54.0, 14.0, MUTED);
    }
    label("ANIMATIONS", 990.0, 104.0);
    if button(Rect::new(990.0, 116.0, 240.0, 40.0), "Boss fight", true, false) {
        go = Some(Shown::Fight(false));
    }
    if button(Rect::new(990.0, 164.0, 240.0, 40.0), "Ending boss fight", true, false) {
        go = Some(Shown::Fight(true));
    }
    wrapped("In a fight, the buttons by Surrender: Hit 130 shows a huge hit's impact frames, Break breaks the enemy's load, Win ends the fight.", 990.0, 232.0, 240.0, 16.0, MUTED);
    if button(Rect::new(990.0, 640.0, 240.0, 44.0), "Back", true, false) {
        go = Some(Shown::Back);
    }
    go
}

/// Debug builds, mid-fight: 0 a huge hit on the enemy, 1 its load broken, 2 the fight won.
pub fn debug_bar() -> Option<u8> {
    if modal() || fx(|fx| fx.impact.is_some()) {
        return None;
    }
    // between Surrender and the board's left edge (472)
    [("Hit 130", 216.0, 84.0), ("Break", 308.0, 76.0), ("Win", 392.0, 60.0)]
        .iter()
        .enumerate()
        .find_map(|(k, &(s, x, w))| button(Rect::new(x, 652.0, w, 36.0), s, true, false).then_some(k as u8))
}

/// A four-point star of radius `r`, turned by `spin` radians.
fn sparkle(p: Vec2, r: f32, spin: f32, c: Color) {
    for k in 0..4 {
        let a = spin + k as f32 * std::f32::consts::FRAC_PI_2;
        let (tip, side) = (Vec2::from_angle(a) * r, Vec2::from_angle(a + std::f32::consts::FRAC_PI_2) * r * 0.22);
        draw_triangle(p + side, p - side, p + tip, c);
    }
}

/// A summoning circle lying flat on the floor: two rings, ticks and runes, slowly turning.
fn rune_circle(c: Vec2, rx: f32, ry: f32, col: Color, a: f32) {
    let t = now() as f32;
    let at = |ang: f32, k: f32| c + vec2(ang.cos() * rx * k, ang.sin() * ry * k);
    // rings drawn as many short segments; macroquad's own ellipse outline shows corners this wide
    for (k, w, f) in [(1.0, 2.0, 1.0), (0.86, 1.0, 0.7), (0.55, 1.0, 0.5)] {
        const SEG: usize = 96;
        for i in 0..SEG {
            let (p, q) = (at(i as f32 * std::f32::consts::TAU / SEG as f32, k), at((i + 1) as f32 * std::f32::consts::TAU / SEG as f32, k));
            draw_line(p.x, p.y, q.x, q.y, w, fade(col, a * f));
        }
    }
    for k in 0..48 {
        let ang = t * 0.12 + k as f32 * std::f32::consts::TAU / 48.0;
        let (p, q) = (at(ang, 0.86), at(ang, if k % 4 == 0 { 1.0 } else { 0.93 }));
        draw_line(p.x, p.y, q.x, q.y, 1.0, fade(col, a * 0.6));
    }
    // a counter-turning triangle inside
    for k in 0..3 {
        let ang = -t * 0.08 + k as f32 * std::f32::consts::TAU / 3.0;
        let (p, q) = (at(ang, 0.55), at(ang + std::f32::consts::TAU / 3.0, 0.55));
        draw_line(p.x, p.y, q.x, q.y, 1.0, fade(col, a * 0.5));
    }
    for k in 0..8 {
        let p = at(t * 0.12 + k as f32 * std::f32::consts::TAU / 8.0 + 0.2, 0.705);
        sparkle(p, 4.0, 0.0, fade(mix(col, INK, 0.4), a * (0.6 + 0.4 * (t * 2.0 + k as f32).sin())));
    }
}

/// The title menu: Play goes to the overworld, Back quits the game.
/// `deeds`: those the save has done, for which heroes stand in the circle.
pub fn title(deeds: u32) -> Option<Go> {
    let t = now() as f32;
    ambient_motes(18, 11.0, 0.22);
    // the lords of The Below watch from the dark either side, the nearer ones larger and less faint
    let lurk = [(150.0, 150.0, 90.0), (70.0, 330.0, 70.0), (250.0, 310.0, 110.0), (120.0, 500.0, 100.0), (270.0, 580.0, 80.0), (60.0, 660.0, 64.0)];
    for (k, d) in BOSSES.iter().flat_map(|f| f.iter()).enumerate().take(12) {
        let (x, y, size) = lurk[k / 2];
        let x = if k % 2 == 0 { x } else { 1280.0 - x };
        let bob = (t * 0.6 + k as f32 * 1.3).sin() * 4.0;
        let breathe = 0.75 + 0.25 * (t * 0.4 + k as f32).sin();
        sprite(d.sprite, x - size / 2.0, y - size / 2.0 + bob, size, fade(INK, size / 110.0 * 0.2 * breathe));
    }
    // the heroes waiting in a summoning circle
    rune_circle(vec2(640.0, 640.0), 280.0, 40.0, ACCENT, 0.28);
    for k in 0..4 {
        // sparks drifting up out of the circle
        let f = k as f32;
        let up = (t * 0.25 + f * 0.37).fract();
        let p = vec2(640.0 + ((f * 71.0) % 520.0 - 260.0) + (t + f).sin() * 8.0, 640.0 - up * 150.0);
        sparkle(p, 3.0, t + f, fade(mix(ACCENT, INK, 0.5), 0.4 * (1.0 - up)));
    }
    // only the heroes this save has: one a deed opens joins the circle once it is done
    let heroes: Vec<&Hero> = HEROES.iter().filter(|h| is_open(deeds, Unlock::Hero(h.name))).collect();
    for (i, h) in heroes.iter().enumerate() {
        let x = 640.0 + (i as f32 - (heroes.len() - 1) as f32 / 2.0) * 135.0;
        let hit_r = Rect::new(x - 48.0, 540.0, 96.0, 108.0);
        let hot = !modal() && hit_r.contains(mouse());
        let hv = hover_lerp(hit_r, hot);
        draw_ellipse(x, 640.0, 40.0 + 6.0 * hv, 7.0 + 1.5 * hv, 0.0, fade(OUT, 0.6));
        if hv > 0.05 {
            draw_ellipse_lines(x, 640.0, 44.0 + 6.0 * hv, 9.0, 0.0, 1.2, fade(INK, 0.45 * hv));
        }
        sprite(h.sprite, x - 48.0, 548.0 - 8.0 * hv + (t * 2.0 + i as f32 * 2.1).sin() * 3.0, 96.0, WHITE);
        if hv > 0.15 {
            centered(h.name, x, 668.0, 14.0, fade(INK, hv));
        }
    }
    // the name drops in once, as the game opens; coming back from the map it is simply there
    let t0 = fx(|fx| fx.screen_t);
    let rise = if fx(|fx| fx.fade) > 0.0 { 0.0 } else { 1.0 - ease_out3((now() / 0.6) as f32) };
    lively_centered("MINEBOUND", 640.0, 140.0 - rise * 30.0, 92.0, INK, Motion::Wave);
    centered("Mine your boards for power. Spend it before theirs spends you.", 640.0, 184.0, 20.0, MUTED);
    let mut go = None;
    let slide = |k: usize| (1.0 - stagger(t0, k, 0.05, 0.24)) * 14.0;
    if button(Rect::new(540.0, 222.0 + slide(0), 200.0, 48.0), "Play", true, true) {
        go = Some(Go::Play);
    }
    if button(Rect::new(540.0, 284.0 + slide(1), 200.0, 44.0), "Settings", true, false) {
        SETTINGS.with(|s| s.set(true));
        start_modal_anim();
    }
    if button(Rect::new(540.0, 342.0 + slide(2), 200.0, 44.0), "Quit", true, false) {
        go = Some(Go::Back);
    }
    if cfg!(debug_assertions) && button(Rect::new(540.0, 400.0 + slide(3), 200.0, 44.0), "Gallery", true, false) {
        go = Some(Go::Gallery);
    }
    go
}

/// Each descent's place on the map of The Below.
const PLACES: [Vec2; 6] = [vec2(170.0, 560.0), vec2(330.0, 410.0), vec2(520.0, 520.0), vec2(610.0, 300.0), vec2(790.0, 430.0), vec2(820.0, 180.0)];
/// The daily descent's medallion, on an island of its own in the north-west.
const DAILY_AT: Vec2 = vec2(110.0, 190.0);
/// The land is a grid of tiles: (left, top), tile pitch, columns, rows.
const LAND: (f32, f32, f32, usize, usize) = (40.0, 110.0, 22.0, 39, 27);

/// A fixed pseudo-random number in 0..1 for tile (c, r), so the land looks the same every frame.
fn noise(c: usize, r: usize) -> f32 {
    let s = (c as f64 * 12.9898 + r as f64 * 78.233).sin() * 43758.5453;
    (s - s.floor()) as f32
}

/// A grid of land tiles: (left, top), tile pitch, columns, rows.
type Grid = (f32, f32, f32, usize, usize);

/// What each tile of a grid is: 0 open air, 1 unexplored land, 2 land explored (where `explored` says so).
/// The land is soft blobs `(x, y, radius)` minus chasms, its edge roughened per tile.
fn terrain(grid: Grid, blobs: &[(f32, f32, f32)], chasms: &[(f32, f32, f32)], explored: impl Fn(Vec2) -> bool) -> Vec<Vec<u8>> {
    let (x0, y0, cell, cols, rows) = grid;
    let pull = |set: &[(f32, f32, f32)], p: Vec2| set.iter().map(|&(x, y, r)| r * r / ((p.x - x).powi(2) + (p.y - y).powi(2) + 1.0)).sum::<f32>();
    (0..rows)
        .map(|r| {
            (0..cols)
                .map(|c| {
                    let p = vec2(x0 + c as f32 * cell + cell / 2.0, y0 + r as f32 * cell + cell / 2.0);
                    let field = pull(blobs, p) - pull(chasms, p) + (noise(c, r) - 0.5) * 0.5;
                    if field <= 0.9 {
                        0
                    } else if explored(p) {
                        2
                    } else {
                        1
                    }
                })
                .collect()
        })
        .collect()
}

/// What each tile of The Below is. The land is a few soft blobs round the places, minus three chasms;
/// it lies explored round each open place.
fn land(unlocked: usize) -> Vec<Vec<u8>> {
    let mut blobs: Vec<(f32, f32, f32)> = PLACES.iter().map(|p| (p.x, p.y, 62.0)).collect();
    // the daily descent's island, apart from the rest
    blobs.push((DAILY_AT.x, DAILY_AT.y, 50.0));
    blobs.extend([(250.0, 500.0, 55.0), (430.0, 470.0, 58.0), (470.0, 330.0, 48.0), (700.0, 330.0, 52.0), (690.0, 520.0, 44.0), (690.0, 200.0, 46.0), (100.0, 470.0, 40.0), (880.0, 300.0, 40.0), (250.0, 300.0, 36.0), (560.0, 640.0, 34.0)]);
    let chasms = [(560.0, 400.0, 58.0), (230.0, 640.0, 40.0), (880.0, 560.0, 46.0)];
    terrain(LAND, &blobs, &chasms, |p| PLACES[..unlocked].iter().any(|q| q.distance(p) < 105.0) || DAILY_AT.distance(p) < 40.0)
}

/// Draws land from `terrain`: unexplored tiles raised grey, explored ones turned over with their edge
/// numbered like a half-solved board (about one hidden tile in five counts as a mine). `seed` picks
/// which tiles are mines and their shades, so each map has its own.
/// `clear`: where rooms and their labels sit, kept free of numbers so a label never reads as "Battle 1".
fn draw_land(kind: &[Vec<u8>], grid: Grid, seed: usize, clear: &[Rect]) {
    let (x0, y0, cell, cols, rows) = grid;
    let at = |c: usize, r: usize| kind.get(r).and_then(|row| row.get(c)).copied().unwrap_or(0);
    let mine = |c: usize, r: usize| at(c, r) == 1 && noise(c + 50 + seed, r + 50) < 0.22;
    // all the tiles first, then all the numbers: each switch between shapes and text is a new GPU
    // draw call with buffers of its own, and hundreds of them made the driver take ~650 MB
    let mut numbers = vec![];
    for r in 0..rows {
        for c in 0..cols {
            let t = Rect::new(x0 + c as f32 * cell, y0 + r as f32 * cell, cell - 3.0, cell - 3.0);
            match at(c, r) {
                1 => block(t, if noise(c + 9 + seed, r + 3) < 0.5 { rgb(0x3a3a3a) } else { rgb(0x333333) }),
                2 => {
                    draw_rectangle_lines(t.x, t.y, t.w, t.h, 1.0, rgb(0x262626));
                    let n = (0..9).filter(|k| mine((c + k % 3).wrapping_sub(1), (r + k / 3).wrapping_sub(1))).count();
                    if n > 0 && !clear.iter().any(|c| c.overlaps(&t)) {
                        numbers.push((n, t));
                    }
                }
                _ => {}
            }
        }
    }
    // sized with the tiles: 14 on The Below's 22 px tiles
    let size = (14.0 * cell / 22.0).round();
    for (n, t) in numbers {
        centered(&n.to_string(), t.center().x, t.y + cell * 15.0 / 22.0, size, if n > 1 { INK } else { MUTED });
    }
    // the land is backdrop: nodes, labels and the medallion are meant to sit on it
    overlay();
}

/// A dotted line, like the route on The Below.
fn dotted(a: Vec2, b: Vec2, gap: f32, r: f32, c: Color) {
    let n = (a.distance(b) / gap) as usize;
    for k in 0..n {
        let p = a.lerp(b, k as f32 / n as f32);
        draw_circle(p.x, p.y, r, c);
    }
}

/// A dotted route whose dots gently march forward from `a` toward `b`.
fn dotted_flow(a: Vec2, b: Vec2, gap: f32, r: f32, c: Color, speed: f32) {
    let dist = a.distance(b).max(1.0);
    let n = (dist / gap) as usize;
    let phase = (now() as f32 * speed).fract();
    for k in 0..n {
        let q = (k as f32 + phase) / n as f32;
        if q < 1.0 {
            let p = a.lerp(b, q);
            draw_circle(p.x, p.y, r, c);
        }
    }
}

/// The world map, The Below: pick an open descent and a bearer, then descend.
/// `pick` is (descent, hero) and lasts between visits.
pub fn world(save: &mut Save, pick: &mut (usize, usize), today: u64) -> Option<Go> {
    let mut go = None;
    text("The Below", 48.0, 70.0, 44.0, INK);
    let found = save.endings.count_ones();
    text(&format!("{} of {} descents open  ·  secret endings found: {found} of {}", save.unlocked, EXPEDITIONS.len(), ENDINGS.len()), 50.0, 96.0, 15.0, MUTED);
    let tx = 48.0 + measure("The Below", 44.0).width + 28.0;
    if button(Rect::new(tx, 40.0, 110.0, 36.0), "< Title", true, false) {
        go = Some(Go::Back);
    }
    let fresh = save.new_deeds().count_ones();
    let deeds = if fresh > 0 { format!("Deeds · {fresh} new") } else { format!("Deeds {} of {}", save.deeds.count_ones(), DEEDS.len()) };
    if button(Rect::new(tx + 122.0, 40.0, 170.0, 36.0), &deeds, true, fresh > 0) {
        DEEDS_ON.with(|d| d.set(true));
        start_modal_anim();
    }
    if button(Rect::new(tx + 304.0, 40.0, 170.0, 36.0), &format!("Forge · {} ore", save.ore), true, false) {
        FORGE_ON.with(|f| f.set(true));
        start_modal_anim();
    }
    if let Some(relic) = save.crucible {
        let name = relic.name();
        let bw = (measure(&name, 14.0).width + 24.0).clamp(110.0, 220.0);
        let br = Rect::new(tx + 486.0, 44.0, bw, 28.0);
        block(br, CARD);
        outline(br, GOLD);
        fit_text(&name, br.x + 10.0, br.y + 19.0, br.w - 20.0, 14.0, GOLD);
        if !modal() && br.contains(mouse()) {
            let body = format!(
                "+ {}\n+ {}\n- {}\nConsumed on your next non-daily descent.",
                CRUCIBLE_BOONS[relic.boons[0]].desc, CRUCIBLE_BOONS[relic.boons[1]].desc, CRUCIBLE_CURSES[relic.curse].desc
            );
            tip_text(&format!("Crucible · {name}"), body);
        }
    }

    // the land: unexplored tiles are raised grey; around each open place it lies turned over, its
    // edge numbered like a half-solved board (about one hidden tile in five counts as a mine)
    // the land only changes when a descent opens, so build it once per `unlocked`
    thread_local!(static LAND_AT: std::cell::RefCell<(usize, Vec<Vec<u8>>)> = const { std::cell::RefCell::new((usize::MAX, Vec::new())) });
    let kind = LAND_AT.with(|l| {
        let mut l = l.borrow_mut();
        if l.0 != save.unlocked {
            *l = (save.unlocked, land(save.unlocked));
        }
        l.1.clone()
    });
    draw_land(&kind, LAND, 0, &[]);
    // the route between places, dotted; open legs gently march toward the next descent
    for (idx, w) in PLACES.windows(2).enumerate() {
        if idx + 1 < save.unlocked {
            dotted_flow(w[0], w[1], 9.0, 1.2, INK, 0.7);
        } else {
            dotted(w[0], w[1], 9.0, 1.0, fade(MUTED, 0.65));
        }
    }

    let t = now() as f32;
    ambient_motes(10, 23.0, 0.14);
    let mut shown = pick.0;
    for (i, x) in EXPEDITIONS.iter().enumerate() {
        let p = PLACES[i];
        let open = i < save.unlocked;
        let hot = (mouse() - p).length() < 34.0 && !modal();
        if hot {
            shown = i;
        }
        let chosen = i == pick.0 && !DAILY_ON.with(|d| d.get());
        let edge = if open { INK } else { MUTED };
        let hv = hover_lerp(Rect::new(p.x - 32.0, p.y - 32.0, 64.0, 64.0), hot && open);
        let r = 32.0 + 4.0 * hv;
        if chosen {
            let pulse = (t * 1.1).fract();
            draw_poly_lines(p.x, p.y, 4, r + 4.0 + pulse * 14.0, 0.0, 1.4, fade(INK, (1.0 - pulse) * 0.65));
        }
        draw_poly(p.x, p.y, 4, r, 0.0, if chosen { INK } else { BG });
        draw_poly_lines(p.x, p.y, 4, r, 0.0, 1.5, edge);
        let numeral = ["I", "II", "III", "IV", "V", "VI"][i];
        stamped(numeral, p.x, p.y + 9.0, 26.0, if chosen { OUT } else { edge });
        // labels sit on a dark strip so they read over the land
        let cleared = (0..HEROES.len()).any(|h| save.best[i][h] >= CLEARED);
        let name = if open { x.name } else { "unknown" };
        let lw = measure(name, 14.0).width.max(if cleared { measure("cleared", 13.0).width } else { 0.0 });
        block(Rect::new(p.x - lw / 2.0 - 6.0, p.y + 38.0, lw + 12.0, if cleared { 38.0 } else { 20.0 }), BG);
        centered(name, p.x, p.y + 52.0, 14.0, if open { INK } else { rgb(0x8a8a8a) });
        if cleared {
            centered("cleared", p.x, p.y + 70.0, 13.0, MUTED);
        }
        if hot && open && clicked() {
            play(Sfx::Click);
            pick.0 = i;
            DAILY_ON.with(|d| d.set(false));
            ring(p, 28.0, 56.0, 4, 0.0, 2.0, INK, 0.28);
        }
    }

    // today's descent, on its own island: the same run for everyone, as many tries as you like. Picked, it fills
    // the card on the right like any descent.
    thread_local!(static DAILY_ON: std::cell::Cell<bool> = const { std::cell::Cell::new(false) });
    let (dhero, dexp) = daily(today);
    let cleared = save.daily == (today, CLEARED);
    let daily_hot = {
        let p = DAILY_AT;
        let on = DAILY_ON.with(|d| d.get());
        let hot = (mouse() - p).length() < 34.0 && !modal();
        let r = if hot { 35.0 } else { 32.0 };
        if on {
            let pulse = (t * 1.1).fract();
            draw_circle_lines(p.x, p.y, r + 3.0 + pulse * 13.0, 1.4, fade(GOLD, (1.0 - pulse) * 0.7));
        }
        draw_circle(p.x, p.y, r, if on { GOLD } else if hot { CARD_HI } else { BG });
        draw_circle_lines(p.x, p.y, r, 2.0, GOLD);
        sprite(HEROES[dhero].sprite, p.x - 20.0, p.y - 20.0, 40.0, if on { BLACK } else { WHITE });
        // how deep a try got says little; only whether today's run was beaten
        let result = if cleared { "Daily · cleared" } else { "Daily" };
        let lw = measure(result, 14.0).width;
        block(Rect::new(p.x - lw / 2.0 - 6.0, p.y + 38.0, lw + 12.0, 20.0), BG);
        centered(result, p.x, p.y + 52.0, 14.0, GOLD);
        if hot && clicked() {
            play(Sfx::Click);
            DAILY_ON.with(|d| d.set(true));
            ring(p, 28.0, 56.0, 0, 0.0, 2.0, GOLD, 0.28);
        }
        hot
    };
    let daily_on = DAILY_ON.with(|d| d.get());
    // the card shows what is pointed at, else what is picked
    let show_daily = daily_hot || (daily_on && shown == pick.0);

    // the chosen (or pointed at) descent, and who carries it down
    let shown = if show_daily { dexp } else { shown };
    let x = &EXPEDITIONS[shown];
    let open = show_daily || shown < save.unlocked;
    draw_line(950.0, 48.0, 950.0, 672.0, 1.0, FAINT);
    let (lx, w) = (982.0, 256.0);
    if show_daily {
        lively("TODAY'S DESCENT", lx, 72.0, 15.0, GOLD, Motion::Wave);
    } else {
        label(&format!("DESCENT {}", ["I", "II", "III", "IV", "V", "VI"][shown]), lx, 72.0);
    }
    // the daily goes by its own name, shrunk to fit the column if it runs long
    let title = if show_daily { daily_name(today) } else if open { x.name.to_string() } else { "Unknown".to_string() };
    let size = 36.0f32.min(36.0 * w / measure(&title, 36.0).width);
    text(&title, lx, 112.0, size, if open { INK } else { MUTED });
    let blurb = if show_daily {
        format!("Down through {}. The same run for everyone today.", x.name.replace(' ', "\u{a0}"))
    } else {
        x.text.to_string()
    };
    // flavour gives way first, then who waits below; the law and how to open the way always stay
    let mut col = Column::new(lx, 140.0, w, 440.0, 15.0);
    col.block(0, None, [blurb], MUTED);
    let daily_law = ["Every floor is cursed."];
    let rules = x.rules.iter().filter(|r| !(show_daily && r.starts_with("No extra")));
    let laws: Vec<&str> = daily_law.iter().filter(|_| show_daily).chain(rules).copied().collect();
    let n = laws.len();
    col.block(3, Some("ITS LAW"), laws, INK).short(format!("{n} laws. Point here to read them."));
    if !open {
        // no-break spaces keep a place name whole, so "Ember" or "Mines" in it is not read as a rules word
        let prev = EXPEDITIONS[shown - 1].name.replace(' ', "\u{a0}");
        col.block(2, None, [format!("Win {prev} to open the way here.")], INK);
    } else {
        let lords = BOSSES.iter().map(|pair| pair.iter().map(|d| d.name.replace(' ', "\u{a0}")).collect::<Vec<_>>().join(", "));
        col.block(1, Some("LORDS BELOW"), lords, INK);
    }
    col.draw();

    // the bearer: one sigil per hero; the chosen one is outlined, with its numbers below. The daily's
    // bearer is set for the day.
    let by = 466.0;
    label(if show_daily { "BEARER · SET FOR TODAY" } else { "BEARER" }, lx, by);
    let bearer = if show_daily { dhero } else { pick.1 };
    for (i, h) in HEROES.iter().enumerate() {
        let r = Rect::new(lx + i as f32 * 52.0, by + 12.0, 46.0, 46.0);
        // a bearer a deed opens waits faint until it is done, saying which
        if !is_open(save.deeds, Unlock::Hero(h.name)) {
            outline(r, FAINT);
            sprite(h.sprite, r.x + 5.0, r.y + 5.0, 36.0, fade(WHITE, 0.08));
            if r.contains(mouse()) {
                let deed = DEEDS.iter().find(|d| d.opens == Unlock::Hero(h.name)).expect("a locked hero has its deed");
                tip_text(h.name, format!("Opens with the deed {}: {}", deed.name, deed.task));
            }
            continue;
        }
        let on = i == bearer;
        let hot = r.contains(mouse()) && !modal() && !show_daily;
        if hot {
            block(r, CARD_HI);
        }
        outline(r, if on { INK } else { FAINT });
        let dim = if show_daily { 0.15 } else { 0.45 };
        sprite(h.sprite, r.x + 5.0, r.y + 5.0, 36.0, if on || hot { WHITE } else { fade(WHITE, dim) });
        if hot && clicked() {
            play(Sfx::Click);
            pick.1 = i;
            fx(|fx| {
                fx.bearer_t = now();
                fx.last_bearer = i;
            });
            rect_ring(r, 10.0, 1.8, INK, 0.22);
        }
    }
    let bp = ease_out3(((now() - fx(|fx| fx.bearer_t)) as f32 / 0.18).clamp(0.0, 1.0));
    let bslide = (1.0 - bp) * 8.0;
    let h = &HEROES[bearer];
    text(h.name, lx + bslide, by + 94.0, 24.0, INK);
    let best = match save.best[pick.0][pick.1] {
        _ if show_daily => if cleared { "cleared today" } else { "not cleared today" }.to_string(),
        0 => "new here".to_string(),
        b if b >= CLEARED => "cleared".to_string(),
        b => format!("best: floor {b}"),
    };
    text(&format!("{} HP · {} power · {best}", h.hp, h.power), lx + bslide, by + 114.0, 14.0, MUTED);
    wrapped(h.passive_desc, lx, by + 138.0, w, 14.0, INK);
    for (k, &e) in h.elements.iter().enumerate() {
        gem(e, vec2(lx + 200.0 + k as f32 * 20.0, by + 87.0), 7.0);
    }
    // the button goes where the pick is; the daily may be tried again and again
    let ok = daily_on || pick.0 < save.unlocked;
    let descend = button(Rect::new(lx, 662.0, w, 46.0), "Descend", ok, true);
    if ok && (descend || (is_key_pressed(KeyCode::Enter) && !modal())) {
        go = Some(if daily_on { Go::Daily } else { Go::Play });
    }
    SHEET_TIPS.with(|t| t.set(true));
    let deeds_closed = DEEDS_ON.with(|d| d.get()) && deeds_sheet(save);
    let forge_closed = FORGE_ON.with(|f| f.get()) && forge_sheet(save);
    SHEET_TIPS.with(|t| t.set(false));
    if deeds_closed {
        DEEDS_ON.with(|d| d.set(false));
        save.deeds_seen = save.deeds;
        save.store();
    }
    if forge_closed {
        FORGE_ON.with(|f| f.set(false));
    }
    go
}

/// The forge, over the map: spend ore on wagers (permanent toggles) and The Crucible (repeatable
/// one-run bespoke relics: 2 boons + 1 drawback, consumed on descent). Stores the save on every change.
fn forge_sheet(save: &mut Save) -> bool {
    let press = is_mouse_button_pressed(MouseButton::Left) && !SETTINGS.with(|s| s.get());
    let mp = modal_open_p();
    overlay();
    draw_rectangle(-2000.0, -2000.0, 5280.0, 5000.0, fade(OUT, 0.7 * mp));
    let panel = Rect::new(150.0, 24.0 + (1.0 - mp) * 16.0, 980.0, 672.0);
    plate(panel, CARD);
    stamped("Forge", 640.0, panel.y + 52.0, 38.0, INK);
    let pct: u32 = WAGERS.iter().enumerate().filter(|(k, _)| save.wagers_on & 1 << k != 0).map(|(_, w)| w.ore_pct).sum();
    // the ore held, big, with what the wagers on add to the next run's
    let opop = (1.0 - (now() - fx(|fx| fx.forge_t)) as f32 / 0.25).clamp(0.0, 1.0);
    let ore = save.ore.to_string();
    let tail = format!("  ore   ·   wagers on: +{pct}% ore");
    let (ow, tw) = (measure(&ore, 26.0).width, measure(&tail, 15.0).width);
    let ox = 640.0 - (ow + tw) / 2.0;
    text(&ore, ox, panel.y + 88.0 - opop * 3.0, 26.0, GOLD);
    text(&tail, ox + ow, panel.y + 86.0, 15.0, MUTED);
    let mut changed = false;

    // left column: buy a wager once, then switch it on or off
    let (lx, cw) = (panel.x + 36.0, 444.0);
    let owned = save.wagers.count_ones();
    text(&format!("WAGERS · {owned} of {} bought", WAGERS.len()), lx, panel.y + 128.0, 15.0, ACCENT);
    text("Harder foes, more ore.", lx, panel.y + 148.0, 14.0, MUTED);
    for (k, w) in WAGERS.iter().enumerate() {
        let card = Rect::new(lx, panel.y + 162.0 + k as f32 * 74.0, cw, 66.0);
        let (have, on) = (save.wagers & 1 << k != 0, save.wagers_on & 1 << k != 0);
        if on {
            block(card, CARD_HI);
        }
        outline(card, if on { INK } else { FAINT });
        let icon = Rect::new(card.x + 10.0, card.y + 10.0, 46.0, 46.0);
        outline(icon, if have { INK } else { FAINT });
        sprite(w.icon, icon.x + 5.0, icon.y + 5.0, 36.0, if on { WHITE } else if have { fade(WHITE, 0.6) } else { fade(WHITE, 0.25) });
        let tx = icon.right() + 14.0;
        text(w.name, tx, card.y + 26.0, 18.0, if have { INK } else { MUTED });
        let bonus = if w.ore_pct > 0 { format!("+{}% ore", w.ore_pct) } else { "no bonus".into() };
        text(&bonus, tx + measure(w.name, 18.0).width + 12.0, card.y + 26.0, 14.0, if on { GOLD } else { MUTED });
        wrapped_fit(w.desc, tx, card.y + 48.0, 250.0, 14.0, 18.0, MUTED);
        let b = Rect::new(card.right() - 108.0, card.y + 16.0, 96.0, 34.0);
        if !have {
            if panel_button_on(b, &format!("Buy {}", w.price), save.ore >= w.price, false, press) && save.buy_wager(k) {
                play(Sfx::Coins);
                burst(b.center(), 18, 180.0, 6.0, GOLD);
                rect_ring(card, 10.0, 2.0, GOLD, 0.28);
                fx(|fx| fx.forge_t = now());
                changed = true;
            }
        } else if panel_button_on(b, if on { "On" } else { "Off" }, true, on, press) {
            play(Sfx::Click);
            save.wagers_on ^= 1 << k;
            rect_ring(card, 8.0, 1.8, INK, 0.22);
            fx(|fx| fx.forge_t = now());
            changed = true;
        }
    }

    // divider between Wagers and The Crucible
    draw_line(panel.x + 502.0, panel.y + 116.0, panel.x + 502.0, panel.bottom() - 78.0, 1.0, FAINT);

    // right column: The Crucible (repeatable one-run bespoke relic crafting)
    let (rx, rw) = (panel.x + 524.0, 420.0);
    text("THE CRUCIBLE", rx, panel.y + 128.0, 15.0, GOLD);
    text("Smelt a one-run relic: 2 boons + 1 drawback. Consumed on descent.", rx, panel.y + 148.0, 13.0, MUTED);
    let slot = Rect::new(rx, panel.y + 166.0, rw, 276.0);
    plate(slot, WELL);
    let ore_seed = (save.ore as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    let make_rng = |salt: u64| crate::rng::Rng::new((get_time() * 1_000_000.0) as u64 ^ ore_seed ^ salt);
    if let Some(relic) = save.crucible {
        outline(slot, GOLD);
        let name = relic.name();
        fit_text(&name, slot.x + 20.0, slot.y + 42.0, slot.w - 40.0, 28.0, GOLD);
        draw_line(slot.x + 20.0, slot.y + 58.0, slot.right() - 20.0, slot.y + 58.0, 1.0, FAINT);
        text("BOONS", slot.x + 20.0, slot.y + 84.0, 13.0, ACCENT);
        let y1 = wrapped_fit(&format!("+ {}", CRUCIBLE_BOONS[relic.boons[0]].desc), slot.x + 20.0, slot.y + 108.0, slot.w - 40.0, 16.0, 38.0, INK);
        let y2 = wrapped_fit(&format!("+ {}", CRUCIBLE_BOONS[relic.boons[1]].desc), slot.x + 20.0, y1 + 8.0, slot.w - 40.0, 16.0, 38.0, INK);
        text("DRAWBACK", slot.x + 20.0, y2 + 24.0, 13.0, BLOOD);
        wrapped_fit(&format!("- {}", CRUCIBLE_CURSES[relic.curse].desc), slot.x + 20.0, y2 + 48.0, slot.w - 40.0, 16.0, 44.0, BLOOD);
        text("Ready for your next non-daily descent.", slot.x + 20.0, slot.bottom() - 16.0, 13.0, MUTED);
    } else {
        centered("No relic forged", slot.center().x, slot.center().y - 6.0, 24.0, MUTED);
        centered("Spend ore to smelt a bespoke relic for one run.", slot.center().x, slot.center().y + 22.0, 14.0, MUTED);
    }

    let smelt_cost = save.crucible_cost();
    let smelt_label = if save.crucible.is_some() { format!("Reforge All · {smelt_cost} ore") } else { format!("Smelt Relic · {smelt_cost} ore") };
    let sb = Rect::new(rx, slot.bottom() + 18.0, rw, 44.0);
    if panel_button_on(sb, &smelt_label, save.ore >= smelt_cost, true, press) && save.smelt_crucible(&mut make_rng(0x51A7)) {
        play(Sfx::LevelUp);
        burst(slot.center(), 24, 220.0, 6.5, GOLD);
        rect_ring(slot, 14.0, 2.2, GOLD, 0.32);
        fx(|fx| fx.forge_t = now());
        changed = true;
    }
    let tb = Rect::new(rx, sb.bottom() + 12.0, 268.0, 40.0);
    let can_temper = save.crucible.is_some() && save.ore >= CRUCIBLE_TEMPER_COST;
    if panel_button_on(tb, &format!("Temper Drawback · {CRUCIBLE_TEMPER_COST} ore"), can_temper, false, press) && save.temper_crucible(&mut make_rng(0xC011)) {
        play(Sfx::Coins);
        burst(vec2(slot.center().x, slot.bottom() - 60.0), 18, 180.0, 5.5, BLOOD);
        rect_ring(slot, 10.0, 1.8, INK, 0.24);
        fx(|fx| fx.forge_t = now());
        changed = true;
    }
    let db = Rect::new(tb.right() + 12.0, sb.bottom() + 12.0, rw - tb.w - 12.0, 40.0);
    if panel_button_on(db, "Discard", save.crucible.is_some(), false, press) && save.crucible.is_some() {
        play(Sfx::Click);
        save.discard_crucible();
        fx(|fx| fx.forge_t = now());
        changed = true;
    }
    wrapped("Reforge rolls 2 new boons and a new drawback. Temper keeps both boons and rerolls only the drawback.", rx, tb.bottom() + 18.0, rw, 13.0, MUTED);

    if changed {
        save.store();
    }
    particles();
    panel_button(Rect::new(540.0, panel.bottom() - 58.0, 200.0, 40.0), "Close", true, press)
}

/// Every deed, done or not, and what it opens, on one sheet over the map. True once closed.
fn deeds_sheet(save: &Save) -> bool {
    let press = is_mouse_button_pressed(MouseButton::Left) && !SETTINGS.with(|s| s.get());
    let mp = modal_open_p();
    overlay();
    draw_rectangle(-2000.0, -2000.0, 5280.0, 5000.0, fade(OUT, 0.7 * mp));
    let panel = Rect::new(200.0, 24.0 + (1.0 - mp) * 16.0, 880.0, 672.0);
    plate(panel, CARD);
    stamped("Deeds", 640.0, panel.y + 52.0, 38.0, INK);
    centered("Each one, done once, opens something for every run after.", 640.0, panel.y + 80.0, 15.0, MUTED);
    let fresh = save.new_deeds();
    for (k, d) in DEEDS.iter().enumerate() {
        let y = panel.y + 104.0 + k as f32 * 64.0;
        let done = save.deeds & 1 << k != 0;
        if k > 0 {
            draw_line(panel.x + 36.0, y - 6.0, panel.right() - 36.0, y - 6.0, 1.0, FAINT);
        }
        // a filled box for done, an empty one for still to do
        let mark = Rect::new(panel.x + 40.0, y + 10.0, 14.0, 14.0);
        if done { block(mark, INK) } else { outline(mark, MUTED) }
        text(d.name, panel.x + 70.0, y + 22.0, 18.0, if done { INK } else { MUTED });
        if fresh & 1 << k != 0 {
            lively("new", panel.x + 78.0 + measure(d.name, 18.0).width, y + 22.0, 14.0, GOLD, Motion::Wave);
        }
        text(d.task, panel.x + 70.0, y + 44.0, 14.0, MUTED);
        // what it opens: its sigil, its name and what it does
        let (kind, name, what, sigil) = match d.opens {
            Unlock::Pact(n) => {
                let id = CHARMS.iter().position(|c| c.name == n).expect("a deed's pact exists");
                ("Pact", n, CHARMS[id].desc.to_string(), Some(CHARMS[id].icon.0))
            }
            Unlock::Brand(n) => {
                let b = BRANDS.iter().find(|b| b.name == n).expect("a deed's brand exists");
                ("Brand", n, format!("Burnt onto a rite: {}.", b.effect.text(1)), None)
            }
            Unlock::Hero(n) => {
                let h = HEROES.iter().find(|h| h.name == n).expect("a deed's hero exists");
                ("Bearer", n, h.passive_desc.to_string(), Some(h.sprite))
            }
        };
        let ox = panel.x + 470.0;
        let icon = Rect::new(ox, y + 4.0, 40.0, 40.0);
        outline(icon, if done { INK } else { FAINT });
        match sigil {
            Some(sp) => sprite(sp, icon.x + 5.0, icon.y + 5.0, 30.0, if done { WHITE } else { fade(WHITE, 0.35) }),
            None => draw_poly_lines(icon.center().x, icon.center().y, 4, 11.0, 0.0, 1.5, if done { INK } else { MUTED }),
        }
        text(&format!("{kind} · {name}"), ox + 54.0, y + 18.0, 15.0, if done { INK } else { MUTED });
        wrapped_fit(&what, ox + 54.0, y + 36.0, 316.0, 13.0, 30.0, MUTED);
    }
    panel_button(Rect::new(540.0, panel.bottom() - 58.0, 200.0, 40.0), "Close", true, press)
}

/// a board tile, and the step from one tile to the next
const TILE: f32 = 54.0;
const STEP: f32 = 56.0;
/// your board's top-left corner, in the middle of the screen
const BOARD: Vec2 = vec2(472.0, 330.0);
/// where each fighter's sigil is centred, and how big: you on the left, the enemy alone at the top
const STAND: [Vec2; 2] = [vec2(104.0, 348.0), vec2(640.0, 82.0)];
/// A hit this big gets impact frames and an outsized number.
const HUGE: i32 = 100;
/// How long each of a huge hit's two impact frames holds, in real seconds.
const IMPACT: f64 = 0.085;
const FIGHTER: [f32; 2] = [96.0, 120.0];
/// the enemy's board, shrunk to a preview in the top right
const PEEK: Vec2 = vec2(1082.0, 62.0);
/// the enemy's block at the top
const FOE: Rect = Rect { x: 440.0, y: 14.0, w: 400.0, h: 284.0 };
const SHOT_TIME: f32 = 0.2;

/// A small grey label over a block of the battle screen.
fn label(s: &str, x: f32, y: f32) {
    text(s, x, y, 15.0, MUTED);
}

/// A thin life bar: white for what is left, grey for the chunk just lost, optional hit/heal flash,
/// shield trim cap, and pulsing incoming-damage preview slice when hovering a rite.
fn life(r: Rect, frac: f32, ghost: f32, shield_frac: f32, hit_fl: f32, heal_fl: f32, preview_frac: f32) {
    block(r, OUT);
    for (f, c) in [(ghost, MUTED), (frac, mix(mix(INK, rgb(0xffffff), hit_fl), rgb(0x7bd88f), heal_fl * 0.75))] {
        if f > 0.0 {
            block(Rect::new(r.x, r.y, r.w * f.min(1.0), r.h), c);
        }
    }
    // hovering a lit attacking rite previews the HP slice it will carve off
    if preview_frac > 0.0 && frac > 0.0 {
        let cut = preview_frac.min(frac);
        let x0 = r.x + r.w * (frac - cut).clamp(0.0, 1.0);
        let w = r.w * cut;
        let pulse = 0.45 + 0.45 * ((now() as f32 * 8.0).sin() * 0.5 + 0.5);
        block(Rect::new(x0, r.y, w, r.h), fade(BLOOD, pulse));
        draw_line(x0, r.y, x0, r.bottom(), 1.5, INK);
    }
    // shield cap along the top edge in shield blue so shield reads at a glance
    if shield_frac > 0.0 {
        let sw = (r.w * shield_frac.min(1.0)).max(4.0);
        block(Rect::new(r.x, r.y - 2.0, sw, 2.5), rgb(0x6fb6ff));
    }
    draw_rectangle_lines(r.x, r.y, r.w, r.h, 1.0, if hit_fl > 0.1 { rgb(0xffffff) } else { INK });
}

/// Crisp procedural fracture fissures drawn directly into cracked board tiles.
fn draw_cracks(r: Rect, cracks: u8, tile_idx: usize, doomed: bool, now: f32) {
    if cracks == 0 {
        return;
    }
    let h = |k: u64| hash01(tile_idx as u64 * 19 + 7, k);
    let p0 = vec2(r.x + (0.14 + 0.22 * h(1)) * r.w, r.y + (0.14 + 0.22 * h(2)) * r.h);
    let p1 = vec2(r.x + (0.42 + 0.18 * h(3)) * r.w, r.y + (0.44 + 0.18 * h(4)) * r.h);
    let p2 = vec2(r.x + (0.68 + 0.20 * h(5)) * r.w, r.y + (0.68 + 0.20 * h(6)) * r.h);
    let seam = fade(OUT, if cracks >= 2 { 0.85 } else { 0.55 });
    let edge = fade(BLOOD, if cracks >= 2 { 0.72 } else { 0.3 });
    let w = if cracks >= 2 { 2.0 } else { 1.4 };
    draw_line(p0.x, p0.y, p1.x, p1.y, w, seam);
    draw_line(p1.x, p1.y, p2.x, p2.y, w, seam);
    draw_line(p0.x + 1.0, p0.y, p1.x + 1.0, p1.y, 1.0, edge);
    if cracks >= 2 {
        // second forked fault fissure
        let q0 = vec2(r.x + (0.72 + 0.16 * h(7)) * r.w, r.y + (0.14 + 0.20 * h(8)) * r.h);
        let q2 = vec2(r.x + (0.16 + 0.20 * h(9)) * r.w, r.y + (0.74 + 0.16 * h(10)) * r.h);
        draw_line(q0.x, q0.y, p1.x, p1.y, 2.0, seam);
        draw_line(p1.x, p1.y, q2.x, q2.y, 1.8, seam);
        draw_line(q0.x, q0.y + 1.0, p1.x, p1.y + 1.0, 1.0, edge);
    }
    if doomed {
        // inward corner warning ticks pulsing when hovered crystal will bring this tile down
        let pulse = 0.55 + 0.45 * (now * 10.0).sin().abs();
        let col = fade(BLOOD, pulse);
        let len = 8.0;
        let inset = 3.0;
        for (cx, cy, dx, dy) in [
            (r.x + inset, r.y + inset, 1.0, 1.0),
            (r.right() - inset, r.y + inset, -1.0, 1.0),
            (r.x + inset, r.bottom() - inset, 1.0, -1.0),
            (r.right() - inset, r.bottom() - inset, -1.0, -1.0),
        ] {
            draw_line(cx, cy, cx + dx * len, cy, 2.0, col);
            draw_line(cx, cy, cx, cy + dy * len, 2.0, col);
        }
    }
}

/// Persistent visual auras behind/around a live fighter: shield ward, brittle crystalline shards,
/// enemy loaded charge ring, burn embers, weaken ash, and marked element micro-signs.
fn draw_fighter_auras(s: usize, f: &Fighter, vis_shield: i32, at: Vec2, size: f32, now: f64, shield_hit: f64, shield_gain: f64, load_t: f64, break_t: f64) {
    let t = now as f32;
    let half = size * 0.5;
    // enemy loaded rites: taut rhythmic charge ring while unbroken, cracked arcs once broken
    if s == 1 && !f.loaded.is_empty() {
        let pulse = 0.5 + 0.5 * (t * 4.5).sin();
        let fresh = (1.0 - ((now - load_t) as f32 / 0.45)).clamp(0.0, 1.0);
        if !f.broken {
            let r = half * (1.05 + 0.05 * pulse + 0.14 * fresh);
            let col = mix(BLOOD, INK, fresh * 0.5);
            draw_circle_lines(at.x, at.y, r, 1.8 + 1.2 * fresh, fade(col, 0.35 + 0.25 * pulse));
            // orbiting tension diamonds, one per loaded rite
            let n = f.loaded.len().min(4);
            for k in 0..n {
                let ang = t * 1.8 + k as f32 * std::f32::consts::TAU / n as f32;
                let p = at + Vec2::from_angle(ang) * (r + 5.0);
                draw_poly(p.x, p.y, 4, 4.5 + 1.5 * pulse, ang.to_degrees(), fade(BLOOD, 0.85));
            }
        } else {
            // broken: faint jagged split halves drifting apart
            let fade_brk = (1.0 - ((now - break_t) as f32 / 0.5)).clamp(0.0, 1.0);
            let r = half * (1.08 + 0.1 * fade_brk);
            for k in 0..4 {
                let a0 = k as f32 * std::f32::consts::FRAC_PI_2 + 0.22;
                let a1 = a0 + 0.7;
                let p0 = at + Vec2::from_angle(a0) * r;
                let p1 = at + Vec2::from_angle(a1) * r;
                draw_line(p0.x, p0.y, p1.x, p1.y, 1.5, fade(MUTED, 0.45 + 0.35 * fade_brk));
            }
        }
    }
    // persistent shield ward (hexagonal); locks in crisply on gain and flashes on hit
    if vis_shield > 0 {
        let hit_p = (1.0 - ((now - shield_hit) as f32 / 0.32)).clamp(0.0, 1.0);
        let gain_p = (1.0 - ((now - shield_gain) as f32 / 0.34)).clamp(0.0, 1.0);
        let r = half * (0.94 + 0.03 * (t * 2.4).sin() + 0.1 * hit_p + 0.16 * gain_p);
        let col = mix(rgb(0x6fb6ff), INK, (hit_p * 0.8).max(gain_p * 0.6));
        draw_poly_lines(at.x, at.y, 6, r, t * 14.0, 1.8 + 2.2 * hit_p.max(gain_p), fade(col, 0.48 + 0.45 * hit_p.max(gain_p)));
        draw_poly(at.x, at.y, 6, r * 0.96, t * 14.0, fade(rgb(0x6fb6ff), 0.06 + 0.12 * gain_p));
        if f.brittle {
            let spike_r = r + 7.0 + 2.0 * (t * 6.0).sin();
            for k in 0..6 {
                let ang = (t * 14.0).to_radians() + k as f32 * std::f32::consts::TAU / 6.0;
                let p = at + Vec2::from_angle(ang) * spike_r;
                draw_poly(p.x, p.y, 4, 4.0, ang.to_degrees(), fade(col, 0.85));
            }
        }
    }
    // burn aura: subtle periodic rising embers
    if f.burn > 0 && gen_range(0.0, 1.0) < (0.14 + f.burn.min(12) as f32 * 0.025) {
        let pos = at + vec2(gen_range(-half * 0.65, half * 0.65), gen_range(-half * 0.2, half * 0.45));
        let vel = vec2(gen_range(-16.0, 16.0), gen_range(-85.0, -35.0));
        let col = mix(rgb(0xff8a4c), GOLD, gen_range(0.0, 0.5));
        fx(|fx| fx.parts.push(Part { pos, vel, born: now, life: gen_range(0.35, 0.7), size: gen_range(3.0, 5.5), color: col, grav: -40.0, home: None }));
    }
    // weak aura: subtle downward-drifting faint ash strokes
    if f.weak > 0 {
        for k in 0..3 {
            let ph = (t * 0.7 + k as f32 * 0.33).fract();
            let x = at.x + (k as f32 - 1.0) * half * 0.6;
            let y = at.y - half * 0.3 + ph * half * 0.9;
            let a = 0.35 * (ph * std::f32::consts::PI).sin();
            draw_line(x, y, x, y + 7.0, 1.5, fade(rgb(0xd2b48c), a));
        }
    }
    // marked elements: tiny crisp signs hovering cleanly off the fighter's right flank (clear of names)
    if !f.mark.is_empty() {
        let step = 18.0;
        let x0 = at.x + half + 20.0;
        for (k, &e) in f.mark.iter().enumerate() {
            let bob = (t * 3.0 + k as f32 * 1.4).sin() * 2.0;
            sign(e, vec2(x0 + k as f32 * step, at.y + bob), 5.5, mix(INK, hue(e), 0.55));
        }
    }
}

/// Power and whatever else a fighter has going: shield, armor, burn and so on.
fn conditions(f: &Fighter, vis_shield: i32) -> String {
    let mut st = format!("Power {}", f.power);
    let dodge = if f.mods.graze > 0 { "Graze" } else { "Evade" };
    let shield = if f.brittle { "Brittle shield" } else { "Shield" };
    for (label, v) in [(shield, vis_shield), ("Armor", f.mods.armor), ("Thorns", f.mods.thorns), (dodge, f.evades_left), ("Burn", f.burn), ("Weak", f.weak), ("Duds", f.duds)] {
        if v > 0 {
            st += &format!(" · {label} {v}");
        }
    }
    if !f.mark.is_empty() {
        st += " · Marked";
    }
    st
}

/// A fighter's conditions, then its mark in the gem signs rites use. Hovering the mark shows what reacts.
fn status_line(f: &Fighter, vis_shield: i32, x: f32, y: f32) {
    let s = conditions(f, vis_shield);
    rich(&s, x, y, 15.0, INK);
    let end = x + measure(&s, 15.0).width;
    let mut gx = end + 14.0;
    for &e in &f.mark {
        gem(e, vec2(gx, y - 5.0), 7.0);
        gx += 20.0;
    }
    let from = end - measure("Marked", 15.0).width;
    if !f.mark.is_empty() && !modal() && Rect::new(from, y - 15.0, gx - from, 20.0).contains(mouse()) {
        MARK_TIP.with(|c| *c.borrow_mut() = Some(f.mark.clone()));
    }
}

/// What a rite bringing `e` to a target whose mark lacks it sets off.
fn reaction_text(e: Element) -> &'static str {
    match e {
        Element::Ember => "ignite: +3 burn on it",
        Element::Tide => "douse: +4 shield for you",
        Element::Gale => "gust: +1 pick for you",
        Element::Stone => "crush: +4 damage",
        Element::Spark => "arc: +2 surge for you",
    }
}

/// An element's name and the reaction it sets off, for its gem sign.
fn element_tip(e: Element) -> (&'static str, &'static str) {
    match e {
        Element::Ember => ("Ember", "Ignite: +3 burn on a target not marked with Ember."),
        Element::Tide => ("Tide", "Douse: +4 shield for you, on a target not marked with Tide."),
        Element::Gale => ("Gale", "Gust: +1 pick for you, on a target not marked with Gale."),
        Element::Stone => ("Stone", "Crush: +4 damage on a target not marked with Stone."),
        Element::Spark => ("Spark", "Arc: +2 surge for you, on a target not marked with Spark."),
    }
}

/// The word a rite bringing `e` to a marked target sets off.
fn react_word(e: Element) -> &'static str {
    match e {
        Element::Ember => "ignite",
        Element::Tide => "douse",
        Element::Gale => "gust",
        Element::Stone => "crush",
        Element::Spark => "arc",
    }
}

/// A rite's name at the level `f` holds it.
fn foe_title(f: &Fighter, c: &Combo) -> String {
    c.title(f.level(c)).to_string()
}

/// Held gems counted by element: the fighter's own elements first (even at 0), then any taken from others.
fn by_element(order: &[Element], held: impl Iterator<Item = Element>) -> Vec<(Element, usize)> {
    let mut v: Vec<(Element, usize)> = order.iter().map(|&e| (e, 0)).collect();
    for e in held {
        match v.iter_mut().find(|x| x.0 == e) {
            Some(x) => x.1 += 1,
            None => v.push((e, 1)),
        }
    }
    v
}

/// Solid rock: a dark slab hatched with diagonal lines.
fn rock(r: Rect) {
    block(r, rgb(0x111111));
    let mut k = 8.0;
    while k < r.w + r.h {
        let a = vec2(r.x + (k - r.h).max(0.0), r.y + k.min(r.h));
        let b = vec2(r.x + k.min(r.w), r.y + (k - r.w).max(0.0));
        draw_line(a.x, a.y, b.x, b.y, 2.0, rgb(0x242424));
        k += 8.0;
    }
}

/// A board tile's ground, before cracks.
const TILE_BG: Color = rgb(0x1c1c1c);

/// A tile's colour under `n` cracks: from bare ground, warming toward red as it nears caving in.
fn strain(n: u8) -> Color {
    mix(TILE_BG, BLOOD, [0.0, 0.2, 0.48, 0.75][(n as usize).min(3)])
}

/// A caved-in tile: a dark hole with broken slabs in it.
fn rubble(r: Rect) {
    block(r, rgb(0x0a0a0a));
    for (x, y, w, h) in [(0.12, 0.2, 0.34, 0.2), (0.52, 0.5, 0.36, 0.22), (0.2, 0.66, 0.26, 0.18)] {
        draw_rectangle(r.x + x * r.w, r.y + y * r.h, w * r.w, h * r.h, rgb(0x262626));
    }
}

/// A jagged bolt from `a` to `z`, with crackling forked side branches redrawn every frame.
fn lightning(a: Vec2, z: Vec2, width: f32, color: Color) {
    let mut prev = a;
    for k in 1..=8 {
        let q = k as f32 / 8.0;
        let next = if k == 8 { z } else { a + (z - a) * q + vec2(gen_range(-10.0, 10.0), gen_range(-20.0, 20.0)) };
        draw_line(prev.x, prev.y, next.x, next.y, width, color);
        draw_line(prev.x, prev.y, next.x, next.y, width * 0.35, INK);
        // crackling side fork from mid-joints
        if (k == 3 || k == 6) && width >= 4.0 {
            let fork_mid = next + vec2(gen_range(-34.0, 34.0), gen_range(-34.0, 34.0));
            let fork_tip = fork_mid + vec2(gen_range(-24.0, 24.0), gen_range(-24.0, 24.0));
            draw_line(next.x, next.y, fork_mid.x, fork_mid.y, width * 0.45, fade(color, 0.7));
            draw_line(fork_mid.x, fork_mid.y, fork_tip.x, fork_tip.y, width * 0.25, fade(INK, 0.55));
        }
        prev = next;
    }
}

/// A diamond pip, filled when `full`.
fn pip(c: Vec2, r: f32, full: bool) {
    if full {
        draw_poly(c.x, c.y, 4, r, 0.0, INK);
    } else {
        draw_poly_lines(c.x, c.y, 4, r, 0.0, 1.5, INK);
    }
}

/// Every skill in flight. Each flies its own way by its first element (fireball, lobbed water,
/// spinning wind blade, boulder, lightning) and shows its effects (flames, a shield dome, healing
/// motes, a hex ring, stolen gems). Level 2 charges up and fires twice; level 3 also calls down a
/// pillar of light and fires three times.
fn draw_shots(shots: &[Shot], now: f64) {
    use std::f32::consts::PI;
    let step = dt() / SHOT_TIME;
    for sh in shots {
        let p = (now - sh.t0) as f32 / SHOT_TIME;
        let (a, z) = (STAND[sh.from], STAND[1 - sh.from]);
        let feet = a + vec2(0.0, FIGHTER[sh.from] * 0.45);
        let (id, lvl) = sh.skill;
        let c = &COMBOS[id];
        let (e1, e2) = (c.needs[0], c.needs.get(1).copied().unwrap_or(c.needs[0]));
        let (c1, c2) = (el_color(e1), el_color(e2));
        let lead = charge(lvl);
        // level 2+: energy gathers into the caster, a ring closing in
        if p < lead {
            let q = p / lead;
            draw_circle_lines(a.x, a.y, 80.0 * (1.0 - q) + 14.0, 5.0, fade(c2, q));
            draw_circle_lines(a.x, a.y, 50.0 * (1.0 - q) + 8.0, 3.0, fade(c1, q));
            if gen_range(0.0, 1.0) < 0.7 {
                stream(a + Vec2::from_angle(gen_range(0.0, 2.0 * PI)) * 90.0, a, 1, mix(c1, INK, 0.3));
            }
        }
        // level 3: the mastery's own sign, brightest as the first bolt hits
        let first_hit = lead + 1.0;
        if sh.master && p < first_hit + 1.4 {
            let alpha = if p < first_hit { p / first_hit } else { 1.0 - (p - first_hit) / 1.4 };
            mastery_fx(c.master, a, z, alpha, now as f32);
        }
        let arc = match e1 {
            Element::Ember => 26.0,
            Element::Tide => 55.0,
            Element::Stone => 85.0,
            Element::Gale | Element::Spark => 0.0,
        };
        for k in 0..lvl {
            let q = p - volley(lvl, k);
            // bolts after the first come in on a different curve
            let bend = [1.0, -0.6, 1.6][k.min(2)];
            let along = |q: f32| {
                let wobble = if e1 == Element::Gale { (q * PI * 4.0).sin() * 14.0 } else { 0.0 };
                a + (z - a) * q - vec2(0.0, (q * PI).sin() * arc * bend + wobble)
            };
            if (0.0..1.0).contains(&q) && !sh.hits {
                // nothing is thrown: a ward circle at the caster's feet tightens while loose dust
                // of the rite's gems is drawn in
                let w = 120.0 - 40.0 * q + k as f32 * 18.0;
                let glow = (q * PI).sin();
                draw_ellipse_lines(feet.x, feet.y, w / 2.0, w / 7.0, 0.0, 2.0, fade(c1, 0.7 * glow));
                draw_ellipse_lines(feet.x, feet.y, w / 3.0, w / 10.5, 0.0, 1.5, fade(c2, 0.5 * glow));
                if gen_range(0.0, 1.0) < 0.35 {
                    let from = feet + vec2(gen_range(-w, w) / 2.0, gen_range(-8.0, 4.0));
                    stream(from, a, 1, fade(mix(c1, INK, 0.25), 0.7));
                }
            } else if (0.0..1.0).contains(&q) {
                let head = along(q);
                let dir_v = (along((q + 0.04).min(1.0)) - head).normalize_or_zero();
                let perp = vec2(-dir_v.y, dir_v.x);
                let tint = mix(INK, hue(e1), 0.5);
                match e1 {
                    Element::Spark => lightning(a, z, 6.0 + 2.0 * lvl as f32, rgb(0xffe066)),
                    Element::Gale => {
                        draw_poly(head.x, head.y, 3, 28.0, now as f32 * 1500.0, fade(tint, 0.85));
                        draw_poly(head.x, head.y, 3, 17.0, -(now as f32) * 1500.0, fade(INK, 0.95));
                        // crisp angled wind-shear ribbons trailing along the flight path
                        for t in 1..6 {
                            let back = along((q - t as f32 * 0.055).max(0.0));
                            let side = if t % 2 == 0 { 1.0 } else { -1.0 } * (8.0 + t as f32 * 2.5);
                            let p0 = back + perp * side;
                            let p1 = p0 - dir_v * 26.0;
                            draw_line(p0.x, p0.y, p1.x, p1.y, 3.0 - t as f32 * 0.35, fade(tint, 0.7 - t as f32 * 0.11));
                        }
                    }
                    Element::Stone => {
                        draw_poly(head.x, head.y, 5, 30.0, now as f32 * 480.0, mix(tint, OUT, 0.4));
                        draw_poly(head.x, head.y, 5, 22.0, now as f32 * 480.0, tint);
                        draw_poly_lines(head.x, head.y, 5, 29.0, now as f32 * 480.0, 2.0, INK);
                        if gen_range(0.0, 1.0) < 0.75 {
                            rise(head, 1, 10.0, mix(tint, OUT, 0.3));
                        }
                    }
                    Element::Tide => {
                        // sweeping crescent wave surge with trailing water ribbon
                        let mut prev = head;
                        for t in 1..7 {
                            let back = along((q - t as f32 * 0.06).max(0.0));
                            let a_t = 0.8 - t as f32 * 0.11;
                            draw_line(prev.x, prev.y, back.x, back.y, 16.0 - t as f32 * 2.0, fade(tint, a_t * 0.45));
                            draw_line(prev.x, prev.y, back.x, back.y, 4.0, fade(INK, a_t * 0.7));
                            prev = back;
                        }
                        draw_poly(head.x, head.y, 4, 24.0, dir_v.y.atan2(dir_v.x).to_degrees(), fade(tint, 0.9));
                        let vel = vec2(gen_range(-50.0, 50.0), gen_range(-70.0, 10.0));
                        fx(|fx| fx.parts.push(Part { pos: head, vel, born: now, life: gen_range(0.28, 0.55), size: gen_range(4.0, 7.5), color: tint, grav: 540.0, home: None }));
                    }
                    Element::Ember => {
                        // blazing fire comet with swirling ember ring & cinder tail
                        for t in (1..6).rev() {
                            let back = q - t as f32 * 0.065;
                            if back > 0.0 {
                                let bp = along(back);
                                draw_circle(bp.x, bp.y, 16.0 - t as f32 * 2.2, fade(tint, 0.55 - t as f32 * 0.08));
                                gem(e1, bp, 20.0 - t as f32 * 2.6);
                            }
                        }
                        draw_circle_lines(head.x, head.y, 24.0 + 4.0 * (now as f32 * 20.0).sin(), 2.0, fade(tint, 0.7));
                        for _ in 0..3 {
                            let vel = vec2(gen_range(-35.0, 35.0), gen_range(-130.0, -40.0));
                            let color = mix(GOLD, rgb(0xff8a4c), gen_range(0.0, 1.0));
                            fx(|fx| fx.parts.push(Part { pos: head, vel, born: now, life: gen_range(0.28, 0.55), size: gen_range(4.0, 8.0), color, grav: -70.0, home: None }));
                        }
                    }
                }
                if e1 != Element::Spark {
                    gem(e1, head, 24.0);
                }
            } else if q >= 1.0 && q - step < 1.0 && !sh.hits {
                // it takes hold quietly: no shake, flash or hit-pause, since nothing was struck
                rise(feet, 6 + 3 * lvl, 34.0, fade(mix(c1, INK, 0.3), 0.6));
                if k == 0 {
                    rite_effects(sh, c, a, z, c2);
                }
            } else if q >= 1.0 && q - step < 1.0 && sh.miss {
                // dodged: a few sparks where it should have struck, nothing more
                burst(z + vec2(30.0, -10.0), 8, 120.0, 4.0, fade(c1, 0.6));
            } else if q >= 1.0 && q - step < 1.0 {
                // the frame this bolt lands
                anim(fx_impact(e1), z + vec2(gen_range(-10.0, 10.0), gen_range(-10.0, 10.0)), 160.0 + 30.0 * lvl as f32, 0.5, mix(el_color(e1), WHITE, 0.25));
                if is_basic(c) || c.effects.iter().any(|e| matches!(e, Effect::Damage(_) | Effect::Opener(_))) {
                    anim(FX_SLASH, z, 110.0 + 18.0 * lvl as f32, 0.26, fade(mix(INK, hue(e1), 0.35), 0.9));
                }
                ring(z, 18.0, 96.0 + 20.0 * lvl as f32, 4, 0.0, 3.5, fade(mix(INK, hue(e1), 0.45), 0.85), 0.32);
                burst(z, 50 + 20 * lvl, 420.0 + 50.0 * lvl as f32, 12.0, c1);
                burst(z, 26, 280.0, 8.0, c2);
                burst(z, 18, 320.0, 9.0, INK);
                quake(if e1 == Element::Stone { 22.0 } else { 12.0 + 4.0 * lvl as f32 });
                flash(c1);
                play(Sfx::Strike);
                hitstop(if k + 1 == lvl { 0.04 + 0.02 * lvl as f64 } else { 0.02 });
                punch();
                if k == 0 {
                    rite_effects(sh, c, a, z, c2);
                }
            }
        }
        // lingering after the first hit: a shockwave (a ring round the caster for a rite that does
        // not hit), and the shield dome or hex ring if it has one
        let u = p - first_hit;
        if (0.0..1.6).contains(&u) {
            let q = u / 1.6;
            if sh.miss {
            } else if sh.hits {
                let half = 24.0 + q * (150.0 + 40.0 * lvl as f32);
                draw_rectangle_lines(z.x - half, z.y - half, half * 2.0, half * 2.0, 14.0 * (1.0 - q), fade(c1, 1.0 - q));
            } else {
                let w = 60.0 + q * 70.0;
                draw_ellipse_lines(feet.x, feet.y, w, w / 3.5, 0.0, 2.0, fade(c1, 0.6 * (1.0 - q)));
            }
        }
        if (0.0..2.4).contains(&u) {
            let fadeout = 1.0 - u / 2.4;
            for e in c.effects.iter().chain(&sh.brand) {
                match e {
                    Effect::Shield(_) => {
                        let lock = ease_out3((u / 1.2).clamp(0.0, 1.0));
                        for ring in 0..lvl {
                            let r = 74.0 - 24.0 * lock + ring as f32 * 10.0;
                            draw_poly_lines(a.x, a.y, 6, r, now as f32 * 25.0 + ring as f32 * 30.0, 3.5, fade(rgb(0x6fb6ff), fadeout));
                        }
                        draw_poly(a.x, a.y, 6, 52.0, now as f32 * 25.0, fade(rgb(0x6fb6ff), 0.14 * fadeout));
                    }
                    Effect::Weaken(_) => {
                        let r = 90.0 * (1.0 - (u / 1.2).min(1.0)) + 22.0;
                        draw_circle_lines(z.x, z.y, r, 5.0, fade(HEX, fadeout));
                        draw_circle_lines(z.x, z.y, r * 0.6, 3.0, fade(HEX, fadeout * 0.7));
                    }
                    _ => {}
                }
            }
        }
    }
}

/// A mastered rite's own sign, `t` strong (0..=1): what its mastery does, drawn over the arena.
fn mastery_fx(m: Option<Effect>, a: Vec2, z: Vec2, t: f32, now: f32) {
    use std::f32::consts::TAU;
    let k = |w: &str| keyword(w).map_or(INK, |x| x.0);
    let chance = |p: f32| gen_range(0.0, 1.0) < p * t;
    // `n` diamonds circling `at`
    let orbit = |at: Vec2, n: usize, r: f32, size: f32, spin: f32, c: Color| {
        for i in 0..n {
            let p = at + Vec2::from_angle(spin + i as f32 * TAU / n as f32) * r;
            draw_poly(p.x, p.y, 4, size, spin.to_degrees() * 2.0, c);
        }
    };
    let ground = |at: Vec2| at + vec2(0.0, 50.0);
    match m {
        // steam vents hiss up round the caster: the mines ahead go off as duds
        Some(Effect::Duds(_)) => {
            for i in 0..4 {
                let x = a.x - 66.0 + i as f32 * 44.0;
                let h = 70.0 + 60.0 * ((now * 9.0 + i as f32 * 1.7).sin() * 0.5 + 0.5);
                draw_rectangle(x - 5.0, ground(a).y - h, 10.0, h, fade(INK, 0.3 * t));
            }
            if chance(0.9) {
                rise(ground(a) + vec2(gen_range(-70.0, 70.0), 0.0), 1, 6.0, fade(INK, 0.7));
            }
        }
        // blood drawn out of the foe and into the caster
        Some(Effect::Lifesteal(_)) => {
            if chance(1.0) {
                stream(z + vec2(gen_range(-30.0, 30.0), gen_range(-30.0, 30.0)), a, 2, k("damage"));
            }
            draw_circle_lines(a.x, a.y, 58.0 + 8.0 * (now * 12.0).sin(), 4.0, fade(k("damage"), 0.7 * t));
        }
        // a sight closing on the target: nothing turns this aside
        Some(Effect::Sure) => {
            let r = 30.0 + 120.0 * (1.0 - t);
            draw_circle_lines(z.x, z.y, r, 3.0, fade(k("damage"), t));
            draw_circle_lines(z.x, z.y, r * 0.55, 2.0, fade(INK, t));
            draw_circle(z.x, z.y, 6.0 * t, fade(INK, t));
        }
        // the surge stays: sparks circle the caster and do not fly
        Some(Effect::KeepSurge) => {
            orbit(a, 6, 62.0, 7.0, now * 5.0, fade(k("surge"), t));
            orbit(a, 6, 44.0, 4.0, -now * 7.0, fade(INK, t));
        }
        // mist rolls over your boards and the mines show through
        Some(Effect::Scout(_)) => {
            for i in 0..5 {
                let x = BOARD.x + ((now * 60.0 + i as f32 * 90.0) % 420.0) - 20.0;
                draw_circle(x, BOARD.y + 50.0 + i as f32 * 60.0, 70.0, fade(INK, 0.14 * t));
            }
            if chance(0.5) {
                rise(BOARD + vec2(gen_range(0.0, 380.0), gen_range(0.0, 380.0)), 1, 4.0, fade(k("reveal"), 0.8));
            }
        }
        // a battlemented wall rises in front of the caster and stays standing
        Some(Effect::Hold) => {
            let (x, w, h) = (a.x + 64.0, 44.0, 120.0 * t);
            let top = ground(a).y - h;
            draw_rectangle(x, top, w, h, fade(k("shield"), 0.45 * t));
            draw_rectangle_lines(x, top, w, h, 2.0, fade(INK, t));
            for i in 0..3 {
                let mx = x + i as f32 * (w - 10.0) / 2.0;
                draw_rectangle(mx, top - 10.0, 10.0, 10.0, fade(k("shield"), 0.45 * t));
                draw_rectangle_lines(mx, top - 10.0, 10.0, 10.0, 2.0, fade(INK, t));
            }
        }
        // the foe's gems are pulled across to the caster
        Some(Effect::Steal(_)) => {
            if chance(0.8) {
                stream(z, a, 1, k("surge"));
            }
            orbit(z, 4, 50.0 + 30.0 * (1.0 - t), 6.0, now * 4.0, fade(INK, t));
        }
        // sand whirls round the foe's eyes
        Some(Effect::Blind(_)) => {
            for i in 0..40 {
                let ang = now * 3.0 + i as f32 * TAU / 40.0;
                let p = z + vec2(ang.cos() * (70.0 + (i % 4) as f32 * 12.0), ang.sin() * 30.0 - 20.0 + (i % 3) as f32 * 16.0);
                draw_rectangle(p.x, p.y, 7.0, 7.0, fade(k("weaken"), t));
            }
        }
        // the sky cracks down onto the foe's crystals
        Some(Effect::PerShatter(_)) => {
            if chance(0.6) {
                for _ in 0..2 {
                    lightning(vec2(z.x + gen_range(-200.0, 200.0), -60.0), z + vec2(gen_range(-40.0, 40.0), 0.0), 4.0, fade(INK, t));
                }
            }
        }
        // the ground splits under a foe that is already falling
        Some(Effect::Execute) => {
            let g = ground(z);
            let mut prev = g - vec2(160.0 * t, 0.0);
            for i in 1..=8 {
                let next = g + vec2(-160.0 * t + i as f32 * 40.0 * t, if i % 2 == 0 { -6.0 } else { 6.0 });
                draw_line(prev.x, prev.y, next.x, next.y, 5.0, fade(k("damage"), t));
                prev = next;
            }
            if chance(0.7) {
                burst(g + vec2(gen_range(-120.0, 120.0), 0.0), 2, 200.0, 6.0, INK);
            }
        }
        // a storm wall turns round the foe with a calm eye in the middle
        Some(Effect::Unused) => {
            for i in 0..3 {
                let r = 60.0 + i as f32 * 26.0;
                draw_poly_lines(z.x, z.y, 7, r, now * (200.0 - i as f32 * 60.0), 3.0, fade(k("reveal"), t * (1.0 - i as f32 * 0.25)));
            }
            draw_circle(z.x, z.y, 14.0, fade(INK, 0.4 * t));
        }
        // crystal shards circle the caster: half the shield stays
        Some(Effect::KeepHalf) => {
            orbit(a, 5, 66.0, 12.0, now * 2.0, fade(k("shield"), 0.8 * t));
            orbit(a, 5, 66.0, 6.0, now * 2.0, fade(INK, t));
        }
        // a sun over the caster pours heat into it
        Some(Effect::Stoke(_)) => {
            let sun = a - vec2(0.0, 120.0);
            draw_circle(sun.x, sun.y, 26.0 * t, fade(k("burn"), t));
            draw_poly_lines(sun.x, sun.y, 12, 40.0 * t, now * 60.0, 3.0, fade(INK, t));
            if chance(0.8) {
                stream(sun, a, 1, k("burn"));
            }
        }
        // a wave washes over the caster and leaves it clean
        Some(Effect::Cleanse) => {
            let x = a.x - 120.0 + 240.0 * (1.0 - t);
            draw_rectangle(x, a.y - 70.0, 18.0, 140.0, fade(k("tide"), 0.5 * t));
            if chance(0.8) {
                rise(a + vec2(gen_range(-50.0, 50.0), 40.0), 1, 8.0, fade(k("heal"), 0.8));
            }
        }
        // the lights go out round the foe
        Some(Effect::DrainAll) => {
            overlay();
    draw_rectangle(-2000.0, -2000.0, 5280.0, 5000.0, fade(OUT, 0.5 * t));
            if chance(0.8) {
                burst(z, 3, 260.0, 6.0, INK);
            }
        }
        // the foe stands in a pot that keeps on boiling
        Some(Effect::Simmer) => {
            draw_ellipse_lines(ground(z).x, ground(z).y, 90.0, 22.0, 0.0, 4.0, fade(k("burn"), t));
            if chance(1.0) {
                rise(ground(z) + vec2(gen_range(-80.0, 80.0), 0.0), 1, 4.0, k("burn"));
            }
        }
        // field lines: shield is drawn in from every side, with no limit
        _ => {
            for i in 0..4 {
                let w = 50.0 + i as f32 * 22.0;
                draw_ellipse_lines(a.x, a.y, w, w * 0.45, now * 40.0 + i as f32 * 45.0, 2.0, fade(k("shield"), t));
            }
        }
    }
}

/// True if a mastery acts on the foe, false if on the caster: its shout goes beside that one.
fn mastery_on_foe(m: Option<Effect>) -> bool {
    matches!(
        m,
        Some(Effect::Lifesteal(_) | Effect::Sure | Effect::Steal(_) | Effect::Blind(_) | Effect::PerShatter(_) | Effect::Execute | Effect::Unused | Effect::DrainAll | Effect::Simmer)
    )
}

/// The short shout a mastered rite gives as it lands.
fn mastery_word(m: Option<Effect>) -> &'static str {
    match m {
        Some(Effect::Duds(_)) => "DUDS",
        Some(Effect::Lifesteal(_)) => "BLOOD DRINK",
        Some(Effect::Sure) => "TRUE STRIKE",
        Some(Effect::KeepSurge) => "SURGE HELD",
        Some(Effect::Scout(_)) => "PROPPED UP",
        Some(Effect::Hold) => "HOLD FAST",
        Some(Effect::Steal(_)) => "STOLEN",
        Some(Effect::Blind(_)) => "BLINDED",
        Some(Effect::PerShatter(_)) => "SKY SPLIT",
        Some(Effect::Execute) => "EXECUTE",
        Some(Effect::Unused) => "STILL EYE",
        Some(Effect::KeepHalf) => "SHIELD KEPT",
        Some(Effect::Stoke(_)) => "STOKED",
        Some(Effect::Cleanse) => "CLEANSED",
        Some(Effect::DrainAll) => "BLACKOUT",
        Some(Effect::Simmer) => "SIMMERING",
        _ => "UNBOUND",
    }
}

/// The visuals of a rite's effects (and its brand's), as its first bolt lands.
fn rite_effects(sh: &Shot, c: &Combo, a: Vec2, z: Vec2, c2: Color) {
    let foe_board = if sh.from == 0 { PEEK + vec2(W as f32 * 12.5, W as f32 * 12.5) } else { BOARD + vec2(W as f32 * STEP / 2.0, W as f32 * STEP / 2.0) };
    let picks_at = if sh.from == 0 { vec2(BOARD.x + 6.0 * STEP - 50.0, 688.0) } else { vec2(PEEK.x + 30.0, PEEK.y + 165.0) };
    let surge_at = vec2(BOARD.x + 6.0 * STEP - 80.0, 311.0);
    for e in c.effects.iter().chain(&sh.brand) {
        match e {
            Effect::Burn(_) => {
                rise(z, 30, 30.0, mix(GOLD_C, BAD, 0.5));
                anim(FX_FLAME, z - vec2(0.0, 8.0), 110.0, 0.7, rgb(0xff8a4c));
                ring(z, 16.0, 74.0, 0, 0.0, 2.5, rgb(0xff8a4c), 0.35);
            }
            Effect::Shield(_) => {
                ring(a, 24.0, 72.0, 6, 0.0, 3.0, rgb(0x6fb6ff), 0.38);
                rise(a, 10, 28.0, rgb(0x6fb6ff));
            }
            Effect::DoubleShield(_) => {
                ring(a, 22.0, 88.0, 6, 0.0, 4.0, rgb(0x6fb6ff), 0.45);
                ring(a, 34.0, 108.0, 6, 30.0, 2.5, INK, 0.5);
                rise(a, 18, 36.0, rgb(0x6fb6ff));
            }
            Effect::ShieldBlast(_) => {
                stream(a, z, 22, rgb(0x6fb6ff));
                ring(z, 24.0, 110.0, 6, 0.0, 4.5, rgb(0x6fb6ff), 0.4);
            }
            Effect::Brittle => {
                ring(a, 78.0, 44.0, 6, 15.0, 3.0, rgb(0x6fb6ff), 0.35);
                burst(a, 16, 180.0, 5.0, rgb(0x6fb6ff));
            }
            Effect::Heal(_) => {
                rise(a, 22, 30.0, rgb(0x7bd88f));
                anim(FX_HEAL, a, 120.0, 0.5, rgb(0x7bd88f));
                ring(a, 18.0, 74.0, 0, 0.0, 2.5, rgb(0x7bd88f), 0.4);
            }
            Effect::Regen(_) => {
                rise(a, 18, 28.0, rgb(0x7bd88f));
                ring(a, 20.0, 68.0, 0, 0.0, 2.0, rgb(0x7bd88f), 0.42);
            }
            Effect::Armor(_) => {
                ring(a, 68.0, 40.0, 255, 0.0, 3.5, rgb(0x6fb6ff), 0.35);
                ring(a, 54.0, 32.0, 255, 0.0, 2.0, INK, 0.35);
            }
            Effect::Stoke(_) => {
                rise(a, 20, 26.0, GOLD);
                ring(a, 20.0, 78.0, 4, 0.0, 3.0, GOLD, 0.38);
            }
            Effect::Spikes(_) => {
                ring(a, 24.0, 78.0, 6, 30.0, 2.5, BLOOD, 0.34);
                burst(a, 18, 220.0, 5.0, BLOOD);
            }
            Effect::Surge(_) => {
                ring(a, 18.0, 70.0, 4, 0.0, 2.5, rgb(0xffe066), 0.32);
                stream(a, surge_at, 12, rgb(0xffe066));
            }
            Effect::Defuse(_) => {
                anim(FX_SLASH, z, 130.0, 0.3, BLOOD);
                ring(z, 26.0, 96.0, 6, 0.0, 3.5, BLOOD, 0.36);
                burst(z, 22, 260.0, 6.0, INK);
            }
            Effect::Gems(_) => {
                let to = if sh.from == 0 { vec2(120.0, 616.0) } else { vec2(PEEK.x + 24.0, 266.0) };
                stream(a, to, 18, GOLD);
                ring(a, 16.0, 58.0, 4, 0.0, 2.0, GOLD, 0.32);
            }
            Effect::SelfDamage(_) => {
                ring(a, 20.0, 62.0, 0, 0.0, 2.5, BLOOD, 0.3);
                burst(a, 12, 160.0, 5.0, BLOOD);
            }
            Effect::Drain(_) => stream(z, a, 16, mix(c2, INK, 0.3)),
            Effect::Pierce => burst(z, 28, 440.0, 8.0, rgb(0x6fb6ff)),
            Effect::Shatter(_) => {
                burst(foe_board, 24, 240.0, 6.0, INK);
                ring(foe_board, 14.0, 80.0, 4, 0.0, 2.5, INK, 0.36);
            }
            Effect::Bury(_) => {
                burst(foe_board, 22, 200.0, 6.0, BLOOD);
                ring(foe_board, 12.0, 86.0, 255, 0.0, 3.0, BLOOD, 0.4);
            }
            Effect::Blind(_) => {
                burst(foe_board, 18, 160.0, 5.0, rgb(0xd2b48c));
                ring(foe_board, 72.0, 22.0, 0, 0.0, 2.5, rgb(0xd2b48c), 0.38);
            }
            Effect::Detonate => {
                anim(FX_FLAME, z, 175.0, 0.55, rgb(0xff8a4c));
                ring(z, 20.0, 128.0, 0, 0.0, 4.5, rgb(0xff8a4c), 0.42);
                burst(z, 36, 420.0, 9.0, rgb(0xff8a4c));
            }
            Effect::Reveal(_) => {
                anim(FX_GALE, a, 90.0, 0.38, rgb(0x5fe0c8));
                stream(a, picks_at, 12, rgb(0x5fe0c8));
            }
            _ => {}
        }
    }
}

/// Draws the battle and handles input. Returns true once it is over and the player clicks Continue.
/// Game seconds a new wave takes to arrive; your input waits for it.
const WAVE_IN: f64 = 1.4;
/// Seconds a turn's banner stays up.
const TURN_BANNER: f32 = 0.95;
/// Seconds a rite's card is up before its bolt leaves: snappy for basics, theatrical for found rites.
fn card_lead(lvl: usize, basic: bool) -> f32 {
    if basic { 0.16 } else { 0.42 + 0.10 * lvl.saturating_sub(1) as f32 }
}
/// Seconds a rite's card stays up in all.
fn card_dur(lvl: usize) -> f32 {
    if lvl == 99 { 0.42 } else { card_lead(lvl, false) + 0.28 }
}
/// Seconds of quiet after a bolt lands before the next action starts.
const SETTLE: f64 = 0.28;

/// When the action that starts now actually gets the stage: once whatever is playing has finished.
/// A change of `d` HP on side `s`, leaving it at `hp`: its floating number and the bar's step, both
/// held until `at`, when the blow that caused it lands.
fn hp_number(fx: &mut Fx, s: usize, d: i32, hp: i32, at: f64) {
    if d <= -HUGE {
        fx.huge = Some((at, s));
    }
    let size = if d <= -HUGE { 120.0 } else { 38.0 + d.abs().min(40) as f32 * 1.1 };
    let (label, colour) = match d {
        // in colour, so they stand out against the white fighters: damage red, healing green
        _ if d <= -15 => (format!("{d}!"), BLOOD),
        _ if d < 0 => (format!("{d}"), BLOOD),
        _ => (format!("{d:+}"), rgb(0x7bd88f)),
    };
    fx.floats.push((STAND[s] - vec2(0.0, 30.0), label, colour, at, size));
    if d < 0 {
        fx.hit[s] = at;
    }
    fx.due[s].push((at, hp));
}

fn queued(now: f64) -> f64 {
    fx(|fx| fx.busy).max(now)
}

/// `ending` is set for the run's last fight (lost, or the final boss): its result card is then the
/// run's summary, holding the expedition it unlocks, if any.
pub fn battle_screen(b: &mut Battle, run: &Run, ending: Option<Option<usize>>, ai_timer: &mut f32) -> bool {
    let (floor, charms) = (run.floor, &run.charms[..]);
    let over = b.over();
    let intro = fx(|fx| fx.intro).filter(|t| now() - t < WAVE_IN);
    // your clicks wait for your turn's banner and for a felled enemy's replacement
    let mine = b.active == 0 && !over && intro.is_none() && fx(|fx| now() >= fx.turn_at && fx.fell.is_none());
    if is_mouse_button_pressed(MouseButton::Left) || is_mouse_button_released(MouseButton::Left) {
        let (p, down) = (mouse(), is_mouse_button_pressed(MouseButton::Left));
        trace(|| {
            let (turn_at, fell, drag) = fx(|fx| (fx.turn_at, fx.fell.is_some(), fx.drag));
            format!(
                "{} at ({:.0},{:.0}) mine={mine} active={} intro={} wait_turn={:.2} fell={fell} drag={drag:?} modal={} stacks={:?}",
                if down { "PRESS" } else { "release" }, p.x, p.y, b.active, intro.is_some(), (turn_at - now()).max(0.0), modal(), b.f[0].stacks
            )
        });
    }

    // compare with last frame: HP changes become floating numbers and a shake, new stacks bounce in,
    // and every skill that landed becomes a shot across the arena
    let now = now();
    // crystals claimed this frame fly to the stacks once the stacks are laid out below
    let mut claims = vec![];
    // who a rite landed on this frame: only then is a shield drop a hit, not its wearing off
    let mut struck = [false; 2];
    // unclamped raw HP damage from casts/smites this frame, so huge overkills still trigger impact frames
    let mut raw_dmg = [0i32; 2];
    for e in std::mem::take(&mut b.events) {
        trace(|| format!("event {e:?}"));
        match e {
            Event::Vent { side, tile, eased } => {
                play(Sfx::Flip);
                if side == 1 {
                    fx(|fx| fx.enemy_cursor = Some((tile, now)));
                }
                let center_of = |i: usize| {
                    if side == 0 {
                        BOARD + vec2((i % W) as f32 * STEP, (i / W) as f32 * STEP) + vec2(TILE, TILE) / 2.0
                    } else {
                        PEEK + vec2((i % W) as f32 * 25.0, (i / W) as f32 * 25.0) + vec2(11.5, 11.5)
                    }
                };
                let at = center_of(tile);
                let (r0, r1) = if side == 0 { (14.0, 82.0) } else { (6.0, 36.0) };
                ring(at, r0, r1, 255, 0.0, if side == 0 { 2.5 } else { 1.5 }, fade(rgb(0x5fe0c8), 0.8), 0.34);
                for &j in &eased {
                    let p = center_of(j);
                    fx(|fx| fx.tile_fx[side][j] = (now, -1, 0.0, 0.0));
                    rise(p, if side == 0 { 6 } else { 2 }, if side == 0 { 16.0 } else { 6.0 }, fade(rgb(0x5fe0c8), 0.8));
                }
            }
            Event::Claim { side, element, tiles } => {
                play(Sfx::Claim);
                for &i in &tiles {
                    if side == 1 {
                        fx(|fx| fx.enemy_cursor = Some((i, now)));
                    }
                    let (ix, iy) = ((i % W) as f32, (i / W) as f32);
                    fx(|fx| {
                        fx.tile_fx[side][i] = (now, 2, 0.0, 0.0);
                        // orthogonal neighbors kick physically outward away from the mined crystal
                        for j in Board::around(i) {
                            let dx = (j % W) as f32 - ix;
                            let dy = (j / W) as f32 - iy;
                            fx.tile_fx[side][j] = (now, 1, dx, dy);
                        }
                    });
                }
                claims.push((side, element, tiles));
            }
            Event::Cave { side } => {
                // the ground gives way: on your board it blasts and shakes the whole screen, on theirs it
                // crumbles; one sound each, so the two never land on the same frame
                if side != 0 {
                    play(Sfx::Mine);
                }
                fx(|fx| {
                    if side == 0 {
                        fx.boom = Some(now);
                    }
                    fx.busy = fx.busy.max(now + 0.5);
                });
            }
            Event::Cast { from, name, element, miss, skill, master, dmg } => {
                // in turn: basics snap quickly; found rites sweep dramatically before firing
                struck[1 - from] |= !miss;
                if !miss {
                    raw_dmg[1 - from] += dmg;
                }
                let start = queued(now);
                let lvl = skill.1;
                let basic = is_basic(&COMBOS[skill.0]);
                fx(|fx| {
                    let t0 = start + card_lead(lvl, basic) as f64;
                    let shot = Shot { from, t0, skill, brand: b.f[from].brands(&COMBOS[skill.0]).first().map(|x| x.effect), hits: b.hits(from, &COMBOS[skill.0]), miss, master };
                    fx.shots.push(shot);
                    // level 99 marks a compact basic banner so Attack/Shield/Heal stay snappy
                    fx.banner.push((name, el_color(element), from, start, if basic { 99 } else { lvl }));
                    if miss {
                        fx.floats.push((STAND[1 - from] - vec2(0.0, 30.0), "MISS".into(), MUTED, lands_at(&shot), 40.0));
                        fx.pending_fx.push((lands_at(&shot) - 0.06, DelayedFx::Dodge { side: 1 - from, strength: 1.0 }));
                    } else if master {
                        let first = t0 + ((charge(lvl) + 1.0) * SHOT_TIME) as f64;
                        // beside whoever it acts on, clear of their name and of the damage number
                        let m = COMBOS[skill.0].master;
                        let side = if mastery_on_foe(m) { 1 - from } else { from };
                        let at = STAND[side] + if side == 1 { vec2(280.0, 70.0) } else { vec2(170.0, -70.0) };
                        fx.floats.push((at, mastery_word(m).into(), MASTERED, first, 30.0));
                    }
                    // what the rite does besides damage, said where it lands, as it lands
                    if !miss {
                        for (k, (side, word)) in callouts(&b.f[from], &COMBOS[skill.0]).into_iter().enumerate() {
                            let side = if side { 1 - from } else { from };
                            let at = STAND[side] + if side == 1 { vec2(-250.0, 40.0) } else { vec2(150.0, -20.0) } + vec2(0.0, k as f32 * 30.0);
                            fx.floats.push((at, word, GOLD, lands_at(&shot), 26.0));
                        }
                    }
                    trace(|| format!("anim cast {name} by side {from}: card in {:.2}s, bolt {:.2}s, lands {:.2}s", start - now, t0 - now, lands_at(&shot) - now));
                    fx.busy = lands_at(&shot) + SETTLE;
                    // your rite's voice as it is thrown; a miss or a blow on you sounds as it lands (a
                    // beat after the bolt's own pop, so they do not blur)
                    if !miss && from == 0 {
                        fx.later.push((t0 + (charge(lvl) * SHOT_TIME) as f64, Sfx::Skill));
                    }
                    match (miss, from) {
                        (true, _) => fx.later.push((lands_at(&shot), Sfx::Miss)),
                        (_, 1) => fx.later.push((lands_at(&shot) + 0.06, Sfx::Hurt)),
                        _ => {}
                    }
                    match element {
                        Element::Spark => fx.later.push((t0 + (charge(lvl) * SHOT_TIME) as f64, Sfx::Zap)),
                        Element::Gale => fx.later.push((t0 + (charge(lvl) * SHOT_TIME) as f64, Sfx::Whoosh)),
                        _ => {}
                    }
                    if lvl >= 2 {
                        fx.later.push((t0, Sfx::Charge));
                    }
                });
            }
            Event::Smite { side, dmg } => {
                raw_dmg[side] += dmg;
                anim(FX_SLASH, STAND[side], 190.0, 0.36, INK);
                ring(STAND[side], 20.0, 150.0, 4, 0.0, 5.0, BLOOD, 0.38);
                burst(STAND[side], 60, 460.0, 11.0, BLOOD);
                burst(STAND[side], 30, 340.0, 8.0, INK);
                punch();
            }
            Event::Proc { side: 0, hook } => {
                for (k, &id) in charms.iter().enumerate() {
                    if CHARMS[id].hook == hook {
                        let slot_c = charm_slot(k, charms.len()).center();
                        fx(|fx| fx.charm_t.insert(id, now));
                        burst(slot_c, 12, 140.0, 5.0, GOLD_C);
                        stream(slot_c, STAND[0], 6, GOLD);
                    }
                }
            }
            Event::Proc { .. } => {}
            Event::Pass { hp, shield, wave } => fx(|fx| {
                if !fx.live {
                    return;
                }
                let at = fx.shots.iter().map(lands_at).chain(fx.boom).fold(now, f64::max);
                for s in 0..2 {
                    let sh = shield[s].max(0);
                    if sh != fx.shield[s] {
                        fx.due_shield[s].push((at, sh));
                        fx.shield[s] = sh;
                    }
                    let h = hp[s].max(0);
                    // a felled enemy's last blow is shown with the wave's change below
                    if (s == 1 && wave != fx.wave) || h == fx.hp[s] {
                        continue;
                    }
                    trace(|| format!("  side {s} hp {} -> {h} by the hand-over: number in {:.2}s", fx.hp[s], at - now));
                    hp_number(fx, s, h - fx.hp[s], h, at);
                    fx.hp[s] = h;
                }
            }),
            Event::Load => {
                play(Sfx::Charge);
                let at = queued(now);
                fx(|fx| {
                    fx.load_t = now;
                    fx.floats.push((STAND[1] + vec2(280.0, 70.0), "LOADED".into(), BLOOD, at, 26.0));
                });
                ring(STAND[1], 88.0, 42.0, 6, 0.0, 3.0, BLOOD, 0.36);
            }
            Event::Break => {
                // as the bolt that broke it lands
                let at = fx(|fx| fx.shots.last().map_or(now, lands_at));
                fx(|fx| {
                    fx.later.push((at, Sfx::Boom));
                    fx.pending_fx.push((at, DelayedFx::BreakBurst));
                    fx.floats.push((STAND[1] + vec2(280.0, 40.0), "BROKEN".into(), BLOOD, at, 40.0));
                });
                quake_at(at, 14.0);
            }
            Event::Brittle { side } => {
                let at = fx(|fx| fx.shots.last().map_or(now, lands_at));
                let pos = STAND[side] + if side == 1 { vec2(-300.0, 10.0) } else { vec2(170.0, -50.0) };
                fx(|fx| {
                    fx.later.push((at, Sfx::Boom));
                    fx.pending_fx.push((at, DelayedFx::BrittleBack { from: side }));
                    fx.floats.push((pos, "SHATTERED".into(), rgb(0x6fb6ff), at, 30.0));
                });
                quake_at(at, 10.0);
            }
            Event::Pierced { side } => {
                // where SHATTERED would show, so a brittle shield that never shattered back says why
                let at = fx(|fx| fx.shots.last().map_or(now, lands_at));
                let pos = STAND[side] + if side == 1 { vec2(-300.0, 10.0) } else { vec2(170.0, -50.0) };
                fx(|fx| {
                    fx.pending_fx.push((at, DelayedFx::PierceSlash { side }));
                    fx.floats.push((pos, "PIERCED".into(), rgb(0x6fb6ff), at, 26.0));
                });
            }
            Event::Burn { side, dmg } => {
                let at = fx(|fx| fx.shots.iter().map(lands_at).chain(fx.boom).fold(now, f64::max));
                fx(|fx| fx.pending_fx.push((at, DelayedFx::BurnTick { side })));
                let _ = dmg;
            }
            Event::Thorns { side, dmg } => {
                let at = fx(|fx| fx.shots.last().map_or(now, lands_at));
                // beside the attacker that took the thorns, clear of the enemy's right-side callouts
                let victim = 1 - side;
                let pos = STAND[victim] + if victim == 0 { vec2(150.0, 20.0) } else { vec2(-250.0, -10.0) };
                fx(|fx| {
                    fx.pending_fx.push((at + 0.04, DelayedFx::ThornsBack { from: side }));
                    fx.floats.push((pos, format!("THORNS -{dmg}"), BLOOD, at + 0.04, 22.0));
                });
            }
            Event::Graze { side } => {
                let at = fx(|fx| fx.shots.last().map_or(now, lands_at));
                fx(|fx| {
                    fx.pending_fx.push((at - 0.04, DelayedFx::Dodge { side, strength: 0.45 }));
                    fx.pending_fx.push((at, DelayedFx::GrazeSparks { side }));
                    fx.floats.push((STAND[side] + vec2(0.0, -54.0), "GRAZED".into(), MUTED, at, 26.0));
                });
            }
            Event::React { from, element } => {
                let word = react_word(element);
                let col = keyword(word).map_or(INK, |k| k.0);
                // beside the target, as the bolt that set it off lands; several stack downward
                let at = fx(|fx| fx.shots.last().map_or(now, lands_at));
                let n = fx(|fx| fx.floats.iter().filter(|f| f.3 == at).count()) as f32;
                let side = 1 - from;
                let pos = STAND[side] + if side == 1 { vec2(-300.0, 40.0 + 30.0 * n) } else { vec2(170.0, -20.0 + 30.0 * n) };
                fx(|fx| {
                    fx.floats.push((pos, word.to_uppercase(), col, at, 26.0));
                    fx.pending_fx.push((at + 0.04 * n as f64, DelayedFx::Reaction { from, element }));
                    // a chime per reaction, stepped apart when several go off from one bolt
                    fx.later.push((at + 0.08 + 0.09 * n as f64, Sfx::React));
                });
            }
        }
    }
    // a banner as each turn starts, once the last turn's actions have played out; the new side
    // waits for it to read
    let turn = (!over).then_some(b.active);
    let start = queued(now);
    let enemy_turn_began = fx(|fx| {
        let changed = fx.turn != turn;
        if changed {
            fx.turn = turn;
            if let Some(s) = turn {
                fx.banner.push((if s == 0 { "YOUR TURN" } else { "ENEMY TURN" }, if s == 0 { GOOD } else { BAD }, s, start, 0));
                fx.turn_at = start + 0.35;
                fx.busy = start + 0.7;
            }
        }
        changed && turn == Some(1)
    });
    if enemy_turn_began {
        *ai_timer = 0.0;
    }
    // sounds, delayed combat bursts, and a mine's blast whose moment has come
    let (due, due_fx, boom) = fx(|fx| {
        let (due, wait): (Vec<_>, Vec<_>) = fx.later.drain(..).partition(|x| x.0 <= now);
        fx.later = wait;
        let (due_fx, wait_fx): (Vec<_>, Vec<_>) = fx.pending_fx.drain(..).partition(|x| x.0 <= now);
        fx.pending_fx = wait_fx;
        let boom = fx.boom.filter(|&t| t <= now);
        if boom.is_some() {
            fx.boom = None;
        }
        (due, due_fx, boom)
    });
    for (_, s) in due {
        play(s);
    }
    for (_, dfx) in due_fx {
        match dfx {
            DelayedFx::Dodge { side, strength } => fx(|fx| fx.dodge[side] = (now, strength)),
            DelayedFx::ShieldHit { side } => {
                fx(|fx| fx.shield_hit[side] = now);
                ring(STAND[side], 30.0, 96.0, 6, 0.0, 3.5, rgb(0x6fb6ff), 0.32);
                burst(STAND[side], 14, 210.0, 5.0, rgb(0x6fb6ff));
            }
            DelayedFx::ShieldBreak { side } => {
                fx(|fx| fx.shield_hit[side] = now);
                ring(STAND[side], 34.0, 124.0, 6, 15.0, 4.5, rgb(0x6fb6ff), 0.38);
                ring(STAND[side], 24.0, 92.0, 6, 45.0, 2.5, INK, 0.3);
                burst(STAND[side], 28, 300.0, 7.0, rgb(0x6fb6ff));
                burst(STAND[side], 14, 220.0, 5.0, INK);
            }
            DelayedFx::BreakBurst => {
                fx(|fx| fx.break_t = now);
                ring(STAND[1], 32.0, 148.0, 6, 0.0, 5.0, BLOOD, 0.42);
                ring(STAND[1], 24.0, 118.0, 4, 20.0, 3.0, INK, 0.36);
                burst(STAND[1], 36, 360.0, 8.0, BLOOD);
                burst(STAND[1], 18, 280.0, 6.0, INK);
            }
            DelayedFx::BrittleBack { from } => {
                let to = 1 - from;
                stream(STAND[from], STAND[to], 24, rgb(0x6fb6ff));
                ring(STAND[from], 24.0, 108.0, 6, 15.0, 4.0, rgb(0x6fb6ff), 0.38);
                burst(STAND[to], 22, 260.0, 7.0, rgb(0x6fb6ff));
            }
            DelayedFx::PierceSlash { side } => {
                anim(FX_SLASH, STAND[side], 150.0, 0.32, rgb(0x6fb6ff));
                ring(STAND[side], 24.0, 106.0, 6, 0.0, 3.5, rgb(0x6fb6ff), 0.34);
                burst(STAND[side], 22, 300.0, 6.0, rgb(0x6fb6ff));
            }
            DelayedFx::Reaction { from, element } => {
                let target = 1 - from;
                match element {
                    Element::Ember => {
                        anim(FX_FLAME, STAND[target], 125.0, 0.5, rgb(0xff8a4c));
                        ring(STAND[target], 18.0, 88.0, 0, 0.0, 3.0, rgb(0xff8a4c), 0.36);
                    }
                    Element::Tide => {
                        stream(STAND[target], STAND[from], 14, rgb(0x6fb6ff));
                        ring(STAND[from], 22.0, 80.0, 6, 0.0, 3.0, rgb(0x6fb6ff), 0.36);
                    }
                    Element::Gale => {
                        anim(FX_GALE, STAND[target], 110.0, 0.4, rgb(0x5fe0c8));
                        let picks = if from == 0 { vec2(BOARD.x + 6.0 * STEP - 50.0, 688.0) } else { vec2(PEEK.x + 30.0, PEEK.y + 165.0) };
                        stream(STAND[target], picks, 12, rgb(0x5fe0c8));
                    }
                    Element::Stone => {
                        ring(STAND[target], 20.0, 105.0, 4, 0.0, 4.5, rgb(0xc9a77c), 0.34);
                        burst(STAND[target], 24, 320.0, 7.0, rgb(0xc9a77c));
                        quake(10.0);
                    }
                    Element::Spark => {
                        stream(STAND[target], STAND[from], 14, rgb(0xffe066));
                        ring(STAND[from], 16.0, 74.0, 4, 0.0, 3.0, rgb(0xffe066), 0.32);
                    }
                }
            }
            DelayedFx::ThornsBack { from } => {
                let to = 1 - from;
                stream(STAND[from], STAND[to], 12, BLOOD);
                burst(STAND[to], 10, 180.0, 5.0, BLOOD);
            }
            DelayedFx::GrazeSparks { side } => {
                burst(STAND[side] + vec2(18.0, -14.0), 14, 220.0, 5.0, INK);
                ring(STAND[side], 14.0, 62.0, 4, 30.0, 2.0, MUTED, 0.25);
            }
            DelayedFx::BurnTick { side } => {
                anim(FX_FLAME, STAND[side], 115.0, 0.5, rgb(0xff8a4c));
                rise(STAND[side], 18, 28.0, rgb(0xff8a4c));
                ring(STAND[side], 16.0, 70.0, 0, 0.0, 2.5, rgb(0xff8a4c), 0.34);
            }
        }
    }
    if boom.is_some() {
        play(Sfx::Boom);
        quake(20.0);
        flash(BAD);
        hitstop(0.05);
        punch();
    }

    // gems slotted into skill sockets stay in your stacks until the skill fires; forget any the
    // battle has since taken away (snatched by the enemy)
    let mut sockets = fx(|fx| std::mem::take(&mut fx.sockets));
    let mut reserved = [0usize; 5];
    for &e in &b.f[0].elements {
        let have = b.f[0].stacks.iter().filter(|&&x| x == e).count();
        for v in sockets.values_mut() {
            v.retain(|&x| x != e || {
                reserved[e as usize] += 1;
                reserved[e as usize] <= have
            });
        }
        reserved[e as usize] = reserved[e as usize].min(have);
    }
    // the gems still loose in your stack row, by index into your stacks
    let free: Vec<usize> = {
        let mut skip = reserved;
        (0..b.f[0].stacks.len())
            .filter(|&i| {
                let s = &mut skip[b.f[0].stacks[i] as usize];
                let loose = *s == 0;
                *s = s.saturating_sub(1);
                loose
            })
            .collect()
    };
    let mut drag = fx(|fx| fx.drag).filter(|d| mine && free.contains(d));
    let dragged = drag.map(|d| b.f[0].stacks[d]);
    let mut felled = None;
    let fallen_now = b.fallen.take();
    let (shake, ghost, bar, vis_shield, stack_t) = fx(|fx| {
        // anything that changed shows once every bolt in flight, and a mine about to blow, has landed
        let at = fx.shots.iter().map(lands_at).chain(fx.boom).fold(now, f64::max);
        if fx.live && fx.wave != b.wave {
            fx.pops.clear(); // fresh boards
            // the old enemy stays up until the blow that felled it lands
            fx.fallen = fallen_now;
            fx.fell = Some((at, 1));
            fx.busy = fx.busy.max(at + WAVE_IN);
            trace(|| format!("anim wave {} -> {}: old enemy falls in {:.2}s", fx.wave, b.wave, at - now));
        }
        if over && !fx.ended {
            fx.ended = true;
            fx.ended_at = f64::MAX;
            fx.fell = Some((at, if b.won() { 1 } else { 0 }));
        }
        let mut shake = [0.0f32; 2];
        for s in 0..2 {
            let f = &b.f[s];
            let (hp, n) = (f.hp.max(0), f.stacks.len());
            let next_wave = fx.live && s == 1 && fx.wave != b.wave;
            if !fx.live {
                trace(|| format!("  screen fresh: side {s} hp taken as {hp}"));
                fx.ghost[s] = hp as f32;
                fx.bar[s] = hp;
                fx.vis_shield[s] = f.shield;
                fx.shield[s] = f.shield;
                fx.battle_t = now;
                fx.tab_x = BOARD.x;
                fx.surge_seen[s] = f.surge;
                fx.picks_seen[s] = f.reveals_left;
                fx.tile_board[s] = f.cur;
                for (i, t) in f.board().tiles.iter().enumerate() {
                    fx.tile_crack[s][i] = t.crack;
                }
            }
            // track tile crack increases/decreases on the active board for jolt/ease feedback
            if fx.live && !next_wave {
                let bd = f.board();
                if fx.tile_board[s] != f.cur {
                    if s == 0 {
                        fx.board_dir = if f.cur > fx.tile_board[0] { 1.0 } else { -1.0 };
                    }
                    fx.tile_board[s] = f.cur;
                    for (i, t) in bd.tiles.iter().enumerate() {
                        fx.tile_crack[s][i] = t.crack;
                        fx.tile_fx[s][i] = (0.0, 0, 0.0, 0.0);
                    }
                } else {
                    for (i, t) in bd.tiles.iter().enumerate() {
                        if t.crack > fx.tile_crack[s][i] && !t.caved && fx.tile_fx[s][i].0 < now - 0.05 {
                            fx.tile_fx[s][i] = (now, 1, 0.0, 0.0);
                        } else if t.crack < fx.tile_crack[s][i] && fx.tile_fx[s][i].0 < now - 0.05 {
                            fx.tile_fx[s][i] = (now, -1, 0.0, 0.0);
                        }
                        fx.tile_crack[s][i] = t.crack;
                    }
                }
                if f.surge != fx.surge_seen[s] {
                    if f.surge > fx.surge_seen[s] {
                        for k in fx.surge_seen[s].max(0)..f.surge.min(16) {
                            fx.surge_t[s][k as usize] = now;
                        }
                    }
                    fx.surge_pop[s] = now;
                    fx.surge_seen[s] = f.surge;
                }
                if f.reveals_left != fx.picks_seen[s] {
                    fx.picks_pop[s] = now;
                    fx.picks_seen[s] = f.reveals_left;
                }
            }
            // a wave's killing blow (or huge overkill) keeps its full raw damage so impact frames still fire
            let clamped_d = if next_wave { -fx.hp[s] } else if !fx.live { 0 } else { hp - fx.hp[s] };
            let d = if raw_dmg[s] > 0 && (-raw_dmg[s]) < clamped_d { -raw_dmg[s] } else { clamped_d };
            if d != 0 {
                hp_number(fx, s, d, if next_wave { 0 } else { hp }, at);
                trace(|| format!("  side {s} hp {} -> {hp}: number {d} in {:.2}s", fx.hp[s], at - now));
            }
            if fx.live && !next_wave {
                for k in fx.stacks[s]..n.min(16) {
                    fx.stack_t[s][k] = now;
                }
            }
            // shield changes wait for their cast or hit to land so the ward never vanishes early
            let ds = f.shield - fx.shield[s];
            if fx.live && !next_wave && ds != 0 {
                if struck[s] && ds < 0 {
                    fx.floats.push((STAND[s] + vec2(0.0, 20.0), format!("{ds} shield"), rgb(0x6fb6ff), at, 30.0));
                    if f.shield == 0 {
                        fx.pending_fx.push((at, DelayedFx::ShieldBreak { side: s }));
                    } else {
                        fx.pending_fx.push((at, DelayedFx::ShieldHit { side: s }));
                    }
                }
                fx.due_shield[s].push((at, f.shield));
            }
            fx.shield[s] = f.shield;
            fx.hp[s] = hp;
            fx.stacks[s] = n;
            // each hit moves the bar as it lands, not all of them at the last one
            while fx.due[s].first().is_some_and(|d| now >= d.0) {
                let (_, v) = fx.due[s].remove(0);
                trace(|| format!("  side {s} bar {} -> {v}", fx.bar[s]));
                if v < fx.bar[s] {
                    fx.bar_flash[s] = now;
                } else if v > fx.bar[s] {
                    fx.heal_flash[s] = now;
                }
                fx.bar[s] = v;
            }
            while fx.due_shield[s].first().is_some_and(|d| now >= d.0) {
                let (_, v) = fx.due_shield[s].remove(0);
                if v > fx.vis_shield[s] {
                    fx.shield_gain[s] = now;
                }
                fx.vis_shield[s] = v;
            }
            fx.ghost[s] += (fx.bar[s] as f32 - fx.ghost[s]) * (dt() * 3.0).min(1.0);
            let since = (now - fx.hit[s]) as f32;
            if (0.0..0.45).contains(&since) {
                shake[s] = (since * 75.0).sin() * 7.0 * (1.0 - since / 0.45);
            }
        }
        // the new enemy steps in: its name and full bar replace the fallen one's
        if fx.takeover.is_some_and(|t| now >= t) {
            fx.takeover = None;
            fx.fallen = None;
            let hp = b.f[1].hp.max(0);
            trace(|| format!("  takeover: bar {} -> {hp}, pending {:?} dropped", fx.bar[1], fx.due[1]));
            (fx.bar[1], fx.ghost[1]) = (hp, hp as f32);
            fx.vis_shield[1] = b.f[1].shield;
            fx.due[1].clear();
            fx.due_shield[1].clear();
        }
        // the killing blow lands
        if let Some((_, s)) = fx.fell.filter(|f| now >= f.0) {
            trace(|| format!("anim side {s} falls"));
            fx.fell = None;
            felled = Some(s);
        }
        fx.live = true;
        fx.wave = b.wave;
        (shake, fx.ghost, fx.bar, fx.vis_shield, fx.stack_t)
    });
    match felled {
        Some(1) => {
            // the enemy bursts apart in slow motion, the sound going dull around it
            play(Sfx::Kill);
            slowmo(if over { 1.0 } else { 0.7 });
            hitstop(0.08);
            punch();
            quake(26.0);
            flash(INK);
            burst(STAND[1], 140, 600.0, 14.0, INK);
            burst(STAND[1], 90, 440.0, 11.0, BAD);
            rise(STAND[1], 50, 50.0, GOLD_C);
            let sprite = fx(|fx| fx.fallen.as_ref().map_or(b.f[1].sprite, |f| f.sprite));
            fx(|fx| fx.vanish = Some((sprite, now)));
            if over {
                play(Sfx::Win);
                fx(|fx| fx.ended_at = get_time());
            } else {
                // the fallen enemy's name and emptied bar (draining in slow motion) hold until the new
                // enemy is announced once the dust settles; it takes over as it walks in
                let wave = ["WAVE 2", "WAVE 3", "WAVE 4"][b.wave.saturating_sub(2).min(2)];
                fx(|fx| {
                    fx.takeover = Some(now + 0.45);
                    fx.intro = Some(now);
                    fx.banner.push((wave, BAD, 1, now + 0.45, 1));
                    fx.busy = fx.busy.max(now + WAVE_IN);
                });
            }
        }
        Some(_) => {
            // you fall: the same slow, dull moment, in red
            play(Sfx::Lose);
            slowmo(1.0);
            hitstop(0.08);
            quake(22.0);
            flash(BAD);
            burst(STAND[0], 110, 480.0, 12.0, BAD);
            burst(STAND[0], 50, 320.0, 9.0, INK);
            fx(|fx| fx.ended_at = get_time());
        }
        None => {}
    }
    let fallen = fx(|fx| fx.fallen.clone());
    let (mut dig_at, mut vent_at, mut swap, mut cast) = (None, None, None, None);

    // where the run stands, top left, above your pacts
    // the counter moves on with the takeover, not while the last wave's enemy is still dying
    let wave = b.wave - usize::from(fallen.is_some() && b.wave > 1);
    label(&format!("FLOOR {}  ·  WAVE {}/{}  ·  TURN {}", floor + 1, wave, b.waves, b.turn), 56.0, 40.0);
    // the warning shows only once it is a few turns off
    if b.pressure() > 0 || b.turn + 3 >= PRESSURE_TURN {
        let note = if b.pressure() > 0 { format!("Overpressure: every rite +{}", b.pressure()) } else { format!("Overpressure after turn {PRESSURE_TURN}") };
        rich(&note, 56.0, 116.0, 15.0, if b.pressure() > 0 { INK } else { MUTED });
    }

    let (bar_flash, heal_flash, tile_fx, surge_t, surge_pop, picks_pop, battle_t, dodge, shield_hit, shield_gain, load_t, break_t, board_dir, redeal_t, chip_pop, cast_kick, enemy_cursor) = fx(|fx| {
        (
            fx.bar_flash, fx.heal_flash, fx.tile_fx, fx.surge_t, fx.surge_pop, fx.picks_pop,
            fx.battle_t, fx.dodge, fx.shield_hit, fx.shield_gain, fx.load_t, fx.break_t,
            fx.board_dir, fx.redeal_t, fx.chip_pop, fx.cast_kick, fx.enemy_cursor,
        )
    });
    // precompute hovered lit hitting rite so the enemy's HP bar and break bar preview it cleanly this frame
    let hover_dmg: i32 = if mine && drag.is_none() && !modal() {
        let ready = b.ready(0);
        let (basics, list): (Vec<&'static Combo>, Vec<&'static Combo>) = b.f[0].rite_list().into_iter().partition(|c| is_basic(c));
        let rx = 880.0;
        let mut dmg = 0;
        for (k, &c) in basics.iter().enumerate() {
            let r = Rect::new(rx - 10.0 + k as f32 * 124.0, 278.0, 118.0, 42.0);
            if r.contains(mouse()) && ready.iter().any(|x| std::ptr::eq(*x, c)) && b.hits(0, c) {
                dmg = b.hit_damage(0, c);
            }
        }
        let rh = (300.0 / list.len().max(1) as f32).min(75.0);
        for (k, &c) in list.iter().enumerate() {
            let r = Rect::new(rx - 10.0, 326.0 + k as f32 * rh, 370.0, rh);
            if r.contains(mouse()) && ready.iter().any(|x| x.name == c.name) && b.hits(0, c) {
                dmg = b.hit_damage(0, c);
            }
        }
        dmg
    } else {
        0
    };

    // the enemy, alone at the top: name, life, and what it would throw if it fired now
    {
        let foe = fallen.as_ref().unwrap_or(&b.f[1]);
        let ex = FOE.x + shake[1];
        if is_boss(&foe.name) {
            lively_centered(&foe.name, 640.0 + shake[1], 186.0, 40.0, BLOOD, Motion::Tremble);
        } else {
            stamped(&foe.name, 640.0 + shake[1], 186.0, 40.0, INK);
        }
        rich_centered(&foe.desc, 640.0 + shake[1], 210.0, 15.0, MUTED);
        label("HP", ex, 236.0);
        right(&format!("{} / {}", bar[1], foe.max_hp), ex + FOE.w, 236.0, 16.0, INK);
        let foe_hit_fl = (1.0 - (now - bar_flash[1]) as f32 / 0.14).clamp(0.0, 1.0);
        let foe_heal_fl = (1.0 - (now - heal_flash[1]) as f32 / 0.25).clamp(0.0, 1.0);
        // HP loss preview accounts for shield soak (unless empty)
        let hp_cut = if hover_dmg > 0 && bar[1] > 0 {
            let k = 1 + foe.mods.shield_block.max(0);
            let soak = (foe.shield * k).min(hover_dmg).max(0);
            (hover_dmg - soak).max(0) as f32 / foe.max_hp.max(1) as f32
        } else {
            0.0
        };
        life(
            Rect::new(ex, 244.0, FOE.w, 8.0),
            bar[1] as f32 / foe.max_hp.max(1) as f32,
            ghost[1] / foe.max_hp.max(1) as f32,
            vis_shield[1] as f32 / foe.max_hp.max(1) as f32,
            foe_hit_fl,
            foe_heal_fl,
            hp_cut,
        );
        status_line(foe, vis_shield[1], ex, 276.0);
        // what it has loaded fires as its next turn starts; a felled enemy throws nothing more
        let loaded: Vec<&'static Combo> = if bar[1] > 0 && fallen.is_none() { foe.loaded.clone() } else { vec![] };
        let what = match loaded.as_slice() {
            [] => "nothing loaded".to_string(),
            [c] => foe_title(foe, c),
            [c, rest @ ..] => format!("{} +{}", foe_title(foe, c), rest.len()),
        };
        let mut x = ex + FOE.w;
        if !loaded.is_empty() {
            let s = format!("{} dmg", b.loaded_damage());
            right(&s, x, 294.0, 16.0, BLOOD);
            x -= measure(&s, 16.0).width + 12.0;
            for &e in foe.cost(loaded[0]).iter().rev() {
                gem(e, vec2(x - 8.0, 289.0), 8.0);
                x -= 20.0;
            }
            // the break: how much of it your rites have dealt this turn (once broken, it hits for half)
            let (took, need) = (foe.took.min(foe.break_at), foe.break_at);
            if foe.broken {
                rich("broken: half", ex, 294.0, 15.0, INK);
            } else {
                let target_f = took as f32 / need.max(1) as f32;
                let (vis, bflash) = fx(|fx| {
                    if target_f > fx.break_vis + 0.01 {
                        fx.break_flash = now;
                    }
                    fx.break_vis += (target_f - fx.break_vis) * (dt() * 12.0).min(1.0);
                    if (fx.break_vis - target_f).abs() < 0.002 {
                        fx.break_vis = target_f;
                    }
                    (fx.break_vis, (1.0 - (now - fx.break_flash) as f32 / 0.25).clamp(0.0, 1.0))
                });
                let will_break = hover_dmg > 0 && took + hover_dmg >= need;
                let brk_txt = if will_break { "BREAK!".to_string() } else { format!("break {took}/{need}") };
                if will_break {
                    lively(&brk_txt, ex, 294.0, 15.0, BLOOD, Motion::Tremble);
                } else {
                    rich(&brk_txt, ex, 294.0, 15.0, INK);
                }
                let w = 90.0;
                let bx = ex + measure(&format!("break {took}/{need}"), 15.0).width + 10.0;
                draw_rectangle(bx, 288.0, w, 4.0, FAINT);
                if hover_dmg > 0 {
                    let prev_f = ((took + hover_dmg).min(need) as f32) / need.max(1) as f32;
                    let pulse = 0.4 + 0.45 * ((now as f32 * 8.0).sin() * 0.5 + 0.5);
                    draw_rectangle(bx, 288.0, w * prev_f, 4.0, fade(if will_break { GOLD } else { BLOOD }, pulse));
                }
                draw_rectangle(bx, 288.0, w * vis, 4.0, mix(BLOOD, INK, bflash));
            }
        }
        let next_left = x - 16.0 - measure(&what, 16.0).width - measure("NEXT", 15.0).width;
        right(&what, x - 4.0, 294.0, 16.0, if loaded.is_empty() { MUTED } else { INK });
        right("NEXT", x - 16.0 - measure(&what, 16.0).width, 294.0, 15.0, MUTED);
        // hover NEXT to inspect all loaded enemy rites and their effects
        if !loaded.is_empty() && !modal() && Rect::new(next_left - 4.0, 276.0, ex + FOE.w - next_left + 8.0, 24.0).contains(mouse()) {
            let mut lines: Vec<String> = loaded.iter().map(|c| format!("{}: {}", foe_title(foe, c), foe.describe(c))).collect();
            if foe.broken {
                lines.push("Broken: fires at half damage.".into());
            } else {
                lines.push(format!("Deal {} rite damage this turn to halve it.", (foe.break_at - foe.took).max(0)));
            }
            tip_text(&format!("Loaded ({} dmg)", b.loaded_damage()), lines.join("\n"));
        }

        // its board, shrunk to a preview, and its stacks
        let fb = foe.board();
        // its turn: the panel says so and is framed, so the eye goes where it is picking
        if b.active == 1 && !over {
            let pulse = 0.65 + 0.35 * ((now as f32 * 5.0).sin() * 0.5 + 0.5);
            text("ITS TURN", PEEK.x, PEEK.y - 12.0, 15.0, BLOOD);
            outline(Rect::new(PEEK.x - 5.0, PEEK.y - 5.0, W as f32 * 25.0 + 8.0, W as f32 * 25.0 + 8.0), fade(BLOOD, pulse));
        } else {
            label("ITS BOARD", PEEK.x, PEEK.y - 12.0);
        }
        // its board signs follow the label, clear of the settings gear in the corner
        let x0 = PEEK.x + measure("ITS BOARD", 15.0).width + 18.0;
        for (k, bd) in foe.boards.iter().enumerate() {
            let at = vec2(x0 + k as f32 * 20.0, PEEK.y - 17.0);
            sign(bd.element, at, if k == foe.cur { 7.5 } else { 6.5 }, if k == foe.cur { INK } else { FAINT });
        }
        let peek = |i: usize| Rect::new(PEEK.x + (i % W) as f32 * 25.0, PEEK.y + (i / W) as f32 * 25.0, 23.0, 23.0);
        for (i, t) in fb.tiles.iter().enumerate() {
            let mut r = peek(i);
            let key = (1, foe.cur, i);
            let (tfx_t, tfx_k, dx, dy) = tile_fx[1][i];
            let el = (now - tfx_t) as f32;
            if tfx_k != 0 && (0.0..0.28).contains(&el) {
                let p = 1.0 - el / 0.28;
                if tfx_k == 1 {
                    let kick = (el / 0.28 * std::f32::consts::PI).sin();
                    r.x += dx * 2.5 * kick + (el * 90.0).sin() * 1.5 * p;
                    r.y += dy * 2.5 * kick;
                } else if tfx_k == -1 {
                    r.y -= p * 2.5;
                } else if tfx_k == 2 {
                    r = scaled(r, 1.0 - 0.18 * (p * std::f32::consts::PI).sin());
                }
            }
            if t.rock {
                block(r, rgb(0x161616));
            } else if t.caved {
                if !fx(|fx| fx.pops.contains_key(&key)) {
                    burst(r.center(), 16, 160.0, 5.0, MUTED);
                    ring(r.center(), 4.0, 24.0, 255, 0.0, 1.8, BLOOD, 0.28);
                }
                born(key);
                block(r, rgb(0x0a0a0a));
            } else {
                unpop(key);
                let mut col = strain(t.crack);
                if tfx_k == 1 && (0.0..0.22).contains(&el) {
                    col = mix(col, INK, (1.0 - el / 0.22) * 0.55);
                } else if tfx_k == -1 && (0.0..0.28).contains(&el) {
                    col = mix(col, rgb(0x5fe0c8), (1.0 - el / 0.28) * 0.4);
                }
                block(r, col);
                if t.crystal {
                    sign(fb.element, r.center(), 6.0, if t.used { FAINT } else { INK });
                }
            }
        }
        // crisp highlight box on the exact tile the enemy just picked on PEEK
        if let Some((ci, ct)) = enemy_cursor {
            let el = (now - ct) as f32;
            if (0.0..0.42).contains(&el) {
                let p = 1.0 - el / 0.42;
                let cr = grow(peek(ci), 2.0 * (1.0 - p));
                outline(cr, fade(INK, p));
            }
        }
        rich(&format!("{} picks", foe.reveals_left), PEEK.x, PEEK.y + 170.0, 14.0, MUTED);
        let mut x = PEEK.x;
        for (e, n) in by_element(&foe.elements, foe.stacks.iter().copied()) {
            sign(e, vec2(x + 8.0, 262.0), 8.0, if n > 0 { INK } else { FAINT });
            text(&n.to_string(), x + 22.0, 268.0, 16.0, if n > 0 { INK } else { MUTED });
            x += 46.0;
        }
    }

    // you, on the left: name, life, conditions and your stacks, one chip per element
    let me = &b.f[0];
    let hx = 56.0 + shake[0];
    // the passive waits behind the name: hover it to read
    text(&me.name, hx, 450.0, 34.0, INK);
    if Rect::new(hx, 418.0, measure(&me.name, 34.0).width, 40.0).contains(mouse()) {
        let head = if run.trait_rank > 0 { format!("Passive · Rank {}", run.trait_rank + 1) } else { "Passive".into() };
        tip_text(&head, me.desc.clone());
    }
    label("HP", hx, 506.0);
    right(&format!("{} / {}", bar[0], me.max_hp), hx + 330.0, 506.0, 16.0, INK);
    let me_hit_fl = (1.0 - (now - bar_flash[0]) as f32 / 0.14).clamp(0.0, 1.0);
    let me_heal_fl = (1.0 - (now - heal_flash[0]) as f32 / 0.25).clamp(0.0, 1.0);
    life(
        Rect::new(hx, 514.0, 330.0, 8.0),
        bar[0] as f32 / me.max_hp.max(1) as f32,
        ghost[0] / me.max_hp.max(1) as f32,
        vis_shield[0] as f32 / me.max_hp.max(1) as f32,
        me_hit_fl,
        me_heal_fl,
        0.0,
    );
    status_line(me, vis_shield[0], hx, 546.0);
    let held = by_element(&me.elements, free.iter().map(|&k| me.stacks[k]));
    let chip_step = 78.0f32.min(330.0 / held.len().max(1) as f32);
    let chip = |j: usize| Rect::new(hx + j as f32 * chip_step, 596.0, chip_step - 10.0, 40.0);
    let chip_of = |e: Element| held.iter().position(|x| x.0 == e).map_or(vec2(hx, 616.0), |j| chip(j).center());
    // just claimed: crystal shatters with a pick-slash, launches a real flying alchemical gem to your
    // stack chip, and shoots a surge spark up to the surge bar
    for (side, element, tiles) in &claims {
        for &i in tiles {
            let (at, to, n) = if *side == 0 {
                let at = BOARD + vec2((i % W) as f32 * STEP, (i / W) as f32 * STEP) + vec2(TILE, TILE) / 2.0;
                (at, chip_of(*element), 12)
            } else {
                (PEEK + vec2((i % W) as f32 * 25.0, (i / W) as f32 * 25.0) + vec2(11.5, 11.5), vec2(PEEK.x + 20.0, 266.0), 10)
            };
            let tint = mix(INK, hue(*element), 0.5);
            stream(at, to, n, tint);
            if *side == 0 {
                fx(|fx| fx.flying_gems.push(FlyingGem { element: *element, from: at, to, t0: now, dur: 0.24 }));
                let surge_pip = vec2(BOARD.x + 6.0 * STEP - 48.0 - (b.surge_cap(0) - b.f[0].surge).max(0) as f32 * 13.0, 311.0);
                stream(at, surge_pip, 5, rgb(0xffe066));
                anim(FX_SLASH, at, 66.0, 0.16, tint);
                burst(at, 14, 180.0, 6.0, tint);
                ring(at, 8.0, 50.0, 4, 0.0, 2.8, tint, 0.28);
            } else {
                ring(at, 4.0, 22.0, 4, 0.0, 1.5, tint, 0.24);
            }
        }
    }

    // your board, in the middle, with whose turn it is and the surge above it
    let board = me.board();
    let turn_label = if over { "" } else if b.active == 0 { "YOUR TURN" } else { "ITS TURN" };
    text(turn_label, BOARD.x, 316.0, 15.0, if b.active == 0 { INK } else { MUTED });
    let (surge, cap) = (b.f[b.active].surge, b.surge_cap(b.active));
    let spop = (1.0 - (now - surge_pop[b.active]) as f32 / 0.22).clamp(0.0, 1.0);
    let times = format!("x{:.1}", 1.0 + (surge * b.surge_step(b.active)) as f32 / 100.0);
    let mut x = BOARD.x + 6.0 * STEP - 2.0;
    let t_col = if surge > 0 { mix(INK, rgb(0xffe066), spop * 0.7) } else { MUTED };
    right(&times, x, 316.0 - spop * 2.0, 15.0, t_col);
    x -= measure(&times, 15.0).width + 14.0;
    for k in (0..cap).rev() {
        let lit = k < surge;
        let pb = if lit { bounce(surge_t[b.active][(k as usize).min(15)]) } else { 1.0 };
        pip(vec2(x - 5.0, 311.0), 5.0 * pb, lit);
        x -= 13.0;
    }
    right("SURGE", x - 2.0, 316.0, 15.0, MUTED);
    // a board dealt anew (fewer tiles mined or fallen than a moment ago) or another one put up drops in, tile by tile
    let shown = board.tiles.iter().filter(|t| t.used || t.caved).count();
    let (came, redealt) = fx(|fx| {
        let (k, n, _) = fx.board_seen;
        let (new, redealt) = (k != me.cur || shown < n, k == me.cur && shown < n && fx.intro.is_none() && fx.fallen.is_none());
        fx.board_seen = (me.cur, shown, if new { now } else { fx.board_seen.2 });
        (fx.board_seen.2, redealt)
    });
    if redealt {
        play(Sfx::Whoosh);
        fx(|fx| {
            fx.redeal_t = now;
            fx.floats.push((BOARD + vec2(W as f32 * STEP / 2.0, -8.0), "FRESH VEIN".into(), GOLD, now, 22.0));
        });
        ring(BOARD + vec2(W as f32 * STEP / 2.0, W as f32 * STEP / 2.0), 150.0, 205.0, 255, 0.0, 2.5, fade(INK, 0.65), 0.34);
    }
    // subtle horizontal glide when swapping boards
    let swap_p = ease_out3(((now - came) as f32 / 0.18).clamp(0.0, 1.0));
    let board_slide_x = if came > 0.0 && now - redeal_t > 0.4 { board_dir * 32.0 * (1.0 - swap_p) } else { 0.0 };
    let tile_at = |i: usize| Rect::new(BOARD.x + board_slide_x + (i % W) as f32 * STEP, BOARD.y + (i / W) as f32 * STEP, TILE, TILE);
    let hover = (0..N).find(|&i| tile_at(i).contains(mouse())).filter(|_| mine && !modal());
    // venting the tile under the mouse: the tiles it would ease
    let eases = hover.map(|i| board.vent_eases(i)).unwrap_or_default();
    if let Some(i) = hover.filter(|_| clicked()) {
        if board.live(i) {
            dig_at = Some(i);
        } else if !eases.is_empty() {
            vent_at = Some(i);
        }
    }
    // mining the crystal under the mouse: the tiles it would crack, and those that would fall
    let aim = hover.filter(|&i| board.live(i));
    let fall = aim.map(|i| board.would_cave(i)).unwrap_or_default();
    let shaken: Vec<usize> = aim.map(|i| Board::around(i).filter(|&j| board.standing(j)).collect()).unwrap_or_default();
    for (i, t) in board.tiles.iter().enumerate() {
        let r = tile_at(i);
        // corner to corner, each tile a beat after the one before; it falls a little as it grows
        let p = (((now - came - ((i % W) + (i / W)) as f64 * 0.025) / 0.2) as f32).clamp(0.0, 1.0);
        let k = if came > 0.0 { 1.0 - (1.0 - p).powi(3) } else { 1.0 };
        if k < 0.05 {
            continue;
        }
        let r = scaled(Rect::new(r.x, r.y - (1.0 - k) * 18.0, r.w, r.h), k);
        let mid = r.center();
        let key = (2, me.cur, i);
        if t.rock {
            rock(r);
            continue;
        }
        if t.caved {
            // it gives way: a cloud of dust, then rubble settles into the hole
            if !fx(|fx| fx.pops.contains_key(&key)) {
                burst(mid, 40, 320.0, 9.0, MUTED);
                burst(mid, 14, 200.0, 6.0, BLOOD);
                ring(mid, 12.0, 62.0, 255, 0.0, 3.0, BLOOD, 0.36);
            }
            let t0 = born(key);
            rubble(scaled(r, bounce(t0)));
            continue;
        }
        unpop(key);
        let hot = hover == Some(i) && (board.live(i) || !eases.is_empty());
        let (doomed, shook, eased) = (fall.contains(&i), shaken.contains(&i), eases.contains(&i));
        let (tfx_t, tfx_k, dx, dy) = tile_fx[0][i];
        let tfx_el = (now - tfx_t) as f32;
        let mut r = Rect::new(r.x, r.y - if hot { 3.0 } else if eased { 1.5 } else { 0.0 }, r.w, r.h);
        // tactile tile micro-motion: directional kick outward when cracked, punch when mined, lift when eased
        if tfx_k == 1 && (0.0..0.24).contains(&tfx_el) {
            let p = 1.0 - tfx_el / 0.24;
            let kick = (tfx_el / 0.24 * std::f32::consts::PI).sin();
            r.x += dx * 5.5 * kick + (tfx_el * 85.0).sin() * 2.5 * p;
            r.y += dy * 5.5 * kick + (tfx_el * 65.0).cos() * 1.2 * p;
        } else if tfx_k == -1 && (0.0..0.3).contains(&tfx_el) {
            let p = (tfx_el / 0.3 * std::f32::consts::PI).sin();
            r.y -= p * 4.5;
        } else if tfx_k == 2 && (0.0..0.22).contains(&tfx_el) {
            let p = (tfx_el / 0.22 * std::f32::consts::PI).sin();
            r = scaled(r, 1.0 - 0.14 * p);
        }
        if doomed {
            r.x += (now as f32 * 48.0 + i as f32 * 2.3).sin() * 1.3;
        }
        let cracked = t.crack + u8::from(shook) - u8::from(eased);
        let mut fill = strain(if doomed { CAVE } else { cracked });
        if tfx_k == 1 && (0.0..0.2).contains(&tfx_el) {
            fill = mix(fill, INK, (1.0 - tfx_el / 0.2) * 0.45);
        } else if tfx_k == -1 && (0.0..0.3).contains(&tfx_el) {
            fill = mix(fill, rgb(0x5fe0c8), (1.0 - tfx_el / 0.3) * 0.35);
        }
        block(r, fill);
        draw_cracks(r, if doomed { CAVE - 1 } else { cracked }, i, doomed, now as f32);
        outline(r, if hot { INK } else if doomed { BLOOD } else if shook || eased { MUTED } else { FAINT });
        if t.crystal {
            // a live crystal bobs on its own beat and swells slightly under the mouse; a mined one is a faint print
            let bob = if t.used { 0.0 } else { (now as f32 * 2.6 + i as f32 * 1.7).sin() * 1.5 };
            let rad = if !t.used && hot { 14.5 } else { 13.0 };
            sign(board.element, r.center() - vec2(0.0, bob), rad, if t.used { FAINT } else { INK });
        }
    }
    // over the tile under the mouse: what a pick there would do
    if let Some(i) = hover {
        let at = tile_at(i);
        let gems = CLAIM_GEMS + me.mods.claim_bonus.max(0) as usize;
        let hurt = (CAVE_DMG - me.mods.mine_guard).max(0) * fall.len() as i32;
        let line = if board.live(i) && fall.is_empty() {
            Some((format!("mine: +{gems} gem{}", if gems == 1 { "" } else { "s" }), INK))
        } else if board.live(i) {
            Some((format!("mine: {} cave-in{}, -{hurt} HP", fall.len(), if fall.len() == 1 { "" } else { "s" }), BLOOD))
        } else if !eases.is_empty() {
            Some((format!("vent: eases {} tile{}", eases.len(), if eases.len() == 1 { "" } else { "s" }), INK))
        } else {
            None
        };
        if let Some((what, col)) = line {
            // above the tile, or under it on the top row, clear of the surge bar
            let (w, y) = (measure(&what, 15.0).width, if i < W { at.bottom() + 27.0 } else { at.y });
            block(Rect::new(at.center().x - w / 2.0 - 6.0, y - 27.0, w + 12.0, 22.0), OUT);
            centered(&what, at.center().x, y - 10.0, 15.0, col);
        }
    }
    // one tab per board under it, the current one smoothly underlined
    let mut tx = BOARD.x;
    let target_tab_x = BOARD.x + me.cur as f32 * 52.0;
    let cur_tab_x = fx(|fx| {
        if fx.tab_x <= 0.0 {
            fx.tab_x = target_tab_x;
        } else {
            fx.tab_x += (target_tab_x - fx.tab_x) * (dt() * 16.0).min(1.0);
        }
        fx.tab_x
    });
    for (k, bd) in me.boards.iter().enumerate() {
        let cur = k == me.cur;
        let r = Rect::new(tx, 672.0, 44.0, 30.0);
        let hot = mine && !cur && r.contains(mouse());
        if hot {
            block(r, CARD_HI);
        }
        sign(bd.element, r.center() + vec2(5.0, if cur { -1.0 } else { 0.0 }), if cur { 9.5 } else { 9.0 }, if cur || hot { INK } else { MUTED });
        // keys 1-3 pick a board too; the faint number in the corner says which
        let key = [KeyCode::Key1, KeyCode::Key2, KeyCode::Key3].get(k).copied();
        if key.is_some() {
            // level with the sign, not raised above it
            text(&(k + 1).to_string(), r.x + 4.0, r.center().y + 4.0, 12.0, FAINT);
        }
        if (hot && clicked()) || (mine && !cur && !modal() && key.is_some_and(is_key_pressed)) {
            swap = Some(k);
        }
        tx += 52.0;
    }
    draw_line(cur_tab_x + 4.0, 702.0, cur_tab_x + 40.0, 702.0, 2.0, INK);
    let ppop = (1.0 - (now - picks_pop[0]) as f32 / 0.25).clamp(0.0, 1.0);
    right(&format!("{} picks left", me.reveals_left), BOARD.x + 6.0 * STEP - 2.0, 694.0 - ppop * 2.5, 15.0, mix(MUTED, INK, ppop));

    // your stacks: drag a chip onto a rite's sockets; pops crisply when a flying gem arrives
    for (j, &(e, n)) in held.iter().enumerate() {
        let base_r = chip(j);
        let n = n - usize::from(drag.is_some_and(|k| me.stacks[k] == e));
        let hot = mine && drag.is_none() && n > 0 && base_r.contains(mouse());
        let cpop = (1.0 - (now - chip_pop[e as usize]) as f32 / 0.24).clamp(0.0, 1.0);
        let r = scaled(base_r, 1.0 + 0.12 * (cpop * std::f32::consts::PI).sin());
        block(r, mix(if hot { CARD_HI } else { BG }, mix(INK, hue(e), 0.5), cpop * 0.28));
        outline(r, if cpop > 0.1 { rgb(0xffffff) } else if n > 0 { INK } else { FAINT });
        // the newest gem of this element bounces in
        let t = free.iter().filter(|&&k| me.stacks[k] == e).map(|&k| stack_t[0][k.min(15)]).fold(0.0, f64::max).max(chip_pop[e as usize]);
        sign(e, vec2(r.x + 20.0, r.center().y), 10.0 * if n > 0 { bounce(t) } else { 1.0 }, if n > 0 { el_color(e) } else { FAINT });
        text(&n.to_string(), r.x + 38.0, r.center().y + 7.0, 20.0, if cpop > 0.2 { rgb(0xffffff) } else if n > 0 { INK } else { MUTED });
        if drag.is_none() && base_r.contains(mouse()) {
            tip(element_tip(e));
        }
        if hot && clicked() {
            drag = free.iter().rev().find(|&&k| me.stacks[k] == e).copied();
            fx(|fx| fx.drag_from = mouse());
        }
    }

    // both fighters' sigils, skills flying between them
    let shots: Vec<Shot> = fx(|fx| {
        fx.shots.retain(|s| now - s.t0 < 2.2);
        fx.shots.clone()
    });
    let hit_at: [f64; 2] = fx(|fx| fx.hit);
    for s in 0..2 {
        let toward = (STAND[1 - s] - STAND[s]).normalize();
        // a throw: the caster crouches back as the card holds (p from -1.2 to 0), then lunges as it lets go
        let swing = shots
            .iter()
            .filter(|c| c.from == s && c.hits)
            .map(|c| (now - c.t0) as f32 / SHOT_TIME - charge(c.skill.1))
            .filter(|p| (-1.2..1.0).contains(p))
            .next_back();
        let (push, crouch) = match swing {
            Some(p) if p < 0.0 => {
                let w = ((p + 1.2) / 1.2).powi(2);
                (-18.0 * w, w)
            }
            Some(p) => (50.0 * (p * std::f32::consts::PI).sin() - 18.0 * (1.0 - p), -(p * std::f32::consts::PI).sin()),
            None => (0.0, 0.0),
        };
        // non-hitting rite (shield, heal, buffs): caster lifts and channels instead of standing frozen
        let channel = shots
            .iter()
            .filter(|c| c.from == s && !c.hits)
            .map(|c| (now - c.t0) as f32 / SHOT_TIME - charge(c.skill.1))
            .filter(|p| (-0.8..1.2).contains(p))
            .next_back()
            .map_or(0.0, |p| (((p + 0.8) / 2.0) * std::f32::consts::PI).sin().max(0.0));
        let since_hit = (now - hit_at[s]) as f32;
        let mut at = STAND[s] + toward * push;
        at.y += (now as f32 * 3.0 + s as f32 * 2.0).sin() * 3.0 - channel * 11.0;
        // Wave 1 crisp opening slide-in (hero from left, enemy from right)
        let w1_el = (now - battle_t) as f32;
        if b.wave == 1 && intro.is_none() && (0.0..0.42).contains(&w1_el) {
            let q = ease_out3(w1_el / 0.42);
            let dir = if s == 0 { -1.0 } else { 1.0 };
            at.x += dir * 130.0 * (1.0 - q);
            at.y -= (q * std::f32::consts::PI).sin() * 12.0;
        }
        // evade / graze sidestep with afterimage
        let (dodge_t, dodge_str) = dodge[s];
        let since_dodge = (now - dodge_t) as f32;
        let dodge_dx = if (0.0..0.36).contains(&since_dodge) {
            let p = since_dodge / 0.36;
            let side_dir = if s == 0 { -1.0 } else { 1.0 };
            side_dir * 46.0 * dodge_str * (p * std::f32::consts::PI).sin()
        } else {
            0.0
        };
        at.x += dodge_dx;
        // knocked back, squashed and flashed red by a hit
        let k = if (0.0..0.45).contains(&since_hit) { 1.0 - since_hit / 0.45 } else { 0.0 };
        at.x += (since_hit * 90.0).sin() * 5.0 * k;
        at -= toward * 34.0 * k * k;
        // a new wave's enemy strides in from the right after its banner appears, and lands with a stomp
        if let (1, Some(t)) = (s, intro) {
            let el = (now - t - 0.45) as f32;
            let q = (el / 0.55).clamp(0.0, 1.0);
            if q == 0.0 {
                continue;
            }
            at.x += 420.0 * (1.0 - q).powi(3);
            at.y -= (q * std::f32::consts::PI).sin() * 26.0;
            if dt() > 0.0 && el >= 0.55 && el - dt() < 0.55 {
                quake(14.0);
                play(Sfx::Strike);
                burst(at + vec2(0.0, FIGHTER[1] * 0.45), 40, 260.0, 9.0, MUTED);
                ring(at + vec2(0.0, FIGHTER[1] * 0.4), 16.0, 96.0, 0, 0.0, 3.0, MUTED, 0.36);
            }
        }
        // the enemy just felled stays on its feet until the blow lands
        let f = if s == 1 { fallen.as_ref().unwrap_or(&b.f[1]) } else { &b.f[0] };
        let dead = bar[s] <= 0;
        let size = FIGHTER[s];
        if !dead {
            draw_fighter_auras(s, f, vis_shield[s], at, size, now, shield_hit[s], shield_gain[s], load_t, break_t);
        }
        // dodge afterimage silhouette
        if !dead && dodge_dx.abs() > 4.0 {
            let ghost_at = at - vec2(dodge_dx * 0.55, 0.0);
            sprite_rect(f.sprite, Rect::new(ghost_at.x - size / 2.0, ghost_at.y - size / 2.0, size, size), fade(WHITE, 0.25));
        }
        let tint = if dead { fade(MUTED, 0.35) } else if k > 0.3 { mix(WHITE, BAD, k) } else { WHITE };
        let (w, h) = (
            size * (1.0 + 0.34 * k + 0.12 * crouch - 0.06 * channel),
            size * (1.0 - 0.22 * k - 0.12 * crouch + 0.1 * channel),
        );
        sprite_rect(f.sprite, Rect::new(at.x - w / 2.0, at.y + size / 2.0 - h, w, h), tint);
    }
    // a felled enemy's sigil flares white and swells away
    if let Some((sprite, t0)) = fx(|fx| fx.vanish) {
        let q = ((now - t0) / 0.5) as f32;
        if q < 1.0 {
            let size = FIGHTER[1] * (1.0 + 0.9 * q);
            sprite_rect(sprite, Rect::new(STAND[1].x - size / 2.0, STAND[1].y - size / 2.0, size, size), fade(WHITE, (1.0 - q).powi(2)));
        }
    }
    draw_shots(&shots, now);
    anims();
    // a fighter at full surge burns with it
    for s in 0..2 {
        if b.active == s && !over && b.f[s].surge > 0 && b.f[s].surge >= b.surge_cap(s) && gen_range(0.0, 1.0) < 0.5 {
            let half = FIGHTER[s] / 2.0;
            let from = STAND[s] + vec2(gen_range(-half * 0.7, half * 0.7), gen_range(0.0, half));
            let (vel, life, size, color) = (vec2(gen_range(-12.0, 12.0), gen_range(-90.0, -40.0)), gen_range(0.4, 0.8), gen_range(4.0, 8.0), mix(GOLD_C, MUTED, gen_range(0.0, 0.7)));
            fx(|fx| fx.parts.push(Part { pos: from, vel, born: now, life, size, color, grav: -80.0, home: None }));
        }
    }

    // the basics, then the rites found, on the right. Click one that is lit to cast it; gems dragged into
    // a rite's sockets choose which it spends.
    let rx = 880.0;
    let ready = b.ready(0);
    let (basics, list): (Vec<&'static Combo>, Vec<&'static Combo>) = me.rite_list().into_iter().partition(|c| is_basic(c));
    for (k, &c) in basics.iter().enumerate() {
        let r = Rect::new(rx - 10.0 + k as f32 * 124.0, 278.0, 118.0, 42.0);
        let lit = ready.iter().any(|x| std::ptr::eq(*x, c));
        let hot = mine && lit && drag.is_none() && r.contains(mouse());
        let h = hover_lerp(r, hot);
        let kick = cast_kick.filter(|(n, _)| *n == c.name).map_or(0.0, |(_, t0)| (1.0 - (now - t0) as f32 / 0.2).clamp(0.0, 1.0));
        let dr = Rect::new(r.x - 6.0 * (kick * std::f32::consts::PI).sin(), r.y - 2.0 * h, r.w, r.h);
        block(dr, mix(mix(BG, CARD_HI, h), INK, kick * 0.35));
        outline(dr, if kick > 0.1 { rgb(0xffffff) } else if lit { INK } else { FAINT });
        if hot {
            corner_ticks(dr, h, INK);
        }
        let e = c.needs[0];
        sign(e, vec2(dr.x + 16.0, dr.y + 14.0), 7.0, if lit { el_color(e) } else { FAINT });
        text(c.name, dr.x + 30.0, dr.y + 20.0, 17.0, if lit { INK } else { MUTED });
        let hit_info = b.hits(0, c).then(|| b.hit_steps(0, c));
        let line = match &hit_info {
            Some((d, _)) => format!("{d} dmg"),
            None => me.describe(c),
        };
        fit_text(&line, dr.x + 10.0, dr.y + 36.0, dr.w - 20.0, 13.0, if lit { MUTED } else { FAINT });
        if r.contains(mouse()) && drag.is_none() {
            if let Some((d, steps)) = hit_info {
                steps_tip(steps, d);
            } else {
                tip_text(c.name, format!("Takes one {e:?} gem. {}. Flat basic: ignores rite buffs.", me.describe(c)));
            }
        }
        if r.contains(mouse()) && is_mouse_button_pressed(MouseButton::Left) {
            trace(|| format!("  on {}: lit={lit} hot={hot} cast_taken={} clicked={}", c.name, cast.is_some(), clicked()));
        }
        if hot && cast.is_none() && clicked() {
            if let Some(paid) = me.payment(c, &me.stacks) {
                cast = Some((c, paid));
            }
        } else if mine && !lit && drag.is_none() && r.contains(mouse()) && clicked() {
            // say why instead of ignoring the click: each basic takes its own element
            let why = if b.wasted(0, c) { "FULL HP".into() } else { format!("NO {} GEM", format!("{e:?}").to_uppercase()) };
            fx(|fx| fx.floats.push((mouse(), why, MUTED, now, 22.0)));
            play(Sfx::Miss);
        }
    }
    // tall enough for a rite's text to wrap to two lines under its name; four rows still clear End turn
    let rh = (300.0 / list.len().max(1) as f32).min(75.0);
    let mut y = 326.0;
    if list.is_empty() {
        text("No rites yet", rx, y + 30.0, 20.0, MUTED);
        wrapped("Elites, treasure and shops teach them. Until then, click a basic above: each gem is an Attack, a Shield or a Heal.", rx, y + 56.0, 350.0, 14.0, MUTED);
    }
    // where a dragged gem may land: (row, rite, its cost)
    let mut targets: Vec<(Rect, &'static Combo, Vec<Element>)> = vec![];
    for &c in &list {
        let (lvl, cost) = (me.level(c), me.cost(c));
        let lit = ready.iter().any(|r| r.name == c.name);
        let slotted = sockets.get(c.name).cloned().unwrap_or_default();
        let r = Rect::new(rx - 10.0, y, 370.0, rh);
        let over_row = mine && r.contains(mouse());
        let kick = cast_kick.filter(|(n, _)| *n == c.name).map_or(0.0, |(_, t0)| (1.0 - (now - t0) as f32 / 0.22).clamp(0.0, 1.0));
        let draw_r = Rect::new(r.x - 8.0 * (kick * std::f32::consts::PI).sin(), r.y, r.w, r.h);
        if kick > 0.05 {
            block(draw_r, mix(CARD_HI, INK, kick * 0.35));
        } else if dragged.is_some_and(|e| count(&cost, e) > count(&slotted, e) || paid_toll(&cost, &slotted).len() < me.mods.toll.max(0) as usize) {
            block(draw_r, if over_row { CARD_HI } else { CARD });
        } else if lit && over_row && drag.is_none() {
            block(draw_r, CARD_HI);
        }
        draw_line(rx, y + rh, rx + 350.0, y + rh, 1.0, FAINT);
        let bright = lit || !slotted.is_empty();
        if lvl > 1 {
            lively(c.title(lvl), rx, y + 30.0, 24.0, fade(level_color(lvl), if bright { 1.0 } else { 0.55 }), Motion::Wave);
        } else {
            text(c.title(lvl), rx, y + 30.0, 24.0, if bright { INK } else { MUTED });
        }
        let mut after = rx + measure(c.title(lvl), 24.0).width + 10.0;
        let brands = me.brands(c);
        if !brands.is_empty() {
            // the names, or just how many when they would run into the sockets
            let names = brands.iter().map(|b| b.name).collect::<Vec<_>>().join(" ");
            let names = if after + measure(&names, 14.0).width > rx + 210.0 { format!("{} brands", brands.len()) } else { names };
            text(&names, after, y + 30.0, 14.0, ACCENT);
            after += measure(&names, 14.0).width + 10.0;
        }
        let reacts: Vec<&str> = b.reactions(0, c).into_iter().map(react_word).collect();
        if !reacts.is_empty() {
            rich(&reacts.join(" "), after, y + 30.0, 14.0, INK);
        }
        // one socket per gem the rite takes, right-aligned, then the toll sockets any gem fills
        let mut unslot = None;
        let toll = me.mods.toll.max(0) as usize;
        let tolled = paid_toll(&cost, &slotted);
        let all = cost.len() + toll;
        for j in 0..all {
            let sr = Rect::new(rx + 350.0 - (all - j) as f32 * 32.0 + 6.0, y + 10.0, 26.0, 26.0);
            let gem_in = if j < cost.len() {
                let e = cost[j];
                (count(&cost[..j], e) < count(&slotted, e)).then_some(e)
            } else {
                tolled.get(j - cost.len()).copied()
            };
            outline(sr, if gem_in.is_some() { INK } else { FAINT });
            match (gem_in, j < cost.len()) {
                (Some(e), _) => sign(e, sr.center(), 9.0, el_color(e)),
                (None, true) => sign(cost[j], sr.center(), 7.0, MUTED),
                (None, false) => toll_sign(sr.center(), 7.0, MUTED),
            }
            if let Some(e) = gem_in.filter(|_| mine && drag.is_none() && sr.contains(mouse())) {
                block(sr, fade(INK, 0.15));
                if clicked() {
                    unslot = Some((e, sr.center()));
                }
            }
        }
        // a brand can make the line long: shrink it to fit beside the damage number, not under it
        let hit = (lit && b.hits(0, c)).then(|| b.hit_steps(0, c));
        let num = hit.as_ref().map(|(d, _)| format!("{d} dmg"));
        let room = 350.0 - num.as_ref().map_or(0.0, |n| measure(n, 15.0).width + 12.0);
        let line = me.describe(c);
        let fit = 14.0 * (room / measure(&line, 14.0).width).min(1.0);
        if fit >= 12.5 {
            rich(&line, rx, y + rh - 12.0, fit, MUTED);
        } else {
            for (k, l) in wrap(&line, room, 13.0).iter().take(2).enumerate() {
                rich(l, rx, y + rh - 26.0 + k as f32 * 15.0, 13.0, MUTED);
            }
        }
        if let (Some((d, steps)), Some(num)) = (hit, num) {
            // hover the number to see how it adds up
            let (nx, ny) = (rx + 350.0, y + rh - 12.0);
            let w = measure(&num, 15.0).width;
            right(&num, nx, ny, 15.0, INK);
            draw_line(nx - w, ny + 4.0, nx, ny + 4.0, 1.0, FAINT);
            if Rect::new(nx - w, ny - 16.0, w, 22.0).contains(mouse()) {
                steps_tip(steps, d);
            }
        }
        if let Some((e, at)) = unslot {
            // take a gem back out of its socket
            if let Some(v) = sockets.get_mut(c.name) {
                if let Some(k) = v.iter().position(|&x| x == e) {
                    v.remove(k);
                }
            }
            burst(at, 8, 110.0, 5.0, el_color(e));
            play(Sfx::Click);
        } else if lit && over_row && drag.is_none() && cast.is_none() && clicked() {
            // a click on a lit rite casts it, paying from your gems
            if let Some(paid) = me.payment(c, &me.stacks) {
                cast = Some((c, paid));
            }
        } else if !lit && over_row && drag.is_none() && clicked() {
            fx(|fx| fx.floats.push((mouse(), "NOT ENOUGH GEMS".into(), MUTED, now, 22.0)));
            play(Sfx::Miss);
        }
        targets.push((r, c, cost));
        y += rh;
    }

    // a dragged gem lands where the button comes up: in a rite's socket.
    // Dropped anywhere else it just goes back to your stacks; nothing fires on a click.
    if let Some(k) = drag {
        let e = b.f[0].stacks[k];
        if !is_mouse_button_down(MouseButton::Left) {
            if let Some((r, c, cost)) = targets.iter().find(|(r, _, _)| r.contains(mouse())) {
                let (r, c) = (*r, *c);
                let v = sockets.entry(c.name).or_default();
                let toll = me.mods.toll.max(0) as usize;
                let all = cost.len() + toll;
                if count(cost, e) > count(v, e) || paid_toll(cost, v).len() < toll {
                    // the socket it lands in: the next empty one of its element, else a toll socket
                    let j = if count(cost, e) > count(v, e) {
                        cost.iter().enumerate().filter(|x| *x.1 == e).nth(count(v, e)).map_or(0, |x| x.0)
                    } else {
                        cost.len() + paid_toll(cost, v).len()
                    };
                    v.push(e);
                    let socket = vec2(rx + 350.0 - (all - j) as f32 * 32.0 + 19.0, r.y + 23.0);
                    burst(socket, 14, 160.0, 6.0, el_color(e));
                    play(Sfx::Claim);
                } else {
                    // refused: say why instead of silently handing the gem back
                    let why = if cost.contains(&e) || toll > 0 { "FULL" } else { "WRONG GEM" };
                    fx(|fx| fx.floats.push((mouse(), why.into(), MUTED, now, 22.0)));
                    play(Sfx::Miss);
                }
                if v.len() >= all {
                    cast = Some((c, sockets.remove(c.name).unwrap_or_default()));
                }
            }
            drag = None;
        }
    }
    if let Some((c, _)) = &cast {
        sockets.remove(c.name);
    }

    // with reveals still unused, Space asks twice so a stray key does not throw them away
    thread_local!(static ASKED: std::cell::Cell<f64> = const { std::cell::Cell::new(f64::MIN) });
    let (left, asked) = (b.f[0].reveals_left, ASKED.with(|a| a.get()));
    let space = mine && !modal() && is_key_pressed(KeyCode::Space);
    let space_ends = space && (left <= 0 || now - asked < 2.0);
    if space {
        ASKED.with(|a| a.set(if space_ends { f64::MIN } else { now }));
    } else if mine && now - asked < 2.0 {
        right(&format!("{left} picks left. Space again ends the turn."), 1230.0, 708.0, 14.0, MUTED);
    }
    let pass = button(Rect::new(880.0, 640.0, 350.0, 44.0), "End turn", mine, true) || space_ends;
    if pass {
        b.end_turn();
    } else if let Some((c, paid)) = cast {
        fx(|fx| fx.cast_kick = Some((c.name, now)));
        // draw element motes from your gem chips into the hero as the rite fires
        for &e in &paid {
            stream(chip_of(e), STAND[0], 6, mix(INK, hue(e), 0.5));
        }
        let before = (b.f[1].hp, b.f[1].shield);
        let ok = b.cast_with(c, &paid);
        trace(|| format!("  CAST {} paid={paid:?} ok={ok} foe {} hp {}->{} shield {}->{}", c.name, b.f[1].name, before.0, b.f[1].hp, before.1, b.f[1].shield));
    } else if let Some(k) = swap {
        b.swap(k);
    } else if let Some(i) = dig_at {
        b.dig(i);
    } else if let Some(i) = vent_at {
        b.vent(i);
    }

    if !over && b.active == 1 {
        // the enemy moves one visible step at a time, each once the last one has played out
        if now >= fx(|fx| fx.busy) {
            *ai_timer += dt();
        }
        if *ai_timer > 0.32 {
            *ai_timer = 0.0;
            b.ai_step();
        }
    } else {
        *ai_timer = 0.0;
    }

    particles();
    charm_bar(charms);
    if let Some(k) = drag {
        if let Some(&e) = b.f[0].stacks.get(k) {
            gem(e, mouse(), 15.0);
        }
    }
    fx(|fx| {
        fx.sockets = sockets;
        fx.drag = drag;
    });
    let dur = |lvl: usize| if lvl == 0 { TURN_BANNER } else { card_dur(lvl) };
    let (banner, flashed) = fx(|fx| {
        fx.banner.retain(|x| now - x.3 < dur(x.4) as f64);
        (fx.banner.iter().filter(|x| x.3 <= now).next_back().copied(), fx.flash)
    });
    if let Some((name, c, from, t0, lvl)) = banner {
        // a skill's name (or whose turn it is) sweeps in from the caster's side along the seam above
        // your board; basics use a sleek quick tag (lvl == 99), upgraded rites get a taller band
        let basic_tag = lvl == 99;
        let up = if basic_tag { 0.0 } else { lvl.saturating_sub(1) as f32 };
        let (dur, el) = (dur(lvl), (now - t0) as f32);
        let dir = if from == 0 { -1.0 } else { 1.0 };
        // eases in with a little overshoot, holds, then slips on out the far side as it fades
        let e = (el / if basic_tag { 0.14 } else { 0.24 }).min(1.0);
        let settle = 1.0 + 2.70158 * (e - 1.0).powi(3) + 1.70158 * (e - 1.0).powi(2);
        let a = ((dur - el) / if basic_tag { 0.14 } else { 0.22 }).clamp(0.0, 1.0);
        let slide = (1.0 - settle) * if basic_tag { 280.0 } else { 620.0 } * dir - (1.0 - a) * 140.0 * dir;
        let (size, band) = if lvl == 0 { (46.0, 64.0) } else if basic_tag { (36.0, 46.0) } else { (62.0 + 8.0 * up, 86.0 + 10.0 * up) };
        let cx = BOARD.x + W as f32 * STEP / 2.0 - 1.0;
        // a turn banner crosses the board itself (no picks until it clears), so NEXT and SURGE stay readable
        let mid = if lvl == 0 { BOARD.y + W as f32 * STEP / 2.0 } else { BOARD.y - 20.0 };
        let top = mid - band / 2.0;
        let wide = if basic_tag { (measure(name, size).width + 72.0).max(190.0) } else { (measure(name, size).width + 120.0).max(W as f32 * STEP + 80.0) };
        let bx = Rect::new(cx + slide - wide / 2.0, top, wide, band);
        overlay();
        // the band flares in the rite's colour as it arrives, then darkens to let the name read
        draw_rectangle(bx.x, bx.y, bx.w, bx.h, fade(mix(OUT, c, 0.45 * (1.0 - e)), 0.92 * a));
        draw_line(bx.x, bx.y, bx.right(), bx.y, 2.0, fade(c, 0.8 * a));
        draw_line(bx.x, bx.bottom(), bx.right(), bx.bottom(), 2.0, fade(c, 0.8 * a));
        stamped(name, cx + slide, mid + size / 3.0, size * (1.0 + 0.35 * (1.0 - e)), fade(c, a));
        if !basic_tag && lvl >= 2 {
            stamped(if lvl == 2 { "LV II" } else { "LV III" }, cx - slide, top - 20.0, 24.0, fade(GOLD_C, a));
        }
    }
    if let Some((c, t0)) = flashed {
        let p = (now - t0) as f32 / 0.18;
        if p < 1.0 {
            overlay();
    draw_rectangle(-2000.0, -2000.0, 5280.0, 5000.0, fade(c, 0.32 * (1.0 - p)));
        }
    }
    // a hit of HUGE or more: the game stops dead on two impact frames, black then inverted
    if let Some(s) = fx(|fx| fx.huge).filter(|h| now >= h.0).map(|h| h.1) {
        fx(|fx| (fx.huge, fx.impact) = (None, Some((get_time(), s))));
        hitstop(IMPACT * 2.0);
        quake(36.0);
        punch();
        play(Sfx::Boom);
        // lingering massive shock-ring waiting right after hitstop ends
        ring(STAND[s], FIGHTER[s] * 0.9, FIGHTER[s] * 2.1, 0, 0.0, 8.0, INK, 0.36);
    }
    if let Some((t0, s)) = fx(|fx| fx.impact) {
        let el = get_time() - t0;
        if el >= IMPACT * 2.0 {
            fx(|fx| fx.impact = None);
        } else {
            let first = el < IMPACT;
            let (bg, ink) = if first { (BLACK, WHITE) } else { (WHITE, BLACK) };
            overlay();
            draw_rectangle(-2000.0, -2000.0, VIEW.x + 4000.0, VIEW.y + 4000.0, bg);
            // one crisp shockwave ring (plus thin echo on the inverted frame) and a single stark cleave line
            let r = FIGHTER[s] * if first { 0.8 } else { 1.18 };
            draw_poly_lines(STAND[s].x, STAND[s].y, 64, r, 0.0, 10.0, ink);
            if !first {
                draw_poly_lines(STAND[s].x, STAND[s].y, 64, FIGHTER[s] * 1.55, 0.0, 3.0, ink);
            }
            let cleave = vec2(260.0, -140.0);
            draw_line(STAND[s].x - cleave.x, STAND[s].y - cleave.y, STAND[s].x + cleave.x, STAND[s].y + cleave.y, if first { 6.0 } else { 3.0 }, ink);
            let fallen = fx(|fx| fx.fallen.as_ref().map(|f| f.sprite));
            let jolt_s = if first { vec2(-6.0, 4.0) } else { vec2(8.0, -5.0) };
            for k in 0..2 {
                let sprite = if k == 1 { fallen.unwrap_or(b.f[1].sprite) } else { b.f[0].sprite };
                let size = FIGHTER[k] * if k == s { if first { 1.18 } else { 1.32 } } else { 1.0 };
                let pos = STAND[k] + if k == s { jolt_s } else { Vec2::ZERO };
                sprite_rect(sprite, Rect::new(pos.x - size / 2.0, pos.y - size / 2.0, size, size), ink);
            }
        }
    }
    fx(|fx| {
        fx.floats.retain(|f| now - f.3 < 1.5);
        for (at, label, c, t0, size) in &fx.floats {
            let el = (now - t0) as f32;
            if el < 0.0 {
                continue;
            }
            // pops with elastic kick, holds a moment, then drifts up with a subtle decelerating curve
            let p = el / 1.5;
            let kick = if el < 0.14 { 1.0 + 0.85 * (1.0 - el / 0.14) } else { 1.0 };
            // one font all the way: growing and shrinking must not cross into the other face
            let f = face_in(if *size >= DISPLAY_FROM { 0 } else { 1 }, (size + 12.0 * (1.0 - p)) * kick);
            let d = FONT.with(|ft| measure_text(label, ft.borrow()[f.0].as_ref(), f.1, f.2));
            // a big number by a fighter near the edge would spill off the canvas: keep it inside
            let x = (at.x - d.width / 2.0).clamp(8.0, VIEW.x - 8.0 - d.width);
            let y = (at.y - ease_out3(p) * 46.0).max(d.offset_y + 30.0);
            let alpha = (1.6 - p * 1.6).clamp(0.0, 1.0);
            // crisp 2 px black outline so combat numbers never wash out against white fighter sprites
            let out = fade(OUT, alpha * 0.9);
            for (ox, oy) in [(-2.0, 0.0), (2.0, 0.0), (0.0, -2.0), (0.0, 2.0)] {
                text_in(f, label, x + ox, y + oy, out);
            }
            text_in(f, label, x, y, fade(*c, alpha));
        }
    });
    // below a quarter HP the screen edge throbs red with a heartbeat
    let me = &b.f[0];
    if me.hp > 0 && me.hp * 4 <= me.max_hp {
        let t = (now * 1.3).fract() as f32;
        let beat = (1.0 - t * 6.0).max(0.0) + (1.0 - (t - 0.2).abs() * 6.0).max(0.0) * 0.7;
        // on the window's edge, which in a wider or taller window lies beyond the 1280x720 canvas
        let half = vec2(screen_width(), screen_height()) / scale() / 2.0;
        let corner = CAM.with(|c| c.get()) - half;
        for i in 0..8 {
            let d = i as f32 * 8.0;
            let size = half * 2.0 - Vec2::splat(2.0 * d);
            draw_rectangle_lines(corner.x + d, corner.y + d, size.x, size.y, 8.0, fade(BAD, (0.3 - i as f32 * 0.035) * (0.4 + 0.6 * beat)));
        }
    }

    // the result waits until the final blow has played out in slow motion, then pops in
    let shown = get_time() - fx(|fx| fx.ended_at) - 1.0;
    if over && shown > 0.0 {
        overlay();
    draw_rectangle(-2000.0, -2000.0, 5280.0, 5000.0, fade(OUT, (shown as f32 * 2.0).min(if ending.is_some() { 0.7 } else { 0.45 })));
        if let Some(opened) = ending {
            return summary(run, Some(b), b.won(), opened);
        }
        let k = bounce(now - shown);
        // an ending's boss fell: the card says which ending was found
        let found = run.ending_found(b);
        let extra = if found.is_some() { 76.0 } else { 0.0 };
        let r = scaled(Rect::new(440.0, 270.0 - extra / 2.0, 400.0, 180.0 + extra), k);
        plate(r, BG);
        stamped(if b.won() { "Victory" } else { "Defeated" }, 640.0, r.y + 74.0 * k, 50.0 * k, if b.won() { GOOD } else { BAD });
        if let Some(e) = found {
            lively_centered("SECRET ENDING FOUND", 640.0, r.y + 112.0 * k, 16.0 * k, GOLD, Motion::Wave);
            stamped(ENDINGS[e].name, 640.0, r.y + 148.0 * k, 30.0 * k, INK);
        }
        let cont = button(Rect::new(540.0, 380.0 + extra / 2.0, 200.0, 40.0), "Continue", true, true);
        return shown > 0.25 && (cont || (!modal() && (is_key_pressed(KeyCode::Enter) || is_key_pressed(KeyCode::Space))));
    }
    false
}

fn node_look(k: NodeKind) -> usize {
    match k {
        NodeKind::Battle => HD + 20,
        NodeKind::Elite => HD + 21,
        NodeKind::Encounter => HD + 22,
        NodeKind::Shop => HD + 23,
        NodeKind::Treasure => HD + 25,
        NodeKind::Boss => HD + 26,
        // a standing stone carved with marks
        NodeKind::Shrine => HD + 30,
    }
}

/// A fixed number in 0..1 for (a, b): splitmix64, so nearby inputs give unrelated outputs.
fn hash01(a: u64, b: u64) -> f32 {
    let mut z = a.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ b.wrapping_add(0x632b_e59b_d9b4_e019);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    ((z ^ (z >> 31)) >> 40) as f32 / (1u64 << 24) as f32
}

/// Where room (c, r) sits. Each floor scatters its rooms: columns spread and shift unevenly and every
/// room is nudged, but never so far that a room passes its neighbour or a tunnel crosses another.
fn node_at(run: &Run, c: usize, r: usize) -> Vec2 {
    let n = run.map[c].len() as f32;
    let floor = run.seed ^ ((run.floor as u64 + 1) * 0x5851);
    let h = |k: u64| hash01(floor, (c * 16 + r) as u64 * 8 + k) - 0.5;
    let col = |k: u64| hash01(floor, c as u64 * 8 + 1000 + k) - 0.5;
    // a shorter map (an ending's floor) sits in the middle of the space a full one takes
    let inset = (COLS + 1 - run.map.len()) as f32 * 70.0;
    let last = c + 1 == run.map.len();
    let spread = 150.0 + col(0) * 60.0;
    let x = 100.0 + inset + c as f32 * 140.0 + if last { 0.0 } else { h(0) * 60.0 };
    let y = 390.0 + col(1) * 80.0 + (r as f32 - (n - 1.0) / 2.0) * spread + if last { 0.0 } else { h(1) * 36.0 };
    vec2(x, y)
}

/// The floor map's land: The Below seen closer, its tiles larger.
const FLOOR_LAND: Grid = (24.0, 100.0, 28.0, 32, 21);

/// The floor's land: blobs round its rooms and along its tunnels, and a few stray ones, so it lies like a
/// patch of The Below. It is explored round the rooms you have been in and those you may enter next.
fn floor_land(run: &Run) -> Vec<Vec<u8>> {
    let mut blobs = vec![];
    let mut walked = vec![];
    for (c, col) in run.map.iter().enumerate() {
        for (r, node) in col.iter().enumerate() {
            let p = node_at(run, c, r);
            blobs.push((p.x, p.y, 46.0));
            for &j in &node.next {
                let q = node_at(run, c + 1, j);
                for k in 1..3 {
                    let m = p.lerp(q, k as f32 / 3.0);
                    blobs.push((m.x, m.y, 24.0));
                }
                if run.trail.windows(2).any(|w| w[0] == (c, r) && w[1] == (c + 1, j)) {
                    walked.push((p, q));
                }
            }
        }
    }
    let floor = run.seed ^ ((run.floor as u64 + 1) * 0x5851);
    let rooms: Vec<Vec2> = blobs.iter().map(|&(x, y, _)| vec2(x, y)).collect();
    for k in 0..5 {
        let p = vec2(80.0 + hash01(floor, 500 + k) * 760.0, 140.0 + hash01(floor, 600 + k) * 520.0);
        blobs.push((p.x, p.y, 20.0 + hash01(floor, 700 + k) * 16.0));
    }
    // chasms in the gaps, never under a room or a tunnel
    let chasms: Vec<(f32, f32, f32)> = (0..24)
        .map(|k| vec2(80.0 + hash01(floor, 800 + k) * 760.0, 140.0 + hash01(floor, 900 + k) * 520.0))
        .filter(|p| rooms.iter().all(|q| q.distance(*p) > 75.0))
        .take(6)
        .map(|p| (p.x, p.y, 44.0))
        .collect();
    let seen: Vec<Vec2> = run.trail.iter().chain(run.choices().iter()).map(|&(c, r)| node_at(run, c, r)).collect();
    let near_line = |p: Vec2, a: Vec2, b: Vec2| {
        let t = ((p - a).dot(b - a) / (b - a).length_squared()).clamp(0.0, 1.0);
        p.distance(a.lerp(b, t)) < 42.0
    };
    terrain(FLOOR_LAND, &blobs, &chasms, |p| seen.iter().any(|q| q.distance(p) < 84.0) || walked.iter().any(|&(a, b)| near_line(p, a, b)))
}

/// A label on a dark strip, so it reads over the land, as on The Below.
fn strip_label(s: &str, x: f32, y: f32, size: f32, c: Color) {
    let w = measure(s, size).width;
    block(Rect::new(x - w / 2.0 - 6.0, y - size - 1.0, w + 12.0, size + 7.0), BG);
    centered(s, x, y, size, c);
}

/// Returns the node the player clicked, if it is one they may enter.
/// The floor is drawn as a closer look at The Below: land round its rooms, turned over where you have
/// been and where you may go next, the rooms as diamonds on a dotted route.
pub fn map_screen(run: &Run) -> Option<(usize, usize)> {
    status(run);
    text(&run.note, 48.0, 92.0, 18.0, GOLD_C);
    let open = run.choices();
    let mut chosen = None;
    // each room's diamond and the label under it (a boss's name runs wider)
    let rooms: Vec<Rect> = (run.map.iter().enumerate())
        .flat_map(|(c, col)| col.iter().enumerate().map(move |(r, node)| (node_at(run, c, r), node.kind == NodeKind::Boss)))
        .map(|(p, boss)| {
            let (half, rad) = if boss { (90.0, 44.0) } else { (50.0, 32.0) };
            Rect::new(p.x - half, p.y - rad, half * 2.0, rad * 2.0 + 34.0)
        })
        .collect();
    // the land changes only with the floor's map and where you have been, so build it once for those
    type Key = (u64, usize, Vec<(usize, usize)>, Option<(usize, usize)>, Vec<Vec<Vec<usize>>>);
    thread_local!(static FLOOR_AT: std::cell::RefCell<Option<(Key, Vec<Vec<u8>>)>> = const { std::cell::RefCell::new(None) });
    let key: Key = (run.seed, run.floor, run.trail.clone(), run.pos, run.map.iter().map(|c| c.iter().map(|n| n.next.clone()).collect()).collect());
    FLOOR_AT.with(|l| {
        let mut l = l.borrow_mut();
        if l.as_ref().is_none_or(|(k, _)| *k != key) {
            *l = Some((key, floor_land(run)));
        }
        draw_land(&l.as_ref().unwrap().1, FLOOR_LAND, run.floor * 7 + run.seed as usize % 97, &rooms);
    });
    let walked = |a: (usize, usize), b: (usize, usize)| run.trail.windows(2).any(|w| w[0] == a && w[1] == b);
    // the route: the way you came bold, the ways on from here gently marching, the rest faint
    for (c, col) in run.map.iter().enumerate() {
        for (r, node) in col.iter().enumerate() {
            let p = node_at(run, c, r);
            for &j in &node.next {
                let q = node_at(run, c + 1, j);
                if walked((c, r), (c + 1, j)) {
                    dotted(p, q, 7.0, 2.0, INK);
                } else if run.pos == Some((c, r)) {
                    dotted_flow(p, q, 9.0, 1.5, INK, 1.1);
                } else {
                    dotted(p, q, 9.0, 1.0, fade(MUTED, 0.6));
                }
            }
        }
    }
    let mt = now() as f32;
    for (c, col) in run.map.iter().enumerate() {
        for (r, node) in col.iter().enumerate() {
            let p = node_at(run, c, r);
            let can = open.contains(&(c, r));
            let here = run.pos == Some((c, r));
            let done = !here && run.trail.contains(&(c, r));
            let boss = node.kind == NodeKind::Boss;
            let size = if boss { 44.0 } else { 32.0 };
            let hot = can && !modal() && (mouse() - p).length() < size;
            let hv = hover_lerp(Rect::new(p.x - size, p.y - size, size * 2.0, size * 2.0), hot);
            let rad = size + 4.0 * hv;
            if can {
                let pulse = (mt * 1.15 + r as f32 * 0.35).fract();
                draw_poly_lines(p.x, p.y, 4, rad + 3.0 + pulse * 12.0, 0.0, 1.4, fade(if boss { BLOOD } else { INK }, (1.0 - pulse) * 0.65));
            } else if boss {
                let pulse = (mt * 0.65).fract();
                draw_poly_lines(p.x, p.y, 4, rad + 4.0 + pulse * 14.0, 0.0, 1.2, fade(BLOOD, (1.0 - pulse) * 0.4));
            }
            draw_poly(p.x, p.y, 4, rad, 0.0, if here { INK } else if hot { CARD_HI } else { BG });
            let edge = if boss { BLOOD } else if can || here { INK } else if done { MUTED } else { fade(MUTED, 0.6) };
            draw_poly_lines(p.x, p.y, 4, rad, 0.0, if boss || can || here { 2.0 } else { 1.5 }, edge);
            let icon = if boss { run.boss_def().sprite } else { node_look(node.kind) };
            let k = if boss { 24.0 } else { 17.0 };
            let bob = if can || here { (mt * 2.8 + c as f32 + r as f32).sin() * 1.6 } else { 0.0 };
            let tint = if here { BLACK } else if can || boss { WHITE } else if done { fade(WHITE, 0.3) } else { fade(WHITE, 0.5) };
            sprite(icon, p.x - k, p.y - k + bob, k * 2.0, tint);
            let y = p.y + rad + 20.0;
            if boss {
                let w = measure(run.boss_def().name, 20.0).width;
                block(Rect::new(p.x - w / 2.0 - 6.0, y - 21.0, w + 12.0, 28.0), BG);
                lively_centered(run.boss_def().name, p.x, y, 20.0, BLOOD, Motion::Tremble);
            } else if here {
                strip_label("You are here", p.x, y, 14.0, INK);
            } else if done {
                strip_label("cleared", p.x, y, 14.0, MUTED);
            } else {
                strip_label(node.kind.name(), p.x, y, 14.0, if can { INK } else { MUTED });
            }
            if hot && clicked() {
                play(Sfx::Click);
                chosen = Some((c, r));
            }
        }
    }

    let side = Rect::new(930.0, 108.0, 326.0, 588.0);
    plate(side, CARD);
    let x = side.x + 22.0;
    // this floor's twist, your skills with their levels, then your charms
    let twist = &TWISTS[run.twist];
    let tone = match twist.tone {
        1 => GOOD,
        -1 => BAD,
        _ => GOLD_C,
    };
    text(["FLOOR CURSE", "FLOOR TWIST", "FLOOR BOON"][(twist.tone + 1) as usize], x, 140.0, 15.0, ACCENT);
    match twist.tone {
        -1 => lively(twist.name, x, 164.0, 20.0, BLOOD, Motion::Tremble),
        1 => lively(twist.name, x, 164.0, 20.0, GOLD, Motion::Wave),
        _ => text(twist.name, x, 164.0, 20.0, tone),
    }
    let mut y = wrapped(twist.desc, x, 186.0, 282.0, 16.0, INK) + 6.0;
    text("RITES", x, y + 8.0, 15.0, ACCENT);
    y += 16.0;
    if run.rites().is_empty() {
        text("None yet", x, y + 15.0, 15.0, MUTED);
        y += 22.0;
    }
    for c in run.rites().into_iter().map(|i| &COMBOS[i]) {
        let lvl = run.level(combo_id(c));
        // the gems it takes at its level, not the base two or three
        let cost = c.cost(lvl);
        for (j, &e) in cost.iter().enumerate() {
            gem(e, vec2(x + 9.0 + j as f32 * 20.0, y + 9.0), 8.0);
        }
        if lvl > 1 {
            lively(c.title(lvl), x + 9.0 + cost.len() as f32 * 20.0, y + 15.0, 16.0, level_color(lvl), Motion::Wave);
        } else {
            text(c.title(lvl), x + 9.0 + cost.len() as f32 * 20.0, y + 15.0, 16.0, INK);
        }
        if lvl > 1 {
            right(&format!("Lv {lvl}"), side.right() - 22.0, y + 15.0, 15.0, if lvl >= MAX_LEVEL { MASTERED } else { GOLD_C });
        }
        y += 22.0;
    }
    let mut held = None;
    if let Some(bl) = run.blessing {
        let r = Rect::new(x - 8.0, y + 2.0, 298.0, 22.0);
        if r.contains(mouse()) {
            // the highlight first, so the label stays on top of it
            block(r, CARD_HI);
            held = Some(&BLESSINGS[bl]);
        }
        text("BLESSING", x, y + 18.0, 15.0, ACCENT);
        text(BLESSINGS[bl].name, x + 90.0, y + 18.0, 16.0, GOLD_C);
        y += 24.0;
    }
    // keys carried: each one's boon and curse on hover
    let mut key_tip = None;
    for &k in &run.keys {
        let e = &ENDINGS[k];
        let r = Rect::new(x - 8.0, y + 2.0, 298.0, 22.0);
        if r.contains(mouse()) {
            block(r, CARD_HI);
            key_tip = Some((format!("{} {} Opens {}.", e.boon, e.curse, e.name), e.key));
        }
        text("KEY", x, y + 18.0, 15.0, ACCENT);
        sprite(e.key_sprite, x + 88.0, y + 3.0, 20.0, WHITE);
        text(e.key, x + 114.0, y + 18.0, 16.0, GOLD_C);
        y += 24.0;
    }
    text("PACTS", x, y + 18.0, 15.0, ACCENT);
    let top = y + 28.0;
    let passive_desc = run.trait_desc();
    let passive_label = if run.trait_rank > 0 { format!("PASSIVE · RANK {}", run.trait_rank + 1) } else { "PASSIVE".into() };
    let (mut tip, mut tip_label): (&str, &str) = held.map_or((passive_desc.as_str(), passive_label.as_str()), |b| (b.desc, "BLESSING"));
    if let Some((desc, name)) = &key_tip {
        (tip, tip_label) = (desc, name);
    }
    let n = run.charms.len();
    {
        // icons, row by row (hover one for its name and text), and a "+N" for any that do not fit
        let (s, cols) = (32.0, 9);
        let cap = cols * ((576.0 - top) / s).floor().max(1.0) as usize;
        let shown = if n > cap { cap - 1 } else { n };
        let slot = |k: usize| Rect::new(x + (k % cols) as f32 * s, top + (k / cols) as f32 * s, 28.0, 28.0);
        for (k, &id) in run.charms.iter().enumerate().take(shown) {
            let r = slot(k);
            let hot = r.contains(mouse());
            if hot {
                tip = CHARMS[id].desc;
                tip_label = CHARMS[id].name;
            }
            charm_icon(id, r, if hot { 0.6 } else { 0.0 });
        }
        if shown < n {
            more_slot(slot(shown), n - shown, &run.charms);
        }
    }
    text(tip_label, x, 596.0, 15.0, ACCENT);
    wrapped(tip, x, 618.0, 282.0, 16.0, INK);
    // kept inside the panel's 282 px column, however long the seed
    let line = format!("Seed {}  M: mute  N: next song", run.seed);
    text(&line, x, 686.0, 14.0 * (282.0 / measure(&line, 14.0).width).min(1.0), MUTED);
    chosen
}

fn charm_card(r: Rect, id: usize, on: bool, held: &[usize]) -> bool {
    let c = &CHARMS[id];
    let edge = rarity_color(c.rarity);
    let hit = card(r, on);
    // the frame weighs more the rarer the pact
    if !(on && r.contains(mouse())) {
        outline(r, match c.rarity {
            Rarity::Common => FAINT,
            Rarity::Rare => MUTED,
            Rarity::Legendary => INK,
        });
    }
    let bob = (now() as f32 * 2.5 + id as f32).sin() * 2.0;
    charm_icon(id, Rect::new(r.x + 20.0, r.y + 22.0 + bob, 64.0, 64.0), 0.0);
    pact_name(id, r.x + 100.0, r.y + 52.0, 24.0, INK);
    rich(c.rarity.name(), r.x + 100.0, r.y + 78.0, 16.0, edge);
    wrapped(c.desc, r.x + 24.0, r.y + 124.0, r.w - 48.0, 16.0, INK);
    let (line, awake) = set_line(held, id);
    let set = c.set.def();
    text(&format!("{} SET", set.name.to_uppercase()), r.x + 24.0, r.bottom() - 56.0, 13.0, if awake { GOLD_C } else { ACCENT });
    wrapped_fit(&line[set.name.len() + 1..], r.x + 24.0, r.bottom() - 34.0, r.w - 48.0, 14.0, 34.0, MUTED);
    hit
}

pub fn pick(run: &Run, options: &[usize]) -> Option<usize> {
    // title and cards sit in the middle of the screen, however many cards there are
    let t0 = fx(|fx| fx.screen_t);
    stamped("Choose a pact", 640.0, 250.0, 40.0, INK);
    let mut chosen = None;
    let x0 = 640.0 - (options.len() as f32 * 350.0 - 30.0) / 2.0;
    for (k, &id) in options.iter().enumerate() {
        let p = stagger(t0, k, 0.06, 0.26);
        let dy = (1.0 - p) * 26.0;
        if charm_card(Rect::new(x0 + k as f32 * 350.0, 300.0 + dy, 320.0, 240.0), id, p > 0.5, &run.charms) {
            chosen = Some(id);
        }
    }
    chosen
}

/// A boss fell: the player picks one skill to upgrade. Returns its index in `COMBOS`.
pub fn upgrade(run: &Run, ids: &[usize]) -> Option<usize> {
    let t0 = fx(|fx| fx.screen_t);
    let boss = run.here() == NodeKind::Boss;
    lively_centered(if boss { "The boss falls" } else { "Old lore in the hoard" }, 640.0, 128.0, 44.0, GOLD, Motion::Wave);
    centered("Claim its strength. One rite grows fiercer, and hungrier.", 640.0, 164.0, 18.0, MUTED);
    let n = ids.len() as f32;
    let (w, gap) = (280.0, 20.0);
    let x0 = 640.0 - (n * w + (n - 1.0) * gap) / 2.0;
    let pulse = (now() as f32 * 5.0).sin() * 0.5 + 0.5;
    let mut chosen = None;
    for (k, &id) in ids.iter().enumerate() {
        let c = &COMBOS[id];
        let lvl = run.level(id);
        let sp = stagger(t0, k, 0.06, 0.26);
        let r = Rect::new(x0 + k as f32 * (w + gap), 196.0 + (1.0 - sp) * 26.0, w, 420.0);
        let col = el_color(c.needs[0]);
        let hot = r.contains(mouse()) && !modal();
        if card(r, sp > 0.5) {
            chosen = Some(id);
            play(Sfx::LevelUp);
        }
        if hot && gen_range(0.0, 1.0) < 0.5 {
            rise(vec2(gen_range(r.x + 20.0, r.right() - 20.0), r.bottom() - 16.0), 1, 4.0, col);
        }
        let x = r.x + 20.0;
        let arrow_dx = (now() as f32 * 5.0).sin() * 2.0;
        text(&format!("LV {lvl}"), x, r.y + 30.0, 16.0, ACCENT);
        text(">", x + 50.0 + arrow_dx, r.y + 30.0, 16.0, GOLD);
        text(&format!("LV {}", lvl + 1), x + 70.0, r.y + 30.0, 16.0, GOLD);
        let more = "+1 brand socket";
        text(more, r.right() - 20.0 - measure(more, 13.0).width, r.y + 30.0, 13.0, MUTED);
        text(c.title(lvl), x, r.y + 60.0, 18.0, MUTED);
        text(c.title(lvl + 1), x, r.y + 98.0, 26.0, if lvl + 1 >= MAX_LEVEL { MASTERED } else { GOLD_C });
        for (row, (label, l)) in [("NOW", lvl), ("UPGRADED", lvl + 1)].into_iter().enumerate() {
            let y = r.y + 136.0 + row as f32 * 62.0;
            text(label, x, y, 14.0, if row == 0 { MUTED } else { ACCENT });
            let cost = c.cost(l);
            for (j, &e) in cost.iter().enumerate() {
                let is_new = row == 1 && j + 1 == cost.len();
                let bob = if is_new { (now() as f32 * 4.5).sin() * 2.0 } else { 0.0 };
                let p = vec2(x + 12.0 + j as f32 * 30.0, y + 24.0 + bob);
                if is_new {
                    // the extra gem the upgrade asks for
                    draw_rectangle_lines(p.x - 15.0, p.y - 15.0, 30.0, 30.0, 1.5, fade(INK, 0.4 + 0.5 * pulse));
                }
                gem(e, p, 11.0);
            }
        }
        let brands = run.brands_of(id);
        let y = wrapped_fit(&rite_text(c, lvl, &brands), x, r.y + 280.0, w - 40.0, 16.0, 44.0, MUTED);
        text("becomes", x, y + 6.0, 14.0, ACCENT);
        wrapped_fit(&rite_text(c, lvl + 1, &brands), x, y + 30.0, w - 40.0, 18.0, r.bottom() - y - 30.0, GOLD_C);
    }
    particles();
    chosen
}

thread_local!(static BRANDING: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) });

/// An elite fell: pick a brand, then the rite to burn it onto. Returns (index in `COMBOS`, index in `BRANDS`).
pub fn brand(run: &Run, offers: &[usize]) -> Option<(usize, usize)> {
    let t0 = fx(|fx| fx.screen_t);
    let Some(chosen) = BRANDING.with(|b| b.get()) else {
        let elite = run.here() == NodeKind::Elite;
        lively_centered(if elite { "The elite falls" } else { "Brands in the hoard" }, 640.0, 200.0, 44.0, GOLD, Motion::Wave);
        centered("Claim its mark. Burnt into a rite, it asks no gem in return.", 640.0, 236.0, 18.0, MUTED);
        let x0 = 640.0 - (offers.len() as f32 * 300.0 - 20.0) / 2.0;
        for (k, &id) in offers.iter().enumerate() {
            let sp = stagger(t0, k, 0.06, 0.26);
            let r = Rect::new(x0 + k as f32 * 300.0, 280.0 + (1.0 - sp) * 24.0, 280.0, 180.0);
            if card(r, !modal() && sp > 0.5) {
                BRANDING.with(|b| b.set(Some(id)));
                fx(|fx| fx.sub_t = now());
            }
            stamped(BRANDS[id].name, r.center().x, r.y + 64.0, 34.0, INK);
            let line = BRANDS[id].effect.text(1);
            rich(&(line[..1].to_uppercase() + &line[1..]), r.x + 24.0, r.y + 124.0, 18.0, INK);
        }
        return None;
    };
    let sub_t = fx(|fx| fx.sub_t);
    let b = &BRANDS[chosen];
    lively_centered(&format!("Burn {} onto a rite", b.name), 640.0, 128.0, 40.0, GOLD, Motion::Wave);
    centered("A rite bears one brand per level. Once it is full, a new burn sears the oldest away.", 640.0, 164.0, 18.0, MUTED);
    let ids = run.rites();
    let n = ids.len() as f32;
    let (w, gap) = (280.0, 20.0);
    let x0 = 640.0 - (n * w + (n - 1.0) * gap) / 2.0;
    let mut done = None;
    for (k, &id) in ids.iter().enumerate() {
        let c = &COMBOS[id];
        let lvl = run.level(id);
        let sp = stagger(sub_t, k, 0.05, 0.24);
        let r = Rect::new(x0 + k as f32 * (w + gap), 196.0 + (1.0 - sp) * 24.0, w, 340.0);
        let hot = r.contains(mouse()) && !modal();
        if card(r, !modal() && sp > 0.5) {
            done = Some((id, chosen));
            play(Sfx::LevelUp);
        }
        if hot && gen_range(0.0, 1.0) < 0.4 {
            rise(vec2(gen_range(r.x + 20.0, r.right() - 20.0), r.bottom() - 16.0), 1, 4.0, rgb(0xff8a4c));
        }
        let x = r.x + 20.0;
        text(c.title(lvl), x, r.y + 40.0, 26.0, INK);
        for (j, &e) in c.cost(lvl).iter().enumerate() {
            gem(e, vec2(x + 12.0 + j as f32 * 30.0, r.y + 76.0), 11.0);
        }
        let old = run.brands_of(id);
        let full = run.sockets_full(id);
        let socks = format!("BRANDS {}/{lvl}", old.len());
        text(&socks, r.right() - 20.0 - measure(&socks, 13.0).width, r.y + 40.0, 13.0, if full { MUTED } else { ACCENT });
        let y = wrapped_fit(&rite_text(c, lvl, &old), x, r.y + 120.0, w - 40.0, 16.0, 80.0, MUTED);
        text(&if full { format!("{} is seared away", old[0].name) } else { "becomes".into() }, x, y + 6.0, 14.0, ACCENT);
        let new: Vec<&Brand> = old.iter().skip(usize::from(full)).copied().chain([b]).collect();
        wrapped_fit(&rite_text(c, lvl, &new), x, y + 30.0, w - 40.0, 18.0, r.bottom() - y - 30.0, GOLD_C);
    }
    if button(Rect::new(565.0, 576.0, 150.0, 40.0), "Back", !modal(), false) {
        BRANDING.with(|b| b.set(None));
    }
    if done.is_some() {
        BRANDING.with(|b| b.set(None));
    }
    particles();
    done
}

thread_local!(static LEARNING: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) });

/// A rite's card: its name, gems, what it does at level 1 and what mastering it adds.
fn rite_card(r: Rect, id: usize, on: bool) -> bool {
    let c = &COMBOS[id];
    let hit = card(r, on);
    let x = r.x + 20.0;
    text(c.name, x, r.y + 40.0, 26.0, INK);
    for (j, &e) in c.needs.iter().enumerate() {
        gem(e, vec2(x + 12.0 + j as f32 * 30.0, r.y + 74.0), 11.0);
    }
    let y = wrapped_fit(&rite_text(c, 1, &[]), x, r.y + 116.0, r.w - 40.0, 18.0, 90.0, INK);
    if let Some(m) = c.master {
        text("MASTERED", x, y + 10.0, 13.0, ACCENT);
        wrapped_fit(&m.text(MAX_LEVEL), x, y + 32.0, r.w - 40.0, 15.0, r.bottom() - y - 40.0, MASTERED);
    }
    hit
}

/// An elite fell, or a hoard teaches: learn one rite of three, letting one go if four are held.
/// Some(None) takes none; Some(Some((rite, let go))) learns one.
pub fn learn(run: &Run, offers: &[usize]) -> Option<Option<(usize, Option<usize>)>> {
    let t0 = fx(|fx| fx.screen_t);
    let full = run.rites().len() >= MAX_RITES;
    let Some(chosen) = LEARNING.with(|l| l.get()) else {
        let title = match run.here() {
            NodeKind::Elite => "The elite falls",
            NodeKind::Boss => "The boss falls",
            _ => "Old lore in the hoard",
        };
        lively_centered(title, 640.0, 150.0, 44.0, GOLD, Motion::Wave);
        let sub = if full { "Learn a rite. You carry four, so one will have to go." } else { "Learn a rite. You can carry four." };
        centered(sub, 640.0, 186.0, 18.0, MUTED);
        let (w, gap) = (300.0, 24.0);
        let x0 = 640.0 - (offers.len() as f32 * (w + gap) - gap) / 2.0;
        let mut out = None;
        for (k, &id) in offers.iter().enumerate() {
            let sp = stagger(t0, k, 0.06, 0.26);
            if rite_card(Rect::new(x0 + k as f32 * (w + gap), 220.0 + (1.0 - sp) * 26.0, w, 230.0), id, !modal() && sp > 0.5) {
                play(Sfx::LevelUp);
                if full {
                    LEARNING.with(|l| l.set(Some(id)));
                    fx(|fx| fx.sub_t = now());
                } else {
                    out = Some(Some((id, None)));
                }
            }
        }
        if button(Rect::new(540.0, 486.0, 200.0, 40.0), "Take none", !modal(), false) {
            out = Some(None);
        }
        return out;
    };
    let sub_t = fx(|fx| fx.sub_t);
    lively_centered(&format!("Learn {}: let one rite go", COMBOS[chosen].name), 640.0, 150.0, 40.0, GOLD, Motion::Wave);
    centered("Its levels and brands go with it.", 640.0, 186.0, 18.0, MUTED);
    let ids = run.rites();
    let (w, gap) = (260.0, 20.0);
    let x0 = 640.0 - (ids.len() as f32 * (w + gap) - gap) / 2.0;
    let mut done = None;
    for (k, &id) in ids.iter().enumerate() {
        let (c, lvl) = (&COMBOS[id], run.level(id));
        let sp = stagger(sub_t, k, 0.05, 0.24);
        let r = Rect::new(x0 + k as f32 * (w + gap), 220.0 + (1.0 - sp) * 24.0, w, 300.0);
        if card(r, !modal() && sp > 0.5) {
            done = Some(Some((chosen, Some(id))));
            play(Sfx::Click);
        }
        let x = r.x + 20.0;
        text(c.title(lvl), x, r.y + 40.0, 24.0, INK);
        if lvl > 1 {
            right(&format!("Lv {lvl}"), r.right() - 20.0, r.y + 40.0, 15.0, if lvl >= MAX_LEVEL { MASTERED } else { GOLD_C });
        }
        for (j, &e) in c.cost(lvl).iter().enumerate() {
            gem(e, vec2(x + 12.0 + j as f32 * 30.0, r.y + 74.0), 11.0);
        }
        wrapped_fit(&rite_text(c, lvl, &run.brands_of(id)), x, r.y + 116.0, w - 40.0, 16.0, 160.0, MUTED);
    }
    if button(Rect::new(565.0, 572.0, 150.0, 40.0), "Back", !modal(), false) {
        LEARNING.with(|l| l.set(None));
    }
    if done.is_some() {
        LEARNING.with(|l| l.set(None));
    }
    done
}

/// The shopkeeper reacts to something for a few seconds.
fn say(line: &'static str) {
    fx(|fx| fx.line = Some((line, now())));
}

/// The hero levelled up after battle: choose Vitality & Spoils (`Some(false)`) or Deepen Trait (`Some(true)`).
pub fn level_up(run: &Run) -> Option<bool> {
    let t0 = fx(|fx| fx.screen_t);
    ambient_motes(14, 53.0, 0.2);
    rune_circle(vec2(640.0, 560.0), 290.0, 42.0, GOLD, 0.22);
    lively_centered(&format!("Level {}", run.hero_lvl), 640.0, 178.0, 48.0, GOLD, Motion::Wave);
    let sub = if run.pending_lvl > 1 {
        format!("Battle hardens the bearer. Choose how to grow ({} choices left).", run.pending_lvl)
    } else {
        "Battle hardens the bearer. Choose how to grow.".into()
    };
    centered(&sub, 640.0, 214.0, 18.0, MUTED);

    let mut chosen = None;
    let sp0 = stagger(t0, 0, 0.06, 0.26);
    let r0 = Rect::new(220.0, 258.0 + (1.0 - sp0) * 26.0, 400.0, 236.0);
    if card(r0, !modal() && sp0 > 0.5) {
        chosen = Some(false);
        play(Sfx::LevelUp);
        burst(r0.center(), 24, 240.0, 7.0, GOLD_C);
        rect_ring(r0, 14.0, 2.2, GOLD, 0.3);
    }
    text("VITALITY & SPOILS", r0.x + 24.0, r0.y + 38.0, 14.0, ACCENT);
    stamped(&format!("+{} HP  ·  +{} Shards", crate::run::LEVEL_HP, crate::run::LEVEL_SHARDS), r0.center().x, r0.y + 92.0, 30.0, GOLD_C);
    let cur_hp = run.max_hp();
    text(&format!("Max HP:  {cur_hp} -> {}", cur_hp + crate::run::LEVEL_HP), r0.x + 24.0, r0.y + 144.0, 17.0, INK);
    text(&format!("Shards:  {} -> {}", run.shards, run.shards + crate::run::LEVEL_SHARDS), r0.x + 24.0, r0.y + 174.0, 17.0, INK);
    text("More room to survive cave-ins and buy wares.", r0.x + 24.0, r0.y + 210.0, 14.0, MUTED);

    let sp1 = stagger(t0, 1, 0.06, 0.26);
    let r1 = Rect::new(660.0, 258.0 + (1.0 - sp1) * 26.0, 400.0, 236.0);
    let h = &HEROES[run.hero];
    if card(r1, !modal() && sp1 > 0.5) {
        chosen = Some(true);
        play(Sfx::LevelUp);
        burst(r1.center(), 24, 240.0, 7.0, GOLD_C);
        rect_ring(r1, 14.0, 2.2, GOLD, 0.3);
    }
    text(&format!("TRAIT · RANK {} -> {}", run.trait_rank + 1, run.trait_rank + 2), r1.x + 24.0, r1.y + 38.0, 14.0, GOLD);
    stamped("Deepen Trait", r1.center().x, r1.y + 92.0, 32.0, INK);
    let y = wrapped_fit(h.trait_up_desc, r1.x + 24.0, r1.y + 132.0, r1.w - 48.0, 17.0, 52.0, GOLD_C);
    let live_trait = run.trait_desc();
    wrapped_fit(&live_trait, r1.x + 24.0, y + 10.0, r1.w - 48.0, 13.0, r1.bottom() - y - 16.0, MUTED);
    particles();
    chosen
}

/// A shrine fight was won: take one blessing, or keep the one held. Some(None) keeps it.
pub fn blessing(run: &Run, offers: &[usize]) -> Option<Option<usize>> {
    let t0 = fx(|fx| fx.screen_t);
    ambient_motes(14, 41.0, 0.18);
    rune_circle(vec2(640.0, 560.0), 290.0, 42.0, GOLD, 0.22);
    lively_centered("The shrine answers", 640.0, 200.0, 44.0, GOLD, Motion::Wave);
    let held = run.blessing.map(|b| BLESSINGS[b].name);
    let sub = match held {
        Some(name) => format!("You hold one blessing at a time. Taking another lets {name} go."),
        None => "Take one blessing. You hold one at a time.".into(),
    };
    centered(&sub, 640.0, 236.0, 18.0, MUTED);
    let x0 = 640.0 - (offers.len() as f32 * 300.0 - 20.0) / 2.0;
    let mut chosen = None;
    for (k, &id) in offers.iter().enumerate() {
        let sp = stagger(t0, k, 0.06, 0.26);
        let r = Rect::new(x0 + k as f32 * 300.0, 280.0 + (1.0 - sp) * 26.0, 280.0, 180.0);
        if card(r, !modal() && sp > 0.5) {
            chosen = Some(Some(id));
            play(Sfx::LevelUp);
        }
        text("BLESSING OF", r.x + 24.0, r.y + 40.0, 14.0, ACCENT);
        stamped(BLESSINGS[id].name, r.center().x, r.y + 84.0, 34.0, GOLD_C);
        wrapped(BLESSINGS[id].desc, r.x + 24.0, r.y + 124.0, r.w - 48.0, 16.0, INK);
    }
    if let Some(name) = held {
        if button(Rect::new(540.0, 500.0, 200.0, 40.0), &format!("Keep {name}"), !modal(), false) {
            chosen = Some(None);
        }
    }
    chosen
}

/// Returns true when the player leaves.
pub fn shop(run: &mut Run, shop: &mut Shop) -> bool {
    let t = now();
    let (screen_t, keeper_hop, restock_t, shown_shards) = fx(|fx| {
        if !fx.shop_shards_init {
            fx.shop_shards = run.shards as f32;
            fx.shop_shards_init = true;
        } else {
            fx.shop_shards += (run.shards as f32 - fx.shop_shards) * (dt() * 12.0).min(1.0);
            if (fx.shop_shards - run.shards as f32).abs() < 0.4 {
                fx.shop_shards = run.shards as f32;
            }
        }
        (fx.screen_t, fx.keeper_hop, fx.restock_t, fx.shop_shards.round() as i32)
    });
    let shelf_t = screen_t.max(restock_t);
    // a plain heading, like every other screen
    stamped("Tinker's Stall", 640.0, 112.0, 40.0, INK);

    // the shopkeeper's booth: the keeper and your purse
    let booth = Rect::new(48.0, 140.0, 290.0, 494.0);
    plate(booth, WELL);
    let hop_el = (t - keeper_hop) as f32;
    let hop = if (0.0..0.32).contains(&hop_el) { (hop_el / 0.32 * std::f32::consts::PI).sin() * 14.0 } else { 0.0 };
    sprite(HD + 27, 118.0, 290.0 + (t as f32 * 2.0).sin() * 3.0 - hop, 150.0, WHITE);
    rounded(Rect::new(72.0, 470.0, 242.0, 2.0), 1.0, fade(MUTED, 0.25));
    text("PURSE", 70.0, 518.0, 15.0, ACCENT);
    let shard_pulse = if shown_shards != run.shards { 1.18 } else { 1.0 };
    shard(vec2(82.0, 556.0), 11.0 * shard_pulse);
    text(&shown_shards.to_string(), 102.0, 568.0, 32.0, GOLD_C);

    // the wares
    // spots past the pacts: the brand on the shelf, then the lesson, then the rite
    let (brand_k, lesson_k, rite_k) = (shop.stock.len(), shop.stock.len() + 1, shop.stock.len() + 2);
    let prev_sel = fx(|fx| fx.pick);
    let mut sel = prev_sel.filter(|&k| shop.stock.get(k).is_some_and(|w| w.is_some()) || (k == brand_k && shop.brand.is_some()) || (k == lesson_k && shop.lesson) || (k == rite_k && shop.rite.is_some()));
    let mut hover = None;
    for k in 0..shop.stock.len() {
        let sp = stagger(shelf_t, k, 0.05, 0.24);
        let base_r = Rect::new(370.0 + k as f32 * 220.0, 140.0 + (1.0 - sp) * 20.0, 200.0, 216.0);
        let Some(id) = shop.stock[k] else {
            let st = fx(|fx| fx.sold_t[k.min(3)]);
            let sb = if st > 0.0 { bounce(st) } else { 1.0 };
            plate(base_r, WELL);
            stamped("SOLD", base_r.center().x, base_r.center().y + 12.0, 34.0 * sb, fade(MUTED, 0.6));
            continue;
        };
        let c = &CHARMS[id];
        let price = Run::price(shop, k);
        let hot = base_r.contains(mouse()) && !modal();
        if hot {
            hover = Some((k, price));
        }
        let hv = hover_lerp(base_r, hot || sel == Some(k));
        let r = Rect::new(base_r.x, base_r.y - 3.0 * hv, base_r.w, base_r.h);
        let edge = rarity_color(c.rarity);
        plate(r, mix(CARD, CARD_HI, hv));
        if hv > 0.05 {
            outline(r, mix(FAINT, INK, hv));
            corner_ticks(r, hv, INK);
        }
        let lift = -4.0 * hv + (t as f32 * 2.5 + k as f32).sin() * 2.0;
        charm_icon(id, Rect::new(r.center().x - 44.0, r.y + 18.0 + lift, 88.0, 88.0), if sel == Some(k) { 0.3 } else { 0.0 });
        pact_name(id, r.center().x - measure(c.name, 18.0).width / 2.0, r.y + 134.0, 18.0, INK);
        // rarity, then the pact's set and how many of it you hold
        let held = run.set_count(c.set as usize);
        let set = format!("  {} {}/{SET_AT}", c.set.def().name, held.min(SET_AT));
        let x = r.center().x - (measure(c.rarity.name(), 14.0).width + measure(&set, 14.0).width) / 2.0;
        rich(c.rarity.name(), x, r.y + 154.0, 14.0, edge);
        text(&set, x + measure(c.rarity.name(), 14.0).width, r.y + 154.0, 14.0, if held + 1 >= SET_AT { GOLD_C } else { MUTED });
        let tag = Rect::new(r.center().x - 52.0, r.y + 168.0, 104.0, 34.0);
        let afford = run.shards >= price;
        block(tag, WELL);
        shard(vec2(tag.x + 22.0, tag.center().y), 8.0);
        text(&price.to_string(), tag.x + 40.0, tag.y + 25.0, 22.0, if afford { GOLD_C } else { MUTED });
        if k == shop.sale {
            block(Rect::new(r.right() - 66.0, r.y - 10.0, 74.0, 26.0), INK);
            centered("-25%", r.right() - 29.0, r.y + 9.0, 16.0, OUT);
        }
        if hot && clicked() {
            sel = Some(k);
            play(Sfx::Click);
            rect_ring(r, 12.0, 1.8, fade(INK, 0.5), 0.2);
        }
    }

    let sp = stagger(shelf_t, brand_k, 0.05, 0.24);
    let base_r = Rect::new(370.0 + brand_k as f32 * 220.0, 140.0 + (1.0 - sp) * 20.0, 200.0, 216.0);
    match shop.brand {
        None => {
            plate(base_r, WELL);
            stamped("SOLD", base_r.center().x, base_r.center().y + 12.0, 34.0, fade(MUTED, 0.6));
        }
        Some(id) => {
            let price = Run::brand_price(shop);
            let hot = base_r.contains(mouse()) && !modal();
            if hot {
                hover = Some((brand_k, price));
            }
            let hv = hover_lerp(base_r, hot || sel == Some(brand_k));
            let r = Rect::new(base_r.x, base_r.y - 3.0 * hv, base_r.w, base_r.h);
            plate(r, mix(CARD, CARD_HI, hv));
            if hv > 0.05 {
                outline(r, mix(FAINT, INK, hv));
                corner_ticks(r, hv, INK);
            }
            text("BRAND", r.x + 10.0, r.y + 22.0, 13.0, ACCENT);
            stamped(BRANDS[id].name, r.center().x, r.y + 84.0, 34.0, INK);
            fit_text(&BRANDS[id].effect.text(1), r.x + 14.0, r.y + 134.0, r.w - 28.0, 15.0, MUTED);
            let tag = Rect::new(r.center().x - 52.0, r.y + 168.0, 104.0, 34.0);
            block(tag, WELL);
            shard(vec2(tag.x + 22.0, tag.center().y), 8.0);
            text(&price.to_string(), tag.x + 40.0, tag.y + 25.0, 22.0, if run.shards >= price { GOLD_C } else { MUTED });
            if hot && clicked() {
                sel = Some(brand_k);
                play(Sfx::Click);
                rect_ring(r, 12.0, 1.8, fade(INK, 0.5), 0.2);
            }
        }
    }

    if sel != prev_sel {
        fx(|fx| fx.sel_t = t);
    }
    let dp = ease_out3(((t - fx(|fx| fx.sel_t)) as f32 / 0.16).clamp(0.0, 1.0));
    // a closer look at the chosen ware, and the button that buys it
    let panel = Rect::new(356.0, 410.0 + (1.0 - dp) * 6.0, 884.0, 156.0);
    plate(panel, CARD);
    match sel.and_then(|k| shop.stock.get(k).copied().flatten().map(|id| (k, id))) {
        Some((k, id)) => {
            let c = &CHARMS[id];
            charm_icon(id, Rect::new(panel.x + 22.0, panel.y + 22.0, 64.0, 64.0), 0.0);
            pact_name(id, panel.x + 106.0, panel.y + 46.0, 28.0, INK);
            rich(c.rarity.name(), panel.x + 106.0, panel.y + 70.0, 16.0, rarity_color(c.rarity));
            let (line, awake) = set_line(&run.charms, id);
            let after = panel.x + 106.0 + measure(c.rarity.name(), 16.0).width + 18.0;
            fit_text(&format!("{} set {}", c.set.def().name, &line[c.set.def().name.len() + 1..]), after, panel.y + 70.0, panel.right() - 260.0 - after, 15.0, if awake { GOLD_C } else { MUTED });
            wrapped(c.desc, panel.x + 106.0, panel.y + 100.0, 500.0, 18.0, INK);
            let price = Run::price(shop, k);
            if button(Rect::new(panel.right() - 240.0, panel.y + 56.0, 210.0, 48.0), &format!("Buy for {price}"), run.shards >= price, true) && run.buy(shop, k) {
                play(Sfx::Coins);
                burst(vec2(panel.right() - 135.0, panel.y + 80.0), 24, 240.0, 7.0, GOLD_C);
                ring(vec2(panel.right() - 135.0, panel.y + 80.0), 18.0, 82.0, 4, 0.0, 2.5, GOLD, 0.32);
                fx(|fx| {
                    fx.keeper_hop = t;
                    fx.sold_t[k.min(3)] = t;
                });
                say("A fine choice. It suits you.");
                sel = None;
            }
        }
        None if sel == Some(rite_k) => {
            // a rite to learn: bought outright while there is room, else by choosing the one it replaces
            let id = shop.rite.expect("sel only points at a rite for sale");
            let price = Run::rite_price(shop);
            text(COMBOS[id].name, panel.x + 22.0, panel.y + 46.0, 28.0, INK);
            for (j, &e) in COMBOS[id].needs.iter().enumerate() {
                gem(e, vec2(panel.x + 34.0 + j as f32 * 26.0, panel.y + 70.0), 9.0);
            }
            wrapped_fit(&rite_text(&COMBOS[id], 1, &[]), panel.x + 22.0, panel.y + 100.0, 310.0, 15.0, 48.0, MUTED);
            let mut bought = false;
            if run.rites().len() < MAX_RITES {
                bought = button(Rect::new(panel.right() - 240.0, panel.y + 56.0, 210.0, 48.0), &format!("Learn for {price}"), run.shards >= price, true) && run.buy_rite(shop, None);
            } else {
                text("You carry four. Replace:", panel.x + 350.0, panel.y + 16.0, 14.0, ACCENT);
                for (j, held) in run.rites().into_iter().enumerate() {
                    let r = Rect::new(panel.x + 350.0 + (j % 2) as f32 * 262.0, panel.y + 26.0 + (j / 2) as f32 * 60.0, 252.0, 50.0);
                    if button(r, COMBOS[held].title(run.level(held)), run.shards >= price, false) && run.buy_rite(shop, Some(held)) {
                        bought = true;
                    }
                }
            }
            if bought {
                play(Sfx::LevelUp);
                burst(vec2(panel.right() - 135.0, panel.y + 80.0), 24, 240.0, 7.0, GOLD_C);
                ring(vec2(panel.right() - 135.0, panel.y + 80.0), 18.0, 82.0, 4, 0.0, 2.5, GOLD, 0.32);
                fx(|fx| fx.keeper_hop = t);
                say("Practice. It shows already.");
                sel = None;
            }
        }
        None if sel.is_some() => {
            // the brand or the lesson: bought by choosing the rite it goes to
            let lesson = sel == Some(lesson_k);
            let (name, what, price) = match shop.brand.filter(|_| !lesson) {
                Some(b) => (BRANDS[b].name, format!("Brand: {}. Free to cast; one per rite level.", BRANDS[b].effect.text(1)), Run::brand_price(shop)),
                None => ("Lesson", "A rite grows a level, and a brand socket with it.".to_string(), Run::lesson_price(shop)),
            };
            text(name, panel.x + 22.0, panel.y + 46.0, 28.0, INK);
            wrapped(&what, panel.x + 22.0, panel.y + 76.0, 300.0, 15.0, MUTED);
            let mut tip = None;
            for (j, id) in run.rites().into_iter().enumerate() {
                let (c, lvl) = (&COMBOS[id], run.level(id));
                let r = Rect::new(panel.x + 350.0 + (j % 2) as f32 * 262.0, panel.y + 20.0 + (j / 2) as f32 * 62.0, 252.0, 52.0);
                if r.contains(mouse()) {
                    tip = Some(id);
                }
                let ok = run.shards >= price && (!lesson || lvl < MAX_LEVEL);
                if button(r, c.title(lvl), ok, false) && if lesson { run.buy_lesson(shop, id) } else { run.buy_brand(shop, id) } {
                    play(if lesson { Sfx::LevelUp } else { Sfx::Coins });
                    burst(r.center(), 24, 240.0, 7.0, GOLD_C);
                    ring(r.center(), 18.0, 74.0, 4, 0.0, 2.5, GOLD, 0.3);
                    fx(|fx| fx.keeper_hop = t);
                    say(if lesson { "Practice. It shows already." } else { "Hold still. It only stings once." });
                    sel = None;
                }
            }
            let line = match tip {
                Some(id) if lesson && run.level(id) >= MAX_LEVEL => "Mastered: nothing left to teach.".into(),
                Some(id) if lesson => rite_text(&COMBOS[id], run.level(id) + 1, &run.brands_of(id)),
                Some(id) => {
                    let old = run.brands_of(id);
                    let full = run.sockets_full(id);
                    let new: Vec<&Brand> = old.iter().skip(usize::from(full)).copied().chain(shop.brand.map(|b| &BRANDS[b])).collect();
                    rite_text(&COMBOS[id], run.level(id), &new)
                }
                None => format!("{price} shards. Choose the rite."),
            };
            wrapped_fit(&line, panel.x + 22.0, panel.y + 126.0, 310.0, 14.0, 28.0, if tip.is_some() { GOLD_C } else { MUTED });
        }
        None => centered("Click a ware to take a closer look.", panel.center().x, panel.center().y + 6.0, 18.0, MUTED),
    }

    // services
    let lesson = Run::lesson_price(shop);
    if button(Rect::new(356.0, 590.0, 210.0, 44.0), &format!("Lesson for {lesson}"), shop.lesson && !run.rites().is_empty() && run.shards >= lesson, sel == Some(lesson_k)) {
        sel = Some(lesson_k);
        play(Sfx::Click);
    }
    let rite = Run::rite_price(shop);
    let rite_label = shop.rite.map_or("Rite sold".to_string(), |id| format!("{} for {rite}", COMBOS[id].name));
    if button(Rect::new(578.0, 590.0, 250.0, 44.0), &rite_label, shop.rite.is_some(), sel == Some(rite_k)) {
        sel = Some(rite_k);
        play(Sfx::Click);
    }
    let restock = Run::restock_price(shop);
    if button(Rect::new(840.0, 590.0, 200.0, 44.0), &format!("Restock {restock}"), run.shards >= restock && run.can_restock(shop), false) && run.restock(shop) {
        play(Sfx::Coins);
        sel = None;
        fx(|fx| {
            fx.restock_t = t;
            fx.keeper_hop = t;
        });
        for k in 0..=SHOP_SIZE {
            burst(vec2(470.0 + k as f32 * 220.0, 250.0), 14, 200.0, 7.0, ACCENT);
        }
        say("Fresh from the deep. Have a look.");
    }
    let leave = button(Rect::new(1052.0, 590.0, 188.0, 44.0), "Leave", true, false);

    // what the keeper says: a reaction for a few seconds, else a comment on what you are eyeing,
    // else a greeting. Each line is typed out.
    let reaction = fx(|fx| fx.line).filter(|l| t - l.1 < 2.5).map(|l| l.0);
    let line = reaction.unwrap_or(match hover {
        Some((_, price)) if price > run.shards => "Shards first, friend.",
        Some((k, _)) if k == shop.sale => "That one is a steal. Truly.",
        _ => ["Mind the floor. Everything else is for sale.", "Pacts, cures, curios. Look, don't lick.", "Back from the deep? Spend before it spends you."][run.floor % 3],
    });
    let since = fx(|fx| {
        if fx.said.0 != line {
            fx.said = (line, t);
        }
        t - fx.said.1
    });
    let shown: String = line.chars().take((since * 50.0) as usize).collect();
    let bubble = Rect::new(62.0, 164.0, 262.0, 96.0);
    plate(bubble, CARD_HI);
    draw_triangle(vec2(176.0, bubble.bottom()), vec2(196.0, bubble.bottom()), vec2(186.0, bubble.bottom() + 14.0), CARD_HI);
    wrapped(&shown, bubble.x + 14.0, bubble.y + 26.0, bubble.w - 28.0, 16.0, INK);

    fx(|fx| fx.pick = sel);
    particles();
    leave
}

/// Returns true when the encounter is finished.
pub fn event(run: &mut Run, e: &Encounter, result: &mut Option<String>) -> bool {
    let t0 = fx(|fx| fx.screen_t);
    let ep = ease_out3(((now() - t0) as f32 / 0.2).clamp(0.0, 1.0));
    let panel = Rect::new(190.0, 100.0 + (1.0 - ep) * 14.0, 900.0, 540.0);
    plate(panel, CARD);
    stamped(e.title, 640.0, panel.y + 70.0, 40.0, INK);
    let y = wrapped(e.text, 240.0, panel.y + 130.0, 800.0, 22.0, MUTED) + 30.0;
    if let Some(r) = result {
        let rp = ease_out3(((now() - fx(|fx| fx.sub_t)) as f32 / 0.22).clamp(0.0, 1.0));
        wrapped(r, 240.0, y + (1.0 - rp) * 12.0, 800.0, 22.0, fade(GOLD_C, rp));
        particles();
        let cont = button(Rect::new(540.0, y + 80.0, 200.0, 44.0), "Continue", true, true);
        return rp > 0.4 && (cont || (!modal() && (is_key_pressed(KeyCode::Enter) || is_key_pressed(KeyCode::Space))));
    }
    // a choice that costs a pact first asks which one; remembered per encounter
    thread_local!(static GIVING: std::cell::Cell<Option<(*const Encounter, usize)>> = const { std::cell::Cell::new(None) });
    let giving = GIVING.with(|g| g.get()).filter(|g| std::ptr::eq(g.0, e)).map(|g| g.1);
    if let Some(k) = giving {
        let c = &e.choices[k];
        centered("Give up which pact?", 640.0, y + 4.0, 18.0, INK);
        let mut tip = "";
        for (j, id) in run.charms.clone().into_iter().enumerate() {
            let r = Rect::new(240.0 + (j % 3) as f32 * 270.0, y + 24.0 + (j / 3) as f32 * 50.0, 260.0, 40.0);
            if r.contains(mouse()) {
                tip = CHARMS[id].desc;
            }
            if button(r, CHARMS[id].name, true, false) {
                *result = Some(run.choose(c, Some(id)));
                fx(|fx| fx.sub_t = now());
                burst(vec2(640.0, y + 20.0), 18, 180.0, 5.0, GOLD);
                GIVING.with(|g| g.set(None));
            }
        }
        wrapped(tip, 240.0, 580.0, 600.0, 16.0, MUTED);
        if button(Rect::new(890.0, 576.0, 150.0, 40.0), "Back", true, false) {
            GIVING.with(|g| g.set(None));
        }
        return false;
    }
    // a way toward a key is hidden once that key cannot be taken
    let shown: Vec<(usize, &Choice)> = e.choices.iter().enumerate().filter(|(_, c)| run.shows(c)).collect();
    for (row, (k, c)) in shown.into_iter().enumerate() {
        let sp = stagger(t0, row + 1, 0.05, 0.22);
        if button(Rect::new(240.0, y + row as f32 * 60.0 + (1.0 - sp) * 16.0, 800.0, 44.0), c.label, run.can_afford(c) && sp > 0.5, false) {
            if c.outs.iter().any(|o| matches!(o, Out::LoseCharm)) {
                GIVING.with(|g| g.set(Some((e as *const Encounter, k))));
            } else {
                *result = Some(run.choose(c, None));
                fx(|fx| fx.sub_t = now());
                burst(vec2(640.0, y + 20.0), 18, 180.0, 5.0, GOLD);
            }
        }
    }
    false
}

/// Returns true when the player heads back to the overworld. `opened` is an expedition this win unlocked.
pub fn end(run: &Run, won: bool, opened: Option<usize>) -> bool {
    summary(run, None, won, opened)
}

/// The run's summary card. It pops in, then its lines arrive one at a time (the damage counting
/// up), then the pacts one by one, then any new expedition, and last the way out. Over the last
/// fight it stands in for the result card; `b` is that fight, not yet added to the run's totals.
fn summary(run: &Run, b: Option<&Battle>, won: bool, opened: Option<usize>) -> bool {
    let t0 = born((9, 9, 9));
    let t = (now() - t0) as f32;
    if won {
        ambient_motes(18, 99.0, 0.25);
    }
    let (wins, dealt, best) = match b {
        Some(b) => (run.wins + b.won() as usize, run.dealt + b.dealt, if b.best.0 > run.best.0 { b.best } else { run.best }),
        None => (run.wins, run.dealt, run.best),
    };
    // a beat crossing `at` this frame, for its sound
    let at_beat = |at: f32| dt() > 0.0 && t >= at && t - dt() < at;
    let appear = |at: f32| ((t - at) / 0.25).clamp(0.0, 1.0);
    let r = scaled(Rect::new(330.0, 66.0, 620.0, 590.0), bounce(t0));
    plate(r, BG);
    // past the last floor's boss the run counts as won, even if an ending's floor ends it
    let past = run.floor >= FLOORS;
    sprite(HEROES[run.hero].sprite, 604.0, r.y + 22.0, 72.0, if won || past { WHITE } else { fade(MUTED, 0.5) });
    let p = appear(0.15);
    if won {
        lively_centered("The last board is clear.", 640.0, r.y + 142.0, 40.0, fade(INK, p), Motion::Rainbow);
    } else if past {
        lively_centered("The depths turn you back.", 640.0, r.y + 142.0, 40.0, fade(INK, p), Motion::Wave);
    } else {
        lively_centered("Your run ends here.", 640.0, r.y + 142.0, 40.0, fade(BLOOD, p), Motion::Tremble);
    }
    let place = run.daily.map_or(EXPEDITIONS[run.exp].name.to_string(), |d| format!("{} (daily)", daily_name(d)));
    let sub = format!("{} (Lv {})  ·  {place}  ·  floor {} of {}", HEROES[run.hero].name, run.hero_lvl, run.floor + 1, run.floors());
    centered(&sub, 640.0, r.y + 174.0, 18.0, fade(MUTED, p));
    draw_line(r.x + 50.0, r.y + 196.0, r.right() - 50.0, r.y + 196.0, 1.0, fade(FAINT, p));
    // the numbers, one line at a time; the damage counts up
    let count = ((t - 0.8) / 0.8).clamp(0.0, 1.0);
    let rows = [
        ("Fights won", wins.to_string()),
        ("Rite damage", ((dealt as f32 * (1.0 - (1.0 - count).powi(3))).round() as i32).to_string()),
        ("Best hit", if best.0 > 0 { format!("{} · {}", best.0, best.1) } else { "none".into() }),
        ("Ore banked", format!("+{}", run.ore(wins, won))),
    ];
    for (k, (label, value)) in rows.iter().enumerate() {
        let at = 0.5 + k as f32 * 0.3;
        if at_beat(at) {
            play(Sfx::Flip);
        }
        let p = appear(at);
        let y = r.y + 232.0 + k as f32 * 31.0;
        text(label, r.x + 60.0 + 30.0 * (1.0 - p), y, 20.0, fade(MUTED, p));
        right(value, r.right() - 60.0, y, 22.0, fade(INK, p));
    }
    // every pact the run gathered, popping in one by one in the order taken
    let n = run.charms.len();
    let pacts_at = 1.5;
    text(&format!("Pacts  {n}"), r.x + 60.0, r.y + 362.0, 20.0, fade(MUTED, appear(pacts_at)));
    let step = 36.0f32.min((r.w - 120.0) / n.max(1) as f32);
    for (k, &id) in run.charms.iter().enumerate() {
        let at = pacts_at + 0.15 + k as f32 * 0.07;
        if t >= at {
            let s = (step - 4.0) * bounce(t0 + at as f64);
            charm_icon(id, Rect::new(r.x + 60.0 + k as f32 * step, r.y + 378.0, s, s), (1.0 - (t - at) / 0.3).max(0.0));
        }
    }
    let mut last = pacts_at + 0.15 + n as f32 * 0.07;
    // secret endings whose boss fell, counting a last fight just won
    let mut found = run.found.clone();
    if let Some(e) = b.and_then(|b| run.ending_found(b)) {
        if !found.contains(&e) {
            found.push(e);
        }
    }
    let mut y = r.y + 460.0;
    if !found.is_empty() {
        last += 0.35;
        if at_beat(last) {
            play(Sfx::LevelUp);
        }
        if t >= last {
            let k = bounce(t0 + last as f64);
            lively_centered(if found.len() == 1 { "SECRET ENDING FOUND" } else { "SECRET ENDINGS FOUND" }, 640.0, r.y + 424.0, 16.0, GOLD, Motion::Wave);
            let names = found.iter().map(|&e| ENDINGS[e].name).collect::<Vec<_>>().join("  ·  ");
            // body size, below the display font's, so the size it pops in at measures the same
            let size = 18.0 * (540.0 / measure(&names, 18.0).width).min(1.0);
            centered(&names, 640.0, r.y + 454.0, size * k, INK);
        }
        y += 22.0;
    }
    if let Some(x) = opened {
        last += 0.35;
        if at_beat(last) {
            play(Sfx::LevelUp);
        }
        if t >= last {
            let k = bounce(t0 + last as f64);
            lively_centered("A NEW PATH OPENS", 640.0, y, 16.0, GOLD, Motion::Wave);
            centered(EXPEDITIONS[x].name, 640.0, y + 32.0, 28.0 * k, INK);
        }
    }
    text(&format!("seed {}", run.seed), r.x + 24.0, r.bottom() - 18.0, 14.0, FAINT);
    t >= last + 0.35 && button(Rect::new(520.0, r.bottom() - 70.0, 240.0, 44.0), "Back to the map", true, true)
}

/// A button for use inside an open panel, where `button` ignores clicks.
fn panel_button(r: Rect, label: &str, primary: bool, press: bool) -> bool {
    panel_button_on(r, label, true, primary, press)
}

/// Like `panel_button`, with an explicit enabled flag so unaffordable actions grey out cleanly.
fn panel_button_on(r: Rect, label: &str, on: bool, primary: bool, press: bool) -> bool {
    let hot = on && r.contains(mouse());
    let down = hot && is_mouse_button_down(MouseButton::Left);
    button_face(r, label, on, primary, hot, down);
    if hot && press {
        play(Sfx::Click);
        rect_ring(r, 9.0, 1.5, fade(INK, 0.45), 0.16);
    }
    hot && press
}

/// The Retreat button in the run's status bar, and the question it asks. True once the player
/// confirms: the run is abandoned and the game goes back to the title.
pub fn retreat() -> bool {
    give_up(Rect::new(1078.0, 20.0, 140.0, 32.0), "Retreat", ["You leave the expedition. This hero, its pacts", "and its shards are lost."])
}

/// Mid-fight: give up the battle, and with it the whole expedition. How far you got still counts.
pub fn surrender() -> bool {
    // a huge hit's impact frames own the whole screen for their blink
    if fx(|fx| fx.impact.is_some()) {
        return false;
    }
    give_up(Rect::new(56.0, 652.0, 150.0, 36.0), "Surrender", ["The fight is lost, and the expedition with it.", "How far you got still counts."])
}

/// A button that asks first: true once the player confirms `word`.
fn give_up(at: Rect, word: &str, lines: [&str; 2]) -> bool {
    if !RETREAT.with(|r| r.get()) {
        if button(at, word, !modal(), false) {
            RETREAT.with(|r| r.set(true));
            start_modal_anim();
        }
        return false;
    }
    let press = is_mouse_button_pressed(MouseButton::Left) && !SETTINGS.with(|s| s.get());
    let mp = modal_open_p();
    overlay();
    draw_rectangle(-2000.0, -2000.0, 5280.0, 5000.0, fade(OUT, 0.6 * mp));
    let panel = Rect::new(390.0, 240.0 + (1.0 - mp) * 16.0, 500.0, 220.0);
    plate(panel, CARD);
    stamped(&format!("{word}?"), 640.0, panel.y + 56.0, 34.0, INK);
    centered(lines[0], 640.0, panel.y + 96.0, 16.0, MUTED);
    centered(lines[1], 640.0, panel.y + 118.0, 16.0, MUTED);
    let quit = panel_button(Rect::new(panel.x + 40.0, panel.bottom() - 68.0, 200.0, 40.0), word, false, press);
    let stay = panel_button(Rect::new(panel.right() - 240.0, panel.bottom() - 68.0, 200.0, 40.0), "Keep going", true, press);
    if quit || stay {
        RETREAT.with(|r| r.set(false));
    }
    quit
}

/// Holds the battle still under the rules sheet. Call before the battle draws.
pub fn hold_for_rules(on: bool) {
    RULES.with(|r| r.set(on));
}

/// How to play, on one sheet over the first battle. True once the player has read it.
pub fn rules() -> bool {
    let press = is_mouse_button_pressed(MouseButton::Left) && !SETTINGS.with(|s| s.get());
    let mp = modal_open_p();
    overlay();
    draw_rectangle(-2000.0, -2000.0, 5280.0, 5000.0, fade(OUT, 0.7 * mp));
    // the sheet is as tall as what it says, centred on the screen
    let mut col = Column::new(320.0, 0.0, 640.0, VIEW.y, 17.0);
    for line in [
        "Mine a crystal for a gem of its element. You get 4 picks a turn.",
        "Mining cracks the tiles touching it. A tile with 3 cracks caves in: 10 damage to you, through shield.",
        "Click a tile without a crystal to vent: it and the tiles touching it lose a crack.",
        "Each gem pays for one flat basic: Attack, Shield or Heal (no rite buffs). Rites you find take more gems, carry buffs and do much more.",
        "Each crystal mined adds surge: +10% damage.",
        "NEXT shows the enemy's loaded rite. Deal its break damage first and it hits for half.",
        "Tabs or keys 1 to 3 swap boards. Hover a tile or a floating word to learn more.",
    ] {
        col.block(1, None, [line], INK);
    }
    let h = 96.0 + col.needed() + 84.0;
    let panel = Rect::new(280.0, ((VIEW.y - h) / 2.0).max(20.0) + (1.0 - mp) * 16.0, 720.0, h);
    plate(panel, CARD);
    stamped("How to play", 640.0, panel.y + 54.0, 38.0, INK);
    col.y = panel.y + 96.0;
    col.draw();
    let ok = panel_button(Rect::new(540.0, panel.bottom() - 64.0, 200.0, 40.0), "Got it", true, press);
    if ok {
        RULES.with(|r| r.set(false));
    }
    ok
}

/// The gear in the corner and, when open, the volume panel. Drawn last, over every screen.
pub fn settings() {
    let open = SETTINGS.with(|s| s.get());
    let press = is_mouse_button_pressed(MouseButton::Left);
    let gear = Rect::new(1236.0, 20.0, 32.0, 32.0);
    let hot = gear.contains(mouse());
    let gh = hover_lerp(gear, hot || open);
    block(grow(gear, 3.0), OUT);
    let gbg = mix(CARD_HI, ACCENT, gh);
    block(gear, gbg);
    drawn(grow(gear, 3.0), || "gear".into());
    // a crisp mechanical gear that smoothly turns 45 degrees when hovered or open
    let (c, ink) = (gear.center(), mix(INK, OUT, gh));
    let ang = gh * std::f32::consts::FRAC_PI_4;
    for k in 0..4 {
        let a = ang + k as f32 * std::f32::consts::FRAC_PI_2;
        let d = Vec2::from_angle(a);
        draw_line(c.x - d.x * 11.5, c.y - d.y * 11.5, c.x + d.x * 11.5, c.y + d.y * 11.5, 4.5, ink);
    }
    draw_circle(c.x, c.y, 8.0, ink);
    draw_circle(c.x, c.y, 4.0, gbg);
    let esc = is_key_pressed(KeyCode::Escape);
    // Esc closes any open modal sheet first before opening Settings on top of it
    let mut esc_consumed = false;
    if esc && !open {
        if HOWTO.with(|h| h.get()) {
            HOWTO.with(|h| h.set(false));
            esc_consumed = true;
        } else if RETREAT.with(|r| r.get()) {
            RETREAT.with(|r| r.set(false));
            esc_consumed = true;
        } else if FORGE_ON.with(|f| f.get()) {
            FORGE_ON.with(|f| f.set(false));
            esc_consumed = true;
        } else if DEEDS_ON.with(|d| d.get()) {
            DEEDS_ON.with(|d| d.set(false));
            esc_consumed = true;
        }
        if esc_consumed {
            play(Sfx::Click);
        }
    }
    let mut toggle = (hot && press) || (esc && !esc_consumed);
    if open {
        let mp = modal_open_p();
        overlay();
        draw_rectangle(-2000.0, -2000.0, 5280.0, 5000.0, fade(OUT, 0.6 * mp));
        let panel = Rect::new(400.0, 210.0 + (1.0 - mp) * 16.0, 480.0, 300.0);
        plate(panel, CARD);
        stamped("Settings", 640.0, panel.y + 52.0, 34.0, fade(INK, mp));
        let (mut music, mut sound) = VOLUME.with(|v| v.get());
        let (vp_row, vp_t) = fx(|fx| fx.vol_pop);
        let vp = (1.0 - ((get_time() - vp_t) as f32 / 0.18)).clamp(0.0, 1.0);
        for (row, (label, level)) in [("Music", &mut music), ("Sound", &mut sound)].into_iter().enumerate() {
            let y = panel.y + 90.0 + row as f32 * 62.0;
            text(label, panel.x + 36.0, y + 26.0, 22.0, fade(INK, mp));
            if panel_button(Rect::new(panel.x + 140.0, y, 40.0, 36.0), "-", false, press) {
                *level = (*level - 1).max(0);
                fx(|fx| fx.vol_pop = (row, get_time()));
            }
            if panel_button(Rect::new(panel.x + 404.0, y, 40.0, 36.0), "+", false, press) {
                *level = (*level + 1).min(10);
                fx(|fx| fx.vol_pop = (row, get_time()));
            }
            for k in 0..10 {
                let on = k < *level;
                let pop = if vp_row == row && on && k + 1 == *level { vp * 4.0 } else { 0.0 };
                let col = if on { mix(ACCENT, INK, if vp_row == row && k + 1 == *level { vp } else { 0.0 }) } else { WELL };
                block(Rect::new(panel.x + 192.0 + k as f32 * 21.0, y + 6.0 - pop, 16.0, 24.0 + pop * 2.0), col);
            }
        }
        VOLUME.with(|v| v.set((music, sound)));
        centered("M mutes everything.  N skips to the next song.", 640.0, panel.y + 230.0, 15.0, fade(MUTED, mp));
        if panel_button(Rect::new(panel.x + 50.0, panel.bottom() - 54.0, 180.0, 36.0), "How to play", false, press) {
            HOWTO.with(|h| h.set(true));
            SETTINGS.with(|s| s.set(false));
            start_modal_anim();
            return;
        }
        toggle |= panel_button(Rect::new(panel.right() - 230.0, panel.bottom() - 54.0, 180.0, 36.0), "Close", true, press);
    }
    if toggle {
        play(Sfx::Click);
        SETTINGS.with(|s| s.set(!open));
        if !open {
            start_modal_anim();
        }
    }
    if !open && HOWTO.with(|h| h.get()) && rules() {
        HOWTO.with(|h| h.set(false));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scattered_rooms_keep_apart_and_their_tunnels_never_cross() {
        let cross = |a: Vec2, b: Vec2, c: Vec2, d: Vec2| {
            let s = |p: Vec2, q: Vec2, r: Vec2| (q - p).perp_dot(r - p);
            s(a, b, c) * s(a, b, d) < 0.0 && s(c, d, a) * s(c, d, b) < 0.0
        };
        for seed in 0..400 {
            let (mut run, _) = Run::new(seed, 0, 0);
            for floor in 0..6 {
                run.floor = floor;
                let links: Vec<(Vec2, Vec2)> = (0..run.map.len())
                    .flat_map(|c| (0..run.map[c].len()).map(move |r| (c, r)))
                    .flat_map(|(c, r)| run.map[c][r].next.iter().map(move |&j| (c, r, j)).collect::<Vec<_>>())
                    .map(|(c, r, j)| (node_at(&run, c, r), node_at(&run, c + 1, j)))
                    .collect();
                for (i, &(a, b)) in links.iter().enumerate() {
                    for &(c, d) in &links[i + 1..] {
                        assert!(!cross(a, b, c, d), "seed {seed} floor {floor}");
                    }
                }
                for c in 0..run.map.len() {
                    for r in 1..run.map[c].len() {
                        let (p, q) = (node_at(&run, c, r - 1), node_at(&run, c, r));
                        assert!(q.y - p.y > 80.0, "seed {seed} floor {floor}: rooms too close");
                        // labels and eyes stay on the map, clear of the status bar and the side panel
                        assert!(p.y > 150.0 && q.y < 640.0 && q.x < 830.0, "seed {seed} floor {floor}: {p} {q}");
                    }
                }
            }
        }
    }

    #[test]
    fn display_text_shares_a_few_glyph_sizes_and_body_text_stays_exact() {
        let sizes = |r: std::ops::RangeInclusive<i32>| r.map(|s| face(s as f32).1).collect::<std::collections::BTreeSet<_>>();
        // every whole size from 24 to 92: about two octaves, so about 16 rungs at most
        assert!(sizes(24..=92).len() <= 17, "{:?}", sizes(24..=92));
        // and a size halfway through an animation lands on a rung a fixed size uses too
        assert!(sizes(24..=92).contains(&face(40.37).1));
        // body text keeps its own exact size, drawn at scale 1
        for s in 10..24 {
            assert_eq!(face(s as f32).2, (s as f32 * BODY_K) / face(s as f32).1 as f32);
            assert!((face(s as f32).2 - 1.0).abs() < 0.05);
        }
    }
}
