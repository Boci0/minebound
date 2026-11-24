use crate::{
    board::{Board, CAVE, N},
    data::*,
    rng::Rng,
};

/// What a cave-in costs whoever's board it was.
pub const CAVE_DMG: i32 = 6;
/// Gems a mined crystal pays, before charms.
pub const CLAIM_GEMS: usize = 1;
/// Tiles on a fresh board dealt one crack from caving in, give or take one.
pub const BASE_WEAK: i32 = 8;
/// Picks a turn: each mines a crystal or digs a vent.
pub const REVEALS: i32 = 4;
/// The fewest picks a player turn can have, however many penalties stack.
pub const MIN_REVEALS: i32 = 3;
pub const PRESSURE_TURN: usize = 12;
/// Safe reveals in a row that count toward surge; each adds `SURGE_PCT` to skill damage.
pub const MAX_SURGE: i32 = 10;
pub const SURGE_PCT: i32 = 10;
/// What the hero must deal in one turn to break the enemy's loaded rites, in percent of the damage they would do.
pub const BREAK_PCT: i32 = 150;
/// The least a load takes to break, so one that deals no damage still asks for a hit.
pub const BREAK_MIN: i32 = 6;
/// Reactions: what a rite bringing an element its target's mark lacks sets off.
pub const IGNITE_BURN: i32 = 3;
pub const DOUSE_SHIELD: i32 = 4;
pub const GUST_REVEALS: i32 = 1;
pub const CRUSH_DMG: i32 = 4;
pub const ARC_SURGE: i32 = 2;

