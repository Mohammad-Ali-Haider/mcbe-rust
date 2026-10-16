//! Bounded six-block shape ray and conservative world/container transactions.
use super::ItemData;
use crate::{
    engine::fluids,
    engine_blocks::BlockId,
    scene::{Client, Result},
};
use mcbe_core::raycast::{Face, Hit};
#[derive(Clone)]
pub struct Target {
    pub hit: Hit,
    pub fluid: bool,
}
fn intersect(
    origin: [f64; 3],
    direction: [f64; 3],
    p: [i32; 3],
    b: [f32; 6],
) -> Option<(f64, Face, [f32; 3])> {
    let faces = [
        (Face::West, Face::East),
        (Face::Down, Face::Up),
        (Face::North, Face::South),
    ];
    let mut near = f64::NEG_INFINITY;
    let mut far = f64::INFINITY;
    let mut nf = Face::West;
    let mut ff = nf;
    for i in 0..3 {
        let lo = p[i] as f64 + b[i] as f64;
        let hi = p[i] as f64 + b[i + 3] as f64;
        if direction[i].abs() < 1e-12 {
            if origin[i] < lo || origin[i] > hi {
                return None;
            }
            continue;
        }
        let mut a = (lo - origin[i]) / direction[i];
        let mut z = (hi - origin[i]) / direction[i];
        let (mut af, mut zf) = faces[i];
        if a > z {
            std::mem::swap(&mut a, &mut z);
            std::mem::swap(&mut af, &mut zf)
        }
        if a > near {
            near = a;
            nf = af
        }
        if z < far {
            far = z;
            ff = zf
        }
        if near > far {
            return None;
        }
    }
    let (t, face) = if near >= 0. { (near, nf) } else { (far, ff) };
    if !(0. ..6.).contains(&t) {
        return None;
    }
    Some((
        t,
        face,
        std::array::from_fn(|i| (origin[i] + direction[i] * t - p[i] as f64) as f32),
    ))
}
pub fn target(c: &Client) -> Result<Option<Target>> {
    let Some(s) = &c.scene.engine else {
        return Ok(None);
    };
    let origin = c.eye();
    let mut direction = c.direction();
    let length = direction.iter().map(|v| v * v).sum::<f64>().sqrt();
    if !length.is_finite() || length == 0. || origin.iter().any(|v| !v.is_finite()) {
        return Err("Invalid item ray".into());
    }
    direction = direction.map(|v| v / length);
    let mut cell = origin.map(|v| v.floor() as i32);
    let step = direction.map(|v| {
        if v > 0. {
            1
        } else if v < 0. {
            -1
        } else {
            0
        }
    });
    let mut crossing = std::array::from_fn::<_, 3, _>(|i| {
        if step[i] == 0 {
            f64::INFINITY
        } else {
            (cell[i] as f64 + if step[i] > 0 { 1. } else { 0. } - origin[i]) / direction[i]
        }
    });
    let stride = direction.map(|v| if v == 0. { f64::INFINITY } else { 1. / v.abs() });
    for _ in 0..24 {
        if !c.scene.inside(cell) {
            return Ok(None);
        }
        if let Some(id) = s.blocks.get(&cell).copied() {
            let block = s.registry.get(id);
            let amount = fluids::amount(s, cell);
            let shapes = if amount > 0 {
                vec![[0., 0., 0., 1., amount as f32 / fluids::FULL as f32, 1.]]
            } else {
                block
                    .selection
                    .clone()
                    .unwrap_or_else(|| s.geometry.boxes(id).to_vec())
            };
            let best = shapes
                .iter()
                .filter_map(|b| intersect(origin, direction, cell, *b))
                .min_by(|a, b| a.0.total_cmp(&b.0));
            if let Some((_, face, local)) = best {
                return Ok(Some(Target {
                    hit: Hit {
                        id: block.id.clone(),
                        position: cell.into(),
                        face,
                        face_location: local.into(),
                    },
                    fluid: amount > 0,
                }));
            }
        }
        let axis = (0..3)
            .min_by(|a, b| crossing[*a].total_cmp(&crossing[*b]))
            .unwrap();
        if crossing[axis] >= 6. {
            break;
        }
        cell[axis] += step[axis];
        crossing[axis] += stride[axis];
    }
    Ok(None)
}
pub fn selected(c: &Client) -> bool {
    c.scene.engine.as_ref().is_some_and(|s| {
        s.player
            .selected()
            .is_some_and(|id| s.registry.get(id).container.is_some())
    })
}
pub fn use_selected(c: &mut Client, force_pour: bool) -> Result<Option<bool>> {
    if !selected(c) {
        return Ok(None);
    }
    if c.view.is_free() {
        return Ok(Some(false));
    }
    let Some(target) = target(c)? else {
        return Ok(Some(false));
    };
    apply(c, &target, force_pour).map(Some)
}
pub fn apply(c: &mut Client, target: &Target, force_pour: bool) -> Result<bool> {
    let s = c.scene.engine.as_ref().ok_or("Item world missing")?;
    let active = s.player.active;
    let Some(mut stack) = s.player.slots[active] else {
        return Ok(false);
    };
    stack.validate_item(&s.registry)?;
    let Some(container) = s.registry.get(stack.id).container else {
        return Ok(false);
    };
    let Some(ItemData::Fluid {
        kind,
        amount: carried,
    }) = stack.data
    else {
        return Ok(false);
    };
    let mut p = [
        target.hit.position.x,
        target.hit.position.y,
        target.hit.position.z,
    ];
    if !fluids::editable(&c.scene, p) {
        return Ok(false);
    }
    let a = fluids::amount(s, p);
    let id = s.blocks.get(&p).copied();
    let collect = target.fluid && !force_pour && carried < container.capacity;
    let (water, next, held) = if collect {
        let Some(w) = id.filter(|id| s.registry.get(*id).fluid == Some(kind)) else {
            return Ok(false);
        };
        let n = a.min(container.capacity - carried);
        if n == 0 {
            return Ok(false);
        };
        (w, a - n, carried + n)
    } else {
        if carried == 0 {
            return Ok(false);
        }
        if !target.fluid || a == fluids::FULL {
            p = crate::engine::blocks::behavior::placement::neighbor(&target.hit)
        }
        if !fluids::editable(&c.scene, p) {
            return Ok(false);
        }
        let id = s.blocks.get(&p).copied();
        if id.is_some_and(|id| s.registry.get(id).fluid != Some(kind)) {
            return Ok(false);
        }
        let a = fluids::amount(s, p);
        let n = carried.min(fluids::FULL - a);
        if n == 0 {
            return Ok(false);
        };
        let w = id
            .or_else(|| {
                s.registry
                    .blocks
                    .iter()
                    .position(|b| b.fluid == Some(kind))
                    .map(|i| i as BlockId)
            })
            .ok_or("Container fluid has no registered material")?;
        (w, a + n, carried - n)
    };
    // One fallible world write precedes the infallible update to this same owned slot.
    c.scene.set_fluid(p, water, next)?;
    stack.data = Some(ItemData::Fluid { kind, amount: held });
    let s = c.scene.engine.as_mut().unwrap();
    s.player.slots[active] = Some(stack);
    s.player.revision = s.player.revision.wrapping_add(1);
    c.changed += 1;
    c.view.swing = 1.;
    Ok(true)
}
