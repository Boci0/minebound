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
    combo("Mistveil", &[Tide, Gale], &[Heal(6), Damage(2)], "Heal 6, +2 damage"),
    combo("Bulwark", &[Tide, Stone], &[Shield(7)], "Gain 7 shield"),
    combo("Conduction", &[Tide, Spark], &[Drain(2), Damage(3)], "Foe loses 2 stacks, +3 damage"),
    combo("Sandstorm", &[Gale, Stone], &[Weaken(2)], "Foe's attacks halved, 2 turns"),
    combo("Thunderclap", &[Gale, Spark], &[Scout(2), Damage(2)], "Flag 2 of your mines, +2 damage"),
    combo("Lodestone", &[Stone, Spark], &[Shield(4), Damage(4)], "+4 damage, gain 4 shield"),
    combo("Eruption", &[Ember, Gale, Stone], &[Damage(10)], "+10 damage"),
    combo("Tempest", &[Tide, Gale, Spark], &[Damage(6), Heal(4)], "+6 damage, heal 4"),
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

pub struct Charm {
    pub name: &'static str,
    pub desc: &'static str,
    pub apply: Tweak,
}

const fn charm(name: &'static str, desc: &'static str, apply: Tweak) -> Charm {
    Charm { name, desc, apply }
}

pub static CHARMS: &[Charm] = &[
    charm("Quick Fingers", "+1 reveal each turn.", |m| m.reveals += 1),
    charm("Whetstone", "+1 power.", |m| m.power += 1),
    charm("Padded Boots", "Mines deal 1 less damage to you.", |m| m.mine_guard += 1),
    charm("Ward Stone", "Gain 6 shield at the start of each wave.", |m| m.start_shield += 6),
    charm("Resonator", "Combo effects are 50% stronger.", |m| m.combo_pct += 50),
    charm("Lucky Coin", "The first mine you hit each wave is a dud.", |m| m.duds += 1),
    charm("Field Rations", "Heal 5 after every battle.", |m| m.heal_after += 5),
    charm("Magpie Purse", "+50% shards from battles.", |m| m.shard_pct += 50),
    charm("Head Start", "Begin each battle with 2 random stacks.", |m| m.start_stacks += 2),
    charm("Slow Fuse", "Your burns deal +1 damage per turn.", |m| m.burn_bonus += 1),
    charm("Dowsing Rod", "2 mines start flagged on each of your boards.", |m| m.scouts += 2),
    charm("Bramble Coat", "Attackers take 2 damage.", |m| m.thorns += 2),
    charm("Saboteur Kit", "+2 mines on every enemy board.", |m| m.foe_mines += 2),
    charm("Sweeper's Map", "1 fewer mine on each of your boards.", |m| m.fewer_mines += 1),
    charm("Leech Fang", "Heal for 25% of the damage your skills deal.", |m| m.lifesteal_pct += 25),
    charm("Ambush Wire", "Deal 6 damage at the start of each wave.", |m| m.opening += 6),
    charm("Shell Fragment", "Gain 2 shield at the start of your turn.", |m| m.shield_turn += 2),
    charm("Glass Blade", "+2 power, but mines deal 2 more damage to you.", |m| {
        m.power += 2;
        m.mine_guard -= 2
    }),
    charm("Heart Root", "+12 max HP.", |m| m.max_hp += 12),
    // build-around charms: each one rewards a different way of playing a turn
    charm("Running Start", "Begin each turn with 2 surge.", |m| m.surge_start += 2),
    charm("Overcharger", "Each surge step is worth +30% instead of +20%.", |m| m.surge_pct += 10),
    charm("Steady Hands", "Surge carries over between your turns. Mines still wipe it.", |m| m.keep_surge += 1),
    charm("Whetted Shard", "Strikes deal +3 damage.", |m| m.strike_bonus += 3),
    charm("Kindling", "Strikes set the enemy burning for 2 turns.", |m| m.strike_burn += 2),
    charm("Opening Gambit", "Your first skill each turn deals +60%.", |m| m.first_pct += 60),
    charm("Resonant Shards", "Each crystal you claim zaps the enemy for 2.", |m| m.claim_zap += 2),
    charm("Sap Tap", "Each crystal you claim heals you for 2.", |m| m.claim_heal += 2),
    charm("Crystal Skin", "Each crystal you claim gives 3 shield.", |m| m.claim_shield += 3),
    charm("Twin Vein", "Crystals give 2 stacks, but your boards hold 2 more mines.", |m| {
        m.claim_bonus += 1;
        m.fewer_mines -= 2
    }),
    charm("Salvager", "Crystals you break with a mine still pay out.", |m| m.salvage += 1),
    charm("Spite Fuse", "Mines you hit also deal 4 damage to the enemy.", |m| m.mine_spite += 4),
    charm("Plate Scraps", "Every hit on you deals 1 less damage.", |m| m.armor += 1),
    charm("Smoke Bomb", "The first enemy skill each turn misses you.", |m| m.evade += 1),
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
    foe("Minelayer Queen", "2 extra mines in your boards. Gains a stack when you hit one.", 58, 2, &[Ember, Stone, Spark], 4, |m| {
        m.foe_mines = 2;
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
