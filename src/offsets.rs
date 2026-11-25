//! Symbols, pointer chains and field offsets taken from the Hexinton
//! "All in One 8.0.4" Cheat Engine table. Each symbol is a rip-relative access
//! to a global; the table resolves it with an AOB scan and then treats the
//! resulting address as the head of a pointer chain, exactly like
//! `[GameDataMan]+8` + `0x6C` in Cheat Engine.

/// Table these offsets came from; bumped when the game patches.
pub const TABLE: &str = "game 1.17.1";

/// `define(LocalPlayerOffset,10EF8)` from the table's enable script.
pub const LOCAL_PLAYER_OFFSET: u64 = 0x10EF8;
/// `define(CHRSP,1E640)`.
pub const CHR_SP: u64 = 0x1E640;

pub struct SymbolDef {
    pub name: &'static str,
    pub pattern: &'static str,
    /// Byte index of the 4-byte rip displacement inside the match.
    pub offset: i32,
    /// Bytes from the displacement to the end of the instruction (+4) plus any
    /// extra the table adds.
    pub additional: i64,
    /// True when the symbol *is* the matched address (a function entry or a
    /// code position), not a rip-relative global.
    pub direct: bool,
}

/// `48 8B 05/0D/3D` style: displacement starts at byte 3 of the match.
const fn sym(name: &'static str, pattern: &'static str, additional: i64) -> SymbolDef {
    SymbolDef {
        name,
        pattern,
        offset: 3,
        additional,
        direct: false,
    }
}

/// `80 3D` (cmp byte [rip+disp], imm8): the displacement starts at byte 2.
const fn sym_cmp(name: &'static str, pattern: &'static str, additional: i64) -> SymbolDef {
    SymbolDef {
        name,
        pattern,
        offset: 2,
        additional,
        direct: false,
    }
}

/// `aobscanmodule` result used as-is: a function entry or code position.
const fn code(name: &'static str, pattern: &'static str) -> SymbolDef {
    SymbolDef {
        name,
        pattern,
        offset: 0,
        additional: 0,
        direct: true,
    }
}

/// The subset of the table's `aobList` eldencode uses. The rest of the list is
/// kept out on purpose: every entry costs a full module scan on attach.
pub const SYMBOLS: &[SymbolDef] = &[
    sym(
        "WorldChrMan",
        "48 8B 05 ?? ?? ?? ?? 48 85 C0 74 0F 48 39 88",
        7,
    ),
    sym(
        "GameDataMan",
        "48 8B 05 ?? ?? ?? ?? 48 85 C0 74 05 48 8B 40 58 C3 C3",
        7,
    ),
    sym(
        "GameMan",
        "48 8B 05 ???????? 80 B8 ???????? 0D 0F94 C0 C3",
        7,
    ),
    sym(
        "EventFlagMan",
        "48 8B 3D ???????? 48 85 FF ???? 32 C0 E9",
        7,
    ),
    sym_cmp(
        "CHR_DBG_FLAGS",
        "80 3D ?? ?? ?? ?? 00 0F 85 ?? ?? ?? ?? 32 C0 48",
        7,
    ),
    sym(
        "CHR_DBG",
        "48 8B 05 ?? ?? ?? ?? 41 83 FF 02 ?? ?? 48 85 C0",
        7,
    ),
    // Item spawning, straight from the table's InventoryAccessor / AddItemFunc
    // scans. Both are code positions, so they are used without a fixup.
    code(
        "InventoryAccessor",
        "44 8B 61 1C 41 8B FC C1 EF 07 40 80 E7 01 41 C1 EC 08 41 80 E4 01 48 8B 0D",
    ),
    code(
        "EventFlagSet",
        "48 89 5C 24 08 48 89 74 24 18 57 48 83 EC 30 48 8B DA 41 0F B6 F8 8B 12 48 8B F1 85 D2 0F 84 ?? ?? ?? ??",
    ),
    code(
        "AddItemFunc",
        "40 55 56 57 41 54 41 55 41 56 41 57 48 8D AC 24 70 FF FF FF 48 81 EC 90 01 00 00 48 C7 45 C8 FE FF FF FF 48 89 9C 24 D8 01 00 00 48 8B 05",
    ),
];

/// Image-relative pointer to the param repository. The flag walks read it, then

/// From the table's itemgib stub: `mov rax, InventoryAccessor+19; mov eax,[rax];
/// cdqe; mov rcx, InventoryAccessor+1D; add rcx, rax; mov rcx,[rcx]`.
pub const INVENTORY_DISP: u64 = 0x19;
pub const INVENTORY_SLOT: u64 = 0x1D;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    U8,
    U32,
    F32,
    /// UTF-16LE name string, read-only.
    Name,
}

