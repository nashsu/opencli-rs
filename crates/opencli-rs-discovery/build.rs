use std::env;
use std::fs;
use std::path::Path;

fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let adapters_dir = Path::new(&manifest_dir)
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("adapters");
    let out_dir = env::var("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("builtin_adapters.rs");

    let mut entries = vec![];

    if adapters_dir.exists() {
        collect_yaml_files(&adapters_dir, &adapters_dir, &mut entries);
    }

    // Sort for deterministic output
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    let mut code = String::from("pub const BUILTIN_ADAPTERS: &[(&str, &str)] = &[\n");
    for (rel_path, abs_path) in &entries {
        // Normalize path separators to forward slash for cross-platform compatibility.
        // Windows uses backslash which Rust interprets as escape sequences in string literals.
        code.push_str(&format!(
            "    (\"{}\", include_str!(\"{}\")),\n",
            rel_path.replace('\\', "/"),
            abs_path.replace('\\', "/")
        ));
    }
    code.push_str("];\n");

    fs::write(&dest_path, code).unwrap();

    // Tell cargo to rerun if adapters change
    println!("cargo:rerun-if-changed={}", adapters_dir.display());
    for (_, abs_path) in &entries {
        println!("cargo:rerun-if-changed={}", abs_path);
    }
}

fn collect_yaml_files(base: &Path, dir: &Path, entries: &mut Vec<(String, String)>) {
    if let Ok(read_dir) = fs::read_dir(dir) {
        for entry in read_dir.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_yaml_files(base, &path, entries);
            } else if path.extension().is_some_and(|e| e == "yaml" || e == "yml") {
                let rel = path
                    .strip_prefix(base)
                    .unwrap()
                    .to_string_lossy()
                    .to_string();
                let abs = path.to_string_lossy().to_string();
                entries.push((rel, abs));
            }
        }
    }
}
