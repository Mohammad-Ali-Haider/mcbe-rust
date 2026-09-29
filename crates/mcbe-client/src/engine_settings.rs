//! Small persisted client preferences and a keyboard/mouse settings controller.
use crate::scene::Result;
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};
use winit::keyboard::KeyCode;
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub version: u32,
    pub render_distance: i32,
    pub gui_scale: u32,
    pub fov: u32,
    pub sensitivity: u32,
    pub vsync: bool,
    pub frame_limit: u32,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            version: 1,
            render_distance: 3,
            gui_scale: 2,
            fov: 70,
            sensitivity: 100,
            vsync: false,
            frame_limit: 0,
        }
    }
}
impl Settings {
    pub fn validate(&self) -> Result<()> {
        if self.version != 1
            || !(2..=crate::engine_stream::MAX_RADIUS).contains(&self.render_distance)
            || !(1..=3).contains(&self.gui_scale)
            || !(50..=110).contains(&self.fov)
            || !(20..=200).contains(&self.sensitivity)
            || ![0, 60, 120, 144, 240].contains(&self.frame_limit)
        {
            return Err("Invalid client settings".into());
        }
        Ok(())
    }
    pub fn read(home: &Path) -> Result<Self> {
        let path = home.join("settings.json");
        if !path.exists() {
            return Ok(Self::default());
        }
        if fs::metadata(&path)?.len() > 4096 {
            return Err("Settings exceed 4 KiB".into());
        }
        let s: Self = serde_json::from_slice(&fs::read(path)?)?;
        s.validate()?;
        Ok(s)
    }
    pub fn write(&self, home: &Path) -> Result<()> {
        self.validate()?;
        fs::create_dir_all(home)?;
        let temp = home.join("settings.json.pending");
        fs::write(&temp, serde_json::to_vec_pretty(self)?)?;
        fs::rename(temp, home.join("settings.json"))?;
        Ok(())
    }
}
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    #[default]
    Closed,
    Pause,
    Settings,
}
#[derive(Default)]
pub struct Menu {
    pub page: Page,
    pub selected: usize,
    pub revision: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    None,
    Changed,
    Resume,
    Save,
    Fullscreen,
}
impl Menu {
    pub fn open(&self) -> bool {
        self.page != Page::Closed
    }
    pub fn escape(&mut self) -> Action {
        self.selected = 0;
        self.revision += 1;
        match self.page {
            Page::Closed => {
                self.page = Page::Pause;
                Action::None
            }
            Page::Settings => {
                self.page = Page::Pause;
                Action::None
            }
            Page::Pause => {
                self.page = Page::Closed;
                Action::Resume
            }
        }
    }
    pub fn count(&self) -> usize {
        if self.page == Page::Pause { 3 } else { 9 }
    }
    pub fn origin(view: [f32; 2]) -> [f32; 2] {
        [(view[0] - 340.) / 2., (view[1] - 268.) / 2.]
    }
    pub fn labels(&self, s: &Settings, streamed: bool, fullscreen: bool) -> Vec<String> {
        if self.page == Page::Pause {
            return vec!["Resume Game".into(), "Settings".into(), "Save World".into()];
        }
        vec![
            if streamed {
                format!("Render Distance: {} chunks", s.render_distance)
            } else {
                "Render Distance: next streamed world".into()
            },
            format!("GUI Scale: {}", s.gui_scale),
            format!("Field of View: {}", s.fov),
            format!("Mouse Sensitivity: {}%", s.sensitivity),
            format!("VSync: {}", if s.vsync { "On" } else { "Off" }),
            format!(
                "Maximum FPS: {}",
                if s.frame_limit == 0 {
                    "Unlimited".into()
                } else {
                    s.frame_limit.to_string()
                }
            ),
            format!("Fullscreen: {}", if fullscreen { "On" } else { "Off" }),
            "Reset Settings".into(),
            "Back".into(),
        ]
    }
    pub fn activate(&mut self, s: &mut Settings, delta: i32) -> Action {
        self.revision += 1;
        if self.page == Page::Pause {
            return match self.selected {
                0 => {
                    self.page = Page::Closed;
                    Action::Resume
                }
                1 => {
                    self.page = Page::Settings;
                    self.selected = 0;
                    Action::None
                }
                _ => Action::Save,
            };
        }
        match self.selected {
            0 => {
                s.render_distance =
                    (s.render_distance + delta).clamp(2, crate::engine_stream::MAX_RADIUS)
            }
            1 => s.gui_scale = (s.gui_scale as i32 + delta).clamp(1, 3) as u32,
            2 => s.fov = (s.fov as i32 + delta * 5).clamp(50, 110) as u32,
            3 => s.sensitivity = (s.sensitivity as i32 + delta * 10).clamp(20, 200) as u32,
            4 => s.vsync = !s.vsync,
            5 => {
                let choices = [0, 60, 120, 144, 240];
                let i = choices.iter().position(|v| *v == s.frame_limit).unwrap();
                s.frame_limit = choices[(i as i32 + delta).rem_euclid(5) as usize]
            }
            6 => return Action::Fullscreen,
            7 => *s = Settings::default(),
            _ => {
                self.page = Page::Pause;
                self.selected = 1;
                return Action::None;
            }
        }
        Action::Changed
    }
    pub fn key(&mut self, s: &mut Settings, key: KeyCode) -> Action {
        match key {
            KeyCode::Home | KeyCode::End if self.page == Page::Settings && self.selected == 0 => {
                s.render_distance = if key == KeyCode::End {
                    crate::engine_stream::MAX_RADIUS
                } else {
                    2
                };
                self.revision += 1;
                Action::Changed
            }
            KeyCode::Escape => self.escape(),
            KeyCode::ArrowUp => {
                self.selected = (self.selected + self.count() - 1) % self.count();
                self.revision += 1;
                Action::None
            }
            KeyCode::ArrowDown | KeyCode::Tab => {
                self.selected = (self.selected + 1) % self.count();
                self.revision += 1;
                Action::None
            }
            KeyCode::ArrowLeft => self.activate(s, -1),
            KeyCode::ArrowRight | KeyCode::Enter | KeyCode::Space => self.activate(s, 1),
            _ => Action::None,
        }
    }
    pub fn hover(&mut self, p: [f32; 2], view: [f32; 2]) {
        if let Some(row) = self.hit(p, view)
            && row != self.selected
        {
            self.selected = row;
            self.revision += 1;
        }
    }
    fn hit(&self, p: [f32; 2], view: [f32; 2]) -> Option<usize> {
        let [x, y] = Self::origin(view);
        let dy = p[1] - y - 34.;
        if p[0] < x || p[0] >= x + 340. || dy < 0. {
            return None;
        }
        let row = (dy / 23.) as usize;
        (row < self.count() && dy % 23. < 20.).then_some(row)
    }
    pub fn click(&mut self, s: &mut Settings, p: [f32; 2], view: [f32; 2]) -> Action {
        if let Some(row) = self.hit(p, view) {
            self.selected = row;
            let [x, _] = Self::origin(view);
            if self.page == Page::Settings && row == 0 && p[0] >= x + 20. && p[0] <= x + 320. {
                let fraction = ((p[0] - x - 30.) / 280.).clamp(0., 1.);
                s.render_distance =
                    2 + (fraction * (crate::engine_stream::MAX_RADIUS - 2) as f32).round() as i32;
                self.revision += 1;
                Action::Changed
            } else {
                self.activate(s, if p[0] < x + 170. { -1 } else { 1 })
            }
        } else {
            Action::None
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn settings_roundtrip_rejects_unbounded_work_and_unknown_fields() {
        let mut s = Settings {
            render_distance: 255,
            ..Default::default()
        };
        assert!(s.validate().is_ok());
        s.render_distance = 256;
        assert!(s.validate().is_err());
        assert!(serde_json::from_str::<Settings>("{}").is_err());
        let s = Settings::default();
        assert_eq!(
            s,
            serde_json::from_slice(&serde_json::to_vec(&s).unwrap()).unwrap()
        );
    }
    #[test]
    fn slider_and_home_end_select_the_full_distance_range() {
        let mut menu = Menu {
            page: Page::Settings,
            ..Default::default()
        };
        let mut s = Settings::default();
        let view = [640., 360.];
        let [x, y] = Menu::origin(view);
        menu.key(&mut s, KeyCode::End);
        assert_eq!(s.render_distance, 255);
        menu.key(&mut s, KeyCode::ArrowRight);
        assert_eq!(s.render_distance, 255);
        menu.key(&mut s, KeyCode::Home);
        assert_eq!(s.render_distance, 2);
        menu.click(&mut s, [x + 310., y + 40.], view);
        assert_eq!(s.render_distance, 255);
        menu.click(&mut s, [x + 30., y + 40.], view);
        assert_eq!(s.render_distance, 2);
        menu.click(&mut s, [x + 170., y + 40.], view);
        assert!((128..=129).contains(&s.render_distance));
    }
    #[test]
    fn menu_mouse_keyboard_and_back_agree() {
        let mut m = Menu::default();
        let mut s = Settings::default();
        m.escape();
        m.selected = 1;
        m.key(&mut s, KeyCode::Enter);
        assert_eq!(m.page, Page::Settings);
        let view = [640., 360.];
        let [x, y] = Menu::origin(view);
        assert_eq!(m.click(&mut s, [x + 330., y + 40.], view), Action::Changed);
        assert_eq!(s.render_distance, 4);
        m.key(&mut s, KeyCode::ArrowLeft);
        assert_eq!(s.render_distance, 3);
        m.escape();
        assert_eq!(m.page, Page::Pause);
        m.escape();
        assert!(!m.open());
    }
}
