//! Gravity, lateral flux and hydrostatic column pressure; every transfer conserves mass.
use super::*;
use crate::scene::{Client, Scene};
const SIDES: [[i32; 3]; 4] = [[1, 0, 0], [0, 0, 1], [-1, 0, 0], [0, 0, -1]];
const PRESSURE_DEPTH: usize = 64;
fn compatible(scene: &Scene, p: Position, id: BlockId) -> bool {
    if !editable(scene, p) {
        return false;
    }
    let s = scene.engine.as_ref().unwrap();
    let b = s.blocks.get(&p).copied();
    b.is_none() || kind(&s.registry, b) == kind(&s.registry, Some(id))
}
/// Free surface endpoint and hydraulic head. An unloaded column cannot conduct flow.
fn column_head(scene: &Scene, mut p: Position, id: BlockId) -> Option<(Position, i64)> {
    for _ in 0..PRESSURE_DEPTH {
        if !compatible(scene, p, id) {
            return None;
        }
        let a = amount(scene.engine.as_ref().unwrap(), p);
        if a < FULL {
            return Some((p, i64::from(p[1]) * i64::from(FULL) + i64::from(a)));
        }
        let up = [p[0], p[1] + 1, p[2]];
        if !scene.inside(up) {
            return None;
        }
        if scene.engine.as_ref().unwrap().blocks.get(&up).is_some() && !compatible(scene, up, id) {
            return None;
        }
        p = up;
    }
    None
}
fn head(scene: &Scene, p: Position, id: BlockId) -> Option<(Position, i64)> {
    if let Some(h) = column_head(scene, p, id) {
        return Some(h);
    }
    // Covered passages transmit pressure to an accessible free surface.
    let mut queue = std::collections::VecDeque::from([p]);
    let mut visited = std::collections::BTreeSet::from([p]);
    for _ in 0..128 {
        let q = queue.pop_front()?;
        if let Some(h) = column_head(scene, q, id) {
            return Some(h);
        }
        for d in [
            [0, 1, 0],
            [0, -1, 0],
            [1, 0, 0],
            [-1, 0, 0],
            [0, 0, 1],
            [0, 0, -1],
        ] {
            let n = std::array::from_fn(|i| q[i] + d[i]);
            if compatible(scene, n, id)
                && amount(scene.engine.as_ref().unwrap(), n) > 0
                && visited.insert(n)
            {
                queue.push_back(n);
            }
        }
    }
    None
}
fn transfer(
    scene: &mut Scene,
    from: Position,
    to: Position,
    id: BlockId,
    limit: u16,
    directions: [[f32; 3]; 2],
) -> Result<u16> {
    if from == to || !compatible(scene, from, id) || !compatible(scene, to, id) {
        return Ok(0);
    }
    let s = scene.engine.as_ref().unwrap();
    let a = amount(s, from);
    let b = amount(s, to);
    let n = limit.min(a).min(FULL - b);
    if n == 0 {
        return Ok(0);
    }
    if [from, to].iter().any(|p| {
        s.machines
            .furnaces
            .get(p)
            .is_some_and(|f| f.slots.iter().any(Option::is_some))
    }) {
        return Ok(0);
    }
    // Validate the whole two-cell transaction before either write.
    let values = [(from, a - n), (to, b + n)];
    let mut overrides = s.fluids.amounts.len() as isize;
    for (p, v) in values {
        overrides +=
            isize::from(v > 0 && v < FULL) - isize::from(s.fluids.amounts.contains_key(&p));
    }
    if overrides > MAX_AMOUNTS as isize {
        return Ok(0);
    }
    if let Some(stream) = &s.stream {
        let mut edits = stream.edits.len() as isize;
        for (p, v) in values {
            edits += isize::from(stream.generator.block(p) != (v > 0).then_some(id))
                - isize::from(stream.edits.contains_key(&p));
        }
        if edits > crate::engine_stream::MAX_EDITS as isize {
            return Ok(0);
        }
    }
    let fits = |values: &[(Position, u16); 2]| {
        let mut amounts = s.fluids.amounts.len() as isize;
        let mut edits = s.stream.as_ref().map_or(0, |v| v.edits.len()) as isize;
        for (p, v) in values {
            amounts +=
                isize::from(*v > 0 && *v < FULL) - isize::from(s.fluids.amounts.contains_key(p));
            if let Some(stream) = &s.stream {
                edits += isize::from(stream.generator.block(*p) != (*v > 0).then_some(id))
                    - isize::from(stream.edits.contains_key(p));
            }
            if amounts > MAX_AMOUNTS as isize || edits > crate::engine_stream::MAX_EDITS as isize {
                return false;
            }
        }
        true
    };
    let mut values = values;
    if !fits(&values) {
        values.reverse();
        if !fits(&values) {
            return Ok(0);
        }
    }
    for (p, v) in values {
        scene.set_fluid(p, id, v)?;
    }
    let f = &mut scene.engine.as_mut().unwrap().fluids;
    f.flux(from, directions[0], n);
    f.flux(to, directions[1], n);
    f.counters.transfers += 1;
    Ok(n)
}
fn step(scene: &mut Scene, p: Position, phase: u64) -> Result<()> {
    if !editable(scene, p) {
        return Ok(());
    }
    let s = scene.engine.as_ref().unwrap();
    let Some(id) = s
        .blocks
        .get(&p)
        .copied()
        .filter(|id| s.registry.get(*id).fluid.is_some())
    else {
        return Ok(());
    };
    let down = [p[0], p[1] - 1, p[2]];
    // Gravity fills available space before pressure is distributed sideways.
    transfer(scene, p, down, id, FULL, [[0., -1., 0.]; 2])?;
    if amount(scene.engine.as_ref().unwrap(), p) == 0 {
        return Ok(());
    }
    for i in 0..4 {
        let d = SIDES[(i + (phase as usize) % 4) % 4];
        let q = std::array::from_fn(|i| p[i] + d[i]);
        if !compatible(scene, q, id) {
            continue;
        }
        let Some((mut source, a)) = head(scene, p, id) else {
            continue;
        };
        let Some((dest, b)) = head(scene, q, id) else {
            continue;
        };
        if a <= b + 1 {
            continue;
        }
        if amount(scene.engine.as_ref().unwrap(), source) == 0 {
            source[1] -= 1;
        }
        let limit = ((a - b) / 2).min(i64::from(FULL / 4)) as u16;
        let side = d.map(|v| v as f32);
        let source_flow = if source == p {
            side
        } else {
            std::array::from_fn(|i| (p[i] - source[i]).signum() as f32)
        };
        let dest_flow = if dest == q {
            side
        } else {
            std::array::from_fn(|i| (dest[i] - q[i]).signum() as f32)
        };
        let moved = transfer(scene, source, dest, id, limit, [source_flow, dest_flow])?;
        if moved > 0 {
            let f = &mut scene.engine.as_mut().unwrap().fluids;
            if source != p {
                f.flux(p, side, moved);
            }
            if dest != q {
                f.flux(q, side, moved);
            }
            f.wake(p);
            f.wake(q);
        }
    }
    Ok(())
}
pub fn tick(client: &mut Client) -> Result<()> {
    let Some(state) = client.scene.engine.as_mut() else {
        return Ok(());
    };
    let started = std::time::Instant::now();
    state.fluids.begin_step();
    if let Some(k) = state.fluids.deferred() {
        hydrate_chunk(state, k);
        state.fluids.counters.deferred_steps += 1;
    }
    let count = state.fluids.queued().min(STEP_BUDGET);
    state.fluids.counters.last_processed = 0;
    for _ in 0..count {
        if started.elapsed() >= std::time::Duration::from_millis(1) {
            break;
        }
        let f = &mut client.scene.engine.as_mut().unwrap().fluids;
        let Some(p) = f.take() else {
            break;
        };
        let phase = f.counters.processed;
        f.counters.processed += 1;
        f.counters.last_processed += 1;
        step(&mut client.scene, p, phase)?;
    }
    let counters = &mut client.scene.engine.as_mut().unwrap().fluids.counters;
    counters.last_step_ms = started.elapsed().as_secs_f64() * 1000.;
    counters.max_step_ms = counters.max_step_ms.max(counters.last_step_ms);
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn tank() -> (Client, BlockId) {
        let mut c = crate::engine::generate(44).unwrap();
        let s = c.scene.engine.as_mut().unwrap();
        s.blocks.clear();
        c.scene.cells.clear();
        let water = s.registry.find("water").unwrap();
        let stone = s.registry.find("slate").unwrap();
        for x in -5i32..=5 {
            for y in 0..=8 {
                for z in -5i32..=5 {
                    if y == 0 || x.abs() == 5 || z.abs() == 5 {
                        if y == 0 {
                            c.scene
                                .engine
                                .as_mut()
                                .unwrap()
                                .blocks
                                .insert([x, y, z], stone);
                        } else {
                            c.scene.change_engine([x, y, z], Some(stone)).unwrap();
                        }
                    }
                }
            }
        }
        (c, water)
    }
    fn mass(c: &Client) -> u64 {
        let s = c.scene.engine.as_ref().unwrap();
        s.blocks.keys().map(|p| u64::from(amount(s, p))).sum()
    }
    #[test]
    fn falling_and_spreading_conserve_mass_and_settle_with_bounded_work() {
        let (mut c, w) = tank();
        c.scene.set_fluid([0, 7, 0], w, FULL).unwrap();
        for _ in 0..600 {
            tick(&mut c).unwrap();
            assert_eq!(mass(&c), u64::from(FULL));
            assert!(
                c.scene
                    .engine
                    .as_ref()
                    .unwrap()
                    .fluids
                    .counters
                    .last_processed
                    <= STEP_BUDGET
            );
        }
        let s = c.scene.engine.as_ref().unwrap();
        assert!(s.blocks.keys().filter(|p| amount(s, *p) > 0).count() > 8);
        assert!(
            s.blocks
                .keys()
                .filter(|p| amount(s, *p) > 0)
                .all(|p| p[1] == 1)
        );
        assert_eq!(s.fluids.queued(), 0);
    }
    #[test]
    fn connected_columns_equalize_through_a_submerged_passage() {
        let (mut c, w) = tank();
        let stone = c
            .scene
            .engine
            .as_ref()
            .unwrap()
            .registry
            .find("slate")
            .unwrap();
        // Two enclosed 1-wide shafts connected beneath a solid divider.
        for x in -4..=4 {
            for y in 1..=7 {
                for z in -4..=4 {
                    if z != 0 || !([-1, 1].contains(&x) || x == 0 && y == 1) {
                        c.scene.change_engine([x, y, z], Some(stone)).unwrap();
                    }
                }
            }
        }
        for y in 1..=5 {
            c.scene.set_fluid([-1, y, 0], w, FULL).unwrap();
        }
        c.scene.set_fluid([0, 1, 0], w, FULL).unwrap();
        c.scene.set_fluid([1, 1, 0], w, FULL).unwrap();
        let initial = mass(&c);
        tick(&mut c).unwrap();
        assert_eq!(mass(&c), initial);
        assert!(
            c.scene.engine.as_ref().unwrap().fluids.current([1, 2, 0])[1] > 0.,
            "Pressure outlet current must rise"
        );
        for _ in 0..300 {
            tick(&mut c).unwrap();
            assert_eq!(mass(&c), initial);
        }
        let left = head(&c.scene, [-1, 1, 0], w).unwrap().1;
        let right = head(&c.scene, [1, 1, 0], w).unwrap().1;
        assert!((left - right).abs() <= 1, "{left} vs {right}");
    }
}
