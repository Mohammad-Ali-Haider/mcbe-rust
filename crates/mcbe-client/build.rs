// Embed the same authoring files used by live reload; no second block catalog.
use std::{env, fs, path::Path};
fn visit(root: &Path, dir: &Path, out: &mut String) {
    let mut paths: Vec<_> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    paths.sort();
    for p in paths {
        if p.is_dir() {
            visit(root, &p, out);
        } else if p.extension().is_some_and(|e| e == "json") {
            let name = p
                .strip_prefix(root)
                .unwrap()
                .to_str()
                .unwrap()
                .replace('\\', "/");
            out.push_str(&format!(
                "({name:?}, include_bytes!({:?})),\n",
                p.to_str().unwrap()
            ));
        }
    }
}
fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../content/base")
        .canonicalize()
        .unwrap();
    println!("cargo:rerun-if-changed={}", root.display());
    let mut out = String::from("static EMBEDDED: &[(&str, &[u8])] = &[\n");
    visit(&root, &root, &mut out);
    out.push_str("];\n");
    fs::write(
        Path::new(&env::var("OUT_DIR").unwrap()).join("builtin_content.rs"),
        out,
    )
    .unwrap();
}
