// no console window behind the game on Windows when it is double-clicked
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod battle;
mod ctl;
mod board;
mod data;
mod devlog;
mod rng;
mod run;
mod save;
mod sfx;
mod ui;
mod update;

use macroquad::audio::{load_sound_from_bytes, play_sound, set_sound_volume, stop_sound, PlaySoundParams, Sound};
use macroquad::prelude::*;
// named imports win over the prelude's glob: debug builds can be driven from a file
use ctl::{is_key_down, is_key_pressed, is_mouse_button_pressed};
use run::{Run, Stage, FLOORS};
use save::Save;
use ui::Go;

/// Which screen the game is on. A run carries the expedition its win opened, for the end screen.
enum Screen {
    Title,
    World,
    Run(Run, Stage, Option<usize>),
    /// debug builds: songs and animations on demand, with the song picked (if any)
    Gallery(Option<usize>),
}

/// Background music (CC0), read from assets/music beside the game: HoliznaCC0's albums for calm and
/// fight music, OpenGameArt tracks for bosses (credits in assets/music/License.txt).
/// (file name, length in seconds, mood: CALM on every screen but a fight, FIGHT, BOSS; place: the floor
/// (0-2), an ending's floor (3 + its index in ENDINGS), or MENUS for screens outside a run)
const TRACKS: [(&str, f64, u8, u8); 21] = [
    ("floor-1", 211.9, CALM, 0),
    ("cobwebs", 149.3, FIGHT, 0),
    ("hunted-by-the-evil", 131.6, BOSS, 0),
    ("floor-2", 164.0, CALM, 1),
    ("haunted-houses", 134.1, FIGHT, 1),
    ("the-confrontation", 79.6, BOSS, 1),
    ("floor-3", 202.0, CALM, 2),
    ("between-life-and-death", 163.7, FIGHT, 2),
    ("dark-castle", 96.0, BOSS, 2),
    // The Drowned Archive
    ("floor-4", 298.0, CALM, 3),
    ("nocturnal", 194.6, FIGHT, 3),
    ("sinister-abode", 147.0, BOSS, 3),
    // The Furnace Heart
    ("floor-5", 246.0, CALM, 4),
    ("burning-in-the-atmosphere", 264.8, FIGHT, 4),
    ("villain-emerges", 130.3, BOSS, 4),
    // The Silent Engine
    ("floor-6", 220.0, CALM, 5),
    ("little-green-men", 156.7, FIGHT, 5),
    ("castle-boss", 124.0, BOSS, 5),
    ("halloween-soundscape", 234.0, CALM, MENUS),
    ("an-ocean-in-outer-space", 132.0, CALM, MENUS),
    ("never-speak-of-the-devil", 145.9, CALM, MENUS),
];
/// The place of the title, the world map and the gallery.
const MENUS: u8 = 6;
const CALM: u8 = 0;
const FIGHT: u8 = 1;
const BOSS: u8 = 2;
/// Loudness of the music at the top of the volume slider.
const MUSIC_VOLUME: f32 = 0.5;
/// Seconds the old track takes to fade out when the mood changes.
const FADE: f64 = 0.8;

struct Music {
    playing: Option<Sound>,
    /// index in TRACKS of the track playing (or last tried)
    track: usize,
    /// when the current track ends (or when to try again, if it failed to load)
    until: f64,
    /// when the current track began fading out, if it is
    fading: Option<f64>,
    /// the next track's file, being read off the game thread (a read froze the game for ~30 ms)
    loading: Option<std::sync::mpsc::Receiver<std::io::Result<Vec<u8>>>>,
}

impl Music {
    fn new() -> Music {
        Music { playing: None, track: usize::MAX, until: 0.0, fading: None, loading: None }
    }

