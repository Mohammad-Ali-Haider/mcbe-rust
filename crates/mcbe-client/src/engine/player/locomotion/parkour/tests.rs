use super::*;
use crate::engine_player;
fn fixture(axis: usize, top: i32) -> Client {
    let mut c = crate::engine::generate(44).unwrap();
    let s = c.scene.engine.as_mut().unwrap();
    s.blocks.clear();
    let stone = s.registry.find("slate").unwrap();
    for x in -6..=6 {
        for z in -24..=24 {
            s.blocks.insert([x, 0, z], stone);
        }
    }
    for tangent in -20..=20 {
        for y in 1..top {
            let mut p = [0, y, 0];
            p[axis] = 2;
            p[2 - axis] = tangent;
            s.blocks.insert(p, stone);
        }
    }
    let p = if axis == 2 {
        Vec3::new(0.5, 1., 1.7)
    } else {
        Vec3::new(1.7, 3., 0.5)
    };
    c.set_spawn(p).unwrap();
    c.yaw = 0.;
    c
}
fn tick(c: &mut Client, i: Input) {
    engine_player::tick(c, i).unwrap();
    assert!(probes::clear_at(c, c.actor.position).unwrap());
}
#[test]
fn climb_catches_holds_and_pulls_onto_registered_ledge_without_penetration() {
    let mut c = fixture(2, 6);
    let input = Input {
        grab: true,
        forward: 1.,
        ..Default::default()
    };
    for _ in 0..35 {
        tick(&mut c, input);
    }
    assert_eq!(c.parkour.action, Action::Hanging);
    assert!((c.actor.position.y - 4.6).abs() < 0.001);
    let p = c.actor.position;
    for _ in 0..50 {
        tick(
            &mut c,
            Input {
                grab: true,
                ..Default::default()
            },
        );
    }
    assert_eq!(c.actor.position, p);
    tick(
        &mut c,
        Input {
            grab: true,
            jump: true,
            ..Default::default()
        },
    );
    assert_eq!(c.parkour.action, Action::Mantling);
    for _ in 0..20 {
        tick(
            &mut c,
            Input {
                grab: true,
                ..Default::default()
            },
        );
    }
    assert_eq!(c.parkour.action, Action::Idle);
    assert!(
        (c.actor.position.y - 6.).abs() < 0.001 && c.actor.position.z > 2.3 && c.actor.on_ground
    );
}
#[test]
fn wall_run_is_bounded_and_wall_jump_preserves_an_away_impulse() {
    let mut c = fixture(0, 9);
    c.actor.on_ground = false;
    let input = Input {
        grab: true,
        forward: 1.,
        sprint: true,
        ..Default::default()
    };
    let z = c.actor.position.z;
    for _ in 0..10 {
        tick(&mut c, input);
    }
    assert_eq!(c.parkour.action, Action::WallRunning);
    assert!(c.actor.position.z > z + 2.8);
    let p = c.actor.position;
    tick(
        &mut c,
        Input {
            jump: true,
            ..input
        },
    );
    assert_eq!(c.parkour.action, Action::WallJump);
    for _ in 0..4 {
        tick(&mut c, input);
    }
    assert!(c.actor.position.x < p.x - 0.8 && c.actor.position.y > p.y + 0.8);
    let mut c = fixture(0, 9);
    c.actor.on_ground = false;
    for _ in 0..31 {
        tick(&mut c, input);
    }
    assert_eq!(c.parkour.action, Action::Idle);
    assert!(c.parkour.cooldown > 0);
}
#[test]
fn changed_support_ceiling_and_modes_release_or_block_safely() {
    let mut c = fixture(2, 6);
    let input = Input {
        grab: true,
        forward: 1.,
        ..Default::default()
    };
    for _ in 0..35 {
        tick(&mut c, input);
    }
    assert_eq!(c.parkour.action, Action::Hanging);
    let stone = c
        .scene
        .engine
        .as_ref()
        .unwrap()
        .registry
        .find("slate")
        .unwrap();
    c.scene
        .engine
        .as_mut()
        .unwrap()
        .blocks
        .insert([0, 7, 2], stone);
    tick(
        &mut c,
        Input {
            grab: true,
            jump: true,
            ..Default::default()
        },
    );
    assert_eq!(c.parkour.action, Action::Hanging);
    let removed = c
        .scene
        .engine
        .as_ref()
        .unwrap()
        .blocks
        .keys()
        .filter(|p| p[2] == 2 && p[1] != 0)
        .collect::<Vec<_>>();
    for p in removed {
        c.scene.engine.as_mut().unwrap().blocks.remove(&p);
    }

    tick(
        &mut c,
        Input {
            grab: true,
            ..Default::default()
        },
    );
    assert_eq!(c.parkour.action, Action::Idle);
    engine_player::mode(&mut c, engine_player::Mode::Flying).unwrap();
    assert_eq!(c.parkour.action, Action::Idle);
}
#[test]
fn drop_and_teleport_clear_attachment_without_persisting_it() {
    let mut c = fixture(2, 6);
    let input = Input {
        grab: true,
        forward: 1.,
        ..Default::default()
    };
    for _ in 0..35 {
        tick(&mut c, input);
    }
    let y = c.actor.position.y;
    for _ in 0..8 {
        tick(
            &mut c,
            Input {
                sneak: true,
                ..Default::default()
            },
        );
    }
    assert_eq!(c.parkour.action, Action::Idle);
    assert!(c.actor.position.y < y - 0.5);
    c.set_spawn(Vec3::new(-3., 1., -3.)).unwrap();
    assert_eq!(c.parkour.action, Action::Idle);
    assert_eq!(c.parkour.cooldown, 0);
}

