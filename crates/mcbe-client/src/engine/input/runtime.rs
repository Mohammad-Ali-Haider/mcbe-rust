//! Main-thread snapshot consumer; the platform backend never runs on the render thread.
use super::{
    gamepad::{Context, Frame, Router, Snapshot},
    xinput::Service,
};
use std::time::{Duration, Instant};
pub struct Runtime {
    pub keyboard: crate::engine::ui::controller_keyboard::Keyboard,
    pub now: Duration,
    pub context: Context,
    pub router: Router,
    pub frame: Frame,
    pub sprint: bool,
    pub switched: bool,
    service: Option<Service>,
    epoch: Instant,
    pub injected: bool,
    pub injections: u64,
}
impl Default for Runtime {
    fn default() -> Self {
        Self {
            keyboard: Default::default(),
            now: Duration::ZERO,
            context: Context::Blocked,
            router: Router::default(),
            frame: Frame::default(),
            sprint: false,
            switched: false,
            service: None,
            epoch: Instant::now(),
            injected: false,
            injections: 0,
        }
    }
}
impl Runtime {
    pub fn start(&mut self) -> std::io::Result<()> {
        if self.service.is_none() {
            self.service = Some(Service::start()?);
        }
        Ok(())
    }
    pub fn elapsed(&self) -> Duration {
        self.epoch.elapsed()
    }
    pub fn poll(&mut self, context: Context) -> Frame {
        let snapshot = self.service.as_mut().and_then(Service::snapshot);
        self.sample(snapshot, context, self.elapsed())
    }
    pub fn sample(&mut self, snapshot: Option<Snapshot>, context: Context, now: Duration) -> Frame {
        self.now = now;
        self.context = context;
        let previous_device = self.router.device;
        self.frame = self.router.sample(snapshot, context, now);
        self.switched = previous_device != self.router.device;
        if context != Context::Gameplay || self.frame.movement[1] <= 0.1 {
            self.sprint = false;
        }
        if context == Context::Gameplay && self.frame.pressed(super::gamepad::Button::LeftStick) {
            self.sprint = !self.sprint;
        }
        self.frame
    }
    pub fn suspend(&mut self) {
        self.context = Context::Blocked;
        self.router.suspend();
        self.frame = Frame::default();
        self.sprint = false;
    }
    pub fn event_time(&self) -> Instant {
        self.epoch.checked_add(self.now).unwrap_or(self.epoch)
    }
    pub fn keyboard_activity(&mut self) {
        self.switched = false;
        self.keyboard.close();
        self.router.keyboard_activity();
        self.frame = Frame::default();
        self.sprint = false;
    }
    pub fn observation(&self) -> serde_json::Value {
        serde_json::json!({"backend":"Windows XInput 1.4","worker_started":self.service.is_some(),"poll_calls":self.service.as_ref().map_or(0,Service::poll_calls),"connected_slot":self.router.slot,"active_device":self.router.device,"revision":self.router.revision,"injected_raw_state":self.injected,"injections":self.injections})
    }
}
