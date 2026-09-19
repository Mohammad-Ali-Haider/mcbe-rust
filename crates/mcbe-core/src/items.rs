//! Version-bound pristine item stacks reconstructed from retained API observations.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    sync::Arc,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StackError {
    ArgumentOutOfBounds,
    PropertyOutOfBounds,
    InvalidIdentifier(String),
    UnsupportedNumber,
    DifferentRegistry,
}
impl StackError {
    pub fn category(&self) -> &'static str {
        match self {
            Self::ArgumentOutOfBounds => "ArgumentOutOfBoundsError",
            Self::PropertyOutOfBounds => "PropertyOutOfBoundsError",
            Self::InvalidIdentifier(_) => "Error",
            Self::UnsupportedNumber => "UnsupportedNumber",
            Self::DifferentRegistry => "DifferentRegistry",
        }
    }
}
impl fmt::Display for StackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {:?}", self.category(), self)
    }
}
impl std::error::Error for StackError {}

pub fn normalize_id(id: &str) -> Result<String, StackError> {
    if id.is_empty() {
        return Err(StackError::InvalidIdentifier(id.into()));
    }
    Ok(if id.contains(':') {
        id.into()
    } else {
        format!("minecraft:{id}")
    })
}
fn amount(value: f64, setter: bool) -> Result<u8, StackError> {
    if !value.is_finite() {
        return Err(StackError::UnsupportedNumber);
    }
    if value < i32::MIN as f64 || value > i32::MAX as f64 {
        return Err(StackError::ArgumentOutOfBounds);
    }
    let truncated = value.trunc();
    if !(1.0..=255.0).contains(&truncated) {
        return Err(if setter {
            StackError::PropertyOutOfBounds
        } else {
            StackError::ArgumentOutOfBounds
        });
    }
    Ok(truncated as u8)
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ItemDefinition {
    pub id: String,
    pub type_id: String,
    pub max_amount: u8,
    pub is_stackable: bool,
    pub tags: BTreeSet<String>,
    pub components: Vec<String>,
}
#[derive(Clone, Debug)]
pub struct ItemStack {
    definition: Arc<ItemDefinition>,
    source: Arc<str>,
    amount: u8,
}
impl ItemStack {
    pub fn source_id(&self) -> &str {
        &self.source
    }
    pub fn definition(&self) -> &ItemDefinition {
        &self.definition
    }
    pub fn amount(&self) -> u8 {
        self.amount
    }
    pub fn set_amount(&mut self, value: f64) -> Result<(), StackError> {
        let valid = amount(value, true)?;
        self.amount = if self.definition.type_id.is_empty() {
            0
        } else {
            valid.min(self.definition.max_amount)
        };
        Ok(())
    }
    pub fn is_stackable_with(&self, other: &Self) -> Result<bool, StackError> {
        if self.source != other.source {
            return Err(StackError::DifferentRegistry);
        }
        Ok(self.definition.is_stackable
            && other.definition.is_stackable
            && self.definition.type_id == other.definition.type_id)
    }
    pub fn observation(&self) -> Value {
        serde_json::json!({"typeId":self.definition.type_id,"amount":self.amount,
            "maxAmount":self.definition.max_amount,"isStackable":self.definition.is_stackable})
    }
}

pub struct Registry {
    items: BTreeMap<String, Arc<ItemDefinition>>,
    blocks: BTreeMap<String, Value>,
    source: Arc<str>,
    metadata: Value,
    observed_items: usize,
}
impl Registry {
    pub fn from_bytes(bytes: &[u8]) -> crate::Result<Self> {
        let report: Value = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
        let rows = report["items"].as_array().ok_or("Missing items")?;
        let block_rows = report["blocks"].as_array().ok_or("Missing blocks")?;
        let done = &report["completion_record"];
        if report["complete"] != true
            || report["errors"].as_array().is_none_or(|v| !v.is_empty())
            || done["kind"] != "done"
            || done["errors"] != 0
            || done["items"].as_u64() != Some(rows.len() as u64)
            || done["blocks"].as_u64() != Some(block_rows.len() as u64)
            || rows.is_empty()
        {
            return Err("Incomplete or inconsistent registry capture".into());
        }
        let mut items = BTreeMap::new();
        for row in rows {
            let definition: ItemDefinition =
                serde_json::from_value(row.clone()).map_err(|e| e.to_string())?;
            if definition.max_amount == 0
                || definition.id.is_empty()
                || items
                    .insert(definition.id.clone(), Arc::new(definition))
                    .is_some()
            {
                return Err("Invalid or duplicate item definition".into());
            }
        }
        if let Some(cases) = report["stack_cases"].as_array() {
            for case in cases {
                if case["success"] == true
                    && case["inputAmount"].as_f64() == Some(1.0)
                    && case["typeId"] == ""
                {
                    let id = normalize_id(
                        case["item"]
                            .as_str()
                            .ok_or("Invalid empty-item identifier")?,
                    )
                    .map_err(|e| e.to_string())?;
                    let max_amount = case["maxAmount"]
                        .as_u64()
                        .and_then(|n| u8::try_from(n).ok())
                        .filter(|n| *n > 0)
                        .ok_or("Invalid empty-item maximum")?;
                    let is_stackable = case["isStackable"]
                        .as_bool()
                        .ok_or("Invalid empty-item stackability")?;
                    items.entry(id.clone()).or_insert_with(|| {
                        Arc::new(ItemDefinition {
                            id,
                            type_id: String::new(),
                            max_amount,
                            is_stackable,
                            tags: BTreeSet::new(),
                            components: Vec::new(),
                        })
                    });
                }
            }
        }
        let mut blocks = BTreeMap::new();
        for row in block_rows {
            let id = row["id"]
                .as_str()
                .filter(|s| !s.is_empty())
                .ok_or("Missing block identifier")?;
            if !row["states"].is_object()
                || !row["tags"].is_array()
                || blocks.insert(id.into(), row.clone()).is_some()
            {
                return Err("Invalid or duplicate default block".into());
            }
        }
        Ok(Self {
            items,
            blocks,
            source: format!("{:x}", Sha256::digest(bytes)).into(),
            metadata: report["metadata"].clone(),
            observed_items: rows.len(),
        })
    }
    pub fn source_id(&self) -> &str {
        &self.source
    }
    pub fn metadata(&self) -> &Value {
        &self.metadata
    }
    pub fn observed_items(&self) -> usize {
        self.observed_items
    }
    pub fn item_count(&self) -> usize {
        self.items.len()
    }
    pub fn block_count(&self) -> usize {
        self.blocks.len()
    }
    pub fn definition(&self, id: &str) -> Option<&ItemDefinition> {
        self.items.get(id).map(AsRef::as_ref)
    }
    pub fn default_block(&self, id: &str) -> Result<Option<&Value>, StackError> {
        Ok(self.blocks.get(&normalize_id(id)?))
    }
    pub fn tag_items(&self) -> BTreeMap<String, BTreeSet<String>> {
        let mut tags: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for def in self.items.values() {
            for tag in &def.tags {
                tags.entry(tag.clone())
                    .or_default()
                    .insert(def.type_id.clone());
            }
        }
        tags
    }
    pub fn create_stack(&self, id: &str, value: f64) -> Result<ItemStack, StackError> {
        let amount = amount(value, false)?;
        let id = normalize_id(id)?;
        let definition = self
            .items
            .get(&id)
            .ok_or(StackError::InvalidIdentifier(id))?;
        Ok(ItemStack {
            definition: Arc::clone(definition),
            source: Arc::clone(&self.source),
            amount,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn fixture() -> Value {
        json!({"complete":true,"errors":[],"completion_record":{"kind":"done","errors":0,"items":3,"blocks":1},
            "items":[
                {"id":"minecraft:stone","typeId":"minecraft:stone","maxAmount":64,"isStackable":true,"tags":["material"],"components":[]},
                {"id":"minecraft:egg","typeId":"minecraft:egg","maxAmount":16,"isStackable":true,"tags":[],"components":[]},
                {"id":"minecraft:sword","typeId":"minecraft:sword","maxAmount":1,"isStackable":false,"tags":[],"components":[]}],
            "blocks":[{"id":"minecraft:stone","states":{},"tags":[]}],
            "stack_cases":[{"success":true,"item":"air","inputAmount":1,"typeId":"","maxAmount":255,"isStackable":true}]})
    }
    fn registry() -> Registry {
        Registry::from_bytes(&serde_json::to_vec(&fixture()).unwrap()).unwrap()
    }
    #[test]
    fn constructor_setter_and_empty_item() {
        let r = registry();
        let mut egg = r.create_stack("egg", 255.9).unwrap();
        assert_eq!(egg.amount(), 255);
        egg.set_amount(255.9).unwrap();
        assert_eq!(egg.amount(), 16);
        let mut air = r.create_stack("air", 64.0).unwrap();
        assert_eq!(air.amount(), 64);
        air.set_amount(64.0).unwrap();
        assert_eq!(air.amount(), 0);
    }
    #[test]
    fn validation_order_and_atomic_failure() {
        let r = registry();
        assert_eq!(
            r.create_stack("missing", 0.0).unwrap_err(),
            StackError::ArgumentOutOfBounds
        );
        let mut stack = r.create_stack("stone", 7.0).unwrap();
        for (input, error) in [
            (0.9, StackError::PropertyOutOfBounds),
            (2147483648.0, StackError::ArgumentOutOfBounds),
            (f64::NAN, StackError::UnsupportedNumber),
            (f64::INFINITY, StackError::UnsupportedNumber),
        ] {
            assert_eq!(stack.set_amount(input), Err(error));
            assert_eq!(stack.amount(), 7);
        }
    }
    #[test]
    fn pristine_compatibility_and_independent_clones() {
        let r = registry();
        let a = r.create_stack("stone", 255.0).unwrap();
        let mut b = a.clone();
        b.set_amount(2.0).unwrap();
        assert_eq!(a.amount(), 255);
        assert!(a.is_stackable_with(&b).unwrap());
        assert!(
            !a.is_stackable_with(&r.create_stack("egg", 1.0).unwrap())
                .unwrap()
        );
        let sword = r.create_stack("sword", 1.0).unwrap();
        assert!(!sword.is_stackable_with(&sword).unwrap());
        let mut data = fixture();
        data["metadata"] = json!({"different":true});
        let other = Registry::from_bytes(&serde_json::to_vec(&data).unwrap()).unwrap();
        assert_eq!(
            a.is_stackable_with(&other.create_stack("stone", 1.0).unwrap()),
            Err(StackError::DifferentRegistry)
        );
    }
    #[test]
    fn malformed_registry_and_tag_lookup() {
        for field in ["complete", "completion_record", "items", "blocks", "errors"] {
            let mut data = fixture();
            data[field] = Value::Null;
            assert!(Registry::from_bytes(&serde_json::to_vec(&data).unwrap()).is_err());
        }
        let mut data = fixture();
        data["items"][1] = data["items"][0].clone();
        assert!(Registry::from_bytes(&serde_json::to_vec(&data).unwrap()).is_err());
        let r = registry();
        assert_eq!(
            r.tag_items()["material"],
            BTreeSet::from(["minecraft:stone".into()])
        );
        assert!(r.default_block("stone").unwrap().is_some());
        assert_eq!(r.item_count(), 4);
    }
}
