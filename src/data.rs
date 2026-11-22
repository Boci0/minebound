//! Content tables. Text is ASCII only: the built-in font has no other glyphs.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Element {
    Ember,
    Tide,
    Gale,
    Stone,
    Spark,
}
use Element::*;

/// Every passive in the game (hero traits, charms, enemy rules) is a tweak to these numbers.
#[derive(Clone, Default, Debug)]
pub struct Mods {
    pub reveals: i32,
    pub power: i32,
    pub mine_guard: i32,
    pub start_shield: i32,
    pub combo_pct: i32,
    pub duds: i32,
    pub heal_after: i32,
    pub shard_pct: i32,
    pub start_stacks: i32,
    pub burn_bonus: i32,
    pub scouts: i32,
    pub thorns: i32,
    pub foe_mines: i32,
    pub fewer_mines: i32,
    pub lifesteal_pct: i32,
    pub opening: i32,
    pub regen: i32,
    pub shield_turn: i32,
    pub max_hp: i32,
    // surge
    pub surge_start: i32,
    pub surge_pct: i32,
    pub keep_surge: i32,
    /// the opponent's surge cannot pass this (0 = no limit)
    pub foe_surge_cap: i32,
    /// every rite takes this many more gems, of any element
    pub toll: i32,
    // skills
    pub rite_bonus: i32,
    pub rite_burn: i32,
    pub first_pct: i32,
    /// each point of shield stops this many more damage
    pub shield_block: i32,
    /// shield no longer wears off between turns
    pub keep_shield: i32,
    // crystals and mines
    pub claim_zap: i32,
    pub claim_heal: i32,
    pub claim_shield: i32,
    pub claim_bonus: i32,
    pub salvage: i32,
    pub mine_spite: i32,
    /// stacks gained whenever the opponent hits a mine
    pub foe_mine_gift: i32,
    // defence and tempo
    pub armor: i32,
    pub evade: i32,
    /// evaded rites still hit, for half damage, effects and all
    pub graze: i32,
    pub pilfer: i32,
    pub enrage: i32,
    /// +1 power every second turn (the deeper descents' bosses)
    pub slow_enrage: i32,
    // charms that change how a turn plays
    pub reveal_shield: i32,
    pub hoard_shield: i32,
    pub chain: i32,
    /// surge gained when it sets off a mine
    pub mine_surge: i32,
    pub sense: i32,
    pub echo: i32,
    pub desperate: i32,
    /// every rite's damage, in percent more
    pub rite_pct: i32,
    /// burn on yourself as each wave starts
    pub self_burn: i32,
    /// how far past `MAX_SURGE` your surge can climb
    pub surge_room: i32,
    /// the foe's loaded rites break with this many percent less damage
    pub easy_break: i32,
}

pub type Tweak = fn(&mut Mods);

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Effect {
    Damage(i32),
    Burn(i32),
    Shield(i32),
    Heal(i32),
    Weaken(i32),
    Drain(usize),
    Pierce,
    /// more damage for each surge step the caster holds
    PerSurge(i32),
    /// all the burn left on the foe goes off at once
    Detonate,
    /// extra reveals this turn
    Reveal(i32),
    /// breaks crystals on the foe's board
    Shatter(usize),
    /// plants hidden mines in the foe's board
    Bury(usize),
    /// surge steps gained after the cast
    Surge(i32),
    /// more damage for each reveal the caster used this turn
    PerReveal(i32),
    /// the caster's shield doubles, gaining at most this much
    DoubleShield(i32),
    /// the caster's shield joins the hit at this percent (no surge), then is gone
    ShieldBlast(i32),
    /// the caster's shield turns brittle: the next rite that hits it is blocked, then all of it shatters onto the attacker
    Brittle,
    /// more damage for each burn on the foe as the rite lands
    PerBurn(i32),
    /// more damage for each quarter of its HP the caster has lost (up to 3)
    PerQuarterLost(i32),
    /// the caster pays HP, never below 1
    SelfDamage(i32),
    // masteries: what a rite gains at level 3, one of its own each
    /// the caster's next this many mines are duds
    Duds(i32),
    /// the caster heals this percent of the damage it deals
    Lifesteal(i32),
    /// cannot miss and ignores armor
    Sure,
    /// spends no surge step
    KeepSurge,
    /// defuses this many mines on each of the caster's boards
    Scout(usize),
    /// the caster's shield does not wear off at its next turn
    Hold,
    /// takes this many of the foe's stacks for the caster
    Steal(usize),
    /// the foe has this many fewer reveals next turn
    Blind(i32),
    /// more damage for each foe crystal the rite breaks
    PerShatter(i32),
    /// double damage on a foe at half HP or below
    Execute,
    /// reveals left unused this turn count for `PerReveal` too
    Unused,
    /// the shield thrown by `ShieldBlast` is only half spent
    KeepHalf,
    /// the caster gains this much power for the rest of the battle
    Stoke(i32),
    /// clears the caster's burn and weaken
    Cleanse,
    /// the foe loses every stack
    DrainAll,
    /// the foe's burn stops halving for the rest of the battle
    Simmer,
    // the forge's rites
    /// more damage when it is the caster's first rite this turn
    Opener(i32),
    /// the caster gains this many thorns for the rest of the battle
    Spikes(i32),
}
use Effect::*;

pub struct Combo {
    pub name: &'static str,
    /// names at level 2 and 3: a boss kill upgrades one skill
    pub up: [&'static str; 2],
    pub needs: &'static [Element],
    pub effects: &'static [Effect],
    /// what it gains once mastered (level 3), on top of its grown effects
    pub master: Option<Effect>,
}

/// Highest level a skill can reach.
pub const MAX_LEVEL: usize = 3;
/// How much shield doubling may add at `lvl`: the cap, twice it at level 2, and no cap once mastered.
pub fn double_cap(cap: i32, lvl: usize) -> Option<i32> {
    (lvl < MAX_LEVEL).then_some(cap * lvl as i32)
}

/// An effect's number at skill level `lvl`: +50% per level, and always at least +1 per level
/// (so a 1 still grows: 1, 2, 3).
pub fn at_level(v: i32, lvl: usize) -> i32 {
    (v * (lvl as i32 + 1) / 2).max(v + lvl as i32 - 1)
}

impl Combo {
    pub fn title(&self, lvl: usize) -> &'static str {
        if lvl <= 1 { self.name } else { self.up[(lvl - 2).min(1)] }
    }

    /// Gems it takes at `lvl`: one per element, plus one more of its own elements, in turn, per upgrade.
    pub fn cost(&self, lvl: usize) -> Vec<Element> {
        let n = self.needs.len();
        self.needs.iter().copied().chain((0..lvl.saturating_sub(1)).map(|i| self.needs[i % n])).collect()
    }
}

/// What a rite does at `lvl`, with the brand burnt onto it, if any. A brand stays at level 1.
pub fn rite_text(c: &Combo, lvl: usize, brands: &[&Brand]) -> String {
    let master = c.master.filter(|_| lvl >= MAX_LEVEL);
    let parts: Vec<String> = c.effects.iter().map(|&e| e.text(lvl)).chain(master.map(|e| e.text(lvl))).chain(brands.iter().map(|b| b.effect.text(1))).collect();
    let s = parts.join(", ");
    s[..1].to_uppercase() + &s[1..]
}

impl Effect {
    /// True if a rite with this effect deals damage. The player's rites without one only do what they say.
    pub fn hits(self) -> bool {
        matches!(self, Damage(_) | PerSurge(_) | Detonate | PerReveal(_) | ShieldBlast(_) | PerBurn(_) | PerQuarterLost(_) | Opener(_))
    }

