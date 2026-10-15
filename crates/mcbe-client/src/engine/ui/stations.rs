//! Station adapters connect shared UI slot transactions to persistent world machines.
use crate::{
    engine_inventory::{Button, Slot},
    scene::{Client, Result},
};
pub fn click(
    client: &mut Client,
    p: [i32; 3],
    i: usize,
    button: Button,
    shift: bool,
) -> Result<()> {
    if i >= 3 {
        return Err("Invalid machine slot".into());
    }
    let state = client.scene.engine.as_mut().ok_or("Engine world missing")?;
    let id = state.blocks.get(&p).ok_or("Station no longer loaded")?;
    if state.registry.get(*id).interaction
        != Some(crate::engine::blocks::components::Interaction::Furnace)
    {
        return Err("Furnace no longer present".into());
    }
    state.machines.ensure(p)?;
    state.machines.refresh(p, &state.smelting);
    let f = state.machines.furnaces.get_mut(&p).unwrap();
    let changed = state
        .player
        .click_external(&mut f.slots[i], button, shift, i == 2);
    if changed {
        state.machines.revision = state.machines.revision.wrapping_add(1);
    }
    state.machines.refresh(p, &state.smelting);
    state.machines.wake(p, &state.smelting);
    Ok(())
}
pub fn quick_from_player(client: &mut Client, p: [i32; 3], slot: Slot) -> Result<()> {
    let state = client.scene.engine.as_mut().ok_or("Engine world missing")?;
    let id = state.blocks.get(&p).ok_or("Station no longer loaded")?;
    if state.registry.get(*id).interaction
        != Some(crate::engine::blocks::components::Interaction::Furnace)
    {
        return Err("Furnace no longer present".into());
    }
    state.machines.ensure(p)?;
    state.machines.refresh(p, &state.smelting);
    let Some(stack) = state.player.get(slot)? else {
        return Ok(());
    };
    let target = if state.smelting.get(stack.id).is_some() {
        Some(0)
    } else if state.smelting.fuel(stack.id) > 0 {
        Some(1)
    } else {
        None
    };
    let Some(i) = target else {
        state.player.quick_move(slot)?;
        return Ok(());
    };
    let f = state.machines.furnaces.get_mut(&p).unwrap();
    if state.player.quick_to_external(slot, &mut f.slots[i])? {
        state.machines.revision = state.machines.revision.wrapping_add(1);
    }
    state.machines.refresh(p, &state.smelting);
    state.machines.wake(p, &state.smelting);
    Ok(())
}
