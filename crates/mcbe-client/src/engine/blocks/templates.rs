//! Deterministic deep overrides: objects merge, arrays/scalars replace, null clears.
use serde_json::Value;
pub fn merge(base: &mut Value, overrides: Value) {
    match (base, overrides) {
        (Value::Object(a), Value::Object(b)) => {
            for (k, v) in b {
                if let Some(old) = a.get_mut(&k) {
                    merge(old, v)
                } else {
                    a.insert(k, v);
                }
            }
        }
        (a, b) => *a = b,
    }
}
