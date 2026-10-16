//! GPU-published detail coverage with bounded fades and a compact lookup table.
use std::collections::{BTreeMap, BTreeSet};
pub type Key = [i32; 3];
pub const SLOTS: usize = 65536;
pub const PROBES: usize = 64;
pub const FADE_SECONDS: f64 = 0.24;
#[derive(Clone, Copy)]
struct Entry {
    value: f32,
    target: bool,
}
#[derive(Default)]
pub struct Coverage {
    entries: BTreeMap<Key, Entry>,
    last: Option<f64>,
    dirty: bool,
}
pub fn hash(k: Key) -> usize {
    ((k[0] as u32).wrapping_mul(73856093)
        ^ (k[1] as u32).wrapping_mul(19349663)
        ^ (k[2] as u32).wrapping_mul(83492791)) as usize
        & (SLOTS - 1)
}
impl Coverage {
    pub fn clear(&mut self) {
        self.entries.clear();
        self.last = None;
        self.dirty = true;
    }
    pub fn live(&self, key: Key) -> bool {
        self.entries
            .get(&key)
            .is_some_and(|e| e.value > 0. || e.target)
    }
    pub fn update(&mut self, now: f64, ready: &BTreeSet<Key>) -> bool {
        let step = self.last.map_or(0., |last| {
            ((now - last).max(0.) / FADE_SECONDS).min(1.) as f32
        });
        self.last = Some(now);
        let mut changed = self.dirty;
        self.dirty = false;
        for e in self.entries.values_mut() {
            e.target = false;
        }
        for key in ready {
            if let Some(e) = self.entries.get_mut(key) {
                e.target = true;
            } else {
                self.entries.insert(
                    *key,
                    Entry {
                        value: 0.,
                        target: true,
                    },
                );
                changed = true;
            }
        }
        self.entries.retain(|_, e| {
            let old = e.value;
            e.value = if e.target {
                (e.value + step).min(1.)
            } else {
                (e.value - step).max(0.)
            };
            changed |= old != e.value;
            let keep = e.target || e.value > 0.;
            changed |= !keep;
            keep
        });
        changed
    }
    pub fn table(&self) -> (Vec<[u32; 4]>, usize) {
        let mut out = vec![[u32::MAX; 4]; SLOTS];
        let mut dropped = 0;
        for (k, e) in &self.entries {
            let h = hash(*k);
            let mut added = false;
            for probe in 0..PROBES {
                let slot = &mut out[(h + probe) & (SLOTS - 1)];
                if slot[3] == u32::MAX {
                    *slot = [k[0] as u32, k[1] as u32, k[2] as u32, e.value.to_bits()];
                    added = true;
                    break;
                }
            }
            if !added {
                dropped += 1;
            }
        }
        (out, dropped)
    }
}
#[cfg(test)]
pub fn lookup(table: &[[u32; 4]], key: Key) -> Option<f32> {
    let h = hash(key);
    for probe in 0..PROBES {
        let s = table[(h + probe) & (SLOTS - 1)];
        if s[3] == u32::MAX {
            return None;
        }
        if s[..3] == key.map(|v| v as u32) {
            return Some(f32::from_bits(s[3]));
        }
    }
    None
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn publication_and_eviction_fade_complementary_coverage_and_release_resources() {
        let k = [-2, 12, 3];
        let mut c = Coverage::default();
        let mut ready = BTreeSet::from([k]);
        assert!(c.update(0., &ready));
        assert_eq!(lookup(&c.table().0, k), Some(0.));
        c.update(0.12, &ready);
        assert!((lookup(&c.table().0, k).unwrap() - 0.5).abs() < 0.001);
        assert!(c.live(k));
        c.update(0.24, &ready);
        assert_eq!(lookup(&c.table().0, k), Some(1.));
        assert!(!c.update(0.3, &ready));
        ready.clear();
        c.update(0.42, &ready);
        assert!(c.live(k));
        assert!((lookup(&c.table().0, k).unwrap() - 0.5).abs() < 0.001);
        c.update(0.6, &ready);
        assert!(!c.live(k));
        assert_eq!(lookup(&c.table().0, k), None);
    }
    #[test]
    fn negative_and_high_chunks_share_bounded_gpu_lookup_without_aliasing() {
        let ready = (0..12000)
            .map(|i| [i % 17 - 8, i * 127 - 700000, i / 17 - 350])
            .collect();
        let mut c = Coverage::default();
        c.update(0., &ready);
        c.update(1., &ready);
        let (table, dropped) = c.table();
        assert_eq!(dropped, 0);
        assert_eq!(table.len() * 16, 1024 * 1024);
        for k in ready {
            assert_eq!(lookup(&table, k), Some(1.));
        }
        assert_eq!(lookup(&table, [0, -10000000, 0]), None);
    }
}