    /// Plays tracks of `want` (mood, place): the next one when the current one ends or N is pressed,
    /// and a fade to a new one when the mood or place changes. One track is held in memory at a time: a decoded track
    /// is tens of megabytes.
    /// `only`: play just this track (the gallery's pick), over and over.
    async fn tick(&mut self, want: (u8, u8), only: Option<usize>) {
        if let Some(rx) = &self.loading {
            let read = match rx.try_recv() {
                Err(std::sync::mpsc::TryRecvError::Empty) => return,
                r => r.ok(),
            };
            self.loading = None;
            let name = TRACKS[self.track].0;
            let sound = match read {
                Some(Ok(bytes)) => load_sound_from_bytes(&bytes).await.map_err(|e| e.to_string()),
                Some(Err(e)) => Err(e.to_string()),
                None => Err("reader thread died".into()),
            };
            match sound {
                Ok(s) => {
                    play_sound(&s, PlaySoundParams { looped: false, volume: ui::music_volume() * MUSIC_VOLUME });
                    self.playing = Some(s);
                    self.until = get_time() + TRACKS[self.track].1 + 1.5;
                }
                // no music folder next to the exe: stay silent, look again in a while
                Err(e) => {
                    eprintln!("music not loaded ({name}): {e}");
                    self.until = get_time() + 60.0;
                }
            }
            return;
        }
        let fits = |i: usize| only.map_or((TRACKS[i].2, TRACKS[i].3) == want, |o| o == i);
        let wrong = self.track < TRACKS.len() && !fits(self.track);
        if self.playing.is_some() && wrong && self.fading.is_none() {
            self.fading = Some(get_time());
        }
        let fade = self.fading.map_or(1.0, |t| 1.0 - (get_time() - t) / FADE);
        if let Some(s) = &self.playing {
            set_sound_volume(s, ui::music_volume() * MUSIC_VOLUME * fade.max(0.0) as f32);
        }
        if fade > 0.0 && !(self.playing.is_none() && wrong) && get_time() < self.until && !is_key_pressed(KeyCode::N) {
            return;
        }
        if let Some(old) = self.playing.take() {
            stop_sound(&old);
        }
        self.fading = None;
        // a random track for this mood and place, not the one just played (unless it is the only one)
        let pool: Vec<usize> = (0..TRACKS.len()).filter(|&i| fits(i)).collect();
        let fresh: Vec<usize> = pool.iter().copied().filter(|&i| i != self.track).collect();
        let from = if fresh.is_empty() { &pool } else { &fresh };
        self.track = from[rand::gen_range(0, from.len())];
        let (path, (tx, rx)) = (music_path(TRACKS[self.track].0), std::sync::mpsc::channel());
        std::thread::spawn(move || tx.send(std::fs::read(path)));
        self.loading = Some(rx);
    }
}

/// The music a screen calls for: (a fight's, a boss fight's or the calm between, and where it is).
fn mood(screen: &Screen) -> (u8, u8) {
    let Screen::Run(run, stage, _) = screen else { return (CALM, MENUS) };
    let place = run.ending().map_or(run.floor.min(FLOORS - 1), |e| FLOORS + e) as u8;
    match stage {
        Stage::Battle(b) if data::is_boss(&b.f[1].name) => (BOSS, place),
        Stage::Battle(_) => (FIGHT, place),
        _ => (CALM, place),
    }
}

/// assets/music beside the exe (installed game), else from the working folder (cargo run).
fn music_path(name: &str) -> String {
    let file = format!("assets/music/{name}.ogg");
    let beside_exe = std::env::current_exe().ok().and_then(|exe| Some(exe.parent()?.join(&file)));
    beside_exe.filter(|p| p.exists()).map_or(file, |p| p.to_string_lossy().into_owned())
}

const ART: &[u8] = include_bytes!("../assets/tiny-dungeon/tilemap_packed.png");
/// A mimic chest from Tiny Dungeon is the window and taskbar icon; assets/icon.ico (shortcuts)
/// shows the same sprite.
const ICON_SPRITE: usize = 92;

/// The icon sprite scaled up without smoothing to `size` x `size` RGBA (`N` = size * size * 4).
fn icon_pixels<const N: usize>(sheet: &Image, size: usize) -> [u8; N] {
    let mut out = [0; N];
    for (i, px) in out.chunks_exact_mut(4).enumerate() {
        let x = ICON_SPRITE % 12 * 16 + i % size * 16 / size;
        let y = ICON_SPRITE / 12 * 16 + i / size * 16 / size;
        let at = (y * sheet.width as usize + x) * 4;
        px.copy_from_slice(&sheet.bytes[at..at + 4]);
    }
    out
}

fn conf() -> macroquad::conf::Conf {
    let icon = Image::from_file_with_format(ART, None).ok().map(|sheet| miniquad::conf::Icon {
        small: icon_pixels(&sheet, 16),
        medium: icon_pixels(&sheet, 32),
        big: icon_pixels(&sheet, 64),
    });
    let mut conf: macroquad::conf::Conf = Conf {
        window_title: "Minebound".into(),
        window_width: 1280,
        window_height: 720,
        window_resizable: true,
        high_dpi: true,
        sample_count: 4,
        icon,
        ..Default::default()
    }
    .into();
    // each draw call keeps a GPU buffer this many vertices big for good; its 5000 indices fill at
    // about 3400 vertices of quads, so macroquad's 10000 left 60% of every buffer unused
    conf.draw_call_vertex_capacity = 4000;
    conf
}