    pub fn text(self, lvl: usize) -> String {
        match self {
            Damage(v) => format!("+{} damage", at_level(v, lvl)),
            Burn(n) => format!("+{} burn on foe", at_level(n, lvl)),
            Shield(v) => format!("gain {} shield", at_level(v, lvl)),
            Heal(v) => format!("heal {}", at_level(v, lvl)),
            Weaken(n) => {
                let k = at_level(n, lvl);
                format!("halve foe's hits, {k} turn{}", if k == 1 { "" } else { "s" })
            }
            Drain(n) => {
                let k = at_level(n as i32, lvl);
                format!("foe loses {k} gem{}", if k == 1 { "" } else { "s" })
            }
            Pierce => "shatter foe's shield".into(),
            PerSurge(v) => format!("+{} damage per surge", at_level(v, lvl)),
            Detonate => "detonate foe's burn".into(),
            Reveal(n) => {
                let k = at_level(n, lvl);
                format!("+{k} pick{}", if k == 1 { "" } else { "s" })
            }
            Shatter(n) => {
                let k = at_level(n as i32, lvl);
                format!("break {k} foe crystal{}", if k == 1 { "" } else { "s" })
            }
            Bury(n) => {
                let k = at_level(n as i32, lvl);
                format!("crack {k} foe tile{}", if k == 1 { "" } else { "s" })
            }
            Surge(v) => format!("+{} surge", at_level(v, lvl)),
            PerReveal(v) => format!("+{} damage per pick used", at_level(v, lvl)),
            DoubleShield(cap) => match double_cap(cap, lvl) {
                Some(c) => format!("double your shield (up to +{c})"),
                None => "double your shield, no limit".into(),
            },
            ShieldBlast(p) => match at_level(p, lvl) {
                100 => "all your shield becomes damage".into(),
                q => format!("your shield becomes {q}% damage"),
            },
            Brittle => "it turns brittle".into(),
            PerBurn(v) => format!("+{} damage per burn on foe", at_level(v, lvl)),
            PerQuarterLost(v) => format!("+{} damage per quarter of HP lost", at_level(v, lvl)),
            SelfDamage(v) => format!("you lose {} HP", at_level(v, lvl)),
            Duds(n) => format!("your next {n} cave-ins do no harm"),
            Lifesteal(p) => format!("heal {p}% of its damage"),
            Sure => "can't miss, ignores armor".into(),
            KeepSurge => "spends no surge".into(),
            Scout(n) => format!("prop up {n} tiles on each board"),
            Hold => "shield won't wear off next turn".into(),
            Steal(n) => format!("steal {n} foe gems"),
            Blind(n) => format!("foe gets {n} fewer picks"),
            PerShatter(v) => format!("+{v} damage per crystal broken"),
            Execute => "x2 damage if foe is at half HP".into(),
            Unused => "unused picks count too".into(),
            KeepHalf => "keep half your shield".into(),
            Stoke(n) => format!("+{n} power this battle"),
            Opener(v) => format!("+{} damage as your first rite this turn", at_level(v, lvl)),
            Spikes(n) => format!("+{} thorns this battle", at_level(n, lvl)),
            Cleanse => "clear your burn and weaken".into(),
            DrainAll => "foe loses all its gems".into(),
            Simmer => "foe's burn stops fading".into(),
        }
    }
}

/// Won at a shrine fight. Only one is held at a time: taking another lets the old one go.
/// Each lifts a single stat.
pub struct Blessing {
    pub name: &'static str,
    pub desc: &'static str,
    pub apply: Tweak,
}

const fn blessing(name: &'static str, desc: &'static str, apply: Tweak) -> Blessing {
    Blessing { name, desc, apply }
}

pub static BLESSINGS: &[Blessing] = &[
    blessing("Blade", "Your rites deal +6 damage.", |m| m.rite_bonus += 6),
    blessing("Might", "+2 power: every gem a rite spends hits 2 harder.", |m| m.power += 2),
    blessing("Bulwark", "Gain 6 shield at the start of each of your turns.", |m| m.shield_turn += 6),
    blessing("Iron", "Every hit on you deals 3 less.", |m| m.armor += 3),
    blessing("Mending", "Heal 4 at the start of each of your turns.", |m| m.regen += 4),
    blessing("Cinders", "Your burns deal +3 damage each turn.", |m| m.burn_bonus += 3),
    blessing("Leech", "Heal for 30% of the damage your rites deal.", |m| m.lifesteal_pct += 30),
    blessing("Vigor", "+30 max HP.", |m| m.max_hp += 30),
    blessing("Sight", "+1 pick each turn.", |m| m.reveals += 1),
    blessing("Surge", "Begin each turn with 4 surge.", |m| m.surge_start += 4),
];

/// Burnt onto a rite, a brand adds its effect. It costs no extra gem and does not grow with the rite.
/// A rite holds one brand per level.
pub struct Brand {
    pub name: &'static str,
    pub effect: Effect,
}

const fn brand(name: &'static str, effect: Effect) -> Brand {
    Brand { name, effect }
}

pub static BRANDS: &[Brand] = &[
    brand("Edge", Damage(3)),
    brand("Scorch", Burn(2)),
    brand("Ward", Shield(5)),
    brand("Mend", Heal(4)),
    brand("Hex", Weaken(1)),
    brand("Pilfer", Drain(1)),
    brand("Breach", Pierce),
    brand("Tempo", PerSurge(1)),
    brand("Fuse", Detonate),
    brand("Insight", Reveal(1)),
    brand("Fracture", Shatter(1)),
    brand("Snare", Bury(1)),
    // opened by deeds
    brand("Charge", Surge(2)),
    brand("Dazzle", Blind(1)),
    brand("Siphon", Lifesteal(25)),
];

const fn combo(name: &'static str, up: [&'static str; 2], needs: &'static [Element], effects: &'static [Effect], master: Option<Effect>) -> Combo {
    Combo { name, up, needs, effects, master }
}

