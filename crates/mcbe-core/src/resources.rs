//! Explicit single-pack resource lookup. Engine pack/loose precedence is unknown.
use crate::archive;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fmt, fs,
    io::Read,
    path::{Path, PathBuf},
};
const MAX_FILE: u64 = 64 * 1024 * 1024;
const MAX_TOTAL: usize = 256 * 1024 * 1024;
/// Installed blocks.json contains JSON comments. Preserve strings and line
/// positions while removing only comments outside quoted strings.
pub fn json(bytes: &[u8]) -> crate::Result<serde_json::Value> {
    if bytes.len() > 16 * 1024 * 1024 {
        return Err("Resource JSON exceeds 16 MiB".into());
    }
    let bytes = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
    let mut cleaned = bytes.to_vec();
    let mut i = 0;
    let mut quoted = false;
    let mut escaped = false;
    while i < bytes.len() {
        if quoted {
            if escaped {
                escaped = false;
            } else if bytes[i] == b'\\' {
                escaped = true;
            } else if bytes[i] == b'"' {
                quoted = false;
            }
            i += 1;
            continue;
        }
        if bytes[i] == b'"' {
            quoted = true;
            i += 1;
            continue;
        }
        if bytes[i..].starts_with(b"//") {
            while i < bytes.len() && ![b'\n', b'\r'].contains(&bytes[i]) {
                cleaned[i] = b' ';
                i += 1;
            }
        } else if bytes[i..].starts_with(b"/*") {
            cleaned[i] = b' ';
            cleaned[i + 1] = b' ';
            i += 2;
            let mut closed = false;
            while i < bytes.len() {
                if bytes[i..].starts_with(b"*/") {
                    cleaned[i] = b' ';
                    cleaned[i + 1] = b' ';
                    i += 2;
                    closed = true;
                    break;
                }
                if ![b'\n', b'\r'].contains(&bytes[i]) {
                    cleaned[i] = b' ';
                }
                i += 1;
            }
            if !closed {
                return Err("Unterminated resource JSON comment".into());
            }
        } else {
            i += 1;
        }
    }
    serde_json::from_slice(&cleaned).map_err(|e| e.to_string())
}
#[derive(Debug, PartialEq, Eq)]
pub enum ResourceError {
    Invalid(String),
    Missing(String),
    Ambiguous(String),
    Changed(String),
    Io(String),
}
impl fmt::Display for ResourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for ResourceError {}
type Result<T> = std::result::Result<T, ResourceError>;
fn io(error: std::io::Error) -> ResourceError {
    ResourceError::Io(error.to_string())
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub fn validate_path(logical: &str) -> Result<()> {
    if logical.is_empty() || logical.contains(['\\', ':', '\0']) || logical.starts_with('/') {
        return Err(ResourceError::Invalid(
            "Resource path must be a relative slash-separated path".into(),
        ));
    }
    for part in logical.split('/') {
        let base = part.split('.').next().unwrap_or("").to_ascii_uppercase();
        if part.is_empty()
            || part == "."
            || part == ".."
            || part.ends_with(['.', ' '])
            || ["CON", "PRN", "AUX", "NUL"].contains(&base.as_str())
            || (1..=9).any(|n| base == format!("COM{n}") || base == format!("LPT{n}"))
        {
            return Err(ResourceError::Invalid(
                "Unsafe resource path component".into(),
            ));
        }
    }
    Ok(())
}
#[derive(Clone, Debug, Serialize)]
pub struct Origin {
    pub storage: &'static str,
    pub container: PathBuf,
    pub member: Option<String>,
    pub descriptor_offset: Option<usize>,
    pub payload_offset: Option<usize>,
    pub file_sha256: String,
    pub payload_sha256: String,
}
pub struct Resource {
    pub bytes: Vec<u8>,
    pub origins: Vec<Origin>,
}
struct Member {
    offset: usize,
    length: usize,
    descriptor: usize,
}
struct Container {
    bytes: Vec<u8>,
    members: BTreeMap<String, Member>,
}
pub struct Pack {
    root: PathBuf,
    files: BTreeMap<PathBuf, String>,
    containers: BTreeMap<PathBuf, Container>,
    total: usize,
}
impl Pack {
    pub fn open(root: &Path) -> Result<Self> {
        let root = root.canonicalize().map_err(io)?;
        if !root.is_dir() {
            return Err(ResourceError::Invalid(
                "Pack root must be a directory".into(),
            ));
        }
        Ok(Self {
            root,
            files: BTreeMap::new(),
            containers: BTreeMap::new(),
            total: 0,
        })
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn source_hashes(&self) -> &BTreeMap<PathBuf, String> {
        &self.files
    }
    fn physical(&self, path: &Path) -> Result<Option<PathBuf>> {
        match path.canonicalize() {
            Ok(canonical) => {
                if !canonical.starts_with(&self.root) || !canonical.is_file() {
                    return Err(ResourceError::Invalid(
                        "Resource escapes pack or is not a file".into(),
                    ));
                }
                Ok(Some(canonical))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(io(e)),
        }
    }
    fn bytes(&mut self, path: &Path) -> Result<Vec<u8>> {
        let file = fs::File::open(path).map_err(io)?;
        if file.metadata().map_err(io)?.len() > MAX_FILE {
            return Err(ResourceError::Invalid(
                "Resource container exceeds 64 MiB".into(),
            ));
        }
        let mut bytes = Vec::new();
        file.take(MAX_FILE + 1)
            .read_to_end(&mut bytes)
            .map_err(io)?;
        if bytes.len() as u64 > MAX_FILE {
            return Err(ResourceError::Invalid(
                "Resource grew beyond byte bound".into(),
            ));
        }
        let hash = digest(&bytes);
        if self.files.get(path).is_some_and(|old| *old != hash) {
            return Err(ResourceError::Changed(path.display().to_string()));
        }
        if !self.files.contains_key(path) {
            self.total = self
                .total
                .checked_add(bytes.len())
                .ok_or_else(|| ResourceError::Invalid("Resource total overflow".into()))?;
            if self.total > MAX_TOTAL {
                return Err(ResourceError::Invalid(
                    "Pack snapshot exceeds 256 MiB".into(),
                ));
            }
            self.files.insert(path.to_path_buf(), hash);
        }
        Ok(bytes)
    }
    pub fn read(&mut self, logical: &str) -> Result<Resource> {
        validate_path(logical)?;
        let mut found: Option<Resource> = None;
        if let Some(path) = self.physical(&self.root.join(logical))? {
            let bytes = self.bytes(&path)?;
            let hash = digest(&bytes);
            found = Some(Resource {
                bytes,
                origins: vec![Origin {
                    storage: "loose",
                    container: path,
                    member: None,
                    descriptor_offset: None,
                    payload_offset: None,
                    file_sha256: hash.clone(),
                    payload_sha256: hash,
                }],
            });
        }
        if let Some((parent, name)) = logical.rsplit_once('/') {
            let path = self
                .root
                .join("__brarchive")
                .join(format!("{parent}.brarchive"));
            if let Some(path) = self.physical(&path)? {
                if !self.containers.contains_key(&path) {
                    let bytes = self.bytes(&path)?;
                    let mut members = BTreeMap::new();
                    for entry in archive::read(&bytes).map_err(ResourceError::Invalid)? {
                        validate_path(entry.name)?;
                        if members
                            .insert(
                                entry.name.to_owned(),
                                Member {
                                    offset: entry.offset,
                                    length: entry.payload.len(),
                                    descriptor: entry.descriptor_offset,
                                },
                            )
                            .is_some()
                        {
                            return Err(ResourceError::Ambiguous(
                                "Duplicate archive member name".into(),
                            ));
                        }
                    }
                    self.containers
                        .insert(path.clone(), Container { bytes, members });
                }
                let container = &self.containers[&path];
                if let Some(member) = container.members.get(name) {
                    let bytes = &container.bytes[member.offset..member.offset + member.length];
                    let origin = Origin {
                        storage: "archive",
                        container: path.clone(),
                        member: Some(name.into()),
                        descriptor_offset: Some(member.descriptor),
                        payload_offset: Some(member.offset),
                        file_sha256: self.files[&path].clone(),
                        payload_sha256: digest(bytes),
                    };
                    if let Some(loose) = &mut found {
                        if loose.bytes != bytes {
                            return Err(ResourceError::Ambiguous(format!(
                                "Loose/archive bytes disagree for {logical}"
                            )));
                        }
                        loose.origins.push(origin);
                    } else {
                        found = Some(Resource {
                            bytes: bytes.to_vec(),
                            origins: vec![origin],
                        });
                    }
                }
            }
        }
        found.ok_or_else(|| ResourceError::Missing(logical.into()))
    }
    pub fn verify_unchanged(&self) -> Result<()> {
        for (path, hash) in &self.files {
            let file = fs::File::open(path).map_err(io)?;
            if file.metadata().map_err(io)?.len() > MAX_FILE {
                return Err(ResourceError::Changed(path.display().to_string()));
            }
            let mut bytes = Vec::new();
            file.take(MAX_FILE + 1)
                .read_to_end(&mut bytes)
                .map_err(io)?;
            if digest(&bytes) != *hash {
                return Err(ResourceError::Changed(path.display().to_string()));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    fn folder() -> PathBuf {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::current_dir().unwrap().join(format!(
            "../../target/test-resources/{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(path.join("__brarchive")).unwrap();
        fs::create_dir_all(path.join("textures")).unwrap();
        path
    }
    fn archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut data = vec![0; 16 + 256 * entries.len()];
        data[..8].copy_from_slice(&crate::archive::MAGIC.to_le_bytes());
        data[8..12].copy_from_slice(&(entries.len() as u32).to_le_bytes());
        data[12] = 1;
        let mut offset = 0u32;
        for (i, (name, payload)) in entries.iter().enumerate() {
            let at = 16 + i * 256;
            data[at] = name.len() as u8;
            data[at + 1..at + 1 + name.len()].copy_from_slice(name.as_bytes());
            data[at + 248..at + 252].copy_from_slice(&offset.to_le_bytes());
            data[at + 252..at + 256].copy_from_slice(&(payload.len() as u32).to_le_bytes());
            offset += payload.len() as u32;
            data.extend_from_slice(payload);
        }
        data
    }
    #[test]
    fn agreement_conflict_and_snapshot_change() {
        let root = folder();
        let bytes = archive(&[("a.json", b"same")]);
        fs::write(root.join("__brarchive/textures.brarchive"), bytes).unwrap();
        fs::write(root.join("textures/a.json"), b"same").unwrap();
        let mut pack = Pack::open(&root).unwrap();
        let resource = pack.read("textures/a.json").unwrap();
        assert_eq!(resource.origins.len(), 2);
        assert_eq!(resource.bytes, b"same");
        pack.verify_unchanged().unwrap();
        fs::write(root.join("textures/a.json"), b"different").unwrap();
        assert!(matches!(
            pack.verify_unchanged(),
            Err(ResourceError::Changed(_))
        ));
        assert!(matches!(
            Pack::open(&root).unwrap().read("textures/a.json"),
            Err(ResourceError::Ambiguous(_))
        ));
    }
    #[test]
    fn archive_only_missing_and_duplicate_names() {
        let root = folder();
        fs::write(
            root.join("__brarchive/textures.brarchive"),
            archive(&[("a.png", b"pixels")]),
        )
        .unwrap();
        let mut pack = Pack::open(&root).unwrap();
        assert_eq!(
            pack.read("textures/a.png").unwrap().origins[0].payload_offset,
            Some(272)
        );
        assert!(matches!(
            pack.read("textures/missing.png"),
            Err(ResourceError::Missing(_))
        ));
        fs::write(
            root.join("__brarchive/textures.brarchive"),
            archive(&[("a.png", b"one"), ("a.png", b"two")]),
        )
        .unwrap();
        assert!(matches!(
            Pack::open(&root).unwrap().read("textures/a.png"),
            Err(ResourceError::Ambiguous(_))
        ));
    }
    #[test]
    fn unsafe_paths_are_rejected() {
        for path in [
            "",
            "../a",
            "/a",
            "C:/a",
            "a\\b",
            "a//b",
            "a/./b",
            "a/../b",
            "CON.png",
            "a/LPT9.json",
            "a/end.",
            "a/last ",
        ] {
            assert!(validate_path(path).is_err(), "{path}");
        }
        assert!(validate_path("textures/blocks/stone.png").is_ok());
    }
    #[test]
    fn resource_json_preserves_strings_and_accepts_comments() {
        let text = r#"{
            // installed resource comment
            "url":"https://example.test/a/*literal*/",
            "quoted":"a\"//b", /* another comment */
            "unicode":"雪"
        }"#;
        let parsed = json(text.as_bytes()).unwrap();
        assert_eq!(parsed["url"], "https://example.test/a/*literal*/");
        assert_eq!(parsed["quoted"], "a\"//b");
        assert_eq!(parsed["unicode"], "雪");
        assert!(json(b"{} /* not closed").is_err());
        assert!(json(b"{\"a\":1,}").is_err());
    }
}
