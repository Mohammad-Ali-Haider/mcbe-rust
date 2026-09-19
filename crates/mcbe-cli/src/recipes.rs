use crate::{REFERENCE, Result};
use mcbe_core::{
    items::Registry,
    recipes::{Recipe, RecipeError, TagItems, grid_from_json},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

fn read(path: &Path) -> Result<Vec<u8>> {
    if fs::metadata(path)?.len() > 8 * 1024 * 1024 {
        return Err("Recipe input exceeds 8 MiB".into());
    }
    Ok(fs::read(path)?)
}
fn registry(path: &str) -> Result<Registry> {
    let r = Registry::from_bytes(&read(Path::new(path))?)?;
    if r.metadata()["executable_sha256"] != REFERENCE {
        return Err("Known reference registry required".into());
    }
    Ok(r)
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub fn inspect(args: &[String]) -> Result<bool> {
    if !(4..=5).contains(&args.len()) {
        return Err("recipe REGISTRY DEFINITION GRID_JSON [STATION]".into());
    }
    let registry = registry(&args[1])?;
    let bytes = read(Path::new(&args[2]))?;
    let document: Value = serde_json::from_slice(&bytes)?;
    let recipe = Recipe::read(&document, &registry.tag_items())?;
    let grid = grid_from_json(&serde_json::from_str::<Value>(&args[3])?)?;
    let station = args.get(4).map_or("crafting_table", String::as_str);
    let matches = recipe.matches(&grid, station)?;
    println!(
        "{}",
        serde_json::to_string_pretty(
            &json!({"recipe":recipe.id(),"kind":recipe.kind(),"station":station,"matches":matches,"declared_result":recipe.result(),"grid":grid,"definition_sha256":hash(&bytes),"registry_sha256":registry.source_id(),"scope":"Candidate matching; no inventory consumption or pack selection"})
        )?
    );
    Ok(true)
}

fn walk(root: &Path, depth: usize, files: &mut Vec<PathBuf>) -> Result<()> {
    let metadata = fs::symlink_metadata(root)?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err("Reparse points are not accepted in definition trees".into());
        }
    }
    if metadata.file_type().is_symlink() || depth > 64 || files.len() > 10000 {
        return Err("Definition tree exceeds bounds or contains a link".into());
    }
    if metadata.is_dir() {
        for entry in fs::read_dir(root)? {
            walk(&entry?.path(), depth + 1, files)?;
        }
    } else if root.extension().is_some_and(|s| s == "json") {
        files.push(root.to_path_buf());
    }
    Ok(())
}
pub fn verify(args: &[String]) -> Result<bool> {
    if args.len() != 4 {
        return Err("verify-recipes REGISTRY DEFINITION_ROOT EXPECTED_REPORT".into());
    }
    let registry = registry(&args[1])?;
    let tags = registry.tag_items();
    let root = Path::new(&args[2]);
    let expected_bytes = read(Path::new(&args[3]))?;
    let expected: Value = serde_json::from_slice(&expected_bytes)?;
    if expected["failures"]
        .as_array()
        .is_none_or(|v| !v.is_empty())
        || expected["registry_executable_sha256"] != REFERENCE
    {
        return Err("Successful definition report with known reference required".into());
    }
    let mut files = Vec::new();
    walk(root, 0, &mut files)?;
    files.sort();
    if files.is_empty() {
        return Err("Empty definition tree".into());
    }
    let mut hashes = BTreeMap::new();
    let mut exclusions: BTreeMap<String, usize> = BTreeMap::new();
    let mut failures = Vec::new();
    let mut supported = 0usize;
    let mut passed = 0usize;
    for path in &files {
        let bytes = read(path)?;
        hashes.insert(
            path.strip_prefix(root)?
                .to_string_lossy()
                .replace('\\', "/"),
            hash(&bytes),
        );
        let document: Value = serde_json::from_slice(&bytes)?;
        let recipe = match Recipe::read(&document, &tags) {
            Ok(recipe) => recipe,
            Err(RecipeError::Unsupported(reason)) => {
                *exclusions.entry(reason).or_default() += 1;
                continue;
            }
            Err(error) => {
                failures.push(json!({"path":path,"error":error.to_string()}));
                continue;
            }
        };
        supported += 1;
        let grid = recipe.canonical_grid()?;
        let station = recipe
            .stations()
            .first()
            .map_or("crafting_table", String::as_str);
        if !recipe.matches(&grid, station)? || recipe.matches(&vec![vec![None; 3]; 3], station)? {
            failures.push(json!({"path":path,"error":"Canonical or empty-grid check failed"}));
        } else {
            passed += 1;
        }
    }
    let counts = json!({"definitions":files.len(),"supported_definitions":supported,"canonical_and_empty_checks_passed":passed});
    let expected_matches = counts == expected["counts"]
        && serde_json::to_value(&exclusions)? == expected["exclusions"];
    let unchanged = files.iter().all(|path| {
        let key = path
            .strip_prefix(root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        read(path).is_ok_and(|bytes| hashes.get(&key) == Some(&hash(&bytes)))
    });
    let complete = failures.is_empty() && expected_matches && unchanged;
    println!(
        "{}",
        serde_json::to_string_pretty(
            &json!({"complete":complete,"counts":counts,"exclusions":exclusions,"failures":failures,"matches_prior_report":expected_matches,"inputs_unchanged":unchanged,"definition_hashes":hashes,"expected_report_sha256":hash(&expected_bytes),"registry_sha256":registry.source_id(),"scope":"Definition-derived self-consistency; not live crafting parity or active pack selection"})
        )?
    );
    Ok(complete)
}

pub fn symmetry(args: &[String]) -> Result<bool> {
    if args.len() != 2 {
        return Err("verify-recipe-symmetry NATIVE_EVIDENCE".into());
    }
    let bytes = read(Path::new(&args[1]))?;
    let evidence: Value = serde_json::from_slice(&bytes)?;
    if evidence["executable_sha256"] != REFERENCE {
        return Err("Known native reference evidence required".into());
    }
    let cases = evidence["cases"]
        .as_array()
        .ok_or("Missing native symmetry cases")?;
    if cases.is_empty() {
        return Err("Empty symmetry evidence".into());
    }
    let mut failures = Vec::new();
    for case in cases {
        let mut document = json!({"minecraft:recipe_shaped":{"description":{"identifier":"test:symmetry"},"tags":["crafting_table"],"pattern":["AB"],"key":{"A":{"item":"test:a"},"B":{"item":"test:b"}},"result":{"item":"test:result"}}});
        if case["absent"] != true {
            document["minecraft:recipe_shaped"]["assume_symmetry"] = case["input"].clone();
        }
        let recipe = Recipe::read(&document, &TagItems::new())?;
        let flag = recipe.symmetry().ok_or("Shaped flag missing")?;
        let native = case["native_flag"]
            .as_bool()
            .ok_or("Missing native Boolean")?;
        let grid = grid_from_json(&json!([[{"item":"test:b"},{"item":"test:a"}]]))?;
        let mirrored = recipe.matches(&grid, "crafting_table")?;
        if flag != native || mirrored != native {
            failures.push(json!({"label":case["label"],"native_flag":native,"rust_flag":flag,"mirrored_match":mirrored}));
        }
    }
    let complete = failures.is_empty();
    println!(
        "{}",
        serde_json::to_string_pretty(
            &json!({"complete":complete,"cases":cases.len(),"failures":failures,"evidence_sha256":hash(&bytes),"scope":"Replay of bounded native flag-path evidence with stubbed lookup; not whole-recipe schema or live crafting"})
        )?
    );
    Ok(complete)
}
