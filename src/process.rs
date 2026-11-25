use std::fs;
use std::path::{Path, PathBuf};

/// A running copy of the game. Under Steam/Proton the Windows exe is an
/// ordinary Linux process (Wine hosts the PE image), so everything is discovered
/// through /proc — no Windows API and no memory driver.
#[derive(Clone, Debug)]
pub struct GameProcess {
    pub pid: i32,
    pub comm: String,
    pub cmdline: String,
    pub base: u64,
    pub size: u64,
    pub prefix: Option<PathBuf>,
    pub image: Option<PathBuf>,
}

impl GameProcess {
    pub fn base_hex(&self) -> String {
        format!("{:#012X}", self.base)
    }

    pub fn size_mb(&self) -> f64 {
        self.size as f64 / (1024.0 * 1024.0)
    }
}

fn read_trim(path: &Path) -> Option<String> {
    fs::read(path)
        .ok()
        .map(|bytes| String::from_utf8_lossy(&bytes).trim().to_string())
}

fn cmdline_of(pid: i32) -> String {
    match fs::read(format!("/proc/{pid}/cmdline")) {
        Ok(bytes) => bytes
            .split(|b| *b == 0)
            .filter(|part| !part.is_empty())
            .map(|part| String::from_utf8_lossy(part).to_string())
            .collect::<Vec<_>>()
            .join(" "),
        Err(_) => String::new(),
    }
}

fn pids() -> Vec<i32> {
    let mut out = Vec::new();
    if let Ok(entries) = fs::read_dir("/proc") {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                if let Ok(pid) = name.parse::<i32>() {
                    out.push(pid);
                }
            }
        }
    }
    out.sort_unstable();
    out
}

pub fn anti_cheat_running() -> bool {
    for pid in pids() {
        if let Some(comm) = read_trim(Path::new(&format!("/proc/{pid}/comm"))) {
            let lower = comm.to_ascii_lowercase();
            if lower.contains("easyanticheat") || lower.contains("easyanticheat_eos") {
                return true;
            }
        }
    }
    false
}

fn image_base(pid: i32, needle: &str) -> (u64, u64, Option<PathBuf>) {
    let needle = needle.to_ascii_lowercase();
    let mut lowest: Option<u64> = None;
    let mut highest: u64 = 0;
    let mut offset_zero: Option<u64> = None;
    let mut image: Option<PathBuf> = None;
    if let Ok(maps) = fs::read_to_string(format!("/proc/{pid}/maps")) {
        for line in maps.lines() {
            // address perms offset dev inode path
            let mut parts = line.splitn(6, ' ');
            let Some(range) = parts.next() else { continue };
            let _perms = parts.next();
            let Some(offset) = parts.next() else { continue };
            let _dev = parts.next();
            let _inode = parts.next();
            let path = parts.next().unwrap_or("").trim();
            if path.is_empty() {
                continue;
            }
            let lower = path.to_ascii_lowercase();
            let base_name = lower.rsplit('/').next().unwrap_or(&lower);
            if !(base_name.starts_with(&needle) || lower.ends_with(&needle)) {
                continue;
            }
            let Some((start, end)) = range.split_once('-') else {
                continue;
            };
            let (Ok(start), Ok(end)) =
                (u64::from_str_radix(start, 16), u64::from_str_radix(end, 16))
            else {
                continue;
            };
            lowest = Some(lowest.map_or(start, |value: u64| value.min(start)));
            highest = highest.max(end);
            if u64::from_str_radix(offset, 16).unwrap_or(1) == 0 {
                offset_zero = Some(offset_zero.map_or(start, |value: u64| value.min(start)));
            }
            if image.is_none() {
                image = Some(PathBuf::from(path));
            }
        }
    }
    let base = offset_zero.or(lowest).unwrap_or(0);
    let size = highest.saturating_sub(base);
    (base, size, image)
}

fn prefix_of(pid: i32) -> Option<PathBuf> {
    let raw = fs::read(format!("/proc/{pid}/environ")).ok()?;
    let mut found: Option<PathBuf> = None;
    for entry in raw.split(|b| *b == 0) {
        let text = String::from_utf8_lossy(entry);
        if let Some(value) = text.strip_prefix("STEAM_COMPAT_DATA_PATH=") {
            found = Some(PathBuf::from(value).join("pfx"));
        } else if found.is_none() {
            if let Some(value) = text.strip_prefix("WINEPREFIX=") {
                found = Some(PathBuf::from(value));
            }
        }
    }
    found
}

