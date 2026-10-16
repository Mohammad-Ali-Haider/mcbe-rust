//! XInput calls stay off the window/render thread; only one latest snapshot is queued.
use super::gamepad::{Pad, Snapshot};
use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};
use windows_sys::Win32::UI::Input::XboxController::{XINPUT_STATE, XInputGetState};
pub fn read(slot: u8) -> Option<Snapshot> {
    let mut state = XINPUT_STATE::default();
    // The bounded slot and initialized SDK structure satisfy XInput's pointer contract.
    if unsafe { XInputGetState(u32::from(slot), &mut state) } != 0 {
        return None;
    }
    let p = state.Gamepad;
    let axis = |v: i16| {
        if v < 0 {
            f32::from(v) / 32768.
        } else {
            f32::from(v) / 32767.
        }
    };
    Some(Snapshot {
        slot,
        pad: Pad {
            buttons: u32::from(p.wButtons),
            movement: [axis(p.sThumbLX), axis(p.sThumbLY)],
            look: [axis(p.sThumbRX), axis(p.sThumbRY)],
            triggers: [
                f32::from(p.bLeftTrigger) / 255.,
                f32::from(p.bRightTrigger) / 255.,
            ],
        },
    })
}
pub struct Service {
    stop: SyncSender<()>,
    rx: Receiver<Option<Snapshot>>,
    worker: Option<JoinHandle<()>>,
    latest: Option<Snapshot>,
    pub calls: Arc<AtomicU64>,
}
impl Service {
    pub fn start() -> std::io::Result<Self> {
        let (stop, signal) = mpsc::sync_channel(1);
        let (tx, rx) = mpsc::sync_channel(1);
        let calls = Arc::new(AtomicU64::new(0));
        let count = calls.clone();
        let worker = std::thread::Builder::new()
            .name("controller-poll".into())
            .spawn(move || {
                let mut active = None;
                let mut scan = Instant::now();
                loop {
                    let mut snapshot = None;
                    if let Some(slot) = active {
                        count.fetch_add(1, Ordering::Relaxed);
                        snapshot = read(slot);
                        if snapshot.is_none() {
                            active = None;
                            scan = Instant::now();
                        }
                    }
                    if active.is_none() && Instant::now() >= scan {
                        for slot in 0..4 {
                            count.fetch_add(1, Ordering::Relaxed);
                            if let Some(p) = read(slot) {
                                active = Some(slot);
                                snapshot = Some(p);
                                break;
                            }
                        }
                        scan = Instant::now() + Duration::from_secs(1);
                    }
                    // A full single-slot channel retains one snapshot instead of growing a backlog.
                    match tx.try_send(snapshot) {
                        Ok(()) | Err(mpsc::TrySendError::Full(_)) => {}
                        Err(mpsc::TrySendError::Disconnected(_)) => break,
                    }
                    if !matches!(
                        signal.recv_timeout(if active.is_some() {
                            Duration::from_millis(8)
                        } else {
                            Duration::from_millis(100)
                        }),
                        Err(mpsc::RecvTimeoutError::Timeout)
                    ) {
                        break;
                    }
                }
            })?;
        Ok(Self {
            stop,
            rx,
            worker: Some(worker),
            latest: None,
            calls,
        })
    }
    pub fn snapshot(&mut self) -> Option<Snapshot> {
        if let Ok(p) = self.rx.try_recv() {
            self.latest = p;
        }
        self.latest
    }
    pub fn poll_calls(&self) -> u64 {
        self.calls.load(Ordering::Relaxed)
    }
}
impl Drop for Service {
    fn drop(&mut self) {
        let _ = self.stop.try_send(());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn platform_worker_polls_and_joins_on_shutdown() {
        let mut service = Service::start().unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        while service.poll_calls() == 0 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(service.poll_calls() > 0);
        let _ = service.snapshot();
        drop(service);
    }
}
