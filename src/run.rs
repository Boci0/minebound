use crate::{
    battle::{Battle, Fighter},
    data::*,
    rng::Rng,
};

pub const FLOORS: usize = 3;
pub const COLS: usize = 5;
/// HP everyone recovers after a won fight
pub const SHOP_SIZE: usize = 4;
/// the first restock; each one after costs this much more
pub const RESTOCK_PRICE: i32 = 10;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NodeKind {
    Battle,
    Elite,
    Encounter,
    Shop,
    Treasure,
    Boss,
    /// a fight whose prize is a blessing
    Shrine,
}

impl NodeKind {
    pub fn name(self) -> &'static str {
        ["Battle", "Elite", "Encounter", "Shop", "Treasure", "Boss", "Shrine"][self as usize]
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
    /// a boss fell: pick one of these skills (by index in `COMBOS`) to upgrade
    Upgrade(Vec<usize>),
    /// a shrine fight was won: pick one of these blessings (by index in `BLESSINGS`), or keep yours
    Blessing(Vec<usize>),
    /// an elite fell: pick one of these brands (by index in `BRANDS`) and a rite to burn it onto
    Brand(Vec<usize>),
    Shop(Shop),
    Event(&'static Encounter, Option<String>),
    End(bool),
}

/// A shop visit. Bought wares leave an empty (`None`) spot on the counter.
pub struct Shop {
    pub stock: Vec<Option<usize>>,
    /// the ware sold at a quarter off
    pub sale: usize,
    pub restocks: i32,
}

pub struct Run {
    pub seed: u64,
    rng: Rng,
    pub hero: usize,
    /// which expedition, by index in `EXPEDITIONS`
    pub exp: usize,
    pub hp: i32,
    pub bonus_hp: i32,
    pub shards: i32,
    pub charms: Vec<usize>,
    /// upgraded skills, by index in `COMBOS`; a repeat is another level
    pub ups: Vec<usize>,
    /// brands burnt onto rites: (index in `COMBOS`, index in `BRANDS`), one per rite
    pub brands: Vec<(usize, usize)>,
    /// the one blessing held, by index in `BLESSINGS`
    pub blessing: Option<usize>,
    pub floor: usize,
    /// which of the floor's two bosses waits at the end
    pub boss: usize,
    /// this floor's rule twist, by index in `TWISTS`
    pub twist: usize,
    pub map: Vec<Vec<Node>>,
    pub pos: Option<(usize, usize)>,
    pub note: String,
    /// pacts given up in the choice being applied, so the same trade cannot hand them back
    given: Vec<usize>,
}

impl Run {
    pub fn new(seed: u64, hero: usize, exp: usize) -> (Run, Stage) {
        let mut run = Run {
            seed,
            rng: Rng::new(seed),
            hero,
            exp,
            hp: HEROES[hero].hp,
            bonus_hp: 0,
            shards: 20,
            charms: vec![],
            ups: vec![],
            brands: vec![],
            blessing: None,
            floor: 0,
            boss: 0,
            twist: 0,
            map: vec![],
            pos: None,
            note: String::new(),
            given: vec![],
        };
        run.gen_map();
        run.note = format!("{}, floor 1.", EXPEDITIONS[exp].name);
        let stage = run.pick();
        (run, stage)
    }

    pub fn mods(&self) -> Mods {
        let mut m = Mods::default();
        (HEROES[self.hero].passive)(&mut m);
        for &c in &self.charms {
            (CHARMS[c].apply)(&mut m);
        }
        if let Some(b) = self.blessing {
            (BLESSINGS[b].apply)(&mut m);
        }
        (TWISTS[self.twist].me)(&mut m);
        (EXPEDITIONS[self.exp].me)(&mut m);
        m
    }

    /// Takes blessing `b` in place of the one held (None keeps it). Max HP gained comes as HP too.
    pub fn take_blessing(&mut self, b: Option<usize>) -> Stage {
        if b.is_some() {
            let before = self.max_hp();
            self.blessing = b;
            self.hp = (self.hp + (self.max_hp() - before).max(0)).min(self.max_hp());
        }
        self.after_node()
    }

