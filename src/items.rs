//! Item catalog extracted from the table's own dropdowns (`itemgibv2` lists),
//! so the ids match what the Cheat Engine table would write.

use std::collections::HashMap;
use std::sync::OnceLock;

pub const RAW: &str = include_str!("../assets/items.tsv");

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Cat {
    Weapon,
    Armor,
    Talisman,
    Goods,
    Ash,
    Magic,
}

impl Cat {
    pub const ALL: [Cat; 6] = [
        Cat::Weapon,
        Cat::Armor,
        Cat::Talisman,
        Cat::Goods,
        Cat::Ash,
        Cat::Magic,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Cat::Weapon => "Weapons",
            Cat::Armor => "Armour",
            Cat::Talisman => "Talismans",
            Cat::Goods => "Items",
            Cat::Ash => "Ashes of War",
            Cat::Magic => "Magic",
        }
    }

    /// Category nibble (bits 28..31) the game's item routine expects. Values
    /// come from the table's `ConvertTypeIndexTable` / itemgib2 lists.
    pub fn type_nibble(self) -> u32 {
        match self {
            Cat::Weapon => 0x0000_0000,
            Cat::Armor => 0x1000_0000,
            Cat::Talisman => 0x2000_0000,
            Cat::Goods | Cat::Magic => 0x4000_0000,
            Cat::Ash => 0x8000_0000,
        }
    }
}

/// Weapon/ash ids carry their upgrade level in the last two digits.
/// Per-weapon upgrade cap, taken from the table's own "max level" list: it hands
/// every weapon the level the game accepts, which is +25 for smithing weapons,
/// +10 for somber ones and +0 for the few that never upgrade. Anything past the
/// cap makes the game fall over, so every write clamps to it.
const LEVELS_TSV: &str = include_str!("../assets/weapon_levels.tsv");

/// Per-weapon upgrade caps: one `family_id<TAB>cap` row per weapon.
fn weapon_levels() -> &'static HashMap<u32, u32> {
    static LEVELS: OnceLock<HashMap<u32, u32>> = OnceLock::new();
    LEVELS.get_or_init(|| {
        let mut map = HashMap::new();
        for line in LEVELS_TSV.lines() {
            let mut parts = line.split('\t');
            let (Some(id), Some(cap)) = (parts.next(), parts.next()) else {
                continue;
            };
            if let (Ok(id), Ok(cap)) = (id.trim().parse(), cap.trim().parse()) {
                map.insert(id, cap);
            }
        }
        map
    })
}

pub fn max_level(cat: Cat, id: u32) -> u32 {
    if cat != Cat::Weapon {
        return 0;
    }
    weapon_levels()
        .get(&(id - id % 10_000))
        .copied()
        .unwrap_or(0)
}

pub fn base_id(id: u32) -> u32 {
    id - id % 100
}

/// The plain id a give is checked against: the category nibble is part of it,
/// the upgrade level is not, because the game stores one row per item.
pub fn plain_id(cat: Cat, id: u32, _level: u32) -> u32 {
    // The row the game stores: no category nibble, no upgrade level.
    if cat == Cat::Weapon {
        base_id(id)
    } else {
        id & 0x0FFF_FFFF
    }
}

/// How many of an item fit in one stack: 99 for consumables, one for gear.
pub fn max_stack(cat: Cat) -> u32 {
    match cat {
        Cat::Goods => 99,
        _ => 1,
    }
}

/// Ten evenly spaced ids from a category, used to recognise that category's
/// param table in the game's memory: one of them is always a real row.
pub fn probe_ids(cat: Cat) -> Vec<u32> {
    let catalog = Catalog::load();
    let ids: Vec<u32> = catalog
        .items
        .iter()
        .filter(|item| item.cat == cat)
        .map(|item| {
            if cat == Cat::Weapon {
                base_id(item.id)
            } else {
                item.id
            }
        })
        .collect();
    let n = ids.len();
    (0..10.min(n)).map(|i| ids[i * n / 10]).collect()
}

