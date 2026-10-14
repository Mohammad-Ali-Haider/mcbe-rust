use crate::scene::Result;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SignalComponent {
    #[serde(default = "delay")]
    pub delay: u16,
    #[serde(default)]
    pub source: u8,
    #[serde(default = "one")]
    pub attenuation: u8,
}
fn delay() -> u16 {
    1
}
fn one() -> u8 {
    1
}
impl SignalComponent {
    pub fn validate(&self) -> Result<()> {
        if self.source > 15
            || !(1..=15).contains(&self.attenuation)
            || !(1..=1200).contains(&self.delay)
        {
            return Err(
                "Signals require source 0..15, attenuation 1..15 and delay 1..1200 ticks".into(),
            );
        }
        Ok(())
    }
}
