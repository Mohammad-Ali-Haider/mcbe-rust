//! Pure input normalization and edge/repeat routing, independent of device APIs.
use std::time::Duration;
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Device {
    #[default]
    KeyboardMouse,
    Gamepad,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Context {
    Gameplay,
    Menu,
    Panel,
    Keyboard,
    Busy,
    Blocked,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Button {
    Up = 1,
    Down = 2,
    Left = 4,
    Right = 8,
    Start = 16,
    View = 32,
    LeftStick = 64,
    RightStick = 128,
    LeftShoulder = 256,
    RightShoulder = 512,
    South = 4096,
    East = 8192,
    West = 16384,
    North = 32768,
    LeftTrigger = 65536,
    RightTrigger = 131072,
}
impl Button {
    pub fn bit(self) -> u32 {
        self as u32
    }
}
const VALID: u32 = 0x3_f3ff;
#[derive(Clone, Copy, Default, Debug)]
pub struct Pad {
    pub buttons: u32,
    pub movement: [f32; 2],
    pub look: [f32; 2],
    pub triggers: [f32; 2],
}
#[derive(Clone, Copy, Default, Debug)]
pub struct Snapshot {
    pub slot: u8,
    pub pad: Pad,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}
impl Direction {
    pub fn vector(self) -> [f32; 2] {
        match self {
            Self::Up => [0., -1.],
            Self::Down => [0., 1.],
            Self::Left => [-1., 0.],
            Self::Right => [1., 0.],
        }
    }
}
#[derive(Clone, Copy, Default, Debug)]
pub struct Frame {
    pub held: u32,
    pub pressed: u32,
    pub released: u32,
    pub movement: [f32; 2],
    pub look: [f32; 2],
    pub navigation: Option<Direction>,
    pub disconnected: bool,
    pub activity: bool,
}
impl Frame {
    pub fn down(self, b: Button) -> bool {
        self.held & b.bit() != 0
    }
    pub fn pressed(self, b: Button) -> bool {
        self.pressed & b.bit() != 0
    }
}
fn finite(v: f32) -> f32 {
    if v.is_finite() { v.clamp(-1., 1.) } else { 0. }
}
/// Radial dead zone, bounded diagonal magnitude and finite output for every input.
pub fn stick(axis: [f32; 2]) -> [f32; 2] {
    let a = axis.map(finite);
    let length = (a[0] * a[0] + a[1] * a[1]).sqrt();
    if length <= 0.18 {
        return [0.; 2];
    }
    let scale = ((length.min(1.) - 0.18) / 0.82) / length;
    [a[0] * scale, a[1] * scale]
}
#[derive(Default)]
struct Repeat {
    direction: Option<Direction>,
    next: Duration,
}
impl Repeat {
    fn poll(&mut self, direction: Option<Direction>, now: Duration) -> Option<Direction> {
        if direction != self.direction {
            self.direction = direction;
            self.next = now + Duration::from_millis(350);
            return direction;
        }
        if direction.is_some() && now >= self.next {
            self.next = now + Duration::from_millis(100);
            return direction;
        }
        None
    }
}
#[derive(Default)]
pub struct Router {
    pub device: Device,
    pub revision: u64,
    pub slot: Option<u8>,
    context: Option<Context>,
    previous: u32,
    raw_buttons: u32,
    blocked: u32,
    blocked_sticks: [bool; 2],
    triggers: [bool; 2],
    previous_axes: [[f32; 2]; 2],
    raw_axes: [[f32; 2]; 2],
    repeat: Repeat,
}
impl Router {
    fn device(&mut self, device: Device) {
        if self.device != device {
            self.device = device;
            self.revision += 1;
        }
    }
    pub fn keyboard_activity(&mut self) {
        self.device(Device::KeyboardMouse);
        self.quarantine();
    }
    pub fn quarantine(&mut self) {
        self.blocked |= self.raw_buttons;
        for i in 0..2 {
            self.blocked_sticks[i] |= stick(self.raw_axes[i]) != [0.; 2];
        }
        self.previous = 0;
        self.repeat = Repeat::default();
    }
    pub fn suspend(&mut self) {
        self.quarantine();
        self.context = Some(Context::Blocked);
    }
    pub fn sample(&mut self, snapshot: Option<Snapshot>, context: Context, now: Duration) -> Frame {
        let Some(snapshot) = snapshot else {
            let disconnected = self.slot.take().is_some();
            let revision = self.revision + u64::from(disconnected);
            let device = self.device;
            *self = Self {
                device,
                revision,
                context: Some(context),
                ..Default::default()
            };
            if disconnected {
                self.device(Device::KeyboardMouse);
            }
            return Frame {
                disconnected,
                ..Default::default()
            };
        };
        let pad = snapshot.pad;
        let mut buttons = pad.buttons & VALID;
        let axes = [stick(pad.movement), stick(pad.look)];
        for (i, value) in pad.triggers.into_iter().enumerate() {
            let value = if value.is_finite() {
                value.clamp(0., 1.)
            } else {
                0.
            };
            self.triggers[i] = if self.triggers[i] {
                value >= 0.4
            } else {
                value > 0.55
            };
            if self.triggers[i] {
                buttons |= if i == 0 {
                    Button::LeftTrigger.bit()
                } else {
                    Button::RightTrigger.bit()
                };
            }
        }
        let connected = self.slot != Some(snapshot.slot);
        let reactivated = self.context == Some(Context::Blocked);
        if !connected && self.context != Some(context) {
            self.quarantine();
        }
        self.raw_buttons = buttons;
        self.raw_axes = [pad.movement, pad.look];
        if connected || self.context != Some(context) {
            self.slot = Some(snapshot.slot);
            self.context = Some(context);
            self.revision += 1;
            if connected || reactivated {
                self.quarantine();
            }
        }
        self.blocked &= buttons;
        buttons &= !self.blocked;
        let mut axes = axes;
        for (i, axis) in axes.iter_mut().enumerate() {
            if *axis == [0.; 2] {
                self.blocked_sticks[i] = false;
            }
            if self.blocked_sticks[i] {
                *axis = [0.; 2];
            }
        }
        if context == Context::Blocked {
            self.quarantine();
            return Frame::default();
        }
        let pressed = buttons & !self.previous;
        let released = self.previous & !buttons;
        self.previous = buttons;
        let changed_axes = axes.iter().zip(self.previous_axes).any(|(a, b)| {
            a[0].abs().max(a[1].abs()) > 0.1 && (a[0] - b[0]).abs().max((a[1] - b[1]).abs()) > 0.025
        });
        self.previous_axes = axes;
        let activity = pressed != 0 || changed_axes;
        if activity {
            self.device(Device::Gamepad);
        }
        let raw_direction = if buttons & Button::Up.bit() != 0 && buttons & Button::Down.bit() == 0
        {
            Some(Direction::Up)
        } else if buttons & Button::Down.bit() != 0 && buttons & Button::Up.bit() == 0 {
            Some(Direction::Down)
        } else if buttons & Button::Left.bit() != 0 && buttons & Button::Right.bit() == 0 {
            Some(Direction::Left)
        } else if buttons & Button::Right.bit() != 0 && buttons & Button::Left.bit() == 0 {
            Some(Direction::Right)
        } else if axes[0][0].abs().max(axes[0][1].abs()) > 0.6 {
            if axes[0][0].abs() > axes[0][1].abs() {
                Some(if axes[0][0] > 0. {
                    Direction::Right
                } else {
                    Direction::Left
                })
            } else {
                Some(if axes[0][1] > 0. {
                    Direction::Up
                } else {
                    Direction::Down
                })
            }
        } else {
            None
        };
        let navigation = if matches!(context, Context::Menu | Context::Panel | Context::Keyboard)
            && self.device == Device::Gamepad
        {
            self.repeat.poll(raw_direction, now)
        } else {
            self.repeat = Repeat::default();
            None
        };
        if self.device != Device::Gamepad {
            return Frame {
                released,
                activity,
                ..Default::default()
            };
        }
        Frame {
            held: buttons,
            pressed,
            released,
            movement: axes[0],
            look: axes[1],
            navigation,
            activity,
            ..Default::default()
        }
    }
}
#[cfg(test)]
mod tests;
