//! Immediate-mode screens: each function draws one screen and reports what was clicked.

use crate::{
    battle::{Battle, Event, PRESSURE_TURN},
    board::{Board, W},
    data::*,
    run::*,
    sfx::{self, Sfx},
};
use macroquad::audio::{play_sound, PlaySoundParams, Sound};
use macroquad::rand::gen_range;
use macroquad::prelude::*;

const fn rgb(hex: u32) -> Color {
    Color::new(((hex >> 16) & 255) as f32 / 255.0, ((hex >> 8) & 255) as f32 / 255.0, (hex & 255) as f32 / 255.0, 1.0)
}

// palette picked to sit with the Tiny Dungeon sprites: plum darks, sand accent, warm white
pub const BG: Color = rgb(0x1c1826);
const BG_LOW: Color = rgb(0x2b1f33);
const OUT: Color = rgb(0x120e19);
const CARD: Color = rgb(0x372f48);
const CARD_HI: Color = rgb(0x4d4266);
const WELL: Color = rgb(0x231c2f);
const INK: Color = rgb(0xfff4e0);
const MUTED: Color = rgb(0xb3a7bd);
const ACCENT: Color = rgb(0xf2a65e);
const GOLD_C: Color = rgb(0xffd76a);
const GOOD: Color = rgb(0x6fe08a);
const BAD: Color = rgb(0xff6b6b);

pub const VIEW: Vec2 = vec2(1280.0, 720.0);

