use crate::{
    board::{Board, N},
    data::*,
    rng::Rng,
};

pub const MINE_DMG: i32 = 2;
pub const BASE_MINES: i32 = 5;
pub const REVEALS: i32 = 6;
pub const PRESSURE_TURN: usize = 7;
/// Safe reveals in a row that count toward surge; each adds `SURGE_PCT` to skill damage.
pub const MAX_SURGE: i32 = 5;
pub const SURGE_PCT: i32 = 20;

#[derive(Clone)]
pub struct Fighter {
    pub name: String,
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
    pub burn: i32,
    pub burn_dmg: i32,
    pub weak: i32,
    pub reveals_left: i32,
    pub duds: i32,
    /// AI only: it has decided to empty its stacks this turn
    firing: bool,
    pub mods: Mods,
}

impl Fighter {
    pub fn board(&self) -> &Board {
        &self.boards[self.cur]
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new(name: &str, desc: &str, hp: i32, max_hp: i32, power: i32, elements: &[Element], mods: Mods, aggro: usize) -> Fighter {
        Fighter {
            name: name.into(),
            desc: desc.into(),
            hp,
            max_hp,
            power: power + mods.power,
            elements: elements.to_vec(),
            aggro,
            boards: vec![],
            cur: 0,
            stacks: vec![],
            surge: 0,
            cast_n: 0,
            evades_left: 0,
            shield: 0,
            burn: 0,
            burn_dmg: 0,
            weak: 0,
            reveals_left: 0,
            duds: 0,
            firing: false,
            mods,
        }
    }
}

/// Things that just happened, for the screen to animate and voice.
pub enum Event {
    Flip,
    Claim,
    Mine,
    Cast { from: usize, name: &'static str, element: Element, miss: bool },
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
        b.start_wave();
        b
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

    /// Combos `s` can pay for right now: one stack of each element the combo needs.
    pub fn ready(&self, s: usize) -> Vec<&'static Combo> {
        COMBOS.iter().filter(|c| c.needs.iter().all(|e| self.f[s].stacks.contains(e))).collect()
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

    /// Damage of the next skill by `s` that spends `spent` stacks (before the target's shield).
    pub fn hit_damage(&self, s: usize, spent: usize, combo: Option<&Combo>) -> i32 {
        self.dmg(s, spent, combo, self.f[s].cast_n == 0)
    }

    fn dmg(&self, s: usize, spent: usize, combo: Option<&Combo>, first: bool) -> i32 {
        let f = &self.f[s];
        let mut d = spent as i32 * f.power;
        match combo {
            Some(c) => {
                for e in c.effects {
                    if let Effect::Damage(v) = e {
                        d += v * (100 + f.mods.combo_pct) / 100;
                    }
                }
            }
            None => d += f.mods.strike_bonus,
        }
        d = (d * (100 + self.surge_step(s) * f.surge) + 50) / 100;
        if first {
            d = (d * (100 + f.mods.first_pct) + 50) / 100;
        }
        d += self.pressure();
        if f.weak > 0 {
            d = (d + 1) / 2;
        }
        (d - self.f[1 - s].mods.armor).max(1)
    }

    /// The biggest combo the given stacks can pay for.
    fn best_combo(stacks: &[Element]) -> Option<&'static Combo> {
        COMBOS.iter().filter(|c| c.needs.iter().all(|e| stacks.contains(e))).max_by_key(|c| c.needs.len())
    }

    /// Total damage if `s` emptied its stacks now, biggest combos first.
    pub fn attack_damage(&self, s: usize) -> i32 {
        let mut stacks = self.f[s].stacks.clone();
        let (mut total, mut first) = (0, self.f[s].cast_n == 0);
        while let Some(c) = Self::best_combo(&stacks) {
            for e in c.needs {
                let k = stacks.iter().position(|x| x == e).unwrap();
                stacks.remove(k);
            }
            total += self.dmg(s, c.needs.len(), Some(c), first);
            first = false;
        }
        for _ in 0..stacks.len() {
            total += self.dmg(s, 1, None, first);
            first = false;
        }
        total
    }