/// The exact id the game's item routine is called with:
/// `(base | category << 28) + level`, the same arithmetic the table's Lua does.
pub fn gib_id(cat: Cat, id: u32, level: u32) -> u32 {
    // Only weapons keep their upgrade level in the id's last two digits. Every
    // other category's id is sent as it is: rounding it down to the nearest 100
    // silently swapped the item (`15010 Beast Liver` became `15000 Sliver of
    // Meat`).
    let base = if cat == Cat::Weapon { base_id(id) } else { id };
    (base | cat.type_nibble()).wrapping_add(level)
}

/// The 0x50-byte item struct from the table's `itembuffer` template: id at
/// +0x24, quantity at +0x28, ash of war at +0x30 (0xFFFFFFFF = none).
pub fn item_buffer(cat: Cat, id: u32, qty: u32, level: u32) -> [u8; 0x50] {
    const TEMPLATE: [u64; 10] = [
        0,
        0,
        0,
        0,
        0xF000_06AE_0000_0001,
        0x0000_0000_0000_0001,
        0xFFFF_FFFF_FFFF_FFFF,
        0xFFFF_FFFF_0000_0000,
        0xFFFF_FFFF_FFFF_FFFF,
        0xFFFF_FFFF_0000_0000,
    ];
    let mut buffer = [0u8; 0x50];
    for (i, word) in TEMPLATE.iter().enumerate() {
        buffer[i * 8..i * 8 + 8].copy_from_slice(&word.to_le_bytes());
    }
    let level = level.min(max_level(cat, id));
    let qty = qty.clamp(1, max_stack(cat));
    buffer[0x24..0x28].copy_from_slice(&gib_id(cat, id, level).to_le_bytes());
    buffer[0x28..0x2C].copy_from_slice(&qty.to_le_bytes());
    buffer[0x30..0x34].copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
    buffer
}

#[derive(Clone, Debug)]
pub struct Item {
    pub id: u32,
    pub name: &'static str,
    pub cat: Cat,
}

pub struct Catalog {
    pub items: Vec<Item>,
}

impl Catalog {
    pub fn load() -> Catalog {
        let mut items = Vec::new();
        for line in RAW.lines() {
            let mut parts = line.splitn(3, '\t');
            let (Some(cat), Some(id), Some(name)) = (parts.next(), parts.next(), parts.next())
            else {
                continue;
            };
            let Ok(id) = id.parse::<u32>() else { continue };
            let cat = match cat {
                "weapon" => Cat::Weapon,
                "armor" => Cat::Armor,
                "talisman" => Cat::Talisman,
                "goods" => Cat::Goods,
                "ash" => Cat::Ash,
                _ => Cat::Magic,
            };
            items.push(Item { id, name, cat });
        }
        // The list holds one row per item plus every infused variant the table
        // spells out; only the plain rows are offered (see the retain below).
        for item in items.iter_mut() {
            // Arrows and bolts are shipped in the weapon list but the game wants
            // them as consumables (the `| 0x40000000` the table's own arrow
            // script writes).
            if item.cat == Cat::Weapon && matches!(item.id / 1_000_000, 50..=53) {
                item.cat = Cat::Goods;
            }
        }
        items.retain(|item| {
            if item.cat != Cat::Weapon {
                return true;
            }
            // Only the standard row: infused variants are not offered.
            (item.id % 10_000) / 100 == 0
        });
        Catalog { items }
}

    /// Name for an id, preferring `cat`; ids that carry an upgrade level
    /// (`Base + 2`) fall back to the base entry.
    pub fn name_of(&self, id: u32, cat: Cat) -> Option<String> {
        let exact = self
            .items
            .iter()
            .find(|item| item.id == id && item.cat == cat)
            .or_else(|| self.items.iter().find(|item| item.id == id));
        if let Some(item) = exact {
            return Some(item.name.to_string());
        }
        let level = id % 100;
        if level > 0 && level <= 25 {
            let base = id - level;
            if let Some(item) = self
                .items
                .iter()
                .find(|item| item.id == base && item.cat == cat)
            {
                return Some(format!("{} +{level}", item.name));
            }
        }
        None
    }

