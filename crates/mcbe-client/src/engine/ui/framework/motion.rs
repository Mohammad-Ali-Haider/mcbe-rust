//! Bounded presentation-only transitions; stable layout remains the hit-test source.
use crate::engine::animation::{Library, Tween};
use std::collections::BTreeMap;
struct Entry {
    tween: Tween,
    role: &'static str,
    seen: u64,
    last: f32,
}
#[derive(Default)]
pub struct Motion {
    entries: BTreeMap<String, Entry>,
    generation: u64,
    now: f64,
}
impl Motion {
    pub fn begin(&mut self, now: f64) {
        self.generation = self.generation.wrapping_add(1);
        self.now = now;
    }
    pub fn finish(&mut self) {
        self.entries.retain(|_, e| e.seen == self.generation);
    }
    pub fn value(
        &mut self,
        id: &str,
        target: f32,
        initial: f32,
        role: &'static str,
        library: &Library,
    ) -> f32 {
        if !target.is_finite() {
            return 0.;
        }
        if !library.enabled || id.len() > 192 {
            return target;
        }
        if self.entries.len() >= 4096 && !self.entries.contains_key(id) {
            return target;
        }
        let e = self.entries.entry(id.into()).or_insert_with(|| Entry {
            tween: Tween::new(initial),
            role,
            seen: self.generation,
            last: initial,
        });
        e.seen = self.generation;
        e.role = role;
        e.tween.retarget(target, self.now, library, role);
        e.last = e.tween.value(self.now, library, role);
        if !e.tween.active(self.now, library, role) {
            e.tween = Tween::new(e.last);
        }
        e.last
    }
    pub fn active(&self, now: f64, library: &Library) -> bool {
        self.entries.values().any(|e| {
            library.enabled
                && (e.tween.active(now, library, e.role)
                    || e.tween.value(now, library, e.role) != e.last)
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selection_and_progress_retarget_smoothly_and_closed_widgets_release_state() {
        let lib = Library::shared();
        let mut m = Motion::default();
        m.begin(1.);
        assert_eq!(m.value("progress", 0.5, 0., "ui_progress", &lib), 0.);
        m.finish();
        assert!(m.active(1.01, &lib));
        m.begin(1.09);
        let half = m.value("progress", 0.5, 0., "ui_progress", &lib);
        m.finish();
        assert!(half > 0. && half < 0.5);
        m.begin(1.09);
        assert_eq!(m.value("progress", 0.2, 0., "ui_progress", &lib), half);
        m.finish();
        m.begin(2.);
        assert_eq!(m.value("progress", 0.2, 0., "ui_progress", &lib), 0.2);
        m.finish();
        assert!(!m.active(2., &lib));
        m.begin(3.);
        m.finish();
        assert!(m.entries.is_empty());
    }
}

#[cfg(test)]
mod stalled_frame_tests {
    use super::*;
    #[test]
    fn a_transition_still_requests_its_final_frame_after_a_long_pause() {
        let lib = Library::shared();
        let mut m = Motion::default();
        m.begin(1.);
        m.value("focus", 1., 0., "ui_focus", &lib);
        m.finish();
        assert!(
            m.active(10., &lib),
            "last drawn frame still differs from settled state"
        );
        m.begin(10.);
        assert_eq!(m.value("focus", 1., 0., "ui_focus", &lib), 1.);
        m.finish();
        assert!(!m.active(10., &lib));
    }
}
