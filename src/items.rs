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
