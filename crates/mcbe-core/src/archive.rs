//! Borrowed views of the observed version-1 archive container.
use crate::Result;
pub const MAGIC: u64 = 0x267052A0B125277D;
pub struct Entry<'a> {
    pub name: &'a str,
    pub descriptor_offset: usize,
    pub offset: usize,
    pub payload: &'a [u8],
}
pub fn read(data: &[u8]) -> Result<Vec<Entry<'_>>> {
    if data.len() < 16 {
        return Err("Truncated archive header".into());
    }
    if u64::from_le_bytes(data[..8].try_into().unwrap()) != MAGIC {
        return Err("Unknown archive magic".into());
    }
    if u32::from_le_bytes(data[12..16].try_into().unwrap()) != 1 {
        return Err("Unsupported archive version".into());
    }
    let count = u32::from_le_bytes(data[8..12].try_into().unwrap()) as usize;
    let base = count
        .checked_mul(256)
        .and_then(|n| n.checked_add(16))
        .ok_or("Archive size overflow")?;
    if base > data.len() {
        return Err("Descriptor table extends past EOF".into());
    }
    let mut entries = Vec::with_capacity(count);
    for i in 0..count {
        let at = 16 + i * 256;
        let size = data[at] as usize;
        if size > 247 {
            return Err("Archive name exceeds descriptor capacity".into());
        }
        let mut raw = &data[at + 1..at + 1 + size];
        while raw.last() == Some(&0) {
            raw = &raw[..raw.len() - 1];
        }
        if raw.contains(&0) {
            return Err("Embedded zero in archive name".into());
        }
        let name = std::str::from_utf8(raw).map_err(|_| "Invalid UTF-8 archive name")?;
        let relative = u32::from_le_bytes(data[at + 248..at + 252].try_into().unwrap()) as usize;
        let length = u32::from_le_bytes(data[at + 252..at + 256].try_into().unwrap()) as usize;
        let offset = base
            .checked_add(relative)
            .ok_or("Archive offset overflow")?;
        let end = offset
            .checked_add(length)
            .ok_or("Archive length overflow")?;
        let payload = data
            .get(offset..end)
            .ok_or("Archive payload extends past EOF")?;
        entries.push(Entry {
            name,
            descriptor_offset: at,
            offset,
            payload,
        });
    }
    Ok(entries)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn borrowed_payload_and_bounds() {
        let mut data = vec![0; 275];
        data[..8].copy_from_slice(&MAGIC.to_le_bytes());
        data[8] = 1;
        data[12] = 1;
        data[16] = 2;
        data[17] = b'a';
        data[268] = 3;
        data[272..].copy_from_slice(b"abc");
        let entries = read(&data).unwrap();
        assert_eq!(entries[0].name, "a");
        assert_eq!(entries[0].payload, b"abc");
        data[268] = 4;
        assert!(read(&data).is_err());
    }
}
