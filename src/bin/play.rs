//! Text-mode play, for playtesting without screenshots: the game's own rules, driven by typed commands.
//!
//!   cargo run --bin play -- SEED HERO EXP COMMANDS_FILE [N]
//!
//! Every call replays the whole run from SEED with the commands in the file (one per line, or split by
//! `;`), so the same file always gives the same game, then prints what the last command did and where
//! the run stands. Append a command to the file and run again to play on. `help` lists the commands.
//! HERO and EXP are indexes (0 = Ashwarden, 0 = Shallow Mines).

#![allow(dead_code)]

#[path = "../battle.rs"]
mod battle;
#[path = "../board.rs"]
mod board;
#[path = "../data.rs"]
mod data;
#[path = "../devlog.rs"]
mod devlog;
#[path = "../rng.rs"]
mod rng;
#[path = "../run.rs"]
mod run;
#[path = "../save.rs"]
mod save;

use battle::{Battle, Event};
use data::*;
use run::{Run, Shop, Stage};

const HELP: &str = "\
battle:  mine C R | vent C R | board K | cast NAME | end   (C, R from 0; K from 1; NAME may be a prefix)
map:     go K
pacts:   pick K            upgrade: up K            blessing: bless K | keep
brand:   brand K RITE      learn: learn K | learn K drop RITE | none
shop:    buy K | buyrite | buyrite drop RITE | buybrand RITE | lesson RITE | restock | leave
event:   choose K | choose K give PACT
any:     look (just show the state again), info NAME (what a rite does)";

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 5 {
        println!("usage: play SEED HERO EXP COMMANDS_FILE\n{HELP}");
        return;
    }
    let num = |i: usize| args[i].parse::<u64>().expect("a number");
    let text = std::fs::read_to_string(&args[4]).unwrap_or_default();
    let cmds: Vec<&str> = text.lines().flat_map(|l| l.split(';')).map(str::trim).filter(|c| !c.is_empty() && !c.starts_with('#')).collect();
    let (mut run, mut stage) = Run::new(num(1), num(2) as usize, num(3) as usize);
    // N (default 1): report what each of the last N commands did, for when several were appended at once
    let last: usize = args.get(5).and_then(|a| a.parse().ok()).unwrap_or(1);
    let mut said = vec![];
    for (n, c) in cmds.iter().enumerate() {
        let mut out = vec![];
        let r = step(&mut run, &mut stage, c, &mut out);
        if n + last >= cmds.len() {
            said.push(format!("> {c}"));
            said.extend(out);
        }
        if let Err(e) = r {
            said.push(format!("command {} `{c}` refused: {e}\n(the commands after it were not played)", n + 1));
            break;
        }
    }
    for s in &said {
        println!("{s}");
    }
    println!("\n{}", show(&run, &stage));
}

