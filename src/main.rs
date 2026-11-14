mod battle;
mod board;
mod data;
mod rng;
mod run;
mod sfx;
mod ui;

use macroquad::audio::{load_sound, load_sound_from_bytes, play_sound, set_sound_volume, stop_sound, PlaySoundParams, Sound};
use macroquad::prelude::*;
use run::{Run, Stage};

/// Background music: "Winter Lofi" by HoliznaCC0 (CC0), read from assets/music beside the game.
/// (file name, length in seconds)
const TRACKS: [(&str, f64); 5] = [
    ("two-hour-delay", 132.0),
    ("winter-blues", 223.4),
    ("first-snow", 174.1),
    ("snow-drift", 173.6),
    ("fire-place", 170.3),
];
/// Loudness of the music at the top of the volume slider.
const MUSIC_VOLUME: f32 = 0.5;

struct Music {
    order: Vec<usize>,
    playing: Option<Sound>,
    /// when the current track ends (or when to try again, if it failed to load)
    until: f64,
}

impl Music {
    fn new() -> Music {
        let mut order: Vec<usize> = (0..TRACKS.len()).collect();
        for i in (1..order.len()).rev() {
            order.swap(i, rand::gen_range(0, i + 1));
        }
        Music { order, playing: None, until: 0.0 }
    }

    /// Starts the next track when the current one ends, or when N is pressed.
    /// One track is held in memory at a time: a decoded track is tens of megabytes.
    async fn tick(&mut self) {
        if let Some(s) = &self.playing {
            set_sound_volume(s, ui::music_volume() * MUSIC_VOLUME);
        }
        if get_time() < self.until && !is_key_pressed(KeyCode::N) {
            return;
        }
        if let Some(old) = self.playing.take() {
            stop_sound(&old);
        }
        self.order.rotate_left(1);
        let (name, secs) = TRACKS[self.order[0]];
        match load_sound(&format!("assets/music/{name}.ogg")).await {
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

fn conf() -> Conf {
    Conf {
        window_title: "Minebound".into(),
        window_width: 1280,
        window_height: 720,
        window_resizable: true,
        high_dpi: true,
        sample_count: 4,
        ..Default::default()
    }
}

#[macroquad::main(conf)]
async fn main() {
    // `minebound <seed>` replays a run; otherwise the clock picks one
    let fixed_seed: Option<u64> = std::env::args().nth(1).and_then(|s| s.parse().ok());
    // Kenney Mini for titles and Pixel Operator for rules text (both CC0), built into the exe
    ui::set_fonts(
        load_ttf_font_from_bytes(include_bytes!("../assets/fonts/kenney-mini.ttf")).ok(),
        load_ttf_font_from_bytes(include_bytes!("../assets/fonts/pixel-operator.ttf")).ok(),
    );
    // Tiny Dungeon by Kenney (CC0), built into the exe
    ui::set_art(Texture2D::from_file_with_format(include_bytes!("../assets/tiny-dungeon/tilemap_packed.png"), None));
    let mut sounds = vec![];
    for s in sfx::ALL {
        let mut takes = vec![];
        for bytes in sfx::takes(s) {
            if let Ok(sound) = load_sound_from_bytes(bytes).await {
                takes.push(sound);
            }
        }
        sounds.push(takes);
    }
    ui::set_sounds(sounds);
    rand::srand((miniquad::date::now() * 1000.0) as u64);
    let mut music = Music::new();
    let mut game: Option<(Run, Stage)> = None;
    let mut ai_timer = 0.0;
    loop {
        clear_background(ui::BG);
        ui::begin_frame();
        music.tick().await;
        let mut quit_run = false;
        match &mut game {
            None => {
                if let Some(hero) = ui::heroes() {
                    let seed = fixed_seed.unwrap_or((miniquad::date::now() * 1000.0) as u64);
                    game = Some(Run::new(seed, hero));
                    ui::transition();
                }
            }
            Some((run, stage)) => {
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
                        ui::run_status(run);
                        ui::pick(options).map(|id| {
                            run.add_charm(id);
                            run.after_node()
                        })
                    }
                    Stage::Shop(stock) => {
                        ui::run_status(run);
                        ui::shop(run, stock).then(|| run.after_node())
                    }
                    Stage::Event(e, result) => {
                        ui::run_status(run);
                        ui::event(run, e, result).then(|| run.after_node())
                    }
                    Stage::End(won) => {
                        quit_run = ui::end(run, *won);
                        None
                    }
                };
                // you may walk away from an expedition anywhere but mid-fight
                if matches!(stage, Stage::Map | Stage::Pick(_) | Stage::Shop(_) | Stage::Event(..)) && ui::retreat() {
                    quit_run = true;
                }
                if let Some(n) = next {
                    *stage = n;
                    ui::transition();
                }
            }
        }
        if quit_run {
            game = None;
            ui::transition();
        }
        ui::settings();
        ui::end_frame();
        next_frame().await;
    }
}
