//! Bulk scripts: "give everything" and the flag unlocks.
//!
//! Item scripts feed a list of ids to the same item routine the Items screen
//! uses, one call per item. The id lists in `assets/scripts.tsv` are taken from
//! the table's own bulk entries (base game and DLC lists merged, duplicates
//! dropped), so a script here adds the same set its counterpart does.
//!
//! Flag scripts call the game's event-flag setter directly, which is how the
//! table's `SetEventFlag` stub does it: `rcx` = event flag manager, `rdx` =
//! pointer to the flag id, `r8b` = 1.

use std::collections::{BTreeMap, HashSet};

use crate::items::{Cat, Catalog};

pub const RAW: &str = include_str!("../assets/scripts.tsv");

/// One item to hand to the game: category, id, quantity, upgrade level.
pub type Give = (Cat, u32, u32, u32);

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum List {
    Weapons,
    WeaponsMax,
    Armor,
    Talismans,
    Crafting,
    Ashes,
    Arrows,
    Consumables,
    Reusables,
    Smithing,
    Maps,
    Cookbooks,
    Whetblades,
    Pouches,
    MemoryStones,
    Multiplayer,
    Physick,
    Perfume,
}

impl List {
    fn key(self) -> &'static str {
        match self {
            List::Weapons => "weapons",
            List::WeaponsMax => "weapons_max",
            List::Armor => "armor",
            List::Talismans => "talismans",
            List::Crafting => "crafting",
            List::Ashes => "ashes",
            List::Arrows => "arrows",
            List::Consumables => "consumables",
            List::Reusables => "reusables",
            List::Smithing => "smithing",
            List::Maps => "maps",
            List::Cookbooks => "cookbooks",
            List::Whetblades => "whetblades",
            List::Pouches => "pouches",
            List::MemoryStones => "memory_stones",
            List::Multiplayer => "multiplayer",
            List::Physick => "physick",
            List::Perfume => "perfume",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// A list straight out of `assets/scripts.tsv`.
    List(List),
    /// The spell list split by id: sorceries sit below 6000.
    Sorceries,
    Incantations,
    /// Every rune item in the catalog, one stack each.
    Runes,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Variant {
    Every,
    /// One entry per family: the highest `+N` variant.
    TopOnly,
}

#[derive(Clone, Copy)]
pub enum Kind {
    Items {
        source: Source,
        variant: Variant,
    },
    /// Set every grace flag ("bonfire" param list).
    UnlockGraces,
    /// Set every summoning-pool sign flag.
    SummoningPools,
    /// The table's boss list, written a byte at a time: `0xFF` marks the boss
    /// dead, `0x00` brings it back.
    Bosses { dead: bool },
    /// Torrent's unlock flags plus the whistle item itself.
    UnlockTorrent,
    /// Melina's invitation flags, which open Roundtable Hold.
    UnlockRoundtable,
    /// The map display switches plus every map piece flag.
    RevealMaps,
    /// Top both flasks up to fourteen charges.
    MaxFlaskNumber,
    /// Take both flasks to +12.
    MaxFlaskCharge,
}

/// The crafting kit's item and the flag that marks the crafting feature as
/// unlocked; the material list is useless without them.
pub const CRAFTING_KIT: u32 = 8_500;
pub const CRAFTING_KIT_FLAG: u32 = 60_120;

const CRAFTING_KIT_NEEDED: [(Cat, u32); 1] = [(Cat::Goods, CRAFTING_KIT)];

/// Items a script assumes you already own, given first when the inventory has
/// none.
pub fn prerequisites(script: &Script) -> &'static [(Cat, u32)] {
    match script.kind {
        Kind::Items {
            source: Source::List(List::Crafting),
            ..
        } => &CRAFTING_KIT_NEEDED,
        _ => &[],
    }
}

/// Byte offsets into `[[EventFlagMan]+0x28]`, taken from the table's
/// "Revive/Kill ALL Bosses" entry.
pub const BOSS_FLAGS: &str = include_str!("../assets/boss_flags.tsv");

pub fn boss_flag_offsets() -> Vec<u64> {
    BOSS_FLAGS
        .lines()
        .filter_map(|line| u64::from_str_radix(line.trim(), 16).ok())
        .collect()
}

/// Torrent: 60100 is the horse feature unlock — without it the game refuses to
/// summon him even with the whistle in the inventory — 4680 marks Melina's
/// hand-off as done, and the other two are the companion flags the whistle
/// normally sets alongside it.
pub const TORRENT_FLAGS: [u32; 4] = [60_100, 4_680, 4_681, 710_520];

/// The item the whistle itself lives in: goods 130, "Spectral Steed Whistle".
pub const WHISTLE: (Cat, u32, u32, u32) = (Cat::Goods, 130, 1, 0);

/// Roundtable Hold: Melina's invitation gate — the accord, the whistle hand-off
/// and the "rested at a grace outside Limgrave" step that makes her offer it.
pub const ROUNDTABLE_FLAGS: [u32; 5] = [4_680, 4_681, 3_062, 3_063, 3_064];

/// The flags that actually unlock the cooking recipes. Handing over the cookbook
/// items is only half of it: the recipe list is flag driven, which is why the
/// books appeared in the inventory and nothing could be cooked. Taken from the
/// table's own `Cookbook_Ids` list (59 entries, one typo and one repeat
/// removed).
pub const COOKBOOK_FLAGS: [u32; 58] = [
    67000, 67010, 67020, 67030, 67050, 67060, 67070, 67080, 67090, 67100,
    67110, 67120, 67130, 67200, 67210, 67220, 67230, 67250, 67260, 67270,
    67280, 67290, 67300, 67310, 67400, 67410, 67420, 67430, 67440, 67450,
    67460, 67470, 67480, 67600, 67610, 67630, 67640, 67650, 67830, 67840,
    67850, 67860, 67870, 67880, 67890, 67900, 67910, 67920, 68000, 68010,
    68020, 68030, 68200, 68210, 68220, 68230, 68400, 68410,
];

/// Items that need flags set as well as the item itself.
pub fn unlock_flags(script: &Script) -> &'static [u32] {
    match script.kind {
        Kind::Items {
            source: Source::List(List::Cookbooks),
            ..
        } => &COOKBOOK_FLAGS,
        _ => &[],
    }
}

