//! Movable saved-column residency and edits that survive eviction.
use crate::{Result, columns::Column, cubes::Cube};
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    sync::Arc,
};
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Bounds {
    pub min: [i32; 3],
    pub max: [i32; 3],
}
impl Bounds {
    pub fn around(center: [i32; 2], radius: u8) -> Result<Self> {
        if radius > 3 {
            return Err("Development column radius must be at most three".into());
        }
        let extent = |v: i32, offset: i32| {
            v.checked_add(offset)
                .and_then(|n| n.checked_mul(16))
                .ok_or_else(|| "Column window extent overflow".to_owned())
        };
        let r = i32::from(radius);
        Ok(Self {
            min: [extent(center[0], -r)?, -64, extent(center[1], -r)?],
            max: [extent(center[0], r + 1)?, 320, extent(center[1], r + 1)?],
        })
    }
    pub fn contains(self, p: [i32; 3]) -> bool {
        (0..3).all(|a| p[a] >= self.min[a] && p[a] < self.max[a])
    }
    pub fn index(self, p: [i32; 3]) -> Option<usize> {
        if !self.contains(p) {
            return None;
        }
        let y = usize::try_from(i64::from(self.max[1]) - i64::from(self.min[1])).ok()?;
        let z = usize::try_from(i64::from(self.max[2]) - i64::from(self.min[2])).ok()?;
        let local = |a: usize| usize::try_from(i64::from(p[a]) - i64::from(self.min[a])).ok();
        local(0)?
            .checked_mul(y)?
            .checked_mul(z)?
            .checked_add(local(1)?.checked_mul(z)?)?
            .checked_add(local(2)?)
    }
    pub fn volume(self) -> Result<usize> {
        (0..3).try_fold(1usize, |n, a| {
            let size = i64::from(self.max[a]) - i64::from(self.min[a]);
            if size <= 0 {
                return Err("Invalid grid extent".into());
            }
            n.checked_mul(size as usize)
                .ok_or_else(|| "Grid volume overflow".into())
        })
    }
}
pub enum State {
    Ready(Arc<Column>),
    Missing,
    Rejected(String),
}
#[derive(Clone, Debug, Serialize)]
pub struct Report {
    pub center: [i32; 2],
    pub radius: u8,
    pub requested: usize,
    pub ready: usize,
    pub missing: usize,
    pub rejected: BTreeMap<String, String>,
    pub loaded: usize,
    pub evicted: usize,
}
pub struct Cache {
    center: Option<[i32; 2]>,
    radius: u8,
    pub residents: BTreeMap<[i32; 2], State>,
    edits: BTreeMap<[i32; 3], Option<Cube>>,
}
impl Cache {
    pub fn new(radius: u8) -> Result<Self> {
        Bounds::around([0, 0], radius)?;
        Ok(Self {
            center: None,
            radius,
            residents: BTreeMap::new(),
            edits: BTreeMap::new(),
        })
    }
    pub fn center(&self) -> Option<[i32; 2]> {
        self.center
    }
    pub fn radius(&self) -> u8 {
        self.radius
    }
    pub fn bounds(&self) -> Result<Bounds> {
        Bounds::around(
            self.center.ok_or("Column cache is not initialized")?,
            self.radius,
        )
    }
    pub fn ready(&self, p: [i32; 3]) -> bool {
        (-64..320).contains(&p[1])
            && matches!(
                self.residents
                    .get(&[p[0].div_euclid(16), p[2].div_euclid(16)]),
                Some(State::Ready(_))
            )
    }
    pub fn refresh(
        &mut self,
        center: [i32; 2],
        mut load: impl FnMut([i32; 2]) -> Result<Option<Column>>,
    ) -> Result<Report> {
        Bounds::around(center, self.radius)?;
        let r = i32::from(self.radius);
        let desired: BTreeSet<_> = (-r..=r)
            .flat_map(|x| (-r..=r).map(move |z| [center[0] + x, center[1] + z]))
            .collect();
        let evicted = self
            .residents
            .keys()
            .filter(|p| !desired.contains(*p))
            .count();
        self.residents.retain(|p, _| desired.contains(p));
        let mut loaded = 0;
        for p in desired {
            if let std::collections::btree_map::Entry::Vacant(entry) = self.residents.entry(p) {
                let state = match load(p) {
                    Ok(Some(column)) if column.position == p => State::Ready(Arc::new(column)),
                    Ok(Some(_)) => State::Rejected("Loader returned a different column".into()),
                    Ok(None) => State::Missing,
                    Err(error) => State::Rejected(error),
                };
                entry.insert(state);
                loaded += 1;
            }
        }
        self.center = Some(center);
        Ok(self.report(loaded, evicted))
    }
    pub fn report(&self, loaded: usize, evicted: usize) -> Report {
        Report {
            center: self.center.unwrap_or([0, 0]),
            radius: self.radius,
            requested: self.residents.len(),
            ready: self
                .residents
                .values()
                .filter(|v| matches!(v, State::Ready(_)))
                .count(),
            missing: self
                .residents
                .values()
                .filter(|v| matches!(v, State::Missing))
                .count(),
            rejected: self
                .residents
                .iter()
                .filter_map(|(p, state)| {
                    if let State::Rejected(reason) = state {
                        Some((format!("{},{}", p[0], p[1]), reason.clone()))
                    } else {
                        None
                    }
                })
                .collect(),
            loaded,
            evicted,
        }
    }
    pub fn edit(&mut self, p: [i32; 3], value: Option<Cube>) -> Result<()> {
        if !self.ready(p) {
            return Err("Cannot edit an unavailable saved column".into());
        }
        let Some(State::Ready(column)) = self
            .residents
            .get(&[p[0].div_euclid(16), p[2].div_euclid(16)])
        else {
            unreachable!()
        };
        if column.containers.contains_key(&p) {
            return Err("Container edits require block entity lifecycle support".into());
        }
        if column.cells.get(&p).copied() == value {
            self.edits.remove(&p);
        } else {
            self.edits.insert(p, value);
        }
        Ok(())
    }
    pub fn edits(&self) -> &BTreeMap<[i32; 3], Option<Cube>> {
        &self.edits
    }
    pub fn base_cells(&self) -> HashMap<[i32; 3], Cube> {
        self.residents
            .values()
            .filter_map(|state| {
                if let State::Ready(column) = state {
                    Some(column)
                } else {
                    None
                }
            })
            .flat_map(|column| column.cells.iter().map(|(p, k)| (*p, *k)))
            .collect()
    }
    pub fn cells(&self) -> HashMap<[i32; 3], Cube> {
        let mut cells = self.base_cells();
        for (p, kind) in &self.edits {
            if self.ready(*p) {
                if let Some(kind) = kind {
                    cells.insert(*p, *kind);
                } else {
                    cells.remove(p);
                }
            }
        }
        cells
    }
    pub fn containers(&self) -> HashMap<[i32; 3], crate::container_geometry::ContainerBlock> {
        self.residents
            .values()
            .filter_map(|s| {
                if let State::Ready(c) = s {
                    Some(c)
                } else {
                    None
                }
            })
            .flat_map(|c| c.containers.iter().map(|(p, k)| (*p, *k)))
            .collect()
    }
    pub fn biome(&self, p: [i32; 3]) -> Result<u32> {
        if !self.ready(p) {
            return Err("Unavailable saved biome column".into());
        }
        let Some(State::Ready(column)) = self
            .residents
            .get(&[p[0].div_euclid(16), p[2].div_euclid(16)])
        else {
            unreachable!()
        };
        column
            .biomes
            .at(p[0].rem_euclid(16) as u8, p[1], p[2].rem_euclid(16) as u8)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn column(p: [i32; 2]) -> Result<Option<Column>> {
        let mut data = vec![1];
        data.extend(1u32.to_le_bytes());
        data.extend([255; 23]);
        Ok(Some(Column {
            position: p,
            cells: BTreeMap::from([([p[0] * 16, -61, p[1] * 16], Cube::Grass)]),
            containers: BTreeMap::new(),
            biomes: crate::biomes::Column::decode(&data)?,
        }))
    }
    #[test]
    fn sliding_window_reuses_columns_and_keeps_edits_across_eviction() {
        let mut cache = Cache::new(1).unwrap();
        let first = cache.refresh([-1, -1], column).unwrap();
        assert_eq!((first.ready, first.loaded, first.evicted), (9, 9, 0));
        let p = [-16, -61, -16];
        cache.edit(p, Some(Cube::Stone)).unwrap();
        cache.edit([-15, -40, -15], Some(Cube::Dirt)).unwrap();
        let next = cache.refresh([0, -1], column).unwrap();
        assert_eq!((next.loaded, next.evicted), (3, 3));
        cache.refresh([8, 8], column).unwrap();
        assert!(!cache.ready(p));
        assert_eq!(cache.edits.len(), 2);
        assert_eq!(cache.residents.len(), 9);
        cache.refresh([-1, -1], column).unwrap();
        assert_eq!(cache.cells()[&p], Cube::Stone);
        assert_eq!(cache.cells()[&[-15, -40, -15]], Cube::Dirt);
        cache.edit(p, Some(Cube::Grass)).unwrap();
        cache.edit([-15, -40, -15], None).unwrap();
        assert!(cache.edits.is_empty());
    }

    #[test]
    fn containers_survive_residency_changes_without_becoming_cube_edits() {
        let load = |p| {
            let mut c = column(p)?.unwrap();
            c.containers.insert(
                [p[0] * 16 + 2, 64, p[1] * 16 + 2],
                crate::container_geometry::ContainerBlock::BARREL,
            );
            Ok(Some(c))
        };
        let mut cache = Cache::new(1).unwrap();
        cache.refresh([0, 0], load).unwrap();
        let p = [2, 64, 2];
        assert_eq!(
            cache.containers().get(&p),
            Some(&crate::container_geometry::ContainerBlock::BARREL)
        );
        assert!(!cache.cells().contains_key(&p));
        assert!(cache.edit(p, Some(Cube::Stone)).is_err());
        assert!(cache.edit(p, None).is_err());
        cache.refresh([3, 0], load).unwrap();
        assert!(!cache.containers().contains_key(&p));
        cache.refresh([0, 0], load).unwrap();
        assert!(cache.containers().contains_key(&p));
        assert!(cache.edits().is_empty());
    }
    #[test]
    fn unavailable_columns_are_distinct_from_recorded_air() {
        let mut cache = Cache::new(1).unwrap();
        let report = cache
            .refresh([0, 0], |p| {
                if p == [0, 0] {
                    column(p)
                } else if p[0] < 0 {
                    Ok(None)
                } else {
                    Err("Unsupported material".into())
                }
            })
            .unwrap();
        assert_eq!(
            (report.ready, report.missing, report.rejected.len()),
            (1, 3, 5)
        );
        assert!(!cache.ready([-1, 0, -1]));
        assert!(cache.edit([-1, 0, -1], Some(Cube::Stone)).is_err());
        assert!(cache.ready([1, 319, 1]));
        assert!(!cache.cells().contains_key(&[1, 319, 1]));
        assert_eq!(cache.biome([1, -64, 1]).unwrap(), 1);
    }
    #[test]
    fn bounds_index_and_overflow_are_checked() {
        let bounds = Bounds::around([-1, -2], 1).unwrap();
        assert_eq!(bounds.min, [-32, -64, -48]);
        assert_eq!(bounds.max, [16, 320, 0]);
        assert_eq!(bounds.volume().unwrap(), 48 * 384 * 48);
        assert_eq!(bounds.index(bounds.min), Some(0));
        assert_eq!(
            bounds.index([15, 319, -1]),
            Some(bounds.volume().unwrap() - 1)
        );
        assert_eq!(bounds.index([-33, 0, -1]), None);
        assert!(Bounds::around([i32::MAX, 0], 1).is_err());
        assert!(Cache::new(4).is_err());
        let mut cache = Cache::new(0).unwrap();
        cache.refresh([0, 0], column).unwrap();
        assert!(cache.refresh([i32::MIN, 0], column).is_err());
        assert_eq!(cache.center(), Some([0, 0]));
    }
}
