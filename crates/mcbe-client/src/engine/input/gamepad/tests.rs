use super::*;
fn at(ms: u64) -> Duration {
    Duration::from_millis(ms)
}
fn snapshot(buttons: u32) -> Option<Snapshot> {
    Some(Snapshot {
        slot: 0,
        pad: Pad {
            buttons,
            ..Default::default()
        },
    })
}
#[test]
fn axes_are_radial_finite_and_bounded() {
    for p in [
        [0.1, 0.1],
        [f32::NAN, 0.],
        [f32::INFINITY, f32::NEG_INFINITY],
    ] {
        assert_eq!(stick(p), [0.; 2]);
    }
    for p in [[1., 1.], [-1., 1.], [f32::MAX, 0.], [-1., -1.]] {
        let a = stick(p);
        assert!(a.iter().all(|v| v.is_finite()));
        assert!((a[0] * a[0] + a[1] * a[1]).sqrt() <= 1.00001);
    }
    assert_eq!(stick([1., 0.]), [1., 0.]);
    assert_eq!(stick([0., -1.]), [0., -1.]);
}
#[test]
fn connect_focus_and_context_changes_require_fresh_presses() {
    let mut r = Router::default();
    let a = Button::South.bit();
    assert!(
        !r.sample(snapshot(a), Context::Gameplay, at(0))
            .pressed(Button::South)
    );
    r.sample(snapshot(0), Context::Gameplay, at(1));
    assert!(
        r.sample(snapshot(a), Context::Gameplay, at(2))
            .pressed(Button::South)
    );
    assert!(
        !r.sample(snapshot(a), Context::Panel, at(3))
            .pressed(Button::South)
    );
    r.sample(snapshot(0), Context::Panel, at(4));
    assert!(
        r.sample(snapshot(a), Context::Panel, at(5))
            .pressed(Button::South)
    );
    r.suspend();
    assert!(
        !r.sample(snapshot(a), Context::Gameplay, at(6))
            .down(Button::South)
    );
    r.sample(snapshot(0), Context::Gameplay, at(7));
    assert!(
        r.sample(snapshot(a), Context::Gameplay, at(8))
            .pressed(Button::South)
    );
    assert!(r.sample(None, Context::Gameplay, at(9)).disconnected);
    assert_eq!(r.device, Device::KeyboardMouse);
    assert!(
        !r.sample(snapshot(a), Context::Gameplay, at(10))
            .down(Button::South)
    );
}
#[test]
fn repeat_is_immediate_then_delayed_and_does_not_burst_after_stalls() {
    let mut r = Router::default();
    r.sample(snapshot(0), Context::Menu, at(0));
    let down = snapshot(Button::Down.bit());
    assert_eq!(
        r.sample(down, Context::Menu, at(1)).navigation,
        Some(Direction::Down)
    );
    assert_eq!(r.sample(down, Context::Menu, at(350)).navigation, None);
    assert_eq!(
        r.sample(down, Context::Menu, at(351)).navigation,
        Some(Direction::Down)
    );
    assert_eq!(
        r.sample(down, Context::Menu, at(10000)).navigation,
        Some(Direction::Down)
    );
    assert_eq!(r.sample(down, Context::Menu, at(10001)).navigation, None);
}
#[test]
fn trigger_hysteresis_and_keyboard_takeover_do_not_leave_stuck_actions() {
    let mut r = Router::default();
    r.sample(snapshot(0), Context::Gameplay, at(0));
    let raw = |t| {
        Some(Snapshot {
            slot: 0,
            pad: Pad {
                triggers: [t, 0.],
                ..Default::default()
            },
        })
    };
    assert!(
        r.sample(raw(0.6), Context::Gameplay, at(1))
            .pressed(Button::LeftTrigger)
    );
    assert!(
        r.sample(raw(0.45), Context::Gameplay, at(2))
            .down(Button::LeftTrigger)
    );
    r.keyboard_activity();
    assert!(
        !r.sample(raw(0.9), Context::Gameplay, at(3))
            .down(Button::LeftTrigger)
    );
    r.sample(raw(0.2), Context::Gameplay, at(4));
    assert!(
        r.sample(raw(0.8), Context::Gameplay, at(5))
            .pressed(Button::LeftTrigger)
    );
}
#[test]
fn held_sticks_are_quarantined_across_menu_transitions() {
    let mut r = Router::default();
    let raw = |axis| {
        Some(Snapshot {
            slot: 0,
            pad: Pad {
                movement: [0., axis],
                ..Default::default()
            },
        })
    };
    r.sample(raw(0.), Context::Gameplay, at(0));
    assert_eq!(
        r.sample(raw(1.), Context::Gameplay, at(1)).movement,
        [0., 1.]
    );
    assert_eq!(r.sample(raw(1.), Context::Menu, at(2)).navigation, None);
    r.sample(raw(0.), Context::Menu, at(3));
    assert_eq!(
        r.sample(raw(1.), Context::Menu, at(4)).navigation,
        Some(Direction::Up)
    );
}

#[test]
fn fresh_button_survives_context_change_while_held_button_is_blocked() {
    let mut r = Router::default();
    r.sample(snapshot(0), Context::Gameplay, at(0));
    r.sample(snapshot(Button::North.bit()), Context::Gameplay, at(1));
    let f = r.sample(
        snapshot(Button::North.bit() | Button::South.bit()),
        Context::Panel,
        at(2),
    );
    assert!(!f.down(Button::North));
    assert!(f.pressed(Button::South));
}
