//! Progress kept between launches, as a few lines of text in %APPDATA%\Minebound\save.txt.
//! No mid-run save: only what the overworld and the settings need.

use crate::data::{ENDINGS, EXPEDITIONS, FIRST_HEROES, HEROES, SETS, SET_AT};
use crate::run::{Run, FLOORS};

/// What a deed opens: a pact, a brand or a hero, by name.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Unlock {
    Pact(&'static str),
    Brand(&'static str),
    Hero(&'static str),
}

pub struct Deed {
    pub name: &'static str,
    /// what to do, as the deeds sheet says it
    pub task: &'static str,
    pub opens: Unlock,
}

/// Done once, kept for good: each opens something new for every run after. Checked as a run ends.
pub static DEEDS: &[Deed] = &[
    Deed { name: "Lordslayer", task: "Beat a floor 1 boss.", opens: Unlock::Pact("Scavenger's Hook") },
    Deed { name: "Heavy Hand", task: "Land a single hit of 100 or more.", opens: Unlock::Brand("Charge") },
    Deed { name: "Hoarder", task: "Hold 8 pacts at once.", opens: Unlock::Pact("War Drum") },
    Deed { name: "Set Bound", task: "Wake a pact set: hold 3 pacts of one set.", opens: Unlock::Pact("Damper Coil") },
    Deed { name: "Keybearer", task: "Carry a secret key.", opens: Unlock::Brand("Dazzle") },
    Deed { name: "Dawn Delver", task: "Clear a daily descent.", opens: Unlock::Brand("Siphon") },
    Deed { name: "Two Paths", task: "Clear a descent with two different bearers.", opens: Unlock::Pact("Bone Pact") },
    Deed { name: "Full Company", task: "Clear a descent with each of the first four bearers.", opens: Unlock::Hero("Galvanist") },
];

/// Whether `u` is open for a save with deeds `done` (anything no deed opens always is).
pub fn is_open(done: u32, u: Unlock) -> bool {
    DEEDS.iter().enumerate().all(|(k, d)| d.opens != u || done & 1 << k != 0)
}

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
    /// the last day a daily descent was tried, and its best result that day (like `best`)
    pub daily: (u64, usize),
    /// secret endings found, one bit per index in `ENDINGS`
    pub endings: u32,
    /// deeds done, one bit per index in `DEEDS`, and those the deeds sheet has shown since
    pub deeds: u32,
    pub deeds_seen: u32,
}

impl Default for Save {
    fn default() -> Save {
        Save { unlocked: 1, best: vec![vec![0; HEROES.len()]; EXPEDITIONS.len()], volume: (6, 8), rules_seen: false, daily: (0, 0), endings: 0, deeds: 0, deeds_seen: 0 }
    }
}

pub const CLEARED: usize = FLOORS + 1;
const ALL_DEEDS: u32 = (1 << DEEDS.len()) - 1;

/// Deeds a run did, ended as `won`, by what it holds as it ends.
fn deeds_in(run: &Run, won: bool) -> u32 {
    let done = [
        (0, run.floor >= 1 || won),
        (1, run.best.0 >= 100),
        (2, run.charms.len() >= 8),
        (3, (0..SETS.len()).any(|k| run.set_count(k) >= SET_AT)),
        (4, !run.keys.is_empty()),
        (5, won && run.daily.is_some()),
    ];
    done.iter().filter(|d| d.1).fold(0, |bits, d| bits | 1 << d.0)
}
/// The rules sheet's edition. A save that read an older one (the minesweeper board was 1) is shown
/// the sheet again.
const RULES: usize = 2;

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
                (Some("rules"), &[n]) => save.rules_seen = n >= RULES,
                (Some("daily"), &[day, r]) => save.daily = (day as u64, r.min(CLEARED)),
                (Some("endings"), &[bits]) => save.endings = bits as u32 & ((1 << ENDINGS.len()) - 1),
                (Some("deeds"), &[done, seen]) => (save.deeds, save.deeds_seen) = (done as u32 & ALL_DEEDS, seen as u32 & ALL_DEEDS),
                (Some("best"), &[x, ref heroes @ ..]) if x < EXPEDITIONS.len() => {
                    for (h, &b) in heroes.iter().enumerate().take(HEROES.len()) {
                        save.best[x][h] = b.min(CLEARED);
                    }
                }
                _ => {}
            }
        }
        // a save from before deeds gets every one its progress already shows
        save.deeds |= save.deeds_from_progress();
        save
    }

    fn text(&self) -> String {
        let mut s = format!("unlocked {}\nvolume {} {}\nrules {}\ndaily {} {}\nendings {}\ndeeds {} {}\n", self.unlocked, self.volume.0, self.volume.1, if self.rules_seen { RULES } else { 0 }, self.daily.0, self.daily.1, self.endings, self.deeds, self.deeds_seen);
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

    /// Notes how far a run got: a daily descent as its day's result, any other as its hero's best,
    /// and the secret endings it found. Past the last floor's boss a run counts as won, even if an
    /// ending's floor ends it. A run that used a developer shortcut is not kept.
    /// Returns the expedition this opened, if any.
    pub fn finish(&mut self, run: &Run, won: bool) -> Option<usize> {
        let (reached, won) = (run.floor + 1, won || run.floor >= FLOORS);
        if !run.debug {
            self.endings |= run.found.iter().fold(0, |bits, &e| bits | 1 << e);
            self.deeds |= deeds_in(run, won);
        }
        let opened = match (run.debug, run.daily) {
            (true, _) => None,
            (_, Some(day)) => {
                let result = if won { CLEARED } else { reached };
                // tries on the same day keep the best of them
                let best = if self.daily.0 == day { self.daily.1.max(result) } else { result };
                self.daily = (day, best);
                None
            }
            _ => self.record(run.exp, run.hero, reached, won),
        };
        self.deeds |= self.deeds_from_progress();
        opened
    }

    /// Deeds the save itself shows done: clears by bearer, keys carried (an ending found), a daily cleared.
    fn deeds_from_progress(&self) -> u32 {
        let bearers = (0..FIRST_HEROES).filter(|&h| self.best.iter().any(|x| x[h] >= CLEARED)).count();
        let done = [
            (4, self.endings != 0),
            (5, self.daily.1 >= CLEARED),
            (6, bearers >= 2),
            (7, bearers >= FIRST_HEROES),
        ];
        done.iter().filter(|d| d.1).fold(0, |bits, d| bits | 1 << d.0)
    }

    /// Deeds done but not yet shown on the deeds sheet.
    pub fn new_deeds(&self) -> u32 {
        self.deeds & !self.deeds_seen
    }

    /// The expedition that ending a run of `exp` this way would unlock.
    pub fn would_open(&self, exp: usize, won: bool) -> Option<usize> {
        (won && exp + 1 == self.unlocked && self.unlocked < EXPEDITIONS.len()).then_some(exp + 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::{BRANDS, CHARMS};

    #[test]
    fn a_save_reads_back_the_same_and_survives_junk() {
        let mut s = Save::default();
        assert_eq!(s.record(0, 1, 2, false), None);
        assert_eq!(s.record(0, 2, 3, true), Some(1));
        // winning an older expedition again opens nothing new
        assert_eq!(s.record(0, 0, 3, true), None);
        s.volume = (3, 10);
        s.rules_seen = true;
        assert!(!Save::parse("rules 1\n").rules_seen, "a save that read the old rules sees the new ones");
        // a daily run keeps its day's result and touches neither bests nor unlocks
        let (mut run, _) = Run::new_daily(20_000);
        run.floor = 1;
        assert_eq!(s.finish(&run, false), None);
        assert_eq!((s.daily, s.unlocked), ((20_000, 2), 2));
        // another try the same day keeps the better result; a new day starts over
        run.floor = 0;
        s.finish(&run, false);
        assert_eq!(s.daily, (20_000, 2));
        let (mut next, _) = Run::new_daily(20_001);
        next.floor = 0;
        s.finish(&next, false);
        assert_eq!(s.daily, (20_001, 1));
        s.daily = (20_000, 2);
        assert_eq!(Save::parse(&s.text()), s);
        assert_eq!(s.best[0], vec![CLEARED, 2, CLEARED, 0, 0]);
        // an ending's boss counts as found even when a later floor ends the run, and the run as won
        let (mut deep, _) = Run::new(5, 3, 0);
        deep.keys = vec![0, 2];
        deep.floor = FLOORS + 1;
        deep.found = vec![0];
        assert_eq!(s.finish(&deep, false), None);
        assert_eq!((s.endings, s.best[0][3]), (1, CLEARED));
        assert_eq!(Save::parse(&s.text()), s);
        // a save from before the fourth hero still loads; the new hero starts fresh
        assert_eq!(Save::parse(&format!("best 0 {CLEARED} 2 {CLEARED}")).best[0], vec![CLEARED, 2, CLEARED, 0, 0]);
        let junk = Save::parse("unlocked 99\nbest 0 1 x\nbest 77 1\nvolume 4\nnonsense");
        assert_eq!(junk.unlocked, EXPEDITIONS.len());
        assert_eq!(junk.volume, (6, 8));
        assert_eq!(Save::parse("endings 255").endings, 7, "only endings that exist");
    }

    #[test]
    fn deeds_open_their_unlocks_and_old_saves_get_credit() {
        let (lordslayer, two_paths, full) = (1, 1 << 6, 1 << 7);
        // nothing a deed opens is open before it is done; everything else always is
        let hook = Unlock::Pact("Scavenger's Hook");
        assert!(!is_open(0, hook) && is_open(lordslayer, hook) && is_open(0, Unlock::Pact("Lucky Coin")));
        assert!(!is_open(0, Unlock::Hero("Galvanist")) && is_open(full, Unlock::Hero("Galvanist")));
        for d in DEEDS {
            let known = match d.opens {
                Unlock::Pact(n) => CHARMS.iter().any(|c| c.name == n),
                Unlock::Brand(n) => BRANDS.iter().any(|b| b.name == n),
                Unlock::Hero(n) => HEROES.iter().any(|h| h.name == n),
            };
            assert!(known, "{} opens something that exists", d.name);
        }
        // a run past floor 1 beat its boss; a debug run counts for nothing
        let mut s = Save::default();
        let (mut run, _) = Run::new(3, 0, 0);
        run.floor = 1;
        s.finish(&run, false);
        assert_eq!((s.deeds, s.new_deeds()), (lordslayer, lordslayer));
        let (mut cheat, _) = Run::new(3, 1, 0);
        (cheat.debug, cheat.best) = (true, (150, "x"));
        s.finish(&cheat, true);
        assert_eq!(s.deeds, lordslayer);
        // clears by two bearers, or all four, from a save written before deeds
        let old = Save::parse(&format!("unlocked 2\nbest 0 {CLEARED} 1 {CLEARED} 0\n"));
        assert_eq!(old.deeds, two_paths);
        let all = Save::parse(&format!("best 0 {CLEARED} {CLEARED} 0 0\nbest 1 0 0 {CLEARED} {CLEARED}\n"));
        assert_eq!(all.deeds, two_paths | full);
        // seen deeds stay seen
        s.deeds_seen = s.deeds;
        assert_eq!(Save::parse(&s.text()), s);
        assert_eq!(Save::parse(&s.text()).new_deeds(), 0);
    }
}