/// Every process whose command line or comm matches `needle` (the game exe).
pub fn scan(needle: &str) -> Vec<GameProcess> {
    let needle_lower = needle.to_ascii_lowercase();
    let mut out = Vec::new();
    for pid in pids() {
        let comm = read_trim(Path::new(&format!("/proc/{pid}/comm"))).unwrap_or_default();
        let cmdline = cmdline_of(pid);
        let comm_lower = comm.to_ascii_lowercase();
        let cmd_lower = cmdline.to_ascii_lowercase();
        if !(comm_lower.contains(&needle_lower) || cmd_lower.contains(&needle_lower)) {
            continue;
        }
        let (base, size, image) = image_base(pid, &needle_lower);
        out.push(GameProcess {
            pid,
            comm: if comm.is_empty() {
                needle.to_string()
            } else {
                comm
            },
            cmdline,
            base,
            size,
            prefix: prefix_of(pid),
            image,
        });
    }
    // Prefer a match with a real image mapping; a straggler process may have
    // already unmapped the executable.
    out.sort_by_key(|p| (p.base == 0, p.pid));
    out
}

/// Readable, non-special mappings of a process.
pub fn regions(pid: i32) -> Vec<(u64, u64)> {
    let mut out = Vec::new();
    if let Ok(maps) = fs::read_to_string(format!("/proc/{pid}/maps")) {
        for line in maps.lines() {
            let mut parts = line.splitn(3, ' ');
            let Some(range) = parts.next() else { continue };
            let Some(perms) = parts.next() else { continue };
            if !perms.starts_with('r') {
                continue;
            }
            let Some((start, end)) = range.split_once('-') else {
                continue;
            };
            let (Ok(start), Ok(end)) =
                (u64::from_str_radix(start, 16), u64::from_str_radix(end, 16))
            else {
                continue;
            };
            if end > start {
                out.push((start, end));
            }
        }
    }
    out
}

/// The game image's executable pages, i.e. where a `int3` gadget can live.
pub fn image_exec_regions(proc: &GameProcess) -> Vec<(u64, u64)> {
    let mut out = Vec::new();
    if let Ok(maps) = fs::read_to_string(format!("/proc/{}/maps", proc.pid)) {
        for line in maps.lines() {
            let mut parts = line.splitn(3, ' ');
            let Some(range) = parts.next() else { continue };
            let Some(perms) = parts.next() else { continue };
            if !perms.contains('x') {
                continue;
            }
            let Some((start, end)) = range.split_once('-') else {
                continue;
            };
            let (Ok(start), Ok(end)) = (
                u64::from_str_radix(start, 16),
                u64::from_str_radix(end, 16),
            ) else {
                continue;
            };
            out.push((start, end));
        }
    }
    out
}

/// The game image's readable pages, i.e. where AOB scans run.
pub fn image_regions(proc: &GameProcess) -> Vec<(u64, u64)> {
    let start = proc.base;
    let end = proc.base.saturating_add(proc.size);
    regions(proc.pid)
        .into_iter()
        .filter(|(a, b)| *b > start && *a < end)
        .collect()
}

/// Character saves (`ER0000.sl2`) inside the Proton prefix.
pub fn saves(prefix: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let users = prefix.join("drive_c/users");
    let Ok(entries) = fs::read_dir(&users) else {
        return out;
    };
    for user in entries.flatten() {
        let root = user.path().join("AppData/Roaming/EldenRing");
        let Ok(slots) = fs::read_dir(&root) else {
            continue;
        };
        for slot in slots.flatten() {
            let file = slot.path().join("ER0000.sl2");
            if file.is_file() {
                out.push(file);
            }
        }
    }
    out
}

fn stamp() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0) as i64;
    // Howard Hinnant's civil_from_days.
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{y:04}-{m:02}-{d:02}_{:02}{:02}{:02}",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// Copy every save found in the prefix into `backups/<timestamp>/`.
pub fn backup_saves(proc: &GameProcess, backups: &Path) -> Result<Vec<PathBuf>, String> {
    let Some(prefix) = &proc.prefix else {
        return Err(
            "no Proton prefix found for this process (STEAM_COMPAT_DATA_PATH is unset)".into(),
        );
    };
    let found = saves(prefix);
    if found.is_empty() {
        return Err(format!(
            "no ER0000.sl2 under {}",
            prefix.join("drive_c/users").display()
        ));
    }
    let dest = backups.join(stamp());
    fs::create_dir_all(&dest).map_err(|err| format!("cannot create {}: {err}", dest.display()))?;
    let mut copied = Vec::new();
    for src in found {
        let slot = src
            .parent()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "slot".to_string());
        let target = dest.join(format!("{slot}_ER0000.sl2"));
        fs::copy(&src, &target).map_err(|err| format!("cannot copy {}: {err}", src.display()))?;
        copied.push(target);
    }
    Ok(copied)
}
