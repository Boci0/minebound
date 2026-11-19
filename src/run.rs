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
    /// prices, in percent of normal
    pub markup: i32,
}

/// Who carries day `day`'s daily descent, and where (one of the first three expeditions): (hero, expedition).
pub fn daily(day: u64) -> (usize, usize) {
    let heroes = HEROES.len() as u64;
    ((day % heroes) as usize, (day / heroes % 3) as usize)
}

/// Day `day`'s daily descent goes by a name of its own, like "Sunken Lode".
pub fn daily_name(day: u64) -> String {
    const HOW: [&str; 12] = ["Ashen", "Sunken", "Hollow", "Silent", "Gilded", "Broken", "Pale", "Hungry", "Crooked", "Drowned", "Bitter", "Still"];
    const WHAT: [&str; 10] = ["Vein", "Seam", "Lode", "Drift", "Pit", "Shaft", "Delve", "Warren", "Rift", "Hoard"];
    // 12 and 10 share a factor, so the noun steps on a slower count to reach every pair
    format!("{} {}", HOW[(day % 12) as usize], WHAT[(day / 12 % 10) as usize])
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
    /// the rooms entered on this floor, in order
    pub trail: Vec<(usize, usize)>,
    pub note: String,
    /// for the end screen: fights won, rite damage dealt, and the biggest single rite (damage, name)
    pub wins: usize,
    pub dealt: i32,
    pub best: (i32, &'static str),
    /// a developer shortcut was used: the run is not saved
    pub debug: bool,
    /// the day this daily descent belongs to (days since 1970, UTC); `None` for an ordinary run
    pub daily: Option<u64>,
    /// keys carried, by index in `ENDINGS`, in that order: each opens a floor past the last boss
    pub keys: Vec<usize>,
    /// endings whose boss fell this run
    pub found: Vec<usize>,
    /// a key offered as soon as the current node is done
    omen: Option<usize>,
    /// the floor whose forge was stoked
    kindled: Option<usize>,
    /// shops left in a row without buying
    bare_shops: usize,
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
            trail: vec![],
            note: String::new(),
            wins: 0,
            dealt: 0,
            best: (0, ""),
            debug: false,
            daily: None,
            keys: vec![],
            found: vec![],
            omen: None,
            kindled: None,
            bare_shops: 0,
            given: vec![],
        };
        run.gen_map();
        run.note = format!("{}, floor 1.", EXPEDITIONS[exp].name);
        let stage = run.pick();
        (run, stage)
    }

    /// Day `day`'s daily descent: the same run for everyone, every floor cursed.
    pub fn new_daily(day: u64) -> (Run, Stage) {
        let (hero, exp) = daily(day);
        let (mut run, stage) = Run::new(day.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xD1B5_4A32_D192_ED03, hero, exp);
        run.daily = Some(day);
        run.gen_map(); // again, now that every floor is cursed
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
        for &k in &self.keys {
            (ENDINGS[k].me)(&mut m);
        }
        m
    }

    /// Floors this run has: the usual three, and one more for each key carried.
    pub fn floors(&self) -> usize {
        FLOORS + self.keys.len()
    }

    /// The secret ending this floor belongs to, past the last boss.
    pub fn ending(&self) -> Option<usize> {
        self.floor.checked_sub(FLOORS).map(|i| self.keys[i])
    }

    /// The secret ending fight `b` finds: its floor's ending, if `b` was that ending's boss and fell.
    pub fn ending_found(&self, b: &Battle) -> Option<usize> {
        self.ending().filter(|_| b.won() && is_boss(&b.f[1].name))
    }

    /// Columns before the boss: an ending's floor is shorter.
    pub fn cols(&self) -> usize {
        if self.ending().is_some() { 3 } else { COLS }
    }

    /// Whether choice `c` is shown: a way toward a key only while the key can still be taken.
    pub fn shows(&self, c: &Choice) -> bool {
        c.outs.iter().all(|o| !matches!(*o, Out::Omen(k) if self.keys.contains(&k) || self.floor >= FLOORS))
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
        match self.ending() {
            Some(e) => &ENDINGS[e].boss,
            None => &BOSSES[self.floor.min(BOSSES.len() - 1)][self.boss],
        }
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
        if col == self.cols() {
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
        self.twist = if EXPEDITIONS[self.exp].curses || self.daily.is_some() {
            let curses: Vec<usize> = (0..TWISTS.len()).filter(|&i| TWISTS[i].tone < 0).collect();
            curses[self.rng.below(curses.len())]
        } else {
            self.rng.below(TWISTS.len())
        };
        let cols = self.cols();
        let mut sizes: Vec<usize> = (0..cols).map(|_| 2 + self.rng.below(2)).collect();
        sizes.push(1);
        let mut map: Vec<Vec<Node>> = Vec::new();
        for (c, &n) in sizes.iter().enumerate() {
            map.push((0..n).map(|_| Node { kind: self.kind_for(c), next: vec![] }).collect());
        }
        // every floor has at least one shrine, somewhere between the first fight and the rest stop
        if !map.iter().flatten().any(|n| n.kind == NodeKind::Shrine) {
            let c = 1 + self.rng.below(cols - 2);
            let r = self.rng.below(map[c].len());
            map[c][r].kind = NodeKind::Shrine;
        }
        for c in 0..cols {
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
        // an ending's floor scales on from the last one: each ending past the first is deeper still
        let floor = self.floor as i32;
        // bosses grow only past the first ending's floor, so a later ending's boss keeps up with its floor
        let deeper = self.floor.saturating_sub(FLOORS) as i32;
        let (hp, power) = if scale { (d.hp * (350 + 200 * floor) / 100 * 125 / 100, d.power + floor + 2) } else { (d.hp * (220 + 40 * deeper) / 100, d.power + deeper) };
        let x = &EXPEDITIONS[self.exp];
        let hp = self.keys.iter().fold(hp * x.hp_pct / 100, |hp, &k| hp * ENDINGS[k].hp_pct / 100);
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
        let pool = self.ending().map_or(NORMALS, |e| ENDINGS[e].normals);
        // stack snatchers wait past the first floor
        let snatches = |d: &EnemyDef| {
            let mut m = Mods::default();
            (d.mods)(&mut m);
            m.pilfer > 0
        };
        let pool: Vec<&EnemyDef> = pool.iter().filter(|d| self.floor > 0 || !snatches(d)).collect();
        let d = pool[self.rng.below(pool.len())];
        self.enemy(d, true, 3)
    }

    pub fn enter(&mut self, col: usize, row: usize) -> Stage {
        self.pos = Some((col, row));
        self.trail.push((col, row));
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
                        let elite = match self.ending() {
                            Some(e) => &ENDINGS[e].elite,
                            None => &ELITES[self.rng.below(ELITES.len())],
                        };
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

    /// Adds a battle's damage to the run's totals. Called once per battle, also when it is given up.
    pub fn tally(&mut self, b: &Battle) {
        self.dealt += b.dealt;
        if b.best.0 > self.best.0 {
            self.best = b.best;
        }
    }

    /// Developer shortcut: straight to the last floor's boss fight.
    pub fn debug_last_fight(&mut self) -> Stage {
        self.debug = true;
        self.floor = FLOORS - 1;
        self.gen_map();
        self.enter(COLS, 0)
    }

    /// Developer shortcut: carry every key and go to the first ending's boss fight.
    pub fn debug_endings(&mut self) -> Stage {
        self.debug = true;
        self.keys = (0..ENDINGS.len()).collect();
        self.hp = self.max_hp();
        self.floor = FLOORS;
        self.gen_map();
        self.enter(self.cols(), 0)
    }

    /// The fight in progress is the last floor's boss: winning it wins the run.
    pub fn last_fight(&self) -> bool {
        self.floor + 1 == self.floors() && self.pos.is_some_and(|(c, r)| self.map[c][r].kind == NodeKind::Boss)
    }

    pub fn finish_battle(&mut self, b: &Battle) -> Stage {
        self.hp = b.f[0].hp;
        self.tally(b);
        if !b.won() {
            return Stage::End(false);
        }
        self.wins += 1;
        let kind = self.pos.map_or(NodeKind::Battle, |(c, r)| self.map[c][r].kind);
        if let Some(e) = self.ending().filter(|_| kind == NodeKind::Boss) {
            self.found.push(e);
        }
        if kind == NodeKind::Elite && self.kindled == Some(self.floor) {
            self.omen = Some(1);
        }
        if self.last_fight() {
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
        // a key's condition was just met: it is offered before the map, and only before the endings
        if let Some(k) = self.omen.take().filter(|k| !self.keys.contains(k) && self.floor < FLOORS) {
            return Stage::Event(&ENDINGS[k].offer, None);
        }
        if matches!(self.pos, Some((c, _)) if c == self.cols()) {
            self.floor += 1;
            self.pos = None;
            self.trail.clear();
            self.gen_map();
            self.note = match self.ending() {
                Some(e) => format!("{}, past the last boss.", ENDINGS[e].name),
                None => format!("{}, floor {}.", EXPEDITIONS[self.exp].name, self.floor + 1),
            };
        }
        Stage::Map
    }

    /// Leaves a shop. The second shop in a row left without buying offers the Clockwork Eye.
    pub fn leave_shop(&mut self, shop: &Shop) -> Stage {
        let bought = shop.stock.iter().any(|w| w.is_none());
        self.bare_shops = if bought { 0 } else { self.bare_shops + 1 };
        if self.bare_shops >= 2 {
            self.bare_shops = 0;
            self.omen = Some(2);
        }
        self.after_node()
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
                Out::Key(k) => {
                    if !self.keys.contains(&k) {
                        let before = self.max_hp();
                        self.keys.push(k);
                        self.keys.sort_unstable();
                        self.hp = (self.hp + (self.max_hp() - before).max(0)).min(self.max_hp());
                        msg.push(format!("You carry the {}. {} {}", ENDINGS[k].key, ENDINGS[k].boon, ENDINGS[k].curse));
                    }
                }
                Out::Omen(k) => self.omen = Some(k),
                Out::Kindle => self.kindled = Some(self.floor),
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
        let markup = self.keys.iter().fold(100, |p, &k| p * ENDINGS[k].price_pct / 100);
        Shop { stock: ids.into_iter().map(Some).collect(), sale, restocks, markup }
    }

    pub fn price(shop: &Shop, k: usize) -> i32 {
        let p = shop.stock[k].map_or(0, |id| CHARMS[id].rarity.price() * shop.markup / 100);
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
    fn stack_snatchers_wait_past_the_first_floor() {
        let (mut run, _) = Run::new(1, 0, 0);
        let rats = |run: &mut Run| (0..200).filter(|_| run.random_normal().mods.pilfer > 0).count();
        assert_eq!(rats(&mut run), 0);
        run.floor = 1;
        assert!(rats(&mut run) > 0);
    }

    #[test]
    fn a_daily_descent_is_the_same_all_day_and_cursed() {
        let (a, b) = (Run::new_daily(20_000).0, Run::new_daily(20_000).0);
        assert_eq!((a.seed, a.hero, a.exp, a.twist), (b.seed, b.hero, b.exp, b.twist));
        assert_ne!(a.seed, Run::new_daily(20_001).0.seed);
        assert!(TWISTS[a.twist].tone < 0 && a.exp < 3);
        // over a few days every hero takes a turn
        let heroes: std::collections::HashSet<usize> = (0..8).map(|d| daily(d).0).collect();
        assert_eq!(heroes.len(), HEROES.len());
        // names repeat only after every pairing has had its day
        let names: std::collections::HashSet<String> = (0..120).map(daily_name).collect();
        assert_eq!(names.len(), 120);
    }

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

    /// Plays a whole run with the enemy AI in the player's seat. Returns (won, floor reached), where
    /// a run past the last floor's boss counts as won on floor 3, as the save counts it.
    fn simulate(seed: u64, hero: usize, exp: usize) -> (bool, usize) {
        let (run, won) = play(seed, hero, exp, &[]);
        (won || run.floor >= FLOORS, run.floor.min(FLOORS - 1))
    }

    /// Plays a whole run, carrying `keys` from the start. Returns the run as it ended, and whether it won.
    fn play(seed: u64, hero: usize, exp: usize, keys: &[usize]) -> (Run, bool) {
        let (mut run, mut stage) = Run::new(seed, hero, exp);
        run.keys = keys.to_vec();
        run.hp = run.max_hp();
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
                    run.leave_shop(&shop)
                }
                Stage::Event(e, _) => {
                    let ok: Vec<&Choice> = e.choices.iter().filter(|c| run.can_afford(c) && run.shows(c)).collect();
                    let c = ok[pick.below(ok.len())];
                    run.choose(c, None);
                    assert!(run.hp >= 1 && run.shards >= 0 && run.hp <= run.max_hp());
                    run.after_node()
                }
                Stage::End(won) => {
                    // the end screen's totals: the best hit is part of the rite damage dealt (a fight can be won on a brittle shield alone)
                    assert!((run.dealt > 0) == (run.best.0 > 0) && run.best.0 <= run.dealt);
                    // every ending's boss that fell is one whose floor was played, in order
                    assert!(run.found.iter().enumerate().all(|(i, &e)| run.keys.get(i) == Some(&e)));
                    return (run, won);
                }
            };
        }
        panic!("run never ends: seed {seed} hero {hero}");
    }

    #[test]
    fn each_key_is_offered_by_its_deed_and_carries_its_boon_and_curse() {
        let (mut run, _) = Run::new(3, 0, 0);
        let reveals = run.mods().reveals;
        // the Brine Lantern: climbing down the well, then taking the lantern the card offers
        let well = ENCOUNTERS.iter().find(|e| e.title == "Echoing Well").unwrap();
        let down = well.choices.iter().find(|c| c.outs.iter().any(|o| matches!(o, Out::Omen(0)))).unwrap();
        run.choose(down, None);
        let Stage::Event(offer, None) = run.after_node() else { panic!("the lantern is offered") };
        run.choose(&offer.choices[0], None);
        assert_eq!((run.keys.clone(), run.mods().reveals), (vec![0], reveals + 1));
        assert!(!run.shows(down), "a key held is not offered again");
        assert!(matches!(run.after_node(), Stage::Map));
        // the Cinder Seal: a stoked forge, then an elite beaten on the same floor
        let forge = ENCOUNTERS.iter().find(|e| e.title == "Abandoned Forge").unwrap();
        run.shards = 50;
        run.choose(&forge.choices[1], None);
        run.map[1][0].kind = NodeKind::Elite;
        let Stage::Battle(mut b) = run.enter(1, 0) else { panic!("an elite fight") };
        b.debug_win();
        assert!(matches!(run.finish_battle(&b), Stage::Brand(_)));
        let Stage::Event(offer, None) = run.burn_brand(run.rites()[0], 0) else { panic!("the seal is offered") };
        run.choose(&offer.choices[0], None);
        assert_eq!((run.keys.clone(), run.mods().rite_pct, run.mods().self_burn), (vec![0, 1], 25, 4));
        // the Clockwork Eye: two shops in a row left without buying; buying in between starts the count again
        let shop = run.stock(0, &[]);
        assert!(matches!(run.leave_shop(&shop), Stage::Map));
        let mut bought = run.stock(0, &[]);
        run.shards = 500;
        assert!(run.buy(&mut bought, 0));
        assert!(matches!(run.leave_shop(&bought), Stage::Map));
        assert!(matches!(run.leave_shop(&shop), Stage::Map));
        let Stage::Event(offer, None) = run.leave_shop(&shop) else { panic!("the eye is offered") };
        let walk_past = run.choose(&offer.choices[1], None);
        assert!(run.keys.len() == 2 && !walk_past.is_empty(), "leaving it leaves it");
        run.keys.push(2);
        let marked = run.stock(0, &[]);
        assert_eq!(Run::price(&marked, (marked.sale + 1) % marked.stock.len()), marked.stock[(marked.sale + 1) % marked.stock.len()].map(|id| CHARMS[id].rarity.price() * 125 / 100).unwrap());
    }

    #[test]
    fn keys_add_short_floors_past_the_last_boss_in_order() {
        let (mut run, _) = Run::new(9, 1, 0);
        run.keys = vec![0, 2];
        assert_eq!(run.floors(), FLOORS + 2);
        run.floor = FLOORS - 1;
        run.gen_map();
        // winning the last regular floor's boss leads on, not to the end
        let Stage::Battle(mut b) = run.enter(COLS, 0) else { panic!() };
        let hp_regular = b.f[1].max_hp;
        b.debug_win();
        assert!(!run.last_fight());
        let mut stage = run.finish_battle(&b);
        while !matches!(stage, Stage::Map) {
            stage = match stage {
                Stage::Upgrade(ids) => run.upgrade(ids[0]),
                Stage::Pick(_) => run.after_node(),
                _ => panic!("only rewards before the next floor"),
            };
        }
        assert_eq!((run.floor, run.ending(), run.cols(), run.map.len()), (FLOORS, Some(0), 3, 4));
        assert_eq!(run.boss_def().name, ENDINGS[0].boss.name);
        let Stage::Battle(b) = run.enter(0, 0) else { panic!() };
        assert!(ENDINGS[0].normals.iter().any(|d| d.name == b.f[1].name), "the ending's own enemies");
        // its boss falls: found, and on to the next key's floor, whose boss is the last fight
        let Stage::Battle(mut b) = run.enter(3, 0) else { panic!() };
        assert!(b.f[1].max_hp > hp_regular && is_boss(&b.f[1].name));
        b.debug_win();
        let mut stage = run.finish_battle(&b);
        while !matches!(stage, Stage::Map) {
            stage = match stage {
                Stage::Upgrade(ids) => run.upgrade(ids[0]),
                Stage::Pick(_) => run.after_node(),
                _ => panic!("only rewards before the next floor"),
            };
        }
        assert_eq!((run.found.clone(), run.ending()), (vec![0], Some(2)));
        run.pos = Some((3, 0));
        assert!(run.last_fight());
    }

    #[test]
    fn the_endings_can_be_cleared_but_ask_for_more() {
        // the bot carrying one key from the start: how often it clears floor 3, then the ending as well
        const RUNS: u64 = 120;
        for e in 0..ENDINGS.len() {
            let (mut past3, mut found) = (0, 0);
            for hero in 0..HEROES.len() {
                for s in 0..RUNS {
                    let (run, _) = play(s, hero, 0, &[e]);
                    past3 += (run.floor >= FLOORS) as usize;
                    found += run.found.contains(&e) as usize;
                }
            }
            println!("{:20} past floor 3: {past3:3}  found: {found:3}  (of {})", ENDINGS[e].name, RUNS as usize * HEROES.len());
            assert!(found > 0, "{} can never be cleared", ENDINGS[e].name);
            assert!(found < past3, "{} is never lost", ENDINGS[e].name);
        }
        // all three keys at once: each ending deeper than the one before, and every curse stacked
        let all: Vec<usize> = (0..ENDINGS.len()).collect();
        let mut reached = [0; 4];
        for hero in 0..HEROES.len() {
            for s in 0..RUNS {
                reached[play(s, hero, 0, &all).0.found.len()] += 1;
            }
        }
        println!("all keys: runs ending with 0/1/2/3 endings found: {reached:?}");
        assert!(reached[3] > 0, "all three endings can be cleared in one run");
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