    /// Ids in one category, optionally narrowed by a case-insensitive substring.
    pub fn count(&self, cat: Cat) -> usize {
        self.items.iter().filter(|item| item.cat == cat).count()
    }

    /// Upgrade cap for an item, straight from the table's per-weapon level list.
    pub fn max_level(&self, id: u32) -> u32 {
        let base = base_id(id);
        let is_weapon = self
            .items
            .iter()
            .any(|item| item.cat == Cat::Weapon && (item.id == id || item.id == base));
        if !is_weapon {
            return 0;
        }
        max_level(Cat::Weapon, id)
    }

    pub fn filter(&self, cat: Cat, query: &str) -> Vec<usize> {
        let needle = query.trim().to_ascii_lowercase();
        self.items
            .iter()
            .enumerate()
            .filter(|(_, item)| item.cat == cat)
            .filter(|(_, item)| {
                if needle.is_empty() {
                    true
                } else {
                    item.name.to_ascii_lowercase().contains(&needle)
                        || item.id.to_string().contains(&needle)
                }
            })
            .map(|(i, _)| i)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn qty_of(buffer: &[u8; 0x50]) -> u32 {
        u32::from_le_bytes(buffer[0x28..0x2C].try_into().unwrap())
    }

    fn id_of(buffer: &[u8; 0x50]) -> u32 {
        u32::from_le_bytes(buffer[0x24..0x28].try_into().unwrap())
    }

    #[test]
    fn somber_weapons_stop_at_ten() {
        // Black Knife (1010000) is somber, Dagger (1000000) is smithing.
        assert_eq!(max_level(Cat::Weapon, 1_010_000), 10);
        assert_eq!(max_level(Cat::Weapon, 1_000_000), 25);
        assert_eq!(id_of(&item_buffer(Cat::Weapon, 1_010_000, 1, 25)), 1_010_010);
        assert_eq!(id_of(&item_buffer(Cat::Weapon, 1_000_000, 1, 25)), 1_000_025);
        // Non-weapons never take a level.
        assert_eq!(
            id_of(&item_buffer(Cat::Armor, 1_000_000, 1, 25)),
            gib_id(Cat::Armor, 1_000_000, 0)
        );
    }


    #[test]
    fn the_item_list_offers_plain_weapons_only() {
        let catalog = Catalog::load();
        assert!(catalog.items.iter().any(|item| item.id == 8_030_000));
        assert!(catalog.items.iter().any(|item| item.id == 1_000_000));
        // No infused variants anywhere: the game builds those from an ash of war.
        assert!(!catalog
            .items
            .iter()
            .any(|item| item.cat == Cat::Weapon && item.id == 1_000_200));
        assert!(!catalog
            .items
            .iter()
            .any(|item| item.cat == Cat::Weapon && item.id == 8_030_200));
        assert!(!catalog
            .items
            .iter()
            .any(|item| item.cat == Cat::Weapon && item.id % 100 != 0));
    }

    #[test]
    fn ammunition_is_a_consumable() {
        let catalog = Catalog::load();
        let arrow = catalog.items.iter().find(|item| item.id == 50_000_000);
        assert_eq!(arrow.map(|item| item.cat), Some(Cat::Goods));
        assert_eq!(max_level(Cat::Weapon, 50_000_000), 0);
    }

    #[test]
    fn only_consumables_stack_and_only_to_99() {
        assert_eq!(qty_of(&item_buffer(Cat::Goods, 1000, 500, 0)), 99);
        assert_eq!(qty_of(&item_buffer(Cat::Goods, 1000, 7, 0)), 7);
        assert_eq!(qty_of(&item_buffer(Cat::Weapon, 1_000_000, 500, 0)), 1);
    }
}
