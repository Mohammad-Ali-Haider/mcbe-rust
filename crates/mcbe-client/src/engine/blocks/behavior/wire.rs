//! Minimal six-neighbor signal behavior, intended as an extensible engine example.
use super::{BlockBehavior, BlockEvent, Command, EventContext, neighbors};
use crate::{
    engine_blocks::{Block, Registry},
    scene::Result,
};
pub struct Wire;
pub fn strength(block: &Block) -> u8 {
    let Some(signal) = &block.signal else {
        return 0;
    };
    if block.behavior != super::super::components::BehaviorComponent::Wire {
        return signal.source;
    }
    let source = signal.source;
    let power = block
        .state
        .as_ref()
        .and_then(|s| s.properties.get("power"))
        .and_then(|p| p.parse::<u8>().ok())
        .unwrap_or(0);
    source.max(power)
}
impl BlockBehavior for Wire {
    fn validate(&self, registry: &Registry, id: super::BlockId) -> Result<()> {
        let b = registry.get(id);
        let identity = b.state.as_ref().ok_or("Wire requires state identity")?;
        let value = identity
            .properties
            .get("power")
            .ok_or("Wire requires power property")?;
        let power = value.parse::<u8>()?;
        if power > 15 || value != &power.to_string() || b.signal.is_none() || b.placement.is_some()
        {
            return Err("Wire requires standard placement, signal and power 0..15".into());
        }
        for power in 0..=15 {
            let mut properties = identity.properties.clone();
            properties.insert("power".into(), power.to_string());
            let other = registry.get(registry.resolve_state(&identity.block, &properties)?);
            if other.behavior != b.behavior || other.signal != b.signal {
                return Err("Wire variants must share behavior and signal settings".into());
            }
        }
        Ok(())
    }

    fn on_event(
        &self,
        c: &EventContext<'_>,
        event: BlockEvent,
        commands: &mut Vec<Command>,
    ) -> Result<()> {
        let b = c.registry.get(c.state);
        let signal = b.signal.as_ref().ok_or("Signal component missing")?;
        if event != BlockEvent::ScheduledTick {
            commands.push(Command::Schedule {
                position: c.position,
                expected: c.state,
                delay: signal.delay,
            });
            return Ok(());
        }
        let power = neighbors(c.position)
            .filter_map(|p| c.blocks.get(&p))
            .map(|id| strength(c.registry.get(*id)).saturating_sub(signal.attenuation))
            .max()
            .unwrap_or(0)
            .max(signal.source);
        let identity = b.state.as_ref().ok_or("Wire identity missing")?;
        let mut properties = identity.properties.clone();
        properties.insert("power".into(), power.to_string());
        let state = c.registry.resolve_state(&identity.block, &properties)?;
        if state != c.state {
            commands.push(Command::Set {
                position: c.position,
                expected: c.state,
                state,
            });
        }
        Ok(())
    }
}
