//! Overworld container block entities and prototype-backed pristine item edits.
//! Other records and metadata are retained as opaque, ordered NBT.
use crate::{
    Result,
    inventory::{ContainerKind, Containers},
    items::{ItemStack, Registry},
    leveldb::Snapshot,
    nbt::{self, Limits, Tag, Value},
    world_edit::Updates,
};
use serde_json::{Value as Json, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

fn compound(value: &Value) -> Result<&[Tag]> {
    if let Value::Compound(tags) = value {
        Ok(tags)
    } else {
        Err("Container NBT compound required".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::leveldb::Entry;
    fn tag(name: &str, value: Value) -> Tag {
        Tag {
            name: name.as_bytes().to_vec(),
            value,
        }
    }
    fn registry() -> Registry {
        Registry::from_bytes(&serde_json::to_vec(&json!({"complete":true,"errors":[],"completion_record":{"kind":"done","errors":0,"items":2,"blocks":0},"items":[
            {"id":"minecraft:egg","typeId":"minecraft:egg","maxAmount":16,"isStackable":true,"tags":[],"components":[]},
            {"id":"minecraft:stone","typeId":"minecraft:stone","maxAmount":64,"isStackable":true,"tags":[],"components":[]}],"blocks":[]})).unwrap()).unwrap()
    }
    fn stored_item(slot: i8, count: i8) -> Value {
        Value::Compound(vec![
            tag("Count", Value::Byte(count)),
            tag("Damage", Value::Short(0)),
            tag("Name", Value::String(b"minecraft:egg".to_vec())),
            tag("Slot", Value::Byte(slot)),
            tag("WasPickedUp", Value::Byte(0)),
        ])
    }
    fn root(id: &str, x: i32, items: Vec<Value>) -> Tag {
        named(Value::Compound(vec![
            tag("id", Value::String(id.as_bytes().to_vec())),
            tag("x", Value::Int(x)),
            tag("y", Value::Int(64)),
            tag("z", Value::Int(2)),
            tag(
                "Items",
                Value::List(if items.is_empty() { 0 } else { 10 }, items),
            ),
            tag("UnmodeledField", Value::Long(0x1234567890)),
        ]))
    }
    fn snapshot(roots: &[Tag]) -> Snapshot {
        let mut key = vec![0; 8];
        key.push(49);
        let data = roots
            .iter()
            .map(|r| nbt::write(r, Limits::default()).unwrap())
            .collect::<Vec<_>>()
            .concat();
        Snapshot {
            entries: BTreeMap::from([(
                key,
                Entry {
                    sequence: 1,
                    value: Some(data),
                },
            )]),
            ..Snapshot::default()
        }
    }
    #[test]
    fn signed_counts_empty_lists_and_opaque_records_survive_edits() {
        let registry = registry();
        let source = snapshot(&[
            root("Barrel", 2, vec![stored_item(0, -1)]),
            root("UnmodeledSign", 6, vec![]),
        ]);
        let store = Store::read(&source, &registry).unwrap();
        let before = store.record_hashes().unwrap();
        let mut contents = store.load_group(&[[2, 64, 2]], &registry).unwrap();
        assert_eq!(contents.get(0, 0.).unwrap().unwrap().amount(), 255);
        contents.clear(0).unwrap();
        let prepared = store.prepare(&[[2, 64, 2]], &contents, &registry).unwrap();
        assert_eq!(prepared.changed_slots, 1);
        assert_eq!(prepared.updates.len(), 1);
        let edited = Snapshot {
            entries: BTreeMap::from([(
                prepared.updates[0].0.clone(),
                Entry {
                    sequence: 2,
                    value: prepared.updates[0].1.clone(),
                },
            )]),
            ..Snapshot::default()
        };
        let reloaded = Store::read(&edited, &registry).unwrap();
        assert_eq!(
            before[&[6, 64, 2]],
            reloaded.record_hashes().unwrap()[&[6, 64, 2]]
        );
        let record = &reloaded.records[&[2, 64, 2]];
        let fields = compound(&reloaded.groups[&record.key][record.index].value).unwrap();
        assert!(
            matches!(field(fields,b"Items").unwrap(),Value::List(0,values)if values.is_empty())
        );
        assert_eq!(
            field(fields, b"UnmodeledField").unwrap(),
            &Value::Long(0x1234567890)
        );
    }
    #[test]
    fn selected_metadata_duplicates_and_missing_prototypes_are_rejected() {
        let registry = registry();
        let source = snapshot(&[root("Barrel", 2, vec![stored_item(0, 1)])]);
        let store = Store::read(&source, &registry).unwrap();
        let mut contents = store.load_group(&[[2, 64, 2]], &registry).unwrap();
        contents
            .set(0, 1., Some(&registry.create_stack("stone", 1.).unwrap()))
            .unwrap();
        assert!(store.prepare(&[[2, 64, 2]], &contents, &registry).is_err());
        let duplicate = snapshot(&[root(
            "Barrel",
            2,
            vec![stored_item(0, 1), stored_item(0, 2)],
        )]);
        assert!(
            Store::read(&duplicate, &registry)
                .unwrap()
                .load_group(&[[2, 64, 2]], &registry)
                .is_err()
        );
        let mut named_item = stored_item(0, 1);
        if let Value::Compound(fields) = &mut named_item {
            fields.push(tag(
                "tag",
                Value::Compound(vec![tag(
                    "display",
                    Value::Compound(vec![tag("Name", Value::String(b"Keep me".to_vec()))]),
                )]),
            ));
        }
        assert!(
            Store::read(&snapshot(&[root("Barrel", 2, vec![named_item])]), &registry)
                .unwrap()
                .load_group(&[[2, 64, 2]], &registry)
                .is_err()
        );
        assert!(Store::read(&snapshot(&[root("Barrel", 32, vec![])]), &registry).is_err());
    }
}
fn optional<'a>(tags: &'a [Tag], name: &[u8]) -> Result<Option<&'a Value>> {
    let mut found = tags.iter().filter(|t| t.name == name);
    let result = found.next().map(|t| &t.value);
    if found.next().is_some() {
        return Err("Duplicate container NBT field".into());
    }
    Ok(result)
}
fn field<'a>(tags: &'a [Tag], name: &[u8]) -> Result<&'a Value> {
    optional(tags, name)?.ok_or_else(|| {
        format!(
            "Missing container NBT field {}",
            String::from_utf8_lossy(name)
        )
    })
}
fn text(value: &Value) -> Result<String> {
    if let Value::String(bytes) = value {
        String::from_utf8(bytes.clone()).map_err(|_| "Invalid UTF-8 container field".into())
    } else {
        Err("Container string required".into())
    }
}
fn integer(value: &Value) -> Result<i32> {
    if let Value::Int(n) = value {
        Ok(*n)
    } else {
        Err("Container int required".into())
    }
}
fn byte(value: &Value) -> Result<u8> {
    if let Value::Byte(n) = value {
        Ok(*n as u8)
    } else {
        Err("Container byte required".into())
    }
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn named(value: Value) -> Tag {
    Tag {
        name: vec![],
        value,
    }
}
fn kind(id: &str) -> Option<ContainerKind> {
    match id {
        "Barrel" => Some(ContainerKind::Barrel),
        "Hopper" => Some(ContainerKind::Hopper),
        "Dispenser" => Some(ContainerKind::Dispenser),
        _ => None,
    }
}
fn capacity(id: &str) -> Option<usize> {
    kind(id)
        .map(ContainerKind::size)
        .or(if id == "Chest" { Some(27) } else { None })
}
fn list(value: &Value) -> Result<&[Value]> {
    match value {
        Value::List(kind, values) if *kind == 10 || *kind == 0 && values.is_empty() => Ok(values),
        _ => Err("Container Items must be a compound list or empty end list".into()),
    }
}

pub struct StoredItem {
    pub slot: usize,
    pub stack: ItemStack,
    pub observation: Json,
    pub pristine: bool,
}
fn item(value: &Value, registry: &Registry) -> Result<StoredItem> {
    let tags = compound(value)?;
    let id = text(field(tags, b"Name")?)?;
    let count = byte(field(tags, b"Count")?)?;
    let slot = usize::from(byte(field(tags, b"Slot")?)?);
    if count == 0 {
        return Err("Zero-count persisted items are outside the measured storage model".into());
    }
    let stack = registry
        .create_stack(&id, f64::from(count))
        .map_err(|e| e.to_string())?;
    let metadata = optional(tags, b"tag")?
        .map(compound)
        .transpose()?
        .unwrap_or(&[]);
    let display = optional(metadata, b"display")?
        .map(compound)
        .transpose()?
        .unwrap_or(&[]);
    let name = optional(display, b"Name")?.map(text).transpose()?;
    let lore = match optional(display, b"Lore")? {
        None => vec![],
        Some(Value::List(8, values)) => values.iter().map(text).collect::<Result<Vec<_>>>()?,
        _ => return Err("Unsupported stored lore format".into()),
    };
    let durability = stack
        .definition()
        .components
        .iter()
        .any(|c| c == "minecraft:durability");
    let damage = optional(metadata, b"Damage")?.map(integer).transpose()?;
    if damage.is_some_and(|n| n < 0) {
        return Err("Negative stored durability damage".into());
    }
    let observation = json!({"typeId":stack.definition().type_id,"amount":count,"maxAmount":stack.definition().max_amount,"isStackable":stack.definition().is_stackable,"nameTag":name,"lore":lore,"damage":if durability{Some(damage.unwrap_or(0))}else{None}});
    let allowed: [&[u8]; 7] = [
        b"Block",
        b"Count",
        b"Damage",
        b"Name",
        b"Slot",
        b"WasPickedUp",
        b"tag",
    ];
    let mut pristine = tags.iter().all(|t| allowed.contains(&t.name.as_slice()))
        && matches!(field(tags, b"Damage")?, Value::Short(0))
        && matches!(optional(tags, b"WasPickedUp")?, Some(Value::Byte(0)))
        && (metadata.is_empty()
            || metadata.len() == 1
                && metadata[0].name == b"Damage"
                && matches!(metadata[0].value, Value::Int(0)));
    if let Some(block) = optional(tags, b"Block")? {
        let fields = compound(block)?;
        let states = compound(field(fields, b"states")?)?;
        let default = registry.default_block(&id).map_err(|e| e.to_string())?;
        pristine = pristine
            && fields.len() == 3
            && text(field(fields, b"name")?)? == id
            && integer(field(fields, b"version")?)? == 18168865
            && default.is_some();
        for property in states {
            let key = String::from_utf8(property.name.clone())
                .map_err(|_| "Invalid item block property")?;
            let value = match &property.value {
                Value::Byte(0) => json!(false),
                Value::Byte(1) => json!(true),
                Value::Int(n) => json!(n),
                Value::String(_) => json!(text(&property.value)?),
                _ => {
                    pristine = false;
                    continue;
                }
            };
            if default.is_none_or(|row| row["states"][&key] != value) {
                pristine = false;
            }
        }
    }
    Ok(StoredItem {
        slot,
        stack,
        observation,
        pristine,
    })
}
#[derive(Clone)]
struct Record {
    key: Vec<u8>,
    index: usize,
    id: String,
}
pub struct Store {
    groups: BTreeMap<Vec<u8>, Vec<Tag>>,
    original: BTreeMap<Vec<u8>, Vec<u8>>,
    records: BTreeMap<[i32; 3], Record>,
    prototypes: BTreeMap<String, Value>,
    template_hashes: BTreeMap<String, String>,
    source_id: String,
}
pub struct Prepared {
    pub updates: Updates,
    pub changed_containers: usize,
    pub changed_slots: usize,
}
impl Store {
    pub fn read(snapshot: &Snapshot, registry: &Registry) -> Result<Self> {
        let mut store = Self {
            groups: BTreeMap::new(),
            original: BTreeMap::new(),
            records: BTreeMap::new(),
            prototypes: BTreeMap::new(),
            template_hashes: BTreeMap::new(),
            source_id: registry.source_id().into(),
        };
        for (key, entry) in &snapshot.entries {
            if key.len() != 9 || key[8] != 49 {
                continue;
            }
            let Some(data) = &entry.value else {
                continue;
            };
            let cx = i32::from_le_bytes(key[..4].try_into().map_err(|_| "Container chunk X")?);
            let cz = i32::from_le_bytes(key[4..8].try_into().map_err(|_| "Container chunk Z")?);
            let mut at = 0;
            let mut tags = vec![];
            while at < data.len() {
                if tags.len() >= 65536 {
                    return Err("Container record limit exceeded".into());
                }
                let (tag, used) = nbt::read_prefix(&data[at..], Limits::default())?;
                if used == 0 {
                    return Err("Empty NBT container prefix".into());
                }
                at += used;
                let fields = compound(&tag.value)?;
                let position = [
                    integer(field(fields, b"x")?)?,
                    integer(field(fields, b"y")?)?,
                    integer(field(fields, b"z")?)?,
                ];
                if [position[0].div_euclid(16), position[2].div_euclid(16)] != [cx, cz] {
                    return Err("Container block entity belongs to a different chunk".into());
                }
                let id = text(field(fields, b"id")?)?;
                let index = tags.len();
                if store
                    .records
                    .insert(
                        position,
                        Record {
                            key: key.clone(),
                            index,
                            id,
                        },
                    )
                    .is_some()
                {
                    return Err("Duplicate block entity position".into());
                }
                tags.push(tag);
            }
            let encoded = tags
                .iter()
                .map(|t| nbt::write(t, Limits::default()))
                .collect::<Result<Vec<_>>>()?
                .concat();
            if encoded != *data {
                return Err("Container stream does not round trip byte exactly".into());
            }
            store.groups.insert(key.clone(), tags);
            store.original.insert(key.clone(), data.clone());
        }
        for record in store.records.values() {
            if capacity(&record.id).is_none() {
                continue;
            }
            let root = &store.groups[&record.key][record.index];
            let Ok(value) = field(compound(&root.value)?, b"Items") else {
                continue;
            };
            let Ok(values) = list(value) else {
                continue;
            };
            let Ok(parsed_items) = values
                .iter()
                .map(|value| item(value, registry))
                .collect::<Result<Vec<_>>>()
            else {
                continue;
            };
            let mut slots = BTreeSet::new();
            if parsed_items.iter().any(|parsed| {
                parsed.slot >= capacity(&record.id).unwrap_or(0) || !slots.insert(parsed.slot)
            }) {
                continue;
            }
            for (value, parsed) in values.iter().zip(parsed_items) {
                if !parsed.pristine {
                    continue;
                }
                let skeleton = Value::Compound(
                    compound(value)?
                        .iter()
                        .filter(|t| t.name != b"Count" && t.name != b"Slot")
                        .cloned()
                        .collect(),
                );
                let digest = hash(&nbt::write(&named(skeleton), Limits::default())?);
                let id = parsed.stack.definition().type_id.clone();
                if store
                    .template_hashes
                    .get(&id)
                    .is_some_and(|old| *old != digest)
                {
                    return Err("Ambiguous pristine item template in source world".into());
                }
                store.template_hashes.insert(id.clone(), digest);
                store.prototypes.entry(id).or_insert_with(|| value.clone());
            }
        }
        Ok(store)
    }
    fn check_registry(&self, registry: &Registry) -> Result<()> {
        if self.source_id != registry.source_id() {
            Err("Container registry snapshot differs".into())
        } else {
            Ok(())
        }
    }
    pub fn positions(&self) -> Vec<[i32; 3]> {
        self.records
            .iter()
            .filter(|(_, r)| capacity(&r.id).is_some())
            .map(|(p, _)| *p)
            .collect()
    }
    pub fn primary_positions(&self, registry: &Registry) -> Result<Vec<[i32; 3]>> {
        let mut result = vec![];
        for target in ["Barrel", "Hopper", "Dispenser"] {
            let mut selected = None;
            for (position, record) in &self.records {
                if record.id == target && self.load_group(&[*position], registry).is_ok() {
                    selected = Some(*position);
                    break;
                }
            }
            result.push(selected.ok_or("Source needs a pristine barrel, hopper and dispenser")?);
        }
        Ok(result)
    }
    pub fn items(&self, position: [i32; 3], registry: &Registry) -> Result<Vec<StoredItem>> {
        self.check_registry(registry)?;
        let record = self
            .records
            .get(&position)
            .ok_or("Missing stored container")?;
        if capacity(&record.id).is_none() {
            return Err("Unmeasured stored container kind".into());
        }
        let root = &self.groups[&record.key][record.index];
        let parsed = list(field(compound(&root.value)?, b"Items")?)?
            .iter()
            .map(|v| item(v, registry))
            .collect::<Result<Vec<_>>>()?;
        let mut seen = BTreeSet::new();
        if parsed
            .iter()
            .any(|item| item.slot >= capacity(&record.id).unwrap_or(0) || !seen.insert(item.slot))
        {
            return Err("Invalid or duplicate stored item slot".into());
        }
        Ok(parsed)
    }
    pub fn observation(&self, position: [i32; 3], registry: &Registry) -> Result<Json> {
        let record = self
            .records
            .get(&position)
            .ok_or("Missing stored container")?;
        let size = capacity(&record.id).ok_or("Unmeasured container capacity")?;
        let mut slots = vec![Json::Null; size];
        for item in self.items(position, registry)? {
            slots[item.slot] = item.observation;
        }
        let root = &self.groups[&record.key][record.index];
        Ok(
            json!({"position":position,"entity_id":record.id,"size":size,"emptySlotsCount":slots.iter().filter(|s|s.is_null()).count(),"slots":slots,"record_sha256":hash(&nbt::write(root,Limits::default())?)}),
        )
    }
    pub fn template_hashes(&self) -> &BTreeMap<String, String> {
        &self.template_hashes
    }
    pub fn record_hashes(&self) -> Result<BTreeMap<[i32; 3], String>> {
        self.records
            .iter()
            .map(|(position, record)| {
                Ok((
                    *position,
                    hash(&nbt::write(
                        &self.groups[&record.key][record.index],
                        Limits::default(),
                    )?),
                ))
            })
            .collect()
    }
    pub fn key_observations(&self) -> Vec<Json> {
        self.original.iter().map(|(key,data)|json!({"key_hex":key.iter().map(|b|format!("{b:02x}")).collect::<String>(),"bytes":data.len(),"sha256":hash(data),"records":self.groups[key].len()})).collect()
    }
    pub fn load_group(&self, positions: &[[i32; 3]], registry: &Registry) -> Result<Containers> {
        self.check_registry(registry)?;
        if positions.iter().collect::<BTreeSet<_>>().len() != positions.len() {
            return Err("Duplicate selected container".into());
        }
        let kinds = positions
            .iter()
            .map(|p| {
                self.records
                    .get(p)
                    .and_then(|r| kind(&r.id))
                    .ok_or("Only pristine barrel/hopper/dispenser mutation is measured")
            })
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let mut result = Containers::with_kinds(registry, &kinds).map_err(|e| e.to_string())?;
        for (index, position) in positions.iter().enumerate() {
            for item in self.items(*position, registry)? {
                if !item.pristine {
                    return Err(
                        "Selected container has item metadata outside pristine mutation support"
                            .into(),
                    );
                }
                result
                    .set(index, item.slot as f64, Some(&item.stack))
                    .map_err(|e| e.to_string())?;
                if result
                    .get(index, item.slot as f64)
                    .map_err(|e| e.to_string())?
                    .is_none_or(|s| s.observation() != item.stack.observation())
                {
                    return Err(
                        "Persisted count normalizes outside the measured mutable state".into(),
                    );
                }
            }
        }
        Ok(result)
    }
    pub fn check_blocks(
        &self,
        snapshot: &Snapshot,
        positions: &[[i32; 3]],
        palette: &crate::blocks::Palette,
    ) -> Result<()> {
        let mut world = crate::chunks::World::new(Snapshot {
            entries: snapshot.entries.clone(),
            ..Snapshot::default()
        });
        for position in positions {
            let record = self
                .records
                .get(position)
                .ok_or("Selected container entity absent")?;
            if kind(&record.id).is_none() {
                return Err("Selected entity has an unsupported mutable container kind".into());
            }
            let block = world
                .block(*position, 0)?
                .ok_or("Selected container has no primary block")?;
            let actual = palette.resolve_saved(&block).map_err(|e| e.to_string())?;
            if actual.id() != format!("minecraft:{}", record.id.to_ascii_lowercase()) {
                return Err("Container block and block entity types differ".into());
            }
        }
        Ok(())
    }
    pub fn prepare(
        &self,
        positions: &[[i32; 3]],
        contents: &Containers,
        registry: &Registry,
    ) -> Result<Prepared> {
        self.check_registry(registry)?;
        if contents.source_id() != self.source_id
            || contents
                .observation()
                .as_array()
                .is_none_or(|a| a.len() != positions.len())
        {
            return Err("Edited container group differs from source/registry".into());
        }
        let original = self.load_group(positions, registry)?;
        let mut edited = BTreeMap::<Vec<u8>, Vec<Tag>>::new();
        let mut changed_containers = 0;
        let mut changed_slots = 0;
        for (index, position) in positions.iter().enumerate() {
            let size = original.container(index).map_err(|e| e.to_string())?.size();
            if contents.container(index).map_err(|e| e.to_string())?.size() != size {
                return Err("Edited container capacity changed".into());
            }
            let mut values = vec![];
            let mut differences = 0;
            for slot in 0..size {
                let current = contents
                    .get(index, slot as f64)
                    .map_err(|e| e.to_string())?;
                let old = original
                    .get(index, slot as f64)
                    .map_err(|e| e.to_string())?;
                if current.as_ref().map(ItemStack::observation)
                    != old.as_ref().map(ItemStack::observation)
                {
                    differences += 1;
                }
                if let Some(stack) = current {
                    let template = self.prototypes.get(&stack.definition().type_id).ok_or(
                        "Requested item has no pristine persistent prototype in this source world",
                    )?;
                    let mut tags = compound(template)?.to_vec();
                    for tag in &mut tags {
                        if tag.name == b"Count" {
                            tag.value = Value::Byte(stack.amount() as i8);
                        }
                        if tag.name == b"Slot" {
                            tag.value = Value::Byte(slot as u8 as i8);
                        }
                    }
                    values.push(Value::Compound(tags));
                }
            }
            if differences == 0 {
                continue;
            }
            changed_containers += 1;
            changed_slots += differences;
            let record = &self.records[position];
            let roots = edited
                .entry(record.key.clone())
                .or_insert_with(|| self.groups[&record.key].clone());
            let fields = match &mut roots[record.index].value {
                Value::Compound(fields) => fields,
                _ => return Err("Stored record compound changed".into()),
            };
            let target = fields
                .iter_mut()
                .find(|t| t.name == b"Items")
                .ok_or("Missing Items field")?;
            target.value = Value::List(if values.is_empty() { 0 } else { 10 }, values);
        }
        if changed_containers == 0 {
            return Err("No container changes to save".into());
        }
        let updates = edited
            .into_iter()
            .map(|(key, roots)| {
                Ok((
                    key,
                    Some(
                        roots
                            .iter()
                            .map(|t| nbt::write(t, Limits::default()))
                            .collect::<Result<Vec<_>>>()?
                            .concat(),
                    ),
                ))
            })
            .collect::<Result<Updates>>()?;
        Ok(Prepared {
            updates,
            changed_containers,
            changed_slots,
        })
    }
}