    pub fn boss_def(&self) -> &'static EnemyDef {
        &BOSSES[self.floor.min(BOSSES.len() - 1)][self.boss]
    }

    pub fn level(&self, id: usize) -> usize {
        1 + self.ups.iter().filter(|&&u| u == id).count()
    }

    pub fn brand_of(&self, id: usize) -> Option<&'static Brand> {
        self.brands.iter().find(|b| b.0 == id).map(|b| &BRANDS[b.1])
    }

    /// The rites this hero can make, by index in `COMBOS`.
    pub fn rites(&self) -> Vec<usize> {
        combos_for(&HEROES[self.hero].elements).iter().map(|c| combo_id(c)).collect()
    }

    /// Burns `brand` onto rite `id`, replacing any brand it had.
    pub fn burn_brand(&mut self, id: usize, brand: usize) -> Stage {
        self.brands.retain(|b| b.0 != id);
        self.brands.push((id, brand));
        self.after_node()
    }

    /// Upgrades skill `id`, then the boss's charm reward.
    pub fn upgrade(&mut self, id: usize) -> Stage {
        self.ups.push(id);
        self.pick()
    }

    pub fn max_hp(&self) -> i32 {
        (HEROES[self.hero].hp + self.bonus_hp + self.mods().max_hp).max(1)
    }

    pub fn add_charm(&mut self, id: usize) {
        let before = self.max_hp();
        self.charms.push(id);
        self.hp += self.max_hp() - before;
    }

    /// Up to `n` charms the player does not own yet; rarer ones come up less often.
    /// `avoid`: ids not to offer this time (the wares a restock replaces).
    fn offer(&mut self, n: usize, avoid: &[usize]) -> Vec<usize> {
        let mut pool: Vec<usize> = (0..CHARMS.len()).filter(|i| !self.charms.contains(i) && !self.given.contains(i) && !avoid.contains(i)).collect();
        let mut ids = vec![];
        while ids.len() < n && !pool.is_empty() {
            let total: usize = pool.iter().map(|&i| CHARMS[i].rarity.weight()).sum();
            let mut roll = self.rng.below(total);
            let k = pool.iter().position(|&i| {
                let w = CHARMS[i].rarity.weight();
                if roll < w {
                    return true;
                }
                roll -= w;
                false
            });
            ids.push(pool.remove(k.expect("roll is below the total weight")));
        }
        ids
    }

    fn pick(&mut self) -> Stage {
        let options = self.offer(3, &[]);
        if options.is_empty() {
            self.shards += 20;
            return self.after_node();
        }
        Stage::Pick(options)
    }

    fn kind_for(&mut self, col: usize) -> NodeKind {
        if col == 0 {
            return NodeKind::Battle;
        }
        if col == COLS {
            return NodeKind::Boss;
        }
        match self.rng.below(100) {
            0..=29 => NodeKind::Battle,
            30..=49 | 85..=93 => NodeKind::Encounter,
            50..=64 if col >= 2 => NodeKind::Elite,
            50..=64 => NodeKind::Battle,
            65..=74 => NodeKind::Shop,
            75..=84 => NodeKind::Shrine,
            _ => NodeKind::Treasure,
        }
    }

    fn gen_map(&mut self) {
        self.boss = self.rng.below(2);
        self.twist = if EXPEDITIONS[self.exp].curses {
            let curses: Vec<usize> = (0..TWISTS.len()).filter(|&i| TWISTS[i].tone < 0).collect();
            curses[self.rng.below(curses.len())]
        } else {
            self.rng.below(TWISTS.len())
        };
        let mut sizes: Vec<usize> = (0..COLS).map(|_| 2 + self.rng.below(2)).collect();
        sizes.push(1);
        let mut map: Vec<Vec<Node>> = Vec::new();
        for (c, &n) in sizes.iter().enumerate() {
            map.push((0..n).map(|_| Node { kind: self.kind_for(c), next: vec![] }).collect());
        }
        // every floor has at least one shrine, somewhere between the first fight and the rest stop
        if !map.iter().flatten().any(|n| n.kind == NodeKind::Shrine) {
            let c = 1 + self.rng.below(COLS - 2);
            let r = self.rng.below(map[c].len());
            map[c][r].kind = NodeKind::Shrine;
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
        let (hp, power) = if scale { (d.hp * (350 + 200 * floor) / 100, d.power + floor) } else { (d.hp * 220 / 100, d.power) };
        let x = &EXPEDITIONS[self.exp];
        let hp = hp * x.hp_pct / 100;
        let mut m = Mods::default();
        (d.mods)(&mut m);
        (TWISTS[self.twist].foe)(&mut m);
        (x.foe)(&mut m);
        // only bosses come unscaled
        if !scale {
            (x.boss)(&mut m);
        }
        m.reveals -= slow;
        Fighter::new(d.name, d.sprite, d.desc, hp, hp, power, d.elements, m, d.aggro)
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
            NodeKind::Battle | NodeKind::Shrine | NodeKind::Elite | NodeKind::Boss => {
                let foes = match kind {
                    NodeKind::Battle | NodeKind::Shrine => {
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
                    _ => vec![self.enemy(self.boss_def(), false, 1)],
                };
                let h = &HEROES[self.hero];
                let mut me = Fighter::new(h.name, h.sprite, h.passive_desc, self.hp, self.max_hp(), h.power, &h.elements, self.mods(), 3);
                me.ups = self.ups.clone();
                me.brands = self.brands.clone();
                Stage::Battle(Battle::new(me, foes, Rng::new(self.rng.next())))
            }
            NodeKind::Encounter => Stage::Event(&ENCOUNTERS[self.rng.below(ENCOUNTERS.len())], None),
            NodeKind::Shop => Stage::Shop(self.stock(0, &[])),
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
            _ => 18 + self.rng.below(7) as i32,
        };
        let shards = base * (100 + m.shard_pct) / 100;
        self.shards += shards;
        // every battle ends at full HP: only the fight in front of you can kill you
        self.hp = self.max_hp();
        self.note = format!("Won the fight: +{shards} shards, HP restored.");
        match kind {
            NodeKind::Battle => self.after_node(),
            NodeKind::Boss => {
                let ids: Vec<usize> = self.rites().into_iter().filter(|&id| self.level(id) < MAX_LEVEL).collect();
                if ids.is_empty() { self.pick() } else { Stage::Upgrade(ids) }
            }
            NodeKind::Shrine => {
                let mut pool: Vec<usize> = (0..BLESSINGS.len()).filter(|&b| Some(b) != self.blessing).collect();
                Stage::Blessing((0..3).map(|_| pool.remove(self.rng.below(pool.len()))).collect())
            }
            NodeKind::Elite => {
                let mut pool: Vec<usize> = (0..BRANDS.len()).collect();
                Stage::Brand((0..3).map(|_| pool.remove(self.rng.below(pool.len()))).collect())
            }
            _ => self.pick(),
        }
    }

    /// Back to the map; past a boss, that means the next floor.
    pub fn after_node(&mut self) -> Stage {
        if matches!(self.pos, Some((c, _)) if c == COLS) {
            self.floor += 1;
            self.pos = None;
            self.gen_map();
            self.note = format!("{}, floor {}.", EXPEDITIONS[self.exp].name, self.floor + 1);
        }
        Stage::Map
    }

    pub fn can_afford(&self, c: &Choice) -> bool {
        let cost: i32 = c.outs.iter().map(|o| if let Out::Shards(v) = o { -v.min(&0) } else { 0 }).sum();
        let gives = c.outs.iter().filter(|o| matches!(o, Out::LoseCharm)).count();
        cost <= self.shards && gives <= self.charms.len()
    }

    /// `give` is the pact the player chose to give up, for choices that take one (random if None).
    pub fn choose(&mut self, c: &Choice, give: Option<usize>) -> String {
        let mut msg = vec![];
        self.apply(c.outs, give, &mut msg);
        self.given.clear();
        msg.join(" ")
    }

    fn apply(&mut self, outs: &[Out], give: Option<usize>, msg: &mut Vec<String>) {
        for o in outs {
            match *o {
                Out::MaxHp(v) => {
                    self.bonus_hp += v;
                    self.hp = (self.hp + v.max(0)).clamp(1, self.max_hp());
                    msg.push(format!("Max HP {v:+}."));
                }
                Out::Shards(v) => {
                    self.shards = (self.shards + v).max(0);
                    msg.push(format!("{v:+} shards."));
                }
                Out::GainCharm => match self.offer(1, &[]).first() {
                    Some(&id) => {
                        self.add_charm(id);
                        msg.push(format!("Found a pact: {}.", CHARMS[id].name));
                    }
                    None => {
                        self.shards += 20;
                        msg.push("Nothing new here, only 20 shards.".into());
                    }
                },
                Out::LoseCharm => {
                    if !self.charms.is_empty() {
                        let at = give.and_then(|g| self.charms.iter().position(|&c| c == g));
                        let id = self.charms.remove(at.unwrap_or_else(|| self.rng.below(self.charms.len())));
                        self.given.push(id);
                        self.hp = self.hp.clamp(1, self.max_hp());
                        msg.push(format!("Gave up {}.", CHARMS[id].name));
                    }
                }
                Out::Text(s) => msg.push(s.into()),
                Out::Chance(pct, win, lose) => {
                    let outs = if self.rng.chance(pct) { win } else { lose };
                    self.apply(outs, give, msg);
                }
            }
        }
    }

    fn stock(&mut self, restocks: i32, avoid: &[usize]) -> Shop {
        let ids = self.offer(SHOP_SIZE, avoid);
        let sale = self.rng.below(ids.len().max(1));
        Shop { stock: ids.into_iter().map(Some).collect(), sale, restocks }
    }

    pub fn price(shop: &Shop, k: usize) -> i32 {
        let p = shop.stock[k].map_or(0, |id| CHARMS[id].rarity.price());
        if k == shop.sale { p * 3 / 4 } else { p }
    }

    pub fn restock_price(shop: &Shop) -> i32 {
        RESTOCK_PRICE * (shop.restocks + 1)
    }

    /// Buys the ware in spot `k`.
    pub fn buy(&mut self, shop: &mut Shop, k: usize) -> bool {
        let price = Self::price(shop, k);
        let Some(id) = shop.stock[k].filter(|_| self.shards >= price) else { return false };
        self.shards -= price;
        self.add_charm(id);
        shop.stock[k] = None;
        true
    }

    /// Swaps every ware on the counter for new ones.
    /// Restocking is only worth paying for while some pact is neither yours nor on the counter.
    pub fn can_restock(&self, shop: &Shop) -> bool {
        (0..CHARMS.len()).any(|i| !self.charms.contains(&i) && !shop.stock.contains(&Some(i)))
    }

    pub fn restock(&mut self, shop: &mut Shop) -> bool {
        let price = Self::restock_price(shop);
        if self.shards < price || !self.can_restock(shop) {
            return false;
        }
        self.shards -= price;
        let shelf: Vec<usize> = shop.stock.iter().flatten().copied().collect();
        *shop = self.stock(shop.restocks + 1, &shelf);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn giving_up_a_pact_takes_the_one_chosen() {
        let (mut run, _) = Run::new(1, 0, 0);
        run.charms = vec![3, 5, 8];
        let swap = ENCOUNTERS.iter().find(|e| e.title == "Pact Swapper").unwrap();
        // over many seeds, the pact given up never comes straight back from the same trade
        for seed in 0..50 {
            let (mut run, _) = Run::new(seed, 0, 0);
            run.charms = vec![3, 5, 8];
            run.choose(&swap.choices[0], Some(5));
            assert!(!run.charms.contains(&5), "seed {seed}");
        }
        run.choose(&swap.choices[0], Some(5));
        assert!(!run.charms.contains(&5) && run.charms.contains(&3) && run.charms.contains(&8));
        assert_eq!(run.charms.len(), 4);
    }

    #[test]
    fn every_floor_has_a_shrine_and_one_blessing_is_held_at_a_time() {
        for seed in 0..50 {
            let (run, _) = Run::new(seed, 0, 0);
            assert!(run.map.iter().flatten().any(|n| n.kind == NodeKind::Shrine), "seed {seed}");
        }
        let (mut run, _) = Run::new(1, 0, 0);
        let (vigor, iron) = (BLESSINGS.iter().position(|b| b.name == "Vigor").unwrap(), BLESSINGS.iter().position(|b| b.name == "Iron").unwrap());
        let (hp, max) = (run.hp, run.max_hp());
        run.take_blessing(Some(vigor));
        assert_eq!((run.hp, run.max_hp()), (hp + 30, max + 30), "max HP gained comes as HP too");
        run.take_blessing(Some(iron));
        assert_eq!((run.blessing, run.max_hp(), run.hp), (Some(iron), max, max), "the old blessing is gone");
        assert_eq!(run.mods().armor, 3 + 3, "Ashwarden's own 3, and Iron's 3");
    }

    #[test]
    fn a_restock_never_brings_back_the_wares_it_replaces() {
        for seed in 0..50 {
            let (mut run, _) = Run::new(seed, 0, 0);
            run.shards = 1000;
            let mut shop = run.stock(0, &[]);
            let before: Vec<usize> = shop.stock.iter().flatten().copied().collect();
            assert!(run.restock(&mut shop));
            assert!(shop.stock.iter().flatten().all(|id| !before.contains(id)), "seed {seed}");
        }
    }

    #[test]
    fn every_map_node_leads_to_the_boss_and_can_be_reached() {
        for seed in 0..200 {
            let (run, _) = Run::new(seed, 0, 0);
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
    fn simulate(seed: u64, hero: usize, exp: usize) -> (bool, usize) {
        let (mut run, mut stage) = Run::new(seed, hero, exp);
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
                Stage::Upgrade(ids) => run.upgrade(ids[pick.below(ids.len())]),
                Stage::Blessing(offers) => run.take_blessing(Some(offers[pick.below(offers.len())])),
                Stage::Brand(offers) => {
                    let rites = run.rites();
                    run.burn_brand(rites[pick.below(rites.len())], offers[pick.below(offers.len())])
                }
                Stage::Shop(mut shop) => {
                    for k in 0..shop.stock.len() {
                        run.buy(&mut shop, k);
                    }
                    run.after_node()
                }
                Stage::Event(e, _) => {
                    let ok: Vec<&Choice> = e.choices.iter().filter(|c| run.can_afford(c)).collect();
                    let c = ok[pick.below(ok.len())];
                    run.choose(c, None);
                    assert!(run.hp >= 1 && run.shards >= 0 && run.hp <= run.max_hp());
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
        // every hero's line first, so one failing does not hide how the others did
        let tally: Vec<(usize, usize)> = (0..HEROES.len())
            .map(|hero| {
                let results: Vec<(bool, usize)> = (0..RUNS).map(|s| simulate(s, hero, 0)).collect();
                let wins = results.iter().filter(|r| r.0).count();
                let past_floor_1 = results.iter().filter(|r| r.1 >= 1).count();
                println!("{:12} wins {:3}/{RUNS}  cleared floor 1: {:3}/{RUNS}", HEROES[hero].name, wins, past_floor_1);
                (wins, past_floor_1)
            })
            .collect();
        for (hero, &(wins, past_floor_1)) in tally.iter().enumerate() {
            // the bot plays plainly (never banks stacks for a combo), so a human should beat these
            assert!(past_floor_1 as u64 >= RUNS * 30 / 100, "{} dies too early", HEROES[hero].name);
            assert!((wins as u64) >= RUNS * 5 / 100, "{} cannot win", HEROES[hero].name);
            assert!((wins as u64) <= RUNS * 60 / 100, "{} wins too easily", HEROES[hero].name);
        }
    }

    #[test]
    fn each_expedition_is_harder_than_the_one_before() {
        const RUNS: u64 = 150;
        let score = |exp| (0..HEROES.len()).map(|h| (0..RUNS).map(|s| simulate(s, h, exp)).map(|r| r.1 + r.0 as usize).sum::<usize>()).sum::<usize>();
        let scores: Vec<usize> = (0..EXPEDITIONS.len()).map(score).collect();
        println!("expedition scores (floors reached + wins): {scores:?}");
        assert!(scores.windows(2).all(|w| w[1] <= w[0]), "{scores:?}");
        assert!(scores[EXPEDITIONS.len() - 1] < scores[0] * 2 / 3, "the last expedition must be much harder: {scores:?}");
    }
}
