use super::*;
impl Inventory {
    pub fn stacks_mut(&mut self) -> impl Iterator<Item = &mut Option<Stack>> {
        self.slots
            .iter_mut()
            .chain(&mut self.crafting)
            .chain(&mut self.table)
            .chain(std::iter::once(&mut self.cursor))
            .chain(std::iter::once(&mut self.offhand))
            .chain(self.containers.iter_mut().flatten())
    }
    pub fn rebind(&mut self, r: &crate::engine_blocks::Registry) -> Result<()> {
        self.filters = self.profile.filters(r)?;
        Ok(())
    }
    pub fn accepts(&self, slot: Slot, stack: Stack) -> bool {
        match slot {
            Slot::Container { bank, .. } => self
                .filters
                .get(bank)
                .is_some_and(|ids| ids.is_empty() || ids.contains(&stack.id)),
            _ => true,
        }
    }
    pub fn configure(
        &mut self,
        profile: Profile,
        r: &crate::engine_blocks::Registry,
    ) -> Result<()> {
        let filters = profile.filters(r)?;
        let mut next = self.clone();
        fn resize(slots: &mut Vec<Option<Stack>>, count: usize) -> Result<()> {
            if slots.iter().skip(count).any(Option::is_some) {
                return Err("Shrinking inventory would discard items".into());
            }
            slots.resize(count, None);
            Ok(())
        }
        fn grid(old: &[Option<Stack>], from: usize, to: usize) -> Result<Vec<Option<Stack>>> {
            let mut next = vec![None; to * to];
            for (i, value) in old.iter().copied().enumerate() {
                let (x, y) = (i % from, i / from);
                if x >= to || y >= to {
                    if value.is_some() {
                        return Err("Shrinking crafting grid would discard ingredients".into());
                    }
                } else {
                    next[y * to + x] = value;
                }
            }
            Ok(next)
        }
        resize(&mut next.slots, profile.player_slots)?;
        next.crafting = grid(&self.crafting, self.profile.craft_side, profile.craft_side)?;
        next.table = grid(&self.table, self.profile.table_side, profile.table_side)?;
        for (old, slots) in self.profile.containers.iter().zip(&self.containers) {
            if !profile.containers.iter().any(|c| c.id == old.id)
                && slots.iter().any(Option::is_some)
            {
                return Err("Removing a nonempty container would discard items".into());
            }
        }
        next.containers = profile
            .containers
            .iter()
            .map(|c| {
                let mut slots = self
                    .profile
                    .containers
                    .iter()
                    .position(|old| old.id == c.id)
                    .map_or_else(Vec::new, |i| self.containers[i].clone());
                resize(&mut slots, c.slots)?;
                Ok(slots)
            })
            .collect::<Result<Vec<_>>>()?;
        next.active = next.active.min(profile.hotbar_slots - 1);
        next.profile = profile;
        next.filters = filters;
        for (bank, slots) in next.containers.iter().enumerate() {
            if slots
                .iter()
                .flatten()
                .any(|s| !next.accepts(Slot::Container { bank, index: 0 }, *s))
            {
                return Err("New container filters reject stored items".into());
            }
        }
        if next.profile != self.profile {
            next.revision = next.revision.wrapping_add(1);
        }
        *self = next;
        Ok(())
    }
    /// Insert producer output into a named storage bank, returning any remainder.
    pub fn insert_container(&mut self, bank: usize, mut stack: Stack) -> Result<Option<Stack>> {
        let Some(slots) = self.containers.get(bank) else {
            return Err("Unknown container".into());
        };
        if stack.count == 0 || stack.count > stack.limit() {
            return Err("Invalid container insertion count".into());
        }
        if !self.accepts(Slot::Container { bank, index: 0 }, stack) {
            return Ok(Some(stack));
        }
        let before = slots.clone();
        let slots = &mut self.containers[bank];
        let mut remainder = Some(stack);
        'passes: for empty in [false, true] {
            for value in slots.iter_mut() {
                if let Some(existing) = value {
                    if !empty && existing.compatible(stack) {
                        let n = (existing.limit() - existing.count).min(stack.count);
                        existing.count += n;
                        stack.count -= n;
                    }
                } else if empty {
                    *value = Some(stack);
                    remainder = None;
                    break 'passes;
                }
                if stack.count == 0 {
                    remainder = None;
                    break 'passes;
                } else {
                    remainder = Some(stack);
                }
            }
        }
        if *slots != before {
            self.revision = self.revision.wrapping_add(1);
        }
        Ok(remainder)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine_blocks::Registry;
    fn profile() -> Profile {
        serde_json::from_slice(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../content/base/inventory.json"
        )))
        .unwrap()
    }
    fn stack(id: u16, count: u8) -> Stack {
        Stack {
            id,
            count,
            data: None,
        }
    }
    #[test]
    fn configuration_preserves_named_banks_and_grid_coordinates_and_rejects_loss_atomically() {
        let r = Registry::built_in();
        let mut p = Inventory::empty();
        let mut config = profile();
        p.configure(config.clone(), &r).unwrap();
        p.slots[35] = Some(stack(0, 7));
        p.crafting[3] = Some(stack(1, 9));
        p.insert_container(0, stack(2, 11)).unwrap();
        config.player_slots = 64;
        config.hotbar_slots = 12;
        config.craft_side = 4;
        config.containers.swap(0, 1);
        p.configure(config.clone(), &r).unwrap();
        assert_eq!(p.crafting[5], Some(stack(1, 9)));
        assert_eq!(p.containers[1][0], Some(stack(2, 11)));
        assert_eq!(p.total(), 27);
        let before = format!("{p:?}");
        config.player_slots = 20;
        assert!(p.configure(config.clone(), &r).is_err());
        assert_eq!(format!("{p:?}"), before);
        config.player_slots = 64;
        config.craft_side = 1;
        assert!(p.configure(config.clone(), &r).is_err());
        assert_eq!(format!("{p:?}"), before);
        config.craft_side = 4;
        config.containers.pop();
        assert!(p.configure(config, &r).is_err());
        assert_eq!(format!("{p:?}"), before);
    }
    #[test]
    fn filters_cursor_swaps_and_output_banks_obey_one_policy() {
        let r = Registry::built_in();
        let mut config = profile();
        config.containers[0].accepts = vec![r.get(0).id.clone()];
        config.containers[1].output_only = true;
        config.containers[0].quick_move_to = Some(config.containers[1].id.clone());
        let mut p = Inventory::empty();
        p.configure(config.clone(), &r).unwrap();
        assert_eq!(
            p.insert_container(0, stack(1, 2)).unwrap(),
            Some(stack(1, 2))
        );
        p.cursor = Some(stack(1, 5));
        let rev = p.revision;
        assert!(
            !p.click(Slot::Container { bank: 0, index: 0 }, Button::Left, false)
                .unwrap()
        );
        assert_eq!(p.cursor, Some(stack(1, 5)));
        assert_eq!(p.revision, rev);
        p.cursor = None;
        p.insert_container(0, stack(0, 8)).unwrap();
        assert!(!p.quick_move(Slot::Container { bank: 0, index: 0 }).unwrap());
        p.slots[0] = Some(stack(1, 4));
        p.insert_container(1, stack(2, 6)).unwrap();
        let before = format!("{p:?}");
        p.swap_hotbar(Slot::Container { bank: 1, index: 0 }, 0)
            .unwrap();
        assert_eq!(format!("{p:?}"), before);
        p.cursor = Some(stack(1, 5));
        assert!(
            !p.click(Slot::Container { bank: 1, index: 1 }, Button::Left, false)
                .unwrap()
        );
        p.cursor = None;
        assert!(
            p.click(Slot::Container { bank: 1, index: 0 }, Button::Right, false)
                .unwrap()
        );
        assert_eq!(p.cursor, Some(stack(2, 3)));
        assert_eq!(p.containers[1][0], Some(stack(2, 3)));
        let before = format!("{p:?}");
        config.containers[0].accepts = vec![r.get(1).id.clone()];
        assert!(p.configure(config, &r).is_err());
        assert_eq!(format!("{p:?}"), before);
    }
    #[test]
    fn producer_insertion_merges_reports_remainder_and_invalidates_once() {
        let r = Registry::built_in();
        let mut config = profile();
        config.containers.truncate(1);
        config.containers[0].slots = 1;
        let mut p = Inventory::empty();
        p.configure(config, &r).unwrap();
        let rev = p.revision;
        assert_eq!(p.insert_container(0, stack(0, 60)).unwrap(), None);
        assert_eq!(p.revision, rev + 1);
        assert_eq!(
            p.insert_container(0, stack(0, 10)).unwrap(),
            Some(stack(0, 6))
        );
        assert_eq!(p.containers[0][0], Some(stack(0, 64)));
        assert_eq!(p.revision, rev + 2);
        assert_eq!(
            p.insert_container(0, stack(0, 1)).unwrap(),
            Some(stack(0, 1))
        );
        assert_eq!(p.revision, rev + 2);
        assert!(p.insert_container(99, stack(0, 1)).is_err());
        assert!(p.insert_container(0, stack(0, 0)).is_err());
    }
    #[test]
    fn configured_hotbar_and_crafting_capacity_validate_and_keep_carried_items() {
        let r = Registry::built_in();
        let config = Profile {
            player_slots: 80,
            hotbar_slots: 16,
            craft_side: 8,
            table_side: 8,
            ..profile()
        };
        let mut p = Inventory::empty();
        p.configure(config, &r).unwrap();
        p.select(15).unwrap();
        p.cycle(1);
        assert_eq!(p.active, 0);
        p.cycle(-1);
        assert_eq!(p.active, 15);
        p.crafting[63] = Some(stack(0, 1));
        p.table[63] = Some(stack(1, 1));
        p.cursor = Some(stack(2, 1));
        p.insert_container(1, stack(3, 3)).unwrap();
        assert!(p.close_inventory());
        assert_eq!(p.total(), 6);
        assert_eq!(p.containers[1][0], Some(stack(3, 3)));
        p.validate_items(&r).unwrap();
    }
}
