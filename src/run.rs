use crate::{
    battle::{Battle, Fighter},
    data::*,
    rng::Rng,
};

pub const FLOORS: usize = 3;
pub const COLS: usize = 5;
pub const CHARM_PRICE: i32 = 35;
pub const HEAL_PRICE: i32 = 20;
pub const HEAL_AMOUNT: i32 = 12;
/// HP everyone recovers after a won fight
pub const BREATHER: i32 = 10;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NodeKind {
    Battle,
    Elite,
    Encounter,
    Shop,
    Rest,
    Treasure,
    Boss,
}

impl NodeKind {
    pub fn name(self) -> &'static str {
        ["Battle", "Elite", "Encounter", "Shop", "Rest", "Treasure", "Boss"][self as usize]
    }
}

pub struct Node {
    pub kind: NodeKind,
    /// rows in the next column this node leads to
    pub next: Vec<usize>,
}

/// What the player is looking at. The UI draws it; the simulation plays it.
pub enum Stage {
    Map,
    Battle(Battle),
    Pick(Vec<usize>),
    Shop(Vec<usize>),
    Event(&'static Encounter, Option<String>),
    End(bool),
}

pub struct Run {
    pub seed: u64,
    rng: Rng,
    pub hero: usize,
    pub hp: i32,
    pub bonus_hp: i32,
    pub shards: i32,
    pub charms: Vec<usize>,
    pub floor: usize,
    pub map: Vec<Vec<Node>>,
    pub pos: Option<(usize, usize)>,
    pub note: String,
}

impl Run {
    pub fn new(seed: u64, hero: usize) -> (Run, Stage) {
        let mut run = Run {
            seed,
            rng: Rng::new(seed),
            hero,
            hp: HEROES[hero].hp,
            bonus_hp: 0,
            shards: 20,
            charms: vec![],
            floor: 0,
            map: vec![],
            pos: None,
            note: "Pick a path. Every route ends at the floor's boss.".into(),
        };
        run.gen_map();
        let stage = run.pick();
        (run, stage)
    }

    pub fn mods(&self) -> Mods {
        let mut m = Mods::default();
        (HEROES[self.hero].passive)(&mut m);
        for &c in &self.charms {
            (CHARMS[c].apply)(&mut m);
        }
        m
    }

    pub fn max_hp(&self) -> i32 {
        (HEROES[self.hero].hp + self.bonus_hp + self.mods().max_hp).max(1)
    }

    pub fn add_charm(&mut self, id: usize) {
        let before = self.max_hp();
        self.charms.push(id);
        self.hp += self.max_hp() - before;
    }

    /// Up to `n` charms the player does not own yet.
    fn offer(&mut self, n: usize) -> Vec<usize> {
        let mut ids: Vec<usize> = (0..CHARMS.len()).filter(|i| !self.charms.contains(i)).collect();
        self.rng.shuffle(&mut ids);
        ids.truncate(n);
        ids
    }

    fn pick(&mut self) -> Stage {
        let options = self.offer(3);
        if options.is_empty() {
            self.shards += 20;
            return self.after_node();
        }
        Stage::Pick(options)
    }

    fn kind_for(&mut self, col: usize, row: usize) -> NodeKind {
        if col == 0 {
            return NodeKind::Battle;
        }
        if col == COLS {
            return NodeKind::Boss;
        }
        if col == COLS - 1 && row == 0 {
            return NodeKind::Rest; // always a chance to heal before the boss
        }
        match self.rng.below(100) {
            0..=34 => NodeKind::Battle,
            35..=54 => NodeKind::Encounter,
            55..=69 if col >= 2 => NodeKind::Elite,
            55..=69 => NodeKind::Battle,
            70..=81 => NodeKind::Shop,
            82..=91 => NodeKind::Rest,
            _ => NodeKind::Treasure,
        }
    }

    fn gen_map(&mut self) {
        let mut sizes: Vec<usize> = (0..COLS).map(|_| 2 + self.rng.below(2)).collect();
        sizes.push(1);
        let mut map: Vec<Vec<Node>> = Vec::new();
        for (c, &n) in sizes.iter().enumerate() {
            map.push((0..n).map(|r| Node { kind: self.kind_for(c, r), next: vec![] }).collect());
        }
        for c in 0..COLS {
            let (n, m) = (sizes[c], sizes[c + 1]);
            // walk both columns top to bottom together: links made this way can never cross,
            // and every node on both sides gets one
            let (mut r, mut j) = (0, 0);
            loop {
                map[c][r].next.push(j);
                if r + 1 == n && j + 1 == m {
                    break;
                }
                let step = self.rng.below(3);
                if r + 1 < n && j + 1 < m && step == 0 {
                    r += 1;
                    j += 1;
                } else if j + 1 == m || (r + 1 < n && step == 1) {
                    r += 1;
                } else {
                    j += 1;
                }
            }
        }
        self.map = map;
    }

