//! Debug builds only: drive the game from a file instead of the real mouse and keyboard, and save
//! screenshots, so it can be tested while the window sits in the background.
//!
//! Set `MINEBOUND_CTL` to a folder. Each frame the game takes `cmd.txt` from it (and deletes it), then
//! plays its lines one step per frame. Coordinates are on the 1280x720 canvas.
//!   click X Y | rclick X Y | move X Y | drag X1 Y1 X2 Y2 | key NAME | ctrl NAME | wait SECONDS | shot NAME
//! `shot` saves `NAME.png` in the folder once the frame is drawn. While this is on, real input is ignored.

use macroquad::prelude::*;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::PathBuf;

enum Step {
    Move(Vec2),
    Down(MouseButton, Vec2),
    Up(MouseButton),
    Key(KeyCode, bool),
    KeyUp,
    Wait(f64),
    Shot(String),
}

#[derive(Default)]
struct Ctl {
    dir: Option<PathBuf>,
    queue: VecDeque<Step>,
    pos: Vec2,
    pressed: Option<MouseButton>,
    down: Vec<MouseButton>,
    key: Option<KeyCode>,
    keys_down: Vec<KeyCode>,
    until: f64,
    shot: Option<String>,
}

thread_local! {
    static CTL: RefCell<Ctl> = RefCell::new(Ctl {
        dir: std::env::var_os("MINEBOUND_CTL").filter(|_| cfg!(debug_assertions)).map(PathBuf::from),
        ..Default::default()
    });
}

fn key(name: &str) -> Option<KeyCode> {
    use KeyCode::*;
    Some(match name.to_lowercase().as_str() {
        "space" => Space,
        "enter" => Enter,
        "escape" | "esc" => Escape,
        "1" => Key1,
        "2" => Key2,
        "3" => Key3,
        "4" => Key4,
        "b" => B,
        "e" => E,
        "l" => L,
        "m" => M,
        "n" => N,
        "w" => W,
        _ => return None,
    })
}

/// The steps one line of `cmd.txt` makes; a line it can't read makes none.
fn parse(line: &str) -> Vec<Step> {
    let w: Vec<&str> = line.split_whitespace().collect();
    let num: Vec<f32> = w.iter().skip(1).filter_map(|s| s.parse().ok()).collect();
    let at = |k: usize| vec2(num[k], num[k + 1]);
    match (w.first().copied(), num.len()) {
        (Some("move"), 2) => vec![Step::Move(at(0))],
        (Some("click"), 2) => vec![Step::Move(at(0)), Step::Down(MouseButton::Left, at(0)), Step::Up(MouseButton::Left)],
        (Some("rclick"), 2) => vec![Step::Move(at(0)), Step::Down(MouseButton::Right, at(0)), Step::Up(MouseButton::Right)],
        (Some("drag"), 4) => {
            let (a, z) = (at(0), at(2));
            let mut v = vec![Step::Move(a), Step::Down(MouseButton::Left, a)];
            v.extend((1..=12).map(|k| Step::Move(a + (z - a) * k as f32 / 12.0)));
            v.push(Step::Up(MouseButton::Left));
            v
        }
        (Some("wait"), 1) => vec![Step::Wait(num[0] as f64)],
        (Some(c @ ("key" | "ctrl")), _) => w.get(1).and_then(|n| key(n)).map_or(vec![], |k| vec![Step::Key(k, c == "ctrl"), Step::KeyUp]),
        (Some("shot"), _) => w.get(1).map_or(vec![], |n| vec![Step::Shot(n.to_string())]),
        _ => vec![],
    }
}

/// True if the game is being driven from the file.
pub fn on() -> bool {
    CTL.with(|c| c.borrow().dir.is_some())
}

/// Call at the top of each frame: picks up new commands and plays the next step.
pub fn begin_frame() {
    CTL.with(|c| {
        let c = &mut *c.borrow_mut();
        let Some(dir) = c.dir.clone() else { return };
        (c.pressed, c.key) = (None, None);
        let file = dir.join("cmd.txt");
        if let Ok(text) = std::fs::read_to_string(&file) {
            let _ = std::fs::remove_file(&file);
            c.queue.extend(text.lines().flat_map(parse));
        }
        if get_time() < c.until {
            return;
        }
        match c.queue.pop_front() {
            Some(Step::Move(p)) => c.pos = p,
            Some(Step::Down(b, p)) => {
                (c.pos, c.pressed) = (p, Some(b));
                c.down.push(b);
            }
            Some(Step::Up(b)) => c.down.retain(|&x| x != b),
            Some(Step::Key(k, ctrl)) => {
                c.key = Some(k);
                c.keys_down = if ctrl { vec![KeyCode::LeftControl, k] } else { vec![k] };
            }
            Some(Step::KeyUp) => c.keys_down.clear(),
            Some(Step::Wait(s)) => c.until = get_time() + s,
            Some(Step::Shot(n)) => c.shot = Some(n),
            None => {}
        }
    });
}

/// Call once the frame is drawn, before `next_frame`: saves a screenshot that was asked for.
pub fn end_frame() {
    let Some((dir, name)) = CTL.with(|c| {
        let c = &mut *c.borrow_mut();
        c.shot.take().zip(c.dir.clone()).map(|(n, d)| (d, n))
    }) else {
        return;
    };
    get_screen_data().export_png(&dir.join(format!("{name}.png")).to_string_lossy());
}

/// Where the fake mouse is on the canvas, while driven from the file.
pub fn mouse() -> Option<Vec2> {
    CTL.with(|c| {
        let c = c.borrow();
        c.dir.as_ref().map(|_| c.pos)
    })
}

// Stand-ins for macroquad's input calls: the file's input while it drives, the real input otherwise.

pub fn is_mouse_button_pressed(b: MouseButton) -> bool {
    if on() { CTL.with(|c| c.borrow().pressed == Some(b)) } else { macroquad::input::is_mouse_button_pressed(b) }
}

pub fn is_mouse_button_down(b: MouseButton) -> bool {
    if on() { CTL.with(|c| c.borrow().down.contains(&b)) } else { macroquad::input::is_mouse_button_down(b) }
}

pub fn is_key_pressed(k: KeyCode) -> bool {
    if on() { CTL.with(|c| c.borrow().key == Some(k)) } else { macroquad::input::is_key_pressed(k) }
}

pub fn is_key_down(k: KeyCode) -> bool {
    if on() { CTL.with(|c| c.borrow().keys_down.contains(&k)) } else { macroquad::input::is_key_down(k) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_become_steps() {
        assert_eq!(parse("click 10 20").len(), 3);
        assert_eq!(parse("drag 0 0 120 0").len(), 15);
        assert_eq!(parse("ctrl w").len(), 2);
        assert!(parse("key nope").is_empty() && parse("click 1").is_empty() && parse("").is_empty());
        assert!(matches!(parse("move 5 6")[0], Step::Move(p) if p == vec2(5.0, 6.0)));
    }
}
