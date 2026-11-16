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
    pub mine_no_end: i32,
    pub sense: i32,
    pub echo: i32,
    pub desperate: i32,
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
    /// more damage for each burn on the foe as the rite lands
    PerBurn(i32),
    /// more damage for each quarter of its HP the caster has lost (up to 3)
    PerQuarterLost(i32),
    /// the caster pays HP, never below 1
    SelfDamage(i32),
}
use Effect::*;

pub struct Combo {
    pub name: &'static str,
    /// names at level 2 and 3: a boss kill upgrades one skill
    pub up: [&'static str; 2],
    pub needs: &'static [Element],
    pub effects: &'static [Effect],
}

/// Highest level a skill can reach.
pub const MAX_LEVEL: usize = 3;
/// A rite at `MAX_LEVEL` is mastered: its damage, shield and healing are this much stronger.
pub const MASTERED_PCT: i32 = 50;

/// `v`, made stronger if the rite is mastered.
pub fn mastered(v: i32, lvl: usize) -> i32 {
    if lvl >= MAX_LEVEL { v * (100 + MASTERED_PCT) / 100 } else { v }
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
pub fn rite_text(c: &Combo, lvl: usize, brand: Option<&Brand>) -> String {
    let mut parts: Vec<String> = c.effects.iter().map(|&e| e.text(lvl)).chain(brand.map(|b| b.effect.text(1))).collect();
    if lvl >= MAX_LEVEL {
        parts.push(format!("mastered: +{MASTERED_PCT}% damage, shield and healing"));
    }
    let s = parts.join(", ");
    s[..1].to_uppercase() + &s[1..]
}

impl Effect {
    /// True if a rite with this effect deals damage. The player's rites without one only do what they say.
    pub fn hits(self) -> bool {
        matches!(self, Damage(_) | PerSurge(_) | Detonate | PerReveal(_) | ShieldBlast(_) | PerBurn(_) | PerQuarterLost(_))
    }

    pub fn text(self, lvl: usize) -> String {
        match self {
            Damage(v) => format!("+{} damage", at_level(v, lvl)),
            Burn(n) => format!("+{} burn on foe", at_level(n, lvl)),
            Shield(v) => format!("gain {} shield", at_level(v, lvl)),
            Heal(v) => format!("heal {}", at_level(v, lvl)),
            Weaken(n) => format!("foe's attacks halved, {} turns", at_level(n, lvl)),
            Drain(n) => {
                let k = at_level(n as i32, lvl);
                format!("foe loses {k} stack{}", if k == 1 { "" } else { "s" })
            }
            Pierce => "shatter foe's shield".into(),
            PerSurge(v) => format!("+{} damage per surge", at_level(v, lvl)),
            Detonate => "detonate foe's burn".into(),
            Reveal(n) => {
                let k = at_level(n, lvl);
                format!("+{k} reveal{}", if k == 1 { "" } else { "s" })
            }
            Shatter(n) => {
                let k = at_level(n as i32, lvl);
                format!("break {k} foe crystal{}", if k == 1 { "" } else { "s" })
            }
            Bury(n) => {
                let k = at_level(n as i32, lvl);
                format!("+{k} foe mine{}", if k == 1 { "" } else { "s" })
            }
            Surge(v) => format!("+{} surge", at_level(v, lvl)),
            PerReveal(v) => format!("+{} damage per reveal used", at_level(v, lvl)),
            DoubleShield(cap) => format!("double your shield (up to +{})", at_level(cap, lvl)),
            ShieldBlast(p) => match at_level(p, lvl) {
                100 => "all your shield becomes damage".into(),
                q => format!("your shield becomes {q}% damage"),
            },
            PerBurn(v) => format!("+{} damage per burn on foe", at_level(v, lvl)),
            PerQuarterLost(v) => format!("+{} damage per quarter of HP lost", at_level(v, lvl)),
            SelfDamage(v) => format!("you lose {} HP", at_level(v, lvl)),
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
    blessing("Sight", "+1 reveal each turn.", |m| m.reveals += 1),
    blessing("Surge", "Begin each turn with 2 surge.", |m| m.surge_start += 2),
];

/// Burnt onto a rite, a brand adds its effect. It costs no extra gem and does not grow with the rite.
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
];

const fn combo(name: &'static str, up: [&'static str; 2], needs: &'static [Element], effects: &'static [Effect]) -> Combo {
    Combo { name, up, needs, effects }
}

pub static COMBOS: &[Combo] = &[
    combo("Steam Burst", ["Scald Geyser", "Steamquake"], &[Ember, Tide], &[Damage(4), Reveal(1)]),
    combo("Raze", ["Bloodrush", "Rampage"], &[Ember, Gale], &[Damage(2), PerQuarterLost(2)]),
    combo("Magma", ["Lava Lance", "Molten Core"], &[Ember, Stone], &[Pierce, Damage(3)]),
    combo("Plasma", ["Plasma Lance", "Plasma Nova"], &[Ember, Spark], &[Damage(3), PerSurge(1)]),
    combo("Mistveil", ["Mist Shroud", "Veil of Mists"], &[Tide, Gale], &[Heal(8), Reveal(2)]),
    combo("Bulwark", ["Rampart", "Citadel"], &[Tide, Stone], &[Shield(12)]),
    combo("Conduction", ["Overload", "Siphon Storm"], &[Tide, Spark], &[Damage(3), Surge(2)]),
    combo("Sandstorm", ["Dust Devil", "Desert Wrath"], &[Gale, Stone], &[Weaken(2), Bury(2)]),
    combo("Thunderclap", ["Thunderstrike", "Skybreaker"], &[Gale, Spark], &[Shatter(1), Damage(2)]),
    combo("Lodestone", ["Ironheart", "Magnetar"], &[Stone, Spark], &[DoubleShield(12), Shield(5)]),
    combo("Upheaval", ["Cataclysm", "Worldbreaker"], &[Ember, Gale, Stone], &[SelfDamage(4), Damage(11)]),
    combo("Tempest", ["Hurricane", "Eye of the Storm"], &[Tide, Gale, Spark], &[PerReveal(1)]),
    combo("Geode Surge", ["Crystal Tide", "Geode Titan"], &[Tide, Stone, Spark], &[Damage(6), ShieldBlast(150)]),
    combo("Forgefire", ["Starforge", "Sunforge"], &[Ember, Stone, Spark], &[Damage(8)]),
    combo("Maelstrom", ["Abyssal Gyre", "Leviathan"], &[Tide, Gale, Stone], &[Shield(6), Heal(4)]),
    combo("Short Circuit", ["Arc Flash", "Blackout"], &[Ember, Gale, Spark], &[Damage(6), Drain(1)]),
    combo("Boilover", ["Scalding Flood", "Hellbroth"], &[Ember, Tide, Spark], &[Burn(3), PerBurn(1)]),
];

/// Position of `c` in `COMBOS`.
pub fn combo_id(c: &Combo) -> usize {
    COMBOS.iter().position(|x| std::ptr::eq(x, c)).expect("combos come from COMBOS")
}

/// Only the combos a fighter can actually make with its elements.
pub fn combos_for(elements: &[Element]) -> Vec<&'static Combo> {
    COMBOS.iter().filter(|c| c.needs.iter().all(|e| elements.contains(e))).collect()
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
        passive_desc: "Ashen hide: mines deal 1 less damage, and every hit on you 3 less.",
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
        passive_desc: "Quick read: +1 reveal each turn, and each reveal left unused at turn end becomes 3 shield.",
    },
    Hero {
        name: "Deepforger",
        sprite: HD + 2,
        hp: 96,
        power: 6,
        elements: [Tide, Stone, Spark],
        passive: |m| {
            m.shield_block += 1;
        },
        passive_desc: "Plated: each point of shield you hold stops 2 damage.",
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
        passive_desc: "Searing touch: every rite adds 1 burn on the foe and heals you for 15% of its damage.",
    },
];

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

