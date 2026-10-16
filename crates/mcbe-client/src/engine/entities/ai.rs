//! Behavior interface: sensing and decisions produce goals; physics owns movement.
use super::{components::*, random};
pub struct Context {
    pub tick: u64,
    pub position: [f32; 3],
    pub player: [f32; 3],
    pub sense: f32,
}
pub trait BehaviorSystem: Sync {
    fn decide(&self, brain: &mut Brain, context: &Context);
}
pub struct Idle;
pub struct Wander;
pub struct Flee;
pub struct Follow;
pub struct Chase;
impl BehaviorSystem for Idle {
    fn decide(&self, b: &mut Brain, c: &Context) {
        b.goal = None;
        b.activity = Activity::Idle;
        b.decide_at = c.tick + 20;
    }
}
impl BehaviorSystem for Wander {
    fn decide(&self, b: &mut Brain, c: &Context) {
        if b.goal.is_some_and(|p| distance(p, c.position) > 0.8) && c.tick < b.decide_at {
            return;
        }
        b.decide_at = c.tick + 40 + random(&mut b.random) % 60;
        if random(&mut b.random).is_multiple_of(4) {
            b.goal = None;
            b.activity = Activity::Idle;
            return;
        }
        let angle = (random(&mut b.random) % 65536) as f32 / 65536. * std::f32::consts::TAU;
        let length = 3. + (random(&mut b.random) % 5) as f32;
        b.goal = Some([
            c.position[0] + angle.sin() * length,
            c.position[1],
            c.position[2] + angle.cos() * length,
        ]);
        b.activity = Activity::Wandering;
    }
}
impl BehaviorSystem for Flee {
    fn decide(&self, b: &mut Brain, c: &Context) {
        if distance(c.position, c.player) < c.sense || b.flee_until > c.tick {
            let mut x = c.position[0] - c.player[0];
            let z = c.position[2] - c.player[2];
            if x.abs() + z.abs() < 0.01 {
                x = 1.;
            }
            let length = (x * x + z * z).sqrt();
            b.goal = Some([
                c.position[0] + x / length * 8.,
                c.position[1],
                c.position[2] + z / length * 8.,
            ]);
            b.activity = Activity::Fleeing;
            b.decide_at = c.tick + 5;
        } else {
            Wander.decide(b, c)
        }
    }
}
impl BehaviorSystem for Follow {
    fn decide(&self, b: &mut Brain, c: &Context) {
        let d = distance(c.position, c.player);
        b.goal = (d > 2. && d <= 32.).then_some(c.player);
        b.activity = if b.goal.is_some() {
            Activity::Following
        } else {
            Activity::Idle
        };
        b.decide_at = c.tick + 5;
    }
}
impl BehaviorSystem for Chase {
    fn decide(&self, b: &mut Brain, c: &Context) {
        let d = distance(c.position, c.player);
        if d > 1.6 && d <= c.sense {
            b.goal = Some(c.player);
            b.activity = Activity::Chasing;
            b.decide_at = c.tick + 5;
        } else if d <= 1.6 {
            Idle.decide(b, c)
        } else {
            Wander.decide(b, c)
        }
    }
}
pub fn decide(brain: &mut Brain, context: &Context) {
    let system: &dyn BehaviorSystem = if brain.flee_until > context.tick {
        &Flee
    } else if brain.following {
        &Follow
    } else {
        match brain.behavior {
            Behavior::Idle => &Idle,
            Behavior::Wander => &Wander,
            Behavior::Flee => &Flee,
            Behavior::Follow => &Follow,
            Behavior::Chase => &Chase,
        }
    };
    system.decide(brain, context);
}