/// Plays one command. Errors leave the game as it was.
fn step(run: &mut Run, stage: &mut Stage, cmd: &str, said: &mut Vec<String>) -> Result<(), String> {
    let w: Vec<&str> = cmd.split_whitespace().collect();
    let arg = |i: usize| w.get(i).ok_or_else(|| format!("`{}` needs more", w[0])).and_then(|s| s.parse::<usize>().map_err(|_| format!("`{s}` is not a number")));
    let rest = |i: usize| w[i.min(w.len())..].join(" ");
    match (w[0], &mut *stage) {
        ("look" | "help", _) => {
            if w[0] == "help" {
                said.push(HELP.into());
            }
            return Ok(());
        }
        ("info", _) => {
            let c = find_combo(&rest(1)).ok_or("no rite by that name")?;
            said.push(format!("{}: needs {:?}; {}", c.name, c.needs, describe(c)));
            return Ok(());
        }
        (_, Stage::Battle(b)) => {
            let ok = match w[0] {
                "mine" => b.dig(arg(2)? * 6 + arg(1)?),
                "vent" => b.vent(arg(2)? * 6 + arg(1)?),
                "board" => {
                    b.swap(arg(1)?.checked_sub(1).ok_or("boards count from 1")?);
                    true
                }
                "cast" => {
                    let name = rest(1).to_lowercase();
                    let c = b.f[0].rite_list().into_iter().find(|c| c.name.to_lowercase().starts_with(&name)).ok_or("you hold no rite by that name")?;
                    if b.f[0].payment(c, &b.f[0].stacks).is_none() {
                        return Err(format!("your gems don't pay for {} (needs {:?})", c.name, b.f[0].cost(c)));
                    }
                    if b.wasted(0, c) {
                        return Err("full HP: a heal would do nothing".into());
                    }
                    b.cast(c)
                }
                "end" => {
                    b.end_turn();
                    true
                }
                _ => return Err(format!("not a battle command; {HELP}")),
            };
            if !ok {
                return Err("the game refused it (no picks left, not a live crystal, or not your turn)".into());
            }
            tell(b, said);
            // the enemy plays its whole turn, then it is yours again
            let mut guard = 0;
            while b.active == 1 && !b.over() && guard < 500 {
                b.ai_step();
                tell(b, said);
                guard += 1;
            }
            if b.over() {
                said.push(if b.won() { "== battle won ==".into() } else { "== you fell ==".into() });
                let b = std::mem::replace(b, placeholder());
                *stage = run.finish_battle(&b);
            }
        }
        ("go", Stage::Map) => {
            let k = arg(1)?;
            let (c, r) = *run.choices().get(k).ok_or("no such room")?;
            *stage = run.enter(c, r);
        }
        ("pick", Stage::Pick(options)) => {
            let id = *options.get(arg(1)?).ok_or("no such pact")?;
            run.add_charm(id);
            *stage = run.after_node();
        }
        ("up", Stage::Upgrade(ids)) => {
            let id = *ids.get(arg(1)?).ok_or("no such rite")?;
            *stage = run.upgrade(id);
        }
        ("bless", Stage::Blessing(offers)) => {
            let b = *offers.get(arg(1)?).ok_or("no such blessing")?;
            *stage = run.take_blessing(Some(b));
        }
        ("keep", Stage::Blessing(_)) => *stage = run.take_blessing(None),
        ("brand", Stage::Brand(offers)) => {
            let brand = *offers.get(arg(1)?).ok_or("no such brand")?;
            let id = held(run, &rest(2))?;
            *stage = run.burn_brand(id, brand);
        }
        ("learn", Stage::Learn(offers)) => {
            let id = *offers.get(arg(1)?).ok_or("no such rite")?;
            let drop = if w.get(2) == Some(&"drop") { Some(held(run, &rest(3))?) } else { None };
            if drop.is_none() && run.held.len() >= MAX_RITES {
                return Err(format!("you hold {MAX_RITES} rites: say which to let go, `learn K drop RITE`"));
            }
            *stage = run.learn(Some((id, drop)));
        }
        ("none", Stage::Learn(_)) => *stage = run.learn(None),
        (_, Stage::Shop(shop)) => {
            let ok = match w[0] {
                "buy" => run.buy(shop, arg(1)?),
                "buyrite" => {
                    let drop = if w.get(1) == Some(&"drop") { Some(held(run, &rest(2))?) } else { None };
                    run.buy_rite(shop, drop)
                }
                "buybrand" => {
                    let id = held(run, &rest(1))?;
                    run.buy_brand(shop, id)
                }
                "lesson" => {
                    let id = held(run, &rest(1))?;
                    run.buy_lesson(shop, id)
                }
                "restock" => run.restock(shop),
                "leave" => {
                    let s = std::mem::replace(shop, empty_shop());
                    *stage = run.leave_shop(&s);
                    true
                }
                _ => return Err(format!("not a shop command; {HELP}")),
            };
            if !ok {
                return Err("the shop refused it (not enough shards, sold out, or no room)".into());
            }
        }
        ("choose", Stage::Event(e, _)) => {
            let shown: Vec<&Choice> = e.choices.iter().filter(|c| run.shows(c)).collect();
            let c = *shown.get(arg(1)?).ok_or("no such choice")?;
            if !run.can_afford(c) {
                return Err("you can't afford that choice".into());
            }
            let give = if w.get(2) == Some(&"give") {
                let name = rest(3).to_lowercase();
                Some(run.charms.iter().copied().find(|&i| CHARMS[i].name.to_lowercase().starts_with(&name)).ok_or("you hold no pact by that name")?)
            } else {
                None
            };
            said.push(run.choose(c, give));
            *stage = run.after_node();
        }
        (_, Stage::End(_)) => return Err("the run is over".into()),
        _ => return Err(format!("`{}` doesn't fit this screen; {HELP}", w[0])),
    }
    Ok(())
}

