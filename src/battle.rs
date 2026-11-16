use crate::{
    board::{Board, N},
    data::*,
    rng::Rng,
};

pub const MINE_DMG: i32 = 2;
pub const BASE_MINES: i32 = 5;
pub const REVEALS: i32 = 6;
/// The fewest reveals a player turn can have, however many penalties stack.
pub const MIN_REVEALS: i32 = 5;
pub const PRESSURE_TURN: usize = 12;
/// Safe reveals in a row that count toward surge; each adds `SURGE_PCT` to skill damage.
pub const MAX_SURGE: i32 = 5;
pub const SURGE_PCT: i32 = 20;

/// Every charm hook the battle reports through `Event::Proc`, by `Mods` field name.
pub const HOOKS: &[&str] = &[
    "reveals", "power", "mine_guard", "start_shield", "combo_pct", "duds", "start_stacks", "burn_bonus", "scouts",
    "thorns", "foe_mines", "fewer_mines", "lifesteal_pct", "opening", "shield_turn", "surge_start", "surge_pct",
    "keep_surge", "rite_bonus", "rite_burn", "first_pct", "claim_zap", "claim_heal", "claim_shield", "claim_bonus",
    "salvage", "mine_spite", "armor", "evade", "reveal_shield", "hoard_shield", "chain", "mine_no_end", "sense", "echo",
    "desperate", "heal_after",
];

#[derive(Clone, Default)]
pub struct Fighter {
    pub name: String,
    pub sprite: usize,
    pub desc: String,
    pub hp: i32,
    pub max_hp: i32,
    pub power: i32,
    pub elements: Vec<Element>,
    pub aggro: usize,
    /// one board per element; `cur` is the one on the table
    pub boards: Vec<Board>,
    pub cur: usize,
    /// ammunition: spent by skills
    pub stacks: Vec<Element>,
    /// safe reveals in a row this turn
    pub surge: i32,
    /// skills cast this turn
    pub cast_n: i32,
    /// enemy skills that will still miss this turn
    pub evades_left: i32,
    pub shield: i32,
    /// burn on it: it loses this much HP as its turn starts, then the burn halves
    pub burn: i32,
    pub weak: i32,
    pub reveals_left: i32,
    /// reveals spent this turn
    pub used: i32,
    pub duds: i32,
    /// mines it will still sense (and flag) this turn
    pub sense_left: i32,
    /// AI only: it has decided to empty its stacks this turn
    firing: bool,
    pub mods: Mods,
    /// skills upgraded by boss kills, by index in `COMBOS`; a repeat is another level
    pub ups: Vec<usize>,
    /// brands burnt onto rites: (index in `COMBOS`, index in `BRANDS`), one per rite
    pub brands: Vec<(usize, usize)>,
}

impl Fighter {
    pub fn board(&self) -> &Board {
        &self.boards[self.cur]
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new(name: &str, sprite: usize, desc: &str, hp: i32, max_hp: i32, power: i32, elements: &[Element], mods: Mods, aggro: usize) -> Fighter {
        let (name, desc, elements) = (name.into(), desc.into(), elements.to_vec());
        Fighter { name, sprite, desc, hp, max_hp, power: power + mods.power, elements, aggro, mods, ..Default::default() }
    }

    pub fn level(&self, c: &Combo) -> usize {
        let id = combo_id(c);
        1 + self.ups.iter().filter(|&&u| u == id).count()
    }

    pub fn brand(&self, c: &Combo) -> Option<&'static Brand> {
        let id = combo_id(c);
        self.brands.iter().find(|b| b.0 == id).map(|b| &BRANDS[b.1])
    }

    /// What `c` does for this fighter, each effect with the level it works at: the rite's own grow
    /// with it, a brand's stay at 1.
    pub fn effects<'a>(&'a self, c: &'a Combo) -> impl Iterator<Item = (Effect, usize)> + 'a {
        let lvl = self.level(c);
        c.effects.iter().map(move |&e| (e, lvl)).chain(self.brand(c).map(|b| (b.effect, 1)))
    }

    pub fn describe(&self, c: &Combo) -> String {
        rite_text(c, self.level(c), self.brand(c))
    }

    /// The gems `c` would take out of `stacks`: its cost, plus the toll paid in whatever element is most
    /// plentiful after that. None if the stacks fall short.
    pub fn payment(&self, c: &Combo, stacks: &[Element]) -> Option<Vec<Element>> {
        let mut paid = self.cost(c);
        if !pays(stacks, &paid) {
            return None;
        }
        let mut rest = stacks.to_vec();
        spend(&mut rest, &paid);
        for _ in 0..self.mods.toll.max(0) {
            let e = *rest.iter().max_by_key(|&&e| rest.iter().filter(|&&x| x == e).count())?;
            spend(&mut rest, &[e]);
            paid.push(e);
        }
        Some(paid)
    }

    /// True if `paid` covers `c`'s cost and toll exactly, whichever gems pay the toll.
    pub fn fits(&self, c: &Combo, paid: &[Element]) -> bool {
        paid.len() == self.cost(c).len() + self.mods.toll.max(0) as usize && pays(paid, &self.cost(c))
    }

    /// The gems `c` takes from this fighter, at its level.
    pub fn cost(&self, c: &Combo) -> Vec<Element> {
        c.cost(self.level(c))
    }
}

/// A damage step that adds `v` to `d`, noted in `steps` unless it is nothing.
fn plus(steps: &mut Vec<(String, String)>, d: &mut i32, what: String, v: i32) {
    if v != 0 {
        *d += v;
        steps.push((what, format!("{v:+}")));
    }
}

/// True if `stacks` hold every gem in `cost`, repeats included.
pub fn pays(stacks: &[Element], cost: &[Element]) -> bool {
    cost.iter().all(|e| cost.iter().filter(|&x| x == e).count() <= stacks.iter().filter(|&x| x == e).count())
}

fn spend(stacks: &mut Vec<Element>, cost: &[Element]) {
    for e in cost {
        let k = stacks.iter().position(|x| x == e).expect("checked with pays");
        stacks.remove(k);
    }
}

