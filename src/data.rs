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

impl Element {
    pub fn name(self) -> &'static str {
        ["Ember", "Tide", "Gale", "Stone", "Spark"][self as usize]
    }
}

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
    // skills
    pub strike_bonus: i32,
    pub strike_burn: i32,
    pub first_pct: i32,
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
    pub pilfer: i32,
    pub enrage: i32,
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

pub enum Effect {
    Damage(i32),
    Burn(i32),
    Shield(i32),
    Heal(i32),
    Weaken(i32),
    Drain(usize),
    Scout(usize),
    Pierce,
}
use Effect::*;

pub struct Combo {
    pub name: &'static str,
    pub needs: &'static [Element],
    pub effects: &'static [Effect],
    pub desc: &'static str,
}

const fn combo(name: &'static str, needs: &'static [Element], effects: &'static [Effect], desc: &'static str) -> Combo {
    Combo { name, needs, effects, desc }
}

pub static COMBOS: &[Combo] = &[
    combo("Steam Burst", &[Ember, Tide], &[Damage(6)], "+6 damage"),
    combo("Wildfire", &[Ember, Gale], &[Burn(3)], "Burn foe for 3 turns"),
    combo("Magma", &[Ember, Stone], &[Pierce, Damage(3)], "Shatter foe's shield, +3 damage"),
    combo("Plasma", &[Ember, Spark], &[Damage(7)], "+7 damage"),
    combo("Mistveil", &[Tide, Gale], &[Heal(4), Damage(3)], "Heal 4, +3 damage"),
    combo("Bulwark", &[Tide, Stone], &[Shield(7)], "Gain 7 shield"),
    combo("Conduction", &[Tide, Spark], &[Drain(2), Damage(3)], "Foe loses 2 stacks, +3 damage"),
    combo("Sandstorm", &[Gale, Stone], &[Weaken(2)], "Foe's attacks halved, 2 turns"),
    combo("Thunderclap", &[Gale, Spark], &[Scout(2), Damage(2)], "Flag 2 of your mines, +2 damage"),
    combo("Lodestone", &[Stone, Spark], &[Shield(4), Damage(4)], "+4 damage, gain 4 shield"),
    combo("Eruption", &[Ember, Gale, Stone], &[Damage(10)], "+10 damage"),
    combo("Tempest", &[Tide, Gale, Spark], &[Damage(6), Heal(2)], "+6 damage, heal 2"),
    combo("Geode Surge", &[Tide, Stone, Spark], &[Damage(6), Shield(6)], "+6 damage, gain 6 shield"),
    combo("Forgefire", &[Ember, Stone, Spark], &[Damage(8)], "+8 damage"),
    combo("Maelstrom", &[Tide, Gale, Stone], &[Shield(6), Heal(4)], "Gain 6 shield, heal 4"),
    combo("Short Circuit", &[Ember, Gale, Spark], &[Damage(6), Drain(1)], "+6 damage, foe loses 1 stack"),
];

/// Only the combos a fighter can actually make with its elements.
pub fn combos_for(elements: &[Element]) -> Vec<&'static Combo> {
    COMBOS.iter().filter(|c| c.needs.iter().all(|e| elements.contains(e))).collect()
}

pub struct Hero {
    pub name: &'static str,
    pub hp: i32,
    pub power: i32,
    pub elements: [Element; 3],
    pub passive: Tweak,
    pub passive_desc: &'static str,
}