pub static COMBOS: &[Combo] = &[
    combo("Steam Burst", ["Scald Geyser", "Steamquake"], &[Ember, Tide], &[Damage(4), Reveal(1)], Some(Duds(3))),
    combo("Raze", ["Bloodrush", "Rampage"], &[Ember, Gale], &[Damage(2), PerQuarterLost(2)], Some(Lifesteal(50))),
    combo("Magma", ["Lava Lance", "Molten Core"], &[Ember, Stone], &[Pierce, Damage(3)], Some(Sure)),
    combo("Plasma", ["Plasma Lance", "Plasma Nova"], &[Ember, Spark], &[Damage(3), PerSurge(1)], Some(KeepSurge)),
    combo("Mistveil", ["Mist Shroud", "Veil of Mists"], &[Tide, Gale], &[Heal(8), Reveal(2)], Some(Scout(3))),
    combo("Bulwark", ["Rampart", "Citadel"], &[Tide, Stone], &[Shield(12)], Some(Hold)),
    combo("Conduction", ["Overload", "Siphon Storm"], &[Tide, Spark], &[Damage(3), Surge(4)], Some(Steal(2))),
    combo("Sandstorm", ["Dust Devil", "Desert Wrath"], &[Gale, Stone], &[Weaken(2), Bury(2)], Some(Blind(2))),
    combo("Thunderclap", ["Thunderstrike", "Skybreaker"], &[Gale, Spark], &[Shatter(1), Damage(2)], Some(PerShatter(8))),
    combo("Lodestone", ["Ironheart", "Magnetar"], &[Stone, Spark], &[DoubleShield(12), Brittle], None),
    combo("Upheaval", ["Cataclysm", "Worldbreaker"], &[Ember, Gale, Stone], &[SelfDamage(4), Damage(11)], Some(Execute)),
    combo("Tempest", ["Hurricane", "Eye of the Storm"], &[Tide, Gale, Spark], &[PerReveal(1)], Some(Unused)),
    combo("Geode Surge", ["Crystal Tide", "Geode Titan"], &[Tide, Stone, Spark], &[Damage(6), ShieldBlast(150)], Some(KeepHalf)),
    combo("Forgefire", ["Starforge", "Sunforge"], &[Ember, Stone, Spark], &[Damage(8)], Some(Stoke(2))),
    combo("Maelstrom", ["Abyssal Gyre", "Leviathan"], &[Tide, Gale, Stone], &[Shield(6), Heal(4)], Some(Cleanse)),
    combo("Short Circuit", ["Arc Flash", "Blackout"], &[Ember, Gale, Spark], &[Damage(6), Drain(1)], Some(DrainAll)),
    combo("Boilover", ["Scalding Flood", "Hellbroth"], &[Ember, Tide, Spark], &[Burn(3), PerBurn(1)], Some(Simmer)),
    // bought at the forge with ore: each takes the place of the base rite with the same elements
    // bought at the forge with ore: each takes the place of the base rite with the same elements, and
    // each asks for one way of playing the pair that its base rite never does
    combo("Scald Rain", ["Boiling Rain", "Kettle Storm"], &[Ember, Tide], &[Damage(2), Burn(5)], Some(Detonate)),
    combo("Kindle", ["Stoke the Coals", "Undying Flame"], &[Ember, Gale], &[Stoke(2), Shield(4)], Some(Stoke(1))),
    combo("Blood Pyre", ["Bloodfire", "Last Rites"], &[Ember, Stone], &[SelfDamage(4), Damage(9)], Some(Lifesteal(50))),
    combo("Spark Gap", ["Arc Gap", "First Light"], &[Ember, Spark], &[Opener(7)], Some(Sure)),
    combo("Undertow", ["Riptide", "Drowning Deep"], &[Tide, Gale], &[Damage(3), Blind(1)], Some(DrainAll)),
    combo("Tidal Ram", ["Breaker Wave", "Tsunami"], &[Tide, Stone], &[ShieldBlast(200)], Some(KeepHalf)),
    combo("Capacitor", ["Leyden Jar", "Storm Cell"], &[Tide, Spark], &[Surge(8)], Some(Duds(2))),
    combo("Rockslide", ["Avalanche", "Mountainfall"], &[Gale, Stone], &[Damage(2), Bury(3)], Some(Shatter(2))),
    combo("Ball Lightning", ["Chain Lightning", "Storm Crown"], &[Gale, Spark], &[PerReveal(1)], Some(Unused)),
    combo("Iron Thorns", ["Barbed Hide", "Iron Maiden"], &[Stone, Spark], &[Shield(4), Spikes(1)], Some(Hold)),
];

/// The rites every fighter starts with; the rest of `COMBOS` are the forge's, swapped in one for one.
pub const BASE_COMBOS: usize = 17;
/// What a forge rite brings besides itself: a keystone, on all the time while it is swapped in, that
/// turns its bearer toward one way of playing. In `COMBOS` order past `BASE_COMBOS`.
pub struct Keystone {
    /// the way of playing it opens, as its badge in battle says it
    pub style: &'static str,
    pub text: &'static str,
    /// how the style plays, shown on hover in the forge
    pub hint: &'static str,
    /// the `Mods` field it changes; its badge flashes when battle reports it
    pub hook: &'static str,
    pub apply: Tweak,
}

pub static KEYSTONES: &[Keystone] = &[
    Keystone { style: "Burn", text: "Burn on the foe deals 2 more each turn.", hint: "Burn: stack burn and let it eat them. Mastered, Scald Rain sets it all off.", hook: "burn_bonus", apply: |m| m.burn_bonus += 2 },
    Keystone { style: "Ramp", text: "+1 power every turn.", hint: "Ramp: weak at first, stronger every turn the fight lasts. Cast Kindle early.", hook: "enrage", apply: |m| m.enrage += 1 },
    Keystone { style: "Blood", text: "Below half HP: +3 power.", hint: "Blood: pay HP to hit harder, and hit hardest when nearly dead.", hook: "desperate", apply: |m| m.desperate += 3 },
    Keystone { style: "Alpha", text: "Your first rite each turn deals 20% more.", hint: "Alpha: one huge opening blow each turn, Spark Gap best of all.", hook: "first_pct", apply: |m| m.first_pct += 20 },
    Keystone { style: "Breaker", text: "Their loaded rites break with a third less damage.", hint: "Breaker: break every load they make, so their turns land at half.", hook: "easy_break", apply: |m| m.easy_break += 34 },
    Keystone { style: "Wall", text: "Every crystal you mine gives 1 shield.", hint: "Wall: mine to build shield, then throw it all with Tidal Ram.", hook: "claim_shield", apply: |m| m.claim_shield += 1 },
    Keystone { style: "Battery", text: "Surge carries over between turns and can reach 15.", hint: "Battery: bank surge over quiet turns, then spend it all in one.", hook: "keep_surge", apply: |m| {
        m.keep_surge += 1;
        m.surge_room += 5
    } },
    Keystone { style: "Quarry", text: "The foe's boards start with 1 more fault.", hint: "Quarry: wreck their boards so their own picks cave in on them.", hook: "foe_mines", apply: |m| m.foe_mines += 1 },
    Keystone { style: "Digger", text: "+1 pick each turn, but 2 less power.", hint: "Digger: mine more, hit softer, and cast Ball Lightning after every pick is used.", hook: "reveals", apply: |m| {
        m.reveals += 1;
        m.power -= 2
    } },
    Keystone { style: "Thorns", text: "Start each battle with 2 thorns.", hint: "Thorns: let them hit you; every rite they cast hurts them.", hook: "thorns", apply: |m| m.thorns += 2 },
];

/// Ore for one forge rite.
pub const RITE_PRICE: u32 = 60;

/// A wager, bought once at the forge with ore and then switched on or off before a descent: each
/// changes every enemy (or the run) and pays more ore at the end.
pub struct Wager {
    pub name: &'static str,
    pub desc: &'static str,
    pub price: u32,
    /// more ore at the end of a run, in percent
    pub ore_pct: u32,
    /// enemies' HP, in percent
    pub hp_pct: i32,
    pub foe: Tweak,
    /// its sigil, as a sprite index
    pub icon: usize,
}

/// Index of the wager that hands every enemy a random pact, and the one that hands you one each floor.
pub const WILD_GIFTS: usize = 4;
pub const WILD_HANDS: usize = 5;

pub static WAGERS: &[Wager] = &[
    Wager { name: "Iron Hides", desc: "Enemies have 30% more HP.", price: 40, ore_pct: 25, hp_pct: 130, foe: |_| {}, icon: HD + 88 },
    Wager { name: "Bloodlust", desc: "Enemies gain 1 power every second turn.", price: 40, ore_pct: 25, hp_pct: 100, foe: |m| m.slow_enrage += 1, icon: HD + 47 },
    Wager { name: "Brambles", desc: "Enemies have 2 thorns.", price: 40, ore_pct: 20, hp_pct: 100, foe: |m| m.thorns += 2, icon: HD + 9 },
    Wager { name: "Glass Foes", desc: "Enemies have half HP but 3 more power.", price: 50, ore_pct: 15, hp_pct: 50, foe: |m| m.power += 3, icon: HD + 53 },
    Wager { name: "Wild Gifts", desc: "Every enemy holds a random pact.", price: 60, ore_pct: 20, hp_pct: 100, foe: |_| {}, icon: HD + 25 },
    Wager { name: "Wild Hands", desc: "A random pact for you on every floor.", price: 80, ore_pct: 0, hp_pct: 100, foe: |_| {}, icon: HD + 45 },
];

/// Position of `c` in `COMBOS`.
pub fn combo_id(c: &Combo) -> usize {
    COMBOS.iter().position(|x| std::ptr::eq(x, c)).expect("combos come from COMBOS")
}

/// Only the combos a fighter can actually make with its elements.
pub fn combos_for(elements: &[Element]) -> Vec<&'static Combo> {
    COMBOS[..BASE_COMBOS].iter().filter(|c| c.needs.iter().all(|e| elements.contains(e))).collect()
}