    fn deal_board(&mut self, s: usize, k: usize) {
        // one fewer, the usual, or one more: boards differ in danger as well as shape
        let roll = self.rng.below(3) as i32 - 1;
        let mines = (BASE_MINES + roll + self.f[1 - s].mods.foe_mines - self.f[s].mods.fewer_mines).clamp(2, 14);
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
        }
        for s in 0..2 {
            let d = self.f[s].mods.opening;
            if d > 0 {
                self.hurt(1 - s, d);
            }
        }
        if !self.settle() {
            self.begin_turn();
        }
    }

    /// Shield soaks first. Returns HP actually lost.
    fn hurt(&mut self, s: usize, dmg: i32) -> i32 {
        let f = &mut self.f[s];
        let soak = f.shield.min(dmg).max(0);
        f.shield -= soak;
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
        let f = &mut self.f[a];
        f.reveals_left = (REVEALS + f.mods.reveals).max(1);
        f.surge = (if f.mods.keep_surge > 0 { f.surge } else { 0 }).max(f.mods.surge_start).min(jam);
        f.cast_n = 0;
        f.firing = false;
        if turn > 1 {
            f.power += f.mods.enrage;
        }
        f.hp = (f.hp + f.mods.regen).min(f.max_hp);
        f.shield += f.mods.shield_turn;
        if f.burn > 0 {
            // burn goes straight through shield
            f.burn -= 1;
            f.hp -= f.burn_dmg;
            let msg = format!("{} burns: -{}", f.name, f.burn_dmg);
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
        self.active = 1 - a;
        if self.active == 0 {
            self.turn += 1;
        }
        self.begin_turn();
    }

    /// Nothing left to flip and nothing left to fire: the turn passes on its own.
    fn tidy(&mut self, a: usize) {
        if !self.over() && self.active == a && self.f[a].reveals_left <= 0 && self.f[a].stacks.is_empty() {
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
        let flip = self.f[a].boards[cur].flip(i, &mut self.rng);
        let jam = self.surge_cap(a);
        let f = &mut self.f[a];
        f.reveals_left -= 1;
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
            self.events.push(if n > 0 { Event::Claim } else { Event::Flip });
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
        if f.mods.salvage > 0 {
            let gain = flip.broken;
            f.stacks.extend(std::iter::repeat(element).take(gain));
        }
        let d = if f.duds > 0 {
            f.duds -= 1;
            None
        } else {
            f.surge = 0;
            Some((MINE_DMG - f.mods.mine_guard).max(0))
        };
        let spite = f.mods.mine_spite;
        if spent {
            self.deal_board(a, cur);
        }
        let Some(d) = d else {
            self.log.push(format!("{name} hits a dud mine."));
            self.tidy(a);
            return true;
        };
        self.hurt(a, d);
        self.log.push(format!("{name} hits a mine: -{d}"));
        // some opponents feed on your mistakes
        let t = 1 - a;
        for _ in 0..self.f[t].mods.foe_mine_gift.max(0) {
            let k = self.rng.below(self.f[t].elements.len());
            let e = self.f[t].elements[k];
            self.f[t].stacks.push(e);
        }
        if spite > 0 {
            self.hurt(t, spite);
        }
        if !self.settle() {
            self.end_turn();
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

    /// Strike: fire stack number `k` on its own for plain damage.
    pub fn strike(&mut self, k: usize) -> bool {
        let a = self.active;
        if self.over() || k >= self.f[a].stacks.len() {
            return false;
        }
        let e = self.f[a].stacks.remove(k);
        self.land(a, 1, None, e);
        true
    }

    /// Cast a combo skill, paying one stack of each element it needs.
    pub fn cast(&mut self, c: &'static Combo) -> bool {
        let a = self.active;
        if self.over() || !c.needs.iter().all(|e| self.f[a].stacks.contains(e)) {
            return false;
        }
        for e in c.needs {
            let k = self.f[a].stacks.iter().position(|x| x == e).unwrap();
            self.f[a].stacks.remove(k);
        }
        self.land(a, c.needs.len(), Some(c), c.needs[0]);
        true
    }

    /// Fire the single best thing the active side can afford. False when out of stacks.
    pub fn cast_best(&mut self) -> bool {
        let a = self.active;
        match Self::best_combo(&self.f[a].stacks) {
            Some(c) => self.cast(c),
            None => self.strike(self.f[a].stacks.len().wrapping_sub(1)),
        }
    }

    /// Empty every stack at the current enemy, biggest combos first.
    pub fn unleash(&mut self) {
        let (a, wave) = (self.active, self.wave);
        while self.active == a && self.wave == wave && self.cast_best() {}
    }

    fn land(&mut self, a: usize, spent: usize, combo: Option<&'static Combo>, element: Element) {
        let t = 1 - a;
        let dmg = self.hit_damage(a, spent, combo);
        let name = combo.map_or("Strike", |c| c.name);
        self.f[a].cast_n += 1;
        if self.f[t].evades_left > 0 {
            // the whole skill whiffs: no damage, no effects, stacks still spent
            self.f[t].evades_left -= 1;
            self.events.push(Event::Cast { from: a, name, element, miss: true });
            self.log.push(format!("{}: {name} misses", self.f[a].name));
            self.tidy(a);
            return;
        }
        if combo.is_none() && self.f[a].mods.strike_burn > 0 {
            self.f[t].burn = self.f[t].burn.max(self.f[a].mods.strike_burn);
            self.f[t].burn_dmg = 2 + self.f[a].mods.burn_bonus;
        }
        let pct = 100 + self.f[a].mods.combo_pct;
        for e in combo.map_or(&[][..], |c| c.effects) {
            match *e {
                Effect::Damage(_) => {} // already in hit_damage
                Effect::Burn(n) => {
                    self.f[t].burn = self.f[t].burn.max(n);
                    self.f[t].burn_dmg = 2 + self.f[a].mods.burn_bonus;
                }
                Effect::Shield(v) => self.f[a].shield += v * pct / 100,
                Effect::Heal(v) => self.f[a].hp = (self.f[a].hp + v * pct / 100).min(self.f[a].max_hp),
                Effect::Weaken(n) => self.f[t].weak = self.f[t].weak.max(n),
                Effect::Drain(n) => {
                    let keep = self.f[t].stacks.len().saturating_sub(n);
                    self.f[t].stacks.truncate(keep);
                }
                Effect::Scout(n) => {
                    let cur = self.f[a].cur;
                    self.f[a].boards[cur].flag_mines(n);
                }
                Effect::Pierce => self.f[t].shield = 0,
            }
        }
        let dealt = self.hurt(t, dmg);
        let steal = dealt * self.f[a].mods.lifesteal_pct / 100;
        self.f[a].hp = (self.f[a].hp + steal).min(self.f[a].max_hp);
        let thorns = self.f[t].mods.thorns;
        if thorns > 0 {
            self.hurt(a, thorns);
        }
        self.events.push(Event::Cast { from: a, name, element, miss: false });
        self.log.push(format!("{}: {name} for {dmg}", self.f[a].name));
        if !self.settle() {
            self.tidy(a);
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
        let lethal = n > 0 && self.attack_damage(a) >= self.f[1 - a].hp + self.f[1 - a].shield;
        let can_reveal = f.reveals_left > 0;
        if can_reveal && !lethal {
            // weigh every open tile on every board: progress toward claiming a crystal against
            // the mine risk (a mine next to a crystal also breaks it), with a nudge toward
            // elements it does not hold yet (that is how combos happen)
            let mut best: Option<(usize, usize, f32, f32)> = None;
            for (k, board) in f.boards.iter().enumerate() {
                let fresh = if f.stacks.contains(&board.element) { 1.0 } else { 1.3 };
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
                if n == 0 || (risk < 0.4 && score > 0.0) {
                    self.swap(k);
                    self.reveal(i);
                    return;
                }
            }
        }
        // firing when it declined a risky reveal keeps a cornered fighter from stalling
        if n > 0 && (lethal || n >= f.aggro || can_reveal) {
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

    fn duel() -> Battle {
        let me = Fighter::new("Me", "", 50, 50, 3, &[Ember, Gale, Stone], Mods::default(), 3);
        let foe = Fighter::new("Foe", "", 100, 100, 1, &[Tide, Spark], Mods::default(), 3);
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
        b.f[0].stacks = vec![Ember];
        b.f[0].surge = 1;
        // 3 power x1.2 = 3.6
        assert_eq!(b.hit_damage(0, 1, None), 4);
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
        assert!(b.cast(combo("Wildfire")));
        // 2 stacks x 3 power, no bonus damage
        assert_eq!((b.f[1].hp, b.f[1].burn), (94, 3));
        assert_eq!(b.f[0].stacks, vec![Stone]);
        assert_eq!(b.active, 0, "casting does not end the turn");
        assert!(b.strike(0));
        assert_eq!(b.f[1].hp, 91);
        assert!(b.f[0].stacks.is_empty());
        assert_eq!(b.events.iter().filter(|e| matches!(e, Event::Cast { .. })).count(), 2);
    }

    #[test]
    fn surge_multiplies_skill_damage() {
        let mut b = duel();
        b.f[0].stacks = vec![Ember, Gale, Stone];
        // Eruption: 3 stacks x 3 power + 10
        assert_eq!(b.attack_damage(0), 19);
        b.f[0].surge = MAX_SURGE;
        assert_eq!(b.attack_damage(0), 38);
        b.unleash();
        assert_eq!(b.f[1].hp, 62);
        assert!(b.f[0].stacks.is_empty());
    }

    #[test]
    fn enemy_rules_change_what_a_hit_does() {
        let mut b = duel();
        b.f[1].mods.armor = 2;
        b.f[0].stacks = vec![Ember, Ember];
        assert_eq!(b.hit_damage(0, 1, None), 1, "3 power minus 2 armor");
        b.f[1].mods.armor = 0;
        b.f[1].evades_left = 1;
        b.strike(0);
        assert_eq!(b.f[1].hp, 100, "the first skill misses");
        b.strike(0);
        assert_eq!(b.f[1].hp, 97);

        let mut b = duel();
        b.f[1].mods.foe_surge_cap = 2;
        b.f[0].surge = 0;
        clear_a_crystal(&mut b);
        assert!(b.f[0].surge <= 2, "surge is jammed");
    }

    #[test]
    fn charms_reward_different_plays() {
        let mut b = duel();
        b.f[0].mods.strike_bonus = 3;
        b.f[0].mods.first_pct = 100;
        b.f[0].stacks = vec![Ember, Ember];
        // first Strike: (3 + 3) doubled, second: 3 + 3
        assert_eq!(b.attack_damage(0), 18);
        b.unleash();
        assert_eq!(b.f[1].hp, 82);

        let mut b = duel();
        b.f[0].mods.claim_zap = 2;
        clear_a_crystal(&mut b);
        assert!(b.f[1].hp < 100, "claiming a crystal zaps the enemy");
    }

    #[test]
    fn next_wave_arrives_when_the_enemy_dies() {
        let me = Fighter::new("Me", "", 50, 50, 50, &[Ember], Mods::default(), 1);
        let foes = vec![
            Fighter::new("A", "", 5, 5, 1, &[Tide], Mods::default(), 1),
            Fighter::new("B", "", 5, 5, 1, &[Tide], Mods::default(), 1),
        ];
        let mut b = Battle::new(me, foes, Rng::new(2));
        b.f[0].stacks = vec![Ember, Ember];
        b.unleash();
        assert_eq!((b.wave, b.f[1].name.as_str(), b.active), (2, "B", 0));
        assert_eq!(b.f[0].stacks.len(), 1, "unleash stops when the enemy falls");
        assert!(!b.over());
        b.unleash();
        assert!(b.won());
    }
}
