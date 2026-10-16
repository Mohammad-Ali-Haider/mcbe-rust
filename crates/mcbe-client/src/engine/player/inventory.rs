//! Engine-owned player slots and cursor transactions. Creative placement is unlimited.
use crate::{engine_blocks::BlockId, scene::Result};
mod containers;
pub(crate) mod profile;
use profile::Profile;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stack {
    pub data: Option<crate::engine::items::ItemData>,
    pub id: BlockId,
    pub count: u8,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    Offhand,
    Container { bank: usize, index: usize },
    Player(usize),
    Craft(usize),
    Table(usize),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Left,
    Right,
}
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Hand {
    #[default]
    Main,
    Off,
}
#[derive(Clone, Debug)]
pub struct Inventory {
    pub profile: Profile,
    pub containers: Vec<Vec<Option<Stack>>>,
    filters: Vec<Vec<BlockId>>,
    pub slots: Vec<Option<Stack>>,
    pub offhand: Option<Stack>,
    pub crafting: Vec<Option<Stack>>,
    pub table: Vec<Option<Stack>>,
    pub cursor: Option<Stack>,
    pub active: usize,
    pub revision: u64,
}
impl Inventory {
    pub fn empty() -> Self {
        Self {
            profile: Profile::default(),
            containers: vec![],
            filters: vec![],
            slots: vec![None; 36],
            offhand: None,
            crafting: vec![None; 4],
            table: vec![None; 9],
            cursor: None,
            active: 0,
            revision: 0,
        }
    }
    pub fn starter(count: usize, selected: BlockId) -> Self {
        let mut p = Self::empty();
        for i in 0..count.min(9) {
            p.slots[i] = Some(Stack {
                data: None,
                id: i as BlockId,
                count: 64,
            });
        }
        if usize::from(selected) < 9 {
            p.active = usize::from(selected);
        } else {
            p.slots[0] = Some(Stack {
                data: None,
                id: selected,
                count: 64,
            });
        }
        p
    }
    pub fn hand(&self, hand: Hand) -> Option<Stack> {
        match hand {
            Hand::Main => self.slots.get(self.active).copied().flatten(),
            Hand::Off => self.offhand,
        }
    }
    pub fn hand_slot(&self, hand: Hand) -> Slot {
        match hand {
            Hand::Main => Slot::Player(self.active),
            Hand::Off => Slot::Offhand,
        }
    }
    pub fn use_hand(&self, force_offhand: bool) -> Hand {
        if force_offhand || self.hand(Hand::Main).is_none() {
            Hand::Off
        } else {
            Hand::Main
        }
    }
    pub fn swap_hands(&mut self) -> Result<bool> {
        if self.active >= self.profile.hotbar_slots {
            return Err("Hotbar index outside the configured range".into());
        }
        if self.slots[self.active] == self.offhand {
            return Ok(false);
        }
        std::mem::swap(&mut self.slots[self.active], &mut self.offhand);
        self.revision = self.revision.wrapping_add(1);
        Ok(true)
    }
    /// All-or-nothing merge into player slots, respecting each item's stack limit.
    pub fn receive(&mut self, stack: Stack, count: u16) -> Result<()> {
        if count == 0 || usize::from(count) > self.slots.len() * 64 {
            return Err("Count exceeds the configured inventory capacity".into());
        }
        let mut next = self.clone();
        let order: Vec<_> = (0..self.slots.len()).collect();
        let mut remaining = count;
        while remaining > 0 {
            let n = remaining.min(u16::from(stack.limit())) as u8;
            if next.add(Stack { count: n, ..stack }, &order).is_some() {
                return Err("Not enough inventory space; no items were added".into());
            }
            remaining -= u16::from(n);
        }
        next.revision = next.revision.wrapping_add(1);
        *self = next;
        Ok(())
    }
    pub fn selected(&self) -> Option<BlockId> {
        self.hand(Hand::Main).map(|s| s.id)
    }
    pub fn validate(&self, definitions: usize) -> Result<()> {
        self.profile.validate()?;
        if self.filters.len() != self.containers.len()
            || self.containers.iter().enumerate().any(|(bank, slots)| {
                slots
                    .iter()
                    .flatten()
                    .any(|s| !self.accepts(Slot::Container { bank, index: 0 }, *s))
            })
        {
            return Err("Stored items violate container filters".into());
        }
        if self.slots.len() != self.profile.player_slots
            || self.crafting.len() != self.profile.craft_side.pow(2)
            || self.table.len() != self.profile.table_side.pow(2)
            || self.containers.len() != self.profile.containers.len()
            || self
                .containers
                .iter()
                .zip(&self.profile.containers)
                .any(|(slots, c)| slots.len() != c.slots)
            || self.active >= self.profile.hotbar_slots
            || self
                .slots
                .iter()
                .chain(&self.crafting)
                .chain(&self.table)
                .chain(std::iter::once(&self.cursor))
                .chain(std::iter::once(&self.offhand))
                .chain(self.containers.iter().flatten())
                .flatten()
                .any(|s| s.count == 0 || s.count > s.limit() || usize::from(s.id) >= definitions)
        {
            return Err("Invalid player inventory or cursor stack".into());
        }
        Ok(())
    }
    pub fn changed_slots(&self, base: &Self) -> usize {
        self.slots
            .iter()
            .zip(&base.slots)
            .filter(|(a, b)| a != b)
            .count()
            + self
                .crafting
                .iter()
                .zip(&base.crafting)
                .filter(|(a, b)| a != b)
                .count()
            + self
                .table
                .iter()
                .zip(&base.table)
                .filter(|(a, b)| a != b)
                .count()
            + usize::from(self.cursor != base.cursor)
            + usize::from(self.offhand != base.offhand)
            + self.slots.len().abs_diff(base.slots.len())
            + self.crafting.len().abs_diff(base.crafting.len())
            + self.table.len().abs_diff(base.table.len())
            + usize::from(self.profile != base.profile)
            + self
                .containers
                .iter()
                .enumerate()
                .map(|(i, slots)| match base.containers.get(i) {
                    Some(old) => {
                        slots.iter().zip(old).filter(|(a, b)| a != b).count()
                            + slots.len().abs_diff(old.len())
                    }
                    None => slots.len(),
                })
                .sum::<usize>()
            + base
                .containers
                .iter()
                .skip(self.containers.len())
                .map(Vec::len)
                .sum::<usize>()
    }
    pub fn total(&self) -> usize {
        self.slots
            .iter()
            .chain(&self.crafting)
            .chain(&self.table)
            .chain(std::iter::once(&self.cursor))
            .chain(std::iter::once(&self.offhand))
            .chain(self.containers.iter().flatten())
            .flatten()
            .map(|s| usize::from(s.count))
            .sum()
    }
    pub fn get(&self, slot: Slot) -> Result<Option<Stack>> {
        match slot {
            Slot::Container { bank, index } => self.containers.get(bank).and_then(|s| s.get(index)),
            Slot::Offhand => Some(&self.offhand),
            Slot::Player(i) => self.slots.get(i),
            Slot::Craft(i) => self.crafting.get(i),
            Slot::Table(i) => self.table.get(i),
        }
        .copied()
        .ok_or_else(|| "Invalid inventory slot".into())
    }
    pub(crate) fn set(&mut self, slot: Slot, value: Option<Stack>) -> Result<()> {
        if let Some(stack) = value
            && !self.accepts(slot, stack)
        {
            return Err("Item is not accepted by this container".into());
        }
        let destination = match slot {
            Slot::Container { bank, index } => {
                self.containers.get_mut(bank).and_then(|s| s.get_mut(index))
            }
            Slot::Offhand => Some(&mut self.offhand),
            Slot::Player(i) => self.slots.get_mut(i),
            Slot::Craft(i) => self.crafting.get_mut(i),
            Slot::Table(i) => self.table.get_mut(i),
        }
        .ok_or("Invalid inventory slot")?;
        *destination = value;
        Ok(())
    }
    pub fn select(&mut self, index: usize) -> Result<()> {
        if index >= self.profile.hotbar_slots {
            return Err("Hotbar index outside the configured range".into());
        }
        if self.active != index {
            self.active = index;
            self.revision = self.revision.wrapping_add(1);
        }
        Ok(())
    }
    pub fn cycle(&mut self, delta: i32) {
        let next =
            (self.active as i32 + delta).rem_euclid(self.profile.hotbar_slots as i32) as usize;
        let _ = self.select(next);
    }
    pub fn assign(&mut self, id: BlockId, hotbar: usize) -> Result<()> {
        if hotbar >= self.profile.hotbar_slots {
            return Err("Hotbar index outside the configured range".into());
        }
        self.slots[hotbar] = Some(Stack {
            data: None,
            id,
            count: 64,
        });
        self.revision = self.revision.wrapping_add(1);
        Ok(())
    }
    pub fn discard_cursor(&mut self) {
        if self.cursor.take().is_some() {
            self.revision = self.revision.wrapping_add(1);
        }
    }
    pub fn swap_hotbar(&mut self, slot: Slot, index: usize) -> Result<()> {
        if index >= self.profile.hotbar_slots {
            return Err("Hotbar index outside the configured range".into());
        }
        let value = self.get(slot)?;
        if matches!(slot,Slot::Container{bank,..} if self.profile.containers[bank].output_only) {
            return Ok(());
        }
        let other = self.slots[index];
        self.set(slot, other)?;
        self.slots[index] = value;
        self.revision = self.revision.wrapping_add(1);
        Ok(())
    }
    pub(crate) fn add(&mut self, mut stack: Stack, order: &[usize]) -> Option<Stack> {
        for empty in [false, true] {
            for i in order {
                if let Some(existing) = &mut self.slots[*i] {
                    if !empty && existing.compatible(stack) {
                        let n = (existing.limit() - existing.count).min(stack.count);
                        existing.count += n;
                        stack.count -= n;
                    }
                } else if empty {
                    self.slots[*i] = Some(stack);
                    return None;
                }
                if stack.count == 0 {
                    return None;
                }
            }
        }
        Some(stack)
    }
    pub fn quick_move(&mut self, slot: Slot) -> Result<bool> {
        let Some(stack) = self.get(slot)? else {
            return Ok(false);
        };
        let order: Vec<_> = match slot {
            Slot::Player(i) if i < self.profile.hotbar_slots => {
                (self.profile.hotbar_slots..self.slots.len()).collect()
            }
            Slot::Player(_) => (0..self.profile.hotbar_slots).collect(),
            Slot::Container { .. } | Slot::Offhand | Slot::Craft(_) | Slot::Table(_) => {
                (0..self.slots.len()).collect()
            }
        };
        if let Slot::Container { bank, .. } = slot
            && let Some(target) = self.profile.containers[bank]
                .quick_move_to
                .as_ref()
                .filter(|id| id.as_str() != "player")
        {
            let destination = self
                .profile
                .containers
                .iter()
                .position(|c| &c.id == target)
                .ok_or("Transfer destination missing")?;
            if self.profile.containers[destination].output_only {
                return Ok(false);
            }
            let mut next = self.clone();
            next.set(slot, None)?;
            let remainder = next.insert_container(destination, stack)?;
            next.set(slot, remainder)?;
            if remainder == Some(stack) {
                return Ok(false);
            }
            next.revision = next.revision.wrapping_add(1);
            *self = next;
            return Ok(true);
        }
        let mut next = self.clone();
        next.set(slot, None)?;
        let remainder = next.add(stack, &order);
        next.set(slot, remainder)?;
        if remainder == Some(stack) {
            return Ok(false);
        }
        next.revision = next.revision.wrapping_add(1);
        *self = next;
        Ok(true)
    }
    pub fn click(&mut self, slot: Slot, button: Button, shift: bool) -> Result<bool> {
        if shift {
            return self.quick_move(slot);
        }
        let before = self.get(slot)?;
        let mut value = before;
        let old_cursor = self.cursor;
        if matches!(slot,Slot::Container{bank,..} if self.profile.containers[bank].output_only) {
            self.click_external(&mut value, button, false, true);
        } else {
            transfer(&mut self.cursor, &mut value, button);
        }
        if value.is_some_and(|stack| !self.accepts(slot, stack)) {
            self.cursor = old_cursor;
            return Ok(false);
        }
        self.set(slot, value)?;
        let changed = before != value || old_cursor != self.cursor;
        if changed {
            self.revision = self.revision.wrapping_add(1);
        }
        Ok(changed)
    }
    pub fn click_external(
        &mut self,
        value: &mut Option<Stack>,
        button: Button,
        shift: bool,
        output: bool,
    ) -> bool {
        let old = *value;
        let cursor = self.cursor;
        if shift {
            if let Some(s) = *value {
                *value = self.add(s, &(0..self.slots.len()).collect::<Vec<_>>());
            }
        } else if output {
            if let Some(mut s) = *value {
                let capacity = self.cursor.map_or(s.limit(), |c| {
                    if c.compatible(s) {
                        c.limit() - c.count
                    } else {
                        0
                    }
                });
                let amount = capacity.min(if button == Button::Right {
                    if self.cursor.is_none() {
                        s.count.div_ceil(2)
                    } else {
                        1
                    }
                } else {
                    s.count
                });
                if amount > 0 {
                    self.cursor = Some(Stack {
                        data: s.data,
                        id: s.id,
                        count: self.cursor.map_or(0, |c| c.count) + amount,
                    });
                    s.count -= amount;
                    *value = (s.count > 0).then_some(s);
                }
            }
        } else {
            transfer(&mut self.cursor, value, button);
        }
        let changed = old != *value || cursor != self.cursor;
        if changed {
            self.revision = self.revision.wrapping_add(1);
        }
        changed
    }
    pub fn quick_to_external(&mut self, slot: Slot, value: &mut Option<Stack>) -> Result<bool> {
        let Some(mut s) = self.get(slot)? else {
            return Ok(false);
        };
        let capacity = value.map_or(64, |v| if v.id == s.id { 64 - v.count } else { 0 });
        let n = capacity.min(s.count);
        if n == 0 {
            return Ok(false);
        }
        *value = Some(Stack {
            data: None,
            id: s.id,
            count: value.map_or(0, |v| v.count) + n,
        });
        s.count -= n;
        self.set(slot, (s.count > 0).then_some(s))?;
        self.revision = self.revision.wrapping_add(1);
        Ok(true)
    }
    pub fn settle_cursor(&mut self) -> bool {
        let Some(held) = self.cursor else {
            return true;
        };
        let order: Vec<_> = (0..self.slots.len()).collect();
        self.cursor = self.add(held, &order);
        self.revision = self.revision.wrapping_add(1);
        self.cursor.is_none()
    }
    pub fn close_inventory(&mut self) -> bool {
        let mut next = self.clone();
        if !next.settle_cursor() {
            return false;
        }
        let order: Vec<_> = (0..self.slots.len()).collect();
        for i in 0..self.crafting.len() {
            if let Some(stack) = next.crafting[i].take()
                && next.add(stack, &order).is_some()
            {
                return false;
            }
        }
        for i in 0..self.table.len() {
            if let Some(stack) = next.table[i].take()
                && next.add(stack, &order).is_some()
            {
                return false;
            }
        }
        if self.changed_slots(&next) != 0 {
            next.revision = self.revision.wrapping_add(1);
            *self = next;
        }
        true
    }
    pub fn accept_grid_output(
        &mut self,
        result: Stack,
        consumed: &[usize],
        all: bool,
        table: bool,
    ) -> bool {
        if result.count == 0 || result.count > result.limit() {
            return false;
        }
        if consumed.is_empty()
            || consumed.iter().any(|i| {
                *i >= if table {
                    self.table.len()
                } else {
                    self.crafting.len()
                }
            })
            || consumed
                .iter()
                .copied()
                .collect::<std::collections::HashSet<_>>()
                .len()
                != consumed.len()
        {
            return false;
        }
        let mut next = self.clone();
        if all {
            let order: Vec<_> = (0..self.slots.len()).collect();
            if next.add(result, &order).is_some() {
                return false;
            }
        } else {
            match &mut next.cursor {
                None => next.cursor = Some(result),
                Some(s)
                    if s.compatible(result)
                        && u16::from(s.count) + u16::from(result.count) <= u16::from(s.limit()) =>
                {
                    s.count += result.count
                }
                _ => return false,
            }
        }
        for i in consumed {
            let grid = if table {
                &mut next.table[..]
            } else {
                &mut next.crafting[..]
            };
            let Some(mut stack) = grid[*i] else {
                return false;
            };
            stack.count -= 1;
            grid[*i] = (stack.count > 0).then_some(stack);
        }
        next.revision = next.revision.wrapping_add(1);
        *self = next;
        true
    }
}
fn transfer(cursor: &mut Option<Stack>, value: &mut Option<Stack>, button: Button) {
    match (button, *cursor, *value) {
        (Button::Left, None, _) => {
            *cursor = *value;
            *value = None;
        }
        (Button::Right, None, Some(mut s)) => {
            let n = s.count.div_ceil(2);
            *cursor = Some(Stack { count: n, ..s });
            s.count -= n;
            *value = (s.count > 0).then_some(s);
        }
        (_, Some(mut held), Some(mut s)) if held.compatible(s) => {
            let n = if button == Button::Right {
                1.min(s.limit() - s.count)
            } else {
                held.count.min(s.limit() - s.count)
            };
            s.count += n;
            held.count -= n;
            *value = Some(s);
            *cursor = (held.count > 0).then_some(held);
        }
        (Button::Right, Some(mut held), None) => {
            *value = Some(Stack { count: 1, ..held });
            held.count -= 1;
            *cursor = (held.count > 0).then_some(held);
        }
        (_, Some(held), _) => {
            *cursor = *value;
            *value = Some(held);
        }
        _ => {}
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn table_inputs_return_atomically_and_external_outputs_only_withdraw() {
        let mut p = Inventory::empty();
        p.table[8] = Some(Stack {
            data: None,
            id: 1,
            count: 64,
        });
        let total = p.total();
        assert!(p.close_inventory());
        assert_eq!(p.total(), total);
        assert!(p.table.iter().all(Option::is_none));
        p.slots = vec![
            Some(Stack {
                data: None,
                id: 0,
                count: 64,
            });
            36
        ];
        p.table[0] = Some(Stack {
            data: None,
            id: 1,
            count: 3,
        });
        let old = p.clone();
        assert!(!p.close_inventory());
        assert_eq!(p.changed_slots(&old), 0);
        let mut output = Some(Stack {
            data: None,
            id: 2,
            count: 10,
        });
        p.cursor = Some(Stack {
            data: None,
            id: 1,
            count: 4,
        });
        assert!(!p.click_external(&mut output, Button::Left, false, true));
        assert_eq!(output.unwrap().count, 10);
        p.cursor = Some(Stack {
            data: None,
            id: 2,
            count: 60,
        });
        assert!(p.click_external(&mut output, Button::Left, false, true));
        assert_eq!(p.cursor.unwrap().count, 64);
        assert_eq!(output.unwrap().count, 6);
        assert!(!p.click_external(&mut output, Button::Left, true, true));
        assert_eq!(output.unwrap().count, 6);
        p.slots[2] = None;
        assert!(p.click_external(&mut output, Button::Left, true, true));
        assert!(output.is_none());
        assert_eq!(p.slots[2].unwrap().count, 6);
    }
    #[test]
    fn closing_returns_inputs_and_is_atomic_when_full() {
        let mut p = Inventory::starter(15, 3);
        p.cursor = Some(Stack {
            data: None,
            id: 10,
            count: 63,
        });
        p.crafting[2] = Some(Stack {
            data: None,
            id: 10,
            count: 2,
        });
        let total = p.total();
        assert!(p.close_inventory());
        assert!(p.cursor.is_none());
        assert_eq!(p.crafting, [None; 4]);
        assert_eq!(p.total(), total);
        p.slots = vec![
            Some(Stack {
                data: None,
                id: 0,
                count: 64,
            });
            36
        ];
        p.slots[35] = None;
        p.cursor = Some(Stack {
            data: None,
            id: 1,
            count: 64,
        });
        p.crafting[0] = Some(Stack {
            data: None,
            id: 2,
            count: 1,
        });
        let before = p.clone();
        assert!(!p.close_inventory());
        assert_eq!(p.changed_slots(&before), 0);
        assert_eq!(p.revision, before.revision);
    }
    #[test]
    fn split_merge_swap_and_shift_preserve_items() {
        let mut p = Inventory::empty();
        p.slots[0] = Some(Stack {
            data: None,
            id: 1,
            count: 63,
        });
        let total = p.total();
        p.click(Slot::Player(0), Button::Right, false).unwrap();
        assert_eq!(p.cursor.unwrap().count, 32);
        assert_eq!(p.slots[0].unwrap().count, 31);
        p.click(Slot::Player(1), Button::Right, false).unwrap();
        assert_eq!(p.slots[1].unwrap().count, 1);
        p.click(Slot::Player(0), Button::Left, false).unwrap();
        assert_eq!(p.slots[0].unwrap().count, 62);
        assert!(p.cursor.is_none());
        assert_eq!(p.total(), total);
        p.quick_move(Slot::Player(0)).unwrap();
        assert!(p.slots[0].is_none());
        assert_eq!(p.slots[9].unwrap().count, 62);
        assert_eq!(p.total(), total);
        p.swap_hotbar(Slot::Player(9), 3).unwrap();
        assert_eq!(p.slots[3].unwrap().count, 62);
        assert!(p.slots[9].is_none());
        assert_eq!(p.total(), total);
    }
    #[test]
    fn full_inventory_and_invalid_indices_are_atomic() {
        let mut p = Inventory::empty();
        p.slots = vec![
            Some(Stack {
                data: None,
                id: 0,
                count: 64,
            });
            36
        ];
        p.cursor = Some(Stack {
            data: None,
            id: 1,
            count: 64,
        });
        let total = p.total();
        assert!(!p.settle_cursor());
        assert_eq!(p.total(), total);
        assert!(p.click(Slot::Player(36), Button::Left, false).is_err());
        assert_eq!(p.total(), total);
        assert!(!p.quick_move(Slot::Player(0)).unwrap());
        assert_eq!(p.total(), total);
        p.slots[5] = Some(Stack {
            data: None,
            id: 1,
            count: 60,
        });
        p.cursor = Some(Stack {
            data: None,
            id: 1,
            count: 9,
        });
        p.click(Slot::Player(5), Button::Left, false).unwrap();
        assert_eq!(p.slots[5].unwrap().count, 64);
        assert_eq!(p.cursor.unwrap().count, 5);
    }
    #[test]
    fn generated_transaction_sequences_conserve_counts() {
        let mut p = Inventory::starter(15, 3);
        let initial = p.total();
        let mut seed = 38u64;
        for _ in 0..2000 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let i = (seed % 40) as usize;
            let slot = if i < 36 {
                Slot::Player(i)
            } else {
                Slot::Craft(i - 36)
            };
            p.click(
                slot,
                if seed & 64 == 0 {
                    Button::Left
                } else {
                    Button::Right
                },
                seed & 256 != 0,
            )
            .unwrap();
            p.validate(15).unwrap();
            assert_eq!(p.total(), initial);
        }
    }
}