pub struct Hero {
    pub name: &'static str,
    /// tile in the Tiny Dungeon sprite sheet
    pub sprite: usize,
    pub hp: i32,
    pub power: i32,
    pub elements: [Element; 3],
    pub passive: Tweak,
    pub passive_desc: &'static str,
}

pub static HEROES: &[Hero] = &[
    Hero {
        name: "Ashwarden",
        sprite: HD,
        hp: 100,
        power: 7,
        elements: [Ember, Gale, Stone],
        passive: |m| {
            m.mine_guard += 1;
            m.armor += 3;
        },
        passive_desc: "Ashen hide: cave-ins deal 1 less, every hit on you 3 less.",
    },
    Hero {
        name: "Stormreader",
        sprite: HD + 1,
        hp: 94,
        power: 7,
        elements: [Tide, Gale, Spark],
        passive: |m| {
            m.reveals += 1;
            m.reveal_shield += 3;
        },
        passive_desc: "Quick read: +1 pick a turn; each unused pick becomes 3 shield.",
    },
    Hero {
        name: "Deepforger",
        sprite: HD + 2,
        hp: 96,
        power: 6,
        elements: [Tide, Stone, Spark],
        passive: |m| {
            m.shield_block += 1;
            m.keep_shield += 1;
        },
        passive_desc: "Plated: shield never wears off and stops 2 damage a point.",
    },
    Hero {
        name: "Scaldwright",
        sprite: HD + 66,
        hp: 96,
        power: 6,
        elements: [Ember, Tide, Spark],
        passive: |m| {
            m.rite_burn += 1;
            m.lifesteal_pct += 15;
        },
        passive_desc: "Searing touch: rites add 1 burn and heal 15% of their damage.",
    },
    // opened by a deed (see `save::DEEDS`)
    Hero {
        name: "Galvanist",
        sprite: HD + 101,
        hp: 92,
        power: 7,
        elements: [Ember, Stone, Spark],
        passive: |m| {
            m.surge_pct += 5;
            m.mine_surge += 2;
        },
        passive_desc: "Live wire: each surge step is worth +15%, and every cave-in on your boards gives 2 surge.",
    },
];

/// The heroes every save starts with; the rest are opened by deeds.
pub const FIRST_HEROES: usize = 4;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Rarity {
    Common,
    Rare,
    Legendary,
}

impl Rarity {
    pub fn name(self) -> &'static str {
        ["Common", "Rare", "Legendary"][self as usize]
    }
    pub fn price(self) -> i32 {
        [30, 45, 65][self as usize]
    }
    /// how often it is offered, relative to the others
    pub fn weight(self) -> usize {
        [6, 3, 1][self as usize]
    }
}
use Rarity::*;

/// The set a pact belongs to. Holding `SET_AT` pacts of one set adds its bonus.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Set {
    Guard,
    Blade,
    Delver,
    Surge,
    Blood,
    Cinder,
}

/// Pacts of one set it takes to wake its bonus.
pub const SET_AT: usize = 3;

pub struct SetDef {
    pub name: &'static str,
    pub bonus: &'static str,
    pub apply: Tweak,
}

/// In the order of `Set`.
pub static SETS: &[SetDef] = &[
    SetDef { name: "Guard", bonus: "Every hit on you deals 2 less; gain 5 shield each turn.", apply: |m| {
        m.armor += 2;
        m.shield_turn += 5
    } },
    SetDef { name: "Blade", bonus: "+1 power: every gem a rite spends hits 1 harder.", apply: |m| m.power += 1 },
    SetDef { name: "Delver", bonus: "+1 pick each turn.", apply: |m| m.reveals += 1 },
    SetDef { name: "Surge", bonus: "Begin each turn with 5 surge.", apply: |m| m.surge_start += 5 },
    SetDef { name: "Blood", bonus: "Heal 5 at the start of each of your turns.", apply: |m| m.regen += 5 },
    SetDef { name: "Cinder", bonus: "Your rites add 1 burn, and burns deal +2.", apply: |m| {
        m.rite_burn += 1;
        m.burn_bonus += 2
    } },
];

impl Set {
    pub fn def(self) -> &'static SetDef {
        &SETS[self as usize]
    }
}

pub struct Charm {
    pub name: &'static str,
    pub rarity: Rarity,
    pub set: Set,
    /// sprite sheet tile and its tint (0xffffff = as drawn)
    pub icon: (usize, u32),
    /// the `Mods` field it changes; battle reports it by this name each time it takes effect
    pub hook: &'static str,
    pub desc: &'static str,
    pub apply: Tweak,
}

const fn charm(name: &'static str, rarity: Rarity, set: Set, icon: (usize, u32), hook: &'static str, desc: &'static str, apply: Tweak) -> Charm {
    Charm { name, rarity, set, icon, hook, desc, apply }
}

const PLAIN: u32 = 0xffffff;

