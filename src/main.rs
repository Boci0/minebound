// no console window behind the game on Windows when it is double-clicked
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod battle;
mod board;
mod data;
mod rng;
mod run;
mod save;
mod sfx;
mod ui;

use macroquad::audio::{load_sound, load_sound_from_bytes, play_sound, set_sound_volume, stop_sound, PlaySoundParams, Sound};
use macroquad::prelude::*;
use run::{Run, Stage};
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
}

impl Music {
    fn new() -> Music {
        Music { playing: None, track: usize::MAX, until: 0.0, fading: None }
    }

    /// Plays tracks of `mood`: the next one when the current one ends or N is pressed, and a fade
    /// to a new one when the mood changes. One track is held in memory at a time: a decoded track
    /// is tens of megabytes.
    async fn tick(&mut self, mood: u8) {
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
        let (name, secs, _) = TRACKS[self.track];
        match load_sound(&music_path(name)).await {
            Ok(s) => {
                play_sound(&s, PlaySoundParams { looped: false, volume: ui::music_volume() * MUSIC_VOLUME });
                self.playing = Some(s);
                self.until = get_time() + secs + 1.5;
            }
            // no music folder next to the exe: stay silent, look again in a while
            Err(e) => {
                eprintln!("music not loaded ({name}): {e}");
                self.until = get_time() + 60.0;
            }
        }
    }
}

/// The music a screen calls for: a fight's, a boss fight's, or the calm between.
fn mood(screen: &Screen) -> u8 {
    match screen {
        Screen::Run(_, Stage::Battle(b), _) if data::BOSSES.iter().flatten().any(|d| d.name == b.f[1].name) => BOSS,
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

fn conf() -> Conf {
    let icon = Image::from_file_with_format(ART, None).ok().map(|sheet| miniquad::conf::Icon {
        small: icon_pixels(&sheet, 16),
        medium: icon_pixels(&sheet, 32),
        big: icon_pixels(&sheet, 64),
    });
    Conf {
        window_title: "Minebound".into(),
        window_width: 1280,
        window_height: 720,
        window_resizable: true,
        high_dpi: true,
        sample_count: 4,
        icon,
        ..Default::default()
    }
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
        ui::begin_frame();
        music.tick(mood(&screen)).await;
        let mut quit_run = false;
        let mut to = None;
        match &mut screen {
            Screen::Title => match ui::title() {
                Some(Go::Play) => to = Some(Screen::World),
                Some(Go::Back) => break,
                None => {}
            },
            Screen::World => match ui::world(&save, &mut pick) {
                Some(Go::Play) => {
                    let seed = fixed_seed.unwrap_or((miniquad::date::now() * 1000.0) as u64);
                    let (run, stage) = Run::new(seed, pick.1, pick.0);
                    to = Some(Screen::Run(run, stage, None));
                }
                Some(Go::Back) => to = Some(Screen::Title),
                None => {}
            },
            Screen::Run(run, stage, opened) => {
                // debug builds only: F9 or a middle click wins the current fight, to walk through a run quickly
                if cfg!(debug_assertions) && (is_key_pressed(KeyCode::F9) || is_mouse_button_pressed(MouseButton::Middle)) {
                    if let Stage::Battle(b) = stage {
                        b.debug_win();
                    }
                }
                let next = match stage {
                    Stage::Map => ui::map_screen(run).map(|(c, r)| run.enter(c, r)),
                    Stage::Battle(b) => ui::battle_screen(b, run.floor, &run.charms, &mut ai_timer).then(|| run.finish_battle(b)),
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
                    Stage::Shop(shop) => {
                        ui::status(run);
                        ui::shop(run, shop).then(|| run.after_node())
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
                if matches!(stage, Stage::Map | Stage::Pick(_) | Stage::Upgrade(_) | Stage::Shop(_) | Stage::Event(..)) && ui::retreat() {
                    save.record(run.exp, run.hero, run.floor + 1, false);
                    save.store();
                    quit_run = true;
                }
                if let Some(n) = next {
                    if let Stage::End(won) = n {
                        *opened = save.record(run.exp, run.hero, run.floor + 1, won);
                        save.store();
                        if let Some(x) = *opened {
                            pick.0 = x;
                        }
                    }
                    *stage = n;
                    ui::transition();
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