    /// Nodes the player may step on next.
    pub fn choices(&self) -> Vec<(usize, usize)> {
        match self.pos {
            None => (0..self.map[0].len()).map(|r| (0, r)).collect(),
            Some((c, r)) => self.map[c][r].next.iter().map(|&j| (c + 1, j)).collect(),
        }
    }

    /// `slow`: how many fewer tiles it flips per turn than a hero. Lesser foes sweep slower.
    fn enemy(&self, d: &EnemyDef, scale: bool, slow: i32) -> Fighter {
        let floor = self.floor as i32;
        let (hp, power) = if scale { (d.hp * (85 + 55 * floor) / 100, d.power + floor) } else { (d.hp, d.power) };
        let mut m = Mods::default();
        (d.mods)(&mut m);
        m.reveals -= slow;
        Fighter::new(d.name, d.desc, hp, hp, power, d.elements, m, d.aggro)
    }

    fn random_normal(&mut self) -> Fighter {
        let d = &NORMALS[self.rng.below(NORMALS.len())];
        self.enemy(d, true, 3)
    }

    pub fn enter(&mut self, col: usize, row: usize) -> Stage {
        self.pos = Some((col, row));
        self.note.clear();
        let kind = self.map[col][row].kind;
        match kind {
            NodeKind::Battle | NodeKind::Elite | NodeKind::Boss => {
                let foes = match kind {
                    NodeKind::Battle => {
                        let waves = match self.floor {
                            0 => 1 + self.rng.below(2),
                            1 => 2,
                            _ => 2 + self.rng.below(2),
                        };
                        (0..waves).map(|_| self.random_normal()).collect()
                    }
                    NodeKind::Elite => {
                        let elite = &ELITES[self.rng.below(ELITES.len())];
                        vec![self.random_normal(), self.enemy(elite, true, 2)]
                    }
                    _ => vec![self.enemy(&BOSSES[self.floor.min(BOSSES.len() - 1)], false, 1)],
                };
                let h = &HEROES[self.hero];
                let me = Fighter::new(h.name, h.passive_desc, self.hp, self.max_hp(), h.power, &h.elements, self.mods(), 3);
                Stage::Battle(Battle::new(me, foes, Rng::new(self.rng.next())))
            }
            NodeKind::Encounter => Stage::Event(&ENCOUNTERS[self.rng.below(ENCOUNTERS.len())], None),
            NodeKind::Rest => Stage::Event(&REST, None),
            NodeKind::Shop => Stage::Shop(self.offer(3)),
            NodeKind::Treasure => self.pick(),
        }
    }

    pub fn finish_battle(&mut self, b: &Battle) -> Stage {
        self.hp = b.f[0].hp;
        if !b.won() {
            return Stage::End(false);
        }
        let kind = self.pos.map_or(NodeKind::Battle, |(c, r)| self.map[c][r].kind);
        if kind == NodeKind::Boss && self.floor + 1 == FLOORS {
            return Stage::End(true);
        }
        let m = self.mods();
        let base = match kind {
            NodeKind::Boss => 40,
            NodeKind::Elite => 25 + self.rng.below(10) as i32,
            _ => 12 + self.rng.below(6) as i32,
        };
        let shards = base * (100 + m.shard_pct) / 100;
        self.shards += shards;
        self.hp = (self.hp + BREATHER + m.heal_after).min(self.max_hp());
        self.note = format!("Won the fight: +{shards} shards.");
        if kind == NodeKind::Battle {
            self.after_node()
        } else {
            self.pick()
        }
    }

    /// Back to the map; past a boss, that means the next floor.
    pub fn after_node(&mut self) -> Stage {
        if matches!(self.pos, Some((c, _)) if c == COLS) {
            self.floor += 1;
            self.pos = None;
            self.gen_map();
            self.note = format!("Floor {} begins.", self.floor + 1);
        }
        Stage::Map
    }

    pub fn can_afford(&self, c: &Choice) -> bool {
        let cost: i32 = c.outs.iter().map(|o| if let Out::Shards(v) = o { -v.min(&0) } else { 0 }).sum();
        cost <= self.shards
    }

    pub fn choose(&mut self, c: &Choice) -> String {
        let mut msg = vec![];
        self.apply(c.outs, &mut msg);
        msg.join(" ")
    }

