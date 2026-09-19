//! Candidate recipe matching. Pack selection and inventory mutation remain open.
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

pub type TagItems = BTreeMap<String, BTreeSet<String>>;
pub type Grid = Vec<Vec<Option<Stack>>>;
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecipeError {
    Invalid(String),
    Unsupported(String),
}
impl fmt::Display for RecipeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(s) | Self::Unsupported(s) => f.write_str(s),
        }
    }
}
impl std::error::Error for RecipeError {}
type Result<T> = std::result::Result<T, RecipeError>;
fn invalid(s: &str) -> RecipeError {
    RecipeError::Invalid(s.into())
}
fn unsupported(s: &str) -> RecipeError {
    RecipeError::Unsupported(s.into())
}
fn identifier(value: &str) -> Result<String> {
    if value.is_empty() {
        return Err(invalid("Item identifier must be nonempty"));
    }
    Ok(if value.contains(':') {
        value.into()
    } else {
        format!("minecraft:{value}")
    })
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Stack {
    item: String,
    data: i64,
    count: u32,
}
impl Stack {
    pub fn new(item: &str, data: i64, count: u32) -> Result<Self> {
        if count == 0 {
            return Err(invalid("Stack count must be positive"));
        }
        Ok(Self {
            item: identifier(item)?,
            data,
            count,
        })
    }
}

#[derive(Clone)]
enum Ingredient {
    Item {
        id: String,
        data: i64,
    },
    Tag {
        members: BTreeSet<String>,
        data: Option<i64>,
    },
}
impl Ingredient {
    fn read(value: &Value, tags: &TagItems) -> Result<Self> {
        let obj = value
            .as_object()
            .ok_or_else(|| unsupported("Only object ingredients are implemented"))?;
        if obj
            .keys()
            .any(|key| !["item", "tag", "data", "count"].contains(&key.as_str()))
        {
            return Err(unsupported("Unknown ingredient fields"));
        }
        if obj.contains_key("item") == obj.contains_key("tag") {
            return Err(unsupported("Ingredient must specify either item or tag"));
        }
        if obj.get("count").is_some_and(|v| v.as_f64() != Some(1.0)) {
            return Err(unsupported(
                "Multi-count ingredient slot semantics are unverified",
            ));
        }
        let data = match obj.get("data") {
            None => 0,
            Some(v) => v.as_i64().ok_or_else(|| {
                unsupported("Legacy wildcard/invalid data value requires reference verification")
            })?,
        };
        if data < 0 || data == 32767 {
            return Err(unsupported(
                "Legacy wildcard/invalid data value requires reference verification",
            ));
        }
        if let Some(value) = obj.get("item") {
            let id = identifier(
                value
                    .as_str()
                    .ok_or_else(|| invalid("Invalid item identifier"))?,
            )?;
            if id.matches(':').count() != 1 {
                return Err(unsupported(
                    "Inline legacy item metadata requires normalization",
                ));
            }
            Ok(Self::Item { id, data })
        } else {
            let id = identifier(
                obj["tag"]
                    .as_str()
                    .ok_or_else(|| invalid("Invalid tag identifier"))?,
            )?;
            let members = tags
                .get(&id)
                .ok_or_else(|| RecipeError::Unsupported(format!("Unresolved item tag: {id}")))?
                .clone();
            Ok(Self::Tag {
                members,
                data: obj.contains_key("data").then_some(data),
            })
        }
    }
    fn matches(&self, stack: Option<&Stack>) -> bool {
        let Some(stack) = stack else {
            return false;
        };
        match self {
            Self::Item { id, data } => stack.item == *id && stack.data == *data,
            Self::Tag { members, data } => {
                members.contains(&stack.item) && data.is_none_or(|d| stack.data == d)
            }
        }
    }
    fn example(&self) -> Result<Stack> {
        match self {
            Self::Item { id, data } => Stack::new(id, *data, 1),
            Self::Tag { members, data } => Stack::new(
                members
                    .first()
                    .ok_or_else(|| invalid("Empty tag has no canonical input"))?,
                data.unwrap_or(0),
                1,
            ),
        }
    }
}
enum Pattern {
    Shaped {
        cells: Vec<Vec<Option<Ingredient>>>,
        symmetry: bool,
    },
    Shapeless(Vec<Ingredient>),
}
pub struct Recipe {
    id: String,
    kind: String,
    stations: Vec<String>,
    result: Value,
    pattern: Pattern,
}
impl Recipe {
    pub fn read(document: &Value, tags: &TagItems) -> Result<Self> {
        let obj = document
            .as_object()
            .ok_or_else(|| invalid("Expected recipe document object"))?;
        let kinds: Vec<_> = obj
            .keys()
            .filter(|s| s.starts_with("minecraft:recipe_"))
            .collect();
        if kinds.len() != 1 {
            return Err(invalid("Expected exactly one recipe definition"));
        }
        let kind = kinds[0].as_str();
        let shaped = match kind {
            "minecraft:recipe_shaped" => true,
            "minecraft:recipe_shapeless" => false,
            _ => {
                return Err(RecipeError::Unsupported(format!(
                    "Recipe family not implemented: {kind}"
                )));
            }
        };
        let body = obj[kind]
            .as_object()
            .ok_or_else(|| invalid("Expected recipe body object"))?;
        let common = [
            "description",
            "group",
            "priority",
            "result",
            "tags",
            "unlock",
        ];
        let specific = if shaped {
            &["key", "pattern", "assume_symmetry"][..]
        } else {
            &["ingredients"][..]
        };
        if body
            .keys()
            .any(|s| !common.contains(&s.as_str()) && !specific.contains(&s.as_str()))
        {
            return Err(unsupported("Unknown recipe fields"));
        }
        let id = body
            .get("description")
            .and_then(|v| v["identifier"].as_str())
            .ok_or_else(|| invalid("Missing recipe identifier"))?
            .to_string();
        let result = body
            .get("result")
            .ok_or_else(|| invalid("Missing recipe result"))?
            .clone();
        let stations = match body.get("tags") {
            None => Vec::new(),
            Some(Value::Array(values)) => values
                .iter()
                .map(|v| {
                    v.as_str()
                        .map(str::to_owned)
                        .ok_or_else(|| invalid("Invalid station tag"))
                })
                .collect::<Result<Vec<_>>>()?,
            _ => return Err(invalid("Invalid station tags")),
        };
        let pattern = if shaped {
            let keys = body
                .get("key")
                .and_then(Value::as_object)
                .ok_or_else(|| invalid("Missing pattern keys"))?;
            let mut ingredients = BTreeMap::new();
            for (symbol, value) in keys {
                ingredients.insert(symbol.clone(), Ingredient::read(value, tags)?);
            }
            let rows = body
                .get("pattern")
                .and_then(Value::as_array)
                .ok_or_else(|| invalid("Missing pattern"))?;
            let matrix: Vec<Vec<char>> = rows
                .iter()
                .map(|v| {
                    v.as_str()
                        .map(|s| s.chars().collect())
                        .ok_or_else(|| invalid("Pattern rows must be strings"))
                })
                .collect::<Result<_>>()?;
            let mut bounds: Option<(usize, usize, usize, usize)> = None;
            for (y, row) in matrix.iter().enumerate() {
                for (x, _) in row.iter().enumerate().filter(|(_, c)| **c != ' ') {
                    bounds = Some(match bounds {
                        None => (x, x, y, y),
                        Some((l, r, t, b)) => (l.min(x), r.max(x), t.min(y), b.max(y)),
                    });
                }
            }
            let (left, right, top, bottom) =
                bounds.ok_or_else(|| invalid("Pattern has no ingredients"))?;
            if right - left >= 3 || bottom - top >= 3 {
                return Err(unsupported("Patterns larger than 3x3 are not implemented"));
            }
            let mut cells = Vec::new();
            for row in &matrix[top..=bottom] {
                let mut line = Vec::new();
                for x in left..=right {
                    let c = row.get(x).copied().unwrap_or(' ');
                    line.push(if c == ' ' {
                        None
                    } else {
                        Some(
                            ingredients
                                .get(&c.to_string())
                                .ok_or_else(|| invalid("Missing pattern key"))?
                                .clone(),
                        )
                    });
                }
                cells.push(line);
            }
            Pattern::Shaped {
                cells,
                symmetry: body.get("assume_symmetry") != Some(&Value::Bool(false)),
            }
        } else {
            let rows = body
                .get("ingredients")
                .and_then(Value::as_array)
                .ok_or_else(|| invalid("Missing ingredients"))?;
            if rows.is_empty() || rows.len() > 9 {
                return Err(unsupported("Shapeless ingredient count must be 1..9"));
            }
            Pattern::Shapeless(
                rows.iter()
                    .map(|v| Ingredient::read(v, tags))
                    .collect::<Result<_>>()?,
            )
        };
        Ok(Self {
            id,
            kind: kind.into(),
            stations,
            result,
            pattern,
        })
    }
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn kind(&self) -> &str {
        &self.kind
    }
    pub fn result(&self) -> &Value {
        &self.result
    }
    pub fn stations(&self) -> &[String] {
        &self.stations
    }
    pub fn symmetry(&self) -> Option<bool> {
        match &self.pattern {
            Pattern::Shaped { symmetry, .. } => Some(*symmetry),
            _ => None,
        }
    }
    pub fn canonical_grid(&self) -> Result<Grid> {
        match &self.pattern {
            Pattern::Shaped { cells, .. } => cells
                .iter()
                .map(|row| {
                    row.iter()
                        .map(|v| v.as_ref().map(Ingredient::example).transpose())
                        .collect()
                })
                .collect(),
            Pattern::Shapeless(ingredients) => {
                let mut slots = ingredients
                    .iter()
                    .map(|v| v.example().map(Some))
                    .collect::<Result<Vec<_>>>()?;
                slots.resize(9, None);
                Ok(slots.chunks(3).map(<[_]>::to_vec).collect())
            }
        }
    }
    pub fn matches(&self, grid: &[Vec<Option<Stack>>], station: &str) -> Result<bool> {
        let width = grid.first().map_or(0, Vec::len);
        if grid.is_empty()
            || width == 0
            || grid.len() > 3
            || width > 3
            || grid.iter().any(|row| row.len() != width)
        {
            return Err(invalid("Grid must be rectangular and fit in 3x3"));
        }
        if !self.stations.iter().any(|s| s == station) {
            return Ok(false);
        }
        match &self.pattern {
            Pattern::Shapeless(ingredients) => {
                let slots: Vec<_> = grid.iter().flatten().filter_map(Option::as_ref).collect();
                if slots.len() != ingredients.len() {
                    return Ok(false);
                }
                let mut candidates: Vec<Vec<usize>> = ingredients
                    .iter()
                    .map(|v| {
                        slots
                            .iter()
                            .enumerate()
                            .filter_map(|(i, s)| v.matches(Some(s)).then_some(i))
                            .collect()
                    })
                    .collect();
                candidates.sort_by_key(Vec::len);
                fn assign(candidates: &[Vec<usize>], used: u16) -> bool {
                    let Some((first, rest)) = candidates.split_first() else {
                        return true;
                    };
                    first
                        .iter()
                        .any(|i| used & (1 << i) == 0 && assign(rest, used | (1 << i)))
                }
                Ok(assign(&candidates, 0))
            }
            Pattern::Shaped { cells, symmetry } => {
                let h = cells.len();
                let w = cells[0].len();
                if h > grid.len() || w > width {
                    return Ok(false);
                }
                for flip in 0..=usize::from(*symmetry) {
                    for oy in 0..=grid.len() - h {
                        for ox in 0..=width - w {
                            let matched = grid.iter().enumerate().all(|(y, row)| {
                                row.iter().enumerate().all(|(x, stack)| {
                                    let ingredient =
                                        if x >= ox && x < ox + w && y >= oy && y < oy + h {
                                            cells[y - oy]
                                                [if flip == 1 { w - 1 - (x - ox) } else { x - ox }]
                                            .as_ref()
                                        } else {
                                            None
                                        };
                                    ingredient.map_or_else(
                                        || stack.is_none(),
                                        |v| v.matches(stack.as_ref()),
                                    )
                                })
                            });
                            if matched {
                                return Ok(true);
                            }
                        }
                    }
                }
                Ok(false)
            }
        }
    }
}

pub fn grid_from_json(value: &Value) -> Result<Grid> {
    let rows = value
        .as_array()
        .ok_or_else(|| invalid("Grid must be an array"))?;
    rows.iter()
        .map(|row| {
            row.as_array()
                .ok_or_else(|| invalid("Grid row must be an array"))?
                .iter()
                .map(|cell| {
                    if cell.is_null() {
                        return Ok(None);
                    }
                    let obj = cell
                        .as_object()
                        .ok_or_else(|| invalid("Grid cell must be stack object or null"))?;
                    if obj
                        .keys()
                        .any(|s| !["item", "data", "count"].contains(&s.as_str()))
                    {
                        return Err(invalid("Unknown stack fields"));
                    }
                    let item = obj
                        .get("item")
                        .and_then(Value::as_str)
                        .ok_or_else(|| invalid("Missing stack item"))?;
                    let data = match obj.get("data") {
                        None => 0,
                        Some(v) => v
                            .as_i64()
                            .ok_or_else(|| invalid("Stack data must be integer"))?,
                    };
                    let count = match obj.get("count") {
                        None => 1,
                        Some(v) => v
                            .as_u64()
                            .and_then(|n| u32::try_from(n).ok())
                            .ok_or_else(|| invalid("Stack count must be positive integer"))?,
                    };
                    Stack::new(item, data, count).map(Some)
                })
                .collect()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn shaped(pattern: Value, symmetry: Value) -> Value {
        json!({"minecraft:recipe_shaped":{"description":{"identifier":"test:tool"},"tags":["crafting_table"],"pattern":pattern,"key":{"A":{"item":"test:a"},"B":{"item":"test:b"}},"assume_symmetry":symmetry,"result":{"item":"test:tool"}}})
    }
    fn shapeless(ingredients: Value) -> Value {
        json!({"minecraft:recipe_shapeless":{"description":{"identifier":"test:mixed"},"tags":["crafting_table"],"ingredients":ingredients,"result":{"item":"test:mixed"}}})
    }
    fn grid(v: Value) -> Grid {
        grid_from_json(&v).unwrap()
    }
    #[test]
    fn translation_gaps_and_extra_items() {
        let r = Recipe::read(
            &shaped(json!([" A ", "A A"]), json!(true)),
            &TagItems::new(),
        )
        .unwrap();
        let g =
            grid(json!([[null,{"item":"test:a"},null],[{"item":"test:a"},null,{"item":"test:a"}]]));
        assert!(r.matches(&g, "crafting_table").unwrap());
        assert!(!r.matches(&g, "furnace").unwrap());
        let g = grid(
            json!([[null,{"item":"test:a"},null],[{"item":"test:a"},{"item":"test:a"},{"item":"test:a"}]]),
        );
        assert!(!r.matches(&g, "crafting_table").unwrap());
        let r = Recipe::read(
            &shaped(json!(["  ", " A", " B"]), json!(true)),
            &TagItems::new(),
        )
        .unwrap();
        assert!(r.matches(&grid(json!([[null,null,null],[null,{"item":"test:a"},null],[null,{"item":"test:b"},null]])),"crafting_table").unwrap());
    }
    #[test]
    fn symmetry_is_boolean_only_and_does_not_rotate() {
        let mirrored = grid(json!([[{"item":"test:b"},{"item":"test:a"}]]));
        for flag in [
            Value::Null,
            json!(0),
            json!(""),
            json!([]),
            json!({}),
            json!("false"),
            json!(true),
        ] {
            let r = Recipe::read(&shaped(json!(["AB"]), flag), &TagItems::new()).unwrap();
            assert!(r.matches(&mirrored, "crafting_table").unwrap());
            assert!(
                !r.matches(
                    &grid(json!([[{"item":"test:a"}],[{"item":"test:b"}]])),
                    "crafting_table"
                )
                .unwrap()
            );
        }
        let r = Recipe::read(&shaped(json!(["AB"]), json!(false)), &TagItems::new()).unwrap();
        assert!(!r.matches(&mirrored, "crafting_table").unwrap());
    }
    #[test]
    fn shapeless_backtracks_and_uses_distinct_slots() {
        let tags = TagItems::from([(
            "test:wood".into(),
            BTreeSet::from(["test:oak".into(), "test:birch".into()]),
        )]);
        let r = Recipe::read(
            &shapeless(json!([{"tag":"test:wood"},{"item":"test:oak"}])),
            &tags,
        )
        .unwrap();
        assert!(
            r.matches(
                &grid(json!([[{"item":"test:oak"},{"item":"test:birch"}]])),
                "crafting_table"
            )
            .unwrap()
        );
        assert!(
            !r.matches(
                &grid(json!([[{"item":"test:birch"},{"item":"test:birch"}]])),
                "crafting_table"
            )
            .unwrap()
        );
        let r = Recipe::read(
            &shapeless(json!([{"item":"test:oak"},{"item":"test:oak"}])),
            &tags,
        )
        .unwrap();
        assert!(
            !r.matches(
                &grid(json!([[{"item":"test:oak","count":64}]])),
                "crafting_table"
            )
            .unwrap()
        );
    }
    #[test]
    fn metadata_and_explicit_unsupported_semantics() {
        let r = Recipe::read(
            &shapeless(json!([{"item":"stone","data":2}])),
            &TagItems::new(),
        )
        .unwrap();
        assert!(
            r.matches(
                &grid(json!([[{"item":"minecraft:stone","data":2}]])),
                "crafting_table"
            )
            .unwrap()
        );
        assert!(
            !r.matches(
                &grid(json!([[{"item":"stone","data":1}]])),
                "crafting_table"
            )
            .unwrap()
        );
        for ingredient in [
            json!({"tag":"missing"}),
            json!({"item":"x","count":6}),
            json!({"item":"x","data":32767}),
            json!({"item":"x","components":{}}),
            json!({"item":"minecraft:stone:2"}),
        ] {
            assert!(matches!(
                Recipe::read(&shapeless(json!([ingredient])), &TagItems::new()),
                Err(RecipeError::Unsupported(_))
            ));
        }
    }
    #[test]
    fn invalid_patterns_and_grids_fail() {
        assert!(Recipe::read(&shaped(json!(["Q"]), json!(true)), &TagItems::new()).is_err());
        let r = Recipe::read(&shaped(json!(["A"]), json!(true)), &TagItems::new()).unwrap();
        assert!(
            r.matches(&[vec![None], vec![None, None]], "crafting_table")
                .is_err()
        );
        assert!(grid_from_json(&json!([[{"item":"stone","count":0}]])).is_err());
        assert!(Recipe::read(&shaped(json!(["    "]), json!(true)), &TagItems::new()).is_err());
    }
}