/// What just happened in battle, one line per thing worth knowing.
fn tell(b: &mut Battle, said: &mut Vec<String>) {
    let who = |s: usize| if s == 0 { "you" } else { "foe" };
    for e in std::mem::take(&mut b.events) {
        let line = match e {
            Event::Cast { from, name, miss, .. } => format!("{} cast {name}{}", who(from), if miss { " (missed)" } else { "" }),
            Event::Cave { side } => format!("ground caved in on {}", who(side)),
            Event::Claim { side, element, tiles } => format!("{} mined {} {element:?} crystal(s)", who(side), tiles.len()),
            Event::Proc { side, hook } => format!("{}: {hook} took effect", who(side)),
            Event::Load => format!("foe loaded {:?}", b.f[1].loaded.iter().map(|c| c.name).collect::<Vec<_>>()),
            Event::Break => "you broke the foe's load: it fires for half".into(),
            Event::React { from, element } => format!("{} set off a {element:?} reaction", who(from)),
            Event::Brittle { side } => format!("{}'s brittle shield shattered back", who(side)),
            Event::Pierced { side } => format!("{}'s shield was shattered", who(side)),
            Event::Pass { hp, .. } => format!("-- turn passes (you {} HP, foe {} HP) --", hp[0], hp[1]),
            Event::Flip => continue,
        };
        said.push(line);
    }
}

fn show(run: &Run, stage: &Stage) -> String {
    let h = &HEROES[run.hero];
    let rites: Vec<String> = run
        .held
        .iter()
        .map(|&i| {
            let brands: Vec<&str> = run.brands.iter().filter(|b| b.0 == i).map(|b| BRANDS[b.1].name).collect();
            format!("{} L{}{}", COMBOS[i].name, run.level(i), if brands.is_empty() { String::new() } else { format!(" +{}", brands.join("+")) })
        })
        .collect();
    // in battle the run's HP is only brought up to date when the battle ends: the battle line has the real one
    let hp = if let Stage::Battle(b) = stage { b.f[0].hp } else { run.hp };
    let mut s = format!(
        "{} | floor {} | HP {}/{} | shards {} | rites {:?} | pacts {:?} | blessing {:?}\n",
        h.name, run.floor + 1, hp, run.max_hp(), run.shards, rites,
        run.charms.iter().map(|&c| CHARMS[c].name).collect::<Vec<_>>(), run.blessing.map(|b| BLESSINGS[b].name)
    );
    match stage {
        Stage::Map => {
            s += "MAP: choose a room with `go K`\n";
            for (k, (c, r)) in run.choices().iter().enumerate() {
                s += &format!("  {k}: {} (column {c})\n", run.map[*c][*r].kind.name());
            }
        }
        Stage::Battle(b) => s += &show_battle(b),
        Stage::Pick(o) => {
            s += "PICK A PACT (`pick K`):\n";
            for (k, &i) in o.iter().enumerate() {
                s += &format!("  {k}: {} ({:?}, {:?} set): {}\n", CHARMS[i].name, CHARMS[i].rarity, CHARMS[i].set, CHARMS[i].desc);
            }
        }
        Stage::Upgrade(ids) => {
            s += "UPGRADE A RITE (`up K`):\n";
            for (k, &i) in ids.iter().enumerate() {
                let (c, l) = (&COMBOS[i], run.level(i));
                s += &format!("  {k}: {} L{l} -> L{}: becomes {}, costs {:?} (was {:?})\n", c.name, l + 1, c.title(l + 1), c.cost(l + 1), c.cost(l));
            }
        }
        Stage::Blessing(o) => {
            s += "BLESSING (`bless K`, or `keep` yours):\n";
            for (k, &i) in o.iter().enumerate() {
                s += &format!("  {k}: {}: {}\n", BLESSINGS[i].name, BLESSINGS[i].desc);
            }
        }
        Stage::Brand(o) => {
            s += "BRAND (`brand K RITE`):\n";
            for (k, &i) in o.iter().enumerate() {
                s += &format!("  {k}: {} ({})\n", BRANDS[i].name, BRANDS[i].effect.text(1));
            }
        }
        Stage::Learn(o) => {
            s += &format!("LEARN A RITE (`learn K`, `learn K drop RITE` when holding {MAX_RITES}, or `none`):\n");
            for (k, &i) in o.iter().enumerate() {
                s += &format!("  {k}: {} needs {:?}: {}\n", COMBOS[i].name, COMBOS[i].needs, describe(&COMBOS[i]));
            }
        }
        Stage::Shop(shop) => {
            s += "SHOP:\n";
            for (k, id) in shop.stock.iter().enumerate() {
                if let Some(i) = id {
                    s += &format!("  buy {k}: {} for {}{}: {}\n", CHARMS[*i].name, Run::price(shop, k), if k == shop.sale { " (sale)" } else { "" }, CHARMS[*i].desc);
                }
            }
            if let Some(i) = shop.rite {
                s += &format!("  buyrite: {} for {} (needs {:?}): {}\n", COMBOS[i].name, Run::rite_price(shop), COMBOS[i].needs, describe(&COMBOS[i]));
            }
            if let Some(i) = shop.brand {
                s += &format!("  buybrand RITE: {} for {} ({})\n", BRANDS[i].name, Run::brand_price(shop), BRANDS[i].effect.text(1));
            }
            if shop.lesson {
                s += &format!("  lesson RITE: +1 level for {}\n", Run::lesson_price(shop));
            }
            s += &format!("  restock for {}{} | leave\n", Run::restock_price(shop), if run.can_restock(shop) { "" } else { " (can't)" });
        }
        Stage::Event(e, _) => {
            s += &format!("EVENT {}: {}\n", e.title, e.text);
            for (k, c) in e.choices.iter().filter(|c| run.shows(c)).enumerate() {
                s += &format!("  choose {k}: {}{}\n", c.label, if run.can_afford(c) { "" } else { " (can't afford)" });
            }
        }
        Stage::End(won) => s += &format!("RUN OVER: {}\n", if *won { "cleared" } else { "fallen" }),
    }
    s
}