/// Grace flags. The table reads these out of `BonfireWarpParam`; the flag ids
/// live in four ranges — 71xxx legacy dungeons, 72xxx the DLC, 73xxx caves and
/// catacombs, 76xxx the open world — so the same set is written from the ranges.
/// A flag that no grace uses is a spare byte and does nothing.
pub fn grace_flags() -> Vec<u32> {
    let mut flags = Vec::with_capacity(4_000);
    for range in [
        71_000..=71_999,
        72_000..=72_999,
        73_000..=73_999,
        76_000..=76_999,
    ] {
        flags.extend(range);
    }
    flags
}

/// Summoning pools: every martyr effigy flag lives in the 670xxx block.
pub fn summoning_pool_flags() -> Vec<u32> {
    (670_000..=670_999).collect()
}

/// Map reveal, in two layers: first the display switches (overworld,
/// underground, Shadow Realm), then every map piece flag — the same flags the
/// map fragments set when the game hands them over, which is why the fragments
/// alone can leave the map blank.
pub fn map_reveal_flags() -> Vec<u32> {
    let mut flags = vec![62_000, 62_001, 62_002, 82_001, 82_002];
    flags.extend(62_004..=62_012);
    flags.extend(62_020..=62_084);
    flags
}

#[derive(Clone, Copy)]
pub struct Script {
    pub name: &'static str,
    pub blurb: &'static str,
    pub kind: Kind,
}

pub struct Entry {
    pub cat: Cat,
    pub id: u32,
    pub qty: u32,
    /// Comes from one of the DLC-only lists.
    pub dlc: bool,
}

pub struct Library {
    lists: BTreeMap<&'static str, Vec<Entry>>,
}

