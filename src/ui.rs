//! Immediate-mode screens: each function draws one screen and reports what was clicked.

use crate::{
    battle::{Battle, Event, Fighter, PRESSURE_TURN},
    board::{Board, W},
    data::*,
    run::*,
    save::{Save, CLEARED},
    sfx::{self, Sfx},
};
use macroquad::audio::{play_sound, PlaySoundParams, Sound};
use macroquad::rand::gen_range;
use macroquad::prelude::*;

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
const ICE: Color = rgb(0xbdbdbd);
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
    /// the effect of the brand on that rite, if it has one
    brand: Option<Effect>,
    /// false for a rite that deals no damage: it rises from the caster and settles back on them
    hits: bool,
    /// dodged: it flies past and fizzles, with no impact, shake or effects
    miss: bool,
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

/// Purely visual memory: what the screen showed last frame, so changes can be animated.
#[derive(Default)]
struct Fx {
    live: bool,
    wave: usize,
    hp: [i32; 2],
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
    /// the win or lose jingle has played for this battle
    ended: bool,
    parts: Vec<Part>,
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
    /// a mine just flipped: when it goes off
    boom: Option<f64>,
    /// a killing blow on its way: (game time it lands, side that falls)
    fell: Option<(f64, usize)>,
    /// the enemy a wave change replaced, shown until the blow that felled it lands
    fallen: Option<Fighter>,
    /// HP each bar shows, and the change it is waiting to show: (game time, HP)
    bar: [i32; 2],
    due: [Option<(f64, i32)>; 2],
    /// a felled enemy's sigil flaring out: (sprite, start)
    vanish: Option<(usize, f64)>,
    /// whose turn the screen last showed
    turn: Option<usize>,
    /// a quick zoom-in on a heavy hit: start
    punch: f64,
    /// the shop ware being looked at
    pick: Option<usize>,
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
    /// the rite a clicked gem goes to, by row in your rite list; right-click steps it down the list
    aim: Option<usize>,
    /// when the next wave's enemy arrived (game time); it walks in and you wait for it
    intro: Option<f64>,
    /// when the battle ended (real time); the result panel waits for the kill to play out
    ended_at: f64,
    /// sheet animations playing: (frames, centre, size, start, duration, tint)
    anims: Vec<(&'static [(f32, f32, f32)], Vec2, f32, f64, f32, Color)>,
    /// your board as last drawn: (which one, tiles turned over, when it last came in anew)
    board_seen: (usize, usize, f64),
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
    /// game time and this frame's step: real time, slowed for a kill and stopped for a hit-stop
    static CLOCK: std::cell::Cell<(f64, f32)> = const { std::cell::Cell::new((0.0, 0.0)) };
    /// real times until which (slow motion, hit-stop) last
    static WARP: std::cell::Cell<(f64, f64)> = const { std::cell::Cell::new((0.0, 0.0)) };
    /// how muffled the audio is right now, 0..=1
    static MUFFLE: std::cell::Cell<f32> = const { std::cell::Cell::new(0.0) };
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
    WARP.with(|w| w.set((t + secs, w.get().1)));
}

/// Freezes the game for a blink, to sell a heavy hit.
fn hitstop(secs: f64) {
    let t = get_time();
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
    // the user found full strength overkill: every shake plays at a third, and a weaker one never cuts a stronger short
    let (t, s) = (now(), strength / 3.0);
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

/// Call when the screen changes: forgets the old screen's effects and fades the new one in.
pub fn transition() {
    fx(|fx| *fx = Fx { fade: now(), ..Default::default() });
}

pub fn end_frame() {
    draw_tip();
    let a = fx(|fx| 1.0 - (now() - fx.fade) / 0.25) as f32;
    if a > 0.0 {
        draw_rectangle(-2000.0, -2000.0, 5280.0, 5000.0, fade(BG, a));
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

/// Scale for a thing identified by `key`, bouncing in from the first frame it is drawn.
fn pop(key: (usize, usize, usize)) -> f32 {
    bounce(born(key))
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
    // muffled while you hold a gem, during slow motion, and behind a panel
    let want: f32 = if real < slow {
        1.0
    } else if fx(|fx| fx.drag.is_some()) {
        0.65
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
    let jolt = if since < 0.3 {
        vec2((since * 71.0).sin(), (since * 93.0).cos()) * strength * (1.0 - since / 0.3)
    } else {
        Vec2::ZERO
    };
    let since = (now() - fx(|fx| fx.punch)) as f32;
    let z = if (0.0..0.28).contains(&since) { 1.0 + 0.035 * (1.0 - since / 0.28).powi(2) } else { 1.0 };
    CAM.with(|c| c.set(VIEW / 2.0 + jolt));
    set_camera(&Camera2D { target: VIEW / 2.0 + jolt, zoom: vec2(2.0 * k / w, 2.0 * k / h) * z, ..Default::default() });
    draw_rectangle(-2000.0, -2000.0, 5280.0, 4720.0, BG);
}

fn mouse() -> Vec2 {
    let window = vec2(screen_width(), screen_height());
    (Vec2::from(mouse_position()) - window / 2.0) / scale() + VIEW / 2.0
}

/// True while a panel (settings, retreat) is open over the screen.
fn modal() -> bool {
    SETTINGS.with(|s| s.get()) || RETREAT.with(|r| r.get()) || RULES.with(|r| r.get())
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

/// Word-wraps into width `w`; returns the y of the line after the last one.
fn wrapped(s: &str, x: f32, mut y: f32, w: f32, size: f32, c: Color) -> f32 {
    for line in wrap(s, w, size) {
        rich(&line, x, y, size, c);
        y += size * 1.25;
    }
    y
}

// Colour, used sparingly against the grey: rules keywords, and the few names that move.
const BLOOD: Color = rgb(0xff5d5d);
const GOLD: Color = rgb(0xf0c75e);

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
        "damage" | "mine" | "mines" => still(BLOOD),
        "shard" | "shards" => still(GOLD),
        "surge" | "spark" => still(rgb(0xffe066)),
        "reveal" | "reveals" | "flag" | "flagged" | "gale" => still(rgb(0x5fe0c8)),
        _ => None,
    }
}

/// Rules words that explain themselves: (heading, explanation). They float on the page, so a
/// player can tell they are worth hovering.
fn glossary(word: &str) -> Option<(&'static str, &'static str)> {
    let w = word.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase();
    Some(match w.as_str() {
        "burn" | "burns" | "burning" => ("Burn", "Loses 1 HP for each burn on it as its turn starts, straight through shield. Then the burn halves. New burn adds to what is there."),
        "detonate" => ("Detonate", "All the burn on the foe goes off at once, 3 damage for each. Then the burn is gone."),
        "weak" | "weaken" | "halved" => ("Weak", "Its rites deal half damage for the turns shown."),
        "shield" => ("Shield", "Soaks damage before HP does. Half of it wears off at the start of its owner's next turn. Burn goes straight through."),
        "armor" => ("Armor", "Every hit on it deals this much less, down to 1."),
        "evade" => ("Evade", "The first rite cast at it each turn misses: no damage, no effects, and the gems are still spent."),
        "grazes" | "graze" => ("Graze", "The first rite cast at it each turn deals half damage. Everything else the rite does still lands."),
        "surge" => ("Surge", "Each safe flip in a row adds a step, +20% rite damage, up to 5. Every rite you cast spends a step, and a mine wipes them all."),
        "overpressure" => ("Overpressure", "From turn 13 on, every rite deals 2 more each turn, so no fight can stall forever."),
        "power" => ("Power", "Damage for each gem a rite spends, before its bonus and surge."),
        "duds" => ("Duds", "Mines that will be found harmlessly this wave instead of going off."),
        "reveal" | "reveals" => ("Reveals", "Tiles you can flip this turn. Penalties never leave you fewer than 5."),
        "crystal" | "crystals" => ("Crystal", "Clear every safe tile around it to claim a gem of its board's element. A mine beside it breaks it. The corner number counts the mines touching it."),
        "rare" => ("Rare", "Changes how a turn plays. Offered less often than Common pacts; 45 shards in a shop."),
        "legendary" => ("Legendary", "Bends a core rule. Offered rarely; 65 shards in a shop."),
        _ => return None,
    })
}

thread_local!(static TIP: std::cell::Cell<Option<(&'static str, &'static str)>> = const { std::cell::Cell::new(None) });

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
    if !modal() {
        TIP.with(|c| c.set(Some(t)));
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
    let Some((head, body)) = TIP.with(|c| c.take()) else { return };
    let lines = wrap(body, 290.0, 15.0);
    let h = 44.0 + lines.len() as f32 * 19.0;
    let m = mouse();
    let r = Rect::new((m.x + 16.0).min(VIEW.x - 330.0), if m.y + 20.0 + h > VIEW.y { m.y - 12.0 - h } else { m.y + 20.0 }, 318.0, h);
    plate(r, CARD);
    text(head, r.x + 14.0, r.y + 24.0, 16.0, INK);
    for (k, line) in lines.iter().enumerate() {
        text(line, r.x + 14.0, r.y + 46.0 + k as f32 * 19.0, 15.0, MUTED);
    }
}

/// One line of text with its keywords in colour (and the line's own alpha). Words with a
/// tooltip float and explain themselves when hovered.
fn rich(s: &str, x: f32, y: f32, size: f32, c: Color) {
    let space = measure("a a", size).width - measure("aa", size).width;
    let mut at = x;
    for word in s.split(' ') {
        let w = measure(word, size).width;
        let explained = glossary(word);
        let (col, motion) = match keyword(word) {
            Some((k, m)) => (Color { a: c.a, ..k }, m),
            None => (c, None),
        };
        match motion.or(explained.map(|_| Motion::Wave)) {
            Some(m) => lively(word, at, y, size, col, m),
            None => text(word, at, y, size, col),
        }
        if let Some(t) = explained.filter(|_| Rect::new(at, y - size, w, size * 1.3).contains(mouse())) {
            tip(t);
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
    let t = now() as f32;
    for (i, (b, ch)) in s.char_indices().enumerate() {
        let (f, at) = (i as f32, x + measure(&s[..b], size).width);
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
        text(&ch.to_string(), at + dx, y + dy, size, col);
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

/// A clickable card. Returns true on the frame it is clicked.
fn card(r: Rect, on: bool) -> bool {
    let hot = on && r.contains(mouse());
    plate(r, if hot { CARD_HI } else { CARD });
    if hot {
        outline(r, INK);
    }
    if hot && clicked() {
        play(Sfx::Click);
    }
    hot && clicked()
}

/// A primary button is solid white with black text; the others are an outline. Disabled ones go grey.
fn button(r: Rect, label: &str, on: bool, primary: bool) -> bool {
    let hot = on && r.contains(mouse());
    button_face(r, label, on, primary, hot);
    if hot && clicked() {
        play(Sfx::Click);
    }
    hot && clicked()
}

/// How a button looks: shared by ordinary buttons and the ones on modal panels.
fn button_face(r: Rect, label: &str, on: bool, primary: bool, hot: bool) {
    let (fill, edge, ink) = match (on, primary, hot) {
        (false, ..) => (BG, FAINT, MUTED),
        (true, true, false) => (INK, INK, OUT),
        (true, true, true) => (rgb(0xffffff), rgb(0xffffff), OUT),
        (true, false, false) => (BG, INK, INK),
        (true, false, true) => (CARD_HI, INK, INK),
    };
    block(r, fill);
    outline(r, edge);
    let d = measure(label, 18.0);
    text(label, r.x + (r.w - d.width) / 2.0, r.y + r.h / 2.0 + 6.0, 18.0, ink);
}

/// Draws tile `idx` of the sprite sheet as a `size` square.
fn sprite(idx: usize, x: f32, y: f32, size: f32, tint: Color) {
    sprite_rect(idx, Rect::new(x, y, size, size), tint);
}

/// Draws sprite `idx` stretched over `r`: a Tiny Dungeon tile, or from `HD` up a character.
fn sprite_rect(idx: usize, r: Rect, tint: Color) {
    // sample a hair inside the tile: at fractional positions (the idle bob, window scaling,
    // anti-aliased edges) the exact border would pick up a row of the neighbouring tile
    const INSET: f32 = 0.1;
    if idx >= HD {
        let k = idx - HD;
        let source = Rect::new((k % 8) as f32 * 128.0, (k / 8) as f32 * 128.0, 128.0, 128.0);
        // the sigils are still, so characters (the first 20 and the shopkeeper) breathe:
        // a slow squash from the feet up. Map and charm icons hold still.
        let alive = k < 20 || k == 27;
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
        Element::Gale => vec![up, vec![(-2.5, 12.0), (22.5, 12.0)]],
        Element::Stone => vec![down, vec![(-2.5, 8.0), (22.5, 8.0)]],
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

/// The same sign in colour `c`, for greyed-out and highlighted ones.
fn sign(e: Element, p: Vec2, r: f32, c: Color) {
    let k = r / 9.0;
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
    let mut x = 260.0;
    for (label, value, c) in [
        ("HP", format!("{}/{}", run.hp, run.max_hp()), GOOD),
        ("Shards", run.shards.to_string(), GOLD_C),
        ("Floor", format!("{}/{}", run.floor + 1, FLOORS), INK),
        ("Pacts", run.charms.len().to_string(), ACCENT),
    ] {
        text(label, x, 43.0, 18.0, MUTED);
        let w = measure(label, 18.0).width;
        text(&value, x + w + 8.0, 43.0, 20.0, c);
        x += 190.0;
    }
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
        let panel = Rect::new(r.x.min(VIEW.x - 330.0), r.bottom() + 12.0, 310.0, 88.0);
        plate(panel, CARD);
        pact_name(id, panel.x + 14.0, panel.y + 24.0, 16.0, INK);
        rich(c.rarity.name(), panel.right() - 14.0 - measure(c.rarity.name(), 16.0).width, panel.y + 24.0, 16.0, rarity_color(c.rarity));
        wrapped(c.desc, panel.x + 14.0, panel.y + 48.0, panel.w - 28.0, 16.0, MUTED);
    }
}

/// What the title and the overworld ask the game to do next.
pub enum Go {
    Play,
    Back,
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
pub fn title() -> Option<Go> {
    let t = now() as f32;
    // the heroes waiting in a summoning circle
    rune_circle(vec2(640.0, 640.0), 280.0, 40.0, ACCENT, 0.28);
    for k in 0..4 {
        // sparks drifting up out of the circle
        let f = k as f32;
        let up = (t * 0.25 + f * 0.37).fract();
        let p = vec2(640.0 + ((f * 71.0) % 520.0 - 260.0) + (t + f).sin() * 8.0, 640.0 - up * 150.0);
        sparkle(p, 3.0, t + f, fade(mix(ACCENT, INK, 0.5), 0.4 * (1.0 - up)));
    }
    for (i, h) in HEROES.iter().enumerate() {
        let x = 640.0 + (i as f32 - (HEROES.len() - 1) as f32 / 2.0) * 135.0;
        draw_ellipse(x, 640.0, 40.0, 7.0, 0.0, fade(OUT, 0.6));
        sprite(h.sprite, x - 48.0, 548.0 + (t * 2.0 + i as f32 * 2.1).sin() * 3.0, 96.0, WHITE);
    }
    // the name drops in once, as the game opens; coming back from the map it is simply there
    let rise = if fx(|fx| fx.fade) > 0.0 { 0.0 } else { 1.0 - ((now() / 0.6) as f32).min(1.0) };
    lively_centered("MINEBOUND", 640.0, 140.0 - rise * 30.0, 92.0, INK, Motion::Wave);
    centered("Sweep your boards for power. Spend it before theirs spends you.", 640.0, 184.0, 20.0, MUTED);
    let mut go = None;
    if button(Rect::new(540.0, 222.0, 200.0, 48.0), "Play", true, true) {
        go = Some(Go::Play);
    }
    if button(Rect::new(540.0, 284.0, 200.0, 44.0), "Settings", true, false) {
        SETTINGS.with(|s| s.set(true));
    }
    if button(Rect::new(540.0, 342.0, 200.0, 44.0), "Quit", true, false) {
        go = Some(Go::Back);
    }
    for (k, line) in [
        // the rules, as the Below would say them
        "Bare the ground around a crystal and it answers to you. Walk unbroken, and the surge rises.",
        "Feed the sockets and the rite is spoken. What you cannot spend, throw at their face.",
        "The first stone never bites. The rest remember your name.",
    ]
    .iter()
    .enumerate()
    {
        rich_centered(line, 640.0, 424.0 + k as f32 * 22.0, 16.0, fade(MUTED, 0.8));
    }
    go
}

/// Each descent's place on the map of The Below.
const PLACES: [Vec2; 6] = [vec2(170.0, 560.0), vec2(330.0, 410.0), vec2(520.0, 520.0), vec2(610.0, 300.0), vec2(790.0, 430.0), vec2(820.0, 180.0)];
/// The land is a grid of tiles: (left, top), tile pitch, columns, rows.
const LAND: (f32, f32, f32, usize, usize) = (40.0, 110.0, 22.0, 39, 27);

/// A fixed pseudo-random number in 0..1 for tile (c, r), so the land looks the same every frame.
fn noise(c: usize, r: usize) -> f32 {
    let s = (c as f64 * 12.9898 + r as f64 * 78.233).sin() * 43758.5453;
    (s - s.floor()) as f32
}

/// What each tile of The Below is: 0 open air, 1 unexplored land, 2 land explored around an open place.
/// The land is a few soft blobs round the places, minus three chasms, its edge roughened per tile.
fn land(unlocked: usize) -> Vec<Vec<u8>> {
    let mut blobs: Vec<(f32, f32, f32)> = PLACES.iter().map(|p| (p.x, p.y, 62.0)).collect();
    blobs.extend([(250.0, 500.0, 55.0), (430.0, 470.0, 58.0), (470.0, 330.0, 48.0), (700.0, 330.0, 52.0), (690.0, 520.0, 44.0), (690.0, 200.0, 46.0), (100.0, 470.0, 40.0), (880.0, 300.0, 40.0), (250.0, 300.0, 36.0), (560.0, 640.0, 34.0)]);
    let chasms = [(560.0, 400.0, 58.0), (230.0, 640.0, 40.0), (880.0, 560.0, 46.0)];
    let (x0, y0, cell, cols, rows) = LAND;
    let pull = |set: &[(f32, f32, f32)], p: Vec2| set.iter().map(|&(x, y, r)| r * r / ((p.x - x).powi(2) + (p.y - y).powi(2) + 1.0)).sum::<f32>();
    (0..rows)
        .map(|r| {
            (0..cols)
                .map(|c| {
                    let p = vec2(x0 + c as f32 * cell + cell / 2.0, y0 + r as f32 * cell + cell / 2.0);
                    let field = pull(&blobs, p) - pull(&chasms, p) + (noise(c, r) - 0.5) * 0.5;
                    if field <= 0.9 {
                        0
                    } else if PLACES[..unlocked].iter().any(|q| q.distance(p) < 105.0) {
                        2
                    } else {
                        1
                    }
                })
                .collect()
        })
        .collect()
}

/// The world map, The Below: pick an open descent and a bearer, then descend.
/// `pick` is (descent, hero) and lasts between visits.
pub fn world(save: &Save, pick: &mut (usize, usize)) -> Option<Go> {
    let mut go = None;
    text("The Below", 48.0, 70.0, 44.0, INK);
    text(&format!("{} of {} descents open", save.unlocked, EXPEDITIONS.len()), 50.0, 96.0, 15.0, MUTED);
    let tx = 48.0 + measure("The Below", 44.0).width + 28.0;
    if button(Rect::new(tx, 40.0, 110.0, 36.0), "< Title", true, false) {
        go = Some(Go::Back);
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
    let (x0, y0, cell, cols, rows) = LAND;
    let at = |c: usize, r: usize| kind.get(r).and_then(|row| row.get(c)).copied().unwrap_or(0);
    let mine = |c: usize, r: usize| at(c, r) == 1 && noise(c + 50, r + 50) < 0.22;
    // all the tiles first, then all the numbers: each switch between shapes and text is a new GPU
    // draw call with buffers of its own, and hundreds of them made the driver take ~650 MB
    let mut numbers = vec![];
    for r in 0..rows {
        for c in 0..cols {
            let t = Rect::new(x0 + c as f32 * cell, y0 + r as f32 * cell, cell - 3.0, cell - 3.0);
            match at(c, r) {
                1 => block(t, if noise(c + 9, r + 3) < 0.5 { rgb(0x3a3a3a) } else { rgb(0x333333) }),
                2 => {
                    draw_rectangle_lines(t.x, t.y, t.w, t.h, 1.0, rgb(0x262626));
                    let n = (0..9).filter(|k| mine((c + k % 3).wrapping_sub(1), (r + k / 3).wrapping_sub(1))).count();
                    if n > 0 {
                        numbers.push((n, t));
                    }
                }
                _ => {}
            }
        }
    }
    for (n, t) in numbers {
        centered(&n.to_string(), t.center().x, t.y + 15.0, 14.0, if n > 1 { INK } else { MUTED });
    }
    // the route between places, dotted
    for w in PLACES.windows(2) {
        let (a, b) = (w[0], w[1]);
        let n = (a.distance(b) / 9.0) as usize;
        for k in 0..n {
            let p = a.lerp(b, k as f32 / n as f32);
            draw_circle(p.x, p.y, 1.0, INK);
        }
    }

    let mut shown = pick.0;
    for (i, x) in EXPEDITIONS.iter().enumerate() {
        let p = PLACES[i];
        let open = i < save.unlocked;
        let hot = (mouse() - p).length() < 34.0 && !modal();
        if hot {
            shown = i;
        }
        let chosen = i == pick.0;
        let edge = if open { INK } else { MUTED };
        let r = if hot && open { 35.0 } else { 32.0 };
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
        }
    }

    // the chosen (or pointed at) descent, and who carries it down
    let x = &EXPEDITIONS[shown];
    let open = shown < save.unlocked;
    draw_line(950.0, 48.0, 950.0, 672.0, 1.0, FAINT);
    let (lx, w) = (982.0, 256.0);
    label(&format!("DESCENT {}", ["I", "II", "III", "IV", "V", "VI"][shown]), lx, 72.0);
    text(if open { x.name } else { "Unknown" }, lx, 112.0, 36.0, if open { INK } else { MUTED });
    let mut y = wrapped(x.text, lx, 140.0, w, 15.0, MUTED) + 14.0;
    label("ITS LAW", lx, y);
    for rule in x.rules {
        y = wrapped(rule, lx, y + 20.0, w, 15.0, INK) - 6.0;
    }
    y += 22.0;
    if !open {
        // no-break spaces keep a place name whole, so "Ember" or "Mines" in it is not read as a rules word
        let prev = EXPEDITIONS[shown - 1].name.replace(' ', "\u{a0}");
        y = wrapped(&format!("Win {prev} to open the way here."), lx, y, w, 15.0, INK) + 14.0;
    }
    // who waits at the bottom of each floor; a sealed descent keeps them to one line, so its longer law still fits
    label("LORDS BELOW", lx, y);
    y += 8.0;
    if !open {
        wrapped("Unseen until the way opens.", lx, y + 20.0, w, 15.0, MUTED);
    }
    for pair in BOSSES.iter().filter(|_| open) {
        let names: Vec<&str> = pair.iter().map(|d| d.name).collect();
        y = wrapped(&names.join(" or "), lx, y + 20.0, w, 15.0, INK) - 6.0;
    }

    // the bearer: one sigil per hero; the chosen one is outlined, with its numbers below
    let by = 466.0;
    label("BEARER", lx, by);
    for (i, h) in HEROES.iter().enumerate() {
        let r = Rect::new(lx + i as f32 * 60.0, by + 12.0, 50.0, 50.0);
        let on = i == pick.1;
        let hot = r.contains(mouse()) && !modal();
        if hot {
            block(r, CARD_HI);
        }
        outline(r, if on { INK } else { FAINT });
        sprite(h.sprite, r.x + 5.0, r.y + 5.0, 40.0, if on || hot { WHITE } else { fade(WHITE, 0.45) });
        if hot && clicked() {
            play(Sfx::Click);
            pick.1 = i;
        }
    }
    let h = &HEROES[pick.1];
    text(h.name, lx, by + 94.0, 24.0, INK);
    let best = match save.best[pick.0][pick.1] {
        0 => "new here".to_string(),
        b if b >= CLEARED => "cleared".to_string(),
        b => format!("best: floor {b}"),
    };
    text(&format!("{} HP · {} power · {best}", h.hp, h.power), lx, by + 114.0, 14.0, MUTED);
    wrapped(h.passive_desc, lx, by + 138.0, w, 14.0, INK);
    for (k, &e) in h.elements.iter().enumerate() {
        gem(e, vec2(lx + 200.0 + k as f32 * 20.0, by + 87.0), 7.0);
    }
    let descend = button(Rect::new(lx, 662.0, w, 46.0), "Descend", pick.0 < save.unlocked, true);
    if descend || (is_key_pressed(KeyCode::Enter) && !modal()) {
        go = Some(Go::Play);
    }
    go
}

/// a board tile, and the step from one tile to the next
const TILE: f32 = 54.0;
const STEP: f32 = 56.0;
/// your board's top-left corner, in the middle of the screen
const BOARD: Vec2 = vec2(472.0, 330.0);
/// where each fighter's sigil is centred, and how big: you on the left, the enemy alone at the top
const STAND: [Vec2; 2] = [vec2(104.0, 348.0), vec2(640.0, 82.0)];
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

/// A thin life bar: white for what is left, grey for the chunk just lost.
fn life(r: Rect, frac: f32, ghost: f32) {
    block(r, OUT);
    for (f, c) in [(ghost, MUTED), (frac, INK)] {
        if f > 0.0 {
            block(Rect::new(r.x, r.y, r.w * f.min(1.0), r.h), c);
        }
    }
    draw_rectangle_lines(r.x, r.y, r.w, r.h, 1.0, INK);
}

/// Power and whatever else a fighter has going: shield, armor, burn and so on.
fn conditions(f: &Fighter) -> String {
    let mut st = format!("Power {}", f.power);
    let dodge = if f.mods.graze > 0 { "Graze" } else { "Evade" };
    for (label, v) in [("Shield", f.shield), ("Armor", f.mods.armor), (dodge, f.evades_left), ("Burn", f.burn), ("Weak", f.weak), ("Duds", f.duds)] {
        if v > 0 {
            st += &format!(" · {label} {v}");
        }
    }
    st
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

/// A mine: a round body with eight spikes.
fn mine_mark(c: Vec2, r: f32, col: Color) {
    for k in 0..4 {
        let d = Vec2::from_angle(k as f32 * std::f32::consts::FRAC_PI_4) * r;
        draw_line(c.x - d.x, c.y - d.y, c.x + d.x, c.y + d.y, 2.0, col);
    }
    draw_circle(c.x, c.y, r * 0.6, col);
}

/// A jagged bolt from `a` to `z`, redrawn with new kinks every frame.
fn lightning(a: Vec2, z: Vec2, width: f32, color: Color) {
    let mut prev = a;
    for k in 1..=8 {
        let q = k as f32 / 8.0;
        let next = if k == 8 { z } else { a + (z - a) * q + vec2(gen_range(-8.0, 8.0), gen_range(-18.0, 18.0)) };
        draw_line(prev.x, prev.y, next.x, next.y, width, color);
        draw_line(prev.x, prev.y, next.x, next.y, width * 0.35, INK);
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
        let (e1, e2) = (c.needs[0], c.needs[1]);
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
        // level 3: a pillar of light comes down on the target, brightest as the first bolt hits
        let first_hit = lead + 1.0;
        if lvl >= 3 && p < first_hit + 1.2 {
            let alpha = if p < first_hit { p / first_hit } else { 1.0 - (p - first_hit) / 1.2 };
            let w = 18.0 + 40.0 * alpha;
            let at = if sh.hits { z } else { a };
            draw_rectangle(at.x - w / 2.0, -60.0, w, at.y + 110.0, fade(mix(c1, INK, 0.3), alpha * 0.55));
            draw_rectangle(at.x - w / 6.0, -60.0, w / 3.0, at.y + 110.0, fade(INK, alpha * 0.8));
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
                match e1 {
                    Element::Spark => lightning(a, z, 6.0 + 2.0 * lvl as f32, c1),
                    Element::Gale => {
                        draw_poly(head.x, head.y, 3, 26.0, now as f32 * 1400.0, fade(c1, 0.8));
                        draw_poly(head.x, head.y, 3, 16.0, -(now as f32) * 1400.0, fade(c2, 0.9));
                        for t in 1..5 {
                            let back = along((q - t as f32 * 0.06).max(0.0));
                            draw_line(back.x, back.y - 10.0, back.x + 18.0, back.y - 10.0, 3.0, fade(c1, 0.6 - t as f32 * 0.12));
                        }
                    }
                    Element::Stone => {
                        draw_poly(head.x, head.y, 4, 30.0, now as f32 * 500.0, mix(c1, OUT, 0.35));
                        draw_poly(head.x, head.y, 4, 22.0, now as f32 * 500.0, c1);
                        if gen_range(0.0, 1.0) < 0.5 {
                            rise(head, 1, 8.0, mix(c1, OUT, 0.3));
                        }
                    }
                    _ => {
                        // a comet: fading copies trail behind the head
                        for t in (1..6).rev() {
                            let back = q - t as f32 * 0.07;
                            if back > 0.0 {
                                gem(e1, along(back), 22.0 - t as f32 * 3.0);
                            }
                        }
                        let n = if e1 == Element::Ember { 3 } else { 1 };
                        for _ in 0..n {
                            let (vel, grav, color) = if e1 == Element::Ember {
                                (vec2(gen_range(-30.0, 30.0), gen_range(-120.0, -40.0)), -60.0, mix(GOLD_C, BAD, gen_range(0.0, 1.0)))
                            } else {
                                (vec2(gen_range(-40.0, 40.0), gen_range(-60.0, 0.0)), 600.0, mix(c1, INK, gen_range(0.0, 0.6)))
                            };
                            fx(|fx| fx.parts.push(Part { pos: head, vel, born: now, life: gen_range(0.3, 0.6), size: gen_range(4.0, 8.0), color, grav, home: None }));
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
                burst(z, 50 + 20 * lvl, 420.0 + 50.0 * lvl as f32, 12.0, c1);
                burst(z, 26, 280.0, 8.0, c2);
                burst(z, 18, 320.0, 9.0, INK);
                quake(if e1 == Element::Stone { 22.0 } else { 12.0 + 4.0 * lvl as f32 });
                flash(c1);
                play(Sfx::Strike);
                hitstop(if k + 1 == lvl { 0.08 + 0.03 * lvl as f64 } else { 0.05 });
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
                        for ring in 0..lvl {
                            let r = 40.0 + 16.0 * (u * 4.0).min(1.0) + ring as f32 * 10.0;
                            draw_poly_lines(a.x, a.y, 6, r, now as f32 * 25.0 + ring as f32 * 30.0, 4.0, fade(ICE, fadeout));
                        }
                        draw_poly(a.x, a.y, 6, 52.0, now as f32 * 25.0, fade(ICE, 0.15 * fadeout));
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

/// The visuals of a rite's effects (and its brand's), as its first bolt lands.
fn rite_effects(sh: &Shot, c: &Combo, a: Vec2, z: Vec2, c2: Color) {
    for e in c.effects.iter().chain(&sh.brand) {
        match e {
            Effect::Burn(_) => {
                rise(z, 30, 30.0, mix(GOLD_C, BAD, 0.5));
                anim(FX_FLAME, z - vec2(0.0, 8.0), 110.0, 0.7, el_color(Element::Ember));
            }
            Effect::Heal(_) => {
                rise(a, 22, 30.0, GOOD);
                anim(FX_HEAL, a, 120.0, 0.5, GOOD);
            }
            Effect::Drain(_) => stream(z, a, 16, mix(c2, INK, 0.3)),
            Effect::Pierce => burst(z, 28, 440.0, 8.0, ICE),
            Effect::Shatter(_) => burst(z, 24, 320.0, 7.0, INK),
            Effect::Bury(_) => burst(z, 16, 160.0, 6.0, BAD),
            Effect::Detonate => burst(z, 36, 420.0, 9.0, rgb(0xff8a4c)),
            Effect::Reveal(_) => burst(a, 12, 140.0, 5.0, rgb(0x5fe0c8)),
            _ => {}
        }
    }
}

/// Draws the battle and handles input. Returns true once it is over and the player clicks Continue.
/// Game seconds a new wave takes to arrive; your input waits for it.
const WAVE_IN: f64 = 1.4;
/// Seconds a turn's banner stays up.
const TURN_BANNER: f32 = 0.95;
/// Seconds a rite's card is up before its bolt leaves (longer for upgraded rites). The card has the
/// stage to itself first, then fades as the caster throws, so the eye follows the bolt.
fn card_lead(lvl: usize) -> f32 {
    0.5 + 0.12 * lvl.saturating_sub(1) as f32
}
/// Seconds a rite's card stays up in all.
fn card_dur(lvl: usize) -> f32 {
    card_lead(lvl) + 0.3
}
/// Seconds of quiet after a bolt lands before the next action starts.
const SETTLE: f64 = 0.4;
/// Seconds from a mine being turned over to it going off.
const BOOM: f64 = 0.28;

/// When the action that starts now actually gets the stage: once whatever is playing has finished.
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

    // compare with last frame: HP changes become floating numbers and a shake, new stacks bounce in,
    // and every skill that landed becomes a shot across the arena
    let now = now();
    // crystals claimed this frame fly to the stacks once the stacks are laid out below
    let mut claims = vec![];
    for e in std::mem::take(&mut b.events) {
        match e {
            Event::Flip => play(Sfx::Flip),
            Event::Claim { side, element, tiles } => {
                play(Sfx::Claim);
                claims.push((side, element, tiles));
            }
            Event::Mine => {
                // the tile turns over to show it, the fuse fizzes for a beat, then it goes off
                play(Sfx::Mine);
                fx(|fx| {
                    fx.boom = Some(now + BOOM);
                    fx.busy = fx.busy.max(now + BOOM + 0.6);
                });
            }
            Event::Cast { from, name, element, miss, skill } => {
                // in turn: the rite's card sweeps in and holds, the caster winds up, the bolt flies
                let start = queued(now);
                let lvl = skill.1;
                fx(|fx| {
                    let t0 = start + card_lead(lvl) as f64;
                    let shot = Shot { from, t0, skill, brand: b.f[from].brand(&COMBOS[skill.0]).map(|x| x.effect), hits: b.hits(from, &COMBOS[skill.0]), miss };
                    fx.shots.push(shot);
                    fx.banner.push((name, el_color(element), from, start, lvl));
                    if miss {
                        fx.floats.push((STAND[1 - from] - vec2(0.0, 30.0), "MISS".into(), MUTED, lands_at(&shot), 40.0));
                    }
                    fx.busy = lands_at(&shot) + SETTLE;
                    // its voice when it is thrown: by first element, and a charge-up once upgraded
                    let voice = match (miss, from) {
                        (true, _) => Sfx::Miss,
                        (_, 1) => Sfx::Hurt,
                        _ => Sfx::Skill,
                    };
                    fx.later.push((t0 + (charge(lvl) * SHOT_TIME) as f64, voice));
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
            Event::Proc { side: 0, hook } => {
                for (k, &id) in charms.iter().enumerate() {
                    if CHARMS[id].hook == hook {
                        fx(|fx| fx.charm_t.insert(id, now));
                        burst(charm_slot(k, charms.len()).center(), 12, 140.0, 5.0, GOLD_C);
                    }
                }
            }
            Event::Proc { .. } => {}
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
    // sounds and a mine's blast whose moment has come
    let (due, boom) = fx(|fx| {
        let (due, wait): (Vec<_>, Vec<_>) = fx.later.drain(..).partition(|x| x.0 <= now);
        fx.later = wait;
        let boom = fx.boom.filter(|&t| t <= now);
        if boom.is_some() {
            fx.boom = None;
        }
        (due, boom)
    });
    for (_, s) in due {
        play(s);
    }
    if boom.is_some() {
        play(Sfx::Boom);
        quake(20.0);
        flash(BAD);
        hitstop(0.09);
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
    let (shake, ghost, bar, stack_t) = fx(|fx| {
        // anything that changed shows once every bolt in flight, and a mine about to blow, has landed
        let at = fx.shots.iter().map(lands_at).chain(fx.boom).fold(now, f64::max);
        if fx.live && fx.wave != b.wave {
            fx.pops.clear(); // fresh boards
            // the old enemy stays up until the blow that felled it lands
            fx.fallen = fallen_now;
            fx.fell = Some((at, 1));
            fx.busy = fx.busy.max(at + WAVE_IN);
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
                fx.ghost[s] = hp as f32;
                fx.bar[s] = hp;
            }
            // a wave's killing blow took whatever the fallen enemy had left
            let d = if next_wave { -fx.hp[s] } else if !fx.live { 0 } else { hp - fx.hp[s] };
            if d != 0 {
                // the number and the bar wait until the skill that caused it visibly lands
                let big = d <= -15;
                let (label, colour) = match d {
                    _ if big => (format!("{d}!"), GOLD_C),
                    _ if d < 0 => (format!("{d}"), BAD),
                    _ => (format!("{d:+}"), GOOD),
                };
                fx.floats.push((STAND[s] - vec2(0.0, 30.0), label, colour, at, 38.0 + d.abs().min(40) as f32 * 1.1));
                if d < 0 {
                    fx.hit[s] = at;
                }
                fx.due[s] = Some((at, if next_wave { 0 } else { hp }));
            }
            if fx.live && !next_wave {
                for k in fx.stacks[s]..n.min(16) {
                    fx.stack_t[s][k] = now;
                }
            }
            fx.hp[s] = hp;
            fx.stacks[s] = n;
            if let Some((_, v)) = fx.due[s].filter(|d| now >= d.0) {
                fx.bar[s] = v;
                fx.due[s] = None;
            }
            fx.ghost[s] += (fx.bar[s] as f32 - fx.ghost[s]) * (dt() * 3.0).min(1.0);
            let since = (now - fx.hit[s]) as f32;
            if (0.0..0.45).contains(&since) {
                shake[s] = (since * 75.0).sin() * 7.0 * (1.0 - since / 0.45);
            }
        }
        // the killing blow lands
        if let Some((_, s)) = fx.fell.filter(|f| now >= f.0) {
            fx.fell = None;
            felled = Some(s);
        }
        fx.live = true;
        fx.wave = b.wave;
        (shake, fx.ghost, fx.bar, fx.stack_t)
    });
    match felled {
        Some(1) => {
            // the enemy bursts apart in slow motion, the sound going dull around it
            play(Sfx::Kill);
            slowmo(if over { 1.8 } else { 1.2 });
            hitstop(0.14);
            punch();
            quake(26.0);
            flash(INK);
            burst(STAND[1], 140, 600.0, 14.0, INK);
            burst(STAND[1], 90, 440.0, 11.0, BAD);
            rise(STAND[1], 50, 50.0, GOLD_C);
            let sprite = fx(|fx| fx.fallen.take().map_or(b.f[1].sprite, |f| f.sprite));
            fx(|fx| fx.vanish = Some((sprite, now)));
            if over {
                play(Sfx::Win);
                fx(|fx| fx.ended_at = get_time());
            } else {
                // the new enemy's bar takes over; it is announced once the dust settles, then walks in
                let wave = ["WAVE 2", "WAVE 3", "WAVE 4"][b.wave.saturating_sub(2).min(2)];
                fx(|fx| {
                    let hp = b.f[1].hp.max(0);
                    (fx.bar[1], fx.ghost[1], fx.due[1]) = (hp, hp as f32, None);
                    fx.intro = Some(now);
                    fx.banner.push((wave, BAD, 1, now + 0.45, 1));
                    fx.busy = fx.busy.max(now + WAVE_IN);
                });
            }
        }
        Some(_) => {
            // you fall: the same slow, dull moment, in red
            play(Sfx::Lose);
            slowmo(1.6);
            hitstop(0.12);
            quake(22.0);
            flash(BAD);
            burst(STAND[0], 110, 480.0, 12.0, BAD);
            burst(STAND[0], 50, 320.0, 9.0, INK);
            fx(|fx| fx.ended_at = get_time());
        }
        None => {}
    }
    let fallen = fx(|fx| fx.fallen.clone());
    let (mut reveal, mut flag, mut swap, mut cast) = (None, None, None, None);

    // where the run stands, top left, above your pacts
    label(&format!("FLOOR {}  ·  WAVE {}/{}  ·  TURN {}", floor + 1, b.wave, b.waves, b.turn), 56.0, 40.0);
    let note = if b.pressure() > 0 { format!("Overpressure: every rite +{}", b.pressure()) } else { format!("Overpressure after turn {PRESSURE_TURN}") };
    rich(&note, 56.0, 116.0, 15.0, if b.pressure() > 0 { INK } else { MUTED });

    // the enemy, alone at the top: name, life, and what it would throw if it fired now
    {
        let foe = fallen.as_ref().unwrap_or(&b.f[1]);
        let ex = FOE.x + shake[1];
        if BOSSES.iter().flatten().any(|d| d.name == foe.name) {
            lively_centered(&foe.name, 640.0 + shake[1], 186.0, 40.0, BLOOD, Motion::Tremble);
        } else {
            stamped(&foe.name, 640.0 + shake[1], 186.0, 40.0, INK);
        }
        rich_centered(&foe.desc, 640.0 + shake[1], 210.0, 15.0, MUTED);
        label("HP", ex, 236.0);
        right(&format!("{} / {}", bar[1], foe.max_hp), ex + FOE.w, 236.0, 16.0, INK);
        life(Rect::new(ex, 244.0, FOE.w, 8.0), bar[1] as f32 / foe.max_hp as f32, ghost[1] / foe.max_hp as f32);
        rich(&conditions(foe), ex, 276.0, 15.0, INK);
        let (what, signs, dmg) = match b.best_combo(1, &foe.stacks) {
            Some(c) => (c.name, foe.cost(c), b.hit_damage(1, c)),
            None => ("nothing yet", vec![], 0),
        };
        let mut x = ex + FOE.w;
        if dmg > 0 {
            let s = format!("{dmg} dmg");
            right(&s, x, 294.0, 16.0, INK);
            x -= measure(&s, 16.0).width + 12.0;
        }
        for &e in signs.iter().rev() {
            gem(e, vec2(x - 8.0, 289.0), 8.0);
            x -= 20.0;
        }
        right(what, x - 4.0, 294.0, 16.0, INK);
        right("NEXT", x - 16.0 - measure(what, 16.0).width, 294.0, 15.0, MUTED);

        // its board, shrunk to a preview, and its stacks
        let fb = foe.board();
        label("ITS BOARD", PEEK.x, PEEK.y - 12.0);
        for (k, bd) in foe.boards.iter().enumerate() {
            let at = vec2(PEEK.x + 142.0 - (foe.boards.len() - 1 - k) as f32 * 20.0, PEEK.y - 17.0);
            sign(bd.element, at, 7.0, if k == foe.cur { INK } else { FAINT });
        }
        for (i, t) in fb.tiles.iter().enumerate() {
            let r = Rect::new(PEEK.x + (i % W) as f32 * 25.0, PEEK.y + (i / W) as f32 * 25.0, 23.0, 23.0);
            let key = (1, foe.cur, i);
            let seen = fx(|fx| fx.pops.contains_key(&key));
            if t.revealed && t.mine && !seen {
                burst(r.center(), 20, 180.0, 6.0, BAD);
            }
            if t.claimed || t.revealed {
                born(key);
            } else {
                unpop(key);
            }
            if t.rock {
                block(r, rgb(0x161616));
            } else if t.crystal && !t.claimed && !t.broken {
                draw_rectangle_lines(r.x, r.y, r.w, r.h, 1.0, INK);
                sign(fb.element, r.center(), 6.0, INK);
            } else if t.crystal {
                draw_rectangle_lines(r.x, r.y, r.w, r.h, 1.0, FAINT);
            } else if !t.revealed {
                block(r, rgb(0x5a5a5a));
            } else if t.mine {
                draw_rectangle_lines(r.x, r.y, r.w, r.h, 1.0, INK);
                mine_mark(r.center(), 6.0, INK);
            } else {
                draw_rectangle_lines(r.x, r.y, r.w, r.h, 1.0, rgb(0x2a2a2a));
                // its numbers too, so you can read how its sweep is going
                let n = fb.adjacent(i);
                if n > 0 {
                    centered(&n.to_string(), r.center().x, r.center().y + 5.0, 14.0, MUTED);
                }
            }
        }
        rich(&format!("{} reveals · {} mines", foe.reveals_left, fb.mines), PEEK.x, PEEK.y + 170.0, 14.0, MUTED);
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
    // a trait that wraps past two lines lifts the name with it, so it never runs into the HP bar
    let lift = wrap(&me.desc, 330.0, 15.0).len().saturating_sub(2) as f32 * 19.0;
    text(&me.name, hx, 434.0 - lift, 34.0, INK);
    wrapped(&me.desc, hx, 458.0 - lift, 330.0, 15.0, MUTED);
    label("HP", hx, 506.0);
    right(&format!("{} / {}", bar[0], me.max_hp), hx + 330.0, 506.0, 16.0, INK);
    life(Rect::new(hx, 514.0, 330.0, 8.0), bar[0] as f32 / me.max_hp as f32, ghost[0] / me.max_hp as f32);
    rich(&conditions(me), hx, 546.0, 15.0, INK);
    let held = by_element(&me.elements, free.iter().map(|&k| me.stacks[k]));
    let chip_step = 78.0f32.min(330.0 / held.len().max(1) as f32);
    let chip = |j: usize| Rect::new(hx + j as f32 * chip_step, 596.0, chip_step - 10.0, 40.0);
    let chip_of = |e: Element| held.iter().position(|x| x.0 == e).map_or(vec2(hx, 616.0), |j| chip(j).center());
    // just claimed: each crystal shatters and its pieces fly to the stacks, even when its board
    // has already been dealt anew under it
    for (side, element, tiles) in &claims {
        for &i in tiles {
            let (at, to, n) = if *side == 0 {
                let at = BOARD + vec2((i % W) as f32 * STEP, (i / W) as f32 * STEP) + vec2(TILE, TILE) / 2.0;
                (at, chip_of(*element), 16)
            } else {
                (PEEK + vec2((i % W) as f32 * 25.0, (i / W) as f32 * 25.0) + vec2(11.5, 11.5), vec2(PEEK.x + 20.0, 266.0), 10)
            };
            stream(at, to, n, INK);
            if *side == 0 {
                burst(at, 10, 150.0, 6.0, INK);
            }
        }
    }

    // your board, in the middle, with whose turn it is and the surge above it
    let board = me.board();
    let turn_label = if over { "" } else if b.active == 0 { "YOUR TURN" } else { "ITS TURN" };
    text(turn_label, BOARD.x, 316.0, 15.0, if b.active == 0 { INK } else { MUTED });
    let (surge, cap) = (b.f[b.active].surge, b.surge_cap(b.active));
    let times = format!("x{:.1}", 1.0 + (surge * b.surge_step(b.active)) as f32 / 100.0);
    let mut x = BOARD.x + 6.0 * STEP - 2.0;
    right(&times, x, 316.0, 15.0, if surge > 0 { INK } else { MUTED });
    x -= measure(&times, 15.0).width + 14.0;
    for k in (0..cap).rev() {
        pip(vec2(x - 6.0, 311.0), 6.0, k < surge);
        x -= 17.0;
    }
    right("SURGE", x - 2.0, 316.0, 15.0, MUTED);
    // a board dealt anew (fewer tiles turned than a moment ago) or another one put up drops in, tile by tile
    let shown = board.tiles.iter().filter(|t| t.revealed).count();
    let (came, redealt) = fx(|fx| {
        let (k, n, _) = fx.board_seen;
        let (new, redealt) = (k != me.cur || shown < n, k == me.cur && shown < n);
        fx.board_seen = (me.cur, shown, if new { now } else { fx.board_seen.2 });
        (fx.board_seen.2, redealt)
    });
    if redealt {
        play(Sfx::Whoosh);
    }
    for (i, t) in board.tiles.iter().enumerate() {
        let r = Rect::new(BOARD.x + (i % W) as f32 * STEP, BOARD.y + (i / W) as f32 * STEP, TILE, TILE);
        // corner to corner, each tile a beat after the one before; it falls a little as it grows
        let p = (((now - came - ((i % W) + (i / W)) as f64 * 0.025) / 0.2) as f32).clamp(0.0, 1.0);
        let k = if came > 0.0 { 1.0 - (1.0 - p).powi(3) } else { 1.0 };
        if k < 0.05 {
            continue;
        }
        let r = scaled(Rect::new(r.x, r.y - (1.0 - k) * 18.0, r.w, r.h), k);
        let mid = r.center();
        // tiles around a crystal that can still be claimed stand out brighter
        let near = Board::neighbors(i).any(|c| board.live(c));
        let up = if near { INK } else { rgb(0xb0b0b0) };
        let key = (0, me.cur, i);
        if t.rock {
            rock(r);
        } else if t.crystal {
            if t.claimed {
                let k = pop(key);
                outline(r, FAINT);
                sign(board.element, mid, 12.0 * k, MUTED);
            } else if t.broken {
                outline(r, FAINT);
                for (a, b) in [((-14.0, -12.0), (2.0, 0.0)), ((2.0, 0.0), (14.0, -6.0)), ((2.0, 0.0), (-4.0, 14.0))] {
                    draw_line(mid.x + a.0, mid.y + a.1, mid.x + b.0, mid.y + b.1, 2.0, MUTED);
                }
            } else {
                unpop(key);
                outline(r, INK);
                // each crystal bobs on its own beat; the number is how many tiles are left around it
                gem(board.element, mid - vec2(0.0, 7.0 + (now as f32 * 2.6 + i as f32 * 1.7).sin() * 1.5), 13.0);
                centered(&board.left_around(i).to_string(), mid.x, r.y + 48.0, 15.0, INK);
                // and, in its corner, the mines touching it: a clue like any number
                let n = board.adjacent(i);
                if n > 0 {
                    mine_mark(vec2(r.x + 9.0, r.y + 9.0), 4.0, MUTED);
                    text(&n.to_string(), r.x + 15.0, r.y + 14.0, 14.0, INK);
                }
            }
        } else if !t.revealed {
            unpop(key);
            let hot = mine && r.contains(mouse());
            let lift = if hot { 3.0 } else { 0.0 };
            block(Rect::new(r.x, r.y - lift, TILE, TILE), if hot { WHITE } else { up });
            if t.flagged {
                // a flag: a pole on a little base, a pennant flying from its top
                let y = mid.y - lift;
                draw_rectangle(mid.x - 4.0, y - 14.0, 3.0, 26.0, OUT);
                draw_rectangle(mid.x - 10.0, y + 10.0, 15.0, 4.0, OUT);
                draw_triangle(vec2(mid.x - 1.0, y - 14.0), vec2(mid.x - 1.0, y - 2.0), vec2(mid.x + 12.0, y - 8.0), OUT);
            }
            if hot && clicked() && !t.flagged {
                reveal = Some(i);
            }
            if hot && is_mouse_button_pressed(MouseButton::Right) {
                flag = Some(i);
            }
        } else {
            // the tile turns over: its back narrows to an edge, then its face widens out
            let t0 = born(key);
            let el = now - t0;
            // a mine shows its fuse for a beat, then blows (in step with the shake and sound)
            if t.mine && dt() > 0.0 && el >= BOOM && el - (dt() as f64) < BOOM {
                burst(mid, 70, 460.0, 13.0, BAD);
                burst(mid, 34, 300.0, 9.0, INK);
                anim(fx_impact(Element::Ember), mid, 170.0, 0.45, mix(BAD, WHITE, 0.2));
            }
            let turn = ((el / 0.16) as f32).clamp(0.0, 1.0);
            if turn < 0.5 {
                let w = TILE * (1.0 - turn * 2.0);
                block(Rect::new(mid.x - w / 2.0, r.y, w, TILE), up);
            } else {
                let q = scaled(r, bounce(t0 + 0.08));
                let w = q.w * (turn * 2.0 - 1.0);
                let q = Rect::new(mid.x - w / 2.0, q.y, w, q.h);
                if t.mine {
                    block(q, OUT);
                }
                if w > 2.0 {
                    outline(q, if t.mine { INK } else { FAINT });
                }
                let n = board.adjacent(i);
                if turn >= 0.75 && t.mine && el < BOOM {
                    // the fuse: the tile reddens and the mine swells and shivers
                    let k = (el / BOOM) as f32;
                    block(q, fade(BAD, 0.3 + 0.45 * k));
                    mine_mark(mid + vec2((now as f32 * 70.0).sin() * 2.0 * k, 0.0), 13.0 + 7.0 * k, mix(INK, WHITE, k));
                } else if turn >= 0.75 && t.mine {
                    mine_mark(mid, 13.0, INK);
                } else if turn >= 0.75 && n > 0 {
                    centered(&n.to_string(), mid.x, mid.y + 8.0, 23.0, if n > 1 { INK } else { ACCENT });
                }
            }
            if t.mine && (BOOM..BOOM + 0.45).contains(&el) {
                // the blast's ring rolls out over the board
                let q = ((el - BOOM) / 0.45) as f32;
                draw_circle_lines(mid.x, mid.y, 20.0 + 170.0 * q, 8.0 * (1.0 - q), fade(BAD, 1.0 - q));
            }
        }
    }
    // one tab per board under it, the current one underlined
    let mut tx = BOARD.x;
    for (k, bd) in me.boards.iter().enumerate() {
        let cur = k == me.cur;
        let r = Rect::new(tx, 672.0, 34.0, 30.0);
        let hot = mine && !cur && r.contains(mouse());
        if hot {
            block(r, CARD_HI);
        }
        sign(bd.element, r.center(), 9.0, if cur || hot { INK } else { MUTED });
        if cur {
            draw_line(r.x + 4.0, r.bottom(), r.right() - 4.0, r.bottom(), 2.0, INK);
        }
        // keys 1-3 pick a board too; the faint number in the corner says which
        let key = [KeyCode::Key1, KeyCode::Key2, KeyCode::Key3].get(k).copied();
        if key.is_some() {
            text(&(k + 1).to_string(), r.x + 3.0, r.y + 11.0, 12.0, FAINT);
        }
        if (hot && clicked()) || (mine && !cur && !modal() && key.is_some_and(is_key_pressed)) {
            swap = Some(k);
        }
        tx += 42.0;
    }
    right(&format!("{} reveals left · {} mines", me.reveals_left, board.mines), BOARD.x + 6.0 * STEP - 2.0, 694.0, 15.0, MUTED);

    // your stacks: drag a chip onto a rite's sockets
    label("STACKS", hx, 584.0);
    for (j, &(e, n)) in held.iter().enumerate() {
        let r = chip(j);
        let n = n - usize::from(drag.is_some_and(|k| me.stacks[k] == e));
        let hot = mine && drag.is_none() && n > 0 && r.contains(mouse());
        block(r, if hot { CARD_HI } else { BG });
        outline(r, if n > 0 { INK } else { FAINT });
        // the newest gem of this element bounces in
        let t = free.iter().filter(|&&k| me.stacks[k] == e).map(|&k| stack_t[0][k.min(15)]).fold(0.0, f64::max);
        sign(e, vec2(r.x + 20.0, r.center().y), 10.0 * if n > 0 { bounce(t) } else { 1.0 }, if n > 0 { el_color(e) } else { FAINT });
        text(&n.to_string(), r.x + 38.0, r.center().y + 7.0, 20.0, if n > 0 { INK } else { MUTED });
        if hot && clicked() {
            drag = free.iter().rev().find(|&&k| me.stacks[k] == e).copied();
            fx(|fx| fx.drag_from = mouse());
        }
    }

    // both fighters' sigils, skills flying between them
    let shots: Vec<Shot> = fx(|fx| {
        fx.shots.retain(|s| now - s.t0 < 1.8);
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
            .last();
        let (push, crouch) = match swing {
            Some(p) if p < 0.0 => {
                let w = ((p + 1.2) / 1.2).powi(2);
                (-18.0 * w, w)
            }
            Some(p) => (50.0 * (p * std::f32::consts::PI).sin() - 18.0 * (1.0 - p), -(p * std::f32::consts::PI).sin()),
            None => (0.0, 0.0),
        };
        let since_hit = (now - hit_at[s]) as f32;
        let mut at = STAND[s] + toward * push;
        at.y += (now as f32 * 3.0 + s as f32 * 2.0).sin() * 3.0;
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
            }
        }
        // the enemy just felled stays on its feet until the blow lands
        let f = if s == 1 { fallen.as_ref().unwrap_or(&b.f[1]) } else { &b.f[0] };
        let dead = bar[s] <= 0;
        let tint = if dead { fade(MUTED, 0.35) } else if k > 0.3 { mix(WHITE, BAD, k) } else { WHITE };
        let size = FIGHTER[s];
        let (w, h) = (size * (1.0 + 0.34 * k + 0.12 * crouch), size * (1.0 - 0.22 * k - 0.12 * crouch));
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

    // rites, on the right: only the ones you can make. Drop gems into a rite's sockets to cast it.
    let rx = 880.0;
    label("RITES", rx, 316.0);
    right("drag gems in, or right-click to pick", rx + 350.0, 316.0, 14.0, MUTED);
    let ready = b.ready(0);
    let list = combos_for(&me.elements);
    // right-click off the board picks the next rite down; then a click on a gem sends it there
    let mut aim = fx(|fx| fx.aim).filter(|&i| i < list.len());
    let on_board = Rect::new(BOARD.x, BOARD.y, W as f32 * STEP, W as f32 * STEP).contains(mouse());
    if mine && !modal() && !on_board && !list.is_empty() && is_mouse_button_pressed(MouseButton::Right) {
        aim = Some(aim.map_or(0, |i| (i + 1) % list.len()));
        play(Sfx::Click);
    }
    let rh = (300.0 / list.len().max(1) as f32).min(66.0);
    let mut y = 326.0;
    // where a dragged gem may land: (row, rite, its cost)
    let mut targets: Vec<(Rect, &'static Combo, Vec<Element>)> = vec![];
    for (row, &c) in list.iter().enumerate() {
        let (lvl, cost) = (me.level(c), me.cost(c));
        let lit = ready.iter().any(|r| r.name == c.name);
        let slotted = sockets.get(c.name).cloned().unwrap_or_default();
        let r = Rect::new(rx - 10.0, y, 370.0, rh);
        let over_row = mine && r.contains(mouse());
        if dragged.is_some_and(|e| count(&cost, e) > count(&slotted, e) || paid_toll(&cost, &slotted).len() < me.mods.toll.max(0) as usize) {
            block(r, if over_row { CARD_HI } else { CARD });
        }
        draw_line(rx, y + rh, rx + 350.0, y + rh, 1.0, FAINT);
        if mine && aim == Some(row) {
            // the picked rite: framed, with a pointer at its left edge
            outline(r, INK);
            let m = vec2(r.x - 6.0, r.center().y);
            draw_triangle(m + vec2(-10.0, -8.0), m + vec2(-10.0, 8.0), m, INK);
        }
        let bright = lit || !slotted.is_empty();
        if lvl > 1 {
            lively(c.title(lvl), rx, y + 30.0, 24.0, fade(GOLD, if bright { 1.0 } else { 0.55 }), Motion::Wave);
        } else {
            text(c.title(lvl), rx, y + 30.0, 24.0, if bright { INK } else { MUTED });
        }
        if let Some(br) = me.brand(c) {
            text(br.name, rx + measure(c.title(lvl), 24.0).width + 10.0, y + 30.0, 14.0, ACCENT);
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
        // a brand can make the line long: shrink it rather than run into the next panel
        let line = me.describe(c);
        rich(&line, rx, y + rh - 12.0, if measure(&line, 14.0).width > 300.0 { 12.0 } else { 14.0 }, MUTED);
        if lit && b.hits(0, c) {
            // hover the number to see how it adds up
            let (d, steps) = b.hit_steps(0, c);
            let (num, nx, ny) = (format!("{d} dmg"), rx + 350.0, y + rh - 12.0);
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
        }
        targets.push((r, c, cost));
        y += rh;
    }

    // a dragged gem lands where the button comes up: in a rite's socket.
    // Dropped anywhere else it just goes back to your stacks; nothing fires on a click.
    if let Some(k) = drag {
        let e = b.f[0].stacks[k];
        if !is_mouse_button_down(MouseButton::Left) {
            // let go where it was picked up: a click, which sends it to the picked rite
            let tapped = mouse().distance(fx(|fx| fx.drag_from)) < 6.0;
            let target = targets.iter().find(|(r, _, _)| r.contains(mouse())).or(if tapped { aim.and_then(|i| targets.get(i)) } else { None });
            if target.is_none() && tapped {
                fx(|fx| fx.floats.push((mouse(), "RIGHT-CLICK TO PICK A RITE".into(), MUTED, now, 18.0)));
            }
            if let Some((r, c, cost)) = target {
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
                    if tapped {
                        // sent by a click, not carried: the gem flies from your stacks to its socket
                        stream(fx(|fx| fx.drag_from), socket, 14, el_color(e));
                    }
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
        right(&format!("{left} reveals left. Space again ends the turn."), 1230.0, 708.0, 14.0, MUTED);
    }
    let pass = button(Rect::new(880.0, 640.0, 350.0, 44.0), "End turn", mine, true) || space_ends;
    if pass {
        b.end_turn();
    } else if let Some((c, paid)) = cast {
        b.cast_with(c, &paid);
    } else if let Some(k) = swap {
        b.swap(k);
    } else if let Some(i) = reveal {
        b.reveal(i);
    } else if let Some(i) = flag {
        b.toggle_flag(i);
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
        fx.aim = aim;
    });
    let dur = |lvl: usize| if lvl == 0 { TURN_BANNER } else { card_dur(lvl) };
    let (banner, flashed) = fx(|fx| {
        fx.banner.retain(|x| now - x.3 < dur(x.4) as f64);
        (fx.banner.iter().filter(|x| x.3 <= now).last().copied(), fx.flash)
    });
    if let Some((name, c, from, t0, lvl)) = banner {
        // a skill's name (or whose turn it is) sweeps in from the caster's side along the seam above
        // your board; upgraded skills get a taller band and their level
        let up = lvl.saturating_sub(1) as f32;
        let (dur, el) = (dur(lvl), (now - t0) as f32);
        let dir = if from == 0 { -1.0 } else { 1.0 };
        // eases in with a little overshoot, holds, then slips on out the far side as it fades
        let e = (el / 0.28).min(1.0);
        let settle = 1.0 + 2.70158 * (e - 1.0).powi(3) + 1.70158 * (e - 1.0).powi(2);
        let a = ((dur - el) / 0.25).clamp(0.0, 1.0);
        let slide = (1.0 - settle) * 620.0 * dir - (1.0 - a) * 140.0 * dir;
        let (size, band) = if lvl == 0 { (46.0, 64.0) } else { (62.0 + 8.0 * up, 86.0 + 10.0 * up) };
        let cx = BOARD.x + W as f32 * STEP / 2.0 - 1.0;
        let mid = BOARD.y - 20.0;
        let top = mid - band / 2.0;
        let wide = (measure(name, size).width + 120.0).max(W as f32 * STEP + 80.0);
        let bx = Rect::new(cx + slide - wide / 2.0, top, wide, band);
        // the band flares in the rite's colour as it arrives, then darkens to let the name read
        draw_rectangle(bx.x, bx.y, bx.w, bx.h, fade(mix(OUT, c, 0.45 * (1.0 - e)), 0.92 * a));
        draw_line(bx.x, bx.y, bx.right(), bx.y, 2.0, fade(c, 0.8 * a));
        draw_line(bx.x, bx.bottom(), bx.right(), bx.bottom(), 2.0, fade(c, 0.8 * a));
        stamped(name, cx + slide, mid + size / 3.0, size * (1.0 + 0.5 * (1.0 - e)), fade(c, a));
        if lvl >= 2 {
            stamped(if lvl == 2 { "LV II" } else { "LV III" }, cx - slide, top - 20.0, 24.0, fade(GOLD_C, a));
        }
    }
    if let Some((c, t0)) = flashed {
        let p = (now - t0) as f32 / 0.18;
        if p < 1.0 {
            draw_rectangle(-2000.0, -2000.0, 5280.0, 5000.0, fade(c, 0.32 * (1.0 - p)));
        }
    }
    fx(|fx| {
        fx.floats.retain(|f| now - f.3 < 1.5);
        for (at, label, c, t0, size) in &fx.floats {
            let el = (now - t0) as f32;
            if el < 0.0 {
                continue;
            }
            // pops, holds a moment, then drifts up and fades over 1.5 s
            let p = el / 1.5;
            let kick = if el < 0.14 { 1.0 + 0.9 * (1.0 - el / 0.14) } else { 1.0 };
            // one font all the way: growing and shrinking must not cross into the other face
            let f = face_in(if *size >= DISPLAY_FROM { 0 } else { 1 }, (size + 12.0 * (1.0 - p)) * kick);
            let w = FONT.with(|ft| measure_text(label, ft.borrow()[f.0].as_ref(), f.1, f.2)).width;
            text_in(f, label, at.x - w / 2.0, at.y - p * 40.0, fade(*c, 1.6 - p * 1.6));
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
    let shown = get_time() - fx(|fx| fx.ended_at) - 1.6;
    if over && shown > 0.0 {
        draw_rectangle(-2000.0, -2000.0, 5280.0, 5000.0, fade(OUT, (shown as f32 * 2.0).min(if ending.is_some() { 0.7 } else { 0.45 })));
        if let Some(opened) = ending {
            return summary(run, Some(b), b.won(), opened);
        }
        let k = bounce(now - shown);
        let r = scaled(Rect::new(440.0, 270.0, 400.0, 180.0), k);
        plate(r, BG);
        stamped(if b.won() { "Victory" } else { "Defeated" }, 640.0, r.y + 74.0 * k, 50.0 * k, if b.won() { GOOD } else { BAD });
        return shown > 0.25 && button(Rect::new(540.0, 380.0, 200.0, 40.0), "Continue", true, true);
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

fn node_at(run: &Run, c: usize, r: usize) -> Vec2 {
    let n = run.map[c].len() as f32;
    vec2(110.0 + c as f32 * 150.0, 390.0 + (r as f32 - (n - 1.0) / 2.0) * 150.0)
}

/// Returns the node the player clicked, if it is one they may enter.
pub fn map_screen(run: &Run) -> Option<(usize, usize)> {
    status(run);
    text(&run.note, 48.0, 92.0, 18.0, GOLD_C);
    let open = run.choices();
    let mut chosen = None;
    for (c, col) in run.map.iter().enumerate() {
        for (r, node) in col.iter().enumerate() {
            let p = node_at(run, c, r);
            for &j in &node.next {
                let q = node_at(run, c + 1, j);
                let live = run.pos == Some((c, r));
                draw_line(p.x, p.y, q.x, q.y, 2.0, if live { INK } else { FAINT });
            }
        }
    }
    for (c, col) in run.map.iter().enumerate() {
        for (r, node) in col.iter().enumerate() {
            let p = node_at(run, c, r);
            let can = open.contains(&(c, r));
            let here = run.pos == Some((c, r));
            let hot = can && (mouse() - p).abs().max_element() < 36.0;
            let mut icon = node_look(node.kind);
            let boss = node.kind == NodeKind::Boss;
            if boss {
                icon = run.boss_def().sprite;
            }
            let half = if hot { 35.0 } else { 32.0 };
            let plate_r = Rect::new(p.x - half, p.y - half, half * 2.0, half * 2.0);
            // where you stand is solid white; the places you may go next are outlined
            block(plate_r, if here { INK } else if hot { CARD_HI } else { BG });
            outline(plate_r, if can || here { INK } else { FAINT });
            sprite(icon, p.x - 24.0, p.y - 24.0, 48.0, if here { BLACK } else if can { WHITE } else { fade(WHITE, 0.35) });
            let label = if here { "You are here" } else if boss { run.boss_def().name } else { node.kind.name() };
            if boss && !here {
                lively_centered(label, p.x, p.y + 66.0, 16.0, BLOOD, Motion::Tremble);
            } else {
                centered(label, p.x, p.y + 66.0, 16.0, if can || here { INK } else { MUTED });
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
    for c in combos_for(&HEROES[run.hero].elements) {
        let lvl = run.level(combo_id(c));
        // the gems it takes at its level, not the base two or three
        let cost = c.cost(lvl);
        for (j, &e) in cost.iter().enumerate() {
            gem(e, vec2(x + 9.0 + j as f32 * 20.0, y + 9.0), 8.0);
        }
        if lvl > 1 {
            lively(c.title(lvl), x + 9.0 + cost.len() as f32 * 20.0, y + 15.0, 16.0, GOLD, Motion::Wave);
        } else {
            text(c.title(lvl), x + 9.0 + cost.len() as f32 * 20.0, y + 15.0, 16.0, INK);
        }
        if lvl > 1 {
            right(&format!("Lv {lvl}"), side.right() - 22.0, y + 15.0, 15.0, GOLD_C);
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
    text("PACTS", x, y + 18.0, 15.0, ACCENT);
    let top = y + 28.0;
    let (mut tip, mut tip_label) = held.map_or((HEROES[run.hero].passive_desc, "PASSIVE"), |b| (b.desc, "BLESSING"));
    let rows = 22.0f32.min((576.0 - top) / run.charms.len().max(1) as f32);
    let n = run.charms.len();
    if rows >= 22.0 {
        for (k, &id) in run.charms.iter().enumerate() {
            let r = Rect::new(x - 8.0, top + k as f32 * rows, 298.0, rows);
            let hot = r.contains(mouse());
            if hot {
                (tip, tip_label) = (CHARMS[id].desc, "PACT");
                block(r, CARD_HI);
            }
            let c = &CHARMS[id];
            sprite(c.icon.0, x, r.y + (rows - 18.0) / 2.0, 18.0, rgb(c.icon.1));
            pact_name(id, x + 26.0, r.y + rows / 2.0 + 6.0, 16.0, if hot { INK } else { rarity_color(c.rarity) });
        }
    } else {
        // too many to list by name: icons, row by row, and a "+N" for any that still do not fit
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

fn charm_card(r: Rect, id: usize, on: bool) -> bool {
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
    hit
}

pub fn pick(options: &[usize]) -> Option<usize> {
    // title and cards sit in the middle of the screen, however many cards there are
    stamped("Choose a pact", 640.0, 250.0, 40.0, INK);
    let mut chosen = None;
    let x0 = 640.0 - (options.len() as f32 * 350.0 - 30.0) / 2.0;
    for (k, &id) in options.iter().enumerate() {
        if charm_card(Rect::new(x0 + k as f32 * 350.0, 300.0, 320.0, 240.0), id, true) {
            chosen = Some(id);
        }
    }
    chosen
}

/// A boss fell: the player picks one skill to upgrade. Returns its index in `COMBOS`.
pub fn upgrade(run: &Run, ids: &[usize]) -> Option<usize> {
    lively_centered("The boss falls", 640.0, 128.0, 44.0, GOLD, Motion::Wave);
    centered("Upgrade one rite. It hits harder, takes a new name and look, and needs one more gem.", 640.0, 164.0, 18.0, MUTED);
    let n = ids.len() as f32;
    let (w, gap) = (280.0, 20.0);
    let x0 = 640.0 - (n * w + (n - 1.0) * gap) / 2.0;
    let pulse = (now() as f32 * 5.0).sin() * 0.5 + 0.5;
    let mut chosen = None;
    for (k, &id) in ids.iter().enumerate() {
        let c = &COMBOS[id];
        let lvl = run.level(id);
        let r = Rect::new(x0 + k as f32 * (w + gap), 196.0, w, 420.0);
        let col = el_color(c.needs[0]);
        let hot = r.contains(mouse()) && !modal();
        if card(r, true) {
            chosen = Some(id);
            play(Sfx::LevelUp);
        }
        if hot && gen_range(0.0, 1.0) < 0.5 {
            rise(vec2(gen_range(r.x + 20.0, r.right() - 20.0), r.bottom() - 16.0), 1, 4.0, col);
        }
        let x = r.x + 20.0;
        text(&format!("LV {lvl}  >  LV {}", lvl + 1), x, r.y + 30.0, 16.0, ACCENT);
        text(c.title(lvl), x, r.y + 60.0, 18.0, MUTED);
        text(c.title(lvl + 1), x, r.y + 98.0, 26.0, GOLD_C);
        for (row, (label, l)) in [("NOW", lvl), ("UPGRADED", lvl + 1)].into_iter().enumerate() {
            let y = r.y + 136.0 + row as f32 * 62.0;
            text(label, x, y, 14.0, if row == 0 { MUTED } else { ACCENT });
            let cost = c.cost(l);
            for (j, &e) in cost.iter().enumerate() {
                let p = vec2(x + 12.0 + j as f32 * 30.0, y + 24.0);
                if row == 1 && j + 1 == cost.len() {
                    // the extra gem the upgrade asks for
                    draw_rectangle_lines(p.x - 15.0, p.y - 15.0, 30.0, 30.0, 1.5, fade(INK, 0.4 + 0.5 * pulse));
                }
                gem(e, p, 11.0);
            }
        }
        let y = wrapped(&rite_text(c, lvl, run.brand_of(id)), x, r.y + 280.0, w - 40.0, 16.0, MUTED);
        text("becomes", x, y + 6.0, 14.0, ACCENT);
        wrapped(&rite_text(c, lvl + 1, run.brand_of(id)), x, y + 30.0, w - 40.0, 18.0, GOLD_C);
    }
    particles();
    chosen
}

thread_local!(static BRANDING: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) });

/// An elite fell: pick a brand, then the rite to burn it onto. Returns (index in `COMBOS`, index in `BRANDS`).
pub fn brand(run: &Run, offers: &[usize]) -> Option<(usize, usize)> {
    let Some(chosen) = BRANDING.with(|b| b.get()) else {
        lively_centered("The elite falls", 640.0, 200.0, 44.0, GOLD, Motion::Wave);
        centered("Take a brand. Burnt onto a rite, it adds its effect for no extra gem. A rite holds one.", 640.0, 236.0, 18.0, MUTED);
        let x0 = 640.0 - (offers.len() as f32 * 300.0 - 20.0) / 2.0;
        for (k, &id) in offers.iter().enumerate() {
            let r = Rect::new(x0 + k as f32 * 300.0, 280.0, 280.0, 180.0);
            if card(r, !modal()) {
                BRANDING.with(|b| b.set(Some(id)));
            }
            stamped(BRANDS[id].name, r.center().x, r.y + 64.0, 34.0, INK);
            let line = BRANDS[id].effect.text(1);
            rich(&(line[..1].to_uppercase() + &line[1..]), r.x + 24.0, r.y + 124.0, 18.0, INK);
        }
        return None;
    };
    let b = &BRANDS[chosen];
    lively_centered(&format!("Burn {} onto a rite", b.name), 640.0, 128.0, 40.0, GOLD, Motion::Wave);
    centered("A rite holds one brand: a new one replaces the old.", 640.0, 164.0, 18.0, MUTED);
    let ids = run.rites();
    let n = ids.len() as f32;
    let (w, gap) = (280.0, 20.0);
    let x0 = 640.0 - (n * w + (n - 1.0) * gap) / 2.0;
    let mut done = None;
    for (k, &id) in ids.iter().enumerate() {
        let c = &COMBOS[id];
        let lvl = run.level(id);
        let r = Rect::new(x0 + k as f32 * (w + gap), 196.0, w, 340.0);
        if card(r, !modal()) {
            done = Some((id, chosen));
            play(Sfx::LevelUp);
        }
        let x = r.x + 20.0;
        text(c.title(lvl), x, r.y + 40.0, 26.0, INK);
        for (j, &e) in c.cost(lvl).iter().enumerate() {
            gem(e, vec2(x + 12.0 + j as f32 * 30.0, r.y + 76.0), 11.0);
        }
        let old = run.brand_of(id);
        let y = wrapped(&rite_text(c, lvl, old), x, r.y + 120.0, w - 40.0, 16.0, MUTED);
        text(&old.map_or("becomes".into(), |o| format!("{} is replaced", o.name)), x, y + 6.0, 14.0, ACCENT);
        wrapped(&rite_text(c, lvl, Some(b)), x, y + 30.0, w - 40.0, 18.0, GOLD_C);
    }
    if button(Rect::new(565.0, 576.0, 150.0, 40.0), "Back", !modal(), false) {
        BRANDING.with(|b| b.set(None));
    }
    if done.is_some() {
        BRANDING.with(|b| b.set(None));
    }
    done
}

/// The shopkeeper reacts to something for a few seconds.
fn say(line: &'static str) {
    fx(|fx| fx.line = Some((line, now())));
}

/// A shrine fight was won: take one blessing, or keep the one held. Some(None) keeps it.
pub fn blessing(run: &Run, offers: &[usize]) -> Option<Option<usize>> {
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
        let r = Rect::new(x0 + k as f32 * 300.0, 280.0, 280.0, 180.0);
        if card(r, !modal()) {
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
    // a plain heading, like every other screen
    stamped("Tinker's Stall", 640.0, 112.0, 40.0, INK);

    // the shopkeeper's booth: the keeper and your purse
    let booth = Rect::new(48.0, 140.0, 290.0, 494.0);
    plate(booth, WELL);
    sprite(HD + 27, 118.0, 290.0 + (t as f32 * 2.0).sin() * 3.0, 150.0, WHITE);
    rounded(Rect::new(72.0, 470.0, 242.0, 2.0), 1.0, fade(MUTED, 0.25));
    text("PURSE", 70.0, 518.0, 15.0, ACCENT);
    shard(vec2(82.0, 556.0), 11.0);
    text(&run.shards.to_string(), 102.0, 568.0, 32.0, GOLD_C);

    // the wares
    let mut sel = fx(|fx| fx.pick).filter(|&k| shop.stock.get(k).is_some_and(|w| w.is_some()));
    let mut hover = None;
    for k in 0..shop.stock.len() {
        let r = Rect::new(370.0 + k as f32 * 220.0, 140.0, 200.0, 216.0);
        let Some(id) = shop.stock[k] else {
            plate(r, WELL);
            stamped("SOLD", r.center().x, r.center().y + 12.0, 34.0, fade(MUTED, 0.6));
            continue;
        };
        let c = &CHARMS[id];
        let price = Run::price(shop, k);
        let hot = r.contains(mouse()) && !modal();
        if hot {
            hover = Some((k, price));
        }
        let edge = rarity_color(c.rarity);
        plate(r, if hot || sel == Some(k) { CARD_HI } else { CARD });
        if sel == Some(k) || hot {
            outline(r, INK);
        }
        let lift = if hot { -6.0 } else { 0.0 } + (t as f32 * 2.5 + k as f32).sin() * 2.0;
        charm_icon(id, Rect::new(r.center().x - 44.0, r.y + 18.0 + lift, 88.0, 88.0), if sel == Some(k) { 0.3 } else { 0.0 });
        pact_name(id, r.center().x - measure(c.name, 18.0).width / 2.0, r.y + 134.0, 18.0, INK);
        rich_centered(c.rarity.name(), r.center().x, r.y + 154.0, 14.0, edge);
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
        }
    }

    // a closer look at the chosen ware, and the button that buys it
    let panel = Rect::new(356.0, 410.0, 884.0, 156.0);
    plate(panel, CARD);
    match sel.and_then(|k| shop.stock[k].map(|id| (k, id))) {
        Some((k, id)) => {
            let c = &CHARMS[id];
            charm_icon(id, Rect::new(panel.x + 22.0, panel.y + 22.0, 64.0, 64.0), 0.0);
            pact_name(id, panel.x + 106.0, panel.y + 46.0, 28.0, INK);
            rich(c.rarity.name(), panel.x + 106.0, panel.y + 70.0, 16.0, rarity_color(c.rarity));
            wrapped(c.desc, panel.x + 106.0, panel.y + 100.0, 500.0, 18.0, INK);
            let price = Run::price(shop, k);
            if button(Rect::new(panel.right() - 240.0, panel.y + 56.0, 210.0, 48.0), &format!("Buy for {price}"), run.shards >= price, true) && run.buy(shop, k) {
                play(Sfx::Coins);
                burst(vec2(panel.right() - 135.0, panel.y + 80.0), 24, 240.0, 7.0, GOLD_C);
                say("A fine choice. It suits you.");
                sel = None;
            }
        }
        None => centered("Click a ware to take a closer look.", panel.center().x, panel.center().y + 6.0, 18.0, MUTED),
    }

    // services
    let restock = Run::restock_price(shop);
    if button(Rect::new(692.0, 590.0, 260.0, 44.0), &format!("Restock for {restock}"), run.shards >= restock && run.can_restock(shop), false) && run.restock(shop) {
        play(Sfx::Coins);
        sel = None;
        for k in 0..SHOP_SIZE {
            burst(vec2(470.0 + k as f32 * 220.0, 250.0), 14, 200.0, 7.0, ACCENT);
        }
        say("Fresh from the deep. Have a look.");
    }
    let leave = button(Rect::new(968.0, 590.0, 272.0, 44.0), "Leave", true, false);

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
    plate(Rect::new(190.0, 100.0, 900.0, 540.0), CARD);
    stamped(e.title, 640.0, 170.0, 40.0, INK);
    let y = wrapped(e.text, 240.0, 230.0, 800.0, 22.0, MUTED) + 30.0;
    if let Some(r) = result {
        wrapped(r, 240.0, y, 800.0, 22.0, GOLD_C);
        return button(Rect::new(540.0, y + 80.0, 200.0, 44.0), "Continue", true, true);
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
                GIVING.with(|g| g.set(None));
            }
        }
        wrapped(tip, 240.0, 580.0, 600.0, 16.0, MUTED);
        if button(Rect::new(890.0, 576.0, 150.0, 40.0), "Back", true, false) {
            GIVING.with(|g| g.set(None));
        }
        return false;
    }
    for (k, c) in e.choices.iter().enumerate() {
        if button(Rect::new(240.0, y + k as f32 * 60.0, 800.0, 44.0), c.label, run.can_afford(c), false) {
            if c.outs.iter().any(|o| matches!(o, Out::LoseCharm)) {
                GIVING.with(|g| g.set(Some((e as *const Encounter, k))));
            } else {
                *result = Some(run.choose(c, None));
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
    let (wins, dealt, best) = match b {
        Some(b) => (run.wins + b.won() as usize, run.dealt + b.dealt, if b.best.0 > run.best.0 { b.best } else { run.best }),
        None => (run.wins, run.dealt, run.best),
    };
    // a beat crossing `at` this frame, for its sound
    let at_beat = |at: f32| dt() > 0.0 && t >= at && t - dt() < at;
    let appear = |at: f32| ((t - at) / 0.25).clamp(0.0, 1.0);
    let r = scaled(Rect::new(330.0, 66.0, 620.0, 590.0), bounce(t0));
    plate(r, BG);
    sprite(HEROES[run.hero].sprite, 604.0, r.y + 22.0, 72.0, if won { WHITE } else { fade(MUTED, 0.5) });
    let p = appear(0.15);
    if won {
        lively_centered("The last board is clear.", 640.0, r.y + 142.0, 40.0, fade(INK, p), Motion::Rainbow);
    } else {
        lively_centered("Your run ends here.", 640.0, r.y + 142.0, 40.0, fade(BLOOD, p), Motion::Tremble);
    }
    let sub = format!("{}  ·  {}  ·  floor {} of {}", HEROES[run.hero].name, EXPEDITIONS[run.exp].name, run.floor + 1, FLOORS);
    centered(&sub, 640.0, r.y + 174.0, 18.0, fade(MUTED, p));
    draw_line(r.x + 50.0, r.y + 196.0, r.right() - 50.0, r.y + 196.0, 1.0, fade(FAINT, p));
    // the numbers, one line at a time; the damage counts up
    let count = ((t - 0.8) / 0.8).clamp(0.0, 1.0);
    let rows = [
        ("Fights won", wins.to_string()),
        ("Rite damage", ((dealt as f32 * (1.0 - (1.0 - count).powi(3))).round() as i32).to_string()),
        ("Best hit", if best.0 > 0 { format!("{} · {}", best.0, best.1) } else { "none".into() }),
    ];
    for (k, (label, value)) in rows.iter().enumerate() {
        let at = 0.5 + k as f32 * 0.3;
        if at_beat(at) {
            play(Sfx::Flip);
        }
        let p = appear(at);
        let y = r.y + 236.0 + k as f32 * 40.0;
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
    if let Some(x) = opened {
        last += 0.35;
        if at_beat(last) {
            play(Sfx::LevelUp);
        }
        if t >= last {
            let k = bounce(t0 + last as f64);
            lively_centered("A NEW PATH OPENS", 640.0, r.y + 460.0, 16.0, GOLD, Motion::Wave);
            centered(EXPEDITIONS[x].name, 640.0, r.y + 492.0, 28.0 * k, INK);
        }
    }
    text(&format!("seed {}", run.seed), r.x + 24.0, r.bottom() - 18.0, 14.0, FAINT);
    t >= last + 0.35 && button(Rect::new(520.0, r.bottom() - 70.0, 240.0, 44.0), "Back to the map", true, true)
}

/// A button for use inside an open panel, where `button` ignores clicks.
fn panel_button(r: Rect, label: &str, primary: bool, press: bool) -> bool {
    let hot = r.contains(mouse());
    button_face(r, label, true, primary, hot);
    if hot && press {
        play(Sfx::Click);
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
    give_up(Rect::new(56.0, 652.0, 150.0, 36.0), "Surrender", ["The fight is lost, and the expedition with it.", "How far you got still counts."])
}

/// A button that asks first: true once the player confirms `word`.
fn give_up(at: Rect, word: &str, lines: [&str; 2]) -> bool {
    if !RETREAT.with(|r| r.get()) {
        if button(at, word, !modal(), false) {
            RETREAT.with(|r| r.set(true));
        }
        return false;
    }
    let press = is_mouse_button_pressed(MouseButton::Left) && !SETTINGS.with(|s| s.get());
    draw_rectangle(-2000.0, -2000.0, 5280.0, 5000.0, fade(OUT, 0.6));
    let panel = Rect::new(390.0, 240.0, 500.0, 220.0);
    plate(panel, CARD);
    stamped(&format!("{word}?"), 640.0, 296.0, 34.0, INK);
    centered(lines[0], 640.0, 336.0, 16.0, MUTED);
    centered(lines[1], 640.0, 358.0, 16.0, MUTED);
    let quit = panel_button(Rect::new(430.0, 392.0, 200.0, 40.0), word, false, press);
    // not Esc: the settings panel answers to Esc in the same frame
    let stay = panel_button(Rect::new(650.0, 392.0, 200.0, 40.0), "Keep going", true, press);
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
    draw_rectangle(-2000.0, -2000.0, 5280.0, 5000.0, fade(OUT, 0.7));
    let panel = Rect::new(300.0, 96.0, 680.0, 528.0);
    plate(panel, CARD);
    stamped("How to play", 640.0, 150.0, 38.0, INK);
    let mut y = 196.0;
    for line in [
        "Each board holds crystals of one element. Clear every safe tile around a crystal to claim a gem of that element.",
        "Numbers count the mines touching a tile. A crystal shows its count in the corner. Right-click a tile to flag it.",
        "A mine costs HP, wipes your surge, breaks the crystals beside it and ends your turn.",
        "Safe flips in a row build surge: +20% rite damage per step. Every rite you cast spends a step, so think about when to fire.",
        "Drag gems into a rite's sockets to cast it. Gems no rite can use wait in your stacks.",
        "Swap boards with the tabs under yours; it is free. Some rites reach into the enemy's board.",
        "Words that float can be hovered for what they mean.",
    ] {
        y = wrapped(line, panel.x + 40.0, y, panel.w - 80.0, 17.0, INK) + 12.0;
    }
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
    block(grow(gear, 3.0), OUT);
    block(gear, if hot || open { ACCENT } else { CARD_HI });
    // a gear, pixel style: a ring with four teeth
    let (c, ink) = (gear.center(), if hot || open { OUT } else { INK });
    draw_rectangle(c.x - 8.0, c.y - 8.0, 16.0, 16.0, ink);
    draw_rectangle(c.x - 3.0, c.y - 12.0, 6.0, 24.0, ink);
    draw_rectangle(c.x - 12.0, c.y - 3.0, 24.0, 6.0, ink);
    draw_rectangle(c.x - 4.0, c.y - 4.0, 8.0, 8.0, if hot || open { ACCENT } else { CARD_HI });
    let mut toggle = (hot && press) || is_key_pressed(KeyCode::Escape);
    if open {
        draw_rectangle(-2000.0, -2000.0, 5280.0, 5000.0, fade(OUT, 0.6));
        let panel = Rect::new(400.0, 210.0, 480.0, 300.0);
        plate(panel, CARD);
        stamped("Settings", 640.0, 262.0, 34.0, INK);
        let (mut music, mut sound) = VOLUME.with(|v| v.get());
        for (row, (label, level)) in [("Music", &mut music), ("Sound", &mut sound)].into_iter().enumerate() {
            let y = 300.0 + row as f32 * 62.0;
            text(label, 436.0, y + 26.0, 22.0, INK);
            if panel_button(Rect::new(540.0, y, 40.0, 36.0), "-", false, press) {
                *level = (*level - 1).max(0);
            }
            if panel_button(Rect::new(804.0, y, 40.0, 36.0), "+", false, press) {
                *level = (*level + 1).min(10);
            }
            for k in 0..10 {
                block(Rect::new(592.0 + k as f32 * 21.0, y + 6.0, 16.0, 24.0), if k < *level { ACCENT } else { WELL });
            }
        }
        VOLUME.with(|v| v.set((music, sound)));
        centered("M mutes everything.  N skips to the next song.", 640.0, 440.0, 15.0, MUTED);
        toggle |= panel_button(Rect::new(560.0, 456.0, 160.0, 36.0), "Close", true, press);
    }
    if toggle {
        play(Sfx::Click);
        SETTINGS.with(|s| s.set(!open));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
