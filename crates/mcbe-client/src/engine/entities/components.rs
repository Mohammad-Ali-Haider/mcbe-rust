//! Small typed components; optional components enable non-physical decorations too.
use mcbe_core::motion::{Actor, Body, Vec3};
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Id(pub u64);
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Transform {
    pub position: [f32; 3],
    pub yaw: f32,
    #[serde(skip)]
    pub previous: [f32; 3],
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Physics {
    pub width: f32,
    pub height: f32,
    pub speed: f32,
    pub velocity: [f32; 3],
    pub grounded: bool,
}
impl Physics {
    pub fn actor(&self, p: [f32; 3]) -> crate::scene::Result<Actor> {
        let mut a = Actor::new(
            Vec3::new(p[0], p[1], p[2]),
            Body {
                width: self.width,
                height: self.height,
            },
        )?;
        a.velocity = Vec3::new(self.velocity[0], self.velocity[1], self.velocity[2]);
        a.on_ground = self.grounded;
        Ok(a)
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Health {
    pub current: u16,
    pub maximum: u16,
    pub hurt_until: u64,
}
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Behavior {
    #[default]
    Idle,
    Wander,
    Flee,
    Follow,
    Chase,
}
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Activity {
    #[default]
    Idle,
    Wandering,
    Fleeing,
    Following,
    Chasing,
    Blocked,
    Sleeping,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Brain {
    pub behavior: Behavior,
    pub goal: Option<[f32; 3]>,
    pub decide_at: u64,
    pub random: u64,
    pub following: bool,
    pub flee_until: u64,
    #[serde(skip)]
    pub path: Vec<[f32; 3]>,
    #[serde(skip)]
    pub replan_at: u64,
    #[serde(skip)]
    pub activity: Activity,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Visual {
    pub model: String,
    #[serde(skip)]
    pub gait: f32,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entity {
    pub id: Id,
    pub kind: String,
    pub persistent: bool,
    pub transform: Transform,
    pub physics: Option<Physics>,
    pub health: Option<Health>,
    pub brain: Option<Brain>,
    pub visual: Option<Visual>,
}
pub fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    a.into_iter()
        .zip(b)
        .map(|(a, b)| (a - b).powi(2))
        .sum::<f32>()
        .sqrt()
}
pub fn position(v: Vec3) -> [f32; 3] {
    [v.x, v.y, v.z]
}
