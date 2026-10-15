//! Dense 16-cubed chunks with copy-on-write snapshots and sparse world allocation.
use crate::engine_blocks::BlockId;
use std::{collections::HashMap, ops::Index, sync::Arc};
pub type Position = [i32; 3];
pub type Key = [i32; 3];
pub const AIR: BlockId = u16::MAX;
pub const CELLS: usize = 4096;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chunk {
    pub ids: Box<[BlockId; CELLS]>,
    pub count: usize,
}
impl Default for Chunk {
    fn default() -> Self {
        Self {
            ids: Box::new([AIR; CELLS]),
            count: 0,
        }
    }
}
pub fn key(p: Position) -> Key {
    p.map(|v| v.div_euclid(16))
}
pub fn index(p: Position) -> usize {
    (p[0].rem_euclid(16) * 256 + p[1].rem_euclid(16) * 16 + p[2].rem_euclid(16)) as usize
}
pub fn position(k: Key, i: usize) -> Position {
    [
        k[0] * 16 + (i / 256) as i32,
        k[1] * 16 + ((i / 16) % 16) as i32,
        k[2] * 16 + (i % 16) as i32,
    ]
}
#[derive(Clone, Default, Debug, PartialEq, Eq)]
pub struct Store {
    chunks: HashMap<Key, Arc<Chunk>>,
    len: usize,
}
impl Store {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn len(&self) -> usize {
        self.len
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    pub fn clear(&mut self) {
        self.chunks.clear();
        self.len = 0;
    }
    pub fn get(&self, p: &Position) -> Option<&BlockId> {
        let v = &self.chunks.get(&key(*p))?.ids[index(*p)];
        (*v != AIR).then_some(v)
    }
    pub fn contains_key(&self, p: &Position) -> bool {
        self.get(p).is_some()
    }
    pub fn insert(&mut self, p: Position, id: BlockId) -> Option<BlockId> {
        assert_ne!(id, AIR, "Air is represented by absence");
        let c = Arc::make_mut(
            self.chunks
                .entry(key(p))
                .or_insert_with(|| Arc::new(Chunk::default())),
        );
        let previous = c.ids[index(p)];
        c.ids[index(p)] = id;
        if previous == AIR {
            c.count += 1;
            self.len += 1;
            None
        } else {
            Some(previous)
        }
    }
    pub fn remove(&mut self, p: &Position) -> Option<BlockId> {
        let k = key(*p);
        let c = Arc::make_mut(self.chunks.get_mut(&k)?);
        let old = c.ids[index(*p)];
        if old == AIR {
            return None;
        }
        c.ids[index(*p)] = AIR;
        c.count -= 1;
        self.len -= 1;
        if c.count == 0 {
            self.chunks.remove(&k);
        }
        Some(old)
    }
    pub fn iter(&self) -> impl Iterator<Item = (Position, &BlockId)> {
        self.chunks.iter().flat_map(|(k, c)| {
            c.ids
                .iter()
                .enumerate()
                .filter(|(_, id)| **id != AIR)
                .map(move |(i, id)| (position(*k, i), id))
        })
    }
    pub fn values(&self) -> impl Iterator<Item = &BlockId> {
        self.iter().map(|(_, id)| id)
    }
    pub fn keys(&self) -> impl Iterator<Item = Position> {
        self.iter().map(|(p, _)| p)
    }
    pub fn chunk_keys(&self) -> impl Iterator<Item = Key> + '_ {
        self.chunks.keys().copied()
    }
    pub fn allocated_bytes(&self) -> usize {
        self.chunks.len() * CELLS * std::mem::size_of::<BlockId>()
    }
    pub fn chunk(&self, k: Key) -> Option<&Arc<Chunk>> {
        self.chunks.get(&k)
    }
    pub fn insert_chunk(&mut self, k: Key, chunk: Arc<Chunk>) {
        if let Some(old) = self.chunks.remove(&k) {
            self.len -= old.count;
        }
        if chunk.count > 0 {
            self.len += chunk.count;
            self.chunks.insert(k, chunk);
        }
    }
    pub fn remove_chunk(&mut self, k: Key) -> Option<Arc<Chunk>> {
        let c = self.chunks.remove(&k)?;
        self.len -= c.count;
        Some(c)
    }
}
impl Extend<(Position, BlockId)> for Store {
    fn extend<T: IntoIterator<Item = (Position, BlockId)>>(&mut self, iter: T) {
        for (p, id) in iter {
            self.insert(p, id);
        }
    }
}
impl FromIterator<(Position, BlockId)> for Store {
    fn from_iter<T: IntoIterator<Item = (Position, BlockId)>>(iter: T) -> Self {
        let mut s = Self::new();
        s.extend(iter);
        s
    }
}
impl From<HashMap<Position, BlockId>> for Store {
    fn from(value: HashMap<Position, BlockId>) -> Self {
        value.into_iter().collect()
    }
}
impl<'a> IntoIterator for &'a Store {
    type Item = (Position, &'a BlockId);
    type IntoIter = Box<dyn Iterator<Item = Self::Item> + 'a>;
    fn into_iter(self) -> Self::IntoIter {
        Box::new(self.iter())
    }
}
impl Index<&Position> for Store {
    type Output = BlockId;
    fn index(&self, p: &Position) -> &BlockId {
        self.get(p).expect("Missing voxel")
    }
}
impl PartialEq<HashMap<Position, BlockId>> for Store {
    fn eq(&self, other: &HashMap<Position, BlockId>) -> bool {
        self.len() == other.len() && self.iter().all(|(p, id)| other.get(&p) == Some(id))
    }
}
impl<const N: usize> From<[(Position, BlockId); N]> for Store {
    fn from(values: [(Position, BlockId); N]) -> Self {
        values.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn signed_coordinates_and_cow_snapshots_preserve_values_without_voxel_copies() {
        let mut s = Store::new();
        for p in [[-1, -1, -1], [-16, -32, 16], [0, 0, 0], [160, 10000, -160]] {
            s.insert(p, 3);
        }
        let old = s.clone();
        assert_eq!(s.len(), 4);
        let k = key([-1, -1, -1]);
        assert!(Arc::ptr_eq(s.chunk(k).unwrap(), old.chunk(k).unwrap()));
        s.insert([-1, -1, -1], 7);
        assert_eq!(old[&[-1, -1, -1]], 3);
        assert_eq!(s[&[-1, -1, -1]], 7);
        assert!(!Arc::ptr_eq(s.chunk(k).unwrap(), old.chunk(k).unwrap()));
        s.remove(&[-1, -1, -1]);
        assert!(s.chunk(k).is_none());
        assert_eq!(s.allocated_bytes(), 3 * 8192);
    }
    #[test]
    fn dense_chunk_storage_matches_hashmap_through_mixed_mutations() {
        let mut s = Store::new();
        let mut reference = HashMap::new();
        let mut seed = 12u64;
        for _ in 0..10000 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let p = [
                (seed % 65) as i32 - 32,
                ((seed >> 8) % 65) as i32 - 32,
                ((seed >> 16) % 65) as i32 - 32,
            ];
            if seed & 2 == 0 {
                assert_eq!(
                    s.insert(p, (seed % 255) as u16),
                    reference.insert(p, (seed % 255) as u16)
                );
            } else {
                assert_eq!(s.remove(&p), reference.remove(&p));
            }
        }
        assert_eq!(s, reference);
        assert_eq!(s.iter().count(), s.len());
    }
}
