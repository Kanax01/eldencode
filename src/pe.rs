//! Minimal PE32+ export lookup for a module as the loader mapped it into the
//! game. Everything takes RVAs, so the same code works on the live image.
//!
//! We need it to reach the game's own `ntdll` exports — the table gets its
//! item buffer from Windows, and so do we.

/// Reads `len` bytes at an RVA.
pub type Reader<'a> = dyn Fn(u64, usize) -> Option<Vec<u8>> + 'a;

fn u16_at(read: &Reader, off: u64) -> Option<u16> {
    let bytes = read(off, 2)?;
    Some(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn u32_at(read: &Reader, off: u64) -> Option<u32> {
    let bytes = read(off, 4)?;
    Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

/// Absolute address of an exported name, or `None` when the module has no such
/// export (or the entry is a forwarder, which must not be jumped to directly).
pub fn find_export(read: &Reader, module_base: u64, name: &str) -> Option<u64> {
    if u16_at(read, 0)? != 0x5A4D {
        return None;
    }
    let pe = u32_at(read, 0x3C)? as u64;
    if u32_at(read, pe)? != 0x0000_4550 {
        return None;
    }
    let optional = pe + 24;
    if u16_at(read, optional)? != 0x20B {
        return None;
    }
    // DataDirectory[0] is the export table.
    let dirs = optional + 112;
    let export_rva = u32_at(read, dirs)? as u64;
    let export_size = u32_at(read, dirs + 4)? as u64;
    if export_rva == 0 || export_size == 0 {
        return None;
    }

    let count = u32_at(read, export_rva + 0x18)?;
    let functions = u32_at(read, export_rva + 0x1C)? as u64;
    let names = u32_at(read, export_rva + 0x20)? as u64;
    let ordinals = u32_at(read, export_rva + 0x24)? as u64;
    if count == 0 || count > 0x1_0000 {
        return None;
    }

    for i in 0..u64::from(count) {
        let name_rva = u32_at(read, names + i * 4)? as u64;
        let raw = read(name_rva, name.len() + 1)?;
        if raw.len() < name.len() + 1
            || &raw[..name.len()] != name.as_bytes()
            || raw[name.len()] != 0
        {
            continue;
        }
        let ordinal = u16_at(read, ordinals + i * 2)? as u64;
        let func_rva = u32_at(read, functions + ordinal * 4)? as u64;
        if func_rva == 0 {
            return None;
        }
        if func_rva >= export_rva && func_rva < export_rva + export_size {
            return None;
        }
        return Some(module_base + func_rva);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A hand-built PE32+ image with one export named "Target" at RVA 0x2000
    /// and one forwarder named "Forwarded" that resolves inside the export
    /// table, which must be rejected.
    fn image() -> Vec<u8> {
        let mut buf = vec![0u8; 0x3000];
        buf[0..2].copy_from_slice(b"MZ");
        let pe = 0x80u32;
        buf[0x3C..0x40].copy_from_slice(&pe.to_le_bytes());
        let pe = pe as usize;
        buf[pe..pe + 4].copy_from_slice(b"PE\0\0");
        let optional = pe + 24;
        buf[optional..optional + 2].copy_from_slice(&0x20Bu16.to_le_bytes());
        let dirs = optional + 112;
        let export_rva = 0x1000u32;
        buf[dirs..dirs + 4].copy_from_slice(&export_rva.to_le_bytes());
        buf[dirs + 4..dirs + 8].copy_from_slice(&0x400u32.to_le_bytes());

        let dir = export_rva as usize;
        buf[dir + 0x14..dir + 0x18].copy_from_slice(&2u32.to_le_bytes()); // functions
        buf[dir + 0x18..dir + 0x1C].copy_from_slice(&2u32.to_le_bytes()); // names
        buf[dir + 0x1C..dir + 0x20].copy_from_slice(&0x1200u32.to_le_bytes()); // functions[]
        buf[dir + 0x20..dir + 0x24].copy_from_slice(&0x1220u32.to_le_bytes()); // names[]
        buf[dir + 0x24..dir + 0x28].copy_from_slice(&0x1240u32.to_le_bytes()); // ordinals[]

        // functions[]: "Target" then a forwarder that points inside the table
        buf[0x1200..0x1204].copy_from_slice(&0x2000u32.to_le_bytes());
        buf[0x1204..0x1208].copy_from_slice(&0x1000u32.to_le_bytes());
        // names[] -> the strings
        buf[0x1220..0x1224].copy_from_slice(&0x1300u32.to_le_bytes());
        buf[0x1224..0x1228].copy_from_slice(&0x1310u32.to_le_bytes());
        // ordinals[]
        buf[0x1240..0x1242].copy_from_slice(&0u16.to_le_bytes());
        buf[0x1242..0x1244].copy_from_slice(&1u16.to_le_bytes());
        // strings
        buf[0x1300..0x1307].copy_from_slice(b"Target\0");
        buf[0x1310..0x131A].copy_from_slice(b"Forwarded\0");
        buf
    }

    #[test]
    fn resolves_a_named_export() {
        let img = image();
        let read = |off: u64, len: usize| {
            let start = usize::try_from(off).ok()?;
            img.get(start..start + len).map(<[u8]>::to_vec)
        };
        assert_eq!(
            find_export(&read, 0x1_4000_0000, "Target"),
            Some(0x1_4000_2000)
        );
        assert_eq!(find_export(&read, 0x1_4000_0000, "Nope"), None);
        assert_eq!(find_export(&read, 0x1_4000_0000, "Forwarded"), None);
    }

    #[test]
    fn rejects_non_pe_bytes() {
        let junk = vec![0u8; 64];
        let read = |off: u64, len: usize| {
            let start = usize::try_from(off).ok()?;
            junk.get(start..start + len).map(<[u8]>::to_vec)
        };
        assert_eq!(find_export(&read, 0, "Target"), None);
    }
}

