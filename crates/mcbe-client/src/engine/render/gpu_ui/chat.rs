use super::{Atlas, Canvas};
use crate::engine::ui::chat::{Chat, Kind};
impl Atlas {
    pub(super) fn chat_overlay(
        &self,
        c: &mut Canvas,
        chat: &Chat,
        viewport: [f32; 2],
        animations: &crate::engine::animation::Library,
    ) {
        let [width, height] = viewport;
        let panel = (width - 8.).clamp(32., 420.);
        let columns = ((panel - 12.) / 8.).max(1.) as usize;
        let rows = if chat.open {
            ((height - 80.) / 12.).clamp(1., 8.) as usize
        } else {
            5
        };
        let now = std::time::Instant::now();
        let lines = if chat.open {
            chat.lines(columns, rows, now)
                .into_iter()
                .map(|(k, t)| (k, t, 1.))
                .collect()
        } else {
            chat.faded_lines(columns, rows, now, animations)
        };
        let input_y = (height - 48.).max(16.);
        let base = if chat.open {
            input_y - 16.
        } else {
            height - 43.
        };
        for (i, (kind, line, opacity)) in lines.iter().rev().enumerate() {
            let y = base - i as f32 * 12.;
            if y < 4. {
                break;
            }
            c.rect([4., y - 2., panel, 12.], [0., 0., 0., 0.65 * opacity]);
            let mut color = match kind {
                Kind::Error => [1., 0.38, 0.38, 1.],
                Kind::System => [0.72, 1., 0.72, 1.],
                Kind::Command => [0.65, 0.75, 0.85, 1.],
                Kind::Player => [1.; 4],
            };
            color[3] *= opacity;
            self.text(c, 8., y, line, color);
        }
        if chat.open {
            c.rect([4., input_y - 3., panel, 15.], [0., 0., 0., 0.85]);
            let input = chat.input_window(columns);
            self.text(c, 8., input_y, &input, [1.; 4]);
            if chat.input.is_empty() {
                self.text(
                    c,
                    18.,
                    input_y,
                    "Type a message or /command",
                    [0.5, 0.5, 0.5, 1.],
                );
            }
            self.text(
                c,
                6.,
                height - 12.,
                "Enter send  Esc close  Up/Down history  Tab complete",
                [0.8, 0.8, 0.8, 1.],
            );
        }
    }
}