fn el_color(e: Element) -> Color {
    match e {
        Element::Ember => rgb(0xff7d4a),
        Element::Tide => rgb(0x4fa8ff),
        Element::Gale => rgb(0x5fe0a8),
        Element::Stone => rgb(0xcbb089),
        Element::Spark => rgb(0xffd84a),
    }
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
    floats: Vec<(Vec2, String, Color, f64)>,
    /// skills in flight: (caster, element, name, start)
    shots: Vec<(usize, Element, &'static str, f64)>,
    fade: f64,
    /// the win or lose jingle has played for this battle
    ended: bool,
    parts: Vec<Part>,
    /// whole-screen shake: (start, strength in pixels)
    quake: (f64, f32),
    /// whole-screen colour flash: (colour, start)
    flash: Option<(Color, f64)>,
    /// a combo's name slammed across the screen: (name, element, caster, start)
    banner: Option<(&'static str, Element, usize, f64)>,
    /// when each of your charms last took effect, by charm id
    charm_t: std::collections::HashMap<usize, f64>,
    /// gems you have slotted into each skill, by skill name; it fires when every socket is full
    sockets: std::collections::HashMap<&'static str, Vec<Element>>,
    /// the gem being dragged: (index in your stacks, where the drag started)
    drag: Option<(usize, Vec2)>,
}

thread_local! {
    /// [display face for titles and numbers, body face for everything small]
    static FONT: std::cell::RefCell<[Option<Font>; 2]> = const { std::cell::RefCell::new([None, None]) };
    /// where the camera is looking this frame, for snapping text to whole screen pixels
    static CAM: std::cell::Cell<Vec2> = const { std::cell::Cell::new(vec2(640.0, 360.0)) };
    /// the "abandon this run?" question is up
    static RETREAT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static ART: std::cell::RefCell<Option<Texture2D>> = const { std::cell::RefCell::new(None) };
    static SCALE: std::cell::Cell<f32> = const { std::cell::Cell::new(1.0) };
    static FX: std::cell::RefCell<Fx> = std::cell::RefCell::new(Fx::default());
    static SOUNDS: std::cell::RefCell<Vec<Vec<Sound>>> = const { std::cell::RefCell::new(Vec::new()) };
    static QUIET: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// (music, sound effects) volume, each 0..=10
    static VOLUME: std::cell::Cell<(i32, i32)> = const { std::cell::Cell::new((6, 8)) };
    static SETTINGS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
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
    let now = get_time();
    fx(|fx| {
        for _ in 0..n {
            let vel = Vec2::from_angle(gen_range(0.0, std::f32::consts::TAU)) * gen_range(0.25, 1.0) * speed;
            fx.parts.push(Part { pos: at, vel, born: now, life: gen_range(0.35, 0.9), size: size * gen_range(0.5, 1.0), color, grav: 520.0, home: None });
        }
    });
}

/// Sends `n` shards from one spot to another, like a claimed crystal flying to your stacks.
fn stream(from: Vec2, to: Vec2, n: usize, color: Color) {
    let now = get_time();
    fx(|fx| {
        for _ in 0..n {
            let vel = Vec2::from_angle(gen_range(0.0, std::f32::consts::TAU)) * gen_range(80.0, 260.0);
            fx.parts.push(Part { pos: from, vel, born: now, life: gen_range(0.45, 0.7), size: gen_range(5.0, 9.0), color, grav: 0.0, home: Some(to) });
        }
    });
}

fn quake(strength: f32) {
    fx(|fx| fx.quake = (get_time(), strength));
}

fn flash(color: Color) {
    fx(|fx| fx.flash = Some((color, get_time())));
}

/// Moves and draws every live particle.
fn particles() {
    let (now, dt) = (get_time(), get_frame_time());
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
            draw_rectangle(p.pos.x - s / 2.0, p.pos.y - s / 2.0, s, s, p.color);
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
/// missing; the built-in font stands in. Both are pixel fonts, so they are sampled without smoothing.
pub fn set_fonts(display: Option<Font>, body: Option<Font>) {
    FONT.with(|f| {
        *f.borrow_mut() = [display, body].map(|font| {
            font.map(|mut font| {
                font.set_filter(FilterMode::Nearest);
                font
            })
        })
    });
}

/// The Tiny Dungeon sprite sheet (12 tiles a row, 16px each).
pub fn set_art(sheet: Texture2D) {
    sheet.set_filter(FilterMode::Nearest);
    ART.with(|a| *a.borrow_mut() = Some(sheet));
}

/// Call when the screen changes: forgets the old screen's effects and fades the new one in.
pub fn transition() {
    fx(|fx| *fx = Fx { fade: get_time(), ..Default::default() });
}

pub fn end_frame() {
    let a = fx(|fx| 1.0 - (get_time() - fx.fade) / 0.25) as f32;
    if a > 0.0 {
        draw_rectangle(-2000.0, -2000.0, 5280.0, 5000.0, fade(BG, a));
    }
}

/// Bounce curve for something that appeared at `t0`: starts small, overshoots, settles at 1.
fn bounce(t0: f64) -> f32 {
    let p = (((get_time() - t0) / 0.22) as f32).clamp(0.0, 1.0);
    0.7 + 0.3 * p + 0.14 * (p * std::f32::consts::PI).sin()
}

/// Scale for a thing identified by `key`, bouncing in from the first frame it is drawn.
fn pop(key: (usize, usize, usize)) -> f32 {
    let now = get_time();
    bounce(fx(|fx| *fx.pops.entry(key).or_insert(now)))
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
    let (w, h) = (screen_width(), screen_height());
    let k = (w / VIEW.x).min(h / VIEW.y).max(0.1);
    SCALE.with(|s| s.set(k));
    if is_key_pressed(KeyCode::M) {
        QUIET.with(|q| q.set(!q.get()));
    }
    // a heavy hit shakes the whole view for a moment
    let (t0, strength) = fx(|fx| fx.quake);
    let since = (get_time() - t0) as f32;
    let jolt = if since < 0.4 {
        vec2((since * 71.0).sin(), (since * 93.0).cos()) * strength * (1.0 - since / 0.4)
    } else {
        Vec2::ZERO
    };
    CAM.with(|c| c.set(VIEW / 2.0 + jolt));
    set_camera(&Camera2D { target: VIEW / 2.0 + jolt, zoom: vec2(2.0 * k / w, 2.0 * k / h), ..Default::default() });
    const BANDS: usize = 24;
    for i in 0..BANDS {
        let band = VIEW.y / BANDS as f32;
        draw_rectangle(-2000.0, i as f32 * band, 5280.0, band + 1.0, mix(BG, BG_LOW, i as f32 / BANDS as f32));
    }
    // slow drifting dust so the screen is never completely still
    let t = get_time() as f32;
    for i in 0..22 {
        let f = i as f32;
        let y = VIEW.y + 40.0 - (t * (9.0 + f % 5.0 * 4.0) + f * 173.0) % (VIEW.y + 80.0);
        let x = (f * 97.0) % VIEW.x + (t * 0.3 + f).sin() * 24.0;
        let size = 4.0 + (f % 3.0) * 4.0;
        draw_rectangle(x, y, size, size, mix(BG_LOW, ACCENT, 0.10 + (f % 4.0) * 0.03));
    }
}

fn mouse() -> Vec2 {
    let window = vec2(screen_width(), screen_height());
    (Vec2::from(mouse_position()) - window / 2.0) / scale() + VIEW / 2.0
}

/// True while a panel (settings, retreat) is open over the screen.
fn modal() -> bool {
    SETTINGS.with(|s| s.get()) || RETREAT.with(|r| r.get())
}

/// A left click meant for the screen. While a panel is open it swallows every click.
fn clicked() -> bool {
    !modal() && is_mouse_button_pressed(MouseButton::Left)
}

/// The display font runs wider than its nominal size; this keeps every layout as it was designed.
const FONT_K: f32 = 0.9;
/// Text this size and up is set in the display face; smaller text in the body face.
const DISPLAY_FROM: f32 = 24.0;

/// Which face, the font size to ask for, and the scale back to the canvas, for text of design `size`.
/// Pixel fonts only look clean when each font pixel covers whole screen pixels, so the size on screen
/// is rounded to the face's pixel grid: Kenney Mini is drawn on 8 px, Pixel Operator on 16 px.
fn face(size: f32) -> (usize, u16, f32) {
    let (k, dpi) = (scale(), screen_dpi_scale());
    let (which, grid, want) = if size >= DISPLAY_FROM { (0, 8.0, size * FONT_K) } else { (1, 16.0, size) };
    let px = (want * k * dpi / grid).round().max(1.0) * grid;
    // macroquad renders at ceil(size * dpi); find the size that lands exactly on the grid.
    // shortcut: at some fractional dpi (1.25) none does and the nearest is used, slightly soft
    let ask = (1..=px as u16).find(|&n| (n as f32 * dpi).ceil() == px).unwrap_or((px / dpi).round() as u16);
    (which, ask, 1.0 / k)
}

/// Moves a canvas point onto the nearest whole physical pixel, so glyph edges do not straddle two.
fn snap(p: Vec2) -> Vec2 {
    let (k, dpi, cam) = (scale(), screen_dpi_scale(), CAM.with(|c| c.get()));
    let half = vec2(screen_width(), screen_height()) / 2.0;
    (((p - cam) * k + half) * dpi).round() / dpi / k - half / k + cam
}

fn text(s: &str, x: f32, y: f32, size: f32, c: Color) {
    let (which, font_size, font_scale) = face(size);
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

/// Centred text with a hard drop shadow, for anything drawn over artwork.
fn stamped(s: &str, cx: f32, y: f32, size: f32, c: Color) {
    let d = (size / 12.0).max(2.0);
    centered(s, cx + d, y + d, size, fade(OUT, c.a));
    centered(s, cx, y, size, c);
}

fn right(s: &str, x: f32, y: f32, size: f32, c: Color) {
    text(s, x - measure(s, size).width, y, size, c);
}

/// Word-wraps into width `w`; returns the y of the line after the last one.
fn wrapped(s: &str, x: f32, mut y: f32, w: f32, size: f32, c: Color) -> f32 {
    let mut line = String::new();
    for word in s.split(' ') {
        let longer = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
        if !line.is_empty() && measure(&longer, size).width > w {
            text(&line, x, y, size, c);
            y += size * 1.25;
            line = word.to_string();
        } else {
            line = longer;
        }
    }
    text(&line, x, y, size, c);
    y + size * 1.25
}

/// Pixel-art panel: a rectangle with its corners notched off.
fn block(r: Rect, c: Color) {
    let u = 4.0f32.min(r.w / 4.0).min(r.h / 4.0);
    draw_rectangle(r.x + u, r.y, r.w - 2.0 * u, r.h, c);
    draw_rectangle(r.x, r.y + u, r.w, r.h - 2.0 * u, c);
}

fn grow(r: Rect, d: f32) -> Rect {
    Rect::new(r.x - d, r.y - d, r.w + 2.0 * d, r.h + 2.0 * d)
}

/// A block with a dark outline and a lit top edge.
fn plate(r: Rect, c: Color) {
    block(grow(r, 4.0), OUT);
    block(r, c);
    draw_rectangle(r.x + 4.0, r.y, r.w - 8.0, 4.0, mix(c, INK, 0.12));
}

/// A clickable card. Returns true on the frame it is clicked.
fn card(r: Rect, on: bool) -> bool {
    let hot = on && r.contains(mouse());
    if hot {
        block(grow(r, 8.0), ACCENT);
    }
    plate(r, if hot { CARD_HI } else { CARD });
    if hot && clicked() {
        play(Sfx::Click);
    }
    hot && clicked()
}

fn button(r: Rect, label: &str, on: bool, primary: bool) -> bool {
    let hot = on && r.contains(mouse());
    let fill = match (on, primary, hot) {
        (false, ..) => WELL,
        (true, true, false) => ACCENT,
        (true, true, true) => mix(ACCENT, INK, 0.35),
        (true, false, false) => CARD_HI,
        (true, false, true) => mix(CARD_HI, INK, 0.25),
    };
    // a pressed-in look: the button sits on a dark base and sinks a little under the cursor
    block(Rect::new(r.x, r.y + 4.0, r.w, r.h), OUT);
    let r = if hot { Rect::new(r.x, r.y + 2.0, r.w, r.h) } else { r };
    block(r, fill);
    let d = measure(label, 20.0);
    let ink = if !on { MUTED } else if primary { OUT } else { INK };
    text(label, r.x + (r.w - d.width) / 2.0, r.y + r.h / 2.0 + 7.0, 20.0, ink);
    if hot && clicked() {
        play(Sfx::Click);
    }
    hot && clicked()
}

/// Draws tile `idx` of the sprite sheet as a `size` square.
fn sprite(idx: usize, x: f32, y: f32, size: f32, tint: Color) {
    ART.with(|a| {
        if let Some(sheet) = a.borrow().as_ref() {
            // sample a hair inside the tile: at fractional positions (the idle bob, window scaling,
            // anti-aliased edges) the exact border would pick up a row of the neighbouring tile
            const INSET: f32 = 0.1;
            let source = Rect::new((idx % 12) as f32 * 16.0 + INSET, (idx / 12) as f32 * 16.0 + INSET, 16.0 - 2.0 * INSET, 16.0 - 2.0 * INSET);
            draw_texture_ex(sheet, x, y, tint, DrawTextureParams { dest_size: Some(vec2(size, size)), source: Some(source), ..Default::default() });
        }
    });
}

// 7x7 pixel crystals: o = body, l = highlight, d = shade. One shape per element as well as one colour.
const GEMS: [[&str; 7]; 5] = [
    ["...o...", "..oo...", "..olo.o", ".ollooo", ".olllod", ".oolodd", "..oddd."],
    ["...o...", "..olo..", "..olo..", ".ollod.", ".olood.", ".ooodd.", "..ddd.."],
    ["...o...", "..olo..", ".ollod.", "olloodd", ".ooodd.", "..odd..", "...d..."],
    [".ooooo.", "olllood", "olooodd", "olooodd", "ooooddd", "ooddddd", ".ddddd."],
    ["...ooo.", "..olo..", ".olo...", ".ooooo.", "...odo.", "..od...", ".od...."],
];

fn gem(e: Element, p: Vec2, r: f32) {
    let (c, u) = (el_color(e), r * 2.0 / 7.0);
    for (y, row) in GEMS[e as usize].iter().enumerate() {
        for (x, ch) in row.bytes().enumerate() {
            let shade = match ch {
                b'o' => c,
                b'l' => mix(c, INK, 0.6),
                b'd' => mix(c, OUT, 0.4),
                _ => continue,
            };
            draw_rectangle(p.x - r + x as f32 * u, p.y - r + y as f32 * u, u + 0.5, u + 0.5, shade);
        }
    }
}

/// `ghost` is where the bar was a moment ago; it shows the chunk just lost.
fn bar(r: Rect, frac: f32, ghost: f32, c: Color) {
    block(grow(r, 3.0), OUT);
    block(r, WELL);
    for (f, c) in [(ghost, mix(c, INK, 0.65)), (frac, c)] {
        if f > 0.0 {
            block(Rect::new(r.x, r.y, (r.w * f.min(1.0)).max(8.0), r.h), c);
        }
    }
}

pub fn status(run: &Run) {
    plate(Rect::new(24.0, 14.0, 1232.0, 44.0), CARD);
    sprite(HEROES[run.hero].sprite, 36.0, 20.0, 32.0, WHITE);
    text(HEROES[run.hero].name, 78.0, 43.0, 22.0, INK);
    let mut x = 260.0;
    for (label, value, c) in [
        ("HP", format!("{}/{}", run.hp, run.max_hp()), GOOD),
        ("Shards", run.shards.to_string(), GOLD_C),
        ("Floor", format!("{}/{}", run.floor + 1, FLOORS), INK),
        ("Charms", run.charms.len().to_string(), ACCENT),
    ] {
        text(label, x, 43.0, 18.0, MUTED);
        let w = measure(label, 18.0).width;
        text(&value, x + w + 8.0, 43.0, 20.0, c);
        x += 190.0;
    }
}


fn rarity_color(r: Rarity) -> Color {
    match r {
        Rarity::Common => rgb(0xb3a7bd),
        Rarity::Rare => rgb(0x5fb0ff),
        Rarity::Legendary => rgb(0xffb347),
    }
}

/// A charm's picture on a small plate edged in its rarity colour.
fn charm_icon(id: usize, r: Rect, lit: f32) {
    let c = &CHARMS[id];
    block(grow(r, 3.0), mix(rarity_color(c.rarity), INK, lit * 0.7));
    block(r, mix(WELL, GOLD_C, lit * 0.45));
    let pad = r.w * 0.12;
    sprite(c.icon.0, r.x + pad, r.y + pad, r.w - 2.0 * pad, rgb(c.icon.1));
}

/// Where charm `k` of `n` sits in the battle's charm bar.
fn charm_slot(k: usize, n: usize) -> Rect {
    let step = 38.0f32.min(396.0 / n.max(1) as f32);
    Rect::new(30.0 + k as f32 * step, 16.0, 32.0, 32.0)
}

/// Your charms along the top of a battle. Each one jumps and glows when it takes effect;
/// hovering one shows what it does.
fn charm_bar(charms: &[usize]) {
    let now = get_time();
    let times = fx(|fx| fx.charm_t.clone());
    let mut tip = None;
    for (k, &id) in charms.iter().enumerate() {
        let since = times.get(&id).map_or(9.0, |t| (now - t) as f32);
        let lit = (1.0 - since / 0.7).max(0.0);
        let r = charm_slot(k, charms.len());
        let jump = if since < 0.35 { (since / 0.35 * std::f32::consts::PI).sin() * 7.0 } else { 0.0 };
        let r = Rect::new(r.x, r.y - jump, r.w, r.h);
        if lit > 0.0 {
            block(grow(r, 6.0 + lit * 4.0), fade(GOLD_C, lit * 0.6));
        }
        charm_icon(id, r, lit);
        if r.contains(mouse()) {
            tip = Some((id, r));
        }
    }
    if let Some((id, r)) = tip {
        let c = &CHARMS[id];
        let panel = Rect::new(r.x.min(VIEW.x - 330.0), r.bottom() + 12.0, 310.0, 88.0);
        plate(panel, CARD);
        text(c.name, panel.x + 14.0, panel.y + 24.0, 16.0, INK);
        right(c.rarity.name(), panel.right() - 14.0, panel.y + 24.0, 16.0, rarity_color(c.rarity));
        wrapped(c.desc, panel.x + 14.0, panel.y + 48.0, panel.w - 28.0, 16.0, MUTED);
    }
}

fn combo_icons(c: &Combo, x: f32, y: f32) -> f32 {
    let mut cx = x + 9.0;
    for &e in c.needs {
        gem(e, vec2(cx, y + 9.0), 8.0);
        cx += 20.0;
    }
    cx
}

pub fn heroes() -> Option<usize> {
    stamped("MINEBOUND", 640.0, 88.0, 68.0, INK);
    centered("Sweep your boards for power. Spend it before theirs spends you.", 640.0, 122.0, 20.0, MUTED);
    let mut chosen = None;
    let t = get_time() as f32;
    for (i, h) in HEROES.iter().enumerate() {
        let r = Rect::new(52.0 + i as f32 * 400.0, 152.0, 376.0, 440.0);
        if card(r, true) {
            chosen = Some(i);
        }
        let x = r.x + 24.0;
        sprite(h.sprite, r.right() - 84.0, r.y + 10.0 + (t * 3.0 + i as f32).sin() * 3.0, 64.0, WHITE);
        text(h.name, x, r.y + 46.0, 32.0, INK);
        text(&format!("HP {}    Power {}", h.hp, h.power), x, r.y + 74.0, 19.0, GOLD_C);
        for (k, &e) in h.elements.iter().enumerate() {
            let ex = x + k as f32 * 110.0;
            gem(e, vec2(ex + 11.0, r.y + 100.0), 11.0);
            text(e.name(), ex + 28.0, r.y + 106.0, 18.0, INK);
        }
        let mut y = wrapped(h.passive_desc, x, r.y + 146.0, r.w - 48.0, 17.0, INK) + 14.0;
        text("SKILLS", x, y, 15.0, ACCENT);
        y += 14.0;
        for c in combos_for(&h.elements) {
            text(c.name, combo_icons(c, x, y), y + 15.0, 18.0, MUTED);
            text(c.desc, x, y + 33.0, 15.0, MUTED);
            y += 46.0;
        }
    }
    for (k, line) in [
        "Clear every safe tile around a crystal to claim a stack of its element. Your first flip on each board is never a mine.",
        "Safe flips in a row build SURGE: up to double damage this turn. A mine ends your turn and breaks the crystals it touches.",
        "Drag gems into a skill's sockets to cast it, or onto the enemy to Strike. Click a lit skill to cast at once. Right click flags.",
    ]
    .iter()
    .enumerate()
    {
        centered(line, 640.0, 634.0 + k as f32 * 26.0, 17.0, MUTED);
    }
    chosen
}

const TILE: f32 = 60.0;
const STEP: f32 = 64.0;
const BOARD_Y: f32 = 226.0;
/// where each fighter stands in the arena
const STAND: [Vec2; 2] = [vec2(548.0, 138.0), vec2(732.0, 138.0)];
const SHOT_TIME: f32 = 0.2;

fn hint_colour(n: usize) -> Color {
    [MUTED, rgb(0x9bd8ff), rgb(0xa5e88f), rgb(0xffb070)][n.min(3)]
}

/// Draws the battle and handles input. Returns true once it is over and the player clicks Continue.
pub fn battle_screen(b: &mut Battle, floor: usize, charms: &[usize], ai_timer: &mut f32) -> bool {
    let over = b.over();
    let mine = b.active == 0 && !over;
    plate(Rect::new(452.0, 12.0, 376.0, 36.0), CARD);
    centered(&format!("Floor {}    Wave {}/{}    Turn {}", floor + 1, b.wave, b.waves, b.turn), 640.0, 37.0, 20.0, INK);

    // compare with last frame: HP changes become floating numbers and a shake, new stacks bounce in,
    // and every skill that landed becomes a shot across the arena
    let now = get_time();
    for e in std::mem::take(&mut b.events) {
        match e {
            Event::Flip => play(Sfx::Flip),
            Event::Claim => play(Sfx::Claim),
            Event::Mine => {
                play(Sfx::Mine);
                quake(11.0);
                flash(BAD);
            }
            Event::Cast { from, name, element, miss } => {
                fx(|fx| {
                    fx.shots.push((from, element, name, now));
                    if miss {
                        fx.floats.push((STAND[1 - from] - vec2(0.0, 30.0), "MISS".into(), MUTED, now));
                    } else if name != "Strike" {
                        fx.banner = Some((name, element, from, now));
                    }
                });
                play(match (miss, from, name) {
                    (true, ..) => Sfx::Miss,
                    (_, 1, _) => Sfx::Hurt,
                    (_, _, "Strike") => Sfx::Strike,
                    _ => Sfx::Skill,
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

    // gems slotted into skill sockets stay in your stacks until the skill fires; forget any the
    // battle has since taken away (spent by Unleash, snatched by the enemy)
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
    let mut drag = fx(|fx| fx.drag).filter(|d| b.active == 0 && !over && free.contains(&d.0));
    let dragged = drag.map(|d| b.f[0].stacks[d.0]);
    let mut felled = false;
    let (shake, ghost, stack_t) = fx(|fx| {
        if fx.live && fx.wave != b.wave {
            fx.pops.clear(); // fresh boards
            felled = true;
        }
        if over && !fx.ended {
            fx.ended = true;
            felled |= b.won();
            play(if b.won() { Sfx::Win } else { Sfx::Lose });
        }
        let mut shake = [0.0f32; 2];
        for s in 0..2 {
            let f = &b.f[s];
            let (hp, n) = (f.hp.max(0), f.stacks.len());
            let fresh = !fx.live || (s == 1 && fx.wave != b.wave);
            if fresh {
                fx.ghost[s] = hp as f32;
            } else if hp != fx.hp[s] {
                let d = hp - fx.hp[s];
                fx.floats.push((STAND[s] - vec2(0.0, 30.0), format!("{d:+}"), if d < 0 { BAD } else { GOOD }, now));
                if d < 0 {
                    fx.hit[s] = now;
                }
            }
            if !fresh {
                for k in fx.stacks[s]..n.min(16) {
                    fx.stack_t[s][k] = now;
                }
            }
            fx.hp[s] = hp;
            fx.stacks[s] = n;
            fx.ghost[s] += (hp as f32 - fx.ghost[s]) * (get_frame_time() * 3.0).min(1.0);
            let since = (now - fx.hit[s]) as f32;
            if since < 0.35 {
                shake[s] = (since * 75.0).sin() * 7.0 * (1.0 - since / 0.35);
            }
        }
        fx.live = true;
        fx.wave = b.wave;
        (shake, fx.ghost, fx.stack_t)
    });
    if felled {
        // the enemy bursts apart
        play(Sfx::Kill);
        quake(13.0);
        flash(INK);
        burst(STAND[1], 60, 380.0, 12.0, INK);
        burst(STAND[1], 40, 260.0, 9.0, BAD);
    }
    let pulse = (now as f32 * 4.0).sin() * 0.5 + 0.5;

    let (mut reveal, mut flag, mut swap, mut strike, mut cast) = (None, None, None, None, None);
    for s in 0..2 {
        let panel = Rect::new(if s == 0 { 24.0 } else { 828.0 } + shake[s], 64.0, 428.0, 640.0);
        let side = if s == 0 { GOOD } else { BAD };
        if b.active == s && !over {
            block(grow(panel, 8.0), mix(side, OUT, pulse * 0.5));
        }
        plate(panel, CARD);
        let x = panel.x + 22.0;
        let f = &b.f[s];
        sprite(f.sprite, x, 72.0, 36.0, WHITE);
        text(&f.name, x + 46.0, 100.0, 26.0, INK);
        if b.active == s && !over {
            right(if s == 0 { "YOUR TURN" } else { "ENEMY TURN" }, x + 384.0, 98.0, 15.0, side);
        }
        text(&f.desc, x, 123.0, 14.0, MUTED);
        bar(Rect::new(x, 132.0, 384.0, 18.0), f.hp as f32 / f.max_hp as f32, ghost[s] / f.max_hp as f32, side);
        stamped(&format!("{} / {}", f.hp.max(0), f.max_hp), x + 192.0, 147.0, 15.0, INK);
        let mut st = format!("Power {}", f.power);
        for (label, v) in [("Shield", f.shield), ("Armor", f.mods.armor), ("Evade", f.evades_left), ("Burn", f.burn), ("Weak", f.weak), ("Duds", f.duds)] {
            if v > 0 {
                st += &format!("    {label} {v}");
            }
        }
        text(&st, x, 174.0, 16.0, GOLD_C);

        // one tab per board; the number is how many stacks of that element are held
        for (k, board) in f.boards.iter().enumerate() {
            let r = Rect::new(x + k as f32 * 76.0, 184.0, 70.0, 32.0);
            let hot = s == 0 && mine && r.contains(mouse());
            block(r, if k == f.cur { ACCENT } else if hot { CARD_HI } else { WELL });
            gem(board.element, vec2(r.x + 22.0, r.y + 16.0), 10.0);
            let held = f.stacks.iter().filter(|&&e| e == board.element).count();
            text(&held.to_string(), r.x + 42.0, r.y + 22.0, 18.0, if k == f.cur { OUT } else { INK });
            if hot && clicked() {
                swap = Some(k);
            }
        }
        let board = f.board();
        right(&format!("Reveals {}    Mines {}", f.reveals_left, board.mines), x + 384.0, 206.0, 15.0, MUTED);

        block(Rect::new(x - 6.0, BOARD_Y - 6.0, 6.0 * STEP + 8.0, 6.0 * STEP + 8.0), OUT);
        let tint = el_color(board.element);
        for (i, t) in board.tiles.iter().enumerate() {
            let r = Rect::new(x + (i % W) as f32 * STEP, BOARD_Y + (i / W) as f32 * STEP, TILE, TILE);
            let mid = r.center();
            // tiles around a crystal that can still be claimed carry a hint of its colour
            let near = Board::neighbors(i).any(|c| board.live(c));
            let key = (s, f.cur, i);
            if t.rock {
                sprite(59, r.x, r.y, TILE, rgb(0x5a5266));
                sprite(42, r.x + 8.0, r.y + 8.0, 44.0, rgb(0x8a8296));
            } else if t.crystal {
                block(r, WELL);
                if t.claimed {
                    if !fx(|fx| fx.pops.contains_key(&key)) {
                        // just claimed: it shatters and the shards fly to the stack row
                        let slot = vec2(x + 15.0 + (f.stacks.len().saturating_sub(1) as f32 * 34.0).min(354.0), 633.0);
                        stream(mid, slot, 16, tint);
                        burst(mid, 10, 150.0, 6.0, mix(tint, INK, 0.5));
                    }
                    let k = pop(key);
                    gem(board.element, mid, 12.0 * k);
                    draw_rectangle(r.x, r.y, r.w, r.h, fade(WELL, 0.55));
                    stamped("+1", mid.x, mid.y + 8.0, 22.0 * k, INK);
                } else if t.broken {
                    sprite(24, r.x + 6.0, r.y + 6.0, 48.0, mix(WHITE, BAD, 0.5));
                } else {
                    unpop(key);
                    block(r, mix(WELL, tint, 0.2));
                    // each crystal bobs on its own beat
                    gem(board.element, mid - vec2(0.0, 6.0 + (now as f32 * 2.6 + i as f32 * 1.7).sin() * 2.0), 17.0);
                    // tiles still to clear before it pays out
                    centered(&board.left_around(i).to_string(), mid.x, r.y + 56.0, 14.0, INK);
                }
            } else if !t.revealed {
                unpop(key);
                let hot = s == 0 && mine && r.contains(mouse());
                let lift = if hot { 4.0 } else { 0.0 };
                sprite(40, r.x, r.y - lift, TILE, if near { mix(WHITE, tint, 0.45) } else { WHITE });
                if hot {
                    draw_rectangle(r.x, r.y - lift, TILE, TILE, fade(INK, 0.18));
                }
                if t.flagged {
                    stamped("!", mid.x, mid.y + 12.0 - lift, 36.0, GOLD_C);
                }
                if hot && clicked() && !t.flagged {
                    reveal = Some(i);
                }
                if hot && is_mouse_button_pressed(MouseButton::Right) {
                    flag = Some(i);
                }
            } else if t.mine {
                if !fx(|fx| fx.pops.contains_key(&key)) {
                    burst(mid, 36, 300.0, 10.0, BAD);
                    burst(mid, 18, 190.0, 7.0, GOLD_C);
                }
                let q = scaled(r, pop(key));
                sprite(48, q.x, q.y, q.w, mix(BAD, OUT, 0.2));
                draw_rectangle(mid.x - 12.0, mid.y - 12.0, 24.0, 24.0, OUT);
                draw_rectangle(mid.x - 16.0, mid.y - 4.0, 32.0, 8.0, OUT);
                draw_rectangle(mid.x - 4.0, mid.y - 16.0, 8.0, 32.0, OUT);
                draw_rectangle(mid.x - 8.0, mid.y - 8.0, 6.0, 6.0, BAD);
            } else {
                let q = scaled(r, pop(key));
                sprite(48, q.x, q.y, q.w, rgb(0x6f6478));
                let n = board.adjacent(i);
                if n > 0 {
                    stamped(&n.to_string(), mid.x, mid.y + 11.0, 30.0, hint_colour(n));
                }
            }
        }

        // held stacks: yours can be dragged into a skill's sockets, or onto the enemy to Strike.
        // Gems already slotted into a socket are shown there instead. The row squeezes when you hoard.
        let row: Vec<usize> = if s == 0 { free.clone() } else { (0..f.stacks.len()).collect() };
        let step = 34.0f32.min(354.0 / row.len().max(1) as f32);
        if f.stacks.is_empty() {
            text("no stacks yet", x, 639.0, 15.0, MUTED);
        }
        for (j, &k) in row.iter().enumerate() {
            let p = vec2(x + 15.0 + j as f32 * step, 633.0);
            let slot = Rect::new(p.x - 15.0, p.y - 15.0, 30.0, 30.0);
            let held = s == 0 && drag.is_some_and(|d| d.0 == k);
            let hot = s == 0 && mine && drag.is_none() && slot.contains(mouse());
            block(slot, if hot { ACCENT } else { WELL });
            if !held {
                gem(f.stacks[k], p, 11.0 * bounce(stack_t[s][k.min(15)]));
            }
            if hot && clicked() {
                drag = Some((k, mouse()));
            }
        }
    }

    // the arena: both fighters, skills flying between them, surge underneath
    let arena = Rect::new(468.0, 64.0, 344.0, 152.0);
    plate(arena, WELL);
    let shots: Vec<(usize, Element, &'static str, f64)> = fx(|fx| {
        fx.shots.retain(|s| now - s.3 < 1.0);
        fx.shots.clone()
    });
    let hit_at: [f64; 2] = fx(|fx| fx.hit);
    for s in 0..2 {
        let toward = if s == 0 { 1.0 } else { -1.0 };
        let lunge = shots.iter().filter(|c| c.0 == s).map(|c| (now - c.3) as f32 / SHOT_TIME).filter(|p| *p < 1.0).last();
        let since_hit = (now - hit_at[s]) as f32;
        let mut at = STAND[s] + vec2(lunge.map_or(0.0, |p| (p * std::f32::consts::PI).sin() * 18.0) * toward, 0.0);
        at.y += (now as f32 * 3.0 + s as f32 * 2.0).sin() * 3.0;
        if since_hit < 0.3 {
            at.x += (since_hit * 90.0).sin() * 5.0;
        }
        draw_rectangle(STAND[s].x - 34.0, STAND[s].y + 42.0, 68.0, 8.0, OUT);
        let dead = b.f[s].hp <= 0;
        let tint = if dead { fade(MUTED, 0.35) } else if since_hit < 0.18 { BAD } else { WHITE };
        sprite(b.f[s].sprite, at.x - 44.0, at.y - 44.0, 88.0, tint);
    }
    for &(from, element, name, t0) in &shots {
        let p = (now - t0) as f32 / SHOT_TIME;
        let (a, z) = (STAND[from], STAND[1 - from]);
        let big = name != "Strike";
        let step = get_frame_time() / SHOT_TIME;
        let along = |q: f32| a + (z - a) * q - vec2(0.0, (q * std::f32::consts::PI).sin() * 26.0);
        if p < 1.0 {
            if big {
                // a comet: fading copies trail behind the head
                for k in (1..6).rev() {
                    let q = p - k as f32 * 0.07;
                    if q > 0.0 {
                        gem(element, along(q), 22.0 - k as f32 * 3.0);
                    }
                }
            }
            gem(element, along(p), if big { 24.0 } else { 14.0 });
        } else if p - step < 1.0 {
            // the frame it lands
            let c = el_color(element);
            burst(z, if big { 44 } else { 12 }, if big { 360.0 } else { 200.0 }, if big { 11.0 } else { 7.0 }, c);
            if big {
                burst(z, 20, 220.0, 8.0, INK);
                quake(9.0);
                flash(c);
            }
        } else if big && p < 2.6 {
            // shockwave ring
            let q = (p - 1.0) / 1.6;
            let half = 24.0 + q * 150.0;
            draw_rectangle_lines(z.x - half, z.y - half, half * 2.0, half * 2.0, 14.0 * (1.0 - q), fade(el_color(element), 1.0 - q));
        }
        if name == "Strike" && p < 4.0 {
            stamped(name, 640.0, 88.0, 20.0, fade(el_color(element), (4.0 - p).min(1.0)));
        }
    }
    // a fighter at full surge burns with it
    for s in 0..2 {
        if b.active == s && !over && b.f[s].surge > 0 && b.f[s].surge >= b.surge_cap(s) && gen_range(0.0, 1.0) < 0.5 {
            let from = STAND[s] + vec2(gen_range(-34.0, 34.0), gen_range(10.0, 44.0));
            let (vel, life, size, color) = (vec2(gen_range(-12.0, 12.0), gen_range(-90.0, -40.0)), gen_range(0.4, 0.8), gen_range(4.0, 8.0), mix(GOLD_C, BAD, gen_range(0.0, 0.7)));
            fx(|fx| fx.parts.push(Part { pos: from, vel, born: now, life, size, color, grav: -80.0, home: None }));
        }
    }
    let surge = b.f[b.active].surge;
    for k in 0..b.surge_cap(b.active) {
        let lit = k < surge;
        let r = Rect::new(640.0 - 62.0 + k as f32 * 20.0, 196.0, 16.0, 12.0);
        block(grow(r, if lit { pulse * 2.0 } else { 0.0 }), if lit { mix(GOLD_C, BAD, k as f32 / 6.0) } else { CARD });
    }
    right("SURGE", 640.0 - 70.0, 207.0, 14.0, if surge > 0 { GOLD_C } else { MUTED });
    text(&format!("x{:.1}", 1.0 + (surge * b.surge_step(b.active)) as f32 / 100.0), 640.0 + 46.0, 208.0, 16.0, if surge > 0 { GOLD_C } else { MUTED });

    // skills: only the ones each side can actually make. Yours are buttons.
    let skills = Rect::new(468.0, 232.0, 344.0, 472.0);
    plate(skills, CARD);
    let x0 = skills.x + 22.0;
    text("YOUR SKILLS", x0, 258.0, 14.0, ACCENT);
    right("drag gems into the sockets", x0 + 300.0, 258.0, 13.0, MUTED);
    let ready = b.ready(0);
    let mut y = 268.0;
    // where a dragged gem may land: (skill row, skill)
    let mut targets: Vec<(Rect, &'static Combo)> = vec![];
    for c in combos_for(&b.f[0].elements) {
        let lit = ready.iter().any(|r| r.name == c.name);
        let slotted = sockets.get(c.name).cloned().unwrap_or_default();
        let r = Rect::new(x0 - 8.0, y - 2.0, 316.0, 42.0);
        let over_row = mine && r.contains(mouse());
        let takes = dragged.filter(|e| c.needs.contains(e) && !slotted.contains(e));
        let fill = match (takes, over_row) {
            (Some(e), true) => mix(CARD_HI, el_color(e), 0.5),
            (Some(e), false) => mix(CARD, el_color(e), 0.2),
            (None, true) if lit && dragged.is_none() => mix(CARD_HI, ACCENT, 0.45),
            _ if lit || !slotted.is_empty() => CARD_HI,
            _ => CARD,
        };
        block(r, fill);
        if lit {
            right(&b.hit_damage(0, c.needs.len(), Some(c)).to_string(), r.right() - 10.0, y + 26.0, 24.0, GOLD_C);
        }
        // one socket per element the skill needs: dark and ghosted until a gem sits in it
        let mut unslot = None;
        for (j, &e) in c.needs.iter().enumerate() {
            let sr = Rect::new(x0 + j as f32 * 28.0 - 2.0, y, 24.0, 24.0);
            let full = slotted.contains(&e);
            block(grow(sr, 2.0), OUT);
            block(sr, if full { mix(WELL, el_color(e), 0.3) } else { WELL });
            gem(e, sr.center(), if full { 10.0 } else { 7.0 });
            if !full {
                draw_rectangle(sr.x, sr.y, sr.w, sr.h, fade(WELL, 0.65));
            }
            if full && mine && drag.is_none() && sr.contains(mouse()) {
                draw_rectangle_lines(sr.x, sr.y, sr.w, sr.h, 2.0, INK);
                if clicked() {
                    unslot = Some(e);
                }
            }
        }
        let cx = x0 + c.needs.len() as f32 * 28.0 + 4.0;
        text(c.name, cx, y + 17.0, 18.0, if lit || !slotted.is_empty() { INK } else { MUTED });
        text(c.desc, x0, y + 37.0, 15.0, if lit { GOLD_C } else { MUTED });
        if let Some(e) = unslot {
            // take a gem back out of its socket
            if let Some(v) = sockets.get_mut(c.name) {
                v.retain(|&x| x != e);
            }
            play(Sfx::Click);
        } else if over_row && lit && drag.is_none() && clicked() {
            // a click casts it straight away, with whatever gems it takes
            cast = Some(c);
        }
        targets.push((r, c));
        y += 46.0;
    }

    // a dragged gem lands where the button comes up: in a skill's socket, or on the enemy as a Strike.
    // Let go without moving and it is a plain click: Strike.
    let enemy = Rect::new(828.0, 64.0, 428.0, 640.0);
    let at_enemy = enemy.contains(mouse()) || Rect::new(640.0, 64.0, 172.0, 152.0).contains(mouse());
    if let Some((k, from)) = drag {
        let e = b.f[0].stacks[k];
        if at_enemy {
            draw_rectangle(enemy.x, enemy.y, enemy.w, enemy.h, fade(BAD, 0.12));
            stamped("Strike", enemy.center().x, 120.0 + 28.0, 26.0, BAD);
        }
        if !is_mouse_button_down(MouseButton::Left) {
            let target = targets.iter().find(|(r, c)| r.contains(mouse()) && c.needs.contains(&e));
            if (mouse() - from).length() < 6.0 || at_enemy {
                strike = Some(k);
            } else if let Some(&(_, c)) = target {
                let v = sockets.entry(c.name).or_default();
                if !v.contains(&e) {
                    v.push(e);
                    play(Sfx::Claim);
                }
                if c.needs.iter().all(|n| v.contains(n)) {
                    sockets.remove(c.name);
                    cast = Some(c);
                }
            }
            drag = None;
        }
    }
    if let Some(c) = cast {
        sockets.remove(c.name);
    }
    y += 22.0;
    text("ENEMY SKILLS", x0, y, 14.0, ACCENT);
    y += 10.0;
    let ready = b.ready(1);
    for c in combos_for(&b.f[1].elements) {
        let lit = ready.iter().any(|r| r.name == c.name);
        let cx = combo_icons(c, x0, y);
        text(c.name, cx, y + 15.0, 16.0, if lit { INK } else { MUTED });
        right(c.desc, x0 + 300.0, y + 15.0, 13.0, MUTED);
        y += 24.0;
    }
    let first = b.log.len().saturating_sub(4);
    for (k, l) in b.log[first..].iter().enumerate() {
        text(l, x0, 600.0 + k as f32 * 19.0, 15.0, MUTED);
    }
    let note = if b.pressure() > 0 {
        format!("Overpressure: every skill +{}", b.pressure())
    } else {
        format!("Overpressure begins after turn {PRESSURE_TURN}")
    };
    centered(&note, 640.0, 692.0, 13.0, if b.pressure() > 0 { GOLD_C } else { MUTED });

    let unleash = button(Rect::new(46.0, 656.0, 226.0, 36.0), &format!("Unleash all: {}", b.attack_damage(0)), mine && !b.f[0].stacks.is_empty(), true);
    let pass = button(Rect::new(284.0, 656.0, 146.0, 36.0), "End turn", mine, false);
    if unleash {
        b.unleash();
    } else if pass {
        b.end_turn();
    } else if let Some(c) = cast {
        b.cast(c);
    } else if let Some(k) = strike {
        b.strike(k);
    } else if let Some(k) = swap {
        b.swap(k);
    } else if let Some(i) = reveal {
        b.reveal(i);
    } else if let Some(i) = flag {
        b.toggle_flag(i);
    }

    if !over && b.active == 1 {
        // the enemy moves one visible step at a time, briskly
        *ai_timer += get_frame_time();
        if *ai_timer > 0.3 {
            *ai_timer = 0.0;
            b.ai_step();
        }
    } else {
        *ai_timer = 0.0;
    }

    particles();
    charm_bar(charms);
    if let Some((k, _)) = drag {
        if let Some(&e) = b.f[0].stacks.get(k) {
            gem(e, mouse(), 15.0);
        }
    }
    fx(|fx| {
        fx.sockets = sockets;
        fx.drag = drag;
    });
    let (banner, flashed) = fx(|fx| (fx.banner, fx.flash));
    if let Some((name, element, from, t0)) = banner {
        // a combo's name slams across the whole screen, entering from the caster's side
        let p = (now - t0) as f32 / 0.8;
        if p < 1.0 {
            let a = if p < 0.6 { 1.0 } else { 1.0 - (p - 0.6) / 0.4 };
            let c = el_color(element);
            let slide = (1.0 - (p / 0.18).min(1.0)).powi(2) * 500.0 * if from == 0 { -1.0 } else { 1.0 };
            draw_rectangle(-2000.0, 268.0, 5280.0, 84.0, fade(OUT, 0.8 * a));
            draw_rectangle(-2000.0, 262.0, 5280.0, 6.0, fade(c, a));
            draw_rectangle(-2000.0, 352.0, 5280.0, 6.0, fade(c, a));
            stamped(name, 640.0 + slide, 330.0, 60.0 + 30.0 * (1.0 - (p / 0.18).min(1.0)), fade(c, a));
        }
    }
    if let Some((c, t0)) = flashed {
        let p = (now - t0) as f32 / 0.18;
        if p < 1.0 {
            draw_rectangle(-2000.0, -2000.0, 5280.0, 5000.0, fade(c, 0.32 * (1.0 - p)));
        }
    }
    fx(|fx| {
        fx.floats.retain(|f| now - f.3 < 1.0);
        for (at, label, c, t0) in &fx.floats {
            let p = (now - t0) as f32;
            stamped(label, at.x, at.y - p * 34.0, 30.0 + 12.0 * (1.0 - p), fade(*c, 1.5 - p * 1.5));
        }
    });

    if over {
        let r = Rect::new(440.0, 270.0, 400.0, 180.0);
        block(grow(r, 10.0), if b.won() { GOOD } else { BAD });
        plate(r, BG);
        stamped(if b.won() { "Victory" } else { "Defeated" }, 640.0, 344.0, 50.0, if b.won() { GOOD } else { BAD });
        return button(Rect::new(540.0, 380.0, 200.0, 40.0), "Continue", true, true);
    }
    false
}

fn node_look(k: NodeKind) -> (Color, usize) {
    match k {
        NodeKind::Battle => (rgb(0xc9584f), 104),
        NodeKind::Elite => (rgb(0x9a62d6), 118),
        NodeKind::Encounter => (rgb(0x4aa6b5), 85),
        NodeKind::Shop => (rgb(0xd9a441), 115),
        NodeKind::Rest => (rgb(0x57b36b), 66),
        NodeKind::Treasure => (rgb(0xd98445), 89),
        NodeKind::Boss => (rgb(0xb8324f), 111),
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
                draw_line(p.x, p.y, q.x, q.y, 8.0, OUT);
                draw_line(p.x, p.y, q.x, q.y, 4.0, if live { ACCENT } else { CARD_HI });
            }
        }
    }
    for (c, col) in run.map.iter().enumerate() {
        for (r, node) in col.iter().enumerate() {
            let p = node_at(run, c, r);
            let can = open.contains(&(c, r));
            let here = run.pos == Some((c, r));
            let hot = can && (mouse() - p).abs().max_element() < 36.0;
            let (colour, icon) = node_look(node.kind);
            let beat = if can { ((get_time() as f32 * 3.5 + r as f32).sin() * 3.0).round() } else { 0.0 };
            let half = if hot { 36.0 } else { 32.0 };
            let plate_r = Rect::new(p.x - half, p.y - half, half * 2.0, half * 2.0);
            if can || here {
                block(grow(plate_r, 8.0 + beat), if here { GOLD_C } else { INK });
            }
            plate(plate_r, if can || here { colour } else { mix(colour, BG, 0.6) });
            sprite(icon, p.x - 24.0, p.y - 24.0, 48.0, if can || here { WHITE } else { fade(WHITE, 0.45) });
            centered(if here { "You are here" } else { node.kind.name() }, p.x, p.y + 66.0, 16.0, if can || here { INK } else { MUTED });
            if hot && clicked() {
                play(Sfx::Click);
                chosen = Some((c, r));
            }
        }
    }

    let side = Rect::new(930.0, 108.0, 326.0, 588.0);
    plate(side, CARD);
    let x = side.x + 22.0;
    text("CHARMS", x, 140.0, 15.0, ACCENT);
    let mut tip = HEROES[run.hero].passive_desc;
    let rows = 22.0f32.min(460.0 / run.charms.len().max(1) as f32);
    for (k, &id) in run.charms.iter().enumerate() {
        let r = Rect::new(x - 8.0, 150.0 + k as f32 * rows, 298.0, rows);
        let hot = r.contains(mouse());
        if hot {
            tip = CHARMS[id].desc;
            block(r, CARD_HI);
        }
        let c = &CHARMS[id];
        sprite(c.icon.0, x, r.y + (rows - 18.0) / 2.0, 18.0, rgb(c.icon.1));
        text(c.name, x + 26.0, r.y + rows / 2.0 + 6.0, 16.0, if hot { INK } else { rarity_color(c.rarity) });
    }
    wrapped(tip, x, 628.0, 282.0, 16.0, INK);
    text(&format!("Seed {}    M: mute    N: next song", run.seed), x, 684.0, 14.0, MUTED);
    chosen
}

fn charm_card(r: Rect, id: usize, on: bool) -> bool {
    let c = &CHARMS[id];
    let edge = rarity_color(c.rarity);
    // the rarity colour frames the whole card; legendaries shimmer
    let shine = if c.rarity == Rarity::Legendary { ((get_time() as f32 * 3.0).sin() * 0.5 + 0.5) * 0.35 } else { 0.0 };
    block(grow(r, 7.0), mix(edge, INK, shine));
    let hit = card(r, on);
    let bob = (get_time() as f32 * 2.5 + id as f32).sin() * 2.0;
    charm_icon(id, Rect::new(r.x + 20.0, r.y + 22.0 + bob, 64.0, 64.0), 0.0);
    text(c.name, r.x + 100.0, r.y + 52.0, 24.0, INK);
    text(c.rarity.name(), r.x + 100.0, r.y + 78.0, 16.0, edge);
    wrapped(c.desc, r.x + 24.0, r.y + 124.0, r.w - 48.0, 16.0, INK);
    hit
}

pub fn pick(options: &[usize]) -> Option<usize> {
    stamped("Choose a charm", 640.0, 150.0, 40.0, INK);
    let mut chosen = None;
    for (k, &id) in options.iter().enumerate() {
        if charm_card(Rect::new(130.0 + k as f32 * 350.0, 220.0, 320.0, 240.0), id, true) {
            chosen = Some(id);
        }
    }
    chosen
}

/// Returns true when the player leaves.
pub fn shop(run: &mut Run, stock: &mut Vec<usize>) -> bool {
    stamped("Tinker's Stall", 640.0, 150.0, 40.0, INK);
    let mut bought = None;
    for (k, &id) in stock.iter().enumerate() {
        let r = Rect::new(130.0 + k as f32 * 350.0, 200.0, 320.0, 270.0);
        charm_card(r, id, false);
        let buy = Rect::new(r.x + 24.0, r.y + 206.0, r.w - 48.0, 40.0);
        let price = CHARMS[id].rarity.price();
        if button(buy, &format!("Buy for {price}"), run.shards >= price, true) && run.buy_charm(id) {
            play(Sfx::Coins);
            bought = Some(k);
        }
    }
    if let Some(k) = bought {
        stock.remove(k);
    }
    let can_heal = run.shards >= HEAL_PRICE && run.hp < run.max_hp();
    if button(Rect::new(130.0, 510.0, 420.0, 44.0), &format!("Patch up: +{HEAL_AMOUNT} HP for {HEAL_PRICE}"), can_heal, false) {
        run.buy_heal();
    }
    button(Rect::new(830.0, 510.0, 320.0, 44.0), "Leave", true, false)
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
    for (k, c) in e.choices.iter().enumerate() {
        if button(Rect::new(240.0, y + k as f32 * 60.0, 800.0, 44.0), c.label, run.can_afford(c), false) {
            *result = Some(run.choose(c));
        }
    }
    false
}

/// Returns true when the player asks for a new run.
pub fn end(run: &Run, won: bool) -> bool {
    sprite(HEROES[run.hero].sprite, 592.0, 130.0, 96.0, if won { WHITE } else { fade(MUTED, 0.5) });
    stamped(if won { "The last board is clear." } else { "Your run ends here." }, 640.0, 290.0, 52.0, if won { GOOD } else { BAD });
    let s = format!("{}    floor {} of {}    {} charms    seed {}", HEROES[run.hero].name, run.floor + 1, FLOORS, run.charms.len(), run.seed);
    centered(&s, 640.0, 340.0, 20.0, MUTED);
    button(Rect::new(540.0, 390.0, 200.0, 44.0), "New run", true, true)
}

/// A button for use inside an open panel, where `button` ignores clicks.
fn panel_button(r: Rect, label: &str, fill: Color, press: bool) -> bool {
    let hot = r.contains(mouse());
    block(Rect::new(r.x, r.y + 4.0, r.w, r.h), OUT);
    block(r, if hot { mix(fill, INK, 0.3) } else { fill });
    centered(label, r.center().x, r.y + r.h / 2.0 + 7.0, 20.0, OUT);
    if hot && press {
        play(Sfx::Click);
    }
    hot && press
}

/// The Retreat button in the run's status bar, and the question it asks. True once the player
/// confirms: the run is abandoned and the game goes back to the title.
pub fn retreat() -> bool {
    if !RETREAT.with(|r| r.get()) {
        if button(Rect::new(1078.0, 20.0, 140.0, 32.0), "Retreat", true, false) {
            RETREAT.with(|r| r.set(true));
        }
        return false;
    }
    let press = is_mouse_button_pressed(MouseButton::Left) && !SETTINGS.with(|s| s.get());
    draw_rectangle(-2000.0, -2000.0, 5280.0, 5000.0, fade(OUT, 0.6));
    let panel = Rect::new(390.0, 240.0, 500.0, 220.0);
    plate(panel, CARD);
    stamped("Retreat?", 640.0, 296.0, 34.0, INK);
    centered("You leave the expedition. This hero, its charms", 640.0, 336.0, 16.0, MUTED);
    centered("and its shards are lost.", 640.0, 358.0, 16.0, MUTED);
    let quit = panel_button(Rect::new(430.0, 392.0, 200.0, 40.0), "Retreat", BAD, press);
    // not Esc: the settings panel answers to Esc in the same frame
    let stay = panel_button(Rect::new(650.0, 392.0, 200.0, 40.0), "Keep going", ACCENT, press);
    if quit || stay {
        RETREAT.with(|r| r.set(false));
    }
    quit
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
            if panel_button(Rect::new(540.0, y, 40.0, 36.0), "-", CARD_HI, press) {
                *level = (*level - 1).max(0);
            }
            if panel_button(Rect::new(804.0, y, 40.0, 36.0), "+", CARD_HI, press) {
                *level = (*level + 1).min(10);
            }
            for k in 0..10 {
                block(Rect::new(592.0 + k as f32 * 21.0, y + 6.0, 16.0, 24.0), if k < *level { ACCENT } else { WELL });
            }
        }
        VOLUME.with(|v| v.set((music, sound)));
        centered("M mutes everything.  N skips to the next song.", 640.0, 440.0, 15.0, MUTED);
        toggle |= panel_button(Rect::new(560.0, 456.0, 160.0, 36.0), "Close", ACCENT, press);
    }
    if toggle {
        play(Sfx::Click);
        SETTINGS.with(|s| s.set(!open));
    }
}
