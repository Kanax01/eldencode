use std::collections::HashMap;

use crate::aob::{self, Pattern};
use crate::mem::{MemError, Memory};
use crate::offsets::{self, Field, Kind};
use crate::process::{self, GameProcess};

#[derive(Clone, Debug)]
pub enum Value {
    U32(u32),
    U8(u8),
    F32(f32),
    Name(String),
}

impl Value {
    pub fn u32(&self) -> Option<u32> {
        match self {
            Value::U32(v) => Some(*v),
            Value::U8(v) => Some(*v as u32),
            Value::F32(v) => Some(*v as u32),
            Value::Name(_) => None,
        }
    }

    pub fn display(&self) -> String {
        match self {
            Value::U32(v) => v.to_string(),
            Value::U8(v) => v.to_string(),
            Value::F32(v) => format!("{v:.3}"),
            Value::Name(v) => v.clone(),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub level: Option<u32>,
    pub runes: Option<u32>,
    pub total_runes: Option<u32>,
    pub hp: Option<u32>,
    pub max_hp: Option<u32>,
    pub fp: Option<u32>,
    pub max_fp: Option<u32>,
    pub stamina: Option<u32>,
    pub max_stamina: Option<u32>,
    pub name: Option<String>,
    pub ng: Option<u32>,
}

pub struct AttachError {
    pub message: String,
    pub hint: Option<String>,
}

pub struct Game {
    pub proc: GameProcess,
    mem: Memory,
    symbols: HashMap<&'static str, u64>,
    pub failed_symbols: Vec<&'static str>,
    pub scanned_mb: f64,
    pub regions: usize,
    /// Item param tables discovered in the game's memory.
    param_tables: std::sync::OnceLock<Vec<Option<ParamTable>>>,
}

/// One of the game's param tables (`EquipParamWeapon` and friends): a header
/// with the row count, then a sorted index of `{ id, data offset, checksum }`
/// entries of 0x18 bytes at `+0x40`. Row ids are 64-bit and every table ends
/// with a `PARAM_SENTINEL` row.
#[derive(Clone, Copy)]
pub struct ParamTable {
    pub head: u64,
    pub rows: u32,
}

/// The row id every param table ends with.
const PARAM_SENTINEL: u64 = 99_999_999;
/// How much of the game's memory is read at a time while looking for params.
const SCAN_CHUNK: usize = 8 << 20;
/// Params live in the big writable mappings; smaller ones are not worth a scan.
const SCAN_MIN_REGION: u64 = 64 << 20;
/// How far around a found table the other param tables are looked for.
const SCAN_WINDOW_BACK: u64 = 384 << 20;
const SCAN_WINDOW_FORWARD: u64 = 64 << 20;

fn add_signed(base: u64, offset: i64) -> Option<u64> {
    if offset >= 0 {
        base.checked_add(offset as u64)
    } else {
        base.checked_sub(offset.unsigned_abs())
    }
}

impl Game {
    /// Open the process and resolve every symbol by scanning the image. This is
    /// the slow part of attaching (a few seconds on a cold CPU cache), so it is
    /// only done on attach or explicit rescan.
    pub fn attach(proc: GameProcess) -> Result<Game, AttachError> {
        let mem = Memory::open(proc.pid).map_err(|err| match err {
            MemError::Permission => AttachError {
                message: format!("cannot open /proc/{}/mem: permission denied", proc.pid),
                hint: Some(
                    "Steam runs the game as your user, but the kernel's yama ptrace scope blocks \
                     non-parent access. Either run \
                     `sudo sysctl -w kernel.yama.ptrace_scope=0` (resets on reboot) or start \
                     eldencode with sudo."
                        .into(),
                ),
            },
            other => AttachError {
                message: format!("cannot open /proc/{}/mem: {other}", proc.pid),
                hint: None,
            },
        })?;

        let regions = process::image_regions(&proc);
        if regions.is_empty() {
            return Err(AttachError {
                message: format!(
                    "no readable image pages for {} (pid {}); is the game past the title screen?",
                    proc.comm, proc.pid
                ),
                hint: None,
            });
        }
        let total: u64 = regions.iter().map(|(a, b)| b - a).sum();

        let mut symbols = HashMap::new();
        let mut failed = Vec::new();
        for def in offsets::SYMBOLS {
            let Ok(pattern) = Pattern::parse(def.pattern) else {
                failed.push(def.name);
                continue;
            };
            let hits = aob::find(&mem, &regions, &pattern, 1);
            match hits.first() {
                Some(&hit) => {
                    if def.direct {
                        symbols.insert(def.name, hit);
                        continue;
                    }
                    let disp_addr = add_signed(hit, def.offset as i64).unwrap_or(hit);
                    match mem.read_i32(disp_addr) {
                        Ok(disp) => {
                            let target = add_signed(disp_addr, disp as i64)
                                .and_then(|v| add_signed(v, def.additional - def.offset as i64))
                                .unwrap_or(0);
                            symbols.insert(def.name, target);
                        }
                        Err(_) => failed.push(def.name),
                    }
                }
                None => failed.push(def.name),
            }
        }

        Ok(Game {
            proc,
            mem,
            symbols,
            failed_symbols: failed,
            scanned_mb: total as f64 / (1024.0 * 1024.0),
            regions: regions.len(),
            param_tables: std::sync::OnceLock::new(),
        })
    }

    /// Walk a Cheat Engine pointer chain: dereference the symbol's global, apply
    /// the table's offsets from last to first with a dereference each step, and
    /// add the first offset to land on the field.
    fn resolve(&self, field: &Field) -> Result<u64, String> {
        let base = self
            .symbols
            .get(field.symbol)
            .copied()
            .ok_or_else(|| format!("symbol {} was not found (game update?)", field.symbol))?;
        if field.offsets.is_empty() {
            return add_signed(base, field.addend).ok_or_else(|| "address overflow".to_string());
        }
        let mut chain = Vec::with_capacity(field.offsets.len());
        for token in field.offsets {
            let off = offsets::parse_offset(token)
                .ok_or_else(|| format!("bad offset '{token}' for {}", field.key))?;
            chain.push(off);
        }
        let mut addr = match self.mem.read_u64(base) {
            Ok(value) => value,
            Err(MemError::Unmapped) => return Err("game data pointer is null".into()),
            Err(err) => return Err(format!("read failed: {err}")),
        };
        if chain.len() > 1 {
            for off in chain[1..].iter().rev() {
                let next = add_signed(addr, *off).ok_or_else(|| "address overflow".to_string())?;
                addr = match self.mem.read_u64(next) {
                    Ok(value) => value,
                    Err(MemError::Unmapped) => return Err("pointer chain is null".into()),
                    Err(err) => return Err(format!("read failed: {err}")),
                };
            }
        }
        add_signed(addr, chain[0])
            .and_then(|v| add_signed(v, field.addend))
            .ok_or_else(|| "address overflow".to_string())
    }

    pub fn read(&self, field: &Field) -> Result<Value, String> {
        let addr = self.resolve(field)?;
        match field.kind {
            Kind::U8 => self
                .mem
                .read_u8(addr)
                .map(Value::U8)
                .map_err(|err| err.to_string()),
            Kind::U32 => self
                .mem
                .read_u32(addr)
                .map(Value::U32)
                .map_err(|err| err.to_string()),
            Kind::F32 => self
                .mem
                .read_f32(addr)
                .map(Value::F32)
                .map_err(|err| err.to_string()),
            Kind::Name => {
                let bytes = self
                    .mem
                    .read_bytes::<32>(addr)
                    .map_err(|err| err.to_string())?;
                let mut units = Vec::new();
                for pair in bytes.chunks(2) {
                    let unit = u16::from_le_bytes([pair[0], pair[1]]);
                    if unit == 0 {
                        break;
                    }
                    units.push(unit);
                }
                Ok(Value::Name(String::from_utf16_lossy(&units)))
            }
        }
    }

    pub fn write(&self, field: &Field, value: &Value) -> Result<(), String> {
        let addr = self.resolve(field)?;
        match (field.kind, value) {
            (Kind::U8, Value::U8(v)) => self.mem.write_u8(addr, *v).map_err(|e| e.to_string()),
            (Kind::U32, Value::U32(v)) => self.mem.write_u32(addr, *v).map_err(|e| e.to_string()),
            (Kind::F32, Value::F32(v)) => self.mem.write_f32(addr, *v).map_err(|e| e.to_string()),
            (Kind::U32, Value::U8(v)) => self
                .mem
                .write_u32(addr, *v as u32)
                .map_err(|e| e.to_string()),
            _ => Err("read-only field".to_string()),
        }
    }

    pub fn set_number(&self, field: &Field, number: u64) -> Result<(), String> {
        match field.kind {
            Kind::U8 => self.write(field, &Value::U8(number.min(u8::MAX as u64) as u8)),
            Kind::U32 => self.write(field, &Value::U32(number.min(u32::MAX as u64) as u32)),
            Kind::F32 => self.write(field, &Value::F32(number as f32)),
            Kind::Name => Err("read-only field".to_string()),
        }
    }

    pub fn toggle(&self, field: &Field) -> Result<u8, String> {
        let current = match self.read(field)? {
            Value::U8(v) => v,
            other => other.u32().unwrap_or(0) as u8,
        };
        let next = if current == 0 { 1 } else { 0 };
        self.set_number(field, next as u64)?;
        Ok(next)
    }

    /// Pointer the game's item routine expects in RCX, resolved the way the
    /// table's stub does it.
    pub fn inventory_manager(&self) -> Option<u64> {
        let accessor = self.symbols.get("InventoryAccessor").copied()?;
        let disp = self
            .mem
            .read_i32(accessor.checked_add(offsets::INVENTORY_DISP)?)
            .ok()? as i64;
        let slot = add_signed(accessor.checked_add(offsets::INVENTORY_SLOT)?, disp)?;
        let manager = self.mem.read_u64(slot).ok()?;
        if manager < 0x10000 {
            None
        } else {
            Some(manager)
        }
    }

    /// Is a character actually loaded?
    ///
    /// The item routine walks the local player on its way to the inventory, so
    /// calling it from the title screen or a loading screen faults inside the
    /// game. The vitals chain is the same one the table uses for HP: it is null
    /// until you are in the world.
    pub fn in_game(&self) -> bool {
        match offsets::VITALS.first() {
            Some(field) => self.resolve(field).is_ok(),
            None => false,
        }
    }

    pub fn add_item_addr(&self) -> Option<u64> {
        self.symbols.get("AddItemFunc").copied()
    }

    fn param_row_id(&self, table: ParamTable, index: u32) -> Option<u64> {
        self.mem
            .read_u64(table.head + 0x40 + u64::from(index) * 0x18)
            .ok()
    }

    /// Is `id` one of this table's rows? The index is sorted, so search it.
    pub fn table_has(&self, table: ParamTable, id: u64) -> bool {
        let (mut low, mut high) = (0u32, table.rows);
        while low < high {
            let mid = low + (high - low) / 2;
            match self.param_row_id(table, mid) {
                Some(value) if value == id => return true,
                Some(value) if value < id => low = mid + 1,
                Some(_) => high = mid,
                None => return false,
            }
        }
        false
    }

    /// Walk back from a row to the table that owns it: the rows above it must
    /// ascend into the header, and the header's row count must be the number of
    /// rows walked. Everything else in memory is rejected by that.
    fn table_at_row(&self, row: u64) -> Option<ParamTable> {
        let mut start = row;
        let mut steps = 0u64;
        loop {
            if steps > 200_000 {
                return None;
            }
            let previous = self.mem.read_u64(start.checked_sub(0x18)?).ok()?;
            let current = self.mem.read_u64(start).ok()?;
            if previous == 0 || previous >= current || previous > 0x2000_0000 {
                break;
            }
            start -= 0x18;
            steps += 1;
        }
        let head = start.checked_sub(0x40)?;
        let packed = self.mem.read_u32(head + 8).ok()?;
        if packed & 0xFFFF > 0xF0 {
            return None;
        }
        let rows = (packed >> 16) & 0xFFFF;
        if rows == 0 || u64::from(rows) != (row - start) / 0x18 + 1 {
            return None;
        }
        Some(ParamTable { head, rows })
    }

    /// The same search, from the end of the range backwards: the game's params
    /// are allocated late, at the top of the arena.
    fn scan_params_back(
        &self,
        lo: u64,
        hi: u64,
        targets: &[(usize, u64)],
        probes: &[Vec<u32>],
        out: &mut [Option<ParamTable>],
        buffer: &mut [u8],
    ) -> bool {
        let mut end = hi;
        while end > lo {
            let len = SCAN_CHUNK.min((end - lo) as usize);
            let start = end - len as u64;
            let chunk = &mut buffer[..len];
            if self.mem.read(start, chunk).is_err() {
                end = start;
                continue;
            }
            for (offset, word) in chunk.chunks_exact(8).enumerate() {
                let value = u64::from_le_bytes(word.try_into().unwrap_or([0; 8]));
                if value >> 32 != 0 {
                    continue;
                }
                let small = value as u32;
                for (index, anchor) in targets.iter() {
                    if *anchor as u32 != small || out[*index].is_some() {
                        continue;
                    }
                    let Some(table) = self.table_at_row(start + offset as u64 * 8) else {
                        continue;
                    };
                    let hits = probes[*index]
                        .iter()
                        .filter(|id| self.table_has(table, u64::from(**id)))
                        .count();
                    let need = if *anchor == PARAM_SENTINEL {
                        1
                    } else {
                        probes[*index].len() / 2 + 1
                    };
                    if hits >= need {
                        out[*index] = Some(table);
                    }
                }
                if targets.iter().all(|(index, _)| out[*index].is_some()) {
                    return true;
                }
            }
            end = start;
        }
        targets.iter().all(|(index, _)| out[*index].is_some())
    }

    /// A table addressed by its header: the only reliable way in, because it
    /// checks the row count against the whole index.
    fn table_at_head(&self, head: u64) -> Option<ParamTable> {
        let packed = self.mem.read_u32(head + 8).ok()?;
        if packed & 0xFFFF > 0xF0 {
            return None;
        }
        let rows = (packed >> 16) & 0xFFFF;
        if rows < 8 || rows > 0x8000 {
            return None;
        }
        let mut start = head + 0x40 + u64::from(rows - 1) * 0x18;
        let mut steps = 0u64;
        while steps + 1 < u64::from(rows) {
            let previous = self.mem.read_u64(start - 0x18).ok()?;
            let current = self.mem.read_u64(start).ok()?;
            if previous == 0 || previous >= current || previous > 0x2000_0000 {
                break;
            }
            start -= 0x18;
            steps += 1;
        }
        if steps + 1 != u64::from(rows) {
            return None;
        }
        Some(ParamTable { head, rows })
    }

    /// Every param header carries this at `+4`; find them all in one range and
    /// keep the ones that are a whole category's table.
    fn scan_param_headers(
        &self,
        lo: u64,
        hi: u64,
        probes: &[Vec<u32>],
        out: &mut [Option<ParamTable>],
        buffer: &mut [u8],
    ) {
        let mut at = lo;
        while at < hi {
            let len = SCAN_CHUNK.min((hi - at) as usize);
            if len < 8 {
                break;
            }
            let chunk = &mut buffer[..len];
            if self.mem.read(at, chunk).is_err() {
                at += len as u64;
                continue;
            }
            for (word, bytes) in chunk.chunks_exact(4).enumerate() {
                if u32::from_le_bytes(bytes.try_into().unwrap_or([0; 4])) != 0x0001_0000 {
                    continue;
                }
                let Some(head) = (at + word as u64 * 4).checked_sub(4) else {
                    continue;
                };
                if head < lo {
                    continue;
                }
                let Some(table) = self.table_at_head(head) else {
                    continue;
                };
                for (index, ids) in probes.iter().enumerate() {
                    if out[index].is_some() || ids.len() < 2 {
                        continue;
                    }
                    let hits = ids
                        .iter()
                        .filter(|id| self.table_has(table, u64::from(**id)))
                        .count();
                    if hits >= ids.len() / 2 + 1 {
                        out[index] = Some(table);
                    }
                }
            }
            at += len as u64;
        }
    }

    /// Find each category's param table by scanning the game's memory for an id
    /// the table must contain — for weapons the sentinel row every table ends
    /// with — then checking that the table really holds that category's ids.
    ///
    /// Weapons are looked for first and everything else is searched only around
    /// the table found that way, because the game keeps its params together:
    /// scanning every large mapping would take half a minute.
    fn discover_params(&self) -> Vec<Option<ParamTable>> {
        let cats = crate::items::Cat::ALL;
        let mut out: Vec<Option<ParamTable>> = vec![None; cats.len()];
        let probes: Vec<Vec<u32>> = cats.iter().map(|cat| crate::items::probe_ids(*cat)).collect();
        // Every probe id is an anchor: one of them is always a row, and a hit
        // is only accepted when the table holds the category's other ids too.
        let mut anchors: Vec<(usize, u64)> = Vec::new();
        for (index, ids) in probes.iter().enumerate() {
            // A weapon table ends with the sentinel row, which finds all three
            // copies of it at once; the other tables are found by their own ids
            // and confirmed by holding a second one.
            if cats[index] == crate::items::Cat::Weapon {
                anchors.push((index, PARAM_SENTINEL));
            }
            for id in [ids.first(), ids.get(ids.len() / 2), ids.last()]
                .into_iter()
                .flatten()
            {
                let entry = (index, u64::from(*id));
                if !anchors.contains(&entry) {
                    anchors.push(entry);
                }
            }
        }
        let weapons: Vec<(usize, u64)> = anchors
            .iter()
            .copied()
            .filter(|(index, _)| cats[*index] == crate::items::Cat::Weapon)
            .collect();
        let first = if weapons.is_empty() {
            anchors.clone()
        } else {
            weapons
        };
        let mut regions = process::regions(self.proc.pid);
        regions.sort_by_key(|(lo, hi)| std::cmp::Reverse(hi - lo));
        let mut buffer = vec![0u8; SCAN_CHUNK];
        let mut home = None;
        for (lo, hi) in regions {
            if hi - lo < SCAN_MIN_REGION {
                break;
            }
            self.scan_params_back(lo, hi, &first, &probes, &mut out, &mut buffer);
            if out.iter().any(|table| table.is_some()) {
                home = Some((lo, hi));
                break;
            }
        }
        if let (Some((lo, hi)), Some(center)) = (home, out.iter().flatten().next().copied()) {
            let from = center.head.saturating_sub(SCAN_WINDOW_BACK).max(lo);
            let to = center.head.saturating_add(SCAN_WINDOW_FORWARD).min(hi);
            self.scan_param_headers(from, to, &probes, &mut out, &mut buffer);
        }
        out
    }

    /// The param table holding a category's items, discovered once per session.
    pub fn item_param(&self, cat: crate::items::Cat) -> Option<ParamTable> {
        let tables = self.param_tables.get_or_init(|| self.discover_params());
        let index = crate::items::Cat::ALL
            .iter()
            .position(|candidate| *candidate == cat)?;
        tables.get(index).copied().flatten()
    }

    /// Does the game have an item param for this id?
    ///
    /// `None` means it could not be checked, which must not block a give;
    /// `Some(false)` means the id does not exist, and sending it is what makes
    /// the game fall over, so it is refused.
    pub fn has_item(&self, cat: crate::items::Cat, id: u32) -> Option<bool> {
        // Callers pass an item id; the id the routine is called with carries the
        // category nibble (`0x20000000 | 1000` for a talisman), and the table
        // stores the bare row. Strip it before comparing, or every category but
        // weapons is refused even though its row exists.
        let id = id & 0x0FFF_FFFF;
        let table = self.item_param(cat)?;
        if self.table_has(table, u64::from(id)) {
            return Some(true);
        }
        if cat != crate::items::Cat::Weapon {
            return Some(false);
        }
        // A weapon's upgrade level rides on top of the row id: a Dagger +5 is
        // row 1000000 sent as 1000005.
        let base = id - id % 100;
        Some(base != id && self.table_has(table, u64::from(base)))
    }

    /// Add an item to the inventory by calling the game's own routine, exactly
    /// like the cheat table's ItemGib: the id carries the category nibble and
    /// the upgrade level, the quantity goes in its own field.
    pub fn give_item(
        &self,
        cat: crate::items::Cat,
        id: u32,
        qty: u32,
        level: u32,
    ) -> Result<u64, String> {
        let add_item = self.add_item_addr().ok_or_else(|| {
            "AddItemFunc was not found — the game build does not match these offsets".to_string()
        })?;
        let inventory = self.inventory_manager().ok_or_else(|| {
            "the inventory manager is null — load a character before giving items".to_string()
        })?;
        if !self.in_game() {
            return Err(
                "no character is loaded — stand in the world before giving items".to_string(),
            );
        }
        // Never send an id the game has no param row for: that is what crashes.
        let plain = crate::items::plain_id(cat, id, level);
        if self.has_item(cat, plain) == Some(false) {
            return Err(format!(
                "the game has no item param for {plain:#X} — refusing to send it"
            ));
        }
        let buffer = crate::items::item_buffer(cat, id, qty, level);
        crate::spawn::log(&format!(
            "give {cat:?} id {id:#X} level {level} qty {qty} as {:#X} (row {plain:#X}), \
             inventory {inventory:#X}, routine {add_item:#X}",
            u32::from_le_bytes(buffer[0x24..0x28].try_into().unwrap_or([0; 4]))
        ));
        let result = crate::spawn::call(&self.mem, &self.proc, add_item, &buffer, &|at| {
            [inventory, at + 0x20, at, 0]
        });
        crate::spawn::log(&match &result {
            Ok(handle) => format!("give id {id:#X}: routine returned {handle:#X}"),
            Err(err) => format!("give id {id:#X}: {err}"),
        });
        result
    }

    pub fn event_flag_setter(&self) -> Option<u64> {
        self.symbols.get("EventFlagSet").copied()
    }

    pub fn event_flag_manager(&self) -> Option<u64> {
        let global = self.symbols.get("EventFlagMan").copied()?;
        let manager = self.mem.read_u64(global).ok()?;
        if manager < 0x10000 {
            None
        } else {
            Some(manager)
        }
    }

    /// Set (or clear) one event flag through the game's own setter, with the
    /// same arguments the table's `SetEventFlag` stub builds: manager in RCX, a
    /// pointer to the id in RDX, the value in R8B.
    pub fn set_event_flag(&self, id: u32, on: bool) -> Result<(), String> {
        let setter = self
            .event_flag_setter()
            .ok_or_else(|| "the event flag setter was not found in this build".to_string())?;
        let manager = self
            .event_flag_manager()
            .ok_or_else(|| "the event flag manager is null — load a save first".to_string())?;
        let bytes = id.to_le_bytes();
        crate::spawn::call(&self.mem, &self.proc, setter, &bytes, &|at| {
            [manager, at, u64::from(on), 0]
        })?;
        Ok(())
    }

    /// Walk one of the game's param lists and collect the event flag id of every
    /// row, mirroring the table's unlock loops: the list head is reached through
    /// `[PARAM_PTR]` → `+list` → `+0x80` → `+0x80`, rows start at `+0x40` with a
    /// stride of `0x18`, and each row holds its flag either at `+0` or behind the
    /// offset stored at `+8` (flag at that target `+4`).
    pub fn param_flag_ids(&self, list: u64, flag_in_sub: bool) -> Result<Vec<u32>, String> {
        let mut ptr = self
            .mem
            .read_u64(self.proc.base + offsets::PARAM_PTR)
            .map_err(|err| format!("cannot read the param repository: {err}"))?;
        for step in [list, 0x80, 0x80] {
            if ptr < 0x10000 {
                return Err("the param repository is null — load a save first".to_string());
            }
            ptr = self
                .mem
                .read_u64(ptr + step)
                .map_err(|err| format!("cannot walk the param list: {err}"))?;
        }
        let head = ptr;
        let packed = self
            .mem
            .read_u32(head + 8)
            .map_err(|err| format!("cannot read the list header: {err}"))?;
        let count = ((packed >> 16) & 0xFFFF) as usize;
        if count == 0 || count > 5000 {
            return Err(format!(
                "the list reports {count} rows — the param layout may have changed"
            ));
        }
        let mut flags = Vec::with_capacity(count);
        for row in 0..count {
            let entry = head + 0x40 + row as u64 * 0x18;
            let id = if flag_in_sub {
                let offset = self
                    .mem
                    .read_u32(entry + 8)
                    .map_err(|err| format!("row {row}: {err}"))? as u64;
                self.mem
                    .read_u32(head + offset + 4)
                    .map_err(|err| format!("row {row}: {err}"))?
            } else {
                self.mem
                    .read_u32(entry)
                    .map_err(|err| format!("row {row}: {err}"))?
            };
            if id == 0 || id > 10_000_000 {
                return Err(format!("row {row} has an implausible flag id {id:#X}"));
            }
            flags.push(id);
        }
        Ok(flags)
    }

    /// `(symbol, resolved address)` for diagnostics; `None` means the AOB scan
    /// did not find the signature in this build of the game.
    pub fn symbols_report(&self) -> Vec<(&'static str, Option<u64>)> {
        offsets::SYMBOLS
            .iter()
            .map(|def| (def.name, self.symbols.get(def.name).copied()))
            .collect()
    }

    pub fn snapshot(&self) -> Snapshot {
        let get = |field: &Field| self.read(field).ok().and_then(|v| v.u32());
        let mut snap = Snapshot {
            ng: get(&offsets::SAVE_STATUS[2]),
            name: match self.read(&offsets::SAVE_STATUS[3]) {
                Ok(Value::Name(name)) if !name.trim().is_empty() => Some(name),
                _ => None,
            },
            ..Default::default()
        };
        for field in offsets::ATTRIBUTES {
            if field.key == "level" {
                snap.level = get(field);
            }
        }
        for field in offsets::RUNES {
            match field.key {
                "runes" => snap.runes = get(field),
                "total_runes" => snap.total_runes = get(field),
                _ => {}
            }
        }
        for field in offsets::VITALS {
            let value = get(field);
            match field.key {
                "hp" => snap.hp = value,
                "max_hp" => snap.max_hp = value,
                "fp" => snap.fp = value,
                "max_fp" => snap.max_fp = value,
                "stamina" => snap.stamina = value,
                "max_stamina" => snap.max_stamina = value,
                _ => {}
            }
        }
        snap
    }

    pub fn runes(&self) -> Option<u32> {
        self.read(&offsets::RUNES[0]).ok().and_then(|v| v.u32())
    }

    pub fn set_runes(&self, amount: u32) -> Result<u32, String> {
        let field = &offsets::RUNES[0];
        self.set_number(field, amount as u64)?;
        Ok(self.runes().unwrap_or(amount))
    }

    pub fn add_runes(&self, delta: i64) -> Result<u32, String> {
        let current = self
            .runes()
            .ok_or_else(|| "cannot read the rune counter".to_string())?;
        let next = (current as i64)
            .saturating_add(delta)
            .clamp(0, u32::MAX as i64) as u32;
        self.set_runes(next)
    }

    pub fn equip(&self, slot: &Field, item_id: u32) -> Result<(), String> {
        self.set_number(slot, item_id as u64)
    }
}
