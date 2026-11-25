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
}

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

    pub fn add_item_addr(&self) -> Option<u64> {
        self.symbols.get("AddItemFunc").copied()
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
        let buffer = crate::items::item_buffer(cat, id, qty, level);
        crate::spawn::call(&self.mem, &self.proc, add_item, &buffer, &|at| {
            [inventory, at + 0x20, at, 0]
        })
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
