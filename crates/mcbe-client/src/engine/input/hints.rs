//! Contextual control rows; UI skins can replace keycaps through ordinary theme roles.
use super::gamepad::Device;
#[derive(Clone, Copy, serde::Serialize)]
pub struct Hint {
    pub keyboard: &'static str,
    pub pad: &'static str,
    pub role: &'static str,
    pub label: &'static str,
}
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub enum Use {
    #[default]
    Place,
    Interact,
    Bucket,
    Entity,
}
fn h(keyboard: &'static str, pad: &'static str, role: &'static str, label: &'static str) -> Hint {
    Hint {
        keyboard,
        pad,
        role,
        label,
    }
}
pub fn rows(
    client: &crate::scene::Client,
    palette: &crate::engine_ui::Palette,
    menu: &crate::engine_settings::Menu,
    keyboard: bool,
    use_kind: Use,
) -> (Vec<Hint>, Vec<Hint>) {
    if keyboard {
        return (
            vec![
                h("Arrows", "D-pad", "pad_nav", "Choose"),
                h("Enter", "A", "pad_a", "Type"),
            ],
            vec![
                h("Backspace", "X", "pad_x", "Erase"),
                h("Esc", "B", "pad_b", "Done"),
            ],
        );
    }
    if menu.open() {
        return (
            vec![
                h("Arrows", "D-pad", "pad_nav", "Navigate / change"),
                h("Enter", "A", "pad_a", "Select"),
            ],
            vec![h("Esc", "B", "pad_b", "Back")],
        );
    }
    if palette.open && palette.screen_id() == "dimensions" {
        return (
            vec![
                h("Arrows", "D-pad", "pad_nav", "Navigate"),
                h("Enter", "A", "pad_a", "Visit"),
                h("PgUp/Dn", "LB/RB", "pad_lb", "Page"),
            ],
            vec![
                h("Tab", "LT/RT", "pad_rt", "Focus"),
                h("Esc", "B", "pad_b", "Close"),
            ],
        );
    }
    if palette.open {
        return (
            vec![
                h("Arrows", "D-pad", "pad_nav", "Navigate"),
                h(
                    "LMB",
                    "A",
                    "pad_a",
                    if palette.hover == Some(crate::engine_ui::Target::Search) {
                        "Search"
                    } else {
                        "Pick / place"
                    },
                ),
                h("RMB", "X", "pad_x", "Split / one"),
                h("Tab", "LT/RT", "pad_rt", "Focus"),
                h("Wheel", "LB/RB", "pad_lb", "Page"),
            ],
            vec![
                h("Shift", "Y", "pad_y", "Quick move"),
                h("Tab", "View", "pad_view", "Search"),
                h("Esc", "B", "pad_b", "Close"),
            ],
        );
    }
    let mut left = vec![
        h("WASD", "LS", "pad_ls", "Move"),
        h("Mouse", "RS", "pad_rs", "Look"),
    ];
    let action = client.parkour.action;
    use crate::engine::player::locomotion::parkour::Action;
    if action == Action::Hanging {
        left.push(h("Space", "A", "pad_a", "Pull up"));
        left.push(h("Shift", "B", "pad_b", "Drop"));
    } else if matches!(action, Action::Climbing | Action::WallRunning) {
        left.push(h("Space", "A", "pad_a", "Wall jump"));
        left.push(h("Shift", "B", "pad_b", "Drop"));
    } else if client.view.is_free() {
        left.push(h("Space", "A", "pad_a", "Rise"));
        left.push(h("Shift", "B", "pad_b", "Descend"));
    } else if client.movement.swimming
        || client.movement.mode != crate::engine_player::Mode::Walking
    {
        left.push(h("Space", "A", "pad_a", "Rise"));
        left.push(h(
            "Shift",
            "B",
            "pad_b",
            if client.movement.swimming {
                "Dive"
            } else {
                "Descend"
            },
        ));
    } else {
        left.push(h("Space", "A", "pad_a", "Jump"));
        left.push(h("C", "RS click", "pad_rs", "Grip wall"));
        left.push(h("Ctrl", "LS click", "pad_ls", "Sprint"));
    }
    let right = if client.view.is_free() {
        vec![
            h("Ctrl", "LS click", "pad_ls", "Faster"),
            h("F5", "X", "pad_x", "Return camera"),
            h("Esc", "Menu", "pad_menu", "Pause"),
        ]
    } else {
        vec![
            h(
                "LMB",
                "RT",
                "pad_rt",
                if use_kind == Use::Entity {
                    "Hit"
                } else {
                    "Remove"
                },
            ),
            h(
                "RMB",
                "LT",
                "pad_lt",
                match use_kind {
                    Use::Place => "Place",
                    Use::Interact => "Open",
                    Use::Bucket => "Collect / pour",
                    Use::Entity => "Follow / stay",
                },
            ),
            h("E", "Y", "pad_y", "Inventory"),
            h("F5", "X", "pad_x", "Camera"),
            h("Esc", "Menu", "pad_menu", "Pause"),
        ]
    };
    let mut right = right;
    if !client.view.is_free()
        && client
            .scene
            .engine
            .as_ref()
            .is_some_and(|s| s.player.offhand.is_some())
    {
        right.insert(2, h("Alt+RMB", "RS+LT", "pad_lt", "Use offhand"));
        right.insert(3, h("V", "RS+X", "pad_rs", "Swap hands"));
    }
    (left, right)
}
impl Hint {
    pub fn style_role(self, device: Device) -> &'static str {
        if device == Device::Gamepad {
            return self.role;
        }
        match self.keyboard {
            "WASD" => "key_wasd",
            "Arrows" => "key_arrows",
            "Mouse" => "mouse_look",
            "LMB" => "mouse_left",
            "RMB" => "mouse_right",
            "Wheel" => "mouse_wheel",
            "Alt+RMB" => "mouse_offhand",
            _ => "keycap",
        }
    }
    pub fn key(self, device: Device) -> &'static str {
        if device == Device::Gamepad {
            self.pad
        } else {
            self.keyboard
        }
    }
}
