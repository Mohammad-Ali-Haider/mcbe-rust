//! Local stair corners are a reusable neighbor behavior, independent of material.
use super::{BlockBehavior, BlockEvent, Command, EventContext, Position};
use crate::{
    engine::world::storage::Store,
    engine_blocks::{BlockId, Registry},
    scene::Result,
};
const DIRECTIONS: [[i32; 3]; 4] = [[0, 0, -1], [1, 0, 0], [0, 0, 1], [-1, 0, 0]];
const NAMES: [&str; 4] = ["north", "east", "south", "west"];
const SHAPES: [&str; 5] = [
    "straight",
    "inner_left",
    "inner_right",
    "outer_left",
    "outer_right",
];
pub struct Stairs;
fn info(r: &Registry, id: BlockId) -> Option<(usize, &str)> {
    let b = r.get(id);
    if !matches!(
        b.placement,
        Some(crate::engine_blocks::Placement::Stairs { .. })
    ) {
        return None;
    }
    let p = &b.state.as_ref()?.properties;
    // Authored facing is the front/down-step side, preserving existing models.
    Some((
        (NAMES
            .iter()
            .position(|n| p.get("facing").is_some_and(|v| v == n))?
            + 2)
            % 4,
        p.get("half")?.as_str(),
    ))
}
fn offset(p: Position, dir: usize, sign: i32) -> Position {
    std::array::from_fn(|a| p[a] + DIRECTIONS[dir][a] * sign)
}
fn different(r: &Registry, b: &Store, p: Position, facing: usize, half: &str) -> bool {
    b.get(&p)
        .and_then(|id| info(r, *id))
        .is_none_or(|(f, h)| f != facing || h != half)
}
pub fn shape(r: &Registry, b: &Store, p: Position, id: BlockId) -> &'static str {
    let Some((f, half)) = info(r, id) else {
        return "straight";
    };
    let left = (f + 3) % 4;
    for (sign, inside) in [(1, false), (-1, true)] {
        if let Some((g, h)) = b.get(&offset(p, f, sign)).and_then(|id| info(r, *id))
            && h == half
            && g % 2 != f % 2
            && different(r, b, offset(p, g, if inside { 1 } else { -1 }), f, half)
        {
            return match (inside, g == left) {
                (true, true) => "inner_left",
                (true, false) => "inner_right",
                (false, true) => "outer_left",
                (false, false) => "outer_right",
            };
        }
    }
    "straight"
}
fn variant(r: &Registry, id: BlockId, shape: &str) -> Result<BlockId> {
    let s = r.get(id).state.as_ref().ok_or("Stair identity missing")?;
    let mut p = s.properties.clone();
    p.insert("shape".into(), shape.into());
    r.resolve_state(&s.block, &p).or_else(|e| {
        if shape == "straight" {
            p.remove("shape");
            r.resolve_state(&s.block, &p)
        } else {
            Err(e)
        }
    })
}
impl BlockBehavior for Stairs {
    fn validate(&self, r: &Registry, id: BlockId) -> Result<()> {
        let Some((_, half)) = info(r, id) else {
            return Err("Stair events require cardinal stair placement and half".into());
        };
        if !["lower", "upper"].contains(&half) {
            return Err("Invalid stair half".into());
        }
        for shape in SHAPES {
            let other = variant(r, id, shape)?;
            if r.get(other).behavior != r.get(id).behavior {
                return Err("Stair shape states must share neighbor behavior".into());
            }
        }
        Ok(())
    }
    fn on_event(
        &self,
        c: &EventContext<'_>,
        _event: BlockEvent,
        out: &mut Vec<Command>,
    ) -> Result<()> {
        let state = variant(
            c.registry,
            c.state,
            shape(c.registry, c.blocks, c.position, c.state),
        )?;
        if state != c.state {
            out.push(Command::Set {
                position: c.position,
                expected: c.state,
                state,
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn state(r: &Registry, f: usize, h: &str, shape: &str) -> BlockId {
        r.resolve_state(
            "oak_stairs",
            &std::collections::BTreeMap::from([
                ("facing".into(), NAMES[f].into()),
                ("half".into(), h.into()),
                ("shape".into(), shape.into()),
            ]),
        )
        .unwrap()
    }
    #[test]
    fn all_rotated_halves_choose_corners_and_guard_parallel_continuations() {
        let r = Registry::built_in();
        let p = [0, 20, 0];
        for h in ["lower", "upper"] {
            for f in 0..4 {
                let id = state(&r, f, h, "straight");
                let actual = (f + 2) % 4;
                for (relative, left) in [(3, true), (1, false)] {
                    let g = (f + relative) % 4;
                    for (sign, inside) in [(1, false), (-1, true)] {
                        let q = offset(p, actual, sign);
                        let other = state(&r, g, h, "straight");
                        let mut b = Store::from([(p, id), (q, other)]);
                        let expected = match (inside, left) {
                            (true, true) => "inner_left",
                            (true, false) => "inner_right",
                            (false, true) => "outer_left",
                            (false, false) => "outer_right",
                        };
                        assert_eq!(shape(&r, &b, p, id), expected);
                        b.insert(offset(p, (g + 2) % 4, if inside { 1 } else { -1 }), id);
                        assert_eq!(shape(&r, &b, p, id), "straight");
                        b.remove(&q);
                        assert_eq!(shape(&r, &b, p, id), "straight");
                        b.insert(
                            q,
                            state(
                                &r,
                                g,
                                if h == "lower" { "upper" } else { "lower" },
                                "straight",
                            ),
                        );
                        assert_eq!(shape(&r, &b, p, id), "straight");
                    }
                }
            }
        }
    }
    #[test]
    fn corner_models_have_the_expected_union_volume_for_every_facing_and_half() {
        let r = Registry::built_in();
        for f in 0..4 {
            for h in ["lower", "upper"] {
                for (shape, volume) in [
                    ("straight", 48),
                    ("inner_left", 56),
                    ("inner_right", 56),
                    ("outer_left", 40),
                    ("outer_right", 40),
                ] {
                    let id = state(&r, f, h, shape);
                    let boxes = &r.get(id).geometry.as_ref().unwrap().boxes;
                    let mut n = 0;
                    for x in 0..4 {
                        for y in 0..4 {
                            for z in 0..4 {
                                let point = [x, y, z].map(|v| (v as f32 + 0.5) / 4.);
                                n += usize::from(boxes.iter().any(|b| {
                                    (0..3).all(|a| point[a] >= b[a] && point[a] < b[a + 3])
                                }));
                            }
                        }
                    }
                    assert_eq!(n, volume, "{f} {h} {shape}");
                }
            }
        }
    }
    #[test]
    fn scheduler_updates_neighbors_and_retries_expansion_until_player_moves_clear() {
        let mut c = crate::engine::generate(37).unwrap();
        let r = c.scene.engine.as_ref().unwrap().registry.clone();
        let p = [0, 20, 0];
        let outer = state(&r, 0, "lower", "outer_left");
        c.set_spawn(mcbe_core::motion::Vec3::new(-0.1, 20.5, 0.75))
            .unwrap();
        c.scene.change_engine(p, Some(outer)).unwrap();
        for _ in 0..5 {
            crate::engine::blocks::systems::simulation::tick(&mut c).unwrap();
        }
        assert_eq!(c.scene.engine.as_ref().unwrap().blocks[&p], outer);
        c.set_spawn(mcbe_core::motion::Vec3::new(5., 25., 5.))
            .unwrap();
        for _ in 0..5 {
            crate::engine::blocks::systems::simulation::tick(&mut c).unwrap();
        }
        assert_eq!(
            c.scene.engine.as_ref().unwrap().blocks[&p],
            state(&r, 0, "lower", "straight")
        );
        let neighbor = [0, 20, 1];
        c.scene
            .change_engine(neighbor, Some(state(&r, 3, "lower", "straight")))
            .unwrap();
        for _ in 0..5 {
            crate::engine::blocks::systems::simulation::tick(&mut c).unwrap();
        }
        assert_eq!(
            c.scene
                .engine
                .as_ref()
                .unwrap()
                .registry
                .get(c.scene.engine.as_ref().unwrap().blocks[&p])
                .state
                .as_ref()
                .unwrap()
                .properties["shape"],
            "outer_left"
        );
        c.scene.change_engine(neighbor, None).unwrap();
        for _ in 0..5 {
            crate::engine::blocks::systems::simulation::tick(&mut c).unwrap();
        }
        assert_eq!(
            c.scene.engine.as_ref().unwrap().blocks[&p],
            state(&r, 0, "lower", "straight")
        );
    }
}
