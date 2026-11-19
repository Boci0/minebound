// no console window behind the game on Windows when it is double-clicked
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod battle;
mod ctl;
mod board;
mod data;
mod rng;
mod run;
mod save;
mod sfx;
mod ui;

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
}

/// Background music by HoliznaCC0 (CC0), read from assets/music beside the game: "Stranger Than
/// Fiction", "Gamer Soundtrack: Haunted Mansion" and "Horseless Headman".
/// (file name, length in seconds, mood: CALM on every screen but a fight, FIGHT, BOSS)
const TRACKS: [(&str, f64, u8); 18] = [
    ("somethings-out-there", 172.8, FIGHT),
    ("burning-in-the-atmosphere", 264.8, FIGHT),
    ("an-ocean-in-outer-space", 132.0, CALM),
    ("little-green-men", 156.7, FIGHT),
    ("sky-fish", 194.4, CALM),
    ("floor-1", 211.9, CALM),
    ("floor-2", 164.0, CALM),
    ("floor-3", 202.0, CALM),
    ("floor-4", 298.0, CALM),
    ("floor-5", 246.0, CALM),
    ("floor-6", 220.0, CALM),
    ("halloween-soundscape", 234.0, CALM),
    ("never-speak-of-the-devil", 145.9, CALM),
    ("between-life-and-death", 163.7, FIGHT),
    ("cobwebs", 149.3, FIGHT),
    ("haunted-houses", 134.1, FIGHT),
    ("nocturnal", 194.6, FIGHT),
    ("tragedy-waiting-to-happen", 119.2, BOSS),
];
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

    /// Plays tracks of `mood`: the next one when the current one ends or N is pressed, and a fade
    /// to a new one when the mood changes. One track is held in memory at a time: a decoded track
    /// is tens of megabytes.
    async fn tick(&mut self, mood: u8) {
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
        let wrong = self.track < TRACKS.len() && TRACKS[self.track].2 != mood;
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
        // a random track of this mood, not the one just played (unless it is the only one)
        let pool: Vec<usize> = (0..TRACKS.len()).filter(|&i| TRACKS[i].2 == mood).collect();
        let fresh: Vec<usize> = pool.iter().copied().filter(|&i| i != self.track).collect();
        let from = if fresh.is_empty() { &pool } else { &fresh };
        self.track = from[rand::gen_range(0, from.len())];
        let (path, (tx, rx)) = (music_path(TRACKS[self.track].0), std::sync::mpsc::channel());
        std::thread::spawn(move || tx.send(std::fs::read(path)));
        self.loading = Some(rx);
    }
}

/// The music a screen calls for: a fight's, a boss fight's, or the calm between.
fn mood(screen: &Screen) -> u8 {
    match screen {
        Screen::Run(_, Stage::Battle(b), _) if data::is_boss(&b.f[1].name) => BOSS,
        Screen::Run(_, Stage::Battle(_), _) => FIGHT,
        _ => CALM,
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
    // the newest open expedition and the first hero
    let mut pick = (save.unlocked - 1, 0);
    let mut screen = Screen::Title;
    let mut ai_timer = 0.0;
    loop {
        clear_background(ui::BG);
        ctl::begin_frame();
        ui::begin_frame();
        music.tick(mood(&screen)).await;
        let mut quit_run = false;
        // days since 1970 (UTC): the daily descent changes as this does
        let today = (miniquad::date::now() / 86_400.0) as u64;
        let mut to = None;
        match &mut screen {
            Screen::Title => match ui::title() {
                Some(Go::Play) => to = Some(Screen::World),
                Some(Go::Back) => break,
                Some(Go::Daily) | None => {}
            },
            Screen::World => match ui::world(&save, &mut pick, today) {
                Some(Go::Play) => {
                    let seed = fixed_seed.unwrap_or((miniquad::date::now() * 1000.0) as u64);
                    let (mut run, mut stage) = Run::new(seed, pick.1, pick.0);
                    // debug builds only: MINEBOUND_ENDINGS=1 starts every run as Ctrl+E does, for when keys can't be sent
                    if cfg!(debug_assertions) && std::env::var_os("MINEBOUND_ENDINGS").is_some() {
                        stage = run.debug_endings();
                    }
                    // debug builds only: MINEBOUND_MASTERED=1 starts with every rite at level 3, to see the masteries
                    if cfg!(debug_assertions) && std::env::var_os("MINEBOUND_MASTERED").is_some() {
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
                None => {}
            },
            Screen::Run(run, stage, opened) => {
                // debug builds only, to reach any screen quickly: F9, Ctrl+W or a middle click wins the current
                // fight, F7 or Ctrl+L loses it, F8 or Ctrl+B jumps to the last floor's boss (laptops often
                // hide F keys behind Fn), Ctrl+E takes every key and jumps to the first ending's boss.
                // A run that used one is not saved.
                if cfg!(debug_assertions) {
                    let ctrl = |k| (is_key_down(KeyCode::LeftControl) || is_key_down(KeyCode::RightControl)) && is_key_pressed(k);
                    let win = is_key_pressed(KeyCode::F9) || ctrl(KeyCode::W) || is_mouse_button_pressed(MouseButton::Middle);
                    let (lose, jump) = (is_key_pressed(KeyCode::F7) || ctrl(KeyCode::L), is_key_pressed(KeyCode::F8) || ctrl(KeyCode::B));
                    let endings = ctrl(KeyCode::E);
                    run.debug |= win || lose || jump || endings;
                    match stage {
                        Stage::Battle(b) if b.over() => {}
                        Stage::Battle(b) if win => b.debug_win(),
                        Stage::Battle(b) if lose => b.f[0].hp = 0,
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
                        ui::pick(options).map(|id| {
                            run.add_charm(id);
                            run.after_node()
                        })
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
                if matches!(stage, Stage::Map | Stage::Pick(_) | Stage::Upgrade(_) | Stage::Blessing(_) | Stage::Brand(_) | Stage::Shop(_) | Stage::Event(..)) && ui::retreat() {
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
            screen = s;
            ui::transition();
        }
        ui::settings();
        if ui::volume() != save.volume {
            save.volume = ui::volume();
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
        for m in [CALM, FIGHT, BOSS] {
            assert!(TRACKS.iter().any(|t| t.2 == m), "no music for mood {m}");
        }
        for (name, _, _) in TRACKS {
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