impl Library {
    pub fn load() -> Library {
        let mut lists: BTreeMap<&'static str, Vec<Entry>> = BTreeMap::new();
        for line in RAW.lines() {
            let mut parts = line.split('\t');
            let (Some(key), Some(cat), Some(id), Some(qty)) = (
                parts.next(),
                parts.next(),
                parts.next(),
                parts.next(),
            ) else {
                continue;
            };
            let dlc = parts.next().map(|flag| flag.trim() == "1").unwrap_or(false);
            let key = match key {
                "weapons" => "weapons",
                "weapons_max" => "weapons_max",
                "armor" => "armor",
                "talismans" => "talismans",
                "crafting" => "crafting",
                "ashes" => "ashes",
                "arrows" => "arrows",
                "consumables" => "consumables",
                "reusables" => "reusables",
                "smithing" => "smithing",
                "maps" => "maps",
                "cookbooks" => "cookbooks",
                "whetblades" => "whetblades",
                "pouches" => "pouches",
                "memory_stones" => "memory_stones",
                "multiplayer" => "multiplayer",
                "physick" => "physick",
                "perfume" => "perfume",
                "magic" => "magic",
                _ => continue,
            };
            let Ok(id) = id.trim().parse::<u32>() else {
                continue;
            };
            let qty = qty.trim().parse::<u32>().unwrap_or(1);
            let cat = match cat.trim() {
                "weapon" => Cat::Weapon,
                "armor" => Cat::Armor,
                "talisman" => Cat::Talisman,
                "ash" => Cat::Ash,
                "magic" => Cat::Magic,
                _ => Cat::Goods,
            };
            lists
                .entry(key)
                .or_default()
                .push(Entry { cat, id, qty, dlc });
        }
        Library { lists }
    }

    fn list(&self, key: &'static str) -> &[Entry] {
        self.lists.get(key).map(Vec::as_slice).unwrap_or(&[])
    }

    /// Items for a script. `include_dlc` decides whether the DLC-only lists are
    /// part of it — they are off by default because DLC ids are not in the item
    /// params of the base game.
    pub fn resolve(&self, script: &Script, catalog: &Catalog, include_dlc: bool) -> Vec<Give> {
        let Kind::Items { source, variant } = script.kind else {
            return Vec::new();
        };
        let wanted = |entry: &&Entry| include_dlc || !entry.dlc;
        let mut out = match source {
            Source::List(list) => {
                let mut out: Vec<Give> = self
                    .list(list.key())
                    .iter()
                    .filter(wanted)
                    .map(|entry| {
                        // Weapon ids carry the upgrade level in the last two
                        // digits, so the pre-maxed list needs no extra work.
                        let level = if entry.cat == Cat::Weapon {
                            entry.id % 100
                        } else {
                            0
                        };
                        (entry.cat, entry.id, entry.qty.max(1), level)
                    })
                    .collect();
                if variant == Variant::TopOnly {
                    out = top_variants(out, catalog);
                }
                out
            }
            Source::Sorceries => self
                .list("magic")
                .iter()
                .filter(wanted)
                .filter(|entry| entry.id < 6000)
                .map(|entry| (Cat::Magic, entry.id, entry.qty.max(1), 0))
                .collect(),
            Source::Incantations => self
                .list("magic")
                .iter()
                .filter(wanted)
                .filter(|entry| entry.id >= 6000)
                .map(|entry| (Cat::Magic, entry.id, entry.qty.max(1), 0))
                .collect(),
            Source::Runes => catalog
                .items
                .iter()
                .filter(|item| item.cat == Cat::Goods && is_rune(item.name))
                .map(|item| (Cat::Goods, item.id, 99, 0))
                .collect(),
        };
        // One call per item. Two rows can name the same item (a variant listed
        // twice, or two spellings of one id), and a bulk script must not add it
        // twice; the id the game is called with is what decides.
        let mut seen: HashSet<u32> = HashSet::new();
        out.retain(|give| seen.insert(item_id(give)));
        out
    }
}

/// The id the game's item routine is called with, so duplicates collapse.
fn item_id(give: &Give) -> u32 {
    let (cat, id, _qty, level) = *give;
    crate::items::gib_id(cat, id, level)
}