    fn apply(&mut self, outs: &[Out], msg: &mut Vec<String>) {
        for o in outs {
            match *o {
                Out::Hp(v) => self.heal(v, msg),
                Out::HealPct(p) => self.heal(self.max_hp() * p / 100, msg),
                Out::MaxHp(v) => {
                    self.bonus_hp += v;
                    self.hp = (self.hp + v.max(0)).clamp(1, self.max_hp());
                    msg.push(format!("Max HP {v:+}."));
                }
                Out::Shards(v) => {
                    self.shards = (self.shards + v).max(0);
                    msg.push(format!("{v:+} shards."));
                }
                Out::GainCharm => match self.offer(1).first() {
                    Some(&id) => {
                        self.add_charm(id);
                        msg.push(format!("Found a charm: {}.", CHARMS[id].name));
                    }
                    None => {
                        self.shards += 20;
                        msg.push("Nothing new here, only 20 shards.".into());
                    }
                },
                Out::Text(s) => msg.push(s.into()),
                Out::Chance(pct, win, lose) => {
                    let outs = if self.rng.chance(pct) { win } else { lose };
                    self.apply(outs, msg);
                }
            }
        }
    }

    // encounters never kill: HP stops at 1
    fn heal(&mut self, v: i32, msg: &mut Vec<String>) {
        let before = self.hp;
        self.hp = (self.hp + v).clamp(1, self.max_hp());
        msg.push(format!("{:+} HP.", self.hp - before));
    }

    pub fn buy_charm(&mut self, id: usize) -> bool {
        if self.shards < CHARM_PRICE {
            return false;
        }
        self.shards -= CHARM_PRICE;
        self.add_charm(id);
        true
    }

    pub fn buy_heal(&mut self) -> bool {
        if self.shards < HEAL_PRICE || self.hp >= self.max_hp() {
            return false;
        }
        self.shards -= HEAL_PRICE;
        self.hp = (self.hp + HEAL_AMOUNT).min(self.max_hp());
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_map_node_leads_to_the_boss_and_can_be_reached() {
        for seed in 0..200 {
            let (run, _) = Run::new(seed, 0);
            assert_eq!(run.map.len(), COLS + 1);
            assert_eq!(run.map[COLS].len(), 1);
            for c in 0..COLS {
                let width = run.map[c + 1].len();
                for node in &run.map[c] {
                    assert!(!node.next.is_empty() && node.next.iter().all(|&j| j < width));
                }
                for j in 0..width {
                    assert!(run.map[c].iter().any(|n| n.next.contains(&j)), "orphan node, seed {seed}");
                }
                for pair in run.map[c].windows(2) {
                    let (upper, lower) = (pair[0].next.iter().max(), pair[1].next.iter().min());
                    assert!(upper <= lower, "links cross, seed {seed}");
                }
            }
        }
    }

    /// Plays a whole run with the enemy AI in the player's seat. Returns (won, floor reached).
    fn simulate(seed: u64, hero: usize) -> (bool, usize) {
        let (mut run, mut stage) = Run::new(seed, hero);
        let mut pick = Rng::new(seed ^ 0xABCD);
        for _ in 0..500 {
            stage = match stage {
                Stage::Map => {
                    let options = run.choices();
                    let (c, r) = options[pick.below(options.len())];
                    run.enter(c, r)
                }
                Stage::Battle(mut b) => {
                    let mut steps = 0;
                    while !b.over() {
                        b.ai_step();
                        steps += 1;
                        assert!(steps < 5000, "battle never ends: seed {seed} hero {hero}");
                    }
                    run.finish_battle(&b)
                }
                Stage::Pick(options) => {
                    run.add_charm(options[0]);
                    run.after_node()
                }
                Stage::Shop(stock) => {
                    for id in stock {
                        run.buy_charm(id);
                    }
                    while run.buy_heal() {}
                    run.after_node()
                }
                Stage::Event(e, _) => {
                    let ok: Vec<&Choice> = e.choices.iter().filter(|c| run.can_afford(c)).collect();
                    let c = ok[pick.below(ok.len())];
                    run.choose(c);
                    assert!(run.hp >= 1 && run.shards >= 0);
                    run.after_node()
                }
                Stage::End(won) => return (won, run.floor),
            };
        }
        panic!("run never ends: seed {seed} hero {hero}");
    }

    #[test]
    fn simulated_runs_end_and_stay_in_a_sane_balance_band() {
        const RUNS: u64 = 300;
        for hero in 0..HEROES.len() {
            let results: Vec<(bool, usize)> = (0..RUNS).map(|s| simulate(s, hero)).collect();
            let wins = results.iter().filter(|r| r.0).count();
            let past_floor_1 = results.iter().filter(|r| r.1 >= 1).count();
            println!(
                "{:12} wins {:3}/{RUNS}  cleared floor 1: {:3}/{RUNS}",
                HEROES[hero].name, wins, past_floor_1
            );
            // the bot plays plainly (never banks stacks for a combo), so a human should beat these
            assert!(past_floor_1 as u64 >= RUNS * 30 / 100, "{} dies too early", HEROES[hero].name);
            assert!((wins as u64) >= RUNS * 5 / 100, "{} cannot win", HEROES[hero].name);
            assert!((wins as u64) <= RUNS * 60 / 100, "{} wins too easily", HEROES[hero].name);
        }
    }
}
