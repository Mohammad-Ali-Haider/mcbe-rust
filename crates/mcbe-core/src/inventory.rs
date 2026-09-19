//! Pristine container operations measured against the version-bound native API.
//! This does not implement inventory GUI gestures, metadata or container tick rules.
use crate::items::{ItemStack, Registry, StackError};
use serde_json::{Value, json};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InventoryError {
    ArgumentOutOfBounds,
    InvalidSlot,
    InvalidContainer,
    InvalidCapacity,
    DifferentRegistry,
    Stack(StackError),
}
impl InventoryError {
    pub fn category(&self) -> &'static str {
        match self {
            Self::ArgumentOutOfBounds => "ArgumentOutOfBoundsError",
            Self::InvalidSlot => "Error",
            Self::Stack(error) => error.category(),
            Self::InvalidContainer => "InvalidContainer",
            Self::InvalidCapacity => "InvalidCapacity",
            Self::DifferentRegistry => "DifferentRegistry",
        }
    }
}
impl fmt::Display for InventoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {:?}", self.category(), self)
    }
}
impl std::error::Error for InventoryError {}
impl From<StackError> for InventoryError {
    fn from(error: StackError) -> Self {
        Self::Stack(error)
    }
}
type Result<T> = std::result::Result<T, InventoryError>;
fn numeric_slot(value: f64) -> Result<usize> {
    if !value.is_finite() {
        return Err(InventoryError::Stack(StackError::UnsupportedNumber));
    }
    let integer = value.trunc();
    if !(0.0..=f64::from(i32::MAX)).contains(&integer) {
        return Err(InventoryError::ArgumentOutOfBounds);
    }
    Ok(integer as usize)
}
#[derive(Clone)]
pub struct Container {
    slots: Vec<Option<ItemStack>>,
    clamp_assignment: bool,
}
#[derive(Clone, Copy, Debug)]
pub enum ContainerKind {
    Barrel,
    Hopper,
    Dispenser,
}
impl ContainerKind {
    pub fn size(self) -> usize {
        match self {
            Self::Barrel => 27,
            Self::Hopper => 5,
            Self::Dispenser => 9,
        }
    }
}
impl Container {
    pub fn size(&self) -> usize {
        self.slots.len()
    }
    pub fn empty_slots_count(&self) -> usize {
        self.slots.iter().filter(|s| s.is_none()).count()
    }
    pub fn observation(&self) -> Value {
        json!({"size":self.size(),"emptySlotsCount":self.empty_slots_count(),"slots":self.slots.iter().map(|s|s.as_ref().map(ItemStack::observation)).collect::<Vec<_>>()})
    }
}
#[derive(Clone)]
pub struct Containers {
    containers: Vec<Container>,
    source: String,
}
impl Containers {
    pub fn new(registry: &Registry, sizes: &[usize]) -> Result<Self> {
        if sizes.is_empty() || sizes.len() > 64 || sizes.iter().any(|s| !(1..=1024).contains(s)) {
            return Err(InventoryError::InvalidCapacity);
        }
        Ok(Self {
            source: registry.source_id().to_owned(),
            containers: sizes
                .iter()
                .map(|size| Container {
                    slots: vec![None; *size],
                    clamp_assignment: false,
                })
                .collect(),
        })
    }
    pub fn with_kinds(registry: &Registry, kinds: &[ContainerKind]) -> Result<Self> {
        let mut result = Self::new(
            registry,
            &kinds.iter().map(|k| k.size()).collect::<Vec<_>>(),
        )?;
        for (container, kind) in result.containers.iter_mut().zip(kinds) {
            container.clamp_assignment = matches!(kind, ContainerKind::Dispenser);
        }
        Ok(result)
    }
    pub fn source_id(&self) -> &str {
        &self.source
    }
    pub fn container(&self, c: usize) -> Result<&Container> {
        self.containers
            .get(c)
            .ok_or(InventoryError::InvalidContainer)
    }
    pub fn observation(&self) -> Value {
        json!(
            self.containers
                .iter()
                .map(Container::observation)
                .collect::<Vec<_>>()
        )
    }
    fn checked(&self, c: usize, index: usize) -> Result<usize> {
        if index >= self.container(c)?.size() {
            Err(InventoryError::InvalidSlot)
        } else {
            Ok(index)
        }
    }
    fn check_stack(&self, stack: &ItemStack) -> Result<()> {
        if stack.source_id() != self.source {
            Err(InventoryError::DifferentRegistry)
        } else {
            Ok(())
        }
    }
    pub fn get(&self, c: usize, slot: f64) -> Result<Option<ItemStack>> {
        let index = self.checked(c, numeric_slot(slot)?)?;
        Ok(self.containers[c].slots[index].clone())
    }
    pub fn set(&mut self, c: usize, slot: f64, stack: Option<&ItemStack>) -> Result<()> {
        let index = self.checked(c, numeric_slot(slot)?)?;
        if let Some(stack) = stack {
            self.check_stack(stack)?;
        }
        // Assignment copies the pristine stack, including constructor oversize.
        self.containers[c].slots[index] = stack
            .filter(|s| !s.definition().type_id.is_empty() && s.amount() > 0)
            .cloned();
        if self.containers[c].clamp_assignment
            && let Some(stack) = &mut self.containers[c].slots[index]
            && stack.amount() > 64
        {
            stack.set_amount(f64::from(stack.amount()))?;
        }
        Ok(())
    }
    pub fn clear(&mut self, c: usize) -> Result<()> {
        self.container(c)?;
        self.containers[c].slots.fill(None);
        Ok(())
    }
    pub fn add(&mut self, c: usize, stack: &ItemStack) -> Result<Option<ItemStack>> {
        self.insert(c, stack, true)
    }
    fn insert(
        &mut self,
        c: usize,
        stack: &ItemStack,
        clamp_input: bool,
    ) -> Result<Option<ItemStack>> {
        self.container(c)?;
        self.check_stack(stack)?;
        if stack.definition().type_id.is_empty() || stack.amount() == 0 {
            return Ok(None);
        }
        let mut remainder = stack.clone();
        // Native addItem clamps its local copy before searching; input is untouched.
        if clamp_input {
            remainder.set_amount(f64::from(stack.amount()))?;
        }
        for slot in &mut self.containers[c].slots {
            let Some(existing) = slot else {
                *slot = Some(remainder);
                return Ok(None);
            };
            if existing.is_stackable_with(&remainder)?
                && existing.amount() < existing.definition().max_amount
            {
                let accepted =
                    (existing.definition().max_amount - existing.amount()).min(remainder.amount());
                existing.set_amount(f64::from(existing.amount() + accepted))?;
                if accepted == remainder.amount() {
                    return Ok(None);
                }
                remainder.set_amount(f64::from(remainder.amount() - accepted))?;
            }
        }
        Ok(Some(remainder))
    }
    fn pair(&self, c: usize, slot: f64, to: usize, destination: f64) -> Result<(usize, usize)> {
        // Binding-level numeric validation precedes either container's slot check.
        let source = numeric_slot(slot)?;
        let target = numeric_slot(destination)?;
        Ok((self.checked(c, source)?, self.checked(to, target)?))
    }
    pub fn move_item(&mut self, c: usize, slot: f64, to: usize, destination: f64) -> Result<()> {
        let (source, target) = self.pair(c, slot, to, destination)?;
        if c == to && source == target {
            return Ok(());
        }
        let value = self.containers[c].slots[source].take();
        // Native moveItem overwrites, including when the source slot is empty.
        self.set(to, target as f64, value.as_ref())?;
        Ok(())
    }
    pub fn swap_items(&mut self, c: usize, slot: f64, to: usize, destination: f64) -> Result<()> {
        let (source, target) = self.pair(c, slot, to, destination)?;
        if c == to && source == target {
            return Ok(());
        }
        let value = self.containers[c].slots[source].take();
        let other = self.containers[to].slots[target].take();
        self.set(c, source as f64, other.as_ref())?;
        self.set(to, target as f64, value.as_ref())?;
        Ok(())
    }
    pub fn transfer_item(&mut self, c: usize, slot: f64, to: usize) -> Result<Option<ItemStack>> {
        let source = self.checked(c, numeric_slot(slot)?)?;
        self.container(to)?;
        let Some(value) = self.containers[c].slots[source].take() else {
            return Ok(None);
        };
        match self.insert(to, &value, c != to) {
            Ok(remainder) => {
                if remainder.is_some() {
                    self.containers[c].slots[source] = remainder.clone();
                }
                Ok(remainder)
            }
            Err(error) => {
                self.containers[c].slots[source] = Some(value);
                Err(error)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn registry() -> Registry {
        Registry::from_bytes(&serde_json::to_vec(&json!({"complete":true,"errors":[],"completion_record":{"kind":"done","errors":0,"items":2,"blocks":0},"items":[
            {"id":"minecraft:stone","typeId":"minecraft:stone","maxAmount":64,"isStackable":true,"tags":[],"components":[]},
            {"id":"minecraft:egg","typeId":"minecraft:egg","maxAmount":16,"isStackable":true,"tags":[],"components":[]}],"blocks":[]})).unwrap()).unwrap()
    }
    #[test]
    fn native_copy_oversize_and_insertion_order() {
        let r = registry();
        let mut c = Containers::new(&r, &[5]).unwrap();
        let mut input = r.create_stack("egg", 255.).unwrap();
        c.set(0, 2., Some(&input)).unwrap();
        input.set_amount(1.).unwrap();
        assert_eq!(c.get(0, 2.).unwrap().unwrap().amount(), 255);
        let big = r.create_stack("egg", 255.).unwrap();
        assert!(c.add(0, &big).unwrap().is_none());
        assert_eq!(big.amount(), 255);
        assert_eq!(c.get(0, 0.).unwrap().unwrap().amount(), 16);
        let mut copy = c.get(0, 0.).unwrap().unwrap();
        copy.set_amount(2.).unwrap();
        assert_eq!(c.get(0, 0.).unwrap().unwrap().amount(), 16);
    }
    #[test]
    fn partial_transfer_self_alias_and_overwrite() {
        let r = registry();
        let mut c = Containers::new(&r, &[2, 1]).unwrap();
        c.set(0, 0., Some(&r.create_stack("stone", 16.).unwrap()))
            .unwrap();
        c.set(1, 0., Some(&r.create_stack("stone", 63.).unwrap()))
            .unwrap();
        assert_eq!(c.transfer_item(0, 0., 1).unwrap().unwrap().amount(), 15);
        assert_eq!(c.get(0, 0.).unwrap().unwrap().amount(), 15);
        c.transfer_item(0, 0., 0).unwrap();
        assert_eq!(c.get(0, 0.).unwrap().unwrap().amount(), 15);
        c.move_item(0, 1., 1, 0.).unwrap();
        assert!(c.get(1, 0.).unwrap().is_none());
    }
    #[test]
    fn native_self_transfer_keeps_raw_count_until_a_merge_reduces_it() {
        let r = registry();
        let mut c = Containers::with_kinds(&r, &[ContainerKind::Hopper]).unwrap();
        c.set(0, 4., Some(&r.create_stack("egg", 64.).unwrap()))
            .unwrap();
        c.transfer_item(0, 4., 0).unwrap();
        assert_eq!(c.get(0, 0.).unwrap().unwrap().amount(), 64);
        c.clear(0).unwrap();
        c.set(0, 0., Some(&r.create_stack("egg", 15.).unwrap()))
            .unwrap();
        c.set(0, 4., Some(&r.create_stack("egg", 64.).unwrap()))
            .unwrap();
        c.transfer_item(0, 4., 0).unwrap();
        assert_eq!(c.get(0, 0.).unwrap().unwrap().amount(), 16);
        assert_eq!(c.get(0, 1.).unwrap().unwrap().amount(), 16);
    }
    #[test]
    fn native_dispenser_limits_assignment_and_move_independently_of_barrel() {
        let r = registry();
        let mut c =
            Containers::with_kinds(&r, &[ContainerKind::Barrel, ContainerKind::Dispenser]).unwrap();
        c.set(0, 0., Some(&r.create_stack("egg", 255.).unwrap()))
            .unwrap();
        assert_eq!(c.get(0, 0.).unwrap().unwrap().amount(), 255);
        c.move_item(0, 0., 1, 0.).unwrap();
        assert_eq!(c.get(1, 0.).unwrap().unwrap().amount(), 16);
    }
    #[test]
    fn validation_is_atomic_and_numeric_checks_precede_slot_checks() {
        let r = registry();
        let mut c = Containers::new(&r, &[2, 1]).unwrap();
        c.set(0, -0.9, Some(&r.create_stack("stone", 8.).unwrap()))
            .unwrap();
        let before = c.observation();
        assert_eq!(
            c.move_item(0, 2., 1, -1.).unwrap_err(),
            InventoryError::ArgumentOutOfBounds
        );
        assert_eq!(
            c.swap_items(0, 0., 1, 1.).unwrap_err(),
            InventoryError::InvalidSlot
        );
        assert_eq!(c.observation(), before);
    }
}