/// Things that just happened, for the screen to animate and voice.
pub enum Event {
    Flip,
    /// `side` claimed the crystals on these tiles of its `element` board. Carried here because the
    /// board may already be dealt anew when the last crystal on it goes
    Claim { side: usize, element: Element, tiles: Vec<usize> },
    Mine,
    /// `skill`: (index in `COMBOS`, level)
    Cast { from: usize, name: &'static str, element: Element, miss: bool, skill: (usize, usize) },
    /// a passive of `side` just took effect; `hook` names the `Mods` field
    Proc { side: usize, hook: &'static str },
}

/// `f[0]` is the player, `f[1]` the current enemy. Both sides obey exactly the same rules.
pub struct Battle {
    pub f: [Fighter; 2],
    pub queue: Vec<Fighter>,
    pub wave: usize,
    pub waves: usize,
    pub turn: usize,
    pub active: usize,
    pub log: Vec<String>,
    pub events: Vec<Event>,
    rng: Rng,
}

impl Battle {
    pub fn new(player: Fighter, mut enemies: Vec<Fighter>, rng: Rng) -> Battle {
        let waves = enemies.len();
        enemies.reverse();
        let first = enemies.pop().expect("a battle needs an enemy");
        let mut b = Battle { f: [player, first], queue: enemies, wave: 1, waves, turn: 1, active: 0, log: vec![], events: vec![], rng };
        for _ in 0..b.f[0].mods.start_stacks {
            let k = b.rng.below(b.f[0].elements.len());
            let e = b.f[0].elements[k];
            b.f[0].stacks.push(e);
        }
        let head_start = b.f[0].mods.start_stacks > 0;
        b.procs(0, &[("start_stacks", head_start)]);
        b.start_wave();
        b
    }

    /// Reports that a passive of `side` took effect, so the screen can flash its charm.
    fn proc(&mut self, side: usize, hook: &'static str) {
        self.procs(side, &[(hook, true)]);
    }

    /// `proc` for each hook whose condition holds.
    fn procs(&mut self, side: usize, hooks: &[(&'static str, bool)]) {
        for &(hook, on) in hooks.iter().filter(|h| h.1) {
            debug_assert!(HOOKS.contains(&hook) && on, "unlisted hook {hook}");
            self.events.push(Event::Proc { side, hook });
        }
    }

    pub fn over(&self) -> bool {
        self.f[0].hp <= 0 || (self.f[1].hp <= 0 && self.queue.is_empty())
    }

    pub fn won(&self) -> bool {
        self.over() && self.f[0].hp > 0
    }

    /// Developer shortcut: fells every enemy at once.
    pub fn debug_win(&mut self) {
        self.queue.clear();
        self.f[1].hp = 0;
    }

    /// Combos `s` can pay for right now, at their level.
    pub fn ready(&self, s: usize) -> Vec<&'static Combo> {
        COMBOS.iter().filter(|c| self.f[s].payment(c, &self.f[s].stacks).is_some()).collect()
    }


    /// Overpressure: long fights escalate, so no shield or regen build can stall forever.
    pub fn pressure(&self) -> i32 {
        2 * self.turn.saturating_sub(PRESSURE_TURN) as i32
    }

    /// How high `s` can push its surge: some opponents jam it.
    pub fn surge_cap(&self, s: usize) -> i32 {
        match self.f[1 - s].mods.foe_surge_cap {
            0 => MAX_SURGE,
            jam => jam.min(MAX_SURGE),
        }
    }

    /// Percent each surge step adds to `s`'s skill damage.
    pub fn surge_step(&self, s: usize) -> i32 {
        SURGE_PCT + self.f[s].mods.surge_pct
    }

    /// Damage of `s` casting rite `c` next (before the target's shield).
    pub fn hit_damage(&self, s: usize, c: &Combo) -> i32 {
        self.hit_steps(s, c).0
    }

    /// `hit_damage`, with the steps that make it: (what, its number), for the screen to show.
    pub fn hit_steps(&self, s: usize, c: &Combo) -> (i32, Vec<(String, String)>) {
        let mut steps = vec![];
        let full = self.dmg_steps(s, c, self.f[s].cast_n, self.f[s].surge, self.f[s].shield, &mut steps);
        let d = match (self.f[1 - s].evades_left > 0 && self.hits(s, c), self.f[1 - s].mods.graze > 0) {
            (true, true) => full / 2, // it will graze
            (true, false) => 0,       // it will miss
            _ => full,
        };
        if d != full {
            steps.push((if d == 0 { "it evades" } else { "it grazes: half" }.into(), format!("= {d}")));
        }
        (d, steps)
    }

    /// True if `s` casting `c` deals damage. The player's rites only hit if they say so; every enemy
    /// rite still strikes with its gems, since for most enemies that is their whole attack.
    pub fn hits(&self, s: usize, c: &Combo) -> bool {
        s == 1 || self.f[s].effects(c).any(|(e, _)| e.hits())
    }

    /// Below half HP, Berserker's Axe adds power.
    fn desperate(&self, s: usize) -> bool {
        let f = &self.f[s];
        f.mods.desperate > 0 && f.hp * 2 <= f.max_hp
    }

    /// `nth`: how many rites `s` already cast this turn; `surge`: what it holds as this one fires.
    fn dmg(&self, s: usize, c: &Combo, nth: i32, surge: i32, shield: i32) -> i32 {
        self.dmg_steps(s, c, nth, surge, shield, &mut vec![])
    }

    /// `dmg`, writing each step that makes it into `steps`, so what the screen shows is what lands.
    fn dmg_steps(&self, s: usize, c: &Combo, nth: i32, surge: i32, shield: i32, steps: &mut Vec<(String, String)>) -> i32 {
        if !self.hits(s, c) {
            return 0;
        }
        let f = &self.f[s];
        let foe = &self.f[1 - s];
        let power = f.power + if self.desperate(s) { f.mods.desperate } else { 0 };
        let gems = f.cost(c).len() as i32;
        let mut d = gems * power;
        steps.push((format!("{gems} gems x {power} power"), d.to_string()));
        plus(steps, &mut d, format!("rite {} this turn", nth + 1), nth * f.mods.chain);
        plus(steps, &mut d, "rite bonus".into(), f.mods.rite_bonus);
        let lvl = f.level(c);
        let mut blast = 0;
        for (e, l) in f.effects(c) {
            let (what, add) = match e {
                Effect::Damage(v) => (c.title(lvl).to_string(), at_level(v, l)),
                Effect::PerSurge(v) => (format!("{surge} surge"), at_level(v, l) * surge),
                Effect::Detonate => (format!("detonate {} burn", foe.burn), foe.burn.max(0) * (3 + f.mods.burn_bonus)),
                Effect::PerReveal(v) => (format!("{} reveals used", f.used), at_level(v, l) * f.used),
                Effect::PerBurn(v) => (format!("{} burn on foe", foe.burn), at_level(v, l) * foe.burn.max(0)),
                Effect::PerQuarterLost(v) => {
                    let q = ((f.max_hp - f.hp) * 4 / f.max_hp.max(1)).clamp(0, 3);
                    (format!("{q} quarters of HP lost"), at_level(v, l) * q)
                }
                Effect::ShieldBlast(p) => {
                    blast = shield.max(0) * at_level(p, l) / 100;
                    continue;
                }
                _ => continue,
            };
            plus(steps, &mut d, what, add * (100 + f.mods.combo_pct) / 100);
        }
        let step = self.surge_step(s) * surge;
        if step > 0 {
            d = (d * (100 + step) + 50) / 100;
            steps.push((format!("surge +{step}%"), format!("= {d}")));
        }
        if nth == 0 && f.mods.first_pct != 0 {
            d = (d * (100 + f.mods.first_pct) + 50) / 100;
            steps.push((format!("first rite +{}%", f.mods.first_pct), format!("= {d}")));
        }
        // thrown shield is not boosted by surge
        plus(steps, &mut d, "your shield, thrown".into(), blast);
        if lvl >= MAX_LEVEL {
            d = mastered(d, lvl);
            steps.push((format!("mastered +{MASTERED_PCT}%"), format!("= {d}")));
        }
        plus(steps, &mut d, "overpressure".into(), self.pressure());
        if f.weak > 0 {
            d = (d + 1) / 2;
            steps.push(("weakened: half".into(), format!("= {d}")));
        }
        let armor = self.f[1 - s].mods.armor;
        if armor != 0 {
            steps.push(("foe armor".into(), format!("{:+}", -armor)));
        }
        (d - armor).max(1)
    }

    /// The biggest combo of `s` the given stacks can pay for.
    pub fn best_combo(&self, s: usize, stacks: &[Element]) -> Option<&'static Combo> {
        COMBOS.iter().filter(|c| self.f[s].payment(c, stacks).is_some()).max_by_key(|c| self.f[s].cost(c).len())
    }

    /// Total damage if `s` cast every rite its stacks pay for now, biggest first.
    /// Each cast spends a surge step, and the first `evades` casts miss (or graze, for half).
    pub fn attack_damage(&self, s: usize) -> i32 {
        let mut stacks = self.f[s].stacks.clone();
        let (mut total, mut nth, mut surge, mut evades) = (0, self.f[s].cast_n, self.f[s].surge, self.f[1 - s].evades_left);
        let mut shield = self.f[s].shield;
        while let Some(c) = self.best_combo(s, &stacks) {
            let paid = self.f[s].payment(c, &stacks).expect("best_combo checked it");
            spend(&mut stacks, &paid);
            let echo = if c.needs.len() == 3 && self.f[s].mods.echo > 0 { 2 } else { 1 };
            for _ in 0..echo {
                let full = self.dmg(s, c, nth, surge, shield);
                if self.f[s].effects(c).any(|(e, _)| matches!(e, Effect::ShieldBlast(_))) {
                    shield = 0;
                }
                if !self.hits(s, c) {
                    continue;
                }
                if evades > 0 {
                    evades -= 1;
                    if self.f[1 - s].mods.graze > 0 {
                        total += full / 2;
                    }
                } else {
                    total += full;
                }
                (nth, surge) = (nth + 1, (surge - 1).max(0));
            }
        }
        total
    }

    fn deal_board(&mut self, s: usize, k: usize) {
        // one fewer, the usual, or one more: boards differ in danger as well as shape
        let roll = self.rng.below(3) as i32 - 1;
        let mut mines = (BASE_MINES + roll + self.f[1 - s].mods.foe_mines - self.f[s].mods.fewer_mines).clamp(2, 14);
        // debug builds only: MINEBOUND_NO_MINES=1 gives the hero mine-free boards, so one flip claims a whole board
        if cfg!(debug_assertions) && s == 0 && std::env::var_os("MINEBOUND_NO_MINES").is_some() {
            mines = 0;
        }
        let mut board = Board::deal(&mut self.rng, self.f[s].elements[k], mines as usize);
        board.flag_mines(self.f[s].mods.scouts.max(0) as usize);
        if k < self.f[s].boards.len() {
            self.f[s].boards[k] = board;
        } else {
            self.f[s].boards.push(board);
        }
    }

    /// Every wave starts on fresh boards, one per element.
    fn deal(&mut self, s: usize) {
        self.f[s].boards.clear();
        self.f[s].cur = 0;
        for k in 0..self.f[s].elements.len() {
            self.deal_board(s, k);
        }
    }

    fn start_wave(&mut self) {
        self.turn = 1;
        self.active = 0;
        for s in 0..2 {
            self.deal(s);
            self.f[s].shield += self.f[s].mods.start_shield;
            self.f[s].duds = self.f[s].mods.duds;
            let m = self.f[s].mods.clone();
            self.procs(s, &[("start_shield", m.start_shield > 0), ("scouts", m.scouts > 0), ("fewer_mines", m.fewer_mines > 0), ("foe_mines", m.foe_mines > 0)]);
        }
        for s in 0..2 {
            let d = self.f[s].mods.opening;
            if d > 0 {
                self.hurt(1 - s, d);
                self.proc(s, "opening");
            }
        }
        if !self.settle() {
            self.begin_turn();
        }
    }

    /// Shield soaks first; with `shield_block`, each point of it stops more. Returns HP actually lost.
    fn hurt(&mut self, s: usize, dmg: i32) -> i32 {
        let f = &mut self.f[s];
        let k = 1 + f.mods.shield_block.max(0);
        let soak = (f.shield * k).min(dmg).max(0);
        f.shield -= (soak + k - 1) / k;
        f.hp -= dmg - soak;
        dmg - soak
    }

    /// Brings in the next wave if the enemy just died. True if a new wave started.
    fn settle(&mut self) -> bool {
        if self.f[1].hp > 0 || self.f[0].hp <= 0 {
            return false;
        }
        let Some(next) = self.queue.pop() else { return false };
        self.log.push(format!("{} falls. {} steps in.", self.f[1].name, next.name));
        self.f[1] = next;
        self.wave += 1;
        let h = self.f[0].mods.heal_after;
        if h > 0 {
            self.f[0].hp = (self.f[0].hp + h).min(self.f[0].max_hp);
            self.proc(0, "heal_after");
        }
        self.start_wave();
        true
    }

    fn begin_turn(&mut self) {
        let (a, turn, jam) = (self.active, self.turn, self.surge_cap(self.active));
        let snatch = self.f[a].mods.pilfer.max(0) as usize;
        if snatch > 0 && !self.f[1 - a].stacks.is_empty() {
            let keep = self.f[1 - a].stacks.len().saturating_sub(snatch);
            self.f[1 - a].stacks.truncate(keep);
            self.log.push(format!("{} snatches a stack.", self.f[a].name));
        }
        self.f[1 - a].evades_left = self.f[1 - a].mods.evade;
        let m = self.f[a].mods.clone();
        let kept = m.keep_surge > 0 && self.f[a].surge > 0;
        self.procs(a, &[("reveals", m.reveals > 0), ("surge_start", m.surge_start > 0), ("keep_surge", kept), ("shield_turn", m.shield_turn > 0)]);
        let bonus = self.f[1 - a].mods.burn_bonus;
        self.procs(1 - a, &[("burn_bonus", self.f[a].burn > 0 && bonus > 0)]);
        let f = &mut self.f[a];
        f.used = 0;
        // penalties stack, but the player always keeps enough reveals to play a board
        f.reveals_left = (REVEALS + f.mods.reveals).max(if a == 0 { MIN_REVEALS } else { 1 });
        f.sense_left = f.mods.sense;
        if turn > 1 {
            // shield wears off: half of it is gone by the start of its owner's next turn
            f.shield /= 2;
        }
        f.surge = (if f.mods.keep_surge > 0 { f.surge } else { 0 }).max(f.mods.surge_start).min(jam);
        f.cast_n = 0;
        f.firing = false;
        if turn > 1 {
            f.power += f.mods.enrage + if turn % 2 == 1 { f.mods.slow_enrage } else { 0 };
        }
        f.hp = (f.hp + f.mods.regen).min(f.max_hp);
        f.shield += f.mods.shield_turn;
        if f.burn > 0 {
            // burn goes straight through shield, then halves, the way shield wears off
            let d = f.burn + bonus;
            f.hp -= d;
            f.burn /= 2;
            let msg = format!("{} burns: -{d}", f.name);
            self.log.push(msg);
            self.settle();
        }
    }

    pub fn end_turn(&mut self) {
        if self.over() {
            return;
        }
        let a = self.active;
        if self.f[a].weak > 0 {
            self.f[a].weak -= 1;
        }
        let f = &mut self.f[a];
        let banked = f.reveals_left.max(0) * f.mods.reveal_shield;
        let hoard = if f.stacks.len() >= 3 { f.mods.hoard_shield } else { 0 };
        f.shield += banked + hoard;
        self.procs(a, &[("reveal_shield", banked > 0), ("hoard_shield", hoard > 0)]);
        self.active = 1 - a;
        if self.active == 0 {
            self.turn += 1;
        }
        self.begin_turn();
    }

    /// Nothing left to flip and nothing left to fire: the turn passes on its own.
    fn tidy(&mut self, a: usize) {
        if !self.over() && self.active == a && self.f[a].reveals_left <= 0 && self.ready(a).is_empty() {
            self.end_turn();
        }
    }

    /// Put another of your boards on the table. Free, any time in your turn.
    pub fn swap(&mut self, k: usize) {
        let a = self.active;
        if !self.over() && k < self.f[a].boards.len() {
            self.f[a].cur = k;
        }
    }

    /// Flip tile `i` on the active side's current board. False if the move is not allowed.
    /// Clearing every safe tile around a crystal claims one stack of the board's element,
    /// and every safe flip in a row builds surge. A mine gives nothing, breaks the crystals
    /// it touches and ends the turn.
    pub fn reveal(&mut self, i: usize) -> bool {
        let a = self.active;
        if self.over() || self.f[a].reveals_left <= 0 {
            return false;
        }
        let cur = self.f[a].cur;
        if i >= N || !self.f[a].boards[cur].open(i) {
            return false;
        }
        let board = &mut self.f[a].boards[cur];
        if board.tiles[i].mine && !board.fresh && self.f[a].sense_left > 0 {
            // Sixth Sense: the mine is found, not set off, and the reveal is not spent
            board.tiles[i].flagged = true;
            self.f[a].sense_left -= 1;
            self.proc(a, "sense");
            return true;
        }
        if board.tiles[i].mine && !board.fresh && self.f[a].duds > 0 {
            // Lucky Coin: a dud is found harmlessly, so it breaks no crystal and keeps surge; the reveal is spent
            board.tiles[i].flagged = true;
            let f = &mut self.f[a];
            (f.duds, f.reveals_left, f.used) = (f.duds - 1, f.reveals_left - 1, f.used + 1);
            let msg = format!("{} hits a dud mine.", f.name);
            self.log.push(msg);
            self.events.push(Event::Flip);
            self.proc(a, "duds");
            self.tidy(a);
            return true;
        }
        let was: Vec<bool> = self.f[a].boards[cur].tiles.iter().map(|t| t.claimed).collect();
        let flip = self.f[a].boards[cur].flip(i, &mut self.rng);
        let tiles: Vec<usize> = (0..N).filter(|&k| self.f[a].boards[cur].tiles[k].claimed && !was[k]).collect();
        let jam = self.surge_cap(a);
        let f = &mut self.f[a];
        f.reveals_left -= 1;
        f.used += 1;
        let spent = f.boards[cur].spent();
        let element = f.boards[cur].element;
        if !flip.mine {
            f.surge = (f.surge + 1).min(jam);
            let n = flip.claimed as i32;
            let gain = flip.claimed * (1 + f.mods.claim_bonus.max(0) as usize);
            f.stacks.extend(std::iter::repeat(element).take(gain));
            f.hp = (f.hp + n * f.mods.claim_heal).min(f.max_hp);
            f.shield += n * f.mods.claim_shield;
            let zap = n * f.mods.claim_zap;
            let m = f.mods.clone();
            self.events.push(if n > 0 { Event::Claim { side: a, element, tiles } } else { Event::Flip });
            let claimed = n > 0;
            self.procs(a, &[("claim_bonus", claimed && m.claim_bonus > 0), ("claim_heal", claimed && m.claim_heal > 0), ("claim_shield", claimed && m.claim_shield > 0), ("claim_zap", zap > 0)]);
            if spent {
                self.deal_board(a, cur);
            }
            if zap > 0 {
                self.hurt(1 - a, zap);
                if self.settle() {
                    return true;
                }
            }
            self.tidy(a);
            return true;
        }
        self.events.push(Event::Mine);
        let name = f.name.clone();
        let salvaged = f.mods.salvage > 0 && flip.broken > 0;
        if salvaged {
            f.stacks.extend(std::iter::repeat(element).take(flip.broken));
        }
        f.surge = 0;
        let d = (MINE_DMG - f.mods.mine_guard).max(0);
        let (spite, keep_going, guard) = (f.mods.mine_spite, f.mods.mine_no_end > 0, f.mods.mine_guard);
        self.procs(a, &[("salvage", salvaged)]);
        if spent {
            self.deal_board(a, cur);
        }
        self.hurt(a, d);
        self.log.push(format!("{name} hits a mine: -{d}"));
        self.procs(a, &[("mine_guard", guard > 0)]);
        // some opponents feed on your mistakes
        let t = 1 - a;
        for _ in 0..self.f[t].mods.foe_mine_gift.max(0) {
            let k = self.rng.below(self.f[t].elements.len());
            let e = self.f[t].elements[k];
            self.f[t].stacks.push(e);
        }
        if spite > 0 {
            self.hurt(t, spite);
            self.proc(a, "mine_spite");
        }
        if !self.settle() {
            if keep_going {
                self.proc(a, "mine_no_end");
                self.tidy(a);
            } else {
                self.end_turn();
            }
        }
        true
    }

    pub fn toggle_flag(&mut self, i: usize) {
        let f = &mut self.f[0];
        if let Some(t) = f.boards[f.cur].tiles.get_mut(i) {
            if !t.revealed && !t.crystal && !t.rock {
                t.flagged = !t.flagged;
            }
        }
    }

    /// Cast a combo skill, paying one stack of each element it needs (and any toll from the most plentiful).
    pub fn cast(&mut self, c: &'static Combo) -> bool {
        let a = self.active;
        match self.f[a].payment(c, &self.f[a].stacks) {
            Some(paid) => self.cast_with(c, &paid),
            None => false,
        }
    }

    /// Cast `c` paying exactly the gems in `paid`, as the player chose them in its sockets.
    pub fn cast_with(&mut self, c: &'static Combo, paid: &[Element]) -> bool {
        let a = self.active;
        if self.over() || !self.f[a].fits(c, paid) || !pays(&self.f[a].stacks, paid) {
            return false;
        }
        spend(&mut self.f[a].stacks, paid);
        let (wave, burn) = (self.wave, self.f[1 - a].burn);
        self.land(a, c);
        if c.needs.len() == 3 && self.f[a].mods.echo > 0 && self.wave == wave && !self.over() {
            // Echo Chamber: the same combo again, free. An echoed Detonate sets off the same burn again.
            if self.f[a].effects(c).any(|(e, _)| e == Effect::Detonate) {
                self.f[1 - a].burn = burn;
            }
            self.proc(a, "echo");
            self.land(a, c);
        }
        true
    }

    /// Cast the biggest rite the active side can afford. False when its stacks pay for none.
    pub fn cast_best(&mut self) -> bool {
        let a = self.active;
        self.best_combo(a, &self.f[a].stacks).is_some_and(|c| self.cast(c))
    }

    fn land(&mut self, a: usize, c: &'static Combo) {
        let t = 1 - a;
        let (dmg, hits) = (self.hit_damage(a, c), self.hits(a, c));
        let (lvl, element) = (self.f[a].level(c), c.needs[0]);
        let (name, skill) = (c.title(lvl), (combo_id(c), lvl));
        let (nth, surged) = (self.f[a].cast_n, self.f[a].surge > 0);
        // every rite that hits spends a surge step, so firing early costs the later ones damage;
        // a setup rite that deals none (Bulwark, Mistveil) spends nothing
        if hits {
            self.f[a].cast_n += 1;
            self.f[a].surge = (self.f[a].surge - 1).max(0);
        }
        // a rite that does not hit is not aimed at the foe's body, so it cannot be evaded
        if !hits {
        } else if self.f[t].evades_left > 0 && self.f[t].mods.graze > 0 {
            // a graze: half damage (already in `dmg`), every effect still lands
            self.f[t].evades_left -= 1;
            self.proc(t, "evade");
            self.log.push(format!("{}: {name} only grazes", self.f[a].name));
        } else if self.f[t].evades_left > 0 {
            // the whole skill whiffs: no damage, no effects, stacks still spent
            self.f[t].evades_left -= 1;
            self.proc(t, "evade");
            self.events.push(Event::Cast { from: a, name, element, miss: true, skill });
            self.log.push(format!("{}: {name} misses", self.f[a].name));
            self.tidy(a);
            return;
        }
        let m = self.f[a].mods.clone();
        let effects: Vec<(Effect, usize)> = self.f[a].effects(c).collect();
        if effects.iter().any(|&(e, _)| e == Effect::Detonate) {
            // its damage is in hit_damage; cleared first, so this cast's own burn stays on
            self.f[t].burn = 0;
        }
        self.f[t].burn += m.rite_burn.max(0);
        let desperate = self.desperate(a);
        self.procs(a, &[
            ("power", hits && m.power > 0),
            ("rite_bonus", hits && m.rite_bonus > 0),
            ("rite_burn", m.rite_burn > 0),
            ("combo_pct", m.combo_pct > 0),
            ("first_pct", hits && nth == 0 && m.first_pct > 0),
            ("chain", hits && nth > 0 && m.chain > 0),
            ("surge_pct", hits && surged && m.surge_pct > 0),
            ("desperate", hits && desperate),
        ]);
        let armored = hits && self.f[t].mods.armor > 0;
        self.procs(t, &[("armor", armored)]);
        let pct = 100 + self.f[a].mods.combo_pct;
        for (e, lvl) in effects {
            let up = |v: i32| at_level(v, lvl);
            match e {
                Effect::Damage(_) => {} // already in hit_damage
                Effect::Burn(n) => self.f[t].burn += up(n),
                Effect::Shield(v) => self.f[a].shield += mastered(up(v) * pct / 100, lvl),
                Effect::Heal(v) => self.f[a].hp = (self.f[a].hp + mastered(up(v) * pct / 100, lvl)).min(self.f[a].max_hp),
                Effect::Weaken(n) => self.f[t].weak = self.f[t].weak.max(up(n)),
                Effect::Drain(n) => {
                    let keep = self.f[t].stacks.len().saturating_sub(up(n as i32) as usize);
                    self.f[t].stacks.truncate(keep);
                }
                Effect::Pierce => self.f[t].shield = 0,
                // in hit_damage, worked out before any effect lands
                Effect::PerSurge(_) | Effect::Detonate | Effect::PerReveal(_) | Effect::PerBurn(_) | Effect::PerQuarterLost(_) => {}
                Effect::Surge(v) => self.f[a].surge = (self.f[a].surge + up(v)).min(self.surge_cap(a)),
                // capped, or casting it again and again would grow shield without end
                Effect::DoubleShield(cap) => self.f[a].shield += self.f[a].shield.clamp(0, up(cap)),
                Effect::ShieldBlast(_) => self.f[a].shield = 0,
                Effect::SelfDamage(v) => self.f[a].hp = (self.f[a].hp - up(v)).max(1),
                Effect::Reveal(n) => self.f[a].reveals_left += up(n),
                Effect::Shatter(n) => self.shatter(t, up(n as i32) as usize),
                Effect::Bury(n) => {
                    let cur = self.f[t].cur;
                    self.f[t].boards[cur].bury(up(n as i32) as usize, &mut self.rng);
                }
            }
        }
        let dealt = self.hurt(t, dmg);
        let steal = dealt * self.f[a].mods.lifesteal_pct / 100;
        self.f[a].hp = (self.f[a].hp + steal).min(self.f[a].max_hp);
        self.procs(a, &[("lifesteal_pct", steal > 0)]);
        let thorns = self.f[t].mods.thorns;
        if thorns > 0 && hits {
            self.hurt(a, thorns);
            self.proc(t, "thorns");
        }
        self.events.push(Event::Cast { from: a, name, element, miss: false, skill });
        let line = if hits { format!("{}: {name} for {dmg}", self.f[a].name) } else { format!("{}: {name}", self.f[a].name) };
        self.log.push(line);
        if !self.settle() {
            self.tidy(a);
        }
    }

    /// Breaks up to `n` of `s`'s claimable crystals on the board it has up; a board left with none is dealt anew.
    fn shatter(&mut self, s: usize, n: usize) {
        let cur = self.f[s].cur;
        let board = &mut self.f[s].boards[cur];
        let live: Vec<usize> = (0..N).filter(|&i| board.live(i)).take(n).collect();
        for &i in &live {
            board.tiles[i].broken = true;
        }
        if !live.is_empty() && board.spent() {
            self.deal_board(s, cur);
        }
    }

    /// One move for whoever is active. Drives the enemy, and the player in simulations.
    pub fn ai_step(&mut self) {
        if self.over() {
            return;
        }
        let a = self.active;
        if self.f[a].firing {
            // one skill per step, so each one can be seen landing
            if !self.cast_best() && self.active == a {
                self.end_turn();
            }
            return;
        }
        let f = &self.f[a];
        let n = f.stacks.len();
        // stacks that pay for some rite: gems no rite can use yet are not ammunition
        let armed = !self.ready(a).is_empty();
        let lethal = armed && self.attack_damage(a) >= self.f[1 - a].hp + self.f[1 - a].shield;
        let can_reveal = f.reveals_left > 0;
        if can_reveal && !lethal {
            // weigh every open tile on every board: progress toward claiming a crystal against
            // the mine risk (a mine next to a crystal also breaks it), with a nudge toward
            // elements it does not hold yet (that is how combos happen)
            let mut best: Option<(usize, usize, f32, f32)> = None;
            // holding nothing that pays for a rite, more of the same gem never helps: work only the
            // boards of gems it lacks, even a jammed one, since a mine there clears it for a new deal
            let lacking = |e: &Element| !f.stacks.contains(e);
            let only_lacking = !armed && f.boards.iter().any(|b| lacking(&b.element) && (0..N).any(|i| b.open(i)));
            for (k, board) in f.boards.iter().enumerate().filter(|(_, b)| !only_lacking || lacking(&b.element)) {
                let fresh = if f.stacks.contains(&board.element) { 1.0 } else if armed { 1.3 } else { 3.0 };
                for i in (0..N).filter(|&i| board.open(i)) {
                    let risk = board.risk(i);
                    let (mut progress, mut at_stake) = (0.3, 0.6);
                    for c in Board::neighbors(i).filter(|&c| board.live(c)) {
                        progress += 1.0 / board.left_around(c).max(1) as f32;
                        at_stake += 0.6;
                    }
                    let score = (1.0 - risk) * progress * fresh - risk * at_stake;
                    if best.is_none_or(|b| score > b.2) {
                        best = Some((k, i, score, risk));
                    }
                }
            }
            if let Some((k, i, score, risk)) = best {
                if !armed || (risk < 0.4 && score > 0.0) {
                    self.swap(k);
                    self.reveal(i);
                    return;
                }
            }
        }
        // firing when it declined a risky reveal keeps a cornered fighter from stalling
        if armed && (lethal || n >= f.aggro || can_reveal) {
            self.f[a].firing = true;
            self.cast_best();
        } else {
            self.end_turn();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Element::*;

    /// Cast every rite the stacks pay for at the current enemy, biggest first.
    fn unleash(b: &mut Battle) {
        let (a, wave) = (b.active, b.wave);
        while b.active == a && b.wave == wave && b.cast_best() {}
    }

    fn duel() -> Battle {
        let me = Fighter::new("Me", 0, "", 50, 50, 3, &[Ember, Gale, Stone], Mods::default(), 3);
        let foe = Fighter::new("Foe", 0, "", 100, 100, 1, &[Tide, Spark], Mods::default(), 3);
        Battle::new(me, vec![foe], Rng::new(1))
    }

    fn combo(name: &str) -> &'static Combo {
        COMBOS.iter().find(|c| c.name == name).unwrap()
    }

    #[test]
    fn stacks_have_no_cap_and_surge_damage_rounds_to_nearest() {
        let mut b = duel();
        b.f[0].stacks = vec![Ember; 20];
        b.f[0].reveals_left = 9;
        let open = (0..N).find(|&i| b.f[0].board().open(i)).unwrap();
        assert!(b.reveal(open), "holding many stacks never blocks a flip");
        b.f[0].surge = 1;
        // Raze: (2 stacks x 3 power + 2) x1.2 = 9.6
        assert_eq!(b.hit_damage(0, combo("Raze")), 10);
    }

    /// Flips every safe tile around the first live crystal of the player's current board.
    fn clear_a_crystal(b: &mut Battle) {
        let c = (0..N).find(|&i| b.f[0].board().live(i)).unwrap();
        loop {
            let board = b.f[0].board();
            if !board.live(c) {
                break;
            }
            // the first flip on a board may move mines, so look again every step
            let Some(j) = Board::neighbors(c).find(|&j| board.open(j) && !board.tiles[j].mine) else { break };
            b.f[0].reveals_left = 9;
            b.reveal(j);
        }
    }

    #[test]
    fn clearing_around_a_crystal_gives_a_stack_and_a_mine_gives_nothing() {
        let mut b = duel();
        assert_eq!(b.f[0].boards.len(), 3, "one board per element");
        let element = b.f[0].board().element;
        let crystal = (0..N).find(|&i| b.f[0].board().tiles[i].crystal).unwrap();
        assert!(!b.reveal(crystal), "crystals cannot be flipped");
        clear_a_crystal(&mut b);
        assert!(!b.f[0].stacks.is_empty(), "the cleared crystal pays out");
        assert!(b.f[0].stacks.iter().all(|&e| e == element));
        assert!(b.f[0].surge > 0, "safe flips build surge");
        assert_eq!(b.active, 0);

        let held = b.f[0].stacks.clone();
        let hp = b.f[0].hp;
        b.f[0].reveals_left = 9;
        let mine = (0..N).find(|&i| b.f[0].board().tiles[i].mine && b.f[0].board().open(i)).unwrap();
        assert!(b.reveal(mine));
        assert_eq!(b.f[0].hp, hp - MINE_DMG);
        assert_eq!(b.f[0].stacks, held, "no stack from a mine, earlier stacks stay");
        assert_eq!((b.active, b.f[0].surge), (1, 0), "a mine ends the turn and wipes surge");
    }

    #[test]
    fn swapping_boards_changes_the_element_you_collect() {
        let mut b = duel();
        b.swap(2);
        assert_eq!(b.f[0].board().element, Stone);
        clear_a_crystal(&mut b);
        assert_eq!(b.f[0].stacks[0], Stone);
        b.swap(0);
        assert!(b.f[0].boards[2].tiles.iter().any(|t| t.revealed), "a board keeps its state while you are away");
    }

    #[test]
    fn skills_spend_only_their_cost_and_the_turn_goes_on() {
        let mut b = duel();
        b.f[0].stacks = vec![Ember, Gale, Stone];
        assert!(!b.cast(combo("Bulwark")), "cannot pay for a combo of other elements");
        assert!(b.cast(combo("Raze")));
        // 2 stacks x 3 power + 2
        assert_eq!(b.f[1].hp, 92);
        assert_eq!(b.f[0].stacks, vec![Stone]);
        assert_eq!(b.active, 0, "casting does not end the turn");
        assert!(!b.cast_best(), "a lone gem pays for no rite");
        assert_eq!((b.f[1].hp, b.f[0].stacks.clone()), (92, vec![Stone]), "and it waits in the stacks");
        assert_eq!(b.events.iter().filter(|e| matches!(e, Event::Cast { .. })).count(), 1);
    }

    #[test]
    fn an_upgraded_skill_costs_one_more_gem_and_hits_harder() {
        let mut b = duel();
        let steam = combo("Steam Burst");
        b.f[0].elements = vec![Ember, Tide];
        b.f[0].ups = vec![combo_id(steam)];
        assert_eq!(b.f[0].cost(steam), vec![Ember, Tide, Ember]);
        b.f[0].stacks = vec![Ember, Tide];
        assert!(!b.cast(steam), "two gems no longer pay for it");
        b.f[0].stacks = vec![Ember, Tide, Ember];
        // 3 stacks x 3 power + 4 x 1.5
        assert_eq!(b.attack_damage(0), 15);
        let reveals = b.f[0].reveals_left;
        assert!(b.cast(steam));
        assert_eq!((b.f[1].hp, b.f[0].stacks.len()), (85, 0));
        assert_eq!(b.f[0].reveals_left, reveals + 2, "steam clears the view: +2 reveals at Lv 2");
        assert!(b.events.iter().any(|e| matches!(e, Event::Cast { name: "Scald Geyser", .. })));
        assert_eq!(rite_text(steam, 3, None), "+8 damage, +3 reveals, mastered: +50% damage, shield and healing");
    }

    #[test]
    fn surge_multiplies_skill_damage() {
        let mut b = duel();
        b.f[0].stacks = vec![Ember, Gale, Stone];
        // Upheaval: 3 stacks x 3 power + 11
        assert_eq!(b.attack_damage(0), 20);
        b.f[0].surge = MAX_SURGE;
        assert_eq!(b.attack_damage(0), 40);

        unleash(&mut b);
        assert_eq!((b.f[1].hp, b.f[0].hp), (100 - 40, 50 - 4), "Upheaval costs its caster 4 HP");
        assert!(b.f[0].stacks.is_empty());
        assert_eq!(b.f[0].surge, MAX_SURGE - 1, "a cast spends a surge step");

        // so a second rite fires at one step less, and the preview knows it
        let mut b = duel();
        b.f[0].stacks = vec![Ember, Gale, Ember, Gale];
        b.f[0].surge = MAX_SURGE;
        let promised = b.attack_damage(0);
        assert_eq!(promised, 16 + 14, "Raze at x2.0, then at x1.8");
        unleash(&mut b);
        assert_eq!((b.f[1].hp, b.f[0].surge), (100 - promised, MAX_SURGE - 2));

        // a Fuse brand detonates the burn on the foe: 3 damage per burn, and the burn is gone,
        // except what this same cast sets burning
        let mut b = duel();
        let (magma, fuse) = (combo("Magma"), BRANDS.iter().position(|x| x.name == "Fuse").unwrap());
        b.f[0].brands = vec![(combo_id(magma), fuse)];
        b.f[0].mods.rite_burn = 1;
        b.f[0].stacks = vec![Ember, Stone];
        b.f[1].burn = 3;
        assert_eq!(b.attack_damage(0), 6 + 3 + 9);
        assert!(b.cast(magma));
        assert_eq!((b.f[1].hp, b.f[1].burn), (100 - 18, 1));

        // Plasma: +1 damage per surge step held
        let mut b = duel();
        b.f[0].elements = vec![Ember, Spark];
        b.f[0].stacks = vec![Ember, Spark];
        let flat = b.attack_damage(0);
        b.f[0].surge = 2;
        assert!(b.attack_damage(0) > flat + 4, "surge both multiplies and adds");
    }

    #[test]
    fn enemy_rules_change_what_a_hit_does() {
        let mut b = duel();
        b.f[1].mods.armor = 2;
        b.f[0].stacks = vec![Ember, Gale, Ember, Gale];
        let raze = combo("Raze");
        assert_eq!(b.hit_damage(0, raze), 6, "8 minus 2 armor");
        b.f[1].mods.armor = 0;
        b.f[1].evades_left = 1;
        assert_eq!((b.hit_damage(0, raze), b.attack_damage(0)), (0, 8), "previews know the first hit misses");
        b.cast(raze);
        assert_eq!(b.f[1].hp, 100, "the first skill misses");
        b.cast(raze);
        assert_eq!(b.f[1].hp, 92);
        // a grazer takes half from the first rite, and its effects (burn) still land
        b.f[1].mods.graze = 1;
        b.f[1].evades_left = 1;
        b.f[1].burn = 0;
        b.f[0].mods.rite_burn = 1;
        b.f[0].stacks = vec![Ember, Gale, Ember, Gale];
        assert_eq!((b.hit_damage(0, raze), b.attack_damage(0)), (4, 12), "previews know the first hit grazes");
        b.cast(raze);
        assert_eq!(b.f[1].hp, 88, "the first skill grazes for half");
        assert!(b.f[1].burn > 0, "and its burn lands");

        let mut b = duel();
        b.f[1].mods.foe_surge_cap = 2;
        b.f[0].surge = 0;
        clear_a_crystal(&mut b);
        assert!(b.f[0].surge <= 2, "surge is jammed");
    }

    #[test]
    fn charms_reward_different_plays() {
        let mut b = duel();
        b.f[0].mods.rite_bonus = 3;
        b.f[0].mods.first_pct = 100;
        b.f[0].stacks = vec![Ember, Gale, Ember, Gale];
        // first Raze: (6 + 3 + 2) doubled, second: 6 + 3 + 2
        assert_eq!(b.attack_damage(0), 33);
        unleash(&mut b);
        assert_eq!(b.f[1].hp, 67);

        // Kindling (and Scaldwright): every rite sets the foe burning
        let mut b = duel();
        b.f[0].mods.rite_burn = 2;
        b.f[0].stacks = vec![Ember, Stone];
        b.cast(combo("Magma"));
        assert_eq!(b.f[1].burn, 2);

        let mut b = duel();
        b.f[0].mods.claim_zap = 2;
        clear_a_crystal(&mut b);
        assert!(b.f[1].hp < 100, "claiming a crystal zaps the enemy");
    }

    #[test]
    fn every_charm_flashes_on_a_hook_the_battle_reports() {
        // these act outside battle, so nothing flashes for them
        let between_fights = ["shard_pct", "max_hp"];
        for c in CHARMS {
            assert!(HOOKS.contains(&c.hook) || between_fights.contains(&c.hook), "{} has hook {}", c.name, c.hook);
        }
    }

    #[test]
    fn turn_shaping_charms() {
        // Chain Lightning: each rite after the first deals 3 more than the one before
        let mut b = duel();
        b.f[0].mods.chain = 3;
        b.f[0].stacks = vec![Ember, Gale, Ember, Gale, Ember, Gale];
        assert_eq!(b.attack_damage(0), 8 + 11 + 14);

        // Echo Chamber: a three-element combo lands twice
        let mut b = duel();
        b.f[0].mods.echo = 1;
        b.f[0].stacks = vec![Ember, Gale, Stone];
        assert!(b.cast(combo("Upheaval")));
        assert_eq!(b.f[1].hp, 100 - 20 * 2);
        assert!(b.events.iter().any(|e| matches!(e, Event::Proc { side: 0, hook: "echo" })));

        // an echoed Fuse detonates the burn twice, as its preview promised
        let mut b = duel();
        let (upheaval, fuse) = (combo("Upheaval"), BRANDS.iter().position(|x| x.name == "Fuse").unwrap());
        b.f[0].brands = vec![(combo_id(upheaval), fuse)];
        b.f[0].mods.echo = 1;
        b.f[0].stacks = vec![Ember, Gale, Stone];
        b.f[1].burn = 3;
        let promised = b.attack_damage(0);
        assert_eq!(promised, (20 + 9) * 2);
        assert!(b.cast(upheaval));
        assert_eq!((b.f[1].hp, b.f[1].burn), (100 - promised, 0));

        // Hourglass and Hoarder's Sack: unused reveals and banked stacks become shield
        let mut b = duel();
        b.f[0].mods.reveal_shield = 2;
        b.f[0].mods.hoard_shield = 6;
        b.f[0].stacks = vec![Ember; 5];
        let left = b.f[0].reveals_left;
        b.end_turn();
        assert_eq!(b.f[0].shield, left * 2 + 6, "the sack gives a flat 6, however many stacks are held");
    }

    #[test]
    fn mine_charms() {
        // Powder Keg: a mine hurts but the turn goes on
        let mut b = duel();
        b.f[0].mods.mine_no_end = 1;
        clear_a_crystal(&mut b);
        let hp = b.f[0].hp;
        b.f[0].reveals_left = 9;
        let mine = (0..N).find(|&i| b.f[0].board().tiles[i].mine && b.f[0].board().open(i)).unwrap();
        assert!(b.reveal(mine));
        assert_eq!((b.active, b.f[0].hp, b.f[0].surge), (0, hp - MINE_DMG, 0));

        // Sixth Sense: the first mine each turn is flagged instead, and costs no reveal
        let mut b = duel();
        b.f[0].sense_left = 1;
        clear_a_crystal(&mut b);
        let (hp, reveals) = (b.f[0].hp, b.f[0].reveals_left);
        let mine = (0..N).find(|&i| b.f[0].board().tiles[i].mine && b.f[0].board().open(i)).unwrap();
        assert!(b.reveal(mine));
        assert!(b.f[0].board().tiles[mine].flagged && !b.f[0].board().tiles[mine].revealed);
        assert_eq!((b.f[0].hp, b.f[0].reveals_left, b.active), (hp, reveals, 0));

        // Lucky Coin: a dud costs the reveal but no HP, no surge, no turn, and breaks no crystal
        for seed in 0..200 {
            let me = Fighter::new("Me", 0, "", 50, 50, 3, &[Ember], Mods::default(), 3);
            let foe = Fighter::new("Foe", 0, "", 100, 100, 1, &[Tide], Mods::default(), 3);
            let mut b = Battle::new(me, vec![foe], Rng::new(seed));
            b.f[0].boards[0].fresh = false;
            b.f[0].duds = 1;
            b.f[0].surge = 2;
            let board = b.f[0].board();
            let Some(mine) = (0..N).find(|&i| board.tiles[i].mine && board.open(i) && Board::neighbors(i).any(|j| board.live(j))) else { continue };
            let (hp, reveals) = (b.f[0].hp, b.f[0].reveals_left);
            assert!(b.reveal(mine));
            let board = b.f[0].board();
            assert!(board.tiles[mine].flagged && Board::neighbors(mine).all(|j| !board.tiles[j].broken));
            assert_eq!((b.f[0].hp, b.f[0].reveals_left, b.f[0].surge, b.f[0].duds, b.active), (hp, reveals - 1, 2, 0, 0));
            return;
        }
        panic!("no seed put a mine next to a crystal");
    }

    #[test]
    fn shields_wear_off_and_reveals_have_a_floor() {
        let mut b = duel();
        b.f[0].shield = 10;
        b.end_turn();
        assert_eq!(b.f[0].shield, 10, "shield holds through the enemy's turn");
        b.end_turn();
        assert_eq!(b.f[0].shield, 5, "and is half gone when yours comes round");

        let mut b = duel();
        b.f[0].mods.reveals = -3;
        b.f[1].mods.reveals = -3;
        b.end_turn();
        assert_eq!(b.f[1].reveals_left, REVEALS - 3, "enemies have no floor");
        b.end_turn();
        assert_eq!(b.f[0].reveals_left, MIN_REVEALS);
    }

    #[test]
    fn a_brand_adds_its_effect_flat_and_free() {
        let mut b = duel();
        let raze = combo("Raze");
        let ward = BRANDS.iter().position(|x| x.name == "Ward").unwrap();
        b.f[0].elements = vec![Ember, Gale];
        b.f[0].ups = vec![combo_id(raze)];
        let cost = b.f[0].cost(raze);
        b.f[0].brands = vec![(combo_id(raze), ward)];
        assert_eq!(b.f[0].cost(raze), cost, "no extra gem");
        assert_eq!(b.f[0].describe(raze), "+3 damage, +3 damage per quarter of HP lost, gain 5 shield", "the brand stays at level 1");
        b.f[0].stacks = cost;
        assert!(b.cast(raze));
        assert_eq!(b.f[0].shield, 5);
    }

    #[test]
    fn a_toll_takes_one_more_gem_of_any_kind() {
        let mut b = duel();
        let wildfire = combo("Raze");
        b.f[0].mods.toll = 1;
        b.f[0].elements = vec![Ember, Gale, Stone];
        b.f[0].stacks = vec![Ember, Gale];
        assert!(b.ready(0).is_empty(), "two gems no longer pay for a two-gem rite");
        b.f[0].stacks = vec![Ember, Gale, Stone, Stone, Ember];
        assert!(!b.cast_with(wildfire, &[Ember, Gale]), "the toll socket must be filled");
        assert!(b.cast_with(wildfire, &[Ember, Gale, Ember]), "any gem pays the toll");
        assert_eq!(b.f[0].stacks, vec![Stone, Stone]);
        b.f[0].stacks = vec![Ember, Gale, Stone, Stone, Ember];
        assert!(b.cast(wildfire));
        assert_eq!(b.f[0].stacks, vec![Stone, Ember], "left alone, it pays in the most plentiful gem");
    }

    #[test]
    fn rites_reach_into_the_foes_board() {
        // Thunderclap breaks a crystal on the board the foe has up
        let mut b = duel();
        b.f[0].elements = vec![Gale, Spark];
        b.f[0].stacks = vec![Gale, Spark];
        let live = |b: &Battle| (0..N).filter(|&i| b.f[1].board().live(i)).count();
        let before = live(&b);
        assert!(b.cast(combo("Thunderclap")));
        assert_eq!(live(&b), before - 1);

        // Sandstorm buries 2 more mines in it, away from the numbers already read
        let mut b = duel();
        b.f[0].stacks = vec![Gale, Stone];
        let mines = |b: &Battle| b.f[1].board().tiles.iter().filter(|t| t.mine).count();
        let before = mines(&b);
        assert!(b.cast(combo("Sandstorm")));
        assert_eq!((mines(&b), b.f[1].board().mines), (before + 2, before + 2));
    }

    #[test]
    fn burn_stacks_then_halves() {
        let mut b = duel();
        b.f[1].burn = 4;
        b.f[0].elements = vec![Ember, Tide, Spark];
        b.f[0].stacks = vec![Ember, Tide, Spark];
        // Boilover: 3 gems x 3 power, +1 for each burn already on the foe
        assert_eq!(b.hit_damage(0, combo("Boilover")), 9 + 4);
        assert!(b.cast(combo("Boilover")));
        assert_eq!(b.f[1].burn, 7, "new burn adds to what is there");
        b.f[0].mods.burn_bonus = 2;
        let hp = b.f[1].hp;
        b.end_turn();
        assert_eq!((b.f[1].hp, b.f[1].burn), (hp - 7 - 2, 3), "it burns for 7 and the bonus, then halves");
    }

    #[test]
    fn rites_without_damage_only_do_what_they_say() {
        let mut b = duel();
        b.f[1].mods.thorns = 2;
        b.f[1].evades_left = 1;
        b.f[0].stacks = vec![Gale, Stone];
        assert_eq!(b.hit_damage(0, combo("Sandstorm")), 0);
        assert!(b.cast(combo("Sandstorm")));
        assert_eq!((b.f[1].hp, b.f[0].hp, b.f[1].weak), (100, 50, 2), "no damage, no thorns, and its weaken lands");
        assert_eq!(b.f[1].evades_left, 1, "and it does not use up the foe's evade");
        // an enemy still strikes with every rite: for most, that is their whole attack
        b.f[1].stacks = vec![Tide, Spark];
        assert!(b.hit_damage(1, combo("Conduction")) > 0);
    }

    #[test]
    fn setup_rites_spend_no_surge_and_mastery_and_echo_add_up() {
        // a rite that deals no damage keeps your surge and your first-rite slot for the hit after it
        let mut b = duel();
        b.f[0].surge = 3;
        b.f[0].stacks = vec![Gale, Stone];
        assert!(b.cast(combo("Sandstorm")));
        assert_eq!((b.f[0].surge, b.f[0].cast_n), (3, 0));

        // mastered: a level 3 rite is half again as strong, shield and heal included
        let mut b = duel();
        let magma = combo("Magma");
        let base = b.hit_damage(0, magma);
        b.f[0].ups = vec![combo_id(magma); 2];
        let (d, steps) = b.hit_steps(0, magma);
        assert_eq!(d, (4 * 3 + 6) * 3 / 2, "4 gems x 3 power, +6, then mastered");
        assert!(d > base && steps.iter().any(|s| s.0.starts_with("mastered")));
        let bulwark = combo("Bulwark");
        b.f[0].ups = vec![combo_id(bulwark); 2];
        b.f[0].stacks = bulwark.cost(3);
        assert!(b.cast(bulwark));
        assert_eq!(b.f[0].shield, 24 * 3 / 2);

        // echoed Geode Surge throws the shield once: the second cast finds it gone
        let mut b = duel();
        b.f[0].mods.echo = 1;
        b.f[0].shield = 20;
        b.f[0].stacks = vec![Tide, Stone, Spark];
        let promised = b.attack_damage(0);
        assert!(b.cast(combo("Geode Surge")));
        assert_eq!(b.f[1].hp, 100 - promised, "the AI's estimate matches what lands");
    }

    #[test]
    fn slow_enrage_and_a_heal_between_waves() {
        let mut b = duel();
        b.f[1].mods.slow_enrage = 1;
        let power = b.f[1].power;
        for _ in 0..5 {
            b.end_turn(); // the enemy's turns 1 to 5 begin
            b.end_turn();
        }
        assert_eq!(b.f[1].power, power + 2, "+1 on turns 3 and 5");

        let me = Fighter::new("Me", 0, "", 50, 50, 3, &[Ember, Gale, Stone], Mods { heal_after: 10, ..Mods::default() }, 3);
        let foe = |hp| Fighter::new("Foe", 0, "", hp, hp, 1, &[Tide, Spark], Mods::default(), 3);
        let mut b = Battle::new(me, vec![foe(1), foe(100)], Rng::new(1));
        b.f[0].hp = 20;
        b.f[0].stacks = vec![Ember, Gale];
        assert!(b.cast(combo("Raze")));
        assert_eq!((b.wave, b.f[0].hp), (2, 30), "Field Rations heals as the next wave steps in");
    }

    #[test]
    fn deepforger_builds_the_wall_then_throws_it() {
        let mut b = duel();
        b.f[0].elements = vec![Tide, Stone, Spark];
        b.f[0].mods.shield_block = 1;
        b.f[0].stacks = vec![Tide, Stone, Stone, Spark, Tide, Stone, Spark];
        assert!(b.cast(combo("Bulwark")));
        assert!(b.cast(combo("Lodestone")));
        assert_eq!((b.f[0].shield, b.f[1].hp), (12 * 2 + 5, 100), "doubled, then 5 more, and no damage");
        // Geode Surge: 3 gems x 3 power, +6, then the 29 shield thrown at 150% (43), and the steps add up
        let (d, steps) = b.hit_steps(0, combo("Geode Surge"));
        assert_eq!(d, 9 + 6 + 43);
        assert_eq!(steps.iter().map(|s| s.1.trim_start_matches('+').parse::<i32>().unwrap()).sum::<i32>(), d);
        assert!(b.cast(combo("Geode Surge")));
        assert_eq!((b.f[0].shield, b.f[1].hp), (0, 100 - 58));
        // Plated: each point of shield stops 2 damage
        b.f[0].shield = 5;
        assert_eq!((b.hurt(0, 7), b.f[0].shield), (0, 1));
    }

    #[test]
    fn reveals_rage_and_surge_pay_off() {
        // Tempest: +1 per reveal used this turn
        let mut b = duel();
        b.f[0].elements = vec![Tide, Gale, Spark];
        let base = b.hit_damage(0, combo("Tempest"));
        b.f[0].used = 4;
        assert_eq!(b.hit_damage(0, combo("Tempest")), base + 4);
        // Conduction: surge after the cast
        b.f[0].stacks = vec![Tide, Spark];
        assert!(b.cast(combo("Conduction")));
        assert_eq!(b.f[0].surge, 2);
        // Raze: +2 for each quarter of HP lost; Upheaval costs HP, but never the last one
        let mut b = duel();
        let raze = combo("Raze");
        let fresh = b.hit_damage(0, raze);
        b.f[0].hp = 24;
        assert_eq!(b.hit_damage(0, raze), fresh + 4, "26 of 50 lost: two quarters");
        b.f[0].hp = 3;
        b.f[0].stacks = vec![Ember, Gale, Stone];
        assert!(b.cast(combo("Upheaval")));
        assert_eq!(b.f[0].hp, 1);
    }

    #[test]
    fn next_wave_arrives_when_the_enemy_dies() {
        let me = Fighter::new("Me", 0, "", 50, 50, 50, &[Ember, Gale], Mods::default(), 1);
        let foes = vec![
            Fighter::new("A", 0, "", 5, 5, 1, &[Tide], Mods::default(), 1),
            Fighter::new("B", 0, "", 5, 5, 1, &[Tide], Mods::default(), 1),
        ];
        let mut b = Battle::new(me, foes, Rng::new(2));
        b.f[0].stacks = vec![Ember, Gale, Ember, Gale];
        unleash(&mut b);
        assert_eq!((b.wave, b.f[1].name.as_str(), b.active), (2, "B", 0));
        assert_eq!(b.f[0].stacks.len(), 2, "unleash stops when the enemy falls");
        assert!(!b.over());
        unleash(&mut b);
        assert!(b.won());
    }
}