#[derive(Clone, Copy)]
pub struct Field {
    pub key: &'static str,
    pub label: &'static str,
    pub symbol: &'static str,
    /// Cheat Engine offset list, in table order. The last entry is applied
    /// first (see `Game::resolve`).
    pub offsets: &'static [&'static str],
    pub addend: i64,
    pub kind: Kind,
    pub note: &'static str,
}

const fn f(
    key: &'static str,
    label: &'static str,
    symbol: &'static str,
    offsets: &'static [&'static str],
    kind: Kind,
    note: &'static str,
) -> Field {
    Field {
        key,
        label,
        symbol,
        offsets,
        addend: 0,
        kind,
        note,
    }
}

/// Resolve a single Cheat Engine offset token: hex, a `index*stride` form, or a
/// table symbol that expands to a constant.
pub fn parse_offset(token: &str) -> Option<i64> {
    let token = token.trim();
    match token {
        "LocalPlayerOffset" => return Some(LOCAL_PLAYER_OFFSET as i64),
        "CHRSP" => return Some(CHR_SP as i64),
        "" => return Some(0),
        _ => {}
    }
    if let Some((index, stride)) = token.split_once('*') {
        let index = i64::from_str_radix(index.trim(), 16).ok()?;
        let stride = i64::from_str_radix(stride.trim(), 16).ok()?;
        return Some(index * stride);
    }
    i64::from_str_radix(token, 16).ok()
}

pub const ATTRIBUTES: &[Field] = &[
    f(
        "level",
        "Level",
        "GameDataMan",
        &["68", "8"],
        Kind::U32,
        "Soul level",
    ),
    f("vigor", "Vigor", "GameDataMan", &["3C", "8"], Kind::U32, ""),
    f("mind", "Mind", "GameDataMan", &["40", "8"], Kind::U32, ""),
    f(
        "endurance",
        "Endurance",
        "GameDataMan",
        &["44", "8"],
        Kind::U32,
        "",
    ),
    f(
        "strength",
        "Strength",
        "GameDataMan",
        &["48", "8"],
        Kind::U32,
        "",
    ),
    f(
        "dexterity",
        "Dexterity",
        "GameDataMan",
        &["4C", "8"],
        Kind::U32,
        "",
    ),
    f(
        "intelligence",
        "Intelligence",
        "GameDataMan",
        &["50", "8"],
        Kind::U32,
        "",
    ),
    f("faith", "Faith", "GameDataMan", &["54", "8"], Kind::U32, ""),
    f(
        "arcane",
        "Arcane",
        "GameDataMan",
        &["58", "8"],
        Kind::U32,
        "",
    ),
    f(
        "humanity",
        "Humanity",
        "GameDataMan",
        &["64", "8"],
        Kind::U32,
        "Unused by the game",
    ),
    f(
        "scadutree",
        "Scadutree Blessing",
        "GameDataMan",
        &["FC", "8"],
        Kind::U8,
        "DLC",
    ),
    f(
        "spirit_ash",
        "Revered Spirit Ash",
        "GameDataMan",
        &["FD", "8"],
        Kind::U8,
        "DLC",
    ),
];

pub const RUNES: &[Field] = &[
    f(
        "runes",
        "Runes held",
        "GameDataMan",
        &["6C", "8"],
        Kind::U32,
        "The number on the HUD",
    ),
    f(
        "total_runes",
        "Runes earned",
        "GameDataMan",
        &["70", "8"],
        Kind::U32,
        "Lifetime counter",
    ),
    f(
        "rune_arc",
        "Rune Arc active",
        "GameDataMan",
        &["FF", "8"],
        Kind::U8,
        "0/1",
    ),
];

