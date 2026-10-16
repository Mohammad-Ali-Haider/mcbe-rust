//! Spawn registered kinds using the same terrain/actor guards as other systems.
use super::*;
use crate::engine::entities::{self, components::Physics};
pub fn run(client: &mut Client, args: &[String]) -> Result<Outcome> {
    let state = client.scene.engine.as_ref().ok_or("No playable world")?;
    if state.entities.records.len() >= entities::MAX_ENTITIES {
        return Err("Entity population limit reached".into());
    }
    let d = state.entities.library.get(&args[0])?;
    let label = d.label.clone();
    let body = d.body.as_ref().map(|b| Physics {
        width: b.width,
        height: b.height,
        speed: b.speed,
        velocity: [0.; 3],
        grounded: false,
    });
    let origin = entities::components::position(client.actor.position);
    let direction = client.direction();
    let yaw = (direction[0] as f32).atan2(direction[2] as f32);
    for distance in [3., 4., 5.] {
        for offset in [0., 0.4, -0.4, 0.8, -0.8] {
            let angle = yaw + offset;
            let (x, z) = (
                origin[0] + angle.sin() * distance,
                origin[2] + angle.cos() * distance,
            );
            let position = if let Some(body) = &body {
                let Some(p) = entities::navigation::surface(client, x, z, origin[1], body) else {
                    continue;
                };
                p
            } else {
                [x, origin[1], z]
            };
            if let Ok(id) = entities::interaction::spawn(client, &args[0], position, true) {
                return Ok(Outcome::message(format!("Spawned {label} #{}", id.0)));
            }
        }
    }
    Err("No clear ground nearby; move into open space before spawning".into())
}
pub fn complete(client: &Client) -> Vec<String> {
    client.scene.engine.as_ref().map_or_else(Vec::new, |s| {
        s.entities.library.kinds.keys().cloned().collect()
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn spawn_command_uses_registered_kinds_and_keeps_failed_requests_atomic() {
        let mut c = crate::engine::generate(37).unwrap();
        let registry = Registry::built_in();
        let before = c.scene.engine.as_ref().unwrap().entities.saved();
        assert!(registry.execute(&mut c, "/spawn missing").is_err());
        assert_eq!(c.scene.engine.as_ref().unwrap().entities.saved(), before);
        registry.execute(&mut c, "/spawn cow").unwrap();
        registry.execute(&mut c, "/spawn pig").unwrap();
        let entities = &c.scene.engine.as_ref().unwrap().entities;
        assert_eq!(entities.records.len(), 2);
        assert!(entities.records.values().all(|e| e.persistent));
        assert!(
            registry
                .complete(&c, "/spawn zom")
                .contains(&"/spawn zombie ".to_string())
        );
    }
}