fn is_rune(name: &str) -> bool {
    let name = name.trim();
    name.starts_with("Golden Rune")
        || name.starts_with("Numen's Rune")
        || name.starts_with("Hero's Rune")
        || name.starts_with("Lord's Rune")
}

/// Keep only the strongest upgrade variant of each talisman family.
fn top_variants(items: Vec<Give>, catalog: &Catalog) -> Vec<Give> {
    let mut best: BTreeMap<String, (u32, Give)> = BTreeMap::new();
    for give in items {
        let name = catalog.name_of(give.1, give.0).unwrap_or_default();
        let (family, rank) = split_variant(&name);
        match best.get(&family) {
            Some((seen, _)) if *seen >= rank => {}
            _ => {
                best.insert(family, (rank, give));
            }
        }
    }
    best.into_values().map(|(_, give)| give).collect()
}

/// `"Erdtree's Favor +2"` → `("Erdtree's Favor", 2)`.
fn split_variant(name: &str) -> (String, u32) {
    let trimmed = name.trim_end();
    if let Some(pos) = trimmed.rfind(" +") {
        if let Ok(rank) = trimmed[pos + 2..].parse::<u32>() {
            return (trimmed[..pos].to_string(), rank);
        }
    }
    (trimmed.to_string(), 0)
}