/// Live values. The table reads these through
/// `WorldChrMan -> LocalPlayerOffset -> 0*10 -> 190 -> 0 -> <off>`.
pub const VITALS: &[Field] = &[
    Field {
        key: "hp",
        label: "HP",
        symbol: "WorldChrMan",
        offsets: &["138", "0", "190", "0*10", "LocalPlayerOffset"],
        addend: 0,
        kind: Kind::U32,
        note: "current",
    },
    Field {
        key: "max_hp",
        label: "Max HP",
        symbol: "WorldChrMan",
        offsets: &["13C", "0", "190", "0*10", "LocalPlayerOffset"],
        addend: 0,
        kind: Kind::U32,
        note: "current cap",
    },
    Field {
        key: "base_max_hp",
        label: "Base Max HP",
        symbol: "WorldChrMan",
        offsets: &["144", "0", "190", "0*10", "LocalPlayerOffset"],
        addend: 0,
        kind: Kind::U32,
        note: "",
    },
    Field {
        key: "fp",
        label: "FP",
        symbol: "WorldChrMan",
        offsets: &["148", "0", "190", "0*10", "LocalPlayerOffset"],
        addend: 0,
        kind: Kind::U32,
        note: "current",
    },
    Field {
        key: "max_fp",
        label: "Max FP",
        symbol: "WorldChrMan",
        offsets: &["14C", "0", "190", "0*10", "LocalPlayerOffset"],
        addend: 0,
        kind: Kind::U32,
        note: "current cap",
    },
    Field {
        key: "stamina",
        label: "Stamina",
        symbol: "WorldChrMan",
        offsets: &["154", "0", "190", "0*10", "LocalPlayerOffset"],
        addend: 0,
        kind: Kind::U32,
        note: "current",
    },
    Field {
        key: "max_stamina",
        label: "Max Stamina",
        symbol: "WorldChrMan",
        offsets: &["158", "0", "190", "0*10", "LocalPlayerOffset"],
        addend: 0,
        kind: Kind::U32,
        note: "current cap",
    },
];

/// Saved-character counters (single dereference off `GameDataMan`).
pub const SAVE_STATUS: &[Field] = &[
    f("deaths", "Deaths", "GameDataMan", &["94"], Kind::U32, ""),
    f(
        "playtime",
        "Play time (ms)",
        "GameDataMan",
        &["A0"],
        Kind::U32,
        "",
    ),
    f(
        "ng",
        "Journey (NG+)",
        "GameDataMan",
        &["120"],
        Kind::U32,
        "Set before loading",
    ),
    f(
        "name",
        "Name",
        "GameDataMan",
        &["9C", "8"],
        Kind::Name,
        "read only",
    ),
];

/// Debug flag block: `CHR_DBG_FLAGS + n`, one byte per flag, as in the table's
/// `[ Character Data ] > ChrDbg` section.
const fn flag(key: &'static str, label: &'static str, addend: i64, note: &'static str) -> Field {
    Field {
        key,
        label,
        symbol: "CHR_DBG_FLAGS",
        offsets: &[],
        addend,
        kind: Kind::U8,
        note,
    }
}

pub const FLAGS: &[Field] = &[
    flag(
        "player_no_dead",
        "Player No Dead",
        0x0,
        "Local player cannot die",
    ),
    flag(
        "player_horse_no_dead",
        "Horse No Dead",
        0x1,
        "Torrent cannot die",
    ),
    flag(
        "player_exterminate",
        "Player Exterminate",
        0x2,
        "Kills the local player",
    ),
    flag(
        "all_no_goods_consume",
        "All No Goods Consume",
        0x3,
        "Items are not used up",
    ),
    flag("all_no_stamina_consume", "All No Stamina Consume", 0x4, ""),
    flag("all_no_fp_consume", "All No FP Consume", 0x5, ""),
    flag("all_no_arrow_consume", "All No Arrow Consume", 0x6, ""),
    flag(
        "all_no_magic_qty_consume",
        "All No Magic Qty Consume",
        0x7,
        "",
    ),
    flag("player_hide", "Player Hide", 0x8, ""),
    flag("player_silence", "Player Silence", 0x9, ""),
    flag(
        "all_no_dead",
        "All No Dead",
        0xA,
        "Applies to every character",
    ),
    flag(
        "all_no_damage",
        "All No Damage",
        0xB,
        "Applies to every character",
    ),
    flag("all_no_hit", "All No Hit", 0xC, ""),
    flag("all_no_attack", "All No Attack", 0xD, ""),
    flag("all_no_move", "All No Move", 0xE, ""),
    flag("all_no_update_ai", "All No Update AI", 0xF, ""),
    flag("all_no_durability", "No Weapon Durability", 0x10, ""),
    flag("all_no_ash_fp", "No Ash of War FP", 0x11, ""),
    flag("all_no_goods_consume2", "All No Goods Consume 2", 0x12, ""),
    flag("force_parry", "Force Parry Mode", 0x13, ""),
    flag("disable_summoning", "Disable Summoning", 0x14, ""),
    flag("omission_mode", "All Omission Mode", 0x28, ""),
];

