//! Float32 motion, measured planar effects, and stone-box collision/stepping.
use crate::Result;
use serde::{Deserialize, Serialize};
use std::ops::{Index, IndexMut};

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}
impl Vec3 {
    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }
    pub fn finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
    }
}
impl Index<usize> for Vec3 {
    type Output = f32;
    fn index(&self, a: usize) -> &f32 {
        match a {
            0 => &self.x,
            1 => &self.y,
            2 => &self.z,
            _ => panic!("Invalid axis"),
        }
    }
}
impl IndexMut<usize> for Vec3 {
    fn index_mut(&mut self, a: usize) -> &mut f32 {
        match a {
            0 => &mut self.x,
            1 => &mut self.y,
            2 => &mut self.z,
            _ => panic!("Invalid axis"),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Body {
    pub width: f32,
    pub height: f32,
}
impl Body {
    pub const PROBE: Self = Self {
        width: 0.6,
        height: 1.8,
    };
    pub const ARMOR_STAND: Self = Self {
        width: 0.5,
        height: 1.975,
    };
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Observation {
    pub location: Vec3,
    pub velocity: Vec3,
    #[serde(rename = "onGround")]
    pub on_ground: bool,
    pub falling: bool,
}
#[derive(Clone, Copy, Debug)]
pub struct Actor {
    pub position: Vec3,
    pub velocity: Vec3,
    pub displacement: Vec3,
    pub on_ground: bool,
    pub falling: bool,
    pub body: Body,
}
impl Actor {
    pub fn new(position: Vec3, body: Body) -> Result<Self> {
        if !position.finite()
            || !body.width.is_finite()
            || !body.height.is_finite()
            || body.width <= 0.0
            || body.height <= 0.0
        {
            return Err("Finite position and positive body dimensions required".into());
        }
        Ok(Self {
            position,
            body,
            velocity: Vec3::default(),
            displacement: Vec3::default(),
            on_ground: true,
            falling: false,
        })
    }
    pub fn impulse(mut self, amount: Vec3) -> Result<Self> {
        if !amount.finite() {
            return Err("Finite impulse required".into());
        }
        for a in 0..3 {
            self.velocity[a] += amount[a];
        }
        if !self.velocity.finite() {
            return Err("Impulse overflow".into());
        }
        Ok(self)
    }
    pub fn observation(self) -> Observation {
        Observation {
            location: self.position,
            velocity: self.displacement,
            on_ground: self.on_ground,
            falling: self.falling,
        }
    }
    fn moved(mut self, position: Vec3, velocity: Vec3, grounded: bool) -> Self {
        for a in 0..3 {
            self.displacement[a] = position[a] - self.position[a];
        }
        self.position = position;
        self.velocity = velocity;
        self.on_ground = grounded;
        self.falling = self.displacement.y < 0.0 && !grounded;
        self
    }
    pub fn tick_plane(self, surface: Plane) -> Result<Self> {
        if self.position.y < surface.y {
            return Err("Initial surface penetration is unmeasured".into());
        }
        let slipperiness = if self.position.y - 0.1 < surface.y {
            surface.slipperiness
        } else {
            0.6
        };
        let drag = if self.on_ground {
            0.91 * slipperiness
        } else {
            0.91
        };
        let mut position = self.position;
        for a in 0..3 {
            position[a] += self.velocity[a];
        }
        let collided = self.velocity.y < 0.0 && position.y < surface.y;
        let grounded = if self.velocity.y != 0.0 {
            collided
        } else {
            self.on_ground
        };
        let mut v = self.velocity;
        if collided {
            position.y = surface.y;
            v.y = if surface.bounce && v.y < -0.08 {
                let drop = self.position.y - surface.y;
                (v.y * v.y + 0.16 * drop).sqrt()
            } else {
                0.0
            };
        }
        v.y = (v.y - 0.08) * 0.98;
        v.x *= drag;
        v.z *= drag;
        if surface.slow && grounded && position.y - 0.2 < surface.y {
            v = slow_horizontal(v);
        }
        Ok(self.moved(position, v, grounded))
    }
    pub fn tick_solid(self, scene: &SolidScene) -> Result<Self> {
        let (position, hit) =
            scene.move_actor(self.position, self.body, self.velocity, self.on_ground)?;
        let grounded = if self.velocity.y != 0.0 || position.y - self.position.y != 0.0 {
            hit[1] && self.velocity.y < 0.0
        } else {
            self.on_ground
        };
        let drag = if self.on_ground { 0.91 * 0.6 } else { 0.91 };
        let mut v = self.velocity;
        for a in 0..3 {
            if hit[a] {
                v[a] = 0.0;
            }
        }
        v.y = (v.y - 0.08) * 0.98;
        v.x *= drag;
        v.z *= drag;
        Ok(self.moved(position, v, grounded))
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Plane {
    pub y: f32,
    pub slipperiness: f32,
    pub slow: bool,
    pub bounce: bool,
}
pub fn slow_horizontal(mut v: Vec3) -> Vec3 {
    if v.y < 0.1 {
        let factor = v.y.abs() * 0.2 + 0.4;
        v.x *= factor;
        v.z *= factor;
    }
    v
}

#[derive(Clone, Copy, Debug)]
pub struct SolidBox {
    pub min: Vec3,
    pub max: Vec3,
}
impl SolidBox {
    pub fn new(min: Vec3, max: Vec3) -> Result<Self> {
        if !min.finite() || !max.finite() || (0..3).any(|a| min[a] >= max[a]) {
            return Err("Finite positive box dimensions required".into());
        }
        Ok(Self { min, max })
    }
}
fn bounds(p: Vec3, b: Body) -> (Vec3, Vec3) {
    let half = b.width * 0.5;
    (
        Vec3::new(p.x - half, p.y, p.z - half),
        Vec3::new(p.x + half, p.y + b.height, p.z + half),
    )
}
fn horizontal_squared(v: Vec3) -> f32 {
    v.x * v.x + v.z * v.z
}
pub struct SolidScene {
    pub boxes: Vec<SolidBox>,
}
impl SolidScene {
    fn axis(&self, low: &mut Vec3, high: &mut Vec3, a: usize, mut amount: f32) -> f32 {
        for b in &self.boxes {
            if !(0..3)
                .filter(|&i| i != a)
                .all(|i| low[i] < b.max[i] && high[i] > b.min[i])
            {
                continue;
            }
            if amount > 0.0 && high[a] <= b.min[a] {
                amount = amount.min(b.min[a] - high[a]);
            } else if amount < 0.0 && low[a] >= b.max[a] {
                amount = amount.max(b.max[a] - low[a]);
            }
        }
        low[a] += amount;
        high[a] += amount;
        amount
    }
    pub fn move_actor(
        &self,
        position: Vec3,
        body: Body,
        velocity: Vec3,
        on_ground: bool,
    ) -> Result<(Vec3, [bool; 3])> {
        let (mut low, mut high) = bounds(position, body);
        if self
            .boxes
            .iter()
            .any(|b| (0..3).all(|a| low[a] < b.max[a] && high[a] > b.min[a]))
        {
            return Err("Initial box penetration is unmeasured".into());
        }
        let mut movement = velocity;
        let mut stepped_y = None;
        for a in [1, 0, 2] {
            movement[a] = self.axis(&mut low, &mut high, a, movement[a]);
        }
        if (on_ground || velocity.y < 0.0 && movement.y != velocity.y)
            && (movement.x != velocity.x || movement.z != velocity.z)
        {
            for expanded in [false, true] {
                let (mut sl, mut sh) = bounds(position, body);
                let mut up = 0.5625;
                if expanded {
                    let (mut wl, mut wh) = (sl, sh);
                    for a in [0, 2] {
                        wl[a] += velocity[a].min(0.0);
                        wh[a] += velocity[a].max(0.0);
                    }
                    up = self.axis(&mut wl, &mut wh, 1, up);
                }
                up = self.axis(&mut sl, &mut sh, 1, up);
                let sx = self.axis(&mut sl, &mut sh, 0, velocity.x);
                let sz = self.axis(&mut sl, &mut sh, 2, velocity.z);
                let down = self.axis(&mut sl, &mut sh, 1, -up);
                let step = Vec3::new(sx, up + down, sz);
                if horizontal_squared(step) > horizontal_squared(movement) {
                    movement = step;
                    stepped_y = Some(sl.y);
                }
            }
        }
        let mut result = position;
        for a in 0..3 {
            result[a] += movement[a];
        }
        if let Some(y) = stepped_y {
            result.y = y;
        }
        Ok((result, std::array::from_fn(|a| movement[a] != velocity[a])))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fast_wall_preserves_slide() {
        let scene = SolidScene {
            boxes: vec![
                SolidBox::new(Vec3::new(2.0, 65.0, -5.0), Vec3::new(3.0, 74.0, 6.0)).unwrap(),
            ],
        };
        let actor = Actor::new(Vec3::new(0.5, 65.0, 0.5), Body::PROBE)
            .unwrap()
            .impulse(Vec3::new(5.0, 0.0, 0.4))
            .unwrap()
            .tick_solid(&scene)
            .unwrap();
        assert!((actor.position.x - 1.7).abs() < 1e-6);
        assert_eq!(actor.velocity.x, 0.0);
        assert!(actor.velocity.z > 0.0);
    }
    #[test]
    fn step_checks_swept_headroom() {
        let scene = SolidScene {
            boxes: vec![
                SolidBox::new(Vec3::new(2.0, 1.0, -5.0), Vec3::new(6.0, 2.0, 6.0)).unwrap(),
                SolidBox::new(Vec3::new(1.0, 4.0, -5.0), Vec3::new(6.0, 5.0, 6.0)).unwrap(),
            ],
        };
        let actor = Actor::new(Vec3::new(0.5, 1.692_959, 0.5), Body::ARMOR_STAND)
            .unwrap()
            .impulse(Vec3::new(3.0, 0.0, 0.0))
            .unwrap()
            .tick_solid(&scene)
            .unwrap();
        assert_eq!(actor.position.y, 2.0);
        assert!(actor.tick_solid(&scene).unwrap().on_ground);
    }
    #[test]
    fn slowing_uses_signed_threshold() {
        assert_eq!(
            slow_horizontal(Vec3::new(1.0, -0.5, 1.0)),
            Vec3::new(0.5, -0.5, 0.5)
        );
    }
    #[test]
    fn air_friction_and_honey_sample_different_depths() {
        let actor = Actor::new(Vec3::new(0.0, 65.1, 0.0), Body::PROBE)
            .unwrap()
            .impulse(Vec3::new(0.3, 0.0, 0.0))
            .unwrap();
        let ice = actor
            .tick_plane(Plane {
                y: 65.0,
                slipperiness: 0.98,
                slow: false,
                bounce: false,
            })
            .unwrap();
        assert_eq!(ice.velocity.x, 0.3 * (0.91 * 0.6));
    }
}