pub static CHARMS: &[Charm] = &[
    charm("Lucky Coin", Common, Set::Delver, (HD + 28, PLAIN), "duds", "The first cave-in on your boards each wave does no harm.", |m| m.duds += 1),
    charm("Dowsing Rod", Common, Set::Delver, (HD + 29, PLAIN), "scouts", "Every board you are dealt has 3 cracks propped up.", |m| m.scouts += 3),
    charm("Ward Stone", Common, Set::Guard, (HD + 30, PLAIN), "start_shield", "Gain 10 shield at the start of each wave.", |m| m.start_shield += 10),
    charm("Field Rations", Common, Set::Blood, (HD + 31, PLAIN), "heal_after", "Heal 10 whenever a new wave steps in.", |m| m.heal_after += 10),
    charm("Magpie Purse", Common, Set::Delver, (HD + 32, PLAIN), "shard_pct", "+50% shards from battles.", |m| m.shard_pct += 50),
    charm("Head Start", Common, Set::Surge, (HD + 33, PLAIN), "start_stacks", "Begin each battle with 3 random gems.", |m| m.start_stacks += 3),
    charm("Bramble Coat", Common, Set::Cinder, (HD + 34, PLAIN), "thorns", "Enemy rites that hit you cost them 4 HP.", |m| m.thorns += 4),
    charm("Saboteur Kit", Common, Set::Delver, (HD + 35, PLAIN), "foe_mines", "+2 faults on every enemy board.", |m| m.foe_mines += 2),
    charm("Sweeper's Map", Common, Set::Delver, (HD + 36, PLAIN), "fewer_mines", "1 fewer fault on each of your boards.", |m| m.fewer_mines += 1),
    charm("Shell Fragment", Common, Set::Guard, (HD + 37, PLAIN), "shield_turn", "Gain 4 shield at the start of your turn.", |m| m.shield_turn += 4),
    charm("Whetted Shard", Common, Set::Blade, (HD + 38, PLAIN), "rite_bonus", "Your rites deal +5 damage.", |m| m.rite_bonus += 5),
    charm("Kindling", Common, Set::Cinder, (HD + 39, PLAIN), "rite_burn", "Every rite you cast adds 1 burn on the enemy.", |m| m.rite_burn += 1),
    charm("Slow Fuse", Common, Set::Cinder, (HD + 40, PLAIN), "burn_bonus", "Your burns deal +2 more damage each turn.", |m| m.burn_bonus += 2),
    charm("Sap Tap", Common, Set::Blood, (HD + 41, PLAIN), "claim_heal", "Each crystal you mine heals you for 2.", |m| m.claim_heal += 2),
    charm("Crystal Skin", Common, Set::Guard, (HD + 42, PLAIN), "claim_shield", "Each crystal you mine gives 3 shield.", |m| m.claim_shield += 3),
    charm("Plate Scraps", Common, Set::Guard, (HD + 43, PLAIN), "armor", "Every hit on you deals 2 less.", |m| m.armor += 2),
    charm("Heart Root", Common, Set::Blood, (HD + 44, PLAIN), "max_hp", "+20 max HP.", |m| m.max_hp += 20),
    // rares change how a turn is played
    charm("Quick Fingers", Rare, Set::Delver, (HD + 45, PLAIN), "reveals", "+1 pick each turn.", |m| m.reveals += 1),
    charm("Resonator", Rare, Set::Blade, (HD + 46, PLAIN), "combo_pct", "Each rite's listed damage, shield and healing are 75% stronger.", |m| m.combo_pct += 75),
    charm("Leech Fang", Rare, Set::Blood, (HD + 47, PLAIN), "lifesteal_pct", "Heal for 30% of the damage your rites deal.", |m| m.lifesteal_pct += 30),
    charm("Ambush Wire", Rare, Set::Cinder, (HD + 48, PLAIN), "opening", "Deal 10 damage at the start of each wave.", |m| m.opening += 10),
    charm("Running Start", Rare, Set::Surge, (HD + 49, PLAIN), "surge_start", "Begin each turn with 6 surge.", |m| m.surge_start += 6),
    charm("Overcharger", Rare, Set::Surge, (HD + 50, PLAIN), "surge_pct", "Each surge step is worth +17% instead of +10%.", |m| m.surge_pct += 7),
    charm("Steady Hands", Rare, Set::Surge, (HD + 51, PLAIN), "keep_surge", "Surge carries over between your turns.", |m| m.keep_surge += 1),
    charm("Opening Gambit", Rare, Set::Blade, (HD + 52, PLAIN), "first_pct", "Your first rite each turn deals +100%.", |m| m.first_pct += 100),
    charm("Resonant Shards", Rare, Set::Cinder, (HD + 53, PLAIN), "claim_zap", "Each crystal you mine zaps the enemy for 3.", |m| m.claim_zap += 3),
    charm("Salvager", Rare, Set::Delver, (HD + 54, PLAIN), "salvage", "Each cave-in on your boards gives a gem of its element.", |m| m.salvage += 1),
    charm("Spite Fuse", Rare, Set::Cinder, (HD + 55, PLAIN), "mine_spite", "Each cave-in on your boards also deals 6 damage to the enemy.", |m| m.mine_spite += 6),
    charm("Hourglass", Rare, Set::Guard, (HD + 56, PLAIN), "reveal_shield", "Each pick you leave unused when your turn ends becomes 3 shield.", |m| m.reveal_shield += 3),
    charm("Hoarder's Sack", Rare, Set::Guard, (HD + 57, PLAIN), "hoard_shield", "End your turn holding 3 or more gems: gain 6 shield.", |m| m.hoard_shield += 6),
    charm("Chain Lightning", Rare, Set::Blade, (HD + 58, PLAIN), "chain", "Each rite after your first in a turn deals +5 more than the one before.", |m| m.chain += 5),
    charm("Powder Keg", Rare, Set::Surge, (HD + 59, PLAIN), "mine_surge", "Each cave-in on your boards gives 3 surge.", |m| m.mine_surge += 3),
    // legendaries bend a core rule
    charm("Twin Vein", Legendary, Set::Delver, (HD + 60, PLAIN), "claim_bonus", "Crystals give 2 gems, but your boards hold 2 more faults.", |m| {
        m.claim_bonus += 1;
        m.fewer_mines -= 2
    }),
    charm("Glass Blade", Legendary, Set::Blade, (HD + 61, PLAIN), "power", "+2 power, but cave-ins deal 2 more damage to you.", |m| {
        m.power += 2;
        m.mine_guard -= 2
    }),
    charm("Smoke Bomb", Legendary, Set::Guard, (HD + 62, PLAIN), "evade", "The first enemy rite each turn misses you.", |m| m.evade += 1),
    charm("Sixth Sense", Legendary, Set::Delver, (HD + 63, PLAIN), "sense", "The first cave-in on your boards each turn does no harm.", |m| m.sense += 1),
    charm("Echo Chamber", Legendary, Set::Blade, (HD + 64, PLAIN), "echo", "Your three-element rites fire twice.", |m| m.echo += 1),
    charm("Berserker's Axe", Legendary, Set::Blood, (HD + 65, PLAIN), "desperate", "Below half HP, +4 power.", |m| m.desperate += 4),
    // opened by deeds
    charm("Scavenger's Hook", Common, Set::Delver, (HD + 97, PLAIN), "foe_mine_gift", "Gain a random gem whenever the enemy's ground caves in.", |m| m.foe_mine_gift += 1),
    charm("War Drum", Rare, Set::Blade, (HD + 98, PLAIN), "slow_enrage", "+1 power every second turn of a battle.", |m| m.slow_enrage += 1),
    charm("Damper Coil", Rare, Set::Surge, (HD + 99, PLAIN), "foe_surge_cap", "The enemy's surge cannot pass 3.", |m| m.foe_surge_cap = 3),
    charm("Bone Pact", Legendary, Set::Blood, (HD + 100, PLAIN), "enrage", "+1 power every turn of a battle, but 20 less max HP.", |m| {
        m.enrage += 1;
        m.max_hp -= 20
    }),
];

/// Sprites from here up are the 128px sigils of assets/sigils/sigils-mono.png (game-icons.net, CC BY 3.0),
/// in the order of its build_sigils.py; below it, tiles of the Tiny Dungeon sheet.
pub const HD: usize = 200;

pub struct EnemyDef {
    pub name: &'static str,
    pub sprite: usize,
    pub desc: &'static str,
    pub hp: i32,
    pub power: i32,
    pub elements: &'static [Element],
    pub mods: Tweak,
}

#[allow(clippy::too_many_arguments)]
const fn foe(name: &'static str, sprite: usize, desc: &'static str, hp: i32, power: i32, elements: &'static [Element], mods: Tweak) -> EnemyDef {
    EnemyDef { name, sprite, desc, hp, power, elements, mods }
}

// Every enemy bends one rule, so each asks for a different answer.
pub static NORMALS: &[EnemyDef] = &[
    foe("Mire Hound", HD + 3, "Grows fiercer: +1 power every turn.", 16, 1, &[Tide, Stone], |m| m.enrage = 1),
    foe("Cinder Imp", HD + 4, "Gains a gem whenever your ground caves in.", 12, 2, &[Ember, Spark], |m| m.foe_mine_gift = 1),
    foe("Gust Wisp", HD + 5, "Slippery: the first rite each turn only grazes it, for half damage.", 12, 1, &[Gale, Spark], |m| {
        m.evade = 1;
        m.graze = 1
    }),
    foe("Pebble Golem", HD + 6, "Stone hide: every hit deals 2 less.", 20, 1, &[Stone, Ember], |m| m.armor = 2),
    foe("Brine Leech", HD + 7, "Heals for half the damage it deals.", 16, 1, &[Tide, Gale], |m| m.lifesteal_pct = 50),
    foe("Static Moth", HD + 8, "Jams your surge at 4.", 12, 2, &[Spark, Tide], |m| m.foe_surge_cap = 4),
    foe("Thorn Creeper", HD + 9, "Every rite that hits it deals 2 back.", 18, 1, &[Stone, Gale], |m| m.thorns = 2),
    foe("Ash Rat", HD + 10, "Snatches one of your gems as its turn starts.", 10, 2, &[Ember, Gale], |m| m.pilfer = 1),
];