pub struct Charm {
    pub name: &'static str,
    pub rarity: Rarity,
    /// sprite sheet tile and its tint (0xffffff = as drawn)
    pub icon: (usize, u32),
    /// the `Mods` field it changes; battle reports it by this name each time it takes effect
    pub hook: &'static str,
    pub desc: &'static str,
    pub apply: Tweak,
}

const fn charm(name: &'static str, rarity: Rarity, icon: (usize, u32), hook: &'static str, desc: &'static str, apply: Tweak) -> Charm {
    Charm { name, rarity, icon, hook, desc, apply }
}

const PLAIN: u32 = 0xffffff;

pub static CHARMS: &[Charm] = &[
    charm("Lucky Coin", Common, (HD + 28, PLAIN), "duds", "The first mine you hit each wave is a dud.", |m| m.duds += 1),
    charm("Dowsing Rod", Common, (HD + 29, PLAIN), "scouts", "3 mines start flagged on each of your boards.", |m| m.scouts += 3),
    charm("Ward Stone", Common, (HD + 30, PLAIN), "start_shield", "Gain 10 shield at the start of each wave.", |m| m.start_shield += 10),
    charm("Field Rations", Common, (HD + 31, PLAIN), "heal_after", "Heal 10 whenever a new wave steps in.", |m| m.heal_after += 10),
    charm("Magpie Purse", Common, (HD + 32, PLAIN), "shard_pct", "+50% shards from battles.", |m| m.shard_pct += 50),
    charm("Head Start", Common, (HD + 33, PLAIN), "start_stacks", "Begin each battle with 3 random stacks.", |m| m.start_stacks += 3),
    charm("Bramble Coat", Common, (HD + 34, PLAIN), "thorns", "Enemy rites that hit you cost them 4 HP.", |m| m.thorns += 4),
    charm("Saboteur Kit", Common, (HD + 35, PLAIN), "foe_mines", "+2 mines on every enemy board.", |m| m.foe_mines += 2),
    charm("Sweeper's Map", Common, (HD + 36, PLAIN), "fewer_mines", "1 fewer mine on each of your boards.", |m| m.fewer_mines += 1),
    charm("Shell Fragment", Common, (HD + 37, PLAIN), "shield_turn", "Gain 4 shield at the start of your turn.", |m| m.shield_turn += 4),
    charm("Whetted Shard", Common, (HD + 38, PLAIN), "rite_bonus", "Your rites deal +5 damage.", |m| m.rite_bonus += 5),
    charm("Kindling", Common, (HD + 39, PLAIN), "rite_burn", "Every rite you cast adds 1 burn on the enemy.", |m| m.rite_burn += 1),
    charm("Slow Fuse", Common, (HD + 40, PLAIN), "burn_bonus", "Your burns deal +2 more damage each turn.", |m| m.burn_bonus += 2),
    charm("Sap Tap", Common, (HD + 41, PLAIN), "claim_heal", "Each crystal you claim heals you for 2.", |m| m.claim_heal += 2),
    charm("Crystal Skin", Common, (HD + 42, PLAIN), "claim_shield", "Each crystal you claim gives 3 shield.", |m| m.claim_shield += 3),
    charm("Plate Scraps", Common, (HD + 43, PLAIN), "armor", "Every hit on you deals 2 less.", |m| m.armor += 2),
    charm("Heart Root", Common, (HD + 44, PLAIN), "max_hp", "+20 max HP.", |m| m.max_hp += 20),
    // rares change how a turn is played
    charm("Quick Fingers", Rare, (HD + 45, PLAIN), "reveals", "+1 reveal each turn.", |m| m.reveals += 1),
    charm("Resonator", Rare, (HD + 46, PLAIN), "combo_pct", "Each rite's listed damage, shield and healing are 75% stronger.", |m| m.combo_pct += 75),
    charm("Leech Fang", Rare, (HD + 47, PLAIN), "lifesteal_pct", "Heal for 30% of the damage your rites deal.", |m| m.lifesteal_pct += 30),
    charm("Ambush Wire", Rare, (HD + 48, PLAIN), "opening", "Deal 10 damage at the start of each wave.", |m| m.opening += 10),
    charm("Running Start", Rare, (HD + 49, PLAIN), "surge_start", "Begin each turn with 3 surge.", |m| m.surge_start += 3),
    charm("Overcharger", Rare, (HD + 50, PLAIN), "surge_pct", "Each surge step is worth +35% instead of +20%.", |m| m.surge_pct += 15),
    charm("Steady Hands", Rare, (HD + 51, PLAIN), "keep_surge", "Surge carries over between your turns. Mines still wipe it.", |m| m.keep_surge += 1),
    charm("Opening Gambit", Rare, (HD + 52, PLAIN), "first_pct", "Your first rite each turn deals +100%.", |m| m.first_pct += 100),
    charm("Resonant Shards", Rare, (HD + 53, PLAIN), "claim_zap", "Each crystal you claim zaps the enemy for 3.", |m| m.claim_zap += 3),
    charm("Salvager", Rare, (HD + 54, PLAIN), "salvage", "Crystals you break with a mine still pay out.", |m| m.salvage += 1),
    charm("Spite Fuse", Rare, (HD + 55, PLAIN), "mine_spite", "Mines you hit also deal 6 damage to the enemy.", |m| m.mine_spite += 6),
    charm("Hourglass", Rare, (HD + 56, PLAIN), "reveal_shield", "Each reveal you leave unused when your turn ends becomes 3 shield.", |m| m.reveal_shield += 3),
    charm("Hoarder's Sack", Rare, (HD + 57, PLAIN), "hoard_shield", "End your turn holding 3 or more stacks: gain 6 shield.", |m| m.hoard_shield += 6),
    charm("Chain Lightning", Rare, (HD + 58, PLAIN), "chain", "Each rite after your first in a turn deals +5 more than the one before.", |m| m.chain += 5),
    charm("Powder Keg", Rare, (HD + 59, PLAIN), "mine_no_end", "Mines no longer end your turn. They still hurt and wipe surge.", |m| m.mine_no_end += 1),
    // legendaries bend a core rule
    charm("Twin Vein", Legendary, (HD + 60, PLAIN), "claim_bonus", "Crystals give 2 stacks, but your boards hold 2 more mines.", |m| {
        m.claim_bonus += 1;
        m.fewer_mines -= 2
    }),
    charm("Glass Blade", Legendary, (HD + 61, PLAIN), "power", "+2 power, but mines deal 2 more damage to you.", |m| {
        m.power += 2;
        m.mine_guard -= 2
    }),
    charm("Smoke Bomb", Legendary, (HD + 62, PLAIN), "evade", "The first enemy rite each turn misses you.", |m| m.evade += 1),
    charm("Sixth Sense", Legendary, (HD + 63, PLAIN), "sense", "The first mine you flip each turn is flagged instead of going off.", |m| m.sense += 1),
    charm("Echo Chamber", Legendary, (HD + 64, PLAIN), "echo", "Your three-element rites fire twice.", |m| m.echo += 1),
    charm("Berserker's Axe", Legendary, (HD + 65, PLAIN), "desperate", "Below half HP, +4 power.", |m| m.desperate += 4),
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
    /// stacks it wants before attacking
    pub aggro: usize,
    pub mods: Tweak,
}

