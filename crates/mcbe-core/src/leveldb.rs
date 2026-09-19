//! Read-only snapshots of stopped Bedrock LevelDB databases.
use crate::Result;
use flate2::{Decompress, FlushDecompress, Status};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};
pub const MAX_BYTES: usize = 64 * 1024 * 1024;
pub struct Cursor<'a> {
    pub data: &'a [u8],
    pub pos: usize,
}
impl<'a> Cursor<'a> {
    pub fn new(data: &'a [u8]) -> Result<Self> {
        if data.len() > MAX_BYTES {
            return Err("Input exceeds byte limit".into());
        }
        Ok(Self { data, pos: 0 })
    }
    pub fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.pos.checked_add(n).ok_or("Offset overflow")?;
        let result = self.data.get(self.pos..end).ok_or("Truncated input")?;
        self.pos = end;
        Ok(result)
    }
    pub fn fixed<const N: usize>(&mut self) -> Result<[u8; N]> {
        Ok(self.take(N)?.try_into().unwrap())
    }
    pub fn varint(&mut self, bits: u32) -> Result<u64> {
        let mut result = 0u64;
        for shift in (0..bits).step_by(7) {
            let byte = self.take(1)?[0];
            let value = u64::from(byte & 127);
            if shift + 7 > bits && value >= (1u64 << (bits - shift)) {
                return Err("Varint overflow".into());
            }
            result |= value << shift;
            if byte & 128 == 0 {
                return Ok(result);
            }
        }
        Err("Varint overflow".into())
    }
    pub fn string(&mut self) -> Result<&'a [u8]> {
        let n = self.varint(32)? as usize;
        self.take(n)
    }
}
pub fn crc32c(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for b in data {
        crc ^= u32::from(*b);
        for _ in 0..8 {
            crc = (crc >> 1) ^ if crc & 1 != 0 { 0x82f63b78 } else { 0 };
        }
    }
    !crc
}
pub fn masked_crc(data: &[u8]) -> u32 {
    crc32c(data).rotate_right(15).wrapping_add(0xa282ead8)
}
pub fn log_records(data: &[u8]) -> Result<Vec<Vec<u8>>> {
    let mut records = Vec::new();
    let mut pending: Option<Vec<u8>> = None;
    for block in data.chunks(32768) {
        let mut c = Cursor::new(block)?;
        while c.pos < block.len() {
            if block.len() - c.pos < 7 {
                if c.take(block.len() - c.pos)?.iter().any(|v| *v != 0) {
                    return Err("Nonzero log trailer".into());
                }
                break;
            }
            let checksum = u32::from_le_bytes(c.fixed()?);
            let length = u16::from_le_bytes(c.fixed()?) as usize;
            let kind = c.take(1)?[0];
            if checksum == 0 && length == 0 && kind == 0 {
                if c.take(block.len() - c.pos)?.iter().any(|v| *v != 0) {
                    return Err("Nonzero log padding".into());
                }
                break;
            }
            let payload = c.take(length)?;
            let mut checked = vec![kind];
            checked.extend_from_slice(payload);
            if masked_crc(&checked) != checksum {
                return Err("Log checksum mismatch".into());
            }
            match kind {
                1 => {
                    if pending.is_some() {
                        return Err("FULL interrupts fragmented record".into());
                    }
                    records.push(payload.to_vec());
                }
                2 => {
                    if pending.is_some() {
                        return Err("Nested FIRST record".into());
                    }
                    pending = Some(payload.to_vec());
                }
                3 | 4 => {
                    let buffer = pending.as_mut().ok_or("Fragment without FIRST")?;
                    if buffer.len() + length > MAX_BYTES {
                        return Err("Log record too large".into());
                    }
                    buffer.extend_from_slice(payload);
                    if kind == 4 {
                        records.push(pending.take().unwrap());
                    }
                }
                _ => return Err("Unsupported log record type".into()),
            }
        }
    }
    if pending.is_some() {
        return Err("Incomplete fragmented record".into());
    }
    Ok(records)
}
struct Table {
    number: u64,
    size: u64,
}
struct Manifest {
    log: u64,
    previous_log: u64,
    last_sequence: u64,
    tables: Vec<Table>,
}
fn manifest(data: &[u8]) -> Result<Manifest> {
    let (mut comparator, mut log, mut sequence) = (None, None, None);
    let mut previous_log = 0;
    let mut tables = BTreeMap::new();
    for record in log_records(data)? {
        let mut c = Cursor::new(&record)?;
        while c.pos < record.len() {
            match c.varint(32)? {
                1 => comparator = Some(c.string()?.to_vec()),
                2 => log = Some(c.varint(64)?),
                3 => {
                    c.varint(64)?;
                }
                4 => sequence = Some(c.varint(64)?),
                9 => previous_log = c.varint(64)?,
                5 => {
                    c.varint(32)?;
                    c.string()?;
                }
                6 => {
                    let level = c.varint(32)?;
                    let number = c.varint(64)?;
                    tables.remove(&(level, number));
                }
                7 => {
                    let level = c.varint(32)?;
                    let number = c.varint(64)?;
                    let size = c.varint(64)?;
                    c.string()?;
                    c.string()?;
                    tables.insert((level, number), Table { number, size });
                }
                _ => return Err("Unsupported manifest tag".into()),
            }
        }
    }
    if comparator.as_deref() != Some(b"leveldb.BytewiseComparator") {
        return Err("Unsupported comparator".into());
    }
    Ok(Manifest {
        log: log.ok_or("Missing manifest log")?,
        previous_log,
        last_sequence: sequence.ok_or("Missing manifest sequence")?,
        tables: tables.into_values().collect(),
    })
}
type Pair = (Vec<u8>, Vec<u8>);
pub fn block_entries(data: &[u8]) -> Result<Vec<Pair>> {
    if data.len() < 8 {
        return Err("Truncated restart array".into());
    }
    let count = u32::from_le_bytes(data[data.len() - 4..].try_into().unwrap()) as usize;
    if count == 0 || count > (data.len() - 4) / 4 {
        return Err("Invalid restart count".into());
    }
    let end = data.len() - 4 * (count + 1);
    let restarts: Vec<usize> = data[end..data.len() - 4]
        .chunks_exact(4)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()) as usize)
        .collect();
    if restarts[0] != 0 || restarts.windows(2).any(|w| w[0] >= w[1]) {
        return Err("Invalid restart ordering".into());
    }
    if end == 0 {
        return if restarts == [0] {
            Ok(vec![])
        } else {
            Err("Invalid empty block".into())
        };
    }
    if *restarts.last().unwrap() >= end {
        return Err("Restart outside entries".into());
    }
    let restarts: BTreeSet<_> = restarts.into_iter().collect();
    let mut seen = BTreeSet::new();
    let mut c = Cursor::new(&data[..end])?;
    let mut previous = Vec::new();
    let mut rows = Vec::new();
    while c.pos < end {
        let pos = c.pos;
        let shared = c.varint(32)? as usize;
        let extra = c.varint(32)? as usize;
        let size = c.varint(32)? as usize;
        if shared > previous.len() {
            return Err("Shared prefix exceeds previous key".into());
        }
        if restarts.contains(&pos) {
            if shared != 0 {
                return Err("Shared restart prefix".into());
            }
            seen.insert(pos);
        }
        let mut key = previous[..shared].to_vec();
        key.extend_from_slice(c.take(extra)?);
        let value = c.take(size)?.to_vec();
        previous = key.clone();
        rows.push((key, value));
    }
    if seen != restarts {
        return Err("Restart is not an entry boundary".into());
    }
    Ok(rows)
}
fn handle(data: &[u8]) -> Result<(u64, u64)> {
    let mut c = Cursor::new(data)?;
    let h = (c.varint(64)?, c.varint(64)?);
    if c.pos != data.len() {
        return Err("Trailing handle bytes".into());
    }
    Ok(h)
}
fn read_block(data: &[u8], (offset, size): (u64, u64)) -> Result<Vec<u8>> {
    let offset = usize::try_from(offset).map_err(|_| "Offset overflow")?;
    let size = usize::try_from(size).map_err(|_| "Size overflow")?;
    let end = offset.checked_add(size).ok_or("Block size overflow")?;
    if end.checked_add(5).ok_or("Block size overflow")? > data.len() - 48 {
        return Err("Table block exceeds bounds".into());
    }
    let stored = &data[offset..end];
    let mode = data[end];
    let checksum = u32::from_le_bytes(data[end + 1..end + 5].try_into().unwrap());
    if masked_crc(&data[offset..=end]) != checksum {
        return Err("Table checksum mismatch".into());
    }
    if mode == 0 {
        return Ok(stored.to_vec());
    }
    if mode != 2 && mode != 4 {
        return Err(format!("Unsupported compression type {mode}"));
    }
    let mut decoder = Decompress::new(mode == 2);
    let mut output = Vec::new();
    let mut status = Status::Ok;
    while status != Status::StreamEnd {
        let mut buffer = [0u8; 32768];
        let before_in = decoder.total_in();
        let before_out = decoder.total_out();
        status = decoder
            .decompress(
                &stored[before_in as usize..],
                &mut buffer,
                FlushDecompress::None,
            )
            .map_err(|_| "Invalid compressed table block")?;
        let produced = (decoder.total_out() - before_out) as usize;
        if output.len() + produced > MAX_BYTES {
            return Err("Decoded table block too large".into());
        }
        output.extend_from_slice(&buffer[..produced]);
        if status != Status::StreamEnd && decoder.total_in() == before_in && produced == 0 {
            return Err("Incomplete compressed table block".into());
        }
    }
    if decoder.total_in() != stored.len() as u64 {
        return Err("Trailing compressed table bytes".into());
    }
    Ok(output)
}
pub fn table_entries(data: &[u8]) -> Result<Vec<Pair>> {
    if data.len() < 48 || data[data.len() - 8..] != [0x57, 0xfb, 0x80, 0x8b, 0x24, 0x75, 0x47, 0xdb]
    {
        return Err("Invalid table footer".into());
    }
    let mut c = Cursor::new(&data[data.len() - 48..data.len() - 8])?;
    let meta = (c.varint(64)?, c.varint(64)?);
    let index = (c.varint(64)?, c.varint(64)?);
    if c.take(40 - c.pos)?.iter().any(|v| *v != 0) {
        return Err("Nonzero footer padding".into());
    }
    for (_, v) in block_entries(&read_block(data, meta)?)? {
        read_block(data, handle(&v)?)?;
    }
    let mut rows = Vec::new();
    for (_, v) in block_entries(&read_block(data, index)?)? {
        rows.extend(block_entries(&read_block(data, handle(&v)?)?)?);
    }
    Ok(rows)
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub sequence: u64,
    pub value: Option<Vec<u8>>,
}
#[derive(Default)]
pub struct Snapshot {
    pub entries: BTreeMap<Vec<u8>, Entry>,
    pub files: BTreeMap<String, String>,
    pub current_log: u64,
    pub last_sequence: u64,
}
impl Snapshot {
    pub fn get(&self, key: &[u8]) -> Option<&[u8]> {
        self.entries.get(key).and_then(|e| e.value.as_deref())
    }
    fn merge(&mut self, key: Vec<u8>, sequence: u64, value: Option<Vec<u8>>) -> Result<()> {
        if let Some(prior) = self.entries.get(&key) {
            if prior.sequence == sequence && prior.value != value {
                return Err("Conflicting values at same sequence".into());
            }
            if prior.sequence >= sequence {
                return Ok(());
            }
        }
        self.entries.insert(key, Entry { sequence, value });
        Ok(())
    }
}
fn batch(data: &[u8], snapshot: &mut Snapshot) -> Result<()> {
    let mut c = Cursor::new(data)?;
    let sequence = u64::from_le_bytes(c.fixed()?);
    let count = u32::from_le_bytes(c.fixed()?) as usize;
    if count > data.len()
        || sequence
            .checked_add(count.saturating_sub(1) as u64)
            .ok_or("Sequence overflow")?
            >= (1u64 << 56)
    {
        return Err("Invalid batch count/sequence".into());
    }
    for i in 0..count {
        let kind = c.take(1)?[0];
        if kind > 1 {
            return Err("Unsupported batch kind".into());
        }
        let key = c.string()?.to_vec();
        let value = if kind == 1 {
            Some(c.string()?.to_vec())
        } else {
            None
        };
        snapshot.merge(key, sequence + i as u64, value)?;
    }
    if c.pos != data.len() {
        return Err("Trailing write batch bytes".into());
    }
    Ok(())
}
fn names(root: &Path) -> Result<Vec<String>> {
    let mut names = fs::read_dir(root)
        .map_err(|e| e.to_string())?
        .map(|e| {
            e.map_err(|e| e.to_string()).and_then(|e| {
                e.file_name()
                    .into_string()
                    .map_err(|_| "Non-UTF8 database filename".into())
            })
        })
        .collect::<Result<Vec<_>>>()?;
    names.sort();
    Ok(names)
}
pub fn read_snapshot(directory: &Path) -> Result<Snapshot> {
    let root = directory.canonicalize().map_err(|e| e.to_string())?;
    let initial = names(&root)?;
    let mut files = BTreeMap::new();
    let mut total = 0usize;
    let mut read = |name: &str| -> Result<Vec<u8>> {
        let path = root.join(name);
        if path.is_symlink()
            || path.canonicalize().map_err(|e| e.to_string())?.parent() != Some(root.as_path())
        {
            return Err("Database file escapes directory".into());
        }
        if fs::metadata(&path).map_err(|e| e.to_string())?.len() > MAX_BYTES as u64 {
            return Err("Database file too large".into());
        }
        let raw = fs::read(path).map_err(|e| e.to_string())?;
        total += raw.len();
        if total > 512 * 1024 * 1024 {
            return Err("Snapshot too large".into());
        }
        files.insert(name.to_owned(), format!("{:x}", Sha256::digest(&raw)));
        Ok(raw)
    };
    let current = read("CURRENT")?;
    let current = std::str::from_utf8(&current).map_err(|_| "Invalid CURRENT")?;
    let manifest_name = current
        .strip_suffix('\n')
        .ok_or("Invalid CURRENT newline")?;
    let digits = manifest_name
        .strip_prefix("MANIFEST-")
        .ok_or("Invalid manifest reference")?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err("Invalid manifest reference".into());
    }
    let state = manifest(&read(manifest_name)?)?;
    let mut snapshot = Snapshot {
        current_log: state.log,
        last_sequence: state.last_sequence,
        ..Snapshot::default()
    };
    for table in state.tables {
        let mut name = format!("{:06}.ldb", table.number);
        if !root.join(&name).exists() {
            name = format!("{:06}.sst", table.number);
        }
        let raw = read(&name)?;
        if raw.len() as u64 != table.size {
            return Err("Table size disagrees with manifest".into());
        }
        for (mut key, value) in table_entries(&raw)? {
            if key.len() < 8 {
                return Err("Truncated internal key".into());
            }
            let at = key.len() - 8;
            let packed = u64::from_le_bytes(key[at..].try_into().unwrap());
            key.truncate(at);
            let kind = packed & 255;
            let sequence = packed >> 8;
            if kind > 1 || sequence > state.last_sequence || kind == 0 && !value.is_empty() {
                return Err("Invalid table sequence/type".into());
            }
            snapshot.merge(key, sequence, if kind == 1 { Some(value) } else { None })?;
        }
    }
    for name in &initial {
        if let Some(digits) = name.strip_suffix(".log")
            && !digits.is_empty()
            && digits.bytes().all(|b| b.is_ascii_digit())
        {
            let number = digits.parse::<u64>().map_err(|_| "Log number overflow")?;
            if number >= state.log || number == state.previous_log {
                for record in log_records(&read(name)?)? {
                    batch(&record, &mut snapshot)?;
                }
            }
        }
    }
    if names(&root)? != initial {
        return Err("Database directory changed during snapshot".into());
    }
    for (name, digest) in &files {
        if format!(
            "{:x}",
            Sha256::digest(fs::read(root.join(name)).map_err(|e| e.to_string())?)
        ) != *digest
        {
            return Err("Database file changed during snapshot".into());
        }
    }
    snapshot.files = files;
    Ok(snapshot)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compressed_blocks_span_output_buffers() {
        use std::io::Write;
        let plain = vec![b'X'; 100_000];
        for mode in [2, 4] {
            let stored = if mode == 2 {
                let mut encoder =
                    flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
                encoder.write_all(&plain).unwrap();
                encoder.finish().unwrap()
            } else {
                let mut encoder =
                    flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
                encoder.write_all(&plain).unwrap();
                encoder.finish().unwrap()
            };
            let size = stored.len();
            let mut table = stored;
            table.push(mode);
            let crc = masked_crc(&table);
            table.extend_from_slice(&crc.to_le_bytes());
            table.extend_from_slice(&[0; 48]);
            assert_eq!(read_block(&table, (0, size as u64)).unwrap(), plain);
            table[0] ^= 1;
            assert!(read_block(&table, (0, size as u64)).is_err());
        }
    }
    #[test]
    fn known_crc_and_varint_overflow() {
        assert_eq!(crc32c(b"123456789"), 0xe3069283);
        assert!(Cursor::new(&[255; 10]).unwrap().varint(64).is_err());
    }
    #[test]
    fn physical_checksum_and_fragments() {
        let mut checked = vec![1];
        checked.extend_from_slice(b"record");
        let mut data = masked_crc(&checked).to_le_bytes().to_vec();
        data.extend_from_slice(&6u16.to_le_bytes());
        data.extend(checked);
        assert_eq!(log_records(&data).unwrap(), vec![b"record".to_vec()]);
        data[8] ^= 1;
        assert!(log_records(&data).is_err());
    }
    #[test]
    fn prefixes_and_invalid_restarts() {
        let mut data = b"\x00\x03\x01catA\x02\x01\x01rB".to_vec();
        data.extend_from_slice(&0u32.to_le_bytes());
        data.extend_from_slice(&1u32.to_le_bytes());
        assert_eq!(
            block_entries(&data).unwrap(),
            vec![
                (b"cat".to_vec(), b"A".to_vec()),
                (b"car".to_vec(), b"B".to_vec())
            ]
        );
        data[0] = 1;
        assert!(block_entries(&data).is_err());
    }
    #[test]
    fn tombstone_sequence_and_conflict() {
        let mut s = Snapshot::default();
        s.merge(b"key".to_vec(), 3, Some(b"old".to_vec())).unwrap();
        s.merge(b"key".to_vec(), 4, None).unwrap();
        s.merge(b"key".to_vec(), 2, Some(b"stale".to_vec()))
            .unwrap();
        assert!(s.get(b"key").is_none());
        assert!(
            s.merge(b"key".to_vec(), 4, Some(b"conflict".to_vec()))
                .is_err()
        );
    }
}
