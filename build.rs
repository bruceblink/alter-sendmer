use serde_json::Value;
use std::{
    env, fs,
    path::{Path, PathBuf},
};

fn flatten(value: &Value, prefix: &str, entries: &mut Vec<(String, String)>) {
    match value {
        Value::Object(object) => {
            for (key, value) in object {
                let full_key = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                flatten(value, &full_key, entries);
            }
        }
        Value::String(value) => entries.push((prefix.to_owned(), value.clone())),
        _ => {}
    }
}

fn locale_source_dir(manifest_dir: &Path) -> PathBuf {
    manifest_dir.join("locales")
}

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let locale_dir = locale_source_dir(&manifest_dir);
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("out dir"));
    let output_path = out_dir.join("locale_data.rs");

    println!("cargo:rerun-if-changed={}", locale_dir.display());
    let mut output = String::from(
        "pub fn lookup(locale: &str, key: &str) -> Option<&'static str> {\n    match locale {\n",
    );

    let mut files = walk_json_files(&locale_dir);
    files.sort_by(|left, right| left.0.cmp(&right.0));

    // English is the canonical fallback. Merge its keys into every generated
    // locale so the binary always has a complete user-facing catalog.
    let english_entries = files
        .iter()
        .find(|(locale, _)| locale == "en")
        .map(|(_, path)| read_entries(path))
        .unwrap_or_default();

    for (locale, path) in files {
        println!("cargo:rerun-if-changed={}", path.display());
        let mut entries = read_entries(&path);
        let known = entries
            .iter()
            .map(|(key, _)| key.clone())
            .collect::<std::collections::HashSet<_>>();
        entries.extend(
            english_entries
                .iter()
                .filter(|(key, _)| !known.contains(key))
                .cloned(),
        );
        entries.sort_by(|left, right| left.0.cmp(&right.0));

        output.push_str(&format!("        {locale:?} => match key {{\n"));
        for (key, value) in entries {
            output.push_str(&format!("            {key:?} => Some({value:?}),\n"));
        }
        output.push_str("            _ => None,\n        },\n");
    }

    output.push_str("        _ => None,\n    }\n}\n");
    fs::write(output_path, output).expect("write generated locale table");
}

fn read_entries(path: &Path) -> Vec<(String, String)> {
    let contents = fs::read_to_string(path).expect("read locale file");
    let value: Value = serde_json::from_str(&contents).expect("parse locale file");
    let mut entries = Vec::new();
    flatten(&value, "", &mut entries);
    entries
}

fn walk_json_files(locale_dir: &Path) -> Vec<(String, PathBuf)> {
    let mut files = Vec::new();
    let Ok(entries) = fs::read_dir(locale_dir) else {
        return files;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let locale = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("en")
                .to_owned();
            if let Ok(children) = fs::read_dir(path) {
                for child in children.flatten() {
                    let child_path = child.path();
                    if child_path.extension().is_some_and(|ext| ext == "json") {
                        files.push((locale.clone(), child_path));
                    }
                }
            }
        } else if path.extension().is_some_and(|ext| ext == "json") {
            let locale = path
                .file_stem()
                .and_then(|name| name.to_str())
                .unwrap_or("en")
                .to_owned();
            files.push((locale, path));
        }
    }
    files
}