fn show_battle(b: &Battle) -> String {
    let (me, foe) = (&b.f[0], &b.f[1]);
    let mut s = format!(
        "BATTLE turn {} wave {}/{} | {}\n  you: HP {}/{} shield {} power {} burn {} weak {} surge {} | picks left {} | gems {:?}\n",
        b.turn, b.wave, b.waves, if b.active == 0 { "your turn" } else { "foe's turn" },
        me.hp, me.max_hp, me.shield, me.power, me.burn, me.weak, me.surge, me.reveals_left, me.stacks
    );
    s += &format!(
        "  foe: {} — {} | HP {}/{} shield {} power {} burn {} weak {} | gems {:?} | loaded {:?} (break {}/{})\n",
        foe.name, foe.desc, foe.hp, foe.max_hp, foe.shield, foe.power, foe.burn, foe.weak, foe.stacks,
        foe.loaded.iter().map(|c| c.name).collect::<Vec<_>>(), foe.took, foe.break_at
    );
    s += "  casts (`cast NAME`):\n";
    for c in me.rite_list() {
        let lit = b.ready(0).iter().any(|x| std::ptr::eq(*x, c));
        let dmg = if b.hits(0, c) { format!(", {} dmg now", b.hit_damage(0, c)) } else { String::new() };
        s += &format!("    {} {} L{} costs {:?}{dmg}: {}\n", if lit { "*" } else { " " }, c.name, me.level(c), me.cost(c), me.describe(c));
    }
    s += &format!("  your boards (`board K`): {}\n", me.boards.iter().enumerate().map(|(k, bd)| format!("{}{} {:?}", if k == me.cur { ">" } else { " " }, k + 1, bd.element)).collect::<Vec<_>>().join("  "));
    s += &format!("  your {:?} board: C = crystal, digit = cracks (3 caves in), # = rock, x = caved, . = mined\n", me.board().element);
    s += &grid(&me.board().tiles);
    s += &format!("  foe's {:?} board:\n", foe.board().element);
    s += &grid(&foe.board().tiles);
    s
}

fn grid(tiles: &[board::Tile]) -> String {
    let mut s = "       0  1  2  3  4  5\n".to_string();
    for r in 0..6 {
        s += &format!("    {r} ");
        for c in 0..6 {
            let t = &tiles[r * 6 + c];
            s += &if t.rock {
                " ##".to_string()
            } else if t.caved {
                " xx".to_string()
            } else if t.crystal && !t.used {
                format!(" C{}", t.crack)
            } else if t.used {
                format!(" .{}", t.crack)
            } else {
                format!("  {}", t.crack)
            };
        }
        s += "\n";
    }
    s
}

fn describe(c: &Combo) -> String {
    c.effects.iter().map(|e| e.text(1)).chain(c.master.map(|m| format!("mastered: {}", m.text(3)))).collect::<Vec<_>>().join(", ")
}

fn find_combo(name: &str) -> Option<&'static Combo> {
    let n = name.to_lowercase();
    COMBOS.iter().find(|c| c.name.to_lowercase().starts_with(&n))
}

fn held(run: &Run, name: &str) -> Result<usize, String> {
    let n = name.to_lowercase();
    run.held.iter().copied().find(|&i| COMBOS[i].name.to_lowercase().starts_with(&n)).ok_or_else(|| format!("you hold no rite called `{name}`"))
}

fn placeholder() -> Battle {
    let f = battle::Fighter::new("", 0, "", 1, 1, 0, &[Element::Ember], Mods::default(), 0);
    Battle::new(f.clone(), vec![f], rng::Rng::new(0))
}

fn empty_shop() -> Shop {
    Shop { stock: vec![], sale: 0, restocks: 0, markup: 100, brand: None, lesson: false, rite: None }
}
