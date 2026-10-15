//! Ordinary blocks subscribe to no events and need no custom event logic.
use super::BlockBehavior;
pub struct Passive;
impl BlockBehavior for Passive {}
