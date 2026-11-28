use crate::{
    battle::{Battle, Fighter},
    data::*,
    rng::Rng,
};

pub const FLOORS: usize = 3;
pub const COLS: usize = 5;
/// HP everyone recovers after a won fight
/// pacts on a shop's shelf; a brand sits beside them
pub const SHOP_SIZE: usize = 3;
pub const BRAND_PRICE: i32 = 40;
/// a lesson levels up a rite; one per shop
pub const LESSON_PRICE: i32 = 60;
/// a new rite to learn; one per shop
pub const RITE_SHOP_PRICE: i32 = 55;
/// the first restock; each one after costs this much more
pub const RESTOCK_PRICE: i32 = 10;
/// in-run hero EXP from won fights (elites and bosses grant more)
pub const XP_NORMAL: u32 = 15;
pub const XP_ELITE: u32 = 30;
pub const XP_BOSS: u32 = 45;
/// rewards when choosing Vitality & Spoils on level-up
pub const LEVEL_HP: i32 = 12;
pub const LEVEL_SHARDS: i32 = 25;

/// EXP required to advance from `lvl` to `lvl + 1`.
pub fn xp_need(lvl: usize) -> u32 {
    25 + (lvl.saturating_sub(1) as u32) * 15
}

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

/// Outcome of an encounter choice (or a pre-resolved notice screen), including any pacts granted.
pub struct EventResult {
    pub text: String,
    pub gained: Vec<usize>,
}

/// Shown when a pact pick triggers after the player already holds every open pact.
static ALL_PACTS_HELD: Encounter = Encounter {
    title: "No Pacts Left",
    text: "Every pact in the depths is already yours.",
    choices: &[],
};

/// What the player is looking at. The UI draws it; the simulation plays it.
pub enum Stage {
    Map,
    Battle(Battle),
    Pick(Vec<usize>),
    /// Wild Hands wager handed over a bonus pact for this floor: (charm id, whether opening Pick follows)
    WildHands(usize, bool),
    /// a boss fell: pick one of these skills (by index in `COMBOS`) to upgrade
    Upgrade(Vec<usize>),
    /// a shrine fight was won: pick one of these blessings (by index in `BLESSINGS`), or keep yours
    Blessing(Vec<usize>),
    /// a hoard holds brands: pick one of these (by index in `BRANDS`) and a rite to burn it onto
    Brand(Vec<usize>),
    /// an elite fell (or a hoard teaches): learn one of these rites (by index in `COMBOS`), or none
    Learn(Vec<usize>),
    /// the hero gained a level: choose +Max HP & Shards or +Trait Power
    LevelUp,
    Shop(Shop),
    Event(&'static Encounter, Option<EventResult>),
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
    /// the brand for sale (index in `BRANDS`), until bought
    pub brand: Option<usize>,
    /// the lesson is still for sale
    pub lesson: bool,
    /// the rite for sale (index in `COMBOS`), until bought
    pub rite: Option<usize>,
}

/// Who carries day `day`'s daily descent, and where (one of the first three expeditions): (hero, expedition).
pub fn daily(day: u64) -> (usize, usize) {
    // only heroes every save has, so the day's run is the same for all
    let heroes = FIRST_HEROES as u64;
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
    /// the rites found this run, by index in `COMBOS`, at most `MAX_RITES`
    pub held: Vec<usize>,
    /// upgraded skills, by index in `COMBOS`; a repeat is another level
    pub ups: Vec<usize>,
    /// brands burnt onto rites: (index in `COMBOS`, index in `BRANDS`), up to the rite's level each, oldest first
    pub brands: Vec<(usize, usize)>,
    /// the one blessing held, by index in `BLESSINGS`
    pub blessing: Option<usize>,
    pub floor: usize,
    /// which of the floor's bosses waits at the end
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
    /// deeds the save had done as the run began, one bit per `save::DEEDS`: what they open can come up
    pub deeds: u32,
    /// wagers on, one bit per `WAGERS`
    pub wagers: u32,
    /// one-run Crucible relic forged in the Forge, if any
    pub crucible: Option<CrucibleRelic>,
    /// in-run hero level, current EXP toward next level, times Trait Power was chosen, and pending level-up choices
    pub hero_lvl: usize,
    pub hero_xp: u32,
    pub trait_rank: usize,
    pub pending_lvl: usize,
    /// encounters met this run, by index in `ENCOUNTERS`: none comes twice until all have
    met: Vec<usize>,
}

impl Run {
    /// A run with nothing a deed opens: the daily's, and every test's.
    pub fn new(seed: u64, hero: usize, exp: usize) -> (Run, Stage) {
        Run::with_deeds(seed, hero, exp, 0)
    }