/// Three per floor; each elite node rolls one from its floor's.
pub static ELITES: &[&[EnemyDef]] = &[
    &[
        foe("The Quarrymaster", HD + 11, "Starts with 8 shield. Every hit deals 1 less.", 45, 2, &[Stone, Ember, Spark], |m| {
            m.start_shield = 8;
            m.armor = 1
        }),
        foe("Crystal Warden", HD + 85, "Each crystal it mines gives it 4 shield.", 42, 2, &[Stone, Tide, Spark], |m| m.claim_shield = 4),
        foe("The Burrower", HD + 86, "2 more faults in your boards.", 44, 2, &[Stone, Gale, Ember], |m| m.foe_mines = 2),
    ],
    &[
        foe("Gale Duchess", HD + 12, "One extra pick. The first rite each turn only grazes her, for half damage.", 38, 2, &[Gale, Tide, Spark], |m| {
            m.reveals = 1;
            m.evade = 1;
            m.graze = 1
        }),
        foe("Flail Sergeant", HD + 87, "Each of its rites after the first in a turn hits 5 harder.", 40, 3, &[Ember, Stone, Spark], |m| m.chain = 5),
        foe("Iron Tortoise", HD + 88, "Gains 4 shield each turn, and its shield never wears off.", 40, 2, &[Stone, Tide, Gale], |m| {
            m.shield_turn = 4;
            m.keep_shield = 1
        }),
    ],
    &[
        foe("Kiln Tyrant", HD + 13, "Its rites set you burning, and it gains +1 power every turn.", 42, 2, &[Ember, Gale, Stone], |m| {
            m.rite_burn = 1;
            m.burn_bonus = 1;
            m.enrage = 1
        }),
        foe("The Executioner", HD + 89, "Its first rite each turn hits double.", 42, 2, &[Ember, Spark, Stone], |m| m.first_pct = 100),
        foe("The Shade", HD + 90, "The first rite each turn misses it.", 40, 2, &[Gale, Tide, Spark], |m| m.evade = 1),
    ],
];

/// Four per floor; each map rolls which one waits at the end.
pub static BOSSES: &[&[EnemyDef]] = &[
    &[
        foe("Minelayer Queen", HD + 14, "1 more fault in your boards. Gains a gem whenever your ground caves in.", 50, 2, &[Ember, Stone, Spark], |m| {
            m.foe_mines = 1;
            m.foe_mine_gift = 1
        }),
        // stack snatchers wait past the first floor: they made it a chore
        foe("The Undertow", HD + 16, "Each turn: heals 2 and gains 2 shield.", 46, 2, &[Tide, Gale, Stone], |m| {
            m.regen = 2;
            m.shield_turn = 2
        }),
        foe("Old Tusker", HD + 91, "Starts with 6 shield. Below half HP, +2 power.", 46, 2, &[Stone, Ember, Gale], |m| {
            m.start_shield = 6;
            m.desperate = 2
        }),
        foe("The Fen Crone", HD + 92, "Each crystal it mines heals it 2.", 44, 2, &[Tide, Gale, Stone], |m| m.claim_heal = 2),
    ],
    &[
        foe("The Sapper King", HD + 15, "Starts with 12 shield. Snatches one of your gems each turn.", 105, 3, &[Ember, Gale, Spark], |m| {
            m.start_shield = 12;
            m.pilfer = 1
        }),
        foe("The Tidecaller", HD + 17, "The first rite each turn only grazes her, for half damage. Heals for 30% of damage dealt.", 96, 3, &[Tide, Spark, Gale], |m| {
            m.evade = 1;
            m.graze = 1;
            m.lifesteal_pct = 30
        }),
        foe("The Broodmother", HD + 93, "Hits you for 10 as she steps in. 1 more fault in your boards.", 100, 3, &[Spark, Stone, Ember], |m| {
            m.opening = 10;
            m.foe_mines = 1
        }),
        foe("Storm Roc", HD + 94, "Its rites deal +25%. Jams your surge at 5.", 96, 3, &[Gale, Spark, Tide], |m| {
            m.rite_pct = 25;
            m.foe_surge_cap = 5
        }),
    ],
    &[
        foe("Null Cartographer", HD + 18, "One extra pick, 3 of its cracks propped up. Jams your surge at 6.", 135, 4, &[Spark, Gale, Ember], |m| {
            m.reveals = 1;
            m.scouts = 3;
            m.foe_surge_cap = 6
        }),
        foe("The Deep Engine", HD + 19, "Iron plating: every hit deals 2 less. +1 power every turn.", 125, 3, &[Stone, Ember, Tide], |m| {
            m.armor = 2;
            m.enrage = 1
        }),
        foe("The Wyrm Below", HD + 95, "Its rites set you burning. Below half HP, +3 power.", 130, 3, &[Ember, Gale, Stone], |m| {
            m.rite_burn = 2;
            m.desperate = 3
        }),
        foe("The Hive Queen", HD + 96, "One extra pick. Each of her rites after the first in a turn hits 8 harder.", 135, 4, &[Spark, Ember, Gale], |m| {
            m.reveals = 1;
            m.chain = 8
        }),
    ],
];

/// A secret ending: a floor past the last boss, opened by carrying its key. A key is strong and
/// cursed at once, and both hold for the rest of the run. Held keys add their floors in this order.
pub struct Ending {
    pub name: &'static str,
    pub key: &'static str,
    pub key_sprite: usize,
    pub boon: &'static str,
    pub curse: &'static str,
    /// the boon, and any curse on you
    pub me: Tweak,
    /// enemies' HP, in percent of normal
    pub hp_pct: i32,
    /// shop prices, in percent of normal
    pub price_pct: i32,
    /// the card that offers the key, once its condition is met
    pub offer: Encounter,
    pub normals: &'static [EnemyDef],
    pub elite: EnemyDef,
    pub boss: EnemyDef,
}