#[test]
fn falling_through_hand_height_still_catches_the_ledge() {
    let mut c = fixture(2, 6);
    c.set_spawn(Vec3::new(0.5, 4.3, 1.7)).unwrap();
    c.actor.on_ground = false;
    c.actor.velocity.y = -0.65;
    c.actor.displacement.y = -0.6;
    tick(
        &mut c,
        Input {
            grab: true,
            ..Default::default()
        },
    );
    assert_eq!(c.parkour.action, Action::Hanging);
    assert!((c.actor.position.y - 4.6).abs() < 0.002);
}
#[test]
fn unavailable_boundaries_and_passable_blocks_are_not_grippable() {
    let mut c = fixture(2, 6);
    c.set_spawn(Vec3::new(0.5, 3., c.scene.bounds.max[2] as f32 - 0.3))
        .unwrap();
    c.actor.on_ground = false;
    tick(
        &mut c,
        Input {
            grab: true,
            forward: 1.,
            ..Default::default()
        },
    );
    assert_eq!(c.parkour.action, Action::Idle);
    let mut c = fixture(2, 6);
    let s = c.scene.engine.as_mut().unwrap();
    let water = s.registry.find("water").unwrap();
    let wall = s
        .blocks
        .keys()
        .filter(|p| p[2] == 2 && p[1] > 0)
        .collect::<Vec<_>>();
    for p in wall {
        s.blocks.insert(p, water);
    }
    tick(
        &mut c,
        Input {
            grab: true,
            ..Default::default()
        },
    );
    assert_eq!(c.parkour.action, Action::Idle);
}
#[test]
fn negative_horizontal_and_high_vertical_coordinates_keep_collision_clear() {
    let mut c = fixture(2, 6);
    let cells = c
        .scene
        .engine
        .as_ref()
        .unwrap()
        .blocks
        .iter()
        .map(|(p, id)| ([p[0] - 32, p[1] + 10000, p[2] - 32], *id))
        .collect::<Vec<_>>();
    let s = c.scene.engine.as_mut().unwrap();
    s.blocks.clear();
    for (p, id) in cells {
        s.blocks.insert(p, id);
    }
    c.scene.bounds = mcbe_core::streaming::Bounds {
        min: [-48, 9990, -64],
        max: [48, 10050, 48],
    };
    c.set_spawn(Vec3::new(-31.5, 10001., -30.3)).unwrap();
    let input = Input {
        grab: true,
        forward: 1.,
        ..Default::default()
    };
    for _ in 0..35 {
        tick(&mut c, input);
    }
    assert_eq!(c.parkour.action, Action::Hanging);
    assert!((c.actor.position.y - 10004.6).abs() < 0.003);
    tick(
        &mut c,
        Input {
            grab: true,
            jump: true,
            ..Default::default()
        },
    );
    for _ in 0..20 {
        tick(
            &mut c,
            Input {
                grab: true,
                ..Default::default()
            },
        );
    }
    assert_eq!(c.parkour.action, Action::Idle);
    assert!((c.actor.position.y - 10006.).abs() < 0.003);
}

#[test]
fn all_four_wall_faces_climb_mantle_and_jump_outward() {
    for axis in [0, 2] {
        for normal in [-1., 1.] {
            let mut c = fixture(axis, 6);
            let mut p = Vec3::new(0.5, 1., 0.5);
            p[axis] = if normal < 0. { 1.7 } else { 3.3 };
            c.set_spawn(p).unwrap();
            c.yaw = match (axis, normal < 0.) {
                (0, true) => std::f32::consts::FRAC_PI_2,
                (0, false) => -std::f32::consts::FRAC_PI_2,
                (2, true) => 0.,
                _ => std::f32::consts::PI,
            };
            for _ in 0..35 {
                tick(
                    &mut c,
                    Input {
                        grab: true,
                        forward: 1.,
                        ..Default::default()
                    },
                );
            }
            assert_eq!(c.parkour.action, Action::Hanging);
            tick(
                &mut c,
                Input {
                    grab: true,
                    jump: true,
                    ..Default::default()
                },
            );
            for _ in 0..20 {
                tick(
                    &mut c,
                    Input {
                        grab: true,
                        ..Default::default()
                    },
                );
            }
            assert!((c.actor.position.y - 6.).abs() < 0.002);
            let mut c = fixture(axis, 6);
            p.y = 3.;
            c.set_spawn(p).unwrap();
            c.actor.on_ground = false;
            c.yaw = if axis == 0 {
                0.
            } else {
                std::f32::consts::FRAC_PI_2
            };
            let input = Input {
                grab: true,
                forward: 1.,
                sprint: true,
                ..Default::default()
            };
            for _ in 0..5 {
                tick(&mut c, input);
            }
            assert_eq!(c.parkour.action, Action::WallRunning);
            let before = c.actor.position[axis];
            tick(
                &mut c,
                Input {
                    jump: true,
                    ..input
                },
            );
            assert!((c.actor.position[axis] - before) * normal > 0.2);
        }
    }
}
