//! Bounded reusable vertex ranges; no per-frame full-scene allocation.
#[derive(Clone)]
pub struct Arena {
    free: Vec<(u32, u32)>,
    capacity: u32,
}
impl Arena {
    pub fn new(capacity: u32) -> Self {
        Self {
            free: vec![(0, capacity)],
            capacity,
        }
    }
    pub fn capacity(&self) -> u32 {
        self.capacity
    }
    pub fn reserve(count: u32) -> u32 {
        count.div_ceil(192) * 192
    }
    pub fn allocate(&mut self, count: u32) -> Option<u32> {
        let i = self.free.iter().position(|(_, size)| *size >= count)?;
        let (base, size) = self.free[i];
        if size == count {
            self.free.remove(i);
        } else {
            self.free[i] = (base + count, size - count);
        }
        Some(base)
    }
    pub fn release(&mut self, base: u32, count: u32) {
        assert!(base + count <= self.capacity);
        self.free.push((base, count));
        self.free.sort_unstable();
        let mut merged: Vec<(u32, u32)> = Vec::new();
        for (p, n) in self.free.drain(..) {
            if let Some((last, size)) = merged.last_mut()
                && *last + *size == p
            {
                *size += n;
            } else {
                merged.push((p, n));
            }
        }
        self.free = merged;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fragmented_ranges_coalesce_and_live_ranges_never_overlap() {
        let mut arena = Arena::new(192 * 32);
        let mut live = Vec::new();
        let mut seed = 51u32;
        for _ in 0..5000 {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            if seed.is_multiple_of(3) && !live.is_empty() {
                let (base, count) = live.swap_remove(seed as usize % live.len());
                arena.release(base, count);
            } else {
                let count = Arena::reserve(3 * (1 + (seed >> 8) % 120));
                if let Some(base) = arena.allocate(count) {
                    assert!(base + count <= arena.capacity());
                    for (a, n) in &live {
                        assert!(base + count <= *a || *a + *n <= base);
                    }
                    live.push((base, count));
                }
            }
        }
        for (base, count) in live {
            arena.release(base, count);
        }
        assert_eq!(arena.allocate(arena.capacity()), Some(0));
        assert_eq!(arena.allocate(192), None);
    }
}