/// Per-character flags that live on the player's ChrIns rather than the debug
/// block. Same chain shape as the vitals.
pub const PLAYER_FLAGS: &[Field] = &[
    Field {
        key: "no_gravity",
        label: "Disable Gravity",
        symbol: "WorldChrMan",
        offsets: &["1D3", "68", "190", "0*10", "LocalPlayerOffset"],
        addend: 0,
        kind: Kind::U8,
        note: "Float mid-air, then turn off",
    },
    Field {
        key: "map_collision",
        label: "Disable Map Collision",
        symbol: "WorldChrMan",
        offsets: &["F0", "58", "0*10", "1E508"],
        addend: 0,
        kind: Kind::U8,
        note: "Walk through walls",
    },
];

/// Slots the game uses for "nothing equipped". Weapon slots fall back to
/// `110000` (Unarmed), armour and ammo to `0xFFFFFFFF`.
pub const EMPTY_SLOT: u32 = 0xFFFF_FFFF;
pub const UNARMED: u32 = 110_000;

/// Equipment slots on the player's ChrAsm. Writing an item id here swaps what
/// the character is holding/wearing without needing the item in the inventory.
///
/// The armour, ammo and talisman offsets are 4 bytes lower than the table's
/// ChrAsm block: read against a live 1.17.1 character, `3CC`/`3D0`/`3D4` hold
/// chest/gauntlets/legs and `3B0`/`3B4` hold arrows/bolts (the table lists
/// helmet/talisman 1 and arrow at `3CC`/`3E0`/`3B4`). Weapon slots match the
/// table exactly, and the third right-hand slot is omitted because that address
/// is the arrow slot on this build.
pub const EQUIP: &[Field] = &[
    f(
        "right_hand_1",
        "Right hand 1",
        "GameDataMan",
        &["3A0", "8"],
        Kind::U32,
        "",
    ),
    f(
        "left_hand_1",
        "Left hand 1",
        "GameDataMan",
        &["39C", "8"],
        Kind::U32,
        "",
    ),
    f(
        "right_hand_2",
        "Right hand 2",
        "GameDataMan",
        &["3A8", "8"],
        Kind::U32,
        "",
    ),
    f(
        "left_hand_2",
        "Left hand 2",
        "GameDataMan",
        &["3A4", "8"],
        Kind::U32,
        "",
    ),
    f(
        "left_hand_3",
        "Left hand 3",
        "GameDataMan",
        &["3AC", "8"],
        Kind::U32,
        "",
    ),
    f(
        "arrows",
        "Arrows",
        "GameDataMan",
        &["3B0", "8"],
        Kind::U32,
        "",
    ),
    f(
        "bolts",
        "Bolts",
        "GameDataMan",
        &["3B4", "8"],
        Kind::U32,
        "",
    ),
    f(
        "helmet",
        "Helmet",
        "GameDataMan",
        &["3C8", "8"],
        Kind::U32,
        "",
    ),
    f(
        "chest",
        "Chest armour",
        "GameDataMan",
        &["3CC", "8"],
        Kind::U32,
        "",
    ),
    f(
        "gauntlets",
        "Gauntlets",
        "GameDataMan",
        &["3D0", "8"],
        Kind::U32,
        "",
    ),
    f(
        "legs",
        "Leg armour",
        "GameDataMan",
        &["3D4", "8"],
        Kind::U32,
        "",
    ),
    f(
        "talisman_1",
        "Talisman 1",
        "GameDataMan",
        &["3DC", "8"],
        Kind::U32,
        "",
    ),
    f(
        "talisman_2",
        "Talisman 2",
        "GameDataMan",
        &["3E0", "8"],
        Kind::U32,
        "",
    ),
    f(
        "talisman_3",
        "Talisman 3",
        "GameDataMan",
        &["3E4", "8"],
        Kind::U32,
        "",
    ),
    f(
        "talisman_4",
        "Talisman 4",
        "GameDataMan",
        &["3E8", "8"],
        Kind::U32,
        "",
    ),
    f(
        "talisman_5",
        "Talisman 5",
        "GameDataMan",
        &["3EC", "8"],
        Kind::U32,
        "",
    ),
];

/// Every field eldencode can read/write, for the overview snapshot.
pub fn all_fields() -> impl Iterator<Item = &'static Field> {
    ATTRIBUTES
        .iter()
        .chain(RUNES.iter())
        .chain(VITALS.iter())
        .chain(SAVE_STATUS.iter())
        .chain(FLAGS.iter())
        .chain(PLAYER_FLAGS.iter())
        .chain(EQUIP.iter())
}
