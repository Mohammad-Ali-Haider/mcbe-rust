//! Finite Script-API-observed container notifications, not a full redstone network.
//! Dispense requests must be executed by an item/entity system before live use.
//! Script inputs follow that tick's world work. Signals arrive at the observed
//! odd global phase after two or three ticks; the last message per delivery wins.
use crate::{Result, container_geometry::ContainerKind};
use std::collections::BTreeMap;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Dispense,
}
pub struct Controller {
    kind: ContainerKind,
    flag: bool,
    phase: u8,
    checks: BTreeMap<u64, bool>,
    dispense: Option<u64>,
    last_tick: Option<u64>,
}
impl Controller {
    pub fn new(kind: ContainerKind, flag: bool) -> Self {
        Self {
            kind,
            flag,
            phase: 0,
            checks: BTreeMap::new(),
            dispense: None,
            last_tick: None,
        }
    }
    pub fn with_phase(kind: ContainerKind, flag: bool, phase: u8) -> Result<Self> {
        if phase > 1 {
            return Err("Container clock phase must be zero or one".into());
        }
        Ok(Self {
            phase,
            ..Self::new(kind, flag)
        })
    }
    fn schedule(&mut self, at: u64, power: bool) -> Result<()> {
        if self.last_tick.map_or(at != 0, |last| at != last) {
            return Err("Notifications follow the observed tick's world work".into());
        }
        let mut due = at
            .checked_add(2)
            .ok_or("Container notification tick overflow")?;
        if (due % 2 + u64::from(self.phase)) % 2 == 0 {
            due = due.checked_add(1).ok_or("Container phase tick overflow")?;
        }
        self.checks.insert(due, power);
        Ok(())
    }
    /// The observed isolated setPermutation context, without adjacent notifications.
    pub fn permutation_changed(&mut self, at: u64, flag: bool, power: bool) -> Result<()> {
        self.schedule(at, power)?;
        self.flag = flag;
        Ok(())
    }
    /// Direct redstone-block changes and the measured stone-spacer control.
    pub fn neighbor_changed(&mut self, at: u64, power: bool) -> Result<()> {
        self.schedule(at, power)
    }
    pub fn flag(&self) -> bool {
        self.flag
    }
    pub fn tick(&mut self, tick: u64) -> Result<Vec<Event>> {
        if self
            .last_tick
            .map_or(tick != 0, |last| last.checked_add(1) != Some(tick))
        {
            return Err("Container ticks must be consecutive from zero".into());
        }
        let mut events = Vec::new();
        // Due block work runs before the signal batch at this observation tick.
        if self.dispense.is_some_and(|due| due <= tick) {
            self.dispense = None;
            events.push(Event::Dispense);
        }
        if let Some(power) = self.checks.remove(&tick)
            && matches!(self.kind, ContainerKind::Dispenser | ContainerKind::Hopper)
        {
            if self.kind == ContainerKind::Dispenser
                && !self.flag
                && power
                && self.dispense.is_none()
            {
                self.dispense = Some(tick.checked_add(4).ok_or("Dispense tick overflow")?);
            }
            self.flag = power;
        }
        self.last_tick = Some(tick);
        Ok(events)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn falling_power_does_not_cancel_an_accepted_dispense() {
        let mut c = Controller::new(ContainerKind::Dispenser, false);
        c.neighbor_changed(0, true).unwrap();
        for tick in 0..=7 {
            let events = c.tick(tick).unwrap();
            if tick == 4 {
                c.neighbor_changed(tick, false).unwrap();
            }
            assert_eq!(events.len(), usize::from(tick == 7));
        }
        assert!(!c.flag());
    }
    #[test]
    fn clock_phase_sets_two_or_three_tick_latency() {
        for phase in 0..=1 {
            let mut c = Controller::with_phase(ContainerKind::Hopper, true, phase).unwrap();
            c.permutation_changed(0, true, false).unwrap();
            c.tick(0).unwrap();
            c.tick(1).unwrap();
            assert!(c.flag());
            c.tick(2).unwrap();
            assert_eq!(c.flag(), phase == 0);
            c.tick(3).unwrap();
            assert!(!c.flag());
            assert!(c.tick(5).is_err());
            assert!(c.neighbor_changed(2, true).is_err());
        }
    }
    #[test]
    fn delayed_pulses_and_same_tick_retrigger_are_retained() {
        let mut c = Controller::with_phase(ContainerKind::Dispenser, false, 1).unwrap();
        let mut events = Vec::new();
        for tick in 0..=10 {
            for event in c.tick(tick).unwrap() {
                events.push((tick, event));
            }
            if tick == 0 || tick == 4 {
                c.neighbor_changed(tick, true).unwrap();
            }
            if tick == 1 {
                c.neighbor_changed(tick, false).unwrap();
            }
        }
        assert_eq!(events, vec![(6, Event::Dispense), (10, Event::Dispense)]);
    }
}
