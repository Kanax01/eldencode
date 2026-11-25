//! Item catalog extracted from the table's own dropdowns (`itemgibv2` lists),
//! so the ids match what the Cheat Engine table would write.

pub const RAW: &str = include_str!("../assets/items.tsv");

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
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
pub fn base_id(id: u32) -> u32 {
    id - id % 100
}

/// The exact id the game's item routine is called with:
/// `(base | category << 28) + level`, the same arithmetic the table's Lua does.
pub fn gib_id(cat: Cat, id: u32, level: u32) -> u32 {
    (base_id(id) | cat.type_nibble()).wrapping_add(level)
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
    buffer[0x24..0x28].copy_from_slice(&gib_id(cat, id, level).to_le_bytes());
    buffer[0x28..0x2C].copy_from_slice(&qty.clamp(1, 999).to_le_bytes());
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

    /// Highest upgrade level for this id.
    ///
    /// Standard weapons share a 10,000-wide id block with their 13 affinity
    /// variants and reach +25; unique (somber) weapons stand alone in their
    /// block and stop at +10. Everything that is not a weapon has no level.
    pub fn max_level(&self, id: u32) -> u32 {
        let base = base_id(id);
        let is_weapon = self
            .items
            .iter()
            .any(|item| item.cat == Cat::Weapon && (item.id == id || item.id == base));
        if !is_weapon {
            return 0;
        }
        let block = base - base % 10_000;
        let family = self
            .items
            .iter()
            .filter(|item| {
                item.cat == Cat::Weapon && item.id >= block && item.id < block + 10_000
            })
            .count();
        if family > 1 {
            25
        } else {
            10
        }
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