    /// A run where what `deeds` opened can come up, from its first pick on.
    pub fn with_deeds(seed: u64, hero: usize, exp: usize, deeds: u32) -> (Run, Stage) {
        let mut run = Run {
            seed,
            rng: Rng::new(seed),
            hero,
            exp,
            hp: HEROES[hero].hp,
            bonus_hp: 0,
            shards: 20,
            charms: vec![],
            held: vec![],
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
            met: vec![],
            deeds,
            wagers: 0,
            crucible: None,
            hero_lvl: 1,
            hero_xp: 0,
            trait_rank: 0,
            pending_lvl: 0,
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
        for _ in 0..self.trait_rank {
            (HEROES[self.hero].trait_up)(&mut m);
        }
        if let Some(r) = self.crucible {
            (CRUCIBLE_BOONS[r.boons[0]].me)(&mut m);
            (CRUCIBLE_BOONS[r.boons[1]].me)(&mut m);
            (CRUCIBLE_CURSES[r.curse].me)(&mut m);
        }
        for &c in &self.charms {
            (CHARMS[c].apply)(&mut m);
        }
        for set in SETS.iter().enumerate().filter(|(k, _)| self.set_count(*k) >= SET_AT).map(|(_, d)| d) {
            (set.apply)(&mut m);
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

    pub fn brands_of(&self, id: usize) -> Vec<&'static Brand> {
        self.brands.iter().filter(|b| b.0 == id).map(|b| &BRANDS[b.1]).collect()
    }

    /// Whether rite `id` has every brand socket filled (one per level), so a new brand sears the oldest away.
    pub fn sockets_full(&self, id: usize) -> bool {
        self.brands.iter().filter(|b| b.0 == id).count() >= self.level(id)
    }

    /// The rites found this run, by index in `COMBOS`.
    pub fn rites(&self) -> Vec<usize> {
        self.held.clone()
    }

    /// What the hero casts in a fight: its three basics, then the rites it found.
    pub fn casts(&self) -> Vec<usize> {
        HEROES[self.hero].basics.iter().enumerate().map(|(k, &e)| basic(k, e)).chain(self.held.iter().copied()).collect()
    }

    /// Re-syncs the live battle hero's rites, levels, brands, and pact mods from the run (debug kit).
    pub fn sync_hero(&self, b: &mut Battle) {
        let old_mod_power = b.f[0].mods.power;
        let old_max = b.f[0].max_hp;
        let mods = self.mods();
        b.f[0].power += mods.power - old_mod_power;
        b.f[0].mods = mods;
        b.f[0].max_hp = self.max_hp();
        b.f[0].hp = (b.f[0].hp + (b.f[0].max_hp - old_max).max(0)).clamp(1, b.f[0].max_hp);
        b.f[0].ups = self.ups.clone();
        b.f[0].brands = self.brands.clone();
        b.f[0].rites = self.casts();
    }

    /// Up to `n` rites the hero's elements make and it does not hold yet, at random.
    fn rite_offer(&mut self, n: usize) -> Vec<usize> {
        let mut pool: Vec<usize> = pool(&HEROES[self.hero].elements).into_iter().filter(|i| !self.held.contains(i)).collect();
        (0..n.min(pool.len())).map(|_| pool.remove(self.rng.below(pool.len()))).collect()
    }

    /// Takes rite `id` (letting `drop` go first, its levels and brands with it); None takes nothing.
    /// After a boss the pact pick follows.
    pub fn learn(&mut self, choice: Option<(usize, Option<usize>)>) -> Stage {
        if let Some((id, drop)) = choice {
            self.teach(id, drop);
        }
        if self.here() == NodeKind::Boss { self.pick() } else { self.after_node() }
    }

    fn teach(&mut self, id: usize, drop: Option<usize>) {
        if let Some(d) = drop {
            self.held.retain(|&x| x != d);
            self.ups.retain(|&x| x != d);
            self.brands.retain(|b| b.0 != d);
        }
        if self.held.len() < MAX_RITES && !self.held.contains(&id) {
            self.held.push(id);
        }
    }

    /// Rites to learn, if any are left to find; else a pact pick.
    fn learn_or_pick(&mut self) -> Stage {
        let offers = self.rite_offer(3);
        if offers.is_empty() { self.pick() } else { Stage::Learn(offers) }
    }

    /// Sets the forge's wagers for this run, before its first room; Wild Hands hands over floor 1's
    /// pact now and returns its reveal stage when one is granted.
    pub fn arm(&mut self, wagers: u32) -> Option<Stage> {
        self.wagers = wagers;
        self.wild_hands().map(|id| Stage::WildHands(id, true))
    }

    /// Leaves the Wild Hands reveal screen: into the opening pact pick on floor 1, or the map after.
    pub fn after_wild_hands(&mut self, then_pick: bool) -> Stage {
        if then_pick { self.pick() } else { Stage::Map }
    }

    /// Equips a one-run Crucible relic carried in from the Forge and syncs starting HP to max HP.
    pub fn equip_crucible(&mut self, relic: Option<CrucibleRelic>) {
        self.crucible = relic;
        self.hp = self.max_hp();
    }

    /// Live description of this run's hero trait, reflecting any Deepen Trait ranks earned.
    pub fn trait_desc(&self) -> String {
        trait_desc(self.hero, self.trait_rank)
    }

    /// EXP needed for the next hero level.
    pub fn xp_next(&self) -> u32 {
        xp_need(self.hero_lvl)
    }

    /// Applies one pending level-up choice: `trait_up = true` strengthens the hero's trait,
    /// `trait_up = false` grants `+LEVEL_HP` max HP and `+LEVEL_SHARDS` shards.
    pub fn level_up(&mut self, trait_up: bool) -> Stage {
        self.pending_lvl = self.pending_lvl.saturating_sub(1);
        if trait_up {
            self.trait_rank += 1;
            self.note = format!("Level {}: deepened trait (Rank {}).", self.hero_lvl, self.trait_rank + 1);
        } else {
            self.bonus_hp += LEVEL_HP;
            self.hp = (self.hp + LEVEL_HP).min(self.max_hp());
            self.shards += LEVEL_SHARDS;
            self.note = format!("Level {}: +{LEVEL_HP} max HP, +{LEVEL_SHARDS} shards.", self.hero_lvl);
        }
        self.after_node()
    }

    fn wagered(&self, k: usize) -> bool {
        self.wagers & 1 << k != 0
    }

    /// With Wild Hands on, a random pact not yet held, as a floor begins. Returns its id if granted.
    fn wild_hands(&mut self) -> Option<usize> {
        if !self.wagered(WILD_HANDS) {
            return None;
        }
        let pool: Vec<usize> = (0..CHARMS.len()).filter(|&i| self.pact_open(i) && !self.charms.contains(&i)).collect();
        if pool.is_empty() {
            return None;
        }
        let id = pool[self.rng.below(pool.len())];
        self.add_charm(id);
        self.note += &format!(" Wild Hands gives {}.", CHARMS[id].name);
        Some(id)
    }

    /// Ore this run banks, ended as `won` after `wins` fights: 2 a fight, 10 a boss, 20 an ending,
    /// and more for each wager on. Developer runs bank none.
    pub fn ore(&self, wins: usize, won: bool) -> u32 {
        if self.debug {
            return 0;
        }
        let bosses = self.floor + won as usize;
        let base = 2 * wins as u32 + 10 * bosses as u32 + 20 * self.found.len() as u32;
        let pct: u32 = (0..WAGERS.len()).filter(|&k| self.wagered(k)).map(|k| WAGERS[k].ore_pct).sum();
        base * (100 + pct) / 100
    }

    /// Burns `brand` onto rite `id`; with its sockets full, the oldest brand on it goes.
    pub fn burn_brand(&mut self, id: usize, brand: usize) -> Stage {
        self.sear(id, brand);
        self.after_node()
    }

    fn sear(&mut self, id: usize, brand: usize) {
        if self.sockets_full(id) {
            let k = self.brands.iter().position(|b| b.0 == id).expect("a full rite has a brand");
            self.brands.remove(k);
        }
        self.brands.push((id, brand));
    }

    /// Upgrades skill `id`, then a boss's charm reward; a hoard's level is the whole prize.
    pub fn upgrade(&mut self, id: usize) -> Stage {
        self.ups.push(id);
        if self.here() == NodeKind::Boss { self.pick() } else { self.after_node() }
    }

    /// Pacts held of set `k` (index in `SETS`).
    pub fn set_count(&self, k: usize) -> usize {
        self.charms.iter().filter(|&&c| CHARMS[c].set as usize == k).count()
    }

    /// How often pact `i` is offered: its rarity's weight, leaning toward sets already held.
    fn weight(&self, i: usize) -> usize {
        CHARMS[i].rarity.weight() * (2 + self.set_count(CHARMS[i].set as usize))
    }

    pub fn max_hp(&self) -> i32 {
        (HEROES[self.hero].hp + self.bonus_hp + self.mods().max_hp).max(1)
    }

    /// Whether pact `i` can come up: always, unless a deed not yet done opens it.
    fn pact_open(&self, i: usize) -> bool {
        crate::save::is_open(self.deeds, crate::save::Unlock::Pact(CHARMS[i].name))
    }

    /// The brands that can come up this run.
    fn brand_pool(&self) -> Vec<usize> {
        (0..BRANDS.len()).filter(|&i| crate::save::is_open(self.deeds, crate::save::Unlock::Brand(BRANDS[i].name))).collect()
    }

    pub fn add_charm(&mut self, id: usize) {
        let before = self.max_hp();
        self.charms.push(id);
        self.hp = (self.hp + self.max_hp() - before).clamp(1, self.max_hp());
    }

    /// Up to `n` charms the player does not own yet; rarer ones come up less often.
    /// `avoid`: ids not to offer this time (the wares a restock replaces).
    /// `allow_burdens`: whether pure-downside cursed burdens may be rolled (random grants only, not picks/shops).
    fn offer(&mut self, n: usize, avoid: &[usize], allow_burdens: bool) -> Vec<usize> {
        let mut pool: Vec<usize> = (0..CHARMS.len())
            .filter(|&i| self.pact_open(i) && (allow_burdens || !CHARMS[i].burden) && !self.charms.contains(&i) && !self.given.contains(&i) && !avoid.contains(&i))
            .collect();
        let mut ids = vec![];
        while ids.len() < n && !pool.is_empty() {
            let total: usize = pool.iter().map(|&i| self.weight(i)).sum();
            let mut roll = self.rng.below(total);
            let k = pool.iter().position(|&i| {
                let w = self.weight(i);
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

    /// A hoard: pacts, a rite to learn, a level for a rite or a brand. Levels and brands need a rite held.
    fn treasure(&mut self) -> Stage {
        let ids: Vec<usize> = self.rites().into_iter().filter(|&id| self.level(id) < MAX_LEVEL).collect();
        match self.rng.below(4) {
            0 => self.pick(),
            2 if !ids.is_empty() => Stage::Upgrade(ids),
            3 if !self.held.is_empty() => {
                let mut pool = self.brand_pool();
                Stage::Brand((0..3).map(|_| pool.remove(self.rng.below(pool.len()))).collect())
            }
            _ => self.learn_or_pick(),
        }
    }

    /// The kind of room the player stands in.
    pub fn here(&self) -> NodeKind {
        self.pos.map_or(NodeKind::Battle, |(c, r)| self.map[c][r].kind)
    }

    fn pick(&mut self) -> Stage {
        let options = self.offer(3, &[], false);
        if options.is_empty() {
            self.shards += 20;
            self.note = "Every pact is already yours: +20 shards.".into();
            return Stage::Event(
                &ALL_PACTS_HELD,
                Some(EventResult {
                    text: "You pocket +20 shards instead.".into(),
                    gained: vec![],
                }),
            );
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
            // the first floor's elites wait a room longer, so a rite can be found before the first
            50..=64 if col >= 2 + usize::from(self.floor == 0) => NodeKind::Elite,
            50..=64 => NodeKind::Battle,
            65..=74 => NodeKind::Shop,
            75..=84 => NodeKind::Shrine,
            _ => NodeKind::Treasure,
        }
    }

    fn gen_map(&mut self) {
        self.boss = self.rng.below(BOSSES[self.floor.min(BOSSES.len() - 1)].len());
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
        // the last rooms before the boss are shops, so every path can spend its shards and buy a rite
        for n in &mut map[cols - 1] {
            n.kind = NodeKind::Shop;
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
    /// `hp_pct`: its kind's share of HP on top of the floor's scaling.
    fn enemy(&self, d: &EnemyDef, scale: bool, slow: i32, hp_pct: i32) -> Fighter {
        // an ending's floor scales on from the last one: each ending past the first is deeper still
        let floor = self.floor as i32;
        // bosses gain 40% HP per floor up to the last, then grow past the first ending's floor so a later ending's boss keeps up;
        // +1 power since bosses load every turn instead of saving up for one big blow (smaller loads break easier)
        let deeper = self.floor.saturating_sub(FLOORS) as i32;
        let (hp, power) = if scale {
            (d.hp * (130 + 160 * floor) / 100 * 125 / 100, d.power + floor + 2 + i32::from(floor >= 2))
        } else {
            (d.hp * (220 + 40 * floor.min(FLOORS as i32 - 1) + 40 * deeper) / 100, d.power + deeper + i32::from(floor > 0) + i32::from(floor >= 2))
        };
        let hp = hp * hp_pct / 100 * 130 / 100;
        let x = &EXPEDITIONS[self.exp];
        let hp = self.keys.iter().fold(hp * x.hp_pct / 100, |hp, &k| hp * ENDINGS[k].hp_pct / 100);
        let mut m = Mods::default();
        (d.mods)(&mut m);
        (TWISTS[self.twist].foe)(&mut m);
        (x.foe)(&mut m);
        let mut hp = hp;
        for w in WAGERS.iter().enumerate().filter(|(k, _)| self.wagered(*k)).map(|(_, w)| w) {
            (w.foe)(&mut m);
            hp = (hp * w.hp_pct / 100).max(1);
        }
        if let Some(r) = self.crucible {
            (CRUCIBLE_BOONS[r.boons[0]].foe)(&mut m);
            (CRUCIBLE_BOONS[r.boons[1]].foe)(&mut m);
            (CRUCIBLE_CURSES[r.curse].foe)(&mut m);
        }
        // only bosses come unscaled
        if !scale {
            (x.boss)(&mut m);
        }
        m.reveals -= slow;
        // enemies never hold gems back (see `ai_step`), so they have no aggro
        Fighter::new(d.name, d.sprite, d.desc, hp, hp, power, d.elements, m, 0)
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
        self.enemy(d, true, 1, 130)
    }

    /// One line of where the run stands, for the dev log.
    pub fn snapshot(&self, stage: &Stage) -> String {
        let stage = match stage {
            Stage::Map => "map".to_string(),
            Stage::Battle(b) => format!("battle vs {} ({} waves)", b.f[1].name, b.waves),
            Stage::Pick(v) => format!("pick a pact of {}", v.len()),
            Stage::WildHands(id, _) => format!("wild hands ({})", CHARMS[*id].name),
            Stage::Upgrade(v) => format!("upgrade one of {}", v.len()),
            Stage::Blessing(v) => format!("blessing of {}", v.len()),
            Stage::Brand(v) => format!("brand of {}", v.len()),
            Stage::Learn(v) => format!("learn one of {:?}", v.iter().map(|&i| COMBOS[i].name).collect::<Vec<_>>()),
            Stage::LevelUp => format!("level up (lv {})", self.hero_lvl),
            Stage::Shop(s) => format!("shop (rite {:?})", s.rite.map(|i| COMBOS[i].name)),
            Stage::Event(e, _) => format!("encounter {}", e.title),
            Stage::End(won) => format!("end, won {won}"),
        };
        let rites: Vec<String> = self.held.iter().map(|&i| format!("{} L{}", COMBOS[i].name, 1 + self.ups.iter().filter(|&&u| u == i).count())).collect();
        format!(
            "stage {stage} | floor {} at {:?} | hp {}/{} shards {} | rites {rites:?} brands {} | pacts {:?} | blessing {:?}",
            self.floor + 1, self.pos, self.hp, self.max_hp(), self.shards, self.brands.len(),
            self.charms.iter().map(|&c| CHARMS[c].name).collect::<Vec<_>>(), self.blessing.map(|b| BLESSINGS[b].name)
        )
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
                            None => {
                                let pool = ELITES[self.floor.min(ELITES.len() - 1)];
                                &pool[self.rng.below(pool.len())]
                            }
                        };
                        // on the first floor the elite fights alone: a hero with only basics meets it fresh
                        if self.floor == 0 {
                            vec![self.enemy(elite, true, 1, 100)]
                        } else {
                            vec![self.random_normal(), self.enemy(elite, true, 1, 100)]
                        }
                    }
                    _ => vec![self.enemy(self.boss_def(), false, 1, 128)],
                };
                let mut foes = foes;
                if self.wagered(WILD_GIFTS) {
                    for f in &mut foes {
                        let id = self.rng.below(CHARMS.len());
                        let power = f.mods.power;
                        (CHARMS[id].apply)(&mut f.mods);
                        f.power += f.mods.power - power;
                        f.desc += &format!(" Wild gift: {}.", CHARMS[id].name);
                    }
                }
                let h = &HEROES[self.hero];
                let trait_text = self.trait_desc();
                let mut me = Fighter::new(h.name, h.sprite, &trait_text, self.hp, self.max_hp(), h.power, &h.elements, self.mods(), 3);
                me.ups = self.ups.clone();
                me.brands = self.brands.clone();
                me.rites = self.casts();
                Stage::Battle(Battle::new(me, foes, Rng::new(self.rng.next())))
            }
            NodeKind::Encounter => {
                if self.met.len() >= ENCOUNTERS.len() {
                    self.met.clear();
                }
                let fresh: Vec<usize> = (0..ENCOUNTERS.len()).filter(|i| !self.met.contains(i)).collect();
                let e = fresh[self.rng.below(fresh.len())];
                self.met.push(e);
                Stage::Event(&ENCOUNTERS[e], None)
            }
            NodeKind::Shop => Stage::Shop(self.stock(0, &[])),
            NodeKind::Treasure => self.treasure(),
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
        let kind = self.here();
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
        let xp = match kind {
            NodeKind::Boss => XP_BOSS,
            NodeKind::Elite => XP_ELITE,
            _ => XP_NORMAL,
        };
        self.hero_xp += xp;
        let mut gained = 0;
        while self.hero_xp >= xp_need(self.hero_lvl) {
            self.hero_xp -= xp_need(self.hero_lvl);
            self.hero_lvl += 1;
            self.pending_lvl += 1;
            gained += 1;
        }
        // every battle ends at full HP: only the fight in front of you can kill you
        self.hp = self.max_hp();
        self.note = if gained > 0 {
            format!("Won the fight: +{shards} shards, +{xp} XP (Level {}!), HP restored.", self.hero_lvl)
        } else {
            format!("Won the fight: +{shards} shards, +{xp} XP, HP restored.")
        };
        match kind {
            NodeKind::Battle => self.after_node(),
            NodeKind::Boss => {
                let ids: Vec<usize> = self.rites().into_iter().filter(|&id| self.level(id) < MAX_LEVEL).collect();
                if ids.is_empty() { self.learn_or_pick() } else { Stage::Upgrade(ids) }
            }
            NodeKind::Shrine => {
                let mut pool: Vec<usize> = (0..BLESSINGS.len()).filter(|&b| Some(b) != self.blessing).collect();
                Stage::Blessing((0..3).map(|_| pool.remove(self.rng.below(pool.len()))).collect())
            }
            NodeKind::Elite => self.learn_or_pick(),
            _ => self.pick(),
        }
    }

    /// Back to the map; past a boss, that means the next floor.
    pub fn after_node(&mut self) -> Stage {
        // a key's condition was just met: it is offered before the map, and only before the endings
        if let Some(k) = self.omen.take().filter(|k| !self.keys.contains(k) && self.floor < FLOORS) {
            return Stage::Event(&ENDINGS[k].offer, None);
        }
        if self.pending_lvl > 0 {
            return Stage::LevelUp;
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
            if let Some(id) = self.wild_hands() {
                return Stage::WildHands(id, false);
            }
        }
        Stage::Map
    }

    /// Leaves a shop. The second shop in a row left without buying offers the Clockwork Eye.
    pub fn leave_shop(&mut self, shop: &Shop) -> Stage {
        let bought = shop.stock.iter().any(|w| w.is_none()) || shop.brand.is_none() || !shop.lesson || shop.rite.is_none();
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
    pub fn choose(&mut self, c: &Choice, give: Option<usize>) -> EventResult {
        let mut msg = vec![];
        let mut gained = vec![];
        self.apply(c.outs, give, &mut msg, &mut gained);
        self.given.clear();
        EventResult { text: msg.join(" "), gained }
    }

    fn apply(&mut self, outs: &[Out], give: Option<usize>, msg: &mut Vec<String>, gained: &mut Vec<usize>) {
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
                Out::GainCharm => match self.offer(1, &[], true).first() {
                    Some(&id) => {
                        self.add_charm(id);
                        gained.push(id);
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
                    self.apply(outs, give, msg, gained);
                }
            }
        }
    }

    fn stock(&mut self, restocks: i32, avoid: &[usize]) -> Shop {
        let ids = self.offer(SHOP_SIZE, avoid, false);
        let sale = self.rng.below(ids.len().max(1));
        let markup = self.keys.iter().fold(100, |p, &k| p * ENDINGS[k].price_pct / 100);
        let pool = self.brand_pool();
        let brand = Some(pool[self.rng.below(pool.len())]);
        let rite = self.rite_offer(1).first().copied();
        Shop { stock: ids.into_iter().map(Some).collect(), sale, restocks, markup, brand, lesson: true, rite }
    }

    pub fn price(shop: &Shop, k: usize) -> i32 {
        let p = shop.stock[k].map_or(0, |id| CHARMS[id].rarity.price() * shop.markup / 100);
        if k == shop.sale { p * 3 / 4 } else { p }
    }

    pub fn brand_price(shop: &Shop) -> i32 {
        BRAND_PRICE * shop.markup / 100
    }

    pub fn lesson_price(shop: &Shop) -> i32 {
        LESSON_PRICE * shop.markup / 100
    }

    pub fn rite_price(shop: &Shop) -> i32 {
        RITE_SHOP_PRICE * shop.markup / 100
    }

    /// Buys the shop's rite, letting `drop` go if every rite slot is taken.
    pub fn buy_rite(&mut self, shop: &mut Shop, drop: Option<usize>) -> bool {
        let price = Self::rite_price(shop);
        let room = self.held.len() < MAX_RITES || drop.is_some_and(|d| self.held.contains(&d));
        let Some(id) = shop.rite.filter(|_| self.shards >= price && room) else { return false };
        self.shards -= price;
        self.teach(id, drop.filter(|_| self.held.len() >= MAX_RITES));
        shop.rite = None;
        true
    }

    /// Buys the shop's brand and burns it onto rite `id`.
    pub fn buy_brand(&mut self, shop: &mut Shop, id: usize) -> bool {
        let price = Self::brand_price(shop);
        let Some(brand) = shop.brand.filter(|_| self.shards >= price) else { return false };
        self.shards -= price;
        self.sear(id, brand);
        shop.brand = None;
        true
    }

    /// Buys the shop's lesson for rite `id`, one level up.
    pub fn buy_lesson(&mut self, shop: &mut Shop, id: usize) -> bool {
        let price = Self::lesson_price(shop);
        if !shop.lesson || self.shards < price || self.level(id) >= MAX_LEVEL {
            return false;
        }
        self.shards -= price;
        self.ups.push(id);
        shop.lesson = false;
        true
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
        (0..CHARMS.len()).any(|i| self.pact_open(i) && !CHARMS[i].burden && !self.charms.contains(&i) && !shop.stock.contains(&Some(i)))
    }

    pub fn restock(&mut self, shop: &mut Shop) -> bool {
        let price = Self::restock_price(shop);
        if self.shards < price || !self.can_restock(shop) {
            return false;
        }
        self.shards -= price;
        let shelf: Vec<usize> = shop.stock.iter().flatten().copied().collect();
        let (lesson, rite) = (shop.lesson, shop.rite);
        *shop = self.stock(shop.restocks + 1, &shelf);
        (shop.lesson, shop.rite) = (lesson, rite);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rite_holds_one_brand_per_level_and_sears_the_oldest() {
        let (mut run, _) = Run::new(1, 0, 0);
        run.held = pool(&HEROES[0].elements)[..1].to_vec();
        let id = run.rites()[0];
        let names = |run: &Run| run.brands_of(id).iter().map(|b| b.name).collect::<Vec<_>>();
        run.burn_brand(id, 0);
        run.burn_brand(id, 1);
        assert_eq!(names(&run), [BRANDS[1].name], "one socket at level 1");
        run.ups.push(id);
        run.burn_brand(id, 2);
        assert_eq!(names(&run), [BRANDS[1].name, BRANDS[2].name], "a level adds a socket");
        run.burn_brand(id, 3);
        assert_eq!(names(&run), [BRANDS[2].name, BRANDS[3].name], "full, the oldest goes");
    }

    #[test]
    fn a_shop_sells_one_lesson_and_one_brand() {
        let (mut run, _) = Run::new(1, 0, 0);
        run.held = pool(&HEROES[0].elements)[..1].to_vec();
        let id = run.rites()[0];
        let mut shop = run.stock(0, &[]);
        run.shards = 500;
        assert!(run.buy_lesson(&mut shop, id) && run.level(id) == 2);
        assert!(!run.buy_lesson(&mut shop, id), "one lesson a shop");
        run.restock(&mut shop);
        assert!(!shop.lesson, "a restock brings no new lesson");
        assert!(run.buy_brand(&mut shop, id) && run.brands_of(id).len() == 1 && shop.brand.is_none());
        assert!(matches!(run.leave_shop(&shop), Stage::Map));
    }

    #[test]
    fn a_run_starts_with_basics_alone_and_learns_up_to_four_rites() {
        let (mut run, _) = Run::new(1, 0, 0);
        assert!(run.rites().is_empty());
        let casts = run.casts();
        assert_eq!(casts.len(), 3);
        assert!(casts.iter().all(|&i| is_basic(&COMBOS[i])));
        let all = pool(&HEROES[0].elements);
        for &id in &all[..MAX_RITES] {
            run.learn(Some((id, None)));
        }
        assert_eq!(run.rites(), all[..MAX_RITES]);
        // full: a fifth is learnt only by letting one go, and its levels and brands go with it
        run.learn(Some((all[MAX_RITES], None)));
        assert_eq!(run.rites().len(), MAX_RITES);
        run.ups.push(all[0]);
        run.brands.push((all[0], 0));
        run.learn(Some((all[MAX_RITES], Some(all[0]))));
        assert!(run.rites().contains(&all[MAX_RITES]) && !run.rites().contains(&all[0]));
        assert!(run.ups.is_empty() && run.brands.is_empty());
        // offers never repeat a rite held
        for _ in 0..20 {
            assert!(run.rite_offer(3).iter().all(|i| !run.held.contains(i)));
        }
        // the shop's rite: room needed, else one let go
        let mut shop = run.stock(0, &[]);
        run.shards = 500;
        if let Some(id) = shop.rite {
            assert!(!run.buy_rite(&mut shop, None), "no room");
            assert!(run.buy_rite(&mut shop, Some(run.held[0])) && run.held.contains(&id) && shop.rite.is_none());
        }
    }

    #[test]
    fn every_floor_ends_in_shops_floor_one_elites_come_late_and_events_do_not_repeat() {
        for seed in 0..40 {
            let (mut run, _) = Run::new(seed, 0, 0);
            let cols = run.cols();
            assert!(run.map[cols - 1].iter().all(|n| n.kind == NodeKind::Shop), "seed {seed}: the rooms before the boss are shops");
            assert!(run.map[..3].iter().flatten().all(|n| n.kind != NodeKind::Elite), "seed {seed}: no elite in floor 1's first three rooms");
            let mut seen = vec![];
            for _ in 0..ENCOUNTERS.len() {
                run.pos = None;
                run.map[0][0].kind = NodeKind::Encounter;
                if let Stage::Event(e, _) = run.enter(0, 0) {
                    assert!(!seen.contains(&e.title), "seed {seed}: {} came twice", e.title);
                    seen.push(e.title);
                }
            }
        }
    }

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
        // over a few days every hero every save has takes a turn, and only those
        let heroes: std::collections::HashSet<usize> = (0..8).map(|d| daily(d).0).collect();
        assert_eq!(heroes.len(), FIRST_HEROES);
        // names repeat only after every pairing has had its day
        let names: std::collections::HashSet<String> = (0..120).map(daily_name).collect();
        assert_eq!(names.len(), 120);
    }

    #[test]
    fn what_a_deed_opens_comes_up_only_once_it_is_done() {
        let hook = CHARMS.iter().position(|c| c.name == "Scavenger's Hook").unwrap();
        let charge = BRANDS.iter().position(|b| b.name == "Charge").unwrap();
        let seen = |deeds: u32| {
            let (mut pact, mut brand) = (false, false);
            for seed in 0..300 {
                let (mut run, _) = Run::with_deeds(seed, 0, 0, deeds);
                pact |= run.offer(6, &[], false).contains(&hook);
                brand |= run.stock(0, &[]).brand == Some(charge);
            }
            (pact, brand)
        };
        assert_eq!(seen(0), (false, false));
        assert_eq!(seen(0b11), (true, true));
    }

    #[test]
    fn wild_hands_shows_a_screen_on_floor_one_and_on_floor_advance() {
        let (mut run, _) = Run::new(7, 0, 0);
        let Some(Stage::WildHands(first, true)) = run.arm(1 << WILD_HANDS) else {
            panic!("Wild Hands should return its floor-1 reveal stage");
        };
        assert_eq!(run.charms, vec![first]);
        let Stage::Pick(options) = run.after_wild_hands(true) else {
            panic!("dismissing floor-1 Wild Hands opens the initial pact pick");
        };
        assert_eq!(options.len(), 3);
        assert!(!options.contains(&first), "opening pick must not repeat Wild Hands' pact");
        run.add_charm(options[0]);
        // beating floor 1's boss advances to floor 2 and shows Wild Hands again before the map
        run.pos = Some((COLS, 0));
        let Stage::WildHands(second, false) = run.after_node() else {
            panic!("floor 2 should open with a Wild Hands reveal stage");
        };
        assert_ne!(second, first);
        assert!(run.charms.contains(&second));
        assert!(matches!(run.after_wild_hands(false), Stage::Map));
    }

    #[test]
    fn encounter_gain_charm_reports_gained_ids_and_empty_pick_shows_shard_notice() {
        let (mut run, _) = Run::new(5, 0, 0);
        run.charms = vec![0];
        let swap = ENCOUNTERS.iter().find(|e| e.title == "Pact Swapper").unwrap();
        let res = run.choose(&swap.choices[0], Some(0));
        assert_eq!(res.gained.len(), 2);
        assert!(res.gained.iter().all(|id| run.charms.contains(id) && *id != 0));
        // when every open pact is already held, pick() gives +20 shards with a resolved notice screen
        run.charms = (0..CHARMS.len()).filter(|&i| run.pact_open(i)).collect();
        let shards_before = run.shards;
        let Stage::Event(notice, Some(res)) = run.pick() else {
            panic!("empty pact pool should show a notice instead of skipping silently");
        };
        assert_eq!(notice.title, "No Pacts Left");
        assert!(res.gained.is_empty() && res.text.contains("20 shards"));
        assert_eq!(run.shards, shards_before + 20);
        assert!(matches!(run.after_node(), Stage::Map));
    }

    #[test]
    fn mediocre_and_cursed_pacts_follow_pool_rules_and_hp_clamp() {
        let mut saw_mediocre = false;
        let mut saw_helpful_cursed = false;
        let mut saw_burden_in_random = false;
        for seed in 0..400 {
            let (mut run, stage) = Run::new(seed, 0, 0);
            if let Stage::Pick(opts) = stage {
                for id in opts {
                    assert!(!CHARMS[id].burden, "Pick must never offer pure burden curses");
                    saw_mediocre |= CHARMS[id].rarity == Rarity::Mediocre;
                    saw_helpful_cursed |= CHARMS[id].rarity == Rarity::Cursed && !CHARMS[id].burden;
                }
            }
            let shop = run.stock(0, &[]);
            for id in shop.stock.into_iter().flatten() {
                assert!(!CHARMS[id].burden, "Shop must never stock pure burden curses");
            }
            if let Some(Stage::WildHands(id, _)) = run.arm(1 << WILD_HANDS) {
                saw_burden_in_random |= CHARMS[id].burden;
            }
        }
        assert!(saw_mediocre, "Mediocre pacts should appear in normal picks");
        assert!(saw_helpful_cursed, "Helpful cursed bargain pacts should appear in normal picks");
        assert!(saw_burden_in_random, "Pure cursed burdens should appear in random grants like Wild Hands");
        // adding a -max_hp curse never drops hp below 1
        let (mut run, _) = Run::new(1, 0, 0);
        let aegis = CHARMS.iter().position(|c| c.name == "Cracked Aegis").unwrap();
        run.hp = 5;
        run.add_charm(aegis);
        assert!(run.hp >= 1 && run.hp <= run.max_hp());
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
        assert_eq!(run.mods().power, 3, "Ashwarden's Ashen might grants +3 power");
    }

    #[test]
    fn hero_levels_up_from_battles_and_crucible_relic_applies() {
        let (mut run, _) = Run::new(7, 0, 0);
        // equip a Crucible relic: Keen (+2 power), Ironbound (+3 armor), Idol (foes +1 power)
        let relic = CrucibleRelic { boons: [1, 2], curse: 0 };
        run.equip_crucible(Some(relic));
        assert_eq!((run.mods().power, run.mods().armor), (3 + 2, 3 + 3));
        assert_eq!(run.random_normal().mods.power, 1, "Idol curse grants enemies +1 power");

        // win an elite (+30 XP >= 25 needed for Lv 2): queues LevelUp after elite reward
        run.map[1][0].kind = NodeKind::Elite;
        let Stage::Battle(mut b) = run.enter(1, 0) else { panic!("elite battle") };
        b.debug_win();
        let Stage::Learn(offers) = run.finish_battle(&b) else { panic!("elite teaches a rite") };
        assert_eq!((run.hero_lvl, run.pending_lvl, run.hero_xp), (2, 1, 5));
        let Stage::LevelUp = run.learn(Some((offers[0], None))) else { panic!("level-up choice follows") };
        // choose Deepen Trait: Ashwarden gains +1 power, +1 armor
        let next = run.level_up(true);
        assert!(matches!(next, Stage::Map));
        assert_eq!((run.trait_rank, run.pending_lvl), (1, 0));
        assert_eq!((run.mods().power, run.mods().armor), (3 + 2 + 1, 3 + 3 + 1));
        assert!(run.trait_desc().contains("+4 power") && run.trait_desc().contains("4 less"), "trait_desc reflects rank upgrades: {}", run.trait_desc());

        // another level-up choosing Vitality & Spoils adds max HP and shards
        run.pending_lvl = 1;
        run.hero_lvl += 1;
        let (hp0, sh0) = (run.max_hp(), run.shards);
        let next = run.level_up(false);
        assert!(matches!(next, Stage::Map));
        assert_eq!((run.max_hp(), run.shards), (hp0 + LEVEL_HP, sh0 + LEVEL_SHARDS));
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

    /// A rite with a free brand socket, if any has one.
    fn socket(run: &Run, pick: &mut Rng) -> usize {
        let free: Vec<usize> = run.rites().into_iter().filter(|&id| !run.sockets_full(id)).collect();
        let from = if free.is_empty() { run.rites() } else { free };
        from[pick.below(from.len())]
    }

    thread_local! {
        /// the deeds `play` starts its runs with (none, unless a probe says otherwise)
        static PLAY_DEEDS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
        /// per fight: (node kind, last foe faced, hero won, [loads, breaks, hero HP lost, turns])
        static PROBE: std::cell::RefCell<Vec<(usize, String, bool, [usize; 4])>> = const { std::cell::RefCell::new(Vec::new()) };
    }

    /// Plays every hero with no deeds and with all of them, and how runs holding each opened pact or
    /// brand did. `cargo test --release probe_deeds -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn probe_deeds() {
        const RUNS: u64 = 300;
        let mut ends: Vec<(Run, bool)> = vec![];
        for hero in 0..HEROES.len() {
            let mut line = format!("{:12}", HEROES[hero].name);
            for deeds in [0, u32::MAX] {
                PLAY_DEEDS.with(|d| d.set(deeds));
                let runs: Vec<(Run, bool)> = (0..RUNS).map(|s| play(s, hero, 0, &[])).collect();
                let wins = runs.iter().filter(|r| r.1 || r.0.floor >= FLOORS).count();
                let past: Vec<String> = (1..FLOORS).map(|f| runs.iter().filter(|r| r.1 || r.0.floor >= f).count().to_string()).collect();
                line += &format!("  deeds {:3}: wins {wins:3}/{RUNS}, past floors {}", if deeds == 0 { "no" } else { "all" }, past.join("/"));
                if deeds != 0 {
                    ends.extend(runs);
                }
            }
            println!("{line}");
        }
        PLAY_DEEDS.with(|d| d.set(0));
        let won = |r: &&(Run, bool)| r.1 || r.0.floor >= FLOORS;
        let all = ends.iter().filter(won).count() as f32 / ends.len() as f32;
        println!("all runs with every deed: win {:.0}%", 100.0 * all);
        for name in ["Scavenger's Hook", "War Drum", "Damper Coil", "Bone Pact"] {
            let id = CHARMS.iter().position(|c| c.name == name).unwrap();
            let held: Vec<&(Run, bool)> = ends.iter().filter(|r| r.0.charms.contains(&id)).collect();
            let w = held.iter().filter(|r| won(r)).count() as f32 / held.len().max(1) as f32;
            println!("pact  {name:17} held in {:4} runs, those win {:.0}%", held.len(), 100.0 * w);
        }
        for name in ["Charge", "Dazzle", "Siphon"] {
            let id = BRANDS.iter().position(|b| b.name == name).unwrap();
            let held: Vec<&(Run, bool)> = ends.iter().filter(|r| r.0.brands.iter().any(|b| b.1 == id)).collect();
            let w = held.iter().filter(|r| won(r)).count() as f32 / held.len().max(1) as f32;
            println!("brand {name:17} held in {:4} runs, those win {:.0}%", held.len(), 100.0 * w);
        }
        // the old ones, for scale
        for name in ["Edge", "Ward", "Tempo"] {
            let id = BRANDS.iter().position(|b| b.name == name).unwrap();
            let held: Vec<&(Run, bool)> = ends.iter().filter(|r| r.0.brands.iter().any(|b| b.1 == id)).collect();
            let w = held.iter().filter(|r| won(r)).count() as f32 / held.len().max(1) as f32;
            println!("brand {name:17} held in {:4} runs, those win {:.0}%  (old)", held.len(), 100.0 * w);
        }
        for name in ["Berserker's Axe", "Chain Lightning", "Overcharger"] {
            let id = CHARMS.iter().position(|c| c.name == name).unwrap();
            let held: Vec<&(Run, bool)> = ends.iter().filter(|r| r.0.charms.contains(&id)).collect();
            let w = held.iter().filter(|r| won(r)).count() as f32 / held.len().max(1) as f32;
            println!("pact  {name:17} held in {:4} runs, those win {:.0}%  (old)", held.len(), 100.0 * w);
        }
    }

    /// Prints how much enemies get done per fight kind. `cargo test --release probe_enemy -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn probe_enemy_pressure() {
        for seed in 0..150 {
            for hero in 0..HEROES.len() {
                play(seed, hero, 0, &[]);
            }
        }
        PROBE.with(|p| {
            let p = p.borrow();
            for (kind, name) in [(0, "normal"), (1, "elite"), (5, "boss"), (6, "shrine")] {
                let rows: Vec<&[usize; 4]> = p.iter().filter(|r| r.0 == kind).map(|r| &r.3).collect();
                let sum = |k: usize| rows.iter().map(|r| r[k]).sum::<usize>() as f32;
                let n = rows.len().max(1) as f32;
                println!("{name:7} fights {:5}  loads/fight {:.2}  broken {:.0}%  hp lost/fight {:.1}  turns {:.1}", rows.len(), sum(0) / n, 100.0 * sum(1) / sum(0).max(1.0), sum(2) / n, sum(3) / n);
            }
            // each elite and boss on its own, by the foe the fight ended on
            let mut names: Vec<(usize, &str)> = p.iter().filter(|r| r.0 == 1 || r.0 == 5).map(|r| (r.0, r.1.as_str())).collect();
            names.sort();
            names.dedup();
            for (kind, who) in names {
                let rows: Vec<_> = p.iter().filter(|r| r.0 == kind && r.1 == who).collect();
                let n = rows.len() as f32;
                let won = rows.iter().filter(|r| r.2).count() as f32;
                let avg = |k: usize| rows.iter().map(|r| r.3[k]).sum::<usize>() as f32 / n;
                println!("{:5} {who:20} fights {:4}  hero wins {:3.0}%  hp lost {:5.1}  turns {:4.1}", if kind == 1 { "elite" } else { "boss" }, rows.len(), 100.0 * won / n, avg(2), avg(3));
            }
        });
    }

    /// Plays a whole run, carrying `keys` from the start. Returns the run as it ended, and whether it won.
    fn play(seed: u64, hero: usize, exp: usize, keys: &[usize]) -> (Run, bool) {
        let (mut run, mut stage) = Run::with_deeds(seed, hero, exp, PLAY_DEEDS.with(|d| d.get()));
        run.arm(0);
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
                    let hp0 = run.hp;
                    while !b.over() {
                        b.ai_step();
                        steps += 1;
                        assert!(
                            steps < 5000,
                            "battle never ends: seed {seed} hero {hero} turn {} active {} foe {} hp [{}/{}] shd [{}/{}] stacks [{}/{}] rev [{}/{}] evades [{}/{}]",
                            b.turn, b.active, b.f[1].name, b.f[0].hp, b.f[1].hp, b.f[0].shield, b.f[1].shield,
                            b.f[0].stacks.len(), b.f[1].stacks.len(), b.f[0].reveals_left, b.f[1].reveals_left,
                            b.f[0].evades_left, b.f[1].evades_left
                        );
                    }
                    let kind = run.pos.map_or(0, |(c, r)| run.map[c][r].kind as usize);
                    let count = |f: fn(&crate::battle::Event) -> bool| b.events.iter().filter(|e| f(e)).count();
                    let row = [count(|e| matches!(e, crate::battle::Event::Load)), count(|e| matches!(e, crate::battle::Event::Break)), (hp0 - b.f[0].hp).max(0) as usize, b.turn];
                    PROBE.with(|p| p.borrow_mut().push((kind, b.f[1].name.to_string(), b.won(), row)));
                    run.finish_battle(&b)
                }
                Stage::Pick(options) => {
                    // the pact that builds most on a set already held
                    let id = *options.iter().rev().max_by_key(|&&id| run.set_count(CHARMS[id].set as usize)).expect("a pick offers pacts");
                    run.add_charm(id);
                    run.after_node()
                }
                Stage::WildHands(_, then_pick) => run.after_wild_hands(then_pick),
                Stage::Upgrade(ids) => run.upgrade(ids[pick.below(ids.len())]),
                Stage::Blessing(offers) => run.take_blessing(Some(offers[pick.below(offers.len())])),
                Stage::Brand(offers) => {
                    let id = socket(&run, &mut pick);
                    run.burn_brand(id, offers[pick.below(offers.len())])
                }
                // a new rite while there is room, else the weakest-looking swap: keep what it has
                Stage::Learn(offers) => {
                    let take = (run.held.len() < MAX_RITES).then(|| (offers[pick.below(offers.len())], None));
                    run.learn(take)
                }
                Stage::LevelUp => run.level_up(pick.below(2) == 0),
                Stage::Shop(mut shop) => {
                    if run.held.len() < MAX_RITES {
                        run.buy_rite(&mut shop, None);
                    }
                    let ids: Vec<usize> = run.rites().into_iter().filter(|&id| run.level(id) < MAX_LEVEL).collect();
                    if !ids.is_empty() {
                        run.buy_lesson(&mut shop, ids[pick.below(ids.len())]);
                    }
                    if !run.held.is_empty() {
                        let id = socket(&run, &mut pick);
                        run.buy_brand(&mut shop, id);
                    }
                    for k in 0..shop.stock.len() {
                        run.buy(&mut shop, k);
                    }
                    run.leave_shop(&shop)
                }
                Stage::Event(_, Some(_)) => run.after_node(),
                Stage::Event(e, None) => {
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
        let Stage::Learn(offers) = run.finish_battle(&b) else { panic!("an elite teaches a rite") };
        let Stage::Event(offer, None) = run.learn(Some((offers[0], None))) else { panic!("the seal is offered") };
        run.choose(&offer.choices[0], None);
        assert!(matches!(run.after_node(), Stage::LevelUp), "elite XP triggers level-up");
        assert!(matches!(run.level_up(true), Stage::Map));
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
        assert!(run.keys.len() == 2 && !walk_past.text.is_empty(), "leaving it leaves it");
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
                Stage::Learn(offers) => run.learn(Some((offers[0], None))),
                Stage::Pick(_) => run.after_node(),
                Stage::LevelUp => run.level_up(true),
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
                Stage::Learn(offers) => run.learn(Some((offers[0], None))),
                Stage::Pick(_) => run.after_node(),
                Stage::LevelUp => run.level_up(true),
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
        // the deepest expeditions sit near zero for the bot, where a run or two either way is noise
        assert!(scores.windows(2).all(|w| w[1] <= w[0] + RUNS as usize / 50), "{scores:?}");
        assert!(scores[EXPEDITIONS.len() - 1] < scores[0] * 2 / 3, "the last expedition must be much harder: {scores:?}");
    }
}
