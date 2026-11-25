use crate::mem::Memory;

/// A byte pattern in the same notation Cheat Engine tables use: hex bytes plus
/// wildcards (`??`, `?`, `xx`). Tokens may also be glued together, which the
/// Hexinton table does in places (`48 8B 05 ???????? 80 B8 ...`), so every
/// token is re-split into byte pairs.
#[derive(Clone, Debug)]
pub struct Pattern {
    mask: Vec<Option<u8>>,
}

impl Pattern {
    pub fn parse(text: &str) -> Result<Pattern, String> {
        let mut mask = Vec::new();
        for token in text.split_whitespace() {
            let token = token.trim();
            if token.is_empty() {
                continue;
            }
            if token.chars().count() % 2 != 0 {
                return Err(format!("odd-length pattern chunk '{token}'"));
            }
            let chars: Vec<char> = token.chars().collect();
            for pair in chars.chunks(2) {
                let byte: String = pair.iter().collect();
                if byte == "??" || byte == "xx" || byte == "XX" || byte == ".." {
                    mask.push(None);
                } else {
                    let value = u8::from_str_radix(&byte, 16)
                        .map_err(|_| format!("'{byte}' is not a hex byte"))?;
                    mask.push(Some(value));
                }
            }
        }
        if mask.is_empty() {
            return Err("empty pattern".to_string());
        }
        Ok(Pattern { mask })
    }

    pub fn len(&self) -> usize {
        self.mask.len()
    }

    fn matches(&self, hay: &[u8]) -> bool {
        self.mask.iter().zip(hay).all(|(want, got)| match want {
            None => true,
            Some(byte) => *byte == *got,
        })
    }
}

const CHUNK: usize = 1 << 20;

/// Scan readable regions for a pattern. Returns absolute addresses of the first
/// `limit` hits. Chunks overlap by `pattern.len() - 1` so matches on a boundary
/// are not missed.
pub fn find(mem: &Memory, regions: &[(u64, u64)], pattern: &Pattern, limit: usize) -> Vec<u64> {
    let mut hits = Vec::new();
    if pattern.len() == 0 || limit == 0 {
        return hits;
    }
    let overlap = pattern.len().saturating_sub(1) as u64;

    for &(start, end) in regions {
        let mut pos = start;
        while pos < end {
            let want = ((end - pos) as usize).min(CHUNK);
            let mut buf = vec![0u8; want];
            if mem.read(pos, &mut buf).is_err() {
                // Unreadable page inside a mapped region: step past it.
                pos += 4096;
                continue;
            }
            let mut i = 0usize;
            while i + pattern.len() <= buf.len() {
                if pattern.matches(&buf[i..i + pattern.len()]) {
                    hits.push(pos + i as u64);
                    if hits.len() >= limit {
                        return hits;
                    }
                    i += pattern.len().max(1);
                } else {
                    i += 1;
                }
            }
            if want as u64 >= end - pos {
                break;
            }
            let step = (want as u64).saturating_sub(overlap).max(1);
            pos += step;
        }
    }
    hits
}