pub static ENDINGS: &[Ending] = &[
    Ending {
        name: "The Drowned Archive",
        key: "Brine Lantern",
        key_sprite: HD + 82,
        boon: "+1 pick each turn.",
        curse: "Enemies have 15% more HP.",
        me: |m| m.reveals += 1,
        hp_pct: 115,
        price_pct: 100,
        offer: Encounter {
            title: "Brine Lantern",
            text: "A lantern burns under the flooded stair, lighting a door no map shows. \
                   Carry it: +1 pick a turn, but enemies +15% HP. Past the last boss, the door opens.",
            choices: &[pick("Take the lantern.", &[Key(0)]), pick("Leave it burning.", &[Text("The light goes out behind you.")])],
        },
        normals: &[
            foe("Lamprey", HD + 67, "Snatches one of your gems as its turn starts, and heals for half the damage it deals.", 18, 2, &[Tide, Gale], |m| {
                m.pilfer = 1;
                m.lifesteal_pct = 50
            }),
            foe("Ink Jelly", HD + 68, "Every rite that hits it deals 3 back. Jams your surge at 6.", 16, 2, &[Tide, Spark], |m| {
                m.thorns = 3;
                m.foe_surge_cap = 6
            }),
            foe("Deepkin", HD + 69, "Heals 3 each turn. Every hit deals 1 less.", 22, 2, &[Tide, Stone], |m| {
                m.regen = 3;
                m.armor = 1
            }),
        ],
        elite: foe("The Angler", HD + 70, "Its lure leaves 2 more faults in your boards. The first rite each turn only grazes it.", 50, 3, &[Tide, Gale, Spark], |m| {
            m.foe_mines = 2;
            m.evade = 1;
            m.graze = 1
        }),
        boss: foe("The Archivist Below", HD + 71, "Snatches two of your gems as its turn starts. Heals 3 each turn.", 200, 5, &[Tide, Stone, Spark], |m| {
            m.pilfer = 2;
            m.regen = 3
        }),
    },
    Ending {
        name: "The Furnace Heart",
        key: "Cinder Seal",
        key_sprite: HD + 83,
        boon: "Your rites deal 25% more damage.",
        curse: "You start every wave with 4 burn.",
        me: |m| {
            m.rite_pct += 25;
            m.self_burn += 4
        },
        hp_pct: 100,
        price_pct: 100,
        offer: Encounter {
            title: "Cinder Seal",
            text: "The forge you stoked left a seal in its ashes, still red. \
                   Carry it: rites +25% damage, but every wave starts you on 4 burn. Past the last boss, the heat leads down.",
            choices: &[pick("Take the seal.", &[Key(1)]), pick("Let it cool.", &[Text("It hisses into grey ash.")])],
        },
        normals: &[
            foe("Salamander", HD + 72, "Its rites set you burning.", 16, 2, &[Ember, Stone], |m| {
                m.rite_burn = 1;
                m.burn_bonus = 1
            }),
            foe("Living Flame", HD + 73, "+1 power every turn. Cave-ins on its boards burst at you for 5.", 14, 3, &[Ember, Gale], |m| {
                m.enrage = 1;
                m.mine_spite = 5
            }),
            foe("Cinder Whirl", HD + 74, "Each crystal it mines scorches you for 3.", 15, 2, &[Ember, Spark], |m| m.claim_zap = 3),
        ],
        elite: foe("Slag Colossus", HD + 75, "Starts with 12 shield and gains 3 each turn. Its rites set you burning.", 55, 3, &[Ember, Stone, Spark], |m| {
            m.start_shield = 12;
            m.shield_turn = 3;
            m.rite_burn = 1
        }),
        boss: foe("The Furnace Heart", HD + 76, "Each of its rites stacks 3 burn on you, and your burn hurts 2 more.", 210, 5, &[Ember, Stone, Gale], |m| {
            m.rite_burn = 3;
            m.burn_bonus = 2
        }),
    },
    Ending {
        name: "The Silent Engine",
        key: "Clockwork Eye",
        key_sprite: HD + 84,
        boon: "Begin each battle with 4 random gems.",
        curse: "Shop prices are 25% higher.",
        me: |m| m.start_stacks += 4,
        hp_pct: 100,
        price_pct: 125,
        offer: Encounter {
            title: "Clockwork Eye",
            text: "A brass eye on the shop wall turns to follow you, ticking. \
                   Carry it: begin each battle with 4 random gems, but shop prices +25%. Past the last boss, something is still running.",
            choices: &[pick("Pry the eye loose.", &[Key(2)]), pick("Walk past it.", &[Text("It keeps watching.")])],
        },
        normals: &[
            foe("Spindle Crawler", HD + 77, "Leaves 1 more fault in your boards, and gains a gem whenever your ground caves in.", 16, 2, &[Spark, Stone], |m| {
                m.foe_mines = 1;
                m.foe_mine_gift = 1
            }),
            foe("Hover Drone", HD + 78, "One extra pick. The first rite each turn only grazes it.", 13, 2, &[Spark, Gale], |m| {
                m.reveals = 1;
                m.evade = 1;
                m.graze = 1
            }),
            foe("Rivet Turret", HD + 79, "Fires 6 damage at you as each wave starts. Every hit deals 3 less.", 20, 3, &[Spark, Stone], |m| {
                m.opening = 6;
                m.armor = 3
            }),
        ],
        elite: foe("The Foreman", HD + 80, "Jams your surge at 4. Every hit deals 2 less.", 52, 3, &[Stone, Spark, Ember], |m| {
            m.foe_surge_cap = 4;
            m.armor = 2
        }),
        boss: foe("The Silent Engine", HD + 81, "Jams your surge at 4 and leaves 2 more faults in your boards. Every hit deals 1 less.", 200, 5, &[Spark, Stone, Tide], |m| {
            m.foe_surge_cap = 4;
            m.foe_mines = 2;
            m.armor = 1
        }),
    },
];

/// True for any boss, a floor's or an ending's.
pub fn is_boss(name: &str) -> bool {
    BOSSES.iter().flat_map(|f| f.iter()).chain(ENDINGS.iter().map(|e| &e.boss)).any(|d| d.name == name)
}

/// The places on the overworld, unlocked one after another by winning the one before.
/// Each is harder than the last; its rules stack on everything else.
pub struct Expedition {
    pub name: &'static str,
    pub text: &'static str,
    pub rules: &'static [&'static str],
    /// enemies' HP, in percent of normal
    pub hp_pct: i32,
    pub me: Tweak,
    /// every enemy
    pub foe: Tweak,
    /// bosses, on top of `foe`
    pub boss: Tweak,
    /// every floor's twist is a curse
    pub curses: bool,
}

pub static EXPEDITIONS: &[Expedition] = &[
    Expedition {
        name: "Shallow Mines",
        text: "Old workings near the surface.",
        rules: &["No extra rules."],
        hp_pct: 100,
        me: NOTHING,
        foe: NOTHING,
        boss: NOTHING,
        curses: false,
    },
    Expedition {
        name: "Flooded Caverns",
        text: "Groundwater got here first. Things grew fat on it.",
        rules: &["Enemies +10% HP."],
        hp_pct: 110,
        me: NOTHING,
        foe: NOTHING,
        boss: NOTHING,
        curses: false,
    },
    Expedition {
        name: "Ember Depths",
        text: "Warm rock. The bosses burn hotter.",
        rules: &["Enemies +10% HP.", "Bosses +1 power every 2 turns.", "About 9 faults on your boards."],
        hp_pct: 110,
        me: |m| m.fewer_mines -= 1,
        foe: NOTHING,
        boss: |m| m.slow_enrage += 1,
        curses: false,
    },
    Expedition {
        name: "Crystal Hollows",
        text: "Caves of glass. Every step rings.",
        rules: &["Enemies +15% HP.", "Bosses +1 power every 2 turns.", "1 fewer pick a turn.", "About 10 faults on your boards."],
        hp_pct: 115,
        me: |m| {
            m.reveals -= 1;
            m.fewer_mines -= 2
        },
        foe: NOTHING,
        boss: |m| m.slow_enrage += 1,
        curses: false,
    },
    Expedition {
        name: "The Rift",
        text: "A crack in the world. Nothing here is kind.",
        rules: &["Every law of Crystal\u{a0}Hollows.", "Every floor is cursed.", "Rites cost 1 more gem.", "About 11 faults on your boards."],
        hp_pct: 115,
        me: |m| {
            m.reveals -= 1;
            m.toll += 1;
            m.fewer_mines -= 3
        },
        foe: NOTHING,
        boss: |m| m.slow_enrage += 1,
        curses: true,
    },
    Expedition {
        name: "The Null",
        text: "Where the maps stop.",
        rules: &["Every law of The\u{a0}Rift.", "Enemies +25% HP, +1 power.", "About 12 faults on your boards."],
        hp_pct: 125,
        me: |m| {
            m.reveals -= 1;
            m.toll += 1;
            m.fewer_mines -= 4
        },
        foe: |m| m.power += 1,
        boss: |m| m.slow_enrage += 1,
        curses: true,
    },
];

/// Each floor rolls one twist that bends the rules for that floor only.
pub struct Twist {
    pub name: &'static str,
    /// 1 blessing, -1 curse, 0 cuts both ways
    pub tone: i32,
    pub desc: &'static str,
    pub me: Tweak,
    pub foe: Tweak,
}

const fn twist(name: &'static str, tone: i32, desc: &'static str, me: Tweak, foe: Tweak) -> Twist {
    Twist { name, tone, desc, me, foe }
}

const NOTHING: Tweak = |_| {};

pub static TWISTS: &[Twist] = &[
    twist("Shallow Seams", 1, "1 fewer fault on each of your boards.", |m| m.fewer_mines += 1, NOTHING),
    twist("Tailwind", 1, "Begin each turn with 2 surge.", |m| m.surge_start += 2, NOTHING),
    twist("Second Wind", 1, "Heal 6 whenever a new wave steps in.", |m| m.heal_after += 6, NOTHING),
    twist("Gold Dust", 1, "+50% shards from battles.", |m| m.shard_pct += 50, NOTHING),
    twist("Fault Lines", -1, "Cave-ins deal 1 more damage to you.", |m| m.mine_guard -= 1, NOTHING),
    twist("Iron Hides", -1, "Enemies start each wave with 6 shield.", NOTHING, |m| m.start_shield += 6),
    twist("Restless Dead", -1, "Enemies have +1 power.", NOTHING, |m| m.power += 1),
    twist("Thick Fog", -1, "1 fewer pick each turn.", |m| m.reveals -= 1, NOTHING),
    twist("Brittle Ground", 0, "1 more fault on every board, yours and theirs.", |m| m.fewer_mines -= 1, |m| m.fewer_mines -= 1),
    twist("Sharp Edges", 0, "Everyone has +1 power.", |m| m.power += 1, |m| m.power += 1),
];