#[macroquad::main(conf)]
async fn main() {
    devlog::catch_panics();
    devlog::log(|| format!("launch {:?}", std::env::args().collect::<Vec<_>>()));
    // `minebound <seed>` replays a run; otherwise the clock picks one
    let fixed_seed: Option<u64> = std::env::args().nth(1).and_then(|s| s.parse().ok());
    // Pirata One for titles and IBM Plex Mono for everything else (both SIL OFL), built into the exe
    ui::set_fonts(
        load_ttf_font_from_bytes(include_bytes!("../assets/fonts/pirata-one.ttf")).ok(),
        load_ttf_font_from_bytes(include_bytes!("../assets/fonts/ibm-plex-mono.ttf")).ok(),
    );
    // Tiny Dungeon by Kenney (CC0), built into the exe
    // characters and icons: game-icons.net glyphs (CC BY 3.0) baked into white silhouettes;
    // spell effects: Kenney's Particle Pack (CC0)
    ui::set_art(
        Texture2D::from_file_with_format(ART, None),
        Texture2D::from_file_with_format(include_bytes!("../assets/sigils/sigils-mono.png"), None),
        Texture2D::from_file_with_format(include_bytes!("../assets/fx/particles.png"), None),
    );
    let mut sounds = vec![];
    for (_, files) in sfx::SOUNDS {
        let mut takes = vec![];
        for bytes in files {
            if let Ok(sound) = load_sound_from_bytes(bytes).await {
                takes.push(sound);
            }
        }
        sounds.push(takes);
    }
    ui::set_sounds(sounds);
    rand::srand((miniquad::date::now() * 1000.0) as u64);
    let mut music = Music::new();
    let mut save = Save::load();
    ui::set_volume(save.volume);
    ui::set_perf_hud(save.perf_hud);
    if !ctl::on() {
        update::check();
    }
    // the newest open expedition and the first hero
    let mut pick = (save.unlocked - 1, 0);
    let mut screen = Screen::Title;
    let mut ai_timer = 0.0;
    loop {
        clear_background(ui::BG);
        ctl::begin_frame();
        ui::begin_frame();
        let only = if let Screen::Gallery(t) = screen { t } else { None };
        music.tick(mood(&screen), only).await;
        let mut quit_run = false;
        // days since 1970 (UTC): the daily descent changes as this does
        let today = (miniquad::date::now() / 86_400.0) as u64;
        let mut to = None;
        match &mut screen {
            Screen::Title => match ui::title(save.deeds) {
                Some(Go::Play) => to = Some(Screen::World),
                Some(Go::Back) => break,
                Some(Go::Gallery) => to = Some(Screen::Gallery(None)),
                Some(Go::Daily) | None => {}
            },
            Screen::Gallery(track) => match ui::gallery(&TRACKS, *track) {
                Some(ui::Shown::Track(i)) => *track = Some(i),
                Some(ui::Shown::Fight(ending)) => {
                    let (mut run, _) = Run::new((miniquad::date::now() * 1000.0) as u64, pick.1, pick.0);
                    let stage = if ending { run.debug_endings() } else { run.debug_last_fight() };
                    to = Some(Screen::Run(run, stage, None));
                }
                Some(ui::Shown::Back) => to = Some(Screen::Title),
                None => {}
            },
            Screen::World => match ui::world(&mut save, &mut pick, today) {
                Some(Go::Play) => {
                    let seed = fixed_seed.unwrap_or((miniquad::date::now() * 1000.0) as u64);
                    let (mut run, mut stage) = Run::with_deeds(seed, pick.1, pick.0, save.deeds);
                    if let Some(relic) = save.take_crucible() {
                        run.equip_crucible(Some(relic));
                        save.store();
                    }
                    if let Some(wh) = run.arm(save.wagers_on) {
                        stage = wh;
                    }
                    // debug builds only: MINEBOUND_ENDINGS=1 starts every run as Ctrl+E does, for when keys can't be sent
                    if cfg!(debug_assertions) && std::env::var_os("MINEBOUND_ENDINGS").is_some() {
                        stage = run.debug_endings();
                    }
                    // debug builds only: MINEBOUND_MASTERED=1 starts with four rites at level 3, to see the masteries
                    // (MINEBOUND_RITES=a,b,.. picks them by index in the hero's pool)
                    if cfg!(debug_assertions) && std::env::var_os("MINEBOUND_MASTERED").is_some() {
                        let all = minebound_pool(pick.1);
                        let want: Vec<usize> = std::env::var("MINEBOUND_RITES").ok().map(|v| v.split(',').filter_map(|x| x.parse().ok()).collect()).unwrap_or_else(|| (0..4).collect());
                        run.held = want.iter().filter_map(|&i| all.get(i).copied()).take(4).collect();
                        run.ups = run.rites().into_iter().flat_map(|id| [id, id]).collect();
                        run.debug = true;
                    }
                    to = Some(Screen::Run(run, stage, None));
                }
                Some(Go::Daily) => {
                    let (run, stage) = Run::new_daily(today);
                    to = Some(Screen::Run(run, stage, None));
                }
                Some(Go::Back) => to = Some(Screen::Title),
                Some(Go::Gallery) | None => {}
            },
            Screen::Run(run, stage, opened) => {
                // debug builds only, to reach any screen quickly: F9, Ctrl+W or a middle click wins the current
                // fight, F7 or Ctrl+L loses it, F8 or Ctrl+B jumps to the last floor's boss (laptops often
                // hide F keys behind Fn), Ctrl+E takes every key and jumps to the first ending's boss, Ctrl+K hits the enemy
                // for 130 to show a huge hit.
                // A run that used one is not saved.
                if cfg!(debug_assertions) {
                    let ctrl = |k| (is_key_down(KeyCode::LeftControl) || is_key_down(KeyCode::RightControl)) && is_key_pressed(k);
                    let win = is_key_pressed(KeyCode::F9) || ctrl(KeyCode::W) || is_mouse_button_pressed(MouseButton::Middle);
                    let (lose, jump) = (is_key_pressed(KeyCode::F7) || ctrl(KeyCode::L), is_key_pressed(KeyCode::F8) || ctrl(KeyCode::B));
                    let (endings, smite) = (ctrl(KeyCode::E), ctrl(KeyCode::K));
                    run.debug |= win || lose || jump || endings || smite;
                    match stage {
                        Stage::Battle(b) if b.over() => {}
                        Stage::Battle(b) if win => b.debug_win(),
                        Stage::Battle(b) if lose => b.f[0].hp = 0,
                        Stage::Battle(b) if smite => b.debug_smite(130),
                        _ => {}
                    }
                    if (jump || endings) && !matches!(stage, Stage::Battle(b) if b.over()) {
                        *stage = if endings { run.debug_endings() } else { run.debug_last_fight() };
                        ui::transition();
                    }
                }
                // the run's last fight shows the run's summary in place of its result card
                let mut summarised = false;
                let next = match stage {
                    Stage::Map => ui::map_screen(run).map(|(c, r)| run.enter(c, r)),
                    Stage::Battle(b) => {
                        // debug builds: the gallery's buttons, so the big animations can be seen on demand
                        if cfg!(debug_assertions) && !b.over() {
                            if let Some(k) = ui::debug_bar() {
                                run.debug = true;
                                match k {
                                    0 => b.debug_smite(130),
                                    1 => b.debug_break(),
                                    _ => b.debug_win(),
                                }
                            }
                        }
                        // the very first fight waits under a sheet of rules until it is read
                        ui::hold_for_rules(!save.rules_seen);
                        let ending = b.over() && (!b.won() || run.last_fight());
                        // past the last floor's boss the run counts as won, whatever an ending's floor does
                        let opens = save.would_open(run.exp, b.won() || run.floor >= FLOORS).filter(|_| !run.debug && run.daily.is_none());
                        let done = ui::battle_screen(b, run, ending.then_some(opens), &mut ai_timer);
                        if !save.rules_seen && ui::rules() {
                            save.rules_seen = true;
                            save.store();
                        }
                        if !b.over() && ui::surrender() {
                            run.tally(b);
                            Some(Stage::End(false))
                        } else if done {
                            summarised = ending;
                            Some(run.finish_battle(b))
                        } else {
                            None
                        }
                    }
                    Stage::Pick(options) => {
                        ui::status(run);
                        ui::pick(run, options).map(|id| {
                            run.add_charm(id);
                            run.after_node()
                        })
                    }
                    Stage::WildHands(id, then_pick) => {
                        ui::status(run);
                        ui::wild_hands(run, *id).then(|| run.after_wild_hands(*then_pick))
                    }
                    Stage::Upgrade(ids) => {
                        ui::status(run);
                        ui::upgrade(run, ids).map(|id| run.upgrade(id))
                    }
                    Stage::Blessing(offers) => {
                        ui::status(run);
                        ui::blessing(run, offers).map(|b| run.take_blessing(b))
                    }
                    Stage::Brand(offers) => {
                        ui::status(run);
                        ui::brand(run, offers).map(|(id, brand)| run.burn_brand(id, brand))
                    }
                    Stage::Learn(offers) => {
                        ui::status(run);
                        ui::learn(run, offers).map(|choice| run.learn(choice))
                    }
                    Stage::LevelUp => {
                        ui::status(run);
                        ui::level_up(run).map(|trait_up| run.level_up(trait_up))
                    }
                    Stage::Shop(shop) => {
                        ui::status(run);
                        ui::shop(run, shop).then(|| run.leave_shop(shop))
                    }
                    Stage::Event(e, result) => {
                        ui::status(run);
                        ui::event(run, e, result).then(|| run.after_node())
                    }
                    Stage::End(won) => {
                        quit_run = ui::end(run, *won, *opened);
                        None
                    }
                };
                // you may walk away from an expedition anywhere but mid-fight; how far you got still counts
                if matches!(stage, Stage::Map | Stage::Pick(_) | Stage::WildHands(..) | Stage::Upgrade(_) | Stage::Blessing(_) | Stage::Brand(_) | Stage::Learn(_) | Stage::LevelUp | Stage::Shop(_) | Stage::Event(..)) && ui::retreat() {
                    // past the last boss a retreat still counts as a win: show what it opened
                    if let Some(x) = save.finish(run, false) {
                        pick.0 = x;
                    }
                    save.store();
                    quit_run = true;
                }
                if let Some(n) = next {
                    if let Stage::End(won) = n {
                        *opened = save.finish(run, won);
                        save.store();
                        if let Some(x) = *opened {
                            pick.0 = x;
                        }
                    }
                    devlog::log(|| run.snapshot(&n));
                    *stage = n;
                    ui::transition();
                    // the summary was already read over the last fight
                    quit_run |= summarised;
                }
            }
        }
        if quit_run {
            to = Some(Screen::World);
        }
        if let Some(s) = to {
            devlog::log(|| format!("screen {}", match &s { Screen::Title => "title", Screen::World => "world", Screen::Run(..) => "run", Screen::Gallery(_) => "gallery" }));
            screen = s;
            ui::transition();
        }
        if ui::settings() {
            break;
        }
        if ui::volume() != save.volume || ui::perf_hud() != save.perf_hud {
            save.volume = ui::volume();
            save.perf_hud = ui::perf_hud();
            save.store();
        }
        ui::end_frame();
        ctl::end_frame();
        next_frame().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_mood_has_music_and_every_track_is_there() {
        // every floor and every ending's floor has its own calm, fight and boss track
        for place in 0..(FLOORS + data::ENDINGS.len()) as u8 {
            for m in [CALM, FIGHT, BOSS] {
                assert!(TRACKS.iter().any(|t| (t.2, t.3) == (m, place)), "no music for mood {m} on place {place}");
            }
        }
        assert!(TRACKS.iter().any(|t| (t.2, t.3) == (CALM, MENUS)));
        for (name, ..) in TRACKS {
            assert!(std::path::Path::new(&music_path(name)).exists(), "missing assets/music/{name}.ogg");
        }
    }

    #[test]
    fn icon_is_the_sprite_scaled_up_without_smoothing() {
        let sheet = Image::from_file_with_format(ART, None).unwrap();
        let small: [u8; 16 * 16 * 4] = icon_pixels(&sheet, 16);
        let big: [u8; 64 * 64 * 4] = icon_pixels(&sheet, 64);
        for (i, px) in big.chunks_exact(4).enumerate() {
            let (x, y) = (i % 64 / 4, i / 64 / 4);
            assert_eq!(px, &small[(y * 16 + x) * 4..][..4]);
        }
        // a sprite on a transparent background, not an empty or solid tile
        assert!(small.chunks_exact(4).any(|p| p[3] == 255) && small.chunks_exact(4).any(|p| p[3] == 0));
    }
}

/// The rites hero `h` can find in a run, by index in `COMBOS`.
fn minebound_pool(h: usize) -> Vec<usize> {
    data::pool(&data::HEROES[h].elements)
}
