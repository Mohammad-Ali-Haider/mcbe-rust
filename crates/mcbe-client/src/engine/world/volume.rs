//! Camera-height and terrain-surface chunk windows with bounded generation payloads.
use super::{
    storage::{self, AIR, CELLS, Chunk, Key},
    stream::{DETAIL_RADIUS, Generator, Position},
};
use crate::scene::Scene;
use mcbe_core::motion::Vec3;
use std::{collections::BTreeSet, sync::Arc};
#[derive(Clone, Default)]
pub struct Volume {
    pub center: Key,
    pub wanted: BTreeSet<Key>,
    pub residents: BTreeSet<Key>,
}
impl Volume {
    pub fn wanted(generator: &Generator, position: Vec3) -> Self {
        let center = storage::key([
            position.x.floor() as i32,
            position.y.floor() as i32,
            position.z.floor() as i32,
        ]);
        let radius = generator.config.radius.min(DETAIL_RADIUS);
        let mut wanted = BTreeSet::new();
        for x in center[0] - radius..=center[0] + radius {
            for z in center[2] - radius..=center[2] + radius {
                if !generator.bounds().contains([x * 16, 0, z * 16]) {
                    continue;
                }
                if let Some(ranges) = generator.terrain.as_ref().and_then(|t| t.residency()) {
                    for (low, high) in ranges {
                        if high > low {
                            for y in low.div_euclid(16)..=(high - 1).div_euclid(16) {
                                wanted.insert([x, y, z]);
                            }
                        }
                    }
                } else {
                    let heights = [[0, 0], [15, 0], [0, 15], [15, 15], [8, 8]]
                        .map(|p| generator.visible_surface(x * 16 + p[0], z * 16 + p[1]).0);
                    let ground = [[0, 0], [15, 0], [0, 15], [15, 15], [8, 8]]
                        .map(|p| generator.ground_height(x * 16 + p[0], z * 16 + p[1]));
                    let low = (ground.iter().min().unwrap() - 64).div_euclid(16);
                    let high = (heights.iter().max().unwrap() + 16).div_euclid(16);
                    for y in low..=high {
                        wanted.insert([x, y, z]);
                    }
                }
                for y in center[1] - 2..=center[1] + 2 {
                    if let Some(world_y) = y.checked_mul(16)
                        && generator.bounds().contains([x * 16, world_y, z * 16])
                    {
                        wanted.insert([x, y, z]);
                    }
                }
            }
        }
        Self {
            center,
            wanted,
            residents: BTreeSet::new(),
        }
    }
}
fn empty_chunk() -> Arc<Chunk> {
    static EMPTY: std::sync::OnceLock<Arc<Chunk>> = std::sync::OnceLock::new();
    EMPTY.get_or_init(|| Arc::new(Chunk::default())).clone()
}
pub fn generate(generator: &Generator, k: Key) -> Arc<Chunk> {
    let t = generator.terrain.as_ref().expect("Modern terrain");
    let samples: Vec<_> = (0..16)
        .flat_map(|x| (0..16).map(move |z| t.sample(k[0] * 16 + x, k[2] * 16 + z)))
        .collect();
    let top = samples
        .iter()
        .filter(|s| s.column.present)
        .map(|s| {
            s.column
                .roof_top
                .unwrap_or(s.water.unwrap_or(s.height).max(s.height) + 6)
        })
        .max()
        .unwrap_or(i32::MIN + 32);
    if k[1] * 16 > top
        || samples
            .iter()
            .all(|s| !s.column.present || s.column.bottom.is_some_and(|b| k[1] * 16 + 15 < b))
    {
        return empty_chunk();
    }
    let tree = t.tree([k[0], k[2]]);
    let mut chunk = Chunk::default();
    for i in 0..CELLS {
        let p = storage::position(k, i);
        if let Some(id) = t.block(p, samples[(i / 256) * 16 + i % 16], tree) {
            chunk.ids[i] = id;
            chunk.count += 1;
        }
    }
    Arc::new(chunk)
}
fn dirty(scene: &mut Scene, key: Key) {
    let state = scene.engine.as_mut().unwrap();
    state.revision = state.revision.wrapping_add(1);
    for d in [
        [0, 0, 0],
        [1, 0, 0],
        [-1, 0, 0],
        [0, 1, 0],
        [0, -1, 0],
        [0, 0, 1],
        [0, 0, -1],
    ] {
        let k = std::array::from_fn(|a| key[a] + d[a]);
        if state.stream.as_ref().unwrap().ready_chunk(k) {
            state.dirty_chunks.insert(k, state.revision);
        }
    }
}
pub fn insert(scene: &mut Scene, key: Key, mut chunk: Arc<Chunk>, ms: f64) {
    let state = scene.engine.as_mut().unwrap();
    let s = state.stream.as_mut().unwrap();
    let v = s.volume.as_mut().unwrap();
    if !v.wanted.contains(&key) || v.residents.contains(&key) {
        s.discarded += 1;
        return;
    }
    if let Some(edits) = s.edit_chunks.get(&key) {
        let c = Arc::make_mut(&mut chunk);
        for (p, id) in edits {
            let i = storage::index(*p);
            let old = c.ids[i];
            let next = id.unwrap_or(AIR);
            if old == AIR && next != AIR {
                c.count += 1;
            }
            if old != AIR && next == AIR {
                c.count -= 1;
            }
            c.ids[i] = next;
        }
    }
    state.simulation.hydrate(
        &state.registry,
        chunk
            .ids
            .iter()
            .enumerate()
            .filter(|(_, id)| **id != AIR)
            .map(|(i, id)| (storage::position(key, i), *id)),
    );
    state.blocks.insert_chunk(key, chunk);
    v.residents.insert(key);
    s.residents.entry([key[0], key[2]]).or_default();
    s.generated += 1;
    s.generation_ms += ms;
    crate::engine::fluids::hydrate_chunk(scene.engine.as_mut().unwrap(), key);
    dirty(scene, key);
}
pub fn recenter(scene: &mut Scene, position: Vec3, force: bool) {
    let state = scene.engine.as_mut().unwrap();
    let s = state.stream.as_mut().unwrap();
    let key = storage::key([
        position.x.floor() as i32,
        position.y.floor() as i32,
        position.z.floor() as i32,
    ]);
    let old = s.volume.as_ref().unwrap();
    if !force && old.center == key && !old.wanted.is_empty() {
        return;
    }
    let mut next = Volume::wanted(&s.generator, position);
    next.residents = old.residents.intersection(&next.wanted).copied().collect();
    let removed = old
        .residents
        .difference(&next.wanted)
        .copied()
        .collect::<Vec<_>>();
    for k in &removed {
        state.blocks.remove_chunk(*k);
        state.fluids.evict(*k);
        state.simulation.evict_chunk(&state.registry, *k);
        s.evicted += 1;
    }
    s.center = [key[0], key[2]];
    s.wanted = next.wanted.iter().map(|k| [k[0], k[2]]).collect();
    let columns: BTreeSet<_> = next.residents.iter().map(|k| [k[0], k[2]]).collect();
    s.residents.retain(|c, _| columns.contains(c));
    s.volume = Some(next);
    state.dirty_chunks.retain(|k, _| s.ready_chunk(*k));
    for k in removed {
        dirty(scene, k);
    }
}
pub fn remesh_block(state: &crate::engine::WorldState, p: Position) -> Option<u16> {
    if state
        .stream
        .as_ref()
        .is_some_and(|s| s.volume.is_some() && !s.ready(p))
    {
        state.stream.as_ref().unwrap().observed(p)
    } else {
        state.blocks.get(&p).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        engine_blocks::Registry,
        engine_stream::{Config, Generator},
    };
    fn generator() -> Generator {
        let r = Registry::built_in();
        Generator::new(
            360036,
            &r,
            Config {
                version: 2,
                terrain: Some(super::super::terrain::Settings::built_in()),
                radius: 3,
                exhibits: Vec::new(),
            },
        )
        .unwrap()
    }
    #[test]
    fn generated_chunks_match_density_samples_at_negative_and_high_coordinates() {
        let g = generator();
        for key in [[-2, -2, 2], [0, 4, 0], [0, 625, 0]] {
            let c = generate(&g, key);
            let mut count = 0;
            for i in 0..CELLS {
                let p = storage::position(key, i);
                let expected = g.block(p).unwrap_or(AIR);
                assert_eq!(c.ids[i], expected, "{p:?}");
                count += usize::from(expected != AIR);
            }
            assert_eq!(c.count, count);
        }
    }
    #[test]
    fn three_dimensional_targets_remain_bounded_at_arbitrary_height() {
        let g = generator();
        for y in [-10000., 0., 10000., 1000000.] {
            let v = Volume::wanted(&g, Vec3::new(0.5, y, 0.5));
            assert!(v.wanted.contains(&storage::key([0, y as i32, 0])));
            assert!(v.wanted.len() <= 49 * 14);
            assert!(v.wanted.iter().any(|k| k[1] == (y as i32).div_euclid(16)));
        }
    }
    #[test]
    fn surface_window_covers_narrow_ravines_and_underwater_floors_between_probes() {
        let g = generator();
        for center in [[0, 0], [-32, 16], [32, -16]] {
            let v = Volume::wanted(
                &g,
                Vec3::new((center[0] * 16) as f32, 10000., (center[1] * 16) as f32),
            );
            for column in v
                .wanted
                .iter()
                .map(|k| [k[0], k[2]])
                .collect::<BTreeSet<_>>()
            {
                for x in 0..16 {
                    for z in 0..16 {
                        let (x, z) = (column[0] * 16 + x, column[1] * 16 + z);
                        let y = g.ground_height(x, z) - 1;
                        assert!(
                            v.wanted.contains(&storage::key([x, y, z])),
                            "Unloaded surface at {:?}",
                            [x, y, z]
                        );
                    }
                }
            }
        }
    }
    #[test]
    fn async_vertical_crossings_bound_queue_and_restore_edits() {
        let mut settings = super::super::terrain::Settings::built_in();
        settings.caves = false;
        let mut c =
            crate::engine_stream::generate_modern(360036, Registry::built_in(), settings).unwrap();
        crate::engine_player::mode(&mut c, crate::engine_player::Mode::Noclip).unwrap();
        c.set_spawn(Vec3::new(0.5, 10002., 0.5)).unwrap();
        c.scene.change_engine([0, 10000, 0], Some(0)).unwrap();
        for y in [10066., 10002.] {
            c.actor.position.y = y;
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
            loop {
                c.sync_region().unwrap();
                let state = c.scene.engine.as_ref().unwrap();
                let v = state.stream.as_ref().unwrap().volume.as_ref().unwrap();
                assert!(c.engine_worker.as_ref().unwrap().chunks_pending.len() <= 2);
                assert!(v.residents.len() <= v.wanted.len());
                assert!(state.dirty_chunks.len() <= v.wanted.len());
                if v.residents == v.wanted
                    && c.engine_worker.as_ref().unwrap().chunks_pending.is_empty()
                {
                    break;
                }
                assert!(std::time::Instant::now() < deadline);
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            assert_eq!(
                c.scene.engine.as_ref().unwrap().blocks.get(&[0, 10000, 0]),
                if y < 10010. { Some(&0) } else { None }
            );
        }
    }
    #[test]
    fn vertical_eviction_preserves_edits_without_duplicating_cells() {
        let mut settings = super::super::terrain::Settings::built_in();
        settings.caves = false;
        let mut c =
            crate::engine_stream::generate_modern(360036, Registry::built_in(), settings).unwrap();
        let spawn = c.actor.position;
        crate::engine_player::mode(&mut c, crate::engine_player::Mode::Noclip).unwrap();
        c.set_spawn(Vec3::new(0.5, 10002., 0.5)).unwrap();
        let id = c
            .scene
            .engine
            .as_ref()
            .unwrap()
            .registry
            .find("clay")
            .unwrap();
        c.scene.change_engine([0, 10000, 0], Some(id)).unwrap();
        assert!(c.scene.cells.is_empty());
        let before = c.scene.engine.as_ref().unwrap().blocks.allocated_bytes();
        assert!(before < 16 * 1024 * 1024);
        c.set_spawn(spawn).unwrap();
        assert!(
            !c.scene
                .engine
                .as_ref()
                .unwrap()
                .blocks
                .contains_key(&[0, 10000, 0])
        );
        assert_eq!(
            c.scene
                .engine
                .as_ref()
                .unwrap()
                .stream
                .as_ref()
                .unwrap()
                .observed([0, 10000, 0]),
            Some(id)
        );
        c.set_spawn(Vec3::new(0.5, 10002., 0.5)).unwrap();
        assert_eq!(
            c.scene.engine.as_ref().unwrap().blocks.get(&[0, 10000, 0]),
            Some(&id)
        );
    }
}
