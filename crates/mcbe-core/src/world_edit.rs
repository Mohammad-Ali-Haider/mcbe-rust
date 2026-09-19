//! Experimental edits to new copies of stopped worlds. Other save invariants are separate.
use crate::{
    Result,
    leveldb::{self, Entry, MAX_BYTES},
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashSet},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};
pub type Updates = Vec<(Vec<u8>, Option<Vec<u8>>)>;
fn varint(mut n: u32, out: &mut Vec<u8>) {
    while n >= 128 {
        out.push((n as u8 & 127) | 128);
        n >>= 7;
    }
    out.push(n as u8);
}
fn sized(data: &[u8], out: &mut Vec<u8>) -> Result<()> {
    varint(
        u32::try_from(data.len()).map_err(|_| "Value length overflow")?,
        out,
    );
    out.extend_from_slice(data);
    Ok(())
}
pub fn encode_batch(sequence: u64, updates: &Updates) -> Result<Vec<u8>> {
    if updates.is_empty()
        || sequence
            .checked_add(updates.len() as u64)
            .is_none_or(|n| n > 1 << 56)
    {
        return Err("Invalid batch sequence/count".into());
    }
    let mut seen = HashSet::new();
    let mut out = sequence.to_le_bytes().to_vec();
    out.extend_from_slice(
        &u32::try_from(updates.len())
            .map_err(|_| "Batch count overflow")?
            .to_le_bytes(),
    );
    for (key, value) in updates {
        if !seen.insert(key) {
            return Err("Duplicate update key".into());
        }
        let length = key
            .len()
            .checked_add(value.as_ref().map_or(0, Vec::len))
            .and_then(|n| n.checked_add(out.len() + 11))
            .ok_or("Batch length overflow")?;
        if length > MAX_BYTES {
            return Err("Batch exceeds byte limit".into());
        }
        out.push(u8::from(value.is_some()));
        sized(key, &mut out)?;
        if let Some(value) = value {
            sized(value, &mut out)?;
        }
    }
    Ok(out)
}
pub fn append_log(prefix: &[u8], record: &[u8]) -> Result<Vec<u8>> {
    if prefix.len() > MAX_BYTES || record.len() > MAX_BYTES {
        return Err("Log exceeds byte limit".into());
    }
    leveldb::log_records(prefix)?;
    let padding = (32768 - prefix.len() % 32768) % 32768;
    let fragments = record.len().div_ceil(32761).max(1);
    if prefix.len() + padding + record.len() + 7 * fragments > MAX_BYTES {
        return Err("Resulting log exceeds byte limit".into());
    }
    let mut out = prefix.to_vec();
    out.resize(out.len() + padding, 0);
    let mut position = 0;
    let mut first = true;
    while first || position < record.len() {
        let size = (record.len() - position).min(32761);
        let last = position + size == record.len();
        let kind = match (first, last) {
            (true, true) => 1,
            (true, false) => 2,
            (false, false) => 3,
            (false, true) => 4,
        };
        let payload = &record[position..position + size];
        let mut checksum = vec![kind];
        checksum.extend_from_slice(payload);
        out.extend_from_slice(&leveldb::masked_crc(&checksum).to_le_bytes());
        out.extend_from_slice(&(size as u16).to_le_bytes());
        out.push(kind);
        out.extend_from_slice(payload);
        position += size;
        first = false;
    }
    Ok(out)
}
fn linked(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}
fn plain(path: &Path) -> Result<fs::Metadata> {
    let meta = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if linked(&meta) {
        return Err("World contains a link or reparse point".into());
    }
    Ok(meta)
}
fn hash_file(path: &Path) -> Result<String> {
    if plain(path)?.len() > MAX_BYTES as u64 {
        return Err("World file exceeds byte limit".into());
    }
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    let mut digest = Sha256::new();
    let mut buffer = [0; 65536];
    let mut total = 0;
    loop {
        let n = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        total += n;
        if total > MAX_BYTES {
            return Err("World file grew beyond limit".into());
        }
        digest.update(&buffer[..n]);
    }
    Ok(format!("{:x}", digest.finalize()))
}
pub fn tree_hashes(root: &Path) -> Result<BTreeMap<String, String>> {
    if !plain(root)?.is_dir() {
        return Err("World root must be a directory".into());
    }
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    let mut pending = vec![(root.clone(), 0)];
    let mut result = BTreeMap::new();
    let mut total = 0u64;
    let mut entries = 0;
    while let Some((directory, depth)) = pending.pop() {
        if depth > 64 {
            return Err("World directory depth limit".into());
        }
        for entry in fs::read_dir(directory).map_err(|e| e.to_string())? {
            let path = entry.map_err(|e| e.to_string())?.path();
            let metadata = plain(&path)?;
            entries += 1;
            if entries > 100_000 {
                return Err("World entry limit".into());
            }
            if !path
                .canonicalize()
                .map_err(|e| e.to_string())?
                .starts_with(&root)
            {
                return Err("World entry escapes root".into());
            }
            if metadata.is_dir() {
                pending.push((path, depth + 1));
            } else if metadata.is_file() {
                total = total
                    .checked_add(metadata.len())
                    .ok_or("World size overflow")?;
                if total > 512 * 1024 * 1024 {
                    return Err("World exceeds total byte limit".into());
                }
                let name = path
                    .strip_prefix(&root)
                    .map_err(|e| e.to_string())?
                    .to_str()
                    .ok_or("Non-UTF8 world path")?
                    .replace('\\', "/");
                result.insert(name, hash_file(&path)?);
            } else {
                return Err("Unsupported world file type".into());
            }
        }
    }
    Ok(result)
}
#[derive(Serialize)]
pub struct EditReport {
    pub source: PathBuf,
    pub destination: PathBuf,
    pub updated_keys: usize,
    pub sequence: u64,
    pub log: String,
    pub batch_bytes: usize,
    pub batch_sha256: String,
    pub source_unchanged: bool,
    pub source_hashes: BTreeMap<String, String>,
    pub destination_hashes: BTreeMap<String, String>,
}
pub fn edit_copy(source: &Path, destination: &Path, updates: &Updates) -> Result<EditReport> {
    plain(source)?;
    let source = source.canonicalize().map_err(|e| e.to_string())?;
    if fs::symlink_metadata(destination).is_ok() {
        return Err("Destination already exists".into());
    }
    let parent = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let parent = parent.canonicalize().map_err(|e| e.to_string())?;
    let destination = parent.join(destination.file_name().ok_or("Missing destination name")?);
    if destination.starts_with(&source) || source.starts_with(&destination) {
        return Err("Destination must be separate from source".into());
    }
    if !plain(&source.join("level.dat"))?.is_file() {
        return Err("Source is not a saved world".into());
    }
    let before = tree_hashes(&source)?;
    let snapshot = leveldb::read_snapshot(&source.join("db"))?;
    let sequence = snapshot
        .entries
        .values()
        .map(|e| e.sequence)
        .max()
        .unwrap_or(0)
        .max(snapshot.last_sequence)
        .checked_add(1)
        .ok_or("Sequence overflow")?;
    let batch = encode_batch(sequence, updates)?;
    let log = snapshot
        .files
        .keys()
        .filter_map(|name| {
            let digits = name.strip_suffix(".log")?;
            if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            let number = digits.parse::<u64>().ok()?;
            (number >= snapshot.current_log).then_some((number, name))
        })
        .max()
        .map(|(_, n)| n.clone())
        .ok_or("No current WAL for experimental writer")?;
    let prefix = fs::read(source.join("db").join(&log)).map_err(|e| e.to_string())?;
    let output = append_log(&prefix, &batch)?;
    if tree_hashes(&source)? != before {
        return Err("Source changed before copy".into());
    }
    fs::create_dir(&destination).map_err(|e| e.to_string())?;
    // Paths come only from the verified source tree; create_new never overwrites an unrelated file.
    for name in before.keys() {
        let target = destination.join(name);
        let source_file = source.join(name);
        plain(&source_file)?;
        fs::create_dir_all(target.parent().ok_or("Missing copy parent")?)
            .map_err(|e| e.to_string())?;
        let mut input = File::open(source_file).map_err(|e| e.to_string())?;
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(target)
            .map_err(|e| e.to_string())?;
        std::io::copy(&mut input, &mut output).map_err(|e| e.to_string())?;
    }
    if tree_hashes(&destination)? != before {
        return Err("Copied world differs from source".into());
    }
    let target_log = destination.join("db").join(&log);
    plain(&target_log)?;
    let mut file = OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(&target_log)
        .map_err(|e| e.to_string())?;
    file.write_all(&output).map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    drop(file);
    let after = leveldb::read_snapshot(&destination.join("db"))?;
    let mut expected = snapshot.entries;
    for (i, (key, value)) in updates.iter().enumerate() {
        expected.insert(
            key.clone(),
            Entry {
                sequence: sequence + i as u64,
                value: value.clone(),
            },
        );
    }
    if after.entries != expected {
        return Err("Written snapshot differs from complete expected entry map".into());
    }
    if tree_hashes(&source)? != before {
        return Err("Source changed during edit".into());
    }
    let destination_hashes = tree_hashes(&destination)?;
    let mut expected_hashes = before.clone();
    expected_hashes.insert(
        format!("db/{log}"),
        format!("{:x}", Sha256::digest(&output)),
    );
    if destination_hashes != expected_hashes {
        return Err("Unexpected changes outside destination WAL".into());
    }
    Ok(EditReport {
        source,
        destination,
        updated_keys: updates.len(),
        sequence,
        log,
        batch_bytes: batch.len(),
        batch_sha256: format!("{:x}", Sha256::digest(&batch)),
        source_unchanged: true,
        source_hashes: before,
        destination_hashes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fragmented_records_preserve_prefix_and_padding() {
        for prefix in [
            vec![],
            append_log(&[], b"prior").unwrap(),
            append_log(&[], &vec![b'x'; 32758]).unwrap(),
        ] {
            for record in [vec![], b"new".to_vec(), vec![b'z'; 120000]] {
                let output = append_log(&prefix, &record).unwrap();
                assert!(output.starts_with(&prefix));
                let mut expected = leveldb::log_records(&prefix).unwrap();
                expected.push(record);
                assert_eq!(leveldb::log_records(&output).unwrap(), expected);
            }
        }
        assert!(encode_batch((1 << 56) - 1, &vec![(vec![1], None), (vec![2], None)]).is_err());
        assert!(encode_batch(0, &vec![(vec![1], None), (vec![1], None)]).is_err());
    }
    #[test]
    fn copied_world_updates_tombstones_and_source_preservation() {
        let root = Path::new("target/test-worlds").join(format!(
            "writer-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let source = root.join("source");
        let db = source.join("db");
        fs::create_dir_all(&db).unwrap();
        fs::write(source.join("level.dat"), b"metadata fixture").unwrap();
        fs::write(db.join("CURRENT"), b"MANIFEST-000002\n").unwrap();
        let mut manifest = vec![1];
        sized(b"leveldb.BytewiseComparator", &mut manifest).unwrap();
        manifest.extend_from_slice(&[2, 3, 4, 0]);
        fs::write(
            db.join("MANIFEST-000002"),
            append_log(&[], &manifest).unwrap(),
        )
        .unwrap();
        let original = vec![
            (b"a".to_vec(), Some(b"old".to_vec())),
            (b"b".to_vec(), Some(b"removed".to_vec())),
        ];
        fs::write(
            db.join("000003.log"),
            append_log(&[], &encode_batch(1, &original).unwrap()).unwrap(),
        )
        .unwrap();
        let before = tree_hashes(&source).unwrap();
        let target = root.join("copy");
        let updates = vec![
            (b"a".to_vec(), Some(b"new".to_vec())),
            (b"b".to_vec(), None),
            (b"c".to_vec(), Some(b"added".to_vec())),
        ];
        let report = edit_copy(&source, &target, &updates).unwrap();
        assert_eq!(report.sequence, 3);
        assert_eq!(tree_hashes(&source).unwrap(), before);
        let after = leveldb::read_snapshot(&target.join("db")).unwrap();
        assert_eq!(after.get(b"a"), Some(b"new".as_slice()));
        assert_eq!(after.get(b"b"), None);
        assert_eq!(after.get(b"c"), Some(b"added".as_slice()));
        for bad in [&source, &source.join("nested"), &target] {
            assert!(edit_copy(&source, bad, &updates).is_err());
        }
        // Small fixtures remain under ignored target/test-worlds for failure inspection.
    }
}