/// Every charm hook the battle reports through `Event::Proc`, by `Mods` field name.
pub const HOOKS: &[&str] = &[
    "reveals", "power", "mine_guard", "start_shield", "combo_pct", "duds", "start_stacks", "burn_bonus", "scouts",
    "thorns", "foe_mines", "fewer_mines", "lifesteal_pct", "opening", "shield_turn", "surge_start", "surge_pct",
    "keep_surge", "rite_bonus", "rite_burn", "first_pct", "claim_zap", "claim_heal", "claim_shield", "claim_bonus",
    "salvage", "mine_spite", "armor", "evade", "reveal_shield", "hoard_shield", "chain", "mine_surge", "sense", "echo",
    "desperate", "heal_after", "foe_mine_gift", "enrage", "slow_enrage", "foe_surge_cap", "easy_break",
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
    /// crystals mined this turn, toward rite damage
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
    /// picks spent this turn
    pub used: i32,
    /// cave-ins it will still shrug off this wave
    pub duds: i32,
    /// cave-ins it will still shrug off this turn
    pub sense_left: i32,
    /// its shield skips wearing off at its next turn (Citadel)
    pub hold: bool,
    /// its shield shatters onto the next attacker whose rite hits it (Lodestone)
    pub brittle: bool,
    /// picks it loses at its next turn (Desert Wrath)
    pub blind: i32,
    /// its burn no longer halves (Hellbroth)
    pub simmer: bool,
    /// AI only: it has decided to empty its stacks this turn
    firing: bool,
    /// the enemy's rites loaded at the end of its turn, fired as its next turn starts
    pub loaded: Vec<&'static Combo>,
    /// rite damage the hero must deal in one turn to break the load (0: nothing to break)
    pub break_at: i32,
    /// rite damage taken this turn, toward breaking its load
    pub took: i32,
    /// its load was broken: it still fires, at half damage
    pub broken: bool,
    /// the elements of the last rite that hit it: a rite bringing any other sets off reactions
    pub mark: Vec<Element>,
    pub mods: Mods,
    /// skills upgraded by boss kills, by index in `COMBOS`; a repeat is another level
    pub ups: Vec<usize>,
    /// brands burnt onto rites: (index in `COMBOS`, index in `BRANDS`), up to the rite's level each, oldest first
    pub brands: Vec<(usize, usize)>,
    /// the rites it can cast, by index in `COMBOS`; empty means the enemy rites its elements make
    pub rites: Vec<usize>,
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

    /// The rites it can cast: a hero's basics and the rites it found, else the ones its elements make.
    pub fn rite_list(&self) -> Vec<&'static Combo> {
        if self.rites.is_empty() { combos_for(&self.elements) } else { self.rites.iter().map(|&i| &COMBOS[i]).collect() }
    }

    pub fn level(&self, c: &Combo) -> usize {
        let id = combo_id(c);
        1 + self.ups.iter().filter(|&&u| u == id).count()
    }

    pub fn brands(&self, c: &Combo) -> Vec<&'static Brand> {
        let id = combo_id(c);
        self.brands.iter().filter(|b| b.0 == id).map(|b| &BRANDS[b.1]).collect()
    }

    /// What `c` does for this fighter, each effect with the level it works at: the rite's own grow
    /// with it, a brand's stay at 1.
    pub fn effects<'a>(&'a self, c: &'a Combo) -> impl Iterator<Item = (Effect, usize)> + 'a {
        let lvl = self.level(c);
        let master = c.master.filter(|_| lvl >= MAX_LEVEL);
        c.effects.iter().copied().chain(master).map(move |e| (e, lvl)).chain(self.brands(c).into_iter().map(|b| (b.effect, 1)))
    }

    pub fn describe(&self, c: &Combo) -> String {
        rite_text(c, self.level(c), &self.brands(c))
    }

    /// True if `c` carries effect `e` for this fighter (a mastery, say).
    pub fn has(&self, c: &Combo, e: Effect) -> bool {
        self.effects(c).any(|(x, _)| x == e)
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
        for _ in 0..self.toll(c) {
            let e = *rest.iter().max_by_key(|&&e| rest.iter().filter(|&&x| x == e).count())?;
            spend(&mut rest, &[e]);
            paid.push(e);
        }
        Some(paid)
    }

    /// True if `paid` covers `c`'s cost and toll exactly, whichever gems pay the toll.
    pub fn fits(&self, c: &Combo, paid: &[Element]) -> bool {
        paid.len() == self.cost(c).len() + self.toll(c) && pays(paid, &self.cost(c))
    }

    /// Extra gems of any kind `c` takes ("Rites cost 1 more gem"): the basics stay one gem.
    pub fn toll(&self, c: &Combo) -> usize {
        if is_basic(c) { 0 } else { self.mods.toll.max(0) as usize }
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
#[derive(Debug)]
pub enum Event {
    Flip,
    /// `side` claimed the crystals on these tiles of its `element` board. Carried here because the
    /// board may already be dealt anew when the last crystal on it goes
    Claim { side: usize, element: Element, tiles: Vec<usize> },
    /// tiles on `side`'s board just caved in
    Cave { side: usize },
    /// `skill`: (index in `COMBOS`, level)
    /// `master`: a mastered rite whose mastery takes effect (Worldbreaker's only on a foe at half HP)
    Cast { from: usize, name: &'static str, element: Element, miss: bool, skill: (usize, usize), master: bool },
    /// a passive of `side` just took effect; `hook` names the `Mods` field
    Proc { side: usize, hook: &'static str },
    /// the enemy loaded rites, to fire as its next turn starts
    Load,
    /// the hero hit hard enough to shatter the enemy's loaded rites
    Break,
    /// `from`'s rite brought `element` to a target marked with others
    React { from: usize, element: Element },
    /// `side`'s brittle shield shattered onto the one who hit it
    Brittle { side: usize },
    /// a rite shattered `side`'s shield before its hit (Pierce); a brittle one is gone without shattering back
    Pierced { side: usize },
    /// the turn passes: both sides' HP (and the wave) as it does, before the next turn's first blow, so
    /// a heal and the hit that follows it in the same moment show as two numbers, not their sum
    Pass { hp: [i32; 2], wave: usize },
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
    /// the hero's rite damage this battle, and its biggest single rite: (damage, rite name)
    pub dealt: i32,
    pub best: (i32, &'static str),
    /// the enemy a wave change just replaced, for the screen to show until the killing blow lands
    pub fallen: Option<Fighter>,
    rng: Rng,
}

impl Battle {
    pub fn new(player: Fighter, mut enemies: Vec<Fighter>, rng: Rng) -> Battle {
        let waves = enemies.len();
        enemies.reverse();
        let first = enemies.pop().expect("a battle needs an enemy");
        let mut b = Battle { f: [player, first], queue: enemies, wave: 1, waves, turn: 1, active: 0, log: vec![], events: vec![], dealt: 0, best: (0, ""), fallen: None, rng };
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

    /// Developer shortcut: the enemy loads (if it has nothing loaded) and the load breaks at once.
    pub fn debug_break(&mut self) {
        if self.f[1].loaded.is_empty() {
            self.f[1].stacks = self.f[1].elements.iter().flat_map(|&e| [e, e]).collect();
            self.load();
        }
        if !self.f[1].loaded.is_empty() && !self.f[1].broken {
            self.f[1].broken = true;
            self.events.push(Event::Break);
        }
    }

    /// Combos `s` can pay for right now, at their level.
    pub fn ready(&self, s: usize) -> Vec<&'static Combo> {
        self.f[s].rite_list().into_iter().filter(|c| self.f[s].payment(c, &self.f[s].stacks).is_some() && !self.wasted(s, c)).collect()
    }

    /// A rite that only heals does nothing at full HP, so it is not cast then: its gems would be lost.
    pub fn wasted(&self, s: usize, c: &Combo) -> bool {
        let f = &self.f[s];
        f.hp >= f.max_hp && f.effects(c).all(|(e, _)| matches!(e, Effect::Heal(_)))
    }


    /// Overpressure: long fights escalate, so no shield or regen build can stall forever.
    pub fn pressure(&self) -> i32 {
        2 * self.turn.saturating_sub(PRESSURE_TURN) as i32
    }

    /// How high `s` can push its surge: some opponents jam it.
    pub fn surge_cap(&self, s: usize) -> i32 {
        let top = MAX_SURGE + self.f[s].mods.surge_room.max(0);
        match self.f[1 - s].mods.foe_surge_cap {
            0 => top,
            jam => jam.min(top),
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
        let d = match (self.f[1 - s].evades_left > 0 && self.hits(s, c) && !self.f[s].has(c, Effect::Sure), self.f[1 - s].mods.graze > 0) {
            (true, true) => full / 2, // it will graze
            (true, false) => 0,       // it will miss
            _ => full,
        };
        if d != full {
            steps.push((if d == 0 { "it evades" } else { "it grazes: half" }.into(), format!("= {d}")));
        }
        // enemies only cast by firing their load, so a broken one halves every rite of it
        if s == 1 && self.f[1].broken {
            steps.push(("its load was broken: half".into(), format!("= {}", d / 2)));
            return (d / 2, steps);
        }
        (d, steps)
    }

    /// True if `s` casting `c` deals damage. The player's rites only hit if they say so; every enemy
    /// rite still strikes with its gems, since for most enemies that is their whole attack.
    pub fn hits(&self, s: usize, c: &Combo) -> bool {
        s == 1 || self.f[s].effects(c).any(|(e, _)| e.hits())
    }

    /// The elements `c` would react with on `s`'s target: those it brings that the target's mark lacks.
    /// Nothing on an unmarked target, and nothing from a rite that does not hit.
    pub fn reactions(&self, s: usize, c: &Combo) -> Vec<Element> {
        let mark = &self.f[1 - s].mark;
        if mark.is_empty() || !self.hits(s, c) {
            return vec![];
        }
        c.needs.iter().copied().filter(|e| !mark.contains(e)).collect()
    }

    /// What the enemy's loaded rites would deal the hero if they fired now (before the hero's shield).
    pub fn loaded_damage(&self) -> i32 {
        let (mut total, mut surge, mut evades) = (0, self.f[1].surge, self.f[0].mods.evade);
        // a broken load hits for half, rite by rite, as `hit_steps` halves it
        let cut = if self.f[1].broken { 2 } else { 1 };
        for (nth, c) in self.f[1].loaded.iter().enumerate() {
            let full = self.dmg(1, c, nth as i32, surge, self.f[1].shield);
            if evades > 0 {
                evades -= 1;
                if self.f[0].mods.graze > 0 {
                    total += full / 2 / cut;
                }
            } else {
                total += full / cut;
            }
            surge = (surge - 1).max(0);
        }
        total
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
        // a rite the hero found (not a basic; enemies carry no found rites) outdoes Attack gem for gem
        if !f.rites.is_empty() && !is_basic(c) {
            plus(steps, &mut d, "found rite".into(), gems * FOUND_PER_GEM);
        }
        let lvl = f.level(c);
        let mut blast = 0;
        for (e, l) in f.effects(c) {
            let (what, add) = match e {
                Effect::Damage(v) => (c.title(lvl).to_string(), at_level(v, l)),
                Effect::PerSurge(v) => (format!("{surge} surge"), at_level(v, l) * surge),
                Effect::Detonate => (format!("detonate {} burn", foe.burn), foe.burn.max(0) * (3 + f.mods.burn_bonus)),
                Effect::PerReveal(v) => {
                    let n = f.used + if f.has(c, Effect::Unused) { f.reveals_left.max(0) } else { 0 };
                    (format!("{n} picks"), at_level(v, l) * n)
                }
                Effect::PerShatter(v) => {
                    let breaks = f.effects(c).find_map(|(e, l)| if let Effect::Shatter(n) = e { Some(at_level(n as i32, l) as usize) } else { None }).unwrap_or(0);
                    let board = foe.board();
                    let n = (0..N).filter(|&i| board.live(i)).count().min(breaks) as i32;
                    (format!("{n} crystals broken"), v * n)
                }
                Effect::PerBurn(v) => (format!("{} burn on foe", foe.burn), at_level(v, l) * foe.burn.max(0)),
                Effect::PerQuarterLost(v) => {
                    let q = ((f.max_hp - f.hp) * 4 / f.max_hp.max(1)).clamp(0, 3);
                    (format!("{q} quarters of HP lost"), at_level(v, l) * q)
                }
                Effect::Opener(v) if nth == 0 => ("first rite this turn".into(), at_level(v, l)),
                Effect::PerTurn(v) => (format!("turn {}", self.turn), at_level(v, l) * self.turn as i32),
                Effect::ShieldBlast(p) => {
                    blast = shield.max(0) * at_level(p, l) / 100;
                    continue;
                }
                _ => continue,
            };
            plus(steps, &mut d, what, add * (100 + f.mods.combo_pct) / 100);
        }
        if self.reactions(s, c).contains(&Element::Stone) {
            plus(steps, &mut d, "Stone reacts: crush".into(), CRUSH_DMG);
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
        if f.mods.rite_pct != 0 {
            d = (d * (100 + f.mods.rite_pct) + 50) / 100;
            steps.push((format!("rites +{}%", f.mods.rite_pct), format!("= {d}")));
        }
        // thrown shield is not boosted by surge
        plus(steps, &mut d, "your shield, thrown".into(), blast);
        if f.has(c, Effect::Execute) && foe.hp * 2 <= foe.max_hp {
            d *= 2;
            steps.push(("foe at half HP: x2".into(), format!("= {d}")));
        }
        plus(steps, &mut d, "overpressure".into(), self.pressure());
        if f.weak > 0 {
            d = (d + 1) / 2;
            steps.push(("weakened: half".into(), format!("= {d}")));
        }
        let armor = if f.has(c, Effect::Sure) { 0 } else { self.f[1 - s].mods.armor };
        if armor != 0 {
            steps.push(("foe armor".into(), format!("{:+}", -armor)));
        }
        (d - armor).max(1)
    }

    /// The biggest combo of `s` the given stacks can pay for.
    pub fn best_combo(&self, s: usize, stacks: &[Element]) -> Option<&'static Combo> {
        self.f[s].rite_list().into_iter().filter(|c| self.f[s].payment(c, stacks).is_some() && !self.wasted(s, c)).max_by_key(|c| self.f[s].cost(c).len())
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
        let mut weak = (BASE_WEAK + roll + self.f[1 - s].mods.foe_mines - self.f[s].mods.fewer_mines).clamp(0, 12);
        // debug builds only: MINEBOUND_NO_MINES=1 gives the hero boards with no tile about to cave in
        if cfg!(debug_assertions) && s == 0 && std::env::var_os("MINEBOUND_NO_MINES").is_some() {
            weak = 0;
        }
        let mut board = Board::deal(&mut self.rng, self.f[s].elements[k], weak as usize);
        board.ease(self.f[s].mods.scouts.max(0) as usize);
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
            // debug builds only: MINEBOUND_GEMS=N starts the hero's every wave with N gems of each element
            if let Some(n) = std::env::var("MINEBOUND_GEMS").ok().filter(|_| cfg!(debug_assertions) && s == 0).and_then(|v| v.parse().ok()) {
                let els = self.f[0].elements.clone();
                self.f[0].stacks.extend(els.iter().flat_map(|&e| std::iter::repeat(e).take(n)));
            }
            // a brittle shield stays brittle into the next wave, as long as the shield lasts
            (self.f[s].hold, self.f[s].blind, self.f[s].simmer) = (false, 0, false);
            self.f[s].burn += self.f[s].mods.self_burn.max(0);
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
        // worn away to nothing, a brittle shield is gone, so a fresh one is not brittle
        f.brittle &= f.shield > 0;
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
        self.fallen = Some(std::mem::replace(&mut self.f[1], next));
        self.wave += 1;
        debug_assert!(self.wave <= self.waves, "wave {} of {}", self.wave, self.waves);
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
            self.log.push(format!("{} snatches a gem.", self.f[a].name));
        }
        self.f[1 - a].evades_left = self.f[1 - a].mods.evade;
        let m = self.f[a].mods.clone();
        let kept = m.keep_surge > 0 && self.f[a].surge > 0;
        self.procs(a, &[("reveals", m.reveals > 0), ("surge_start", m.surge_start > 0), ("keep_surge", kept), ("shield_turn", m.shield_turn > 0)]);
        self.procs(a, &[("enrage", turn > 1 && m.enrage > 0), ("slow_enrage", turn > 1 && turn % 2 == 1 && m.slow_enrage > 0)]);
        // the side whose rule holds the other's surge down flashes as that side's turn starts
        let damped = self.f[1 - a].mods.foe_surge_cap > 0;
        self.procs(1 - a, &[("foe_surge_cap", damped)]);
        let bonus = self.f[1 - a].mods.burn_bonus;
        self.procs(1 - a, &[("burn_bonus", self.f[a].burn > 0 && bonus > 0)]);
        let f = &mut self.f[a];
        let charged = f.surge;
        f.used = 0;
        // penalties stack, but the player always keeps enough reveals to play a board
        f.reveals_left = (REVEALS + f.mods.reveals - f.blind).max(if a == 0 { MIN_REVEALS } else { 1 });
        f.blind = 0;
        f.sense_left = f.mods.sense;
        if turn > 1 && !f.hold && f.mods.keep_shield == 0 {
            // shield wears off: half of it is gone by the start of its owner's next turn
            f.shield /= 2;
        }
        // a shield that is all gone takes its brittleness with it
        f.brittle &= f.shield > 0;
        f.hold = false;
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
            if !f.simmer {
                f.burn /= 2;
            }
            let msg = format!("{} burns: -{d}", f.name);
            self.log.push(msg);
            if self.settle() {
                return;
            }
        }
        if a == 0 {
            self.f[1].took = 0;
        } else {
            self.fire_loaded(charged);
        }
    }

    /// The enemy's loaded rites go off as its turn starts, at the surge it loaded them with.
    fn fire_loaded(&mut self, charged: i32) {
        let rites = std::mem::take(&mut self.f[1].loaded);
        self.f[1].break_at = 0;
        let fresh = self.f[1].surge;
        self.f[1].surge = charged;
        for c in rites {
            if self.over() || self.active != 1 {
                break;
            }
            self.land(1, c);
        }
        self.f[1].surge = fresh;
        self.f[1].cast_n = 0;
        self.f[1].broken = false;
    }

    /// The enemy loads every rite its stacks pay for, biggest first, to fire as its next turn starts.
    fn load(&mut self) {
        self.f[1].broken = false;
        while let Some(c) = self.best_combo(1, &self.f[1].stacks) {
            let paid = self.f[1].payment(c, &self.f[1].stacks).expect("best_combo checked it");
            spend(&mut self.f[1].stacks, &paid);
            self.f[1].loaded.push(c);
        }
        if !self.f[1].loaded.is_empty() {
            self.f[1].break_at = (self.loaded_damage() * (BREAK_PCT - self.f[0].mods.easy_break) / 100).max(BREAK_MIN);
            let names: Vec<&str> = self.f[1].loaded.iter().map(|c| c.title(self.f[1].level(c))).collect();
            self.log.push(format!("{} loads {}", self.f[1].name, names.join(", ")));
            self.events.push(Event::Load);
        }
    }

    pub fn end_turn(&mut self) {
        if self.over() {
            return;
        }
        let a = self.active;
        self.refresh(a);
        if self.f[a].weak > 0 {
            self.f[a].weak -= 1;
        }
        let f = &mut self.f[a];
        let banked = f.reveals_left.max(0) * f.mods.reveal_shield;
        let hoard = if f.stacks.len() >= 3 { f.mods.hoard_shield } else { 0 };
        f.shield += banked + hoard;
        self.procs(a, &[("reveal_shield", banked > 0), ("hoard_shield", hoard > 0)]);
        self.events.push(Event::Pass { hp: [self.f[0].hp, self.f[1].hp], wave: self.wave });
        self.active = 1 - a;
        if self.active == 0 {
            self.turn += 1;
        }
        self.begin_turn();
    }

    /// A side with no picks left and no rite its gems pay for has nothing left to do: its turn passes
    /// on its own (nothing cracks at turn end, so this never springs a cave-in on the hero).
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

    /// The active side mines crystal `i` on its current board, for a pick: a stack of the board's
    /// element, a step of surge, and a crack in every tile around it. False if `i` is no live crystal.
    pub fn dig(&mut self, i: usize) -> bool {
        let a = self.active;
        let cur = self.f[a].cur;
        if self.over() || self.f[a].reveals_left <= 0 || i >= N || !self.f[a].boards[cur].live(i) {
            return false;
        }
        let fell = self.f[a].boards[cur].mine(i).unwrap_or_default();
        let jam = self.surge_cap(a);
        let f = &mut self.f[a];
        f.reveals_left -= 1;
        f.used += 1;
        f.surge = (f.surge + 1).min(jam);
        let element = f.boards[cur].element;
        let gain = CLAIM_GEMS + f.mods.claim_bonus.max(0) as usize;
        f.stacks.extend(std::iter::repeat(element).take(gain));
        f.hp = (f.hp + f.mods.claim_heal).min(f.max_hp);
        f.shield += f.mods.claim_shield;
        let m = f.mods.clone();
        self.events.push(Event::Claim { side: a, element, tiles: vec![i] });
        self.procs(a, &[("claim_bonus", m.claim_bonus > 0), ("claim_heal", m.claim_heal > 0), ("claim_shield", m.claim_shield > 0), ("claim_zap", m.claim_zap > 0)]);
        if m.claim_zap > 0 {
            self.hurt(1 - a, m.claim_zap);
            if self.settle() {
                return true;
            }
        }
        if self.cave(a, fell.len()) {
            return true;
        }
        self.refresh(a);
        self.tidy(a);
        true
    }

    /// The active side digs a vent at tile `i` on its current board, for a pick: it and every tile
    /// around it lose a crack. False if no vent can go there or it would ease nothing.
    pub fn vent(&mut self, i: usize) -> bool {
        let a = self.active;
        let cur = self.f[a].cur;
        if self.over() || self.f[a].reveals_left <= 0 || i >= N || !self.f[a].boards[cur].vent(i) {
            return false;
        }
        self.f[a].reveals_left -= 1;
        self.f[a].used += 1;
        self.events.push(Event::Flip);
        self.tidy(a);
        true
    }

    /// `n` tiles just caved in on side `s`'s board: each one hurts it, unless a charm softens the
    /// fall, and feeds whatever charms answer to it. True if the battle moved on (someone fell).
    fn cave(&mut self, s: usize, n: usize) -> bool {
        if n > 0 {
            self.events.push(Event::Cave { side: s });
        }
        for _ in 0..n {
            if self.over() {
                return true;
            }
            let jam = self.surge_cap(s);
            let f = &mut self.f[s];
            let element = f.board().element;
            // Sixth Sense, then Lucky Coin: it comes down harmlessly
            let (sensed, dud) = (f.sense_left > 0, f.sense_left <= 0 && f.duds > 0);
            if sensed {
                f.sense_left -= 1;
            } else if dud {
                f.duds -= 1;
            }
            let d = if sensed || dud { 0 } else { (CAVE_DMG - f.mods.mine_guard).max(0) };
            f.stacks.extend(std::iter::repeat(element).take(f.mods.salvage.max(0) as usize));
            f.surge = (f.surge + f.mods.mine_surge.max(0)).min(jam);
            let (m, name) = (f.mods.clone(), f.name.clone());
            self.procs(s, &[("sense", sensed), ("duds", dud), ("salvage", m.salvage > 0), ("mine_surge", m.mine_surge > 0)]);
            self.hurt(s, d);
            self.log.push(format!("A tunnel caves in on {name}: -{d}"));
            self.procs(s, &[("mine_guard", d > 0 && m.mine_guard > 0)]);
            // some opponents feed on your mistakes
            let t = 1 - s;
            for _ in 0..self.f[t].mods.foe_mine_gift.max(0) {
                let k = self.rng.below(self.f[t].elements.len());
                let e = self.f[t].elements[k];
                self.f[t].stacks.push(e);
            }
            let gift = self.f[t].mods.foe_mine_gift > 0;
            self.procs(t, &[("foe_mine_gift", gift)]);
            if m.mine_spite > 0 {
                self.hurt(t, m.mine_spite);
                self.proc(s, "mine_spite");
            }
            if self.settle() {
                return true;
            }
        }
        self.over()
    }

    /// Deals anew every board of side `s` with no crystal left to mine.
    fn refresh(&mut self, s: usize) {
        for k in 0..self.f[s].boards.len() {
            if self.f[s].boards[k].spent() {
                self.deal_board(s, k);
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
        if self.over() || self.wasted(a, c) || !self.f[a].fits(c, paid) || !pays(&self.f[a].stacks, paid) {
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
        let (dmg, hits, react) = (self.hit_damage(a, c), self.hits(a, c), self.reactions(a, c));
        let (lvl, element) = (self.f[a].level(c), c.needs[0]);
        let (name, skill) = (c.title(lvl), (combo_id(c), lvl));
        let (nth, surged) = (self.f[a].cast_n, self.f[a].surge > 0);
        // every rite that hits spends a surge step, so firing early costs the later ones damage;
        // a setup rite that deals none (Bulwark, Mistveil) spends nothing
        if hits {
            self.f[a].cast_n += 1;
            if !self.f[a].has(c, Effect::KeepSurge) {
                self.f[a].surge = (self.f[a].surge - 1).max(0);
            }
        }
        // a rite that does not hit is not aimed at the foe's body, so it cannot be evaded
        if !hits || self.f[a].has(c, Effect::Sure) {
        } else if self.f[t].evades_left > 0 && self.f[t].mods.graze > 0 {
            // a graze: half damage (already in `dmg`), every effect still lands
            self.f[t].evades_left -= 1;
            self.proc(t, "evade");
            self.log.push(format!("{}: {name} only grazes", self.f[a].name));
        } else if self.f[t].evades_left > 0 {
            // the whole skill whiffs: no damage, no effects, stacks still spent
            self.f[t].evades_left -= 1;
            self.proc(t, "evade");
            self.events.push(Event::Cast { from: a, name, element, miss: true, skill, master: false });
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
        let master = lvl >= MAX_LEVEL && !(self.f[a].has(c, Effect::Execute) && self.f[t].hp * 2 > self.f[t].max_hp);
        let (shield, mut leech) = (self.f[a].shield, self.f[a].mods.lifesteal_pct);
        let (mut fell, mut pierced) = (0, false);
        for (e, lvl) in effects {
            let up = |v: i32| at_level(v, lvl);
            match e {
                Effect::Damage(_) => {} // already in hit_damage
                Effect::Burn(n) => self.f[t].burn += up(n),
                Effect::Shield(v) => self.f[a].shield += up(v) * pct / 100,
                Effect::Heal(v) => self.f[a].hp = (self.f[a].hp + up(v) * pct / 100).min(self.f[a].max_hp),
                Effect::Weaken(n) => self.f[t].weak = self.f[t].weak.max(up(n)),
                Effect::Drain(n) => {
                    let keep = self.f[t].stacks.len().saturating_sub(up(n as i32) as usize);
                    self.f[t].stacks.truncate(keep);
                }
                Effect::Pierce => {
                    pierced |= self.f[t].shield > 0;
                    (self.f[t].shield, self.f[t].brittle) = (0, false);
                }
                // in hit_damage, worked out before any effect lands
                Effect::PerSurge(_) | Effect::Detonate | Effect::PerReveal(_) | Effect::PerBurn(_) | Effect::PerQuarterLost(_) | Effect::Opener(_) | Effect::PerTurn(_) => {}
                Effect::Spikes(n) => self.f[a].mods.thorns += up(n),
                Effect::Surge(v) => self.f[a].surge = (self.f[a].surge + up(v)).min(self.surge_cap(a)),
                // capped until mastered, or casting it again and again would grow shield without end
                Effect::DoubleShield(cap) => self.f[a].shield += self.f[a].shield.clamp(0, double_cap(cap, lvl).unwrap_or(i32::MAX)),
                Effect::ShieldBlast(_) => self.f[a].shield = if self.f[a].has(c, Effect::KeepHalf) { shield / 2 } else { 0 },
                Effect::Brittle => self.f[a].brittle = true,
                Effect::SelfDamage(v) => self.f[a].hp = (self.f[a].hp - up(v)).max(1),
                Effect::Reveal(n) => self.f[a].reveals_left += up(n),
                Effect::Shatter(n) => self.shatter(t, up(n as i32) as usize),
                // the cave-ins land after the hit, so a wave cannot change under the rite
                Effect::Bury(n) => {
                    let cur = self.f[t].cur;
                    fell += self.f[t].boards[cur].quake(up(n as i32) as usize, &mut self.rng).len();
                }
                // masteries; the ones that change the hit itself are in hit_damage
                Effect::Sure | Effect::KeepSurge | Effect::PerShatter(_) | Effect::Execute | Effect::Unused | Effect::KeepHalf => {}
                Effect::Duds(n) => self.f[a].duds += n,
                Effect::Lifesteal(p) => leech += p,
                Effect::Scout(n) => self.f[a].boards.iter_mut().for_each(|b| b.ease(n)),
                Effect::Hold => self.f[a].hold = true,
                Effect::Steal(n) => {
                    let keep = self.f[t].stacks.len().saturating_sub(n);
                    // they arrive as the caster's own elements, or a hero would hold gems it can't spend
                    for _ in self.f[t].stacks.split_off(keep) {
                        let k = self.rng.below(self.f[a].elements.len());
                        let e = self.f[a].elements[k];
                        self.f[a].stacks.push(e);
                    }
                }
                Effect::Blind(n) => self.f[t].blind += n,
                Effect::Stoke(n) => self.f[a].power += n,
                Effect::Cleanse => (self.f[a].burn, self.f[a].weak) = (0, 0),
                Effect::DrainAll => self.f[t].stacks.clear(),
                Effect::Simmer => self.f[t].simmer = true,
                Effect::Armor(n) => self.f[a].mods.armor += up(n),
                Effect::Regen(n) => self.f[a].mods.regen += up(n),
                Effect::Defuse(n) => {
                    let keep = self.f[t].loaded.len().saturating_sub(up(n as i32) as usize);
                    self.f[t].loaded.truncate(keep);
                }
                Effect::Gems(n) => {
                    for _ in 0..n {
                        let k = self.rng.below(self.f[a].elements.len());
                        let e = self.f[a].elements[k];
                        self.f[a].stacks.push(e);
                    }
                }
            }
        }
        if hits {
            for &e in &react {
                match e {
                    Element::Ember => self.f[t].burn += IGNITE_BURN,
                    Element::Tide => self.f[a].shield += DOUSE_SHIELD,
                    Element::Gale => self.f[a].reveals_left += GUST_REVEALS,
                    Element::Stone => {} // in hit_damage
                    Element::Spark => self.f[a].surge = (self.f[a].surge + ARC_SURGE).min(self.surge_cap(a)),
                }
            }
            self.f[t].mark = c.needs.to_vec();
        }
        if a == 0 && hits {
            self.dealt += dmg;
            if dmg > self.best.0 {
                self.best = (dmg, name);
            }
        }
        // as the hit meets it: the hit may wear the shield to nothing, and it still shatters back
        let (guard, brittle) = (self.f[t].shield, self.f[t].brittle);
        let dealt = self.hurt(t, dmg);
        let steal = dealt * leech / 100;
        self.f[a].hp = (self.f[a].hp + steal).min(self.f[a].max_hp);
        self.procs(a, &[("lifesteal_pct", steal > 0)]);
        let thorns = self.f[t].mods.thorns;
        if thorns > 0 && hits {
            self.hurt(a, thorns);
            self.proc(t, "thorns");
        }
        self.events.push(Event::Cast { from: a, name, element, miss: false, skill, master });
        if pierced {
            self.events.push(Event::Pierced { side: t });
        }
        // a brittle shield still blocks the hit, then all of it shatters onto the attacker
        // (after the cast, so the screen shows it as that rite lands)
        if hits && brittle && guard > 0 {
            (self.f[t].shield, self.f[t].brittle) = (0, false);
            self.hurt(a, guard);
            self.events.push(Event::Brittle { side: t });
            self.log.push(format!("{}'s brittle shield shatters onto {} for {guard}", self.f[t].name, self.f[a].name));
        }
        self.events.extend(react.iter().map(|&element| Event::React { from: a, element }));
        if a == 0 && hits && !self.f[t].loaded.is_empty() && !self.f[t].broken {
            self.f[t].took += dmg;
            if self.f[t].took >= self.f[t].break_at {
                // a break blunts the load rather than stopping it: enemies always land something
                self.f[t].broken = true;
                self.events.push(Event::Break);
                self.procs(a, &[("easy_break", self.f[a].mods.easy_break > 0)]);
                self.log.push(format!("The loaded rites of {} crack: they will hit for half", self.f[t].name));
            }
        }
        let line = if hits { format!("{}: {name} for {dmg}", self.f[a].name) } else { format!("{}: {name}", self.f[a].name) };
        self.log.push(line);
        if self.settle() || self.cave(t, fell) {
            return;
        }
        self.refresh(t);
        self.tidy(a);
    }

    /// Breaks up to `n` of `s`'s claimable crystals on the board it has up; a board left with none is dealt anew.
    fn shatter(&mut self, s: usize, n: usize) {
        let cur = self.f[s].cur;
        let board = &mut self.f[s].boards[cur];
        if board.shatter(n) > 0 && board.spent() {
            self.deal_board(s, cur);
        }
    }

    /// The AI's best pick on `f`'s board `k`: (tile, true to mine it or false to vent it, worth).
    /// A crystal is worth its gem less what mining it would bring down; a vent is worth the tiles it
    /// eases, most of all those one crack from caving in. A `bold` fighter (the hero bot) weighs a
    /// cave-in against the HP it has left, as a player would: 6 damage at full HP is cheap for a gem.
    fn ai_pick(f: &Fighter, k: usize, bold: bool) -> Option<(usize, bool, f32)> {
        let b = &f.boards[k];
        let hurt = (CAVE_DMG - f.mods.mine_guard).max(0) as f32;
        let cave = if bold { 0.2 + 3.0 * hurt / f.hp.max(1) as f32 } else { 0.5 + hurt / 8.0 };
        let mine = (0..N).filter(|&i| b.live(i)).map(|i| {
            let fell = b.would_cave(i);
            let lost = fell.iter().filter(|&&j| b.live(j)).count() as f32;
            (i, true, 1.0 - fell.len() as f32 * cave - lost * 0.7)
        });
        let vent = (0..N).map(|i| {
            let eased = b.vent_eases(i);
            // a vent only pays off as safer mining later, so it is worth as much as cave-ins are feared
            let worth = eased.iter().map(|&j| if b.tiles[j].crack + 1 == CAVE { 0.3 } else { 0.06 }).sum::<f32>() * cave / 1.25;
            (i, false, if eased.is_empty() { f32::MIN } else { worth.min(0.9) })
        });
        mine.chain(vent).max_by(|x, y| x.2.total_cmp(&y.2))
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
            // the best pick on any board, leaning toward the element it holds least of
            let held = |k: usize| f.stacks.iter().filter(|&&e| e == f.boards[k].element).count() as f32;
            let pick = (0..f.boards.len())
                .filter_map(|k| Self::ai_pick(f, k, a == 0).map(|(i, mine, worth)| (k, i, mine, worth - 0.05 * held(k))))
                .max_by(|x, y| x.3.total_cmp(&y.3))
                .filter(|p| p.3 > -0.4);
            if let Some((k, i, mine, _)) = pick {
                self.swap(k);
                if if mine { self.dig(i) } else { self.vent(i) } {
                    return;
                }
            }
        }
        let f = &self.f[a];
        if armed && a == 1 {
            // the enemy never fires mid-turn and never sits on gems: once its picks are done it loads
            // whatever they pay for, and the hero sees it coming (and can break it)
            self.load();
            self.end_turn();
        } else if armed && (lethal || n >= f.aggro || can_reveal) {
            // firing when it declined a risky reveal keeps a cornered fighter from stalling
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
    use crate::board::Tile;
    use Element::*;

    /// Cast every rite the stacks pay for at the current enemy, biggest first.
    fn unleash(b: &mut Battle) {
        let (a, wave) = (b.active, b.wave);
        while b.active == a && b.wave == wave && b.cast_best() {}
    }

    fn duel() -> Battle {
        let me = Fighter::new("Me", 0, "", 50, 50, 3, &[Ember, Gale, Stone], Mods::default(), 3);
        let foe = Fighter::new("Foe", 0, "", 100, 100, 1, &[Tide, Spark], Mods::default(), 3);
        let mut b = Battle::new(me, vec![foe], Rng::new(1));
        // uncracked ground, so a turn ending never brings a tile down by chance
        b.f.iter_mut().flat_map(|f| f.boards.iter_mut()).flat_map(|x| x.tiles.iter_mut()).for_each(|t| t.crack = 0);
        b
    }

    fn combo(name: &str) -> &'static Combo {
        COMBOS.iter().find(|c| c.name == name).unwrap()
    }

    #[test]
    fn stacks_have_no_cap_and_surge_damage_rounds_to_nearest() {
        let mut b = duel();
        b.f[0].stacks = vec![Ember; 20];
        lay_out(&mut b, &[]);
        assert!(b.dig(7), "holding many stacks never blocks mining");
        b.f[0].surge = 2;
        // Raze: (2 stacks x 3 power + 2) x1.2 = 9.6
        assert_eq!(b.hit_damage(0, combo("Raze")), 10);
    }

    /// Clears the player's current board, then puts a crystal on tile 7 (and on 35, so mining the
    /// first never leaves the board spent) and a tile one crack from caving in on each of `weak`.
    fn lay_out(b: &mut Battle, weak: &[usize]) {
        let cur = b.f[0].cur;
        let tiles = &mut b.f[0].boards[cur].tiles;
        tiles.iter_mut().for_each(|t| *t = Tile::default());
        (tiles[7].crystal, tiles[35].crystal) = (true, true);
        for &i in weak {
            tiles[i].crack = CAVE - 1;
        }
        b.f[0].reveals_left = 9;
    }

    #[test]
    fn mining_pays_a_gem_and_cracks_around_and_a_cave_in_only_hurts() {
        let mut b = duel();
        assert_eq!(b.f[0].boards.len(), 3, "one board per element");
        let element = b.f[0].board().element;
        lay_out(&mut b, &[]);
        assert!(!b.dig(8) && !b.vent(8), "only a crystal can be mined, and a vent needs a crack to ease");
        assert!(b.dig(7) && !b.dig(7), "a crystal is mined once");
        assert_eq!(b.f[0].stacks, vec![element; CLAIM_GEMS]);
        assert_eq!((b.f[0].reveals_left, b.f[0].surge), (8, 1), "a pick, and a step of surge");
        assert!(Board::around(7).all(|j| b.f[0].board().tiles[j].crack == 1), "every tile around it cracks");
        assert!(b.vent(14) && b.f[0].board().tiles[8].crack == 0 && b.f[0].reveals_left == 7, "a vent eases around it, for a pick");
        assert!(b.vent(7), "a mined crystal's tile vents like any other");

        lay_out(&mut b, &[8]);
        let (held, hp) = (b.f[0].stacks.len(), b.f[0].hp);
        assert!(b.dig(7));
        assert!(b.f[0].board().tiles[8].caved);
        assert_eq!(b.f[0].hp, hp - CAVE_DMG);
        assert_eq!(b.f[0].stacks.len(), held + CLAIM_GEMS, "the gem still comes");
        assert_eq!(b.active, 0, "a cave-in only hurts: the turn goes on");
    }

    #[test]
    fn swapping_boards_changes_the_element_you_collect() {
        let mut b = duel();
        b.swap(2);
        assert_eq!(b.f[0].board().element, Stone);
        lay_out(&mut b, &[]);
        assert!(b.dig(7));
        assert_eq!(b.f[0].stacks[0], Stone);
        b.swap(0);
        assert!(b.f[0].boards[2].tiles[7].used, "a board keeps its state while you are away");
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
        assert_eq!(rite_text(steam, 3, &[]), "+8 damage, +3 picks, your next 3 cave-ins do no harm");
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
        assert_eq!(promised, 16 + 15, "Raze at x2.0, then at x1.9");
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
        b.f[0].surge = 4;
        assert!(b.attack_damage(0) > flat + 8, "surge both multiplies and adds");
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
        b.f[0].surge = 2;
        lay_out(&mut b, &[]);
        b.dig(7);
        assert_eq!(b.f[0].surge, 2, "surge is jammed");
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
        lay_out(&mut b, &[]);
        b.dig(7);
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
    fn cave_in_charms() {
        // Powder Keg: a cave-in gives surge, on top of the step for the crystal
        let mut b = duel();
        (b.f[0].mods.mine_surge, b.f[0].surge) = (3, 2);
        let hp = b.f[0].hp;
        lay_out(&mut b, &[8]);
        b.dig(7);
        assert_eq!((b.active, b.f[0].hp, b.f[0].surge), (0, hp - CAVE_DMG, 6));

        // Sixth Sense: the first cave-in each turn does no harm
        let mut b = duel();
        b.f[0].sense_left = 1;
        let hp = b.f[0].hp;
        lay_out(&mut b, &[8]);
        b.dig(7);
        assert_eq!((b.f[0].hp, b.f[0].sense_left), (hp, 0));

        // Lucky Coin: a dud does no harm
        let mut b = duel();
        b.f[0].duds = 1;
        let hp = b.f[0].hp;
        lay_out(&mut b, &[8]);
        b.dig(7);
        assert_eq!((b.f[0].hp, b.f[0].duds, b.active), (hp, 0, 0));

        // Salvager: a cave-in gives a stack of its board's element
        let mut b = duel();
        b.f[0].mods.salvage = 1;
        let element = b.f[0].board().element;
        lay_out(&mut b, &[8]);
        b.dig(7);
        assert_eq!(b.f[0].stacks, vec![element; CLAIM_GEMS + 1]);
    }

    #[test]
    fn shields_wear_off_and_reveals_have_a_floor() {
        let mut b = duel();
        b.f[0].shield = 10;
        b.end_turn();
        assert_eq!(b.f[0].shield, 10, "shield holds through the enemy's turn");
        b.end_turn();
        assert_eq!(b.f[0].shield, 5, "and is half gone when yours comes round");
        b.f[0].mods.keep_shield = 1;
        b.end_turn();
        b.end_turn();
        assert_eq!(b.f[0].shield, 5, "unless it is kept (Deepforger)");

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
        let attack = &COMBOS[basic(0, Stone)];
        assert!(b.f[0].fits(attack, &[Stone]), "a basic pays no toll");
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

        // Sandstorm cracks 2 tiles of it
        let mut b = duel();
        b.f[0].stacks = vec![Gale, Stone];
        let cracks = |b: &Battle| b.f[1].board().tiles.iter().map(|t| t.crack as usize).sum::<usize>();
        let before = cracks(&b);
        assert!(b.cast(combo("Sandstorm")));
        assert!(cracks(&b) >= before + 2);
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
    fn a_brittle_shield_blocks_then_shatters_onto_the_attacker() {
        let mut b = duel();
        b.f[0].elements = vec![Tide, Stone, Spark];
        (b.f[0].stacks, b.f[0].shield) = (vec![Stone, Spark], 10);
        assert!(b.cast(combo("Lodestone")));
        assert!(b.f[0].brittle && b.f[0].shield == 20, "doubled, then brittle");
        let conduction = combo("Conduction");
        let dmg = b.hit_damage(1, conduction);
        assert!(dmg > 0 && dmg <= 20);
        let (hp, foe) = (b.f[0].hp, b.f[1].hp);
        b.land(1, conduction);
        assert_eq!(b.f[0].hp, hp, "it still blocks the hit");
        assert_eq!((b.f[0].shield, b.f[0].brittle), (0, false), "then all of it shatters");
        assert_eq!(b.f[1].hp, foe - 20, "onto the attacker, as much as the shield held");
        assert!(b.events.iter().any(|e| matches!(e, Event::Brittle { side: 0 })));
    }

    #[test]
    fn a_pierced_brittle_shield_is_gone_without_shattering_back() {
        let mut b = duel();
        b.f[0].elements = vec![Tide, Stone, Spark];
        (b.f[0].stacks, b.f[0].shield) = (vec![Stone, Spark], 10);
        assert!(b.cast(combo("Lodestone")));
        let foe = b.f[1].hp;
        b.land(1, combo("Magma"));
        assert_eq!((b.f[0].shield, b.f[0].brittle, b.f[1].hp), (0, false, foe), "pierced: nothing shatters back");
        assert!(b.events.iter().any(|e| matches!(e, Event::Pierced { side: 0 })));
        assert!(!b.events.iter().any(|e| matches!(e, Event::Brittle { .. })));
    }

    #[test]
    fn a_shield_worn_to_nothing_leaves_no_brittleness_behind() {
        let mut b = duel();
        (b.f[0].shield, b.f[0].brittle) = (5, true);
        b.hurt(0, 9);
        b.f[0].shield = 12;
        assert!(!b.f[0].brittle, "the new shield is plain");
    }

    #[test]
    fn a_brittle_shield_lasts_into_the_next_wave() {
        let mut b = duel();
        b.f[0].elements = vec![Tide, Stone, Spark];
        (b.f[0].stacks, b.f[0].shield) = (vec![Stone, Spark], 10);
        assert!(b.cast(combo("Lodestone")));
        b.start_wave();
        assert!(b.f[0].brittle && b.f[0].shield == 20, "the next foe still meets it");
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
    fn forge_rites_pay_off_only_when_played_for() {
        // Spark Gap: only as the turn's first rite
        let gap = combo("Spark Gap");
        let mut b = duel();
        b.f[0].elements = vec![Ember, Spark];
        let first = b.hit_damage(0, gap);
        b.f[0].cast_n = 1;
        assert_eq!(b.hit_damage(0, gap), first - 7);
        // Iron Thorns: thorns for the rest of the battle, so cast it early
        let mut b = duel();
        b.f[0].elements = vec![Stone, Spark];
        b.f[0].stacks = vec![Stone, Spark, Stone, Spark];
        assert!(b.cast(combo("Iron Thorns")) && b.cast(combo("Iron Thorns")));
        assert_eq!((b.f[0].mods.thorns, b.f[0].shield, b.f[1].hp), (2, 8, 100), "no hit, two casts, 2 thorns");
    }

    #[test]
    fn setup_rites_spend_no_surge_and_mastery_and_echo_add_up() {
        // a rite that deals no damage keeps your surge and your first-rite slot for the hit after it
        let mut b = duel();
        b.f[0].surge = 3;
        b.f[0].stacks = vec![Gale, Stone];
        assert!(b.cast(combo("Sandstorm")));
        assert_eq!((b.f[0].surge, b.f[0].cast_n), (3, 0));

        // mastered: each level 3 rite gains its own trick, not a flat boost
        let mut b = duel();
        let magma = combo("Magma");
        b.f[0].ups = vec![combo_id(magma); 2];
        b.f[1].mods.armor = 3;
        b.f[1].evades_left = 1;
        assert_eq!(b.hit_damage(0, magma), 4 * 3 + 6, "Molten Core can't miss and ignores armor");
        let bulwark = combo("Bulwark");
        b.f[0].ups = vec![combo_id(bulwark); 2];
        b.f[0].stacks = bulwark.cost(3);
        assert!(b.cast(bulwark));
        assert_eq!(b.f[0].shield, 24);
        assert!(b.f[0].hold, "Citadel's shield skips wearing off once");
        b.end_turn();
        b.end_turn();
        assert_eq!(b.f[0].shield, 24, "the foe did nothing, and the shield did not wear off");
        b.f[1].stacks = vec![Tide, Spark, Tide];
        let siphon = combo("Conduction");
        b.f[0].ups = vec![combo_id(siphon); 2];
        b.f[0].stacks = siphon.cost(3);
        assert!(b.cast(siphon));
        assert_eq!(b.f[1].stacks.len(), 1, "Siphon Storm takes 2 foe stacks");
        assert!(b.f[0].stacks.len() == 2 && b.f[0].stacks.iter().all(|e| b.f[0].elements.contains(e)), "as the caster's own elements");
        let mut b = duel();
        let boil = combo("Boilover");
        b.f[0].ups = vec![combo_id(boil); 2];
        b.f[0].stacks = boil.cost(3);
        assert!(b.cast(boil));
        b.end_turn();
        assert_eq!(b.f[1].burn, 6, "Hellbroth: the foe's burn no longer halves");
        let mut b = duel();
        let tempest = combo("Tempest");
        b.f[0].elements = vec![Tide, Gale, Spark];
        b.f[0].ups = vec![combo_id(tempest); 2];
        let reveals = b.f[0].reveals_left;
        assert!(b.hit_steps(0, tempest).1.iter().any(|s| s.0 == format!("{reveals} picks")), "Eye of the Storm counts unused reveals");

        // echoed Geode Surge throws the shield once: the second cast finds it gone
        let mut b = duel();
        b.f[0].mods.echo = 1;
        b.f[0].shield = 20;
        b.f[0].elements = vec![Tide, Stone, Spark];
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
        assert_eq!((b.f[0].shield, b.f[1].hp), (12 * 2, 100), "doubled, and no damage");
        // doubling adds at most 12, then 24 at level 2, and anything once mastered
        assert_eq!([1, 2, 3].map(|l| double_cap(12, l)), [Some(12), Some(24), None]);
        // Geode Surge: 3 gems x 3 power, +6, then the 24 shield thrown at 150% (36), and the steps add up
        let (d, steps) = b.hit_steps(0, combo("Geode Surge"));
        assert_eq!(d, 9 + 6 + 36);
        assert_eq!(steps.iter().map(|s| s.1.trim_start_matches('+').parse::<i32>().unwrap()).sum::<i32>(), d);
        assert!(b.cast(combo("Geode Surge")));
        assert_eq!((b.f[0].shield, b.f[1].hp), (0, 100 - 51));
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
        assert_eq!(b.f[0].surge, 4);
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
    fn the_heros_turn_ends_itself_once_there_is_nothing_left_to_do() {
        for (gems, ends) in [(vec![], true), (vec![Ember, Gale], false)] {
            let mut b = duel();
            let (cur, n) = (b.f[0].cur, b.f[0].boards[0].tiles.len());
            let i = (0..n).find(|&i| b.f[0].boards[cur].live(i)).unwrap();
            (b.f[0].reveals_left, b.f[0].stacks) = (1, gems);
            assert!(b.dig(i));
            // the last pick: with a rite still paid for the hero keeps the turn, else it passes
            assert_eq!(b.active == 1, ends);
        }
    }

    #[test]
    fn a_heal_is_not_cast_at_full_hp_so_its_gem_is_kept() {
        let mut b = duel();
        let heal = &COMBOS[crate::data::basic(2, Gale)];
        b.f[0].rites = vec![crate::data::basic(2, Gale)];
        b.f[0].stacks = vec![Gale, Gale];
        b.f[0].hp = b.f[0].max_hp;
        assert!(b.ready(0).is_empty() && !b.cast(heal));
        assert_eq!(b.f[0].stacks.len(), 2, "the gem stays");
        b.f[0].hp -= 3;
        assert!(b.cast(heal) && b.f[0].hp == b.f[0].max_hp);
        assert!(!b.cast(heal), "full again: the second waits");
        assert_eq!(b.f[0].stacks.len(), 1);
        // the hand-over carries the HP of that moment, so the screen can show the heal before what follows
        b.events.clear();
        b.end_turn();
        assert!(matches!(b.events.first(), Some(Event::Pass { hp, .. }) if hp[0] == b.f[0].max_hp));
    }

    #[test]
    fn the_enemy_loads_its_rites_and_fires_them_as_its_next_turn_starts() {
        let mut b = duel();
        b.end_turn();
        assert_eq!(b.active, 1);
        b.f[1].stacks = vec![Tide, Spark];
        // however many gems a hero would hold out for, an enemy loads what it can pay for
        (b.f[1].reveals_left, b.f[1].aggro) = (0, 9);
        let hp = b.f[0].hp;
        b.ai_step();
        // loaded, not fired: the hero sees it coming, and how hard it would hit
        assert_eq!(b.active, 0);
        assert_eq!(b.f[0].hp, hp);
        assert_eq!(b.f[1].loaded.len(), 1);
        let due = b.loaded_damage();
        assert!(due > 0 && b.f[1].break_at == (due * BREAK_PCT / 100).max(BREAK_MIN));
        assert!(b.events.iter().any(|e| matches!(e, Event::Load)));
        b.end_turn();
        assert!(b.f[1].loaded.is_empty());
        assert_eq!(b.f[0].hp, hp - due, "it fires the moment its turn starts");
    }

    #[test]
    fn breaker_and_battery_keystones_move_the_break_and_the_surge_cap() {
        let break_at = |easy| {
            let mut b = duel();
            b.f[0].mods.easy_break = easy;
            b.f[1].stacks = vec![Tide, Spark, Tide, Spark];
            b.load();
            b.f[1].break_at
        };
        assert!(break_at(34) < break_at(0), "Breaker: their loads break sooner");
        let mut b = duel();
        b.f[0].mods.surge_room = 5;
        assert_eq!(b.surge_cap(0), MAX_SURGE + 5);
        b.f[1].mods.foe_surge_cap = 3;
        assert_eq!(b.surge_cap(0), 3, "a foe's damper still holds it down");
    }

    #[test]
    fn hitting_hard_enough_in_one_turn_breaks_the_load_to_half() {
        let mut b = duel();
        b.f[1].loaded = vec![combo("Conduction")];
        b.f[1].break_at = 30;
        b.f[0].stacks = vec![Ember, Gale];
        assert!(b.cast(combo("Raze")));
        assert!(!b.f[1].loaded.is_empty() && b.f[1].took > 0, "not enough yet");
        b.f[0].stacks = vec![Ember, Gale, Stone, Ember, Gale, Stone];
        while b.f[0].stacks.len() >= 3 && !b.f[1].broken {
            assert!(b.cast(combo("Upheaval")));
        }
        assert!(b.f[1].broken && b.events.iter().any(|e| matches!(e, Event::Break)));
        let (hp, half) = (b.f[0].hp, b.loaded_damage());
        b.end_turn();
        assert!(half > 0 && b.f[0].hp == hp - half, "a broken load still fires, at half");
        assert!(b.f[1].loaded.is_empty() && !b.f[1].broken);
        // what the hero dealt last turn does not count toward the next break
        b.f[1].loaded = vec![combo("Conduction")];
        b.end_turn();
        assert_eq!(b.f[1].took, 0);
    }

    #[test]
    fn a_rite_bringing_a_new_element_to_a_marked_target_reacts() {
        let mut b = duel();
        let (raze, magma) = (combo("Raze"), combo("Magma"));
        assert!(b.reactions(0, magma).is_empty(), "nothing reacts on an unmarked target");
        b.f[0].stacks = vec![Ember, Gale, Ember, Stone];
        assert!(b.cast(raze));
        assert_eq!(b.f[1].mark, vec![Ember, Gale]);
        // Magma brings Stone: crush is in the damage the rite shows
        assert_eq!(b.reactions(0, magma), vec![Stone]);
        let plain = duel().hit_damage(0, magma);
        assert_eq!(b.hit_damage(0, magma), plain + CRUSH_DMG);
        assert!(b.cast(magma));
        assert!(b.events.iter().any(|e| matches!(e, Event::React { from: 0, element: Stone })));
        assert_eq!(b.f[1].mark, vec![Ember, Stone]);
        // the same rite again brings nothing new
        assert!(b.reactions(0, magma).is_empty());
        // each element's own reaction
        let mut b = duel();
        b.f[1].mark = vec![Stone];
        b.f[0].elements = vec![Ember, Tide, Gale, Spark];
        b.f[0].stacks = vec![Ember, Tide, Gale, Spark];
        let (burn, shield, reveals, surge) = (b.f[1].burn, b.f[0].shield, b.f[0].reveals_left, b.f[0].surge);
        assert!(b.cast(combo("Steam Burst")));
        assert_eq!(b.f[1].burn, burn + IGNITE_BURN);
        assert_eq!(b.f[0].shield, shield + DOUSE_SHIELD);
        assert!(b.cast(combo("Thunderclap")));
        assert_eq!(b.f[0].reveals_left, reveals + 1 + GUST_REVEALS, "Steam Burst's own reveal, then gust");
        assert_eq!(b.f[0].surge, (surge + ARC_SURGE).min(MAX_SURGE));
        // a rite that does not hit neither reacts nor marks
        let mut b = duel();
        b.f[1].mark = vec![Ember];
        assert!(b.reactions(0, combo("Bulwark")).is_empty());
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
