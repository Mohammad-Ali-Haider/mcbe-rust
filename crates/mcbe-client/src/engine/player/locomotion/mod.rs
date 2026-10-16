//! Environment queries and a shared extension interface for movement controllers.
pub(crate) mod parkour;
pub(crate) mod swimming;
use crate::engine::fluids;
use crate::scene::{Client, Input, Result};
#[derive(Clone, Copy, Default, Debug)]
pub struct Environment {
    pub immersion: f32,
    pub head_underwater: bool,
    pub current: [f32; 3],
}
pub fn submerged(client: &Client, point: [f32; 3]) -> bool {
    let Some(s) = &client.scene.engine else {
        return false;
    };
    let p = point.map(|v| v.floor() as i32);
    let a = fluids::amount(s, p);
    a > 0 && point[1] < p[1] as f32 + a as f32 / fluids::FULL as f32
}
impl Environment {
    pub fn sample(client: &Client) -> Self {
        let Some(s) = &client.scene.engine else {
            return Self::default();
        };
        let p = client.actor.position;
        let body = client.actor.body;
        let half = body.width * 0.5;
        let lo = [p.x - half, p.y, p.z - half];
        let hi = [p.x + half, p.y + body.height, p.z + half];
        let mut volume = 0.;
        let mut current = [0.; 3];
        for x in lo[0].floor() as i32..hi[0].ceil() as i32 {
            for y in lo[1].floor() as i32..hi[1].ceil() as i32 {
                for z in lo[2].floor() as i32..hi[2].ceil() as i32 {
                    let a = fluids::amount(s, [x, y, z]);
                    if a == 0 {
                        continue;
                    }
                    let bottom = [x as f32, y as f32, z as f32];
                    let top = [
                        x as f32 + 1.,
                        y as f32 + a as f32 / fluids::FULL as f32,
                        z as f32 + 1.,
                    ];
                    let overlap = (0..3)
                        .map(|i| (hi[i].min(top[i]) - lo[i].max(bottom[i])).max(0.))
                        .product::<f32>();
                    volume += overlap;
                    let flow = s.fluids.current([x, y, z]);
                    for i in 0..3 {
                        current[i] += flow[i] * overlap;
                    }
                }
            }
        }
        Self {
            immersion: (volume / (body.width * body.width * body.height)).clamp(0., 1.),
            current: current.map(|v| if volume > 0. { v / volume } else { 0. }),
            head_underwater: submerged(client, client.eye().map(|v| v as f32)),
        }
    }
}
/// Implement another controller and include it in the ordered dispatch table.
pub trait Controller: Sync {
    fn eligible(&self, environment: &Environment) -> bool;
    fn step(&self, client: &mut Client, input: Input, environment: &Environment) -> Result<bool>;
}
static CONTROLLERS: [&dyn Controller; 2] = [&swimming::Swimming, &parkour::Parkour];
pub fn apply(client: &mut Client, input: Input, environment: &Environment) -> Result<bool> {
    for c in CONTROLLERS {
        if c.eligible(environment) && c.step(client, input, environment)? {
            return Ok(true);
        }
    }
    Ok(false)
}