#[allow(clippy::too_many_arguments)]
const fn foe(name: &'static str, sprite: usize, desc: &'static str, hp: i32, power: i32, elements: &'static [Element], aggro: usize, mods: Tweak) -> EnemyDef {
    EnemyDef { name, sprite, desc, hp, power, elements, aggro, mods }
}

// Every enemy bends one rule, so each asks for a different answer.
pub static NORMALS: &[EnemyDef] = &[
    foe("Mire Hound", HD + 3, "Grows fiercer: +1 power every turn.", 16, 1, &[Tide, Stone], 2, |m| m.enrage = 1),
    foe("Cinder Imp", HD + 4, "Gains a stack whenever you hit a mine.", 12, 2, &[Ember, Spark], 2, |m| m.foe_mine_gift = 1),
    foe("Gust Wisp", HD + 5, "Slippery: the first rite each turn only grazes it, for half damage.", 12, 1, &[Gale, Spark], 3, |m| {
        m.evade = 1;
        m.graze = 1
    }),
    foe("Pebble Golem", HD + 6, "Stone hide: every hit deals 2 less.", 20, 1, &[Stone, Ember], 3, |m| m.armor = 2),
    foe("Brine Leech", HD + 7, "Heals for half the damage it deals.", 16, 1, &[Tide, Gale], 2, |m| m.lifesteal_pct = 50),
    foe("Static Moth", HD + 8, "Jams your surge at 2.", 12, 2, &[Spark, Tide], 3, |m| m.foe_surge_cap = 2),
    foe("Thorn Creeper", HD + 9, "Every rite that hits it deals 2 back.", 18, 1, &[Stone, Gale], 3, |m| m.thorns = 2),
    foe("Ash Rat", HD + 10, "Snatches one of your stacks as its turn starts.", 10, 2, &[Ember, Gale], 2, |m| m.pilfer = 1),
];