pub const SCRIPTS: [Script; 31] = [
    Script {
        name: "all weapons & shields",
        blurb: "every weapon and shield, base game and DLC, at +0",
        kind: Kind::Items {
            source: Source::List(List::Weapons),
            variant: Variant::Every,
        },
    },
    Script {
        name: "all weapons & shields, maxed",
        blurb: "the same list at its top upgrade level (+25 smithing, +10 somber)",
        kind: Kind::Items {
            source: Source::List(List::WeaponsMax),
            variant: Variant::Every,
        },
    },
    Script {
        name: "all armour",
        blurb: "every helm, chest piece, gauntlet and greave",
        kind: Kind::Items {
            source: Source::List(List::Armor),
            variant: Variant::Every,
        },
    },
    Script {
        name: "all talismans",
        blurb: "every talisman, including each +1 and +2 variant",
        kind: Kind::Items {
            source: Source::List(List::Talismans),
            variant: Variant::Every,
        },
    },
    Script {
        name: "all talismans, strongest only",
        blurb: "one talisman per family — the highest +N variant",
        kind: Kind::Items {
            source: Source::List(List::Talismans),
            variant: Variant::TopOnly,
        },
    },
    Script {
        name: "all sorceries",
        blurb: "every sorcery in the spell list",
        kind: Kind::Items {
            source: Source::Sorceries,
            variant: Variant::Every,
        },
    },
    Script {
        name: "all incantations",
        blurb: "every incantation in the spell list",
        kind: Kind::Items {
            source: Source::Incantations,
            variant: Variant::Every,
        },
    },
    Script {
        name: "all ashes of war",
        blurb: "every ash of war, ready to be applied at a grace",
        kind: Kind::Items {
            source: Source::List(List::Ashes),
            variant: Variant::Every,
        },
    },
    Script {
        name: "all crafting materials",
        blurb: "every crafting material, 99 of each",
        kind: Kind::Items {
            source: Source::List(List::Crafting),
            variant: Variant::Every,
        },
    },
    Script {
        name: "all arrows & bolts",
        blurb: "every arrow and bolt, 99 of each",
        kind: Kind::Items {
            source: Source::List(List::Arrows),
            variant: Variant::Every,
        },
    },
    Script {
        name: "all consumables",
        blurb: "greases, pots, throwing items, food and draughts — 99 of each",
        kind: Kind::Items {
            source: Source::List(List::Consumables),
            variant: Variant::Every,
        },
    },
    Script {
        name: "all reusable items",
        blurb: "the key-item style reusables, one of each",
        kind: Kind::Items {
            source: Source::List(List::Reusables),
            variant: Variant::Every,
        },
    },
    Script {
        name: "all upgrade materials",
        blurb: "smithing stones, somber stones and glovewort, 99 of each",
        kind: Kind::Items {
            source: Source::List(List::Smithing),
            variant: Variant::Every,
        },
    },
    Script {
        name: "all maps",
        blurb: "every map fragment, one of each",
        kind: Kind::Items {
            source: Source::List(List::Maps),
            variant: Variant::Every,
        },
    },
    Script {
        name: "all cookbooks",
        blurb: "every cookbook, one of each",
        kind: Kind::Items {
            source: Source::List(List::Cookbooks),
            variant: Variant::Every,
        },
    },
    Script {
        name: "all whetblades",
        blurb: "every whetblade plus the whetstone knife, one of each",
        kind: Kind::Items {
            source: Source::List(List::Whetblades),
            variant: Variant::Every,
        },
    },
    Script {
        name: "all talisman pouches",
        blurb: "three talisman pouches, the base game's set",
        kind: Kind::Items {
            source: Source::List(List::Pouches),
            variant: Variant::Every,
        },
    },
    Script {
        name: "all multiplayer items",
        blurb: "fingers, effigies, ciphers, taunter's tongue and 99 remedies",
        kind: Kind::Items {
            source: Source::List(List::Multiplayer),
            variant: Variant::Every,
        },
    },
    Script {
        name: "all memory stones",
        blurb: "eight memory stones, the base game's set",
        kind: Kind::Items {
            source: Source::List(List::MemoryStones),
            variant: Variant::Every,
        },
    },
    Script {
        name: "money bags",
        blurb: "a stack of 99 of every rune item: golden, hero's, numen's, lord's",
        kind: Kind::Items {
            source: Source::Runes,
            variant: Variant::Every,
        },
    },
    Script {
        name: "all wondrous physick items",
        blurb: "the flask itself plus every crystal tear, base game and DLC",
        kind: Kind::Items {
            source: Source::List(List::Physick),
            variant: Variant::Every,
        },
    },
    Script {
        name: "unlock torrent",
        blurb: "flags Torrent as obtained and gives the spectral steed whistle",
        kind: Kind::UnlockTorrent,
    },
    Script {
        name: "unlock roundtable",
        blurb: "sets Melina's invitation flags so Roundtable Hold is open",
        kind: Kind::UnlockRoundtable,
    },
    Script {
        name: "reveal all maps",
        blurb: "switches the map display on and reveals every map piece",
        kind: Kind::RevealMaps,
    },
    Script {
        name: "kill all bosses",
        blurb: "marks every boss dead — this rewrites quest progress, back up first",
        kind: Kind::Bosses { dead: true },
    },
    Script {
        name: "revive all bosses",
        blurb: "marks every boss alive again, back where it was fought",
        kind: Kind::Bosses { dead: false },
    },
    Script {
        name: "perfume bottles",
        blurb: "the ten empty perfume bottles the arts are refilled from",
        kind: Kind::Items {
            source: Source::List(List::Perfume),
            variant: Variant::Every,
        },
    },
    Script {
        name: "max out flask number",
        blurb: "fourteen flask charges, split between crimson and cerulean",
        kind: Kind::MaxFlaskNumber,
    },
    Script {
        name: "max out flask charge",
        blurb: "both flasks to +12, the highest Sacred Tears take them",
        kind: Kind::MaxFlaskCharge,
    },
    Script {
        name: "unlock all graces",
        blurb: "sets the event flag of every site of grace, base game and DLC",
        kind: Kind::UnlockGraces,
    },
    Script {
        name: "activate all summoning pools",
        blurb: "switches on every martyr effigy / summoning pool on the map",
        kind: Kind::SummoningPools,
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    /// A bulk give must never hand the game the same item twice.
    #[test]
    fn every_script_gives_each_item_once() {
        let library = Library::load();
        let catalog = Catalog::load();
        for script in SCRIPTS.iter() {
            for dlc in [false, true] {
                let gives = library.resolve(script, &catalog, dlc);
                let mut seen: HashSet<u32> = HashSet::new();
                for give in &gives {
                    assert!(
                        seen.insert(item_id(give)),
                        "{} repeats {:?} (dlc {dlc})",
                        script.name,
                        give
                    );
                }
            }
        }
    }
}
