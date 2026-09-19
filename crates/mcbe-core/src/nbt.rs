//! Fixed-width little-endian NBT; preserves float bits, raw strings and ordering.
use crate::Result;
#[derive(Clone, Debug, PartialEq)]
pub struct Tag {
    pub name: Vec<u8>,
    pub value: Value,
}
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(u32),
    Double(u64),
    ByteArray(Vec<u8>),
    String(Vec<u8>),
    List(u8, Vec<Value>),
    Compound(Vec<Tag>),
    IntArray(Vec<i32>),
    LongArray(Vec<i64>),
}
impl Value {
    pub fn kind(&self) -> u8 {
        match self {
            Self::Byte(_) => 1,
            Self::Short(_) => 2,
            Self::Int(_) => 3,
            Self::Long(_) => 4,
            Self::Float(_) => 5,
            Self::Double(_) => 6,
            Self::ByteArray(_) => 7,
            Self::String(_) => 8,
            Self::List(..) => 9,
            Self::Compound(_) => 10,
            Self::IntArray(_) => 11,
            Self::LongArray(_) => 12,
        }
    }
}
#[derive(Clone, Copy)]
pub struct Limits {
    pub bytes: usize,
    pub elements: usize,
    pub depth: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            bytes: 64 * 1024 * 1024,
            elements: 1_000_000,
            depth: 128,
        }
    }
}
struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
    left: usize,
    limits: Limits,
}
impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8]> {
        let end = self.pos.checked_add(n).ok_or("NBT offset overflow")?;
        let result = self.data.get(self.pos..end).ok_or("Truncated NBT")?;
        self.pos = end;
        Ok(result)
    }
    fn fixed<const N: usize>(&mut self) -> Result<[u8; N]> {
        Ok(self.take(N)?.try_into().unwrap())
    }
    fn byte(&mut self) -> Result<u8> {
        Ok(self.fixed::<1>()?[0])
    }
    fn claim(&mut self, n: usize) -> Result<()> {
        self.left = self
            .left
            .checked_sub(n)
            .ok_or("NBT element limit exceeded")?;
        Ok(())
    }
    fn count(&mut self) -> Result<usize> {
        let n = i32::from_le_bytes(self.fixed()?);
        let n = usize::try_from(n).map_err(|_| "Negative NBT length")?;
        self.claim(n)?;
        Ok(n)
    }
    fn string(&mut self) -> Result<Vec<u8>> {
        let n = u16::from_le_bytes(self.fixed()?) as usize;
        Ok(self.take(n)?.to_vec())
    }
    fn payload(&mut self, kind: u8, depth: usize) -> Result<Value> {
        if depth > self.limits.depth {
            return Err("NBT depth limit exceeded".into());
        }
        Ok(match kind {
            1 => Value::Byte(self.byte()? as i8),
            2 => Value::Short(i16::from_le_bytes(self.fixed()?)),
            3 => Value::Int(i32::from_le_bytes(self.fixed()?)),
            4 => Value::Long(i64::from_le_bytes(self.fixed()?)),
            5 => Value::Float(u32::from_le_bytes(self.fixed()?)),
            6 => Value::Double(u64::from_le_bytes(self.fixed()?)),
            7 => {
                let n = self.count()?;
                Value::ByteArray(self.take(n)?.to_vec())
            }
            8 => Value::String(self.string()?),
            9 => {
                let kind = self.byte()?;
                let n = self.count()?;
                if kind > 12 || kind == 0 && n != 0 {
                    return Err("Invalid NBT list type".into());
                }
                let mut values = Vec::with_capacity(n);
                for _ in 0..n {
                    values.push(self.payload(kind, depth + 1)?);
                }
                Value::List(kind, values)
            }
            10 => {
                let mut tags = Vec::new();
                loop {
                    let kind = self.byte()?;
                    if kind == 0 {
                        break;
                    }
                    self.claim(1)?;
                    let name = self.string()?;
                    tags.push(Tag {
                        name,
                        value: self.payload(kind, depth + 1)?,
                    });
                }
                Value::Compound(tags)
            }
            11 => {
                let n = self.count()?;
                let mut values = Vec::with_capacity(n);
                for _ in 0..n {
                    values.push(i32::from_le_bytes(self.fixed()?));
                }
                Value::IntArray(values)
            }
            12 => {
                let n = self.count()?;
                let mut values = Vec::with_capacity(n);
                for _ in 0..n {
                    values.push(i64::from_le_bytes(self.fixed()?));
                }
                Value::LongArray(values)
            }
            _ => return Err("Unsupported NBT tag type".into()),
        })
    }
}
pub fn read_prefix(data: &[u8], limits: Limits) -> Result<(Tag, usize)> {
    if data.len() > limits.bytes {
        return Err("NBT byte limit exceeded".into());
    }
    let mut r = Reader {
        data,
        pos: 0,
        left: limits.elements,
        limits,
    };
    r.claim(1)?;
    let kind = r.byte()?;
    if kind == 0 {
        return Err("Root cannot be TAG_End".into());
    }
    let name = r.string()?;
    let value = r.payload(kind, 0)?;
    Ok((Tag { name, value }, r.pos))
}
pub fn read(data: &[u8], limits: Limits) -> Result<Tag> {
    let (tag, n) = read_prefix(data, limits)?;
    if n != data.len() {
        return Err("Trailing NBT bytes".into());
    }
    Ok(tag)
}
struct Writer {
    data: Vec<u8>,
    left: usize,
    limits: Limits,
}
impl Writer {
    fn emit(&mut self, bytes: &[u8]) -> Result<()> {
        if bytes.len() > self.limits.bytes - self.data.len() {
            return Err("NBT byte limit exceeded".into());
        }
        self.data.extend_from_slice(bytes);
        Ok(())
    }
    fn claim(&mut self, n: usize) -> Result<()> {
        self.left = self
            .left
            .checked_sub(n)
            .ok_or("NBT element limit exceeded")?;
        Ok(())
    }
    fn string(&mut self, s: &[u8]) -> Result<()> {
        let n = u16::try_from(s.len()).map_err(|_| "NBT string too long")?;
        self.emit(&n.to_le_bytes())?;
        self.emit(s)
    }
    fn count(&mut self, n: usize) -> Result<()> {
        self.claim(n)?;
        self.emit(
            &i32::try_from(n)
                .map_err(|_| "NBT array too long")?
                .to_le_bytes(),
        )
    }
    fn named(&mut self, tag: &Tag, depth: usize) -> Result<()> {
        self.claim(1)?;
        self.emit(&[tag.value.kind()])?;
        self.string(&tag.name)?;
        self.payload(&tag.value, depth)
    }
    fn payload(&mut self, value: &Value, depth: usize) -> Result<()> {
        if depth > self.limits.depth {
            return Err("NBT depth limit exceeded".into());
        }
        match value {
            Value::Byte(v) => self.emit(&v.to_le_bytes())?,
            Value::Short(v) => self.emit(&v.to_le_bytes())?,
            Value::Int(v) => self.emit(&v.to_le_bytes())?,
            Value::Long(v) => self.emit(&v.to_le_bytes())?,
            Value::Float(v) => self.emit(&v.to_le_bytes())?,
            Value::Double(v) => self.emit(&v.to_le_bytes())?,
            Value::ByteArray(v) => {
                self.count(v.len())?;
                self.emit(v)?;
            }
            Value::String(v) => self.string(v)?,
            Value::List(kind, values) => {
                if *kind > 12
                    || *kind == 0 && !values.is_empty()
                    || values.iter().any(|v| v.kind() != *kind)
                {
                    return Err("Invalid NBT list type".into());
                }
                self.emit(&[*kind])?;
                self.count(values.len())?;
                for v in values {
                    self.payload(v, depth + 1)?;
                }
            }
            Value::Compound(tags) => {
                for t in tags {
                    self.named(t, depth + 1)?;
                }
                self.emit(&[0])?;
            }
            Value::IntArray(values) => {
                self.count(values.len())?;
                for v in values {
                    self.emit(&v.to_le_bytes())?;
                }
            }
            Value::LongArray(values) => {
                self.count(values.len())?;
                for v in values {
                    self.emit(&v.to_le_bytes())?;
                }
            }
        }
        Ok(())
    }
}
pub fn write(tag: &Tag, limits: Limits) -> Result<Vec<u8>> {
    let mut w = Writer {
        data: Vec::new(),
        left: limits.elements,
        limits,
    };
    w.named(tag, 0)?;
    Ok(w.data)
}
#[derive(Debug, PartialEq)]
pub struct LevelDat {
    pub version: u32,
    pub root: Tag,
}
impl LevelDat {
    pub fn read(data: &[u8]) -> Result<Self> {
        if data.len() < 8 {
            return Err("Truncated level.dat header".into());
        }
        let version = u32::from_le_bytes(data[..4].try_into().unwrap());
        let size = u32::from_le_bytes(data[4..8].try_into().unwrap()) as usize;
        if size != data.len() - 8 {
            return Err("level.dat payload length mismatch".into());
        }
        let root = read(&data[8..], Limits::default())?;
        if root.value.kind() != 10 {
            return Err("level.dat root must be compound".into());
        }
        Ok(Self { version, root })
    }
    pub fn write(&self) -> Result<Vec<u8>> {
        if self.root.value.kind() != 10 {
            return Err("level.dat root must be compound".into());
        }
        let raw = write(&self.root, Limits::default())?;
        let mut result = self.version.to_le_bytes().to_vec();
        result.extend_from_slice(&(raw.len() as u32).to_le_bytes());
        result.extend(raw);
        Ok(result)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_type_preserves_raw_bytes_and_duplicates() {
        let values = vec![
            Value::Byte(-7),
            Value::Short(-30),
            Value::Int(42),
            Value::Long(i64::MIN),
            Value::Float(0x7fc00001),
            Value::Double(0x8000000000000000),
            Value::ByteArray(vec![0, 255]),
            Value::String(vec![0xff, 0, 0x80]),
            Value::List(3, vec![Value::Int(-1), Value::Int(2)]),
            Value::Compound(vec![]),
            Value::IntArray(vec![1, -3]),
            Value::LongArray(vec![i64::MAX]),
            Value::List(0, vec![]),
        ];
        let root = Tag {
            name: vec![0xff],
            value: Value::Compound(
                values
                    .into_iter()
                    .map(|value| Tag {
                        name: b"duplicate".to_vec(),
                        value,
                    })
                    .collect(),
            ),
        };
        let bytes = write(&root, Limits::default()).unwrap();
        assert_eq!(read(&bytes, Limits::default()).unwrap(), root);
        assert!(read(&bytes[..bytes.len() - 1], Limits::default()).is_err());
        assert!(
            read(
                &bytes,
                Limits {
                    elements: 2,
                    ..Limits::default()
                }
            )
            .is_err()
        );
    }
    #[test]
    fn lists_and_depth_are_checked() {
        let tag = Tag {
            name: vec![],
            value: Value::List(1, vec![Value::Int(1)]),
        };
        assert!(write(&tag, Limits::default()).is_err());
        let tag = Tag {
            name: vec![],
            value: Value::Compound(vec![Tag {
                name: vec![],
                value: Value::Int(1),
            }]),
        };
        assert!(
            write(
                &tag,
                Limits {
                    depth: 0,
                    ..Limits::default()
                }
            )
            .is_err()
        );
    }
}
