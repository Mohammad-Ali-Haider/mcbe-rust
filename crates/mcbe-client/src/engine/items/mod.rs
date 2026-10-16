//! Typed owned item data and fluid-container interactions.
pub(crate) mod bucket;
use crate::engine::blocks::components::FluidKind;
use crate::{
    engine_blocks::{BlockId, Registry},
    engine_inventory::Stack,
    scene::Result,
};
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ItemData {
    Fluid { kind: FluidKind, amount: u16 },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FluidContainer {
    pub kind: FluidKind,
    pub capacity: u16,
}
impl Stack {
    pub fn limit(self) -> u8 {
        if self.data.is_some() { 1 } else { 64 }
    }
    pub fn compatible(self, other: Self) -> bool {
        self.id == other.id && self.data == other.data
    }
    pub fn validate_item(self, r: &Registry) -> Result<()> {
        if self.count == 0 || self.count > self.limit() || usize::from(self.id) >= r.blocks.len() {
            return Err("Invalid item stack".into());
        }
        match (r.get(self.id).container, self.data) {
            (None, None) => Ok(()),
            (Some(c), Some(ItemData::Fluid { kind, amount }))
                if kind == c.kind && amount <= c.capacity =>
            {
                Ok(())
            }
            _ => Err("Item contents do not match registered container".into()),
        }
    }
}
impl Registry {
    pub fn catalog_stack(&self, id: BlockId) -> Stack {
        match self.get(id).container {
            Some(c) => Stack {
                id,
                count: 1,
                data: Some(ItemData::Fluid {
                    kind: c.kind,
                    amount: 0,
                }),
            },
            None => Stack {
                id,
                count: 64,
                data: None,
            },
        }
    }
}
impl crate::engine_inventory::Inventory {
    pub fn validate_items(&self, r: &Registry) -> Result<()> {
        self.validate(r.blocks.len())?;
        for s in self
            .slots
            .iter()
            .chain(&self.crafting)
            .chain(&self.table)
            .chain(std::iter::once(&self.cursor))
            .chain(std::iter::once(&self.offhand))
            .chain(self.containers.iter().flatten())
            .flatten()
        {
            s.validate_item(r)?;
        }
        Ok(())
    }
    pub fn assign_item(&mut self, stack: Stack, index: usize) -> Result<()> {
        if index >= self.profile.hotbar_slots {
            return Err("Hotbar index must be 0..8".into());
        }
        self.slots[index] = Some(stack);
        self.revision = self.revision.wrapping_add(1);
        Ok(())
    }
    pub fn catalog_item(&mut self, stack: Stack) {
        self.cursor = Some(stack);
        self.revision = self.revision.wrapping_add(1);
    }
    pub fn add_item(&mut self, stack: Stack) -> bool {
        let mut next = self.clone();
        if next
            .add(stack, &(0..self.slots.len()).collect::<Vec<_>>())
            .is_some()
        {
            return false;
        }
        next.revision = next.revision.wrapping_add(1);
        *self = next;
        true
    }
}

pub fn mesh(s: &crate::engine::WorldState, stack: Stack) -> Vec<crate::engine_mesh::Vertex> {
    let mut out = crate::engine_mesh::item(s, stack.id);
    if let (Some(c), Some(ItemData::Fluid { kind, amount })) =
        (s.registry.get(stack.id).container, stack.data)
        && amount > 0
        && let Some(id) = s.registry.blocks.iter().position(|b| b.fluid == Some(kind))
    {
        let h = 0.16 + 0.5 * amount as f32 / c.capacity as f32;
        crate::engine_mesh::lod_surface(
            &mut out,
            [0, 0, 0],
            0,
            id as u16,
            [0.27, h, 0.27, 0.73, h, 0.73],
            s.geometry.flags(id as u16),
        );
    }
    out
}

#[cfg(test)]
mod offhand_tests;
#[cfg(test)]
mod tests;