pub static ELITES: &[EnemyDef] = &[
    foe("The Quarrymaster", HD + 11, "Starts with 8 shield. Every hit deals 1 less.", 45, 2, &[Stone, Ember, Spark], 4, |m| {
        m.start_shield = 8;
        m.armor = 1
    }),
    foe("Gale Duchess", HD + 12, "One extra reveal. The first rite each turn only grazes her, for half damage.", 38, 2, &[Gale, Tide, Spark], 4, |m| {
        m.reveals = 1;
        m.evade = 1;
        m.graze = 1
    }),
    foe("Kiln Tyrant", HD + 13, "Its rites set you burning, and it gains +1 power every turn.", 42, 2, &[Ember, Gale, Stone], 3, |m| {
        m.rite_burn = 1;
        m.burn_bonus = 1;
        m.enrage = 1
    }),
];

/// Two per floor; each map rolls which one waits at the end.
pub static BOSSES: &[[EnemyDef; 2]] = &[
    [
        foe("Minelayer Queen", HD + 14, "1 extra mine in your boards. Gains a stack when you hit one.", 50, 2, &[Ember, Stone, Spark], 4, |m| {
            m.foe_mines = 1;
            m.foe_mine_gift = 1
        }),
        foe("The Sapper King", HD + 15, "Starts with 6 shield. Snatches one of your stacks each turn.", 46, 2, &[Ember, Gale, Spark], 4, |m| {
            m.start_shield = 6;
            m.pilfer = 1
        }),
    ],
    [
        foe("The Undertow", HD + 16, "Each turn: heals 2 and gains 2 shield.", 105, 3, &[Tide, Gale, Stone], 4, |m| {
            m.regen = 2;
            m.shield_turn = 2
        }),
        foe("The Tidecaller", HD + 17, "The first rite each turn only grazes her, for half damage. Heals for 30% of damage dealt.", 96, 3, &[Tide, Spark, Gale], 4, |m| {
            m.evade = 1;
            m.graze = 1;
            m.lifesteal_pct = 30
        }),
    ],
    [
        foe("Null Cartographer", HD + 18, "One extra reveal, 3 mines pre-flagged. Jams your surge at 3.", 135, 4, &[Spark, Gale, Ember], 5, |m| {
            m.reveals = 1;
            m.scouts = 3;
            m.foe_surge_cap = 3
        }),
        foe("The Deep Engine", HD + 19, "Iron plating: every hit deals 2 less. +1 power every turn.", 125, 3, &[Stone, Ember, Tide], 5, |m| {
            m.armor = 2;
            m.enrage = 1
        }),
    ],
];

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
        text: "The old workings near the surface. Most of the mines here are rusted half shut.",
        rules: &["No extra rules."],
        hp_pct: 100,
        me: NOTHING,
        foe: NOTHING,
        boss: NOTHING,
        curses: false,
    },
    Expedition {
        name: "Flooded Caverns",
        text: "Groundwater found the lower tunnels first. Things down here grew fat on it.",
        rules: &["Enemies have 10% more HP."],
        hp_pct: 110,
        me: NOTHING,
        foe: NOTHING,
        boss: NOTHING,
        curses: false,
    },
    Expedition {
        name: "Ember Depths",
        text: "The rock is warm to the touch, and the bosses here have learned to burn hotter.",
        rules: &["Enemies have 10% more HP.", "Bosses gain +1 power every 2 turns."],
        hp_pct: 110,
        me: NOTHING,
        foe: NOTHING,
        boss: |m| m.slow_enrage += 1,
        curses: false,
    },
    Expedition {
        name: "Crystal Hollows",
        text: "Caves of glass. Every step rings, and the boards hide more than they show.",
        rules: &["Enemies have 15% more HP.", "Bosses gain +1 power every 2 turns.", "1 fewer reveal each turn."],
        hp_pct: 115,
        me: |m| m.reveals -= 1,
        foe: NOTHING,
        boss: |m| m.slow_enrage += 1,
        curses: false,
    },
    Expedition {
        name: "The Rift",
        text: "A crack in the world, below the deepest shaft. Nothing here is kind.",
        rules: &["Every law of Crystal\u{a0}Hollows.", "Every floor rolls a curse.", "Rites cost 1 more gem, any kind."],
        hp_pct: 115,
        me: |m| {
            m.reveals -= 1;
            m.toll += 1
        },
        foe: NOTHING,
        boss: |m| m.slow_enrage += 1,
        curses: true,
    },
    Expedition {
        name: "The Null",
        text: "Where the maps stop. Whoever drew them never came back to finish.",
        rules: &["Every law of The\u{a0}Rift.", "Enemies have 25% more HP and +1 power."],
        hp_pct: 125,
        me: |m| {
            m.reveals -= 1;
            m.toll += 1
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
    twist("Shallow Seams", 1, "1 fewer mine on each of your boards.", |m| m.fewer_mines += 1, NOTHING),
    twist("Tailwind", 1, "Begin each turn with 1 surge.", |m| m.surge_start += 1, NOTHING),
    twist("Second Wind", 1, "Heal 6 whenever a new wave steps in.", |m| m.heal_after += 6, NOTHING),
    twist("Gold Dust", 1, "+50% shards from battles.", |m| m.shard_pct += 50, NOTHING),
    twist("Fault Lines", -1, "Mines deal 1 more damage to you.", |m| m.mine_guard -= 1, NOTHING),
    twist("Iron Hides", -1, "Enemies start each wave with 6 shield.", NOTHING, |m| m.start_shield += 6),
    twist("Restless Dead", -1, "Enemies have +1 power.", NOTHING, |m| m.power += 1),
    twist("Thick Fog", -1, "1 fewer reveal each turn.", |m| m.reveals -= 1, NOTHING),
    twist("Brittle Ground", 0, "1 more mine on every board, yours and theirs.", |m| m.fewer_mines -= 1, |m| m.fewer_mines -= 1),
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
            pick("Stoke the fire. (-10 shards, +5 max HP)", &[Shards(-10), MaxHp(5)]),
            LEAVE,
        ],
    },
    Encounter {
        title: "Flooded Gallery",
        text: "Black water fills the tunnel to the waist. Something shiny rests at the deep end.",
        choices: &[
            pick("Wade along the edge. (-3 max HP, +25 shards)", &[MaxHp(-3), Shards(25)]),
            pick("Swim for it. (40%: gain a pact, else -5 max HP)", &[Chance(40, &[GainCharm], &[Text("Cold, dark, empty."), MaxHp(-5)])]),
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
