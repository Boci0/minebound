//! Progress kept between launches, as a few lines of text in %APPDATA%\Minebound\save.txt.
//! No mid-run save: only what the overworld and the settings need.

use crate::data::{EXPEDITIONS, HEROES};
use crate::run::{Run, FLOORS};

#[derive(Clone, Debug, PartialEq)]
pub struct Save {
    /// how many expeditions are open, from the first
    pub unlocked: usize,
    /// best[expedition][hero]: 0 never tried, n reached floor n, FLOORS + 1 cleared it
    pub best: Vec<Vec<usize>>,
    /// (music, sound effects), each 0..=10
    pub volume: (i32, i32),
    /// the rules sheet has been read once, over the first battle
    pub rules_seen: bool,
    /// the last daily descent tried: (day, result like `best`); one try a day
    pub daily: (u64, usize),
}

impl Default for Save {
    fn default() -> Save {
        Save { unlocked: 1, best: vec![vec![0; HEROES.len()]; EXPEDITIONS.len()], volume: (6, 8), rules_seen: false, daily: (0, 0) }
    }
}

pub const CLEARED: usize = FLOORS + 1;

fn path() -> Option<std::path::PathBuf> {
    Some(std::path::Path::new(&std::env::var_os("APPDATA")?).join("Minebound").join("save.txt"))
}

impl Save {
    /// A missing or damaged file gives a fresh save; any line it can read still counts.
    pub fn load() -> Save {
        path().and_then(|p| std::fs::read_to_string(p).ok()).map_or_else(Save::default, |s| Save::parse(&s))
    }

    fn parse(s: &str) -> Save {
        let mut save = Save::default();
        for line in s.lines() {
            let mut w = line.split_whitespace();
            let key = w.next();
            let nums: Vec<usize> = w.filter_map(|x| x.parse().ok()).collect();
            match (key, nums.as_slice()) {
                (Some("unlocked"), &[n]) => save.unlocked = n.clamp(1, EXPEDITIONS.len()),
                (Some("volume"), &[m, s]) => save.volume = (m.min(10) as i32, s.min(10) as i32),
                (Some("rules"), &[n]) => save.rules_seen = n > 0,
                (Some("daily"), &[day, r]) => save.daily = (day as u64, r.min(CLEARED)),
                (Some("best"), &[x, ref heroes @ ..]) if x < EXPEDITIONS.len() => {
                    for (h, &b) in heroes.iter().enumerate().take(HEROES.len()) {
                        save.best[x][h] = b.min(CLEARED);
                    }
                }
                _ => {}
            }
        }
        save
    }

    fn text(&self) -> String {
        let mut s = format!("unlocked {}\nvolume {} {}\nrules {}\ndaily {} {}\n", self.unlocked, self.volume.0, self.volume.1, self.rules_seen as u8, self.daily.0, self.daily.1);
        for (x, heroes) in self.best.iter().enumerate() {
            let list: Vec<String> = heroes.iter().map(|b| b.to_string()).collect();
            s += &format!("best {x} {}\n", list.join(" "));
        }
        s
    }

    /// Losing progress to a failed write is not worth crashing over; it is retried at the next change.
    pub fn store(&self) {
        let Some(p) = path() else { return };
        let write = || -> std::io::Result<()> {
            std::fs::create_dir_all(p.parent().unwrap())?;
            // write aside, then swap in, so a crash mid-write cannot leave half a file
            let tmp = p.with_extension("tmp");
            std::fs::write(&tmp, self.text())?;
            std::fs::rename(&tmp, &p)
        };
        if let Err(e) = write() {
            eprintln!("progress not saved: {e}");
        }
    }

    /// Notes how far `hero` got on expedition `exp`. Returns the expedition this opened, if any.
    pub fn record(&mut self, exp: usize, hero: usize, floor_reached: usize, won: bool) -> Option<usize> {
        let b = &mut self.best[exp][hero];
        *b = (*b).max(if won { CLEARED } else { floor_reached });
        let opened = self.would_open(exp, won);
        if opened.is_some() {
            self.unlocked += 1;
        }
        opened
    }

    /// Notes how far a run got: a daily descent as its day's result, any other as its hero's best.
    /// A run that used a developer shortcut is not kept. Returns the expedition this opened, if any.
    pub fn finish(&mut self, run: &Run, won: bool) -> Option<usize> {
        let reached = run.floor + 1;
        match (run.debug, run.daily) {
            (true, _) => None,
            (_, Some(day)) => {
                self.daily = (day, if won { CLEARED } else { reached });
                None
            }
            _ => self.record(run.exp, run.hero, reached, won),
        }
    }

    /// The expedition that ending a run of `exp` this way would unlock.
    pub fn would_open(&self, exp: usize, won: bool) -> Option<usize> {
        (won && exp + 1 == self.unlocked && self.unlocked < EXPEDITIONS.len()).then_some(exp + 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_save_reads_back_the_same_and_survives_junk() {
        let mut s = Save::default();
        assert_eq!(s.record(0, 1, 2, false), None);
        assert_eq!(s.record(0, 2, 3, true), Some(1));
        // winning an older expedition again opens nothing new
        assert_eq!(s.record(0, 0, 3, true), None);
        s.volume = (3, 10);
        s.rules_seen = true;
        // a daily run keeps its day's result and touches neither bests nor unlocks
        let (mut run, _) = Run::new_daily(20_000);
        run.floor = 1;
        assert_eq!(s.finish(&run, false), None);
        assert_eq!((s.daily, s.unlocked), ((20_000, 2), 2));
        assert_eq!(Save::parse(&s.text()), s);
        assert_eq!(s.best[0], vec![CLEARED, 2, CLEARED, 0]);
        // a save from before the fourth hero still loads; the new hero starts fresh
        assert_eq!(Save::parse(&format!("best 0 {CLEARED} 2 {CLEARED}")).best[0], vec![CLEARED, 2, CLEARED, 0]);
        let junk = Save::parse("unlocked 99\nbest 0 1 x\nbest 77 1\nvolume 4\nnonsense");
        assert_eq!(junk.unlocked, EXPEDITIONS.len());
        assert_eq!(junk.volume, (6, 8));
    }
}
