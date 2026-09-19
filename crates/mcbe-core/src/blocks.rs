//! Version-bound measured block permutations. Property insertion order is semantic.
use crate::chunks::{BlockState, StateValue};
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    fmt,
    sync::Arc,
};

pub type States = Vec<(String, StateValue)>;
type Key = (String, States);
type PartialKey = (String, String, StateValue);
type Effects = HashMap<(String, String), BTreeSet<String>>;
pub type BlockResult<T> = std::result::Result<T, BlockError>;

#[derive(Debug, PartialEq, Eq)]
pub enum BlockError {
    Evidence(String),
    Unsupported(String),
    Operation(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn fixture() -> (Value, Value) {
        let mut permutations = vec![];
        for direction in [0, 1] {
            for cardinal in ["east", "west"] {
                for active in [false, true] {
                    permutations.push(json!({"inputId":"minecraft:fixture","inputStates":{"direction":direction,"cardinal":cardinal,"active":active},"success":true,"id":"minecraft:fixture","states":{"direction":if cardinal=="east" {0}else{1},"cardinal":cardinal,"active":active},"tags":["solid"]}));
                }
            }
        }
        let mut partial = vec![];
        for (name, value, direction, cardinal, active) in [
            ("direction", json!(0), 0, "east", false),
            ("direction", json!(1), 1, "west", false),
            ("cardinal", json!("east"), 0, "east", false),
            ("cardinal", json!("west"), 1, "west", false),
            ("active", json!(false), 0, "east", false),
            ("active", json!(true), 0, "east", true),
        ] {
            for operation in ["resolve", "withState"] {
                partial.push(json!({"operation":operation,"id":"minecraft:fixture","states":{name:value},"success":true,"result":{"id":"minecraft:fixture","states":{"direction":direction,"cardinal":cardinal,"active":active},"tags":["solid"]}}));
            }
        }
        (
            json!({"complete":true,"errors":[],"metadata":{"executable_sha256":"fixture"},"completion_record":{"kind":"done","errors":0,"blocks":1,"blockStates":3,"permutations":8},"blocks":[{"id":"minecraft:fixture","states":{"direction":0,"cardinal":"east","active":false}}],"block_states":[{"id":"direction","values":[0,1]},{"id":"cardinal","values":["east","west"]},{"id":"active","values":[false,true]}],"permutations":permutations}),
            json!({"complete":true,"errors":[],"metadata":{"executable_sha256":"fixture"},"completion_record":{"kind":"done","blockCases":12},"block_cases":partial}),
        )
    }
    #[test]
    fn aliases_follow_recorded_full_order_and_explicit_partial_order() {
        let (a, b) = fixture();
        let p = Palette::from_reports(&a, &b).unwrap();
        assert_eq!(
            p.resolve("fixture", &states(&json!({"direction":1})).unwrap())
                .unwrap()
                .states()["cardinal"],
            StateValue::String("west".into())
        );
        let first: Value =
            serde_json::from_str(r#"{"direction":1,"cardinal":"east","active":false}"#).unwrap();
        let reverse: Value =
            serde_json::from_str(r#"{"cardinal":"east","direction":1,"active":false}"#).unwrap();
        assert_eq!(
            p.resolve("fixture", &states(&first).unwrap())
                .unwrap()
                .states()["direction"],
            StateValue::Int(0)
        );
        assert_eq!(
            p.resolve("fixture", &states(&reverse).unwrap())
                .unwrap()
                .states()["direction"],
            StateValue::Int(1)
        );
    }
    #[test]
    fn assignment_keeps_unaffected_nondefault_values_and_original() {
        let (a, b) = fixture();
        let p = Palette::from_reports(&a, &b).unwrap();
        let base = p
            .resolve(
                "fixture",
                &states(&json!({"active":true,"cardinal":"west"})).unwrap(),
            )
            .unwrap();
        let changed = p
            .with_state(&base, "direction", StateValue::Int(0))
            .unwrap();
        assert_eq!(base.states()["direction"], StateValue::Int(1));
        assert_eq!(changed.states()["direction"], StateValue::Int(0));
        assert_eq!(changed.states()["active"], StateValue::Bool(true));
        assert_eq!(
            changed.states()["cardinal"],
            StateValue::String("east".into())
        );
    }
    #[test]
    fn invalid_inputs_are_distinct_from_measured_operation_errors() {
        let (a, mut b) = fixture();
        let p = Palette::from_reports(&a, &b).unwrap();
        for input in [json!({"active":1}), json!({"missing":true})] {
            assert!(matches!(
                p.resolve("fixture", &states(&input).unwrap()),
                Err(BlockError::Unsupported(_))
            ));
        }
        assert!(state_value(&json!(1.0)).is_err());
        assert!(matches!(
            p.resolve("missing", &vec![]),
            Err(BlockError::Operation(_))
        ));
        b["block_cases"][11]["success"] = json!(false);
        b["block_cases"][11]["error"] = json!("Error: rejected");
        let p = Palette::from_reports(&a, &b).unwrap();
        let base = p.resolve("fixture", &vec![]).unwrap();
        assert!(matches!(
            p.with_state(&base, "active", StateValue::Bool(true)),
            Err(BlockError::Operation(_))
        ));
        assert_eq!(
            p.resolve("fixture", &states(&json!({"active":true})).unwrap())
                .unwrap()
                .states()["active"],
            StateValue::Bool(true)
        );
    }
    #[test]
    fn completeness_conflicts_and_reference_identity_are_checked() {
        let (mut a, b) = fixture();
        a["permutations"].as_array_mut().unwrap().pop();
        a["completion_record"]["permutations"] = json!(7);
        assert!(Palette::from_reports(&a, &b).is_err());
        let (mut a, b) = fixture();
        a["permutations"][4]["tags"] = json!(["conflict"]);
        assert!(Palette::from_reports(&a, &b).is_err());
        let (a, mut b) = fixture();
        b["metadata"]["executable_sha256"] = json!("other");
        assert!(Palette::from_reports(&a, &b).is_err());
        let (a, mut b) = fixture();
        b["block_cases"].as_array_mut().unwrap().pop();
        b["completion_record"]["blockCases"] = json!(11);
        assert!(Palette::from_reports(&a, &b).is_err());
    }
    #[test]
    fn saved_property_order_is_preserved() {
        let (a, b) = fixture();
        let p = Palette::from_reports(&a, &b).unwrap();
        let saved = BlockState {
            raw_tag: None,
            name: "minecraft:fixture".into(),
            version: 1,
            states: states(&json!({"direction":1,"cardinal":"east","active":false}))
                .unwrap()
                .into_iter()
                .collect(),
            state_order: vec!["cardinal".into(), "direction".into(), "active".into()],
        };
        assert_eq!(
            p.resolve_saved(&saved).unwrap().states()["direction"],
            StateValue::Int(1)
        );
    }
}
impl fmt::Display for BlockError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Evidence(s) => write!(f, "Invalid palette evidence: {s}"),
            Self::Unsupported(s) => write!(f, "Unverified block input: {s}"),
            Self::Operation(s) => write!(f, "Error: {s}"),
        }
    }
}
impl std::error::Error for BlockError {}
fn invalid(s: &str) -> BlockError {
    BlockError::Evidence(s.into())
}
fn array<'a>(value: &'a Value, name: &str) -> BlockResult<&'a Vec<Value>> {
    value[name]
        .as_array()
        .ok_or_else(|| invalid(&format!("Missing array {name}")))
}
fn string<'a>(value: &'a Value, name: &str) -> BlockResult<&'a str> {
    value[name]
        .as_str()
        .ok_or_else(|| invalid(&format!("Missing string {name}")))
}
pub fn state_value(value: &Value) -> BlockResult<StateValue> {
    match value {
        Value::Bool(v) => Ok(StateValue::Bool(*v)),
        Value::String(v) => Ok(StateValue::String(v.clone())),
        Value::Number(v) => v
            .as_i64()
            .and_then(|v| i32::try_from(v).ok())
            .map(StateValue::Int)
            .ok_or_else(|| {
                BlockError::Unsupported("Only exact i32, boolean, and string states".into())
            }),
        _ => Err(BlockError::Unsupported(
            "Only exact i32, boolean, and string states".into(),
        )),
    }
}
pub fn states(value: &Value) -> BlockResult<States> {
    value
        .as_object()
        .ok_or_else(|| BlockError::Unsupported("States must be an object".into()))?
        .iter()
        .map(|(k, v)| Ok((k.clone(), state_value(v)?)))
        .collect()
}
fn canonical_key(id: &str, states: &States) -> BlockResult<Key> {
    let mut sorted = states.clone();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    if sorted.windows(2).any(|w| w[0].0 == w[1].0) {
        return Err(BlockError::Unsupported("Duplicate state name".into()));
    }
    Ok((id.into(), sorted))
}
fn check_done(report: &Value) -> BlockResult<()> {
    if report["complete"] != true
        || !array(report, "errors")?.is_empty()
        || report["completion_record"]["kind"] != "done"
    {
        return Err(invalid("Incomplete capture"));
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Permutation {
    id: String,
    states: BTreeMap<String, StateValue>,
    tags: BTreeSet<String>,
}
impl Permutation {
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn states(&self) -> &BTreeMap<String, StateValue> {
        &self.states
    }
    pub fn tags(&self) -> &BTreeSet<String> {
        &self.tags
    }
    fn ordered_states(&self) -> States {
        self.states
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }
    fn read(row: &Value) -> BlockResult<Self> {
        Ok(Self {
            id: string(row, "id")?.into(),
            states: states(&row["states"])?.into_iter().collect(),
            tags: array(row, "tags")?
                .iter()
                .map(|v| {
                    v.as_str()
                        .map(String::from)
                        .ok_or_else(|| invalid("Invalid tag"))
                })
                .collect::<BlockResult<_>>()?,
        })
    }
}

pub struct Palette {
    reference: String,
    domains: HashMap<String, BTreeSet<StateValue>>,
    defaults: HashMap<String, States>,
    entries: HashMap<Key, Arc<Permutation>>,
    canonical: HashMap<Key, Arc<Permutation>>,
    orders: HashMap<String, HashSet<Vec<String>>>,
    partial: HashMap<PartialKey, Arc<Permutation>>,
    setter: HashMap<PartialKey, Option<Arc<Permutation>>>,
    effects: Effects,
    setter_effects: Effects,
    canonical_count: usize,
}
impl Palette {
    pub fn reference(&self) -> &str {
        &self.reference
    }
    pub fn canonical_count(&self) -> usize {
        self.canonical_count
    }
    pub fn input_count(&self) -> usize {
        self.entries.len()
    }
    pub fn block_count(&self) -> usize {
        self.defaults.len()
    }
    pub fn from_reports(report: &Value, partial: &Value) -> BlockResult<Self> {
        check_done(report)?;
        check_done(partial)?;
        if report["completion_record"]["errors"] != 0 {
            return Err(invalid("Reference reported errors"));
        }
        for (field, count) in [
            ("blocks", "blocks"),
            ("block_states", "blockStates"),
            ("permutations", "permutations"),
        ] {
            if report["completion_record"][count].as_u64()
                != Some(array(report, field)?.len() as u64)
            {
                return Err(invalid("Capture count mismatch"));
            }
        }
        let reference = string(&report["metadata"], "executable_sha256")?.to_owned();
        if reference.is_empty() || partial["metadata"]["executable_sha256"] != reference {
            return Err(invalid("Reference executable mismatch"));
        }
        let mut domains = HashMap::new();
        for row in array(report, "block_states")? {
            let values = array(row, "values")?;
            let domain: BTreeSet<_> = values.iter().map(state_value).collect::<BlockResult<_>>()?;
            if domain.is_empty()
                || domain.len() != values.len()
                || domains
                    .insert(string(row, "id")?.to_owned(), domain)
                    .is_some()
            {
                return Err(invalid("Empty/duplicate state definition"));
            }
        }
        let mut defaults = HashMap::new();
        for row in array(report, "blocks")? {
            if defaults
                .insert(string(row, "id")?.to_owned(), states(&row["states"])?)
                .is_some()
            {
                return Err(invalid("Duplicate block definition"));
            }
        }
        let mut entries = HashMap::new();
        let mut canonical: HashMap<Key, Arc<Permutation>> = HashMap::new();
        let mut orders: HashMap<String, HashSet<Vec<String>>> = HashMap::new();
        for row in array(report, "permutations")? {
            if row["success"] != true {
                return Err(invalid("Rejected full permutation"));
            }
            let id = string(row, "inputId")?;
            let input = states(&row["inputStates"])?;
            let key = canonical_key(id, &input)?;
            orders
                .entry(id.into())
                .or_default()
                .insert(input.iter().map(|(k, _)| k.clone()).collect());
            let output = Permutation::read(row)?;
            let output_key = canonical_key(&output.id, &output.ordered_states())?;
            if canonical
                .get(&output_key)
                .is_some_and(|old| **old != output)
            {
                return Err(invalid("Conflicting canonical permutation"));
            }
            let resolved = canonical
                .entry(output_key)
                .or_insert_with(|| Arc::new(output))
                .clone();
            if entries.insert(key, resolved).is_some() {
                return Err(invalid("Duplicate input permutation"));
            }
        }
        // Checking cardinality plus every unique, in-domain key proves Cartesian coverage
        // without allocating another copy of the full Cartesian product.
        let mut expected = 0usize;
        for (id, default) in &defaults {
            let count = default.iter().try_fold(1usize, |n, (name, _)| {
                n.checked_mul(
                    domains
                        .get(name)
                        .ok_or_else(|| invalid("Missing state domain"))?
                        .len(),
                )
                .ok_or_else(|| invalid("Domain product overflow"))
            })?;
            expected = expected
                .checked_add(count)
                .ok_or_else(|| invalid("Palette count overflow"))?;
            if entries
                .get(&canonical_key(id, default)?)
                .is_none_or(|v| v.id != *id)
            {
                return Err(invalid("Default identity mismatch"));
            }
        }
        for (id, input) in entries.keys() {
            let default = defaults
                .get(id)
                .ok_or_else(|| invalid("Unknown input block"))?;
            if input.len() != default.len()
                || input.iter().any(|(name, value)| {
                    !default.iter().any(|(n, _)| n == name)
                        || !domains.get(name).is_some_and(|d| d.contains(value))
                })
            {
                return Err(invalid("Unexpected input combination"));
            }
        }
        if expected != entries.len() {
            return Err(invalid("Missing Cartesian combinations"));
        }
        let mut result = Self {
            reference,
            domains,
            defaults,
            entries,
            orders,
            canonical: HashMap::new(),
            partial: HashMap::new(),
            setter: HashMap::new(),
            effects: HashMap::new(),
            setter_effects: HashMap::new(),
            canonical_count: canonical.len(),
        };
        let cases = array(partial, "block_cases")?;
        if partial["completion_record"]["blockCases"].as_u64() != Some(cases.len() as u64) {
            return Err(invalid("Partial capture count mismatch"));
        }
        for row in cases {
            let id = string(row, "id")?;
            let input = states(&row["states"])?;
            if input.len() != 1 {
                return Err(invalid("Expected one partial property"));
            }
            let (name, value) = &input[0];
            let default = result
                .defaults
                .get(id)
                .ok_or_else(|| invalid("Unknown partial block"))?;
            if !default.iter().any(|(n, _)| n == name)
                || !result.domains.get(name).is_some_and(|d| d.contains(value))
            {
                return Err(invalid("Unexpected partial input"));
            }
            let setter = match string(row, "operation")? {
                "resolve" => false,
                "withState" => true,
                _ => return Err(invalid("Unknown partial operation")),
            };
            let key = (id.into(), name.clone(), value.clone());
            let transition = if row["success"] == true {
                let output = Permutation::read(&row["result"])?;
                let known = canonical
                    .get(&canonical_key(&output.id, &output.ordered_states())?)
                    .ok_or_else(|| invalid("Partial output absent from palette"))?;
                if **known != output {
                    return Err(invalid("Conflicting partial output"));
                }
                let effects = if setter {
                    &mut result.setter_effects
                } else {
                    &mut result.effects
                };
                let affected = effects.entry((id.into(), name.clone())).or_default();
                affected.insert(name.clone());
                for (k, v) in &output.states {
                    let base = default
                        .iter()
                        .find(|(n, _)| n == k)
                        .ok_or_else(|| invalid("Partial output has unknown property"))?;
                    if base.1 != *v {
                        affected.insert(k.clone());
                    }
                }
                Some(known.clone())
            } else if setter
                && row["success"] == false
                && string(row, "error")?.starts_with("Error:")
            {
                None
            } else {
                return Err(invalid("Unexpected partial failure"));
            };
            let duplicate = if setter {
                result.setter.insert(key, transition).is_some()
            } else {
                result
                    .partial
                    .insert(key, transition.ok_or_else(|| invalid("Resolve failed"))?)
                    .is_some()
            };
            if duplicate {
                return Err(invalid("Duplicate partial observation"));
            }
        }
        let mut expected_partial = 0;
        for (id, default) in &result.defaults {
            for (name, _) in default {
                for value in &result.domains[name] {
                    let key = (id.clone(), name.clone(), value.clone());
                    if !result.partial.contains_key(&key) || !result.setter.contains_key(&key) {
                        return Err(invalid("Missing partial observation"));
                    }
                    expected_partial += 1;
                }
            }
        }
        if result.partial.len() != expected_partial || result.setter.len() != expected_partial {
            return Err(invalid("Unexpected extra partial observations"));
        }
        result.canonical = canonical;
        Ok(result)
    }
    pub fn resolve(&self, id: &str, input: &States) -> BlockResult<Arc<Permutation>> {
        if id.is_empty() {
            return Err(BlockError::Unsupported("Empty block identifier".into()));
        }
        let id = if id.contains(':') {
            id.into()
        } else {
            format!("minecraft:{id}")
        };
        let base = self
            .defaults
            .get(&id)
            .ok_or_else(|| BlockError::Operation(format!("Unknown block {id}")))?;
        self.derive(&id, input, base, false)
    }
    pub fn with_state(
        &self,
        base: &Permutation,
        name: &str,
        value: StateValue,
    ) -> BlockResult<Arc<Permutation>> {
        let known = self
            .canonical
            .get(&canonical_key(&base.id, &base.ordered_states())?);
        if known.is_none_or(|known| **known != *base) {
            return Err(invalid("Permutation belongs to a different palette"));
        }
        self.derive(
            &base.id,
            &vec![(name.into(), value)],
            &base.ordered_states(),
            true,
        )
    }
    pub fn resolve_saved(&self, saved: &BlockState) -> BlockResult<Arc<Permutation>> {
        if saved.state_order.len() != saved.states.len() {
            return Err(invalid("Incomplete saved property order"));
        }
        let ordered = saved
            .state_order
            .iter()
            .map(|name| {
                saved
                    .states
                    .get(name)
                    .map(|v| (name.clone(), v.clone()))
                    .ok_or_else(|| invalid("Invalid saved property order"))
            })
            .collect::<BlockResult<_>>()?;
        self.resolve(&saved.name, &ordered)
    }
    fn derive(
        &self,
        id: &str,
        input: &States,
        base: &States,
        setter: bool,
    ) -> BlockResult<Arc<Permutation>> {
        canonical_key(id, input)?;
        let mut state: BTreeMap<_, _> = base.iter().cloned().collect();
        for (name, value) in input {
            if !state.contains_key(name) {
                return Err(BlockError::Unsupported("Unknown property".into()));
            }
            if !self.domains.get(name).is_some_and(|d| d.contains(value)) {
                return Err(BlockError::Unsupported(
                    "Out-of-domain state coercion".into(),
                ));
            }
        }
        if !setter && input.len() == 1 {
            let (name, value) = &input[0];
            return self
                .partial
                .get(&(id.into(), name.clone(), value.clone()))
                .cloned()
                .ok_or_else(|| invalid("Missing partial"));
        }
        let order: Vec<_> = input.iter().map(|(k, _)| k.clone()).collect();
        if !setter
            && input.len() == base.len()
            && self.orders.get(id).is_some_and(|o| o.contains(&order))
        {
            return self
                .entries
                .get(&canonical_key(id, input)?)
                .cloned()
                .ok_or_else(|| invalid("Missing full input"));
        }
        for (name, value) in input {
            let key = (id.into(), name.clone(), value.clone());
            let transition = if setter {
                self.setter
                    .get(&key)
                    .ok_or_else(|| invalid("Missing setter"))?
                    .as_ref()
                    .ok_or_else(|| {
                        BlockError::Operation("Reference rejects this state assignment".into())
                    })?
            } else {
                self.partial
                    .get(&key)
                    .ok_or_else(|| invalid("Missing partial"))?
            };
            let effects = if setter {
                &self.setter_effects
            } else {
                &self.effects
            };
            for affected in effects
                .get(&(id.into(), name.clone()))
                .ok_or_else(|| invalid("Missing effects"))?
            {
                state.insert(
                    affected.clone(),
                    transition
                        .states
                        .get(affected)
                        .ok_or_else(|| invalid("Missing affected state"))?
                        .clone(),
                );
            }
        }
        self.entries
            .get(&(id.into(), state.into_iter().collect()))
            .cloned()
            .ok_or_else(|| invalid("Missing derived input"))
    }
}