pub static HEROES: &[Hero] = &[
    Hero {
        name: "Ashwarden",
        hp: 66,
        power: 3,
        elements: [Ember, Gale, Stone],
        passive: |m| m.mine_guard += 2,
        passive_desc: "Thick soles: mines deal 2 less damage.",
    },
    Hero {
        name: "Stormreader",
        hp: 62,
        power: 3,
        elements: [Tide, Gale, Spark],
        passive: |m| m.reveals += 1,
        passive_desc: "Quick read: +1 reveal each turn.",
    },
    Hero {
        name: "Deepforger",
        hp: 64,
        power: 3,
        elements: [Tide, Stone, Spark],
        passive: |m| m.start_shield += 5,
        passive_desc: "Plated: gain 5 shield at the start of each wave.",
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
    charm("Lucky Coin", Common, (101, PLAIN), "duds", "The first mine you hit each wave is a dud.", |m| m.duds += 1),
    charm("Dowsing Rod", Common, (130, PLAIN), "scouts", "2 mines start flagged on each of your boards.", |m| m.scouts += 2),
    charm("Ward Stone", Common, (102, PLAIN), "start_shield", "Gain 6 shield at the start of each wave.", |m| m.start_shield += 6),
    charm("Field Rations", Common, (114, PLAIN), "heal_after", "Heal 5 after every battle.", |m| m.heal_after += 5),
    charm("Magpie Purse", Common, (89, PLAIN), "shard_pct", "+50% shards from battles.", |m| m.shard_pct += 50),
    charm("Head Start", Common, (91, PLAIN), "start_stacks", "Begin each battle with 2 random stacks.", |m| m.start_stacks += 2),
    charm("Bramble Coat", Common, (62, 0x7fd17a), "thorns", "Enemy skills that hit you cost them 2 HP.", |m| m.thorns += 2),
    charm("Saboteur Kit", Common, (41, PLAIN), "foe_mines", "+2 mines on every enemy board.", |m| m.foe_mines += 2),
    charm("Sweeper's Map", Common, (66, PLAIN), "fewer_mines", "1 fewer mine on each of your boards.", |m| m.fewer_mines += 1),
    charm("Shell Fragment", Common, (102, 0x9ad0ff), "shield_turn", "Gain 2 shield at the start of your turn.", |m| m.shield_turn += 2),
    charm("Whetted Shard", Common, (103, PLAIN), "strike_bonus", "Strikes deal +3 damage.", |m| m.strike_bonus += 3),
    charm("Kindling", Common, (29, PLAIN), "strike_burn", "Strikes set the enemy burning for 2 turns.", |m| m.strike_burn += 2),
    charm("Slow Fuse", Common, (125, 0xff9a5e), "burn_bonus", "Your burns deal +1 damage per turn.", |m| m.burn_bonus += 1),
    charm("Sap Tap", Common, (126, PLAIN), "claim_heal", "Each crystal you claim heals you for 2.", |m| m.claim_heal += 2),
    charm("Crystal Skin", Common, (56, 0x9ad0ff), "claim_shield", "Each crystal you claim gives 3 shield.", |m| m.claim_shield += 3),
    charm("Plate Scraps", Common, (74, PLAIN), "armor", "Every hit on you deals 1 less.", |m| m.armor += 1),
    charm("Heart Root", Common, (115, PLAIN), "max_hp", "+12 max HP.", |m| m.max_hp += 12),
    // rares change how a turn is played
    charm("Quick Fingers", Rare, (60, 0xffd76a), "reveals", "+1 reveal each turn.", |m| m.reveals += 1),
    charm("Resonator", Rare, (31, PLAIN), "combo_pct", "Combo effects are 50% stronger.", |m| m.combo_pct += 50),
    charm("Leech Fang", Rare, (120, PLAIN), "lifesteal_pct", "Heal for 20% of the damage your skills deal.", |m| m.lifesteal_pct += 20),
    charm("Ambush Wire", Rare, (131, PLAIN), "opening", "Deal 6 damage at the start of each wave.", |m| m.opening += 6),
    charm("Running Start", Rare, (128, PLAIN), "surge_start", "Begin each turn with 2 surge.", |m| m.surge_start += 2),
    charm("Overcharger", Rare, (129, PLAIN), "surge_pct", "Each surge step is worth +30% instead of +20%.", |m| m.surge_pct += 10),
    charm("Steady Hands", Rare, (113, PLAIN), "keep_surge", "Surge carries over between your turns. Mines still wipe it.", |m| m.keep_surge += 1),
    charm("Opening Gambit", Rare, (104, PLAIN), "first_pct", "Your first skill each turn deals +60%.", |m| m.first_pct += 60),
    charm("Resonant Shards", Rare, (56, 0xffd84a), "claim_zap", "Each crystal you claim zaps the enemy for 2.", |m| m.claim_zap += 2),
    charm("Salvager", Rare, (117, PLAIN), "salvage", "Crystals you break with a mine still pay out.", |m| m.salvage += 1),
    charm("Spite Fuse", Rare, (12, 0xff9a5e), "mine_spite", "Mines you hit also deal 4 damage to the enemy.", |m| m.mine_spite += 4),
    charm("Hourglass", Rare, (127, 0xffd76a), "reveal_shield", "Each reveal you leave unused when your turn ends becomes 2 shield.", |m| m.reveal_shield += 2),
    charm("Hoarder's Sack", Rare, (90, PLAIN), "hoard_shield", "End your turn holding 3 or more stacks: gain 1 shield per stack.", |m| m.hoard_shield += 1),
    charm("Chain Lightning", Rare, (129, 0xffd84a), "chain", "Each skill after your first in a turn deals +3 more than the one before.", |m| m.chain += 3),
    charm("Powder Keg", Rare, (82, 0xff9a5e), "mine_no_end", "Mines no longer end your turn. They still hurt and wipe surge.", |m| m.mine_no_end += 1),
    // legendaries bend a core rule
    charm("Twin Vein", Legendary, (56, 0xff8ad8), "claim_bonus", "Crystals give 2 stacks, but your boards hold 2 more mines.", |m| {
        m.claim_bonus += 1;
        m.fewer_mines -= 2
    }),
    charm("Glass Blade", Legendary, (106, 0xb8f0ff), "power", "+2 power, but mines deal 2 more damage to you.", |m| {
        m.power += 2;
        m.mine_guard -= 2
    }),
    charm("Smoke Bomb", Legendary, (121, PLAIN), "evade", "The first enemy skill each turn misses you.", |m| m.evade += 1),
    charm("Sixth Sense", Legendary, (19, PLAIN), "sense", "The first mine you flip each turn is flagged instead of going off.", |m| m.sense += 1),
    charm("Echo Chamber", Legendary, (32, PLAIN), "echo", "Your three-element combos fire twice.", |m| m.echo += 1),
    charm("Berserker's Axe", Legendary, (118, PLAIN), "desperate", "Below half HP, +3 power.", |m| m.desperate += 3),
];

pub struct EnemyDef {
    pub name: &'static str,
    pub desc: &'static str,
    pub hp: i32,
    pub power: i32,
    pub elements: &'static [Element],
    /// stacks it wants before attacking
    pub aggro: usize,
    pub mods: Tweak,
}

const fn foe(
    name: &'static str,
    desc: &'static str,
    hp: i32,
    power: i32,
    elements: &'static [Element],
    aggro: usize,
    mods: Tweak,
) -> EnemyDef {
    EnemyDef { name, desc, hp, power, elements, aggro, mods }
}

// Every enemy bends one rule, so each asks for a different answer.
pub static NORMALS: &[EnemyDef] = &[
    foe("Mire Hound", "Grows fiercer: +1 power every turn.", 16, 1, &[Tide, Stone], 2, |m| m.enrage = 1),
    foe("Cinder Imp", "Gains a stack whenever you hit a mine.", 12, 2, &[Ember, Spark], 2, |m| m.foe_mine_gift = 1),
    foe("Gust Wisp", "Slippery: the first skill each turn misses it.", 12, 1, &[Gale, Spark], 3, |m| m.evade = 1),
    foe("Pebble Golem", "Stone hide: every hit deals 2 less.", 20, 1, &[Stone, Ember], 3, |m| m.armor = 2),
    foe("Brine Leech", "Heals for half the damage it deals.", 16, 1, &[Tide, Gale], 2, |m| m.lifesteal_pct = 50),
    foe("Static Moth", "Jams your surge at 2.", 12, 2, &[Spark, Tide], 3, |m| m.foe_surge_cap = 2),
    foe("Thorn Creeper", "Every skill that hits it deals 2 back.", 18, 1, &[Stone, Gale], 3, |m| m.thorns = 2),
    foe("Ash Rat", "Snatches one of your stacks as its turn starts.", 10, 2, &[Ember, Gale], 2, |m| m.pilfer = 1),
];

pub static ELITES: &[EnemyDef] = &[
    foe("The Quarrymaster", "Starts with 8 shield. Every hit deals 1 less.", 45, 2, &[Stone, Ember, Spark], 4, |m| {
        m.start_shield = 8;
        m.armor = 1
    }),
    foe("Gale Duchess", "One extra reveal. The first skill each turn misses her.", 38, 2, &[Gale, Tide, Spark], 4, |m| {
        m.reveals = 1;
        m.evade = 1
    }),
    foe("Kiln Tyrant", "Burns harder, and +1 power every turn.", 42, 2, &[Ember, Gale, Stone], 3, |m| {
        m.burn_bonus = 1;
        m.enrage = 1
    }),
];

/// One per floor, in order.
pub static BOSSES: &[EnemyDef] = &[
    foe("Minelayer Queen", "1 extra mine in your boards. Gains a stack when you hit one.", 50, 2, &[Ember, Stone, Spark], 4, |m| {
        m.foe_mines = 1;
        m.foe_mine_gift = 1
    }),
    foe("The Undertow", "Each turn: heals 2 and gains 2 shield.", 105, 3, &[Tide, Gale, Stone], 4, |m| {
        m.regen = 2;
        m.shield_turn = 2
    }),
    foe("Null Cartographer", "One extra reveal, 3 mines pre-flagged. Jams your surge at 3.", 135, 4, &[Spark, Gale, Ember], 5, |m| {
        m.reveals = 1;
        m.scouts = 3;
        m.foe_surge_cap = 3
    }),
];

pub enum Out {
    Hp(i32),
    HealPct(i32),
    MaxHp(i32),
    Shards(i32),
    GainCharm,
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

pub static REST: Encounter = Encounter {
    title: "Quiet Hollow",
    text: "A dry alcove with no tiles to step on. For once, nothing here wants to explode.",
    choices: &[pick("Sleep. (Heal 30% HP)", &[HealPct(30)]), pick("Train. (+5 max HP)", &[MaxHp(5)])],
};

pub static ENCOUNTERS: &[Encounter] = &[
    Encounter {
        title: "The Humming Crate",
        text: "A sealed crate sits in the middle of the path, humming a tune you almost recognise.",
        choices: &[
            pick("Pry it open. (-6 HP, gain a charm)", &[Hp(-6), GainCharm]),
            pick("Sell it unopened. (+25 shards)", &[Shards(25)]),
            LEAVE,
        ],
    },
    Encounter {
        title: "Wandering Tinker",
        text: "A tinker with far too many pockets offers you a deal, then a different deal.",
        choices: &[
            pick("Buy a trinket. (-30 shards, gain a charm)", &[Shards(-30), GainCharm]),
            pick("Sell some blood. (-8 HP, +30 shards)", &[Hp(-8), Shards(30)]),
            LEAVE,
        ],
    },
    Encounter {
        title: "Collapsed Tunnel",
        text: "Rubble blocks the way. Something glitters underneath it.",
        choices: &[
            pick("Dig. (50%: +40 shards, else -8 HP)", &[Chance(50, &[Shards(40)], &[Text("The ceiling answers."), Hp(-8)])]),
            pick("Go around.", &[Text("The long way is only long.")]),
        ],
    },
    Encounter {
        title: "Shrine of Small Mercies",
        text: "A tiny shrine, swept clean. Someone still looks after it.",
        choices: &[
            pick("Pray. (Heal 15 HP)", &[Hp(15)]),
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
            pick("Take its satchel. (+30 shards, -5 HP)", &[Shards(30), Hp(-5)]),
        ],
    },
    Encounter {
        title: "Glowing Vein",
        text: "A seam of bright ore runs along the wall, warm to the touch.",
        choices: &[
            pick("Mine it. (+35 shards, 40%: -10 HP)", &[Shards(35), Chance(40, &[Text("The vein bites back."), Hp(-10)], &[])]),
            pick("Reach inside. (50%: charm, else -12 HP)", &[Chance(50, &[GainCharm], &[Text("Something reaches back."), Hp(-12)])]),
            LEAVE,
        ],
    },
    Encounter {
        title: "Deserter's Camp",
        text: "A lone deserter keeps a small fire going and eyes your pack.",
        choices: &[
            pick("Share the fire. (Heal 10 HP)", &[Hp(10)]),
            pick("Rob the camp. (+30 shards, -4 max HP)", &[Shards(30), MaxHp(-4)]),
        ],
    },
    Encounter {
        title: "Echoing Well",
        text: "Your footsteps come back from the well a moment too late.",
        choices: &[
            pick("Throw in 10 shards. (60%: gain a charm)", &[Shards(-10), Chance(60, &[GainCharm], &[Text("Only an echo.")])]),
            pick("Drink. (Heal 8 HP)", &[Hp(8)]),
        ],
    },
    Encounter {
        title: "Unstable Ground",
        text: "The floor ahead is cracked like old glaze.",
        choices: &[
            pick("Sprint across. (-4 HP)", &[Hp(-4)]),
            pick("Tiptoe. (70%: +10 shards, else -12 HP)", &[Chance(70, &[Shards(10)], &[Text("Crack."), Hp(-12)])]),
        ],
    },
];

/// Tile number in the Tiny Dungeon sprite sheet for a hero or enemy.
pub fn sprite(name: &str) -> usize {
    match name {
        "Ashwarden" => 87,
        "Stormreader" => 84,
        "Deepforger" => 86,
        "Mire Hound" => 123,
        "Cinder Imp" => 110,
        "Gust Wisp" => 121,
        "Pebble Golem" => 109,
        "Brine Leech" => 108,
        "Static Moth" => 120,
        "Thorn Creeper" => 122,
        "Ash Rat" => 124,
        "The Quarrymaster" => 100,
        "Gale Duchess" => 99,
        "Kiln Tyrant" => 88,
        "Minelayer Queen" => 92,
        "The Undertow" => 112,
        "Null Cartographer" => 111,
        _ => 96,
    }
}