pub enum Out {
    MaxHp(i32),
    Shards(i32),
    GainCharm,
    /// lose a random charm; a choice with this needs one to give
    LoseCharm,
    Text(&'static str),
    Chance(u32, &'static [Out], &'static [Out]),
    /// take ending `k`'s key
    Key(usize),
    /// ending `k`'s key is offered once this encounter is over
    Omen(usize),
    /// the forge is lit: an elite beaten on this floor offers the Cinder Seal
    Kindle,
}
use Out::*;

pub struct Choice {
    pub label: &'static str,
    pub outs: &'static [Out],
}

pub struct Encounter {
    pub title: &'static str,
    pub text: &'static str,
    pub choices: &'static [Choice],
}

const fn pick(label: &'static str, outs: &'static [Out]) -> Choice {
    Choice { label, outs }
}

const LEAVE: Choice = pick("Walk on.", &[Text("You leave it be.")]);

pub static ENCOUNTERS: &[Encounter] = &[
    Encounter {
        title: "The Humming Crate",
        text: "A sealed crate sits in the middle of the path, humming a tune you almost recognise.",
        choices: &[
            pick("Pry it open. (-3 max HP, gain a pact)", &[MaxHp(-3), GainCharm]),
            pick("Sell it unopened. (+25 shards)", &[Shards(25)]),
            LEAVE,
        ],
    },
    Encounter {
        title: "Wandering Tinker",
        text: "A tinker with far too many pockets offers you a deal, then a different deal.",
        choices: &[
            pick("Strike a bargain. (-30 shards, gain a pact)", &[Shards(-30), GainCharm]),
            pick("Sell some blood. (-4 max HP, +30 shards)", &[MaxHp(-4), Shards(30)]),
            LEAVE,
        ],
    },
    Encounter {
        title: "Collapsed Tunnel",
        text: "Rubble blocks the way. Something glitters underneath it.",
        choices: &[
            pick("Dig. (50%: +40 shards, else -4 max HP)", &[Chance(50, &[Shards(40)], &[Text("The ceiling answers."), MaxHp(-4)])]),
            pick("Go around.", &[Text("The long way is only long.")]),
        ],
    },
    Encounter {
        title: "Shrine of Small Mercies",
        text: "A tiny shrine, swept clean. Someone still looks after it.",
        choices: &[
            pick("Pray. (+3 max HP)", &[MaxHp(3)]),
            pick("Leave an offering. (-20 shards, +6 max HP)", &[Shards(-20), MaxHp(6)]),
        ],
    },
    Encounter {
        title: "The Gambler's Field",
        text: "A grinning stranger has laid out two tiles. 'One of them pays,' they say.",
        choices: &[
            pick("Bet. (-20 shards, 50%: +50 shards)", &[Shards(-20), Chance(50, &[Shards(50)], &[Text("Wrong tile.")])]),
            LEAVE,
        ],
    },
    Encounter {
        title: "Old Sweeper's Ghost",
        text: "A pale figure taps the ground with a stick, counting under its breath.",
        choices: &[
            pick("Listen to its counting. (+4 max HP)", &[MaxHp(4)]),
            pick("Take its satchel. (+30 shards, -3 max HP)", &[Shards(30), MaxHp(-3)]),
        ],
    },
    Encounter {
        title: "Glowing Vein",
        text: "A seam of bright ore runs along the wall, warm to the touch.",
        choices: &[
            pick("Mine it. (+35 shards, 40%: -5 max HP)", &[Shards(35), Chance(40, &[Text("The vein bites back."), MaxHp(-5)], &[])]),
            pick("Reach inside. (50%: pact, else -6 max HP)", &[Chance(50, &[GainCharm], &[Text("Something reaches back."), MaxHp(-6)])]),
            LEAVE,
        ],
    },
    Encounter {
        title: "Deserter's Camp",
        text: "A lone deserter keeps a small fire going and eyes your pack.",
        choices: &[
            pick("Share the fire. (+3 max HP)", &[MaxHp(3)]),
            pick("Rob the camp. (+30 shards, -4 max HP)", &[Shards(30), MaxHp(-4)]),
        ],
    },
    Encounter {
        title: "Echoing Well",
        text: "Your footsteps come back from the well a moment too late.",
        choices: &[
            pick("Throw in 10 shards. (60%: gain a pact)", &[Shards(-10), Chance(60, &[GainCharm], &[Text("Only an echo.")])]),
            pick("Drink. (+2 max HP)", &[MaxHp(2)]),
            pick("Climb down toward the light. (-4 max HP)", &[MaxHp(-4), Text("Far below, something glows under the water."), Omen(0)]),
        ],
    },
    Encounter {
        title: "Unstable Ground",
        text: "The floor ahead is cracked like old glaze.",
        choices: &[
            pick("Sprint across. (-2 max HP, +6 shards)", &[MaxHp(-2), Shards(6)]),
            pick("Tiptoe. (70%: +10 shards, else -6 max HP)", &[Chance(70, &[Shards(10)], &[Text("Crack."), MaxHp(-6)])]),
            LEAVE,
        ],
    },
    Encounter {
        title: "Pact Swapper",
        text: "A hooded trader spreads a cloth covered in sealed pacts. 'Yours for two of mine,' she says.",
        choices: &[pick("Trade. (Give up a pact you choose, gain 2)", &[LoseCharm, GainCharm, GainCharm]), LEAVE],
    },
    Encounter {
        title: "Abandoned Forge",
        text: "The coals are still warm. A crucible waits, big enough for something small and precious.",
        choices: &[
            pick("Melt a pact down. (Give up a pact you choose, +60 shards)", &[LoseCharm, Shards(60)]),
            pick("Stoke the fire. (-10 shards, +5 max HP)", &[Shards(-10), MaxHp(5), Kindle]),
            LEAVE,
        ],
    },
    Encounter {
        title: "Flooded Gallery",
        text: "Black water fills the tunnel to the waist. Something shiny rests at the deep end.",
        choices: &[
            pick("Wade along the edge. (-3 max HP, +25 shards)", &[MaxHp(-3), Shards(25)]),
            pick("Swim for it. (40%: gain a pact, else -5 max HP)", &[Chance(40, &[GainCharm], &[Text("Cold, dark, empty."), MaxHp(-5)])]),
            pick("Dive toward the light below. (-4 max HP)", &[MaxHp(-4), Text("Deeper than the shiny thing, something glows."), Omen(0)]),
            LEAVE,
        ],
    },
    Encounter {
        title: "Crystal Garden",
        text: "Crystals grow from the walls like flowers, ringing softly when you breathe.",
        choices: &[
            pick("Harvest them. (+40 shards, -3 max HP)", &[Shards(40), MaxHp(-3)]),
            pick("Sit and listen. (+4 max HP)", &[MaxHp(4)]),
        ],
    },
    Encounter {
        title: "Lost Prospector",
        text: "An old prospector sits on his pack, map upside down, lamp nearly out.",
        choices: &[
            pick("Walk him out. (-3 max HP, +6 max HP)", &[MaxHp(-3), MaxHp(6)]),
            pick("Take his pack. (+35 shards, 30%: -5 max HP)", &[Shards(35), Chance(30, &[Text("He fights back."), MaxHp(-5)], &[])]),
            LEAVE,
        ],
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_level_adds_at_least_one() {
        assert_eq!((1..=3).map(|l| at_level(1, l)).collect::<Vec<_>>(), [1, 2, 3]);
        // bigger numbers keep the +50% per level
        assert_eq!((1..=3).map(|l| at_level(6, l)).collect::<Vec<_>>(), [6, 9, 12]);
        assert_eq!((1..=3).map(|l| at_level(3, l)).collect::<Vec<_>>(), [3, 4, 6]);
    }
}
