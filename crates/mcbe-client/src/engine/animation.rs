//! Authored bounded scalar timelines and allocation-free, interruption-safe transitions.
use crate::scene::Result;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::Path,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
};
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Easing {
    #[default]
    Linear,
    SmoothStep,
    InCubic,
    OutCubic,
    InOutCubic,
    SineIn,
    SineOut,
}
impl Easing {
    pub fn sample(self, t: f32) -> f32 {
        let t = t.clamp(0., 1.);
        match self {
            Self::Linear => t,
            Self::SmoothStep => t * t * (3. - 2. * t),
            Self::InCubic => t * t * t,
            Self::OutCubic => 1. - (1. - t).powi(3),
            Self::InOutCubic => {
                if t < 0.5 {
                    4. * t * t * t
                } else {
                    1. - (-2. * t + 2.).powi(3) / 2.
                }
            }
            Self::SineIn => 1. - (t * std::f32::consts::FRAC_PI_2).cos(),
            Self::SineOut => (t * std::f32::consts::FRAC_PI_2).sin(),
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Playback {
    #[default]
    Once,
    Loop,
    PingPong,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Keyframe {
    pub time: f32,
    pub value: f32,
    #[serde(default)]
    pub easing: Easing,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Clip {
    pub duration: f32,
    #[serde(default)]
    pub playback: Playback,
    pub tracks: BTreeMap<String, Vec<Keyframe>>,
}
impl Clip {
    pub fn validate(&self) -> Result<()> {
        if !self.duration.is_finite()
            || !(0.001..=60.).contains(&self.duration)
            || self.tracks.is_empty()
            || self.tracks.len() > 32
        {
            return Err("Animation duration/track count invalid".into());
        }
        for (name, keys) in &self.tracks {
            if !crate::engine::blocks::state::identifier(name)
                || !(2..=64).contains(&keys.len())
                || keys[0].time != 0.
                || keys.last().unwrap().time != self.duration
                || keys.windows(2).any(|k| k[0].time >= k[1].time)
                || keys.iter().any(|k| {
                    !k.time.is_finite()
                        || !k.value.is_finite()
                        || !(0. ..=self.duration).contains(&k.time)
                        || k.value.abs() > 10000.
                })
            {
                return Err("Animation track/keyframes invalid".into());
            }
        }
        Ok(())
    }
    pub fn sample(&self, track: &str, seconds: f64) -> Option<f32> {
        if !seconds.is_finite() {
            return None;
        }
        let duration = f64::from(self.duration);
        let t = match self.playback {
            Playback::Once => seconds.clamp(0., duration),
            Playback::Loop => seconds.rem_euclid(duration),
            Playback::PingPong => {
                let t = seconds.rem_euclid(duration * 2.);
                if t > duration { duration * 2. - t } else { t }
            }
        } as f32;
        let keys = self.tracks.get(track)?;
        let i = keys.partition_point(|k| k.time <= t);
        if i == 0 {
            return Some(keys[0].value);
        }
        if i == keys.len() {
            return Some(keys[i - 1].value);
        }
        let a = &keys[i - 1];
        let b = &keys[i];
        let t = a.easing.sample((t - a.time) / (b.time - a.time));
        Some(a.value + (b.value - a.value) * t)
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Library {
    #[serde(skip, default = "next_id")]
    pub id: u64,
    pub version: u32,
    #[serde(default = "enabled")]
    pub enabled: bool,
    pub clips: BTreeMap<String, Clip>,
}
fn next_id() -> u64 {
    static ID: AtomicU64 = AtomicU64::new(1);
    ID.fetch_add(1, Ordering::Relaxed)
}
fn enabled() -> bool {
    true
}
impl Library {
    pub fn shared() -> Arc<Self> {
        static VALUE: OnceLock<Arc<Library>> = OnceLock::new();
        VALUE
            .get_or_init(|| {
                let v: Self = serde_json::from_slice(include_bytes!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../content/base/animations.json"
                )))
                .expect("Built-in animations");
                v.validate().expect("Valid built-in animations");
                Arc::new(v)
            })
            .clone()
    }
    pub fn read_home(home: &Path) -> Result<Self> {
        let p = home.join("content/base/animations.json");
        if !p.is_file() {
            return Ok((*Self::shared()).clone());
        }
        if p.metadata()?.len() > 256 * 1024 {
            return Err("Animation JSON exceeds 256 KiB".into());
        }
        let v: Self = serde_json::from_slice(&std::fs::read(p)?)?;
        v.compile()
    }
    pub fn compile(mut self) -> Result<Self> {
        self.validate()?;
        self.id = next_id();
        Ok(self)
    }
    pub fn validate(&self) -> Result<()> {
        if self.version != 1 || self.clips.len() > 64 {
            return Err("Animation library version/count invalid".into());
        }
        let mut total = 0;
        for (name, clip) in &self.clips {
            if !crate::engine::blocks::state::identifier(name) {
                return Err("Animation clip ID invalid".into());
            }
            clip.validate()?;
            total += clip.tracks.values().map(Vec::len).sum::<usize>();
        }
        if total > 8192 {
            return Err("Animation keyframe budget exceeded".into());
        }
        for name in [
            "player_walk",
            "entity_walk",
            "player_use",
            "ui_focus",
            "ui_selection",
            "ui_progress",
            "ui_reveal",
            "chat_fade",
            "loading",
        ] {
            let c = self
                .clips
                .get(name)
                .ok_or("Required animation clip missing")?;
            let keys = c
                .tracks
                .get("value")
                .ok_or("Required animation value track missing")?;
            if name.starts_with("ui_") || name == "chat_fade" {
                if c.playback != Playback::Once
                    || keys[0].value != 0.
                    || keys.last().unwrap().value != 1.
                    || keys.iter().any(|k| !(0. ..=1.).contains(&k.value))
                {
                    return Err("UI transitions need bounded once-only 0-to-1 tracks".into());
                }
            } else if keys.iter().any(|k| k.value.abs() > 2.) {
                return Err("Built-in motion amplitude exceeds two".into());
            }
        }
        Ok(())
    }
    pub fn sample(&self, name: &str, track: &str, seconds: f64) -> Option<f32> {
        self.clips.get(name)?.sample(track, seconds)
    }
    pub fn clip(&self, name: &str) -> &Clip {
        &self.clips[name]
    }
    pub fn wave(&self, name: &str, phase: f32) -> f32 {
        if !self.enabled || !phase.is_finite() {
            return 0.;
        }
        self.clip(name)
            .sample(
                "value",
                f64::from(phase.rem_euclid(1.)) * f64::from(self.clip(name).duration),
            )
            .unwrap_or(0.)
    }
    pub fn blend(&self, name: &str, seconds: f64) -> f32 {
        if !self.enabled {
            return 1.;
        }
        self.sample(name, "value", seconds)
            .unwrap_or(1.)
            .clamp(0., 1.)
    }
}
#[derive(Clone, Copy, Debug)]
pub struct Tween {
    from: f32,
    to: f32,
    at: f64,
}
impl Tween {
    pub fn new(value: f32) -> Self {
        Self {
            from: if value.is_finite() { value } else { 0. },
            to: if value.is_finite() { value } else { 0. },
            at: 0.,
        }
    }
    pub fn value(&self, now: f64, library: &Library, clip: &str) -> f32 {
        if !self.active(now, library, clip) {
            return self.to;
        }
        self.from + (self.to - self.from) * library.blend(clip, (now - self.at).max(0.))
    }
    pub fn active(&self, now: f64, library: &Library, clip: &str) -> bool {
        library.enabled
            && self.from != self.to
            && now < self.at + f64::from(library.clip(clip).duration)
    }
    pub fn retarget(&mut self, target: f32, now: f64, library: &Library, clip: &str) {
        if !target.is_finite() || !now.is_finite() {
            return;
        }
        if target != self.to {
            self.from = self.value(now, library, clip);
            self.to = target;
            self.at = now;
        }
    }
}
#[cfg(test)]
mod tests;
