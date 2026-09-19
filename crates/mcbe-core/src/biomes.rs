//! Bounded stopped-Overworld Data3D biome palettes, independent of height repair.
use crate::{
    Result, heightmaps,
    leveldb::{Cursor, Snapshot},
};
use std::{collections::HashMap, sync::Arc};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Storage {
    pub bits: u8,
    pub indices: Vec<u16>,
    pub palette: Vec<u32>,
}
impl Storage {
    pub fn at(&self, x: u8, y: u8, z: u8) -> Result<u32> {
        if x >= 16 || y >= 16 || z >= 16 {
            return Err("Local biome coordinate outside subchunk".into());
        }
        let index = *self
            .indices
            .get((usize::from(x) << 8) | (usize::from(z) << 4) | usize::from(y))
            .ok_or("Missing biome index")?;
        self.palette
            .get(usize::from(index))
            .copied()
            .ok_or_else(|| "Biome palette index outside palette".into())
    }
}
#[derive(Clone, Debug)]
pub struct Column {
    pub headers: Vec<u8>,
    pub layers: Vec<Arc<Storage>>,
}
impl Column {
    /// Decode the suffix after Data3D's 512 height bytes; exactly 24 Overworld layers.
    pub fn decode(data: &[u8]) -> Result<Self> {
        let mut cursor = Cursor::new(data)?;
        let mut layers: Vec<Arc<Storage>> = Vec::with_capacity(24);
        let mut headers = Vec::with_capacity(24);
        for _ in 0..24 {
            let header = cursor.take(1)?[0];
            headers.push(header);
            if header == 255 {
                let previous = layers
                    .last()
                    .ok_or("First biome layer cannot reuse predecessor")?;
                let indices = (0..4096)
                    .map(|i| previous.indices[(i & !15) | 15])
                    .collect::<Vec<_>>();
                layers.push(if indices == previous.indices {
                    previous.clone()
                } else {
                    Arc::new(Storage {
                        bits: previous.bits,
                        indices,
                        palette: previous.palette.clone(),
                    })
                });
                continue;
            }
            let bits = header >> 1;
            if header & 1 == 0 || ![0, 1, 2, 3, 4, 5, 6, 8, 16].contains(&bits) {
                return Err(format!("Unsupported biome storage header {header}"));
            }
            let (indices, size) = if bits == 0 {
                (vec![0; 4096], 1)
            } else {
                let per_word = 32 / usize::from(bits);
                let words = cursor
                    .take(4096usize.div_ceil(per_word) * 4)?
                    .chunks_exact(4)
                    .map(|v| u32::from_le_bytes(v.try_into().unwrap()))
                    .collect::<Vec<_>>();
                let indices = (0..4096)
                    .map(|i| {
                        ((words[i / per_word] >> ((i % per_word) * usize::from(bits)))
                            & ((1u32 << bits) - 1)) as u16
                    })
                    .collect();
                let size = u32::from_le_bytes(cursor.fixed()?);
                if size == 0 || size > 4096.min(1u32 << bits) {
                    return Err("Invalid biome palette size".into());
                }
                (indices, size as usize)
            };
            let palette = (0..size)
                .map(|_| cursor.fixed().map(u32::from_le_bytes))
                .collect::<Result<Vec<_>>>()?;
            if indices.iter().any(|&i| usize::from(i) >= size) {
                return Err("Biome palette index outside palette".into());
            }
            layers.push(Arc::new(Storage {
                bits,
                indices,
                palette,
            }));
        }
        if cursor.pos != data.len() {
            return Err("Trailing biome storage bytes".into());
        }
        Ok(Self { headers, layers })
    }
    pub fn at(&self, x: u8, y: i32, z: u8) -> Result<u32> {
        if !(-64..320).contains(&y) {
            return Err("Overworld biome Y outside measured bounds".into());
        }
        self.layers
            .get((y.div_euclid(16) + 4) as usize)
            .ok_or("Missing biome layer")?
            .at(x, y.rem_euclid(16) as u8, z)
    }
}
pub struct World<'a> {
    snapshot: &'a Snapshot,
    cache: HashMap<[i32; 2], Column>,
}
impl<'a> World<'a> {
    pub fn new(snapshot: &'a Snapshot) -> Self {
        Self {
            snapshot,
            cache: HashMap::new(),
        }
    }
    pub fn at(&mut self, position: [i32; 3]) -> Result<u32> {
        let [x, y, z] = position;
        if !(-64..320).contains(&y) {
            return Err("Overworld biome Y outside measured bounds".into());
        }
        let key = [x.div_euclid(16), z.div_euclid(16)];
        if !self.cache.contains_key(&key) {
            let data = self
                .snapshot
                .get(&heightmaps::column_key(key[0], key[1]))
                .ok_or("Absent saved biome column")?;
            let suffix = data.get(512..).ok_or("Truncated Data3D height prefix")?;
            self.cache.insert(key, Column::decode(suffix)?);
        }
        self.cache[&key].at(x.rem_euclid(16) as u8, y, z.rem_euclid(16) as u8)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn uniform_reuse_bounds_and_malformed_data() {
        let mut data = vec![1];
        data.extend(1u32.to_le_bytes());
        data.extend([255; 23]);
        let column = Column::decode(&data).unwrap();
        assert!(Arc::ptr_eq(&column.layers[0], &column.layers[23]));
        assert_eq!(column.at(15, -64, 15).unwrap(), 1);
        assert_eq!(column.at(0, 319, 0).unwrap(), 1);
        assert!(column.at(16, 0, 0).is_err());
        assert!(column.at(0, -65, 0).is_err());
        assert!(column.at(0, 320, 0).is_err());
        for end in 0..data.len() {
            assert!(Column::decode(&data[..end]).is_err());
        }
        data.push(0);
        assert!(Column::decode(&data).is_err());
        data[0] = 255;
        assert!(Column::decode(&data).is_err());
        data[0] = 0;
        assert!(Column::decode(&data).is_err());
    }
    #[test]
    fn packed_padding_and_axis_order() {
        for bits in [1, 2, 3, 4, 5, 6, 8, 16] {
            let mut data = vec![(bits << 1) | 1];
            let per_word = 32 / usize::from(bits);
            let mut words = vec![0u32; 4096usize.div_ceil(per_word)];
            let index = (15 << 8) | (7 << 4) | 3;
            words[index / per_word] |= 1 << ((index % per_word) * usize::from(bits));
            for word in words {
                data.extend(word.to_le_bytes());
            }
            data.extend(2u32.to_le_bytes());
            data.extend(1u32.to_le_bytes());
            data.extend(42u32.to_le_bytes());
            data.extend([255; 23]);
            let column = Column::decode(&data).unwrap();
            assert_eq!(column.at(15, -61, 7).unwrap(), 42);
            assert_eq!(column.at(15, -57, 3).unwrap(), 1);
            assert_eq!(column.at(15, -45, 7).unwrap(), 1);
            // Preserve packed data but truncate the numeric palette to one entry.
            let at = 1 + 4096usize.div_ceil(per_word) * 4;
            data[at..at + 4].copy_from_slice(&1u32.to_le_bytes());
            assert!(Column::decode(&data).is_err());
        }
    }
    #[test]
    fn extension_keeps_top_slice_across_all_later_layers() {
        let mut data = vec![3];
        let mut words = [0u32; 128];
        for (x, y, z) in [(15, 15, 7), (15, 3, 2)] {
            let index = (x << 8) | (z << 4) | y;
            words[index / 32] |= 1 << (index % 32);
        }
        for word in words {
            data.extend(word.to_le_bytes());
        }
        data.extend(2u32.to_le_bytes());
        data.extend(1u32.to_le_bytes());
        data.extend(4u32.to_le_bytes());
        data.extend([255; 23]);
        let column = Column::decode(&data).unwrap();
        assert_eq!(column.at(15, -61, 2).unwrap(), 4);
        assert_eq!(column.at(15, -45, 2).unwrap(), 1);
        assert_eq!(column.at(15, -48, 7).unwrap(), 4);
        assert_eq!(column.at(15, 318, 7).unwrap(), 4);
        assert!(!Arc::ptr_eq(&column.layers[0], &column.layers[1]));
        assert!(Arc::ptr_eq(&column.layers[1], &column.layers[23]));
    }
}
