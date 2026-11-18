// The project's generated-file map, read by plan check, gate-list, the worker
// view and plan request add so they agree on which outputs an input edit implies.
#![allow(dead_code)]

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use toml::{Table, Value};

use crate::tasks::schema::normal_path;

pub(crate) const CONFIG: &str = ".claude/generated.toml";

const KEYS: [&str; 5] = ["id", "inputs", "outputs", "regenerate", "check"];

pub(crate) struct Generator {
    pub(crate) id: String,
    pub(crate) input_globs: Vec<String>,
    pub(crate) inputs: GlobSet,
    pub(crate) outputs: Vec<String>,
    pub(crate) regenerate: String,
    pub(crate) check: String,
}

/// The generators declared under `root`. Empty with no config; `Err` names the
/// generator id and key on a malformed table.
pub(crate) fn load(root: &Path) -> Result<Vec<Generator>, String> {
    let text = match fs::read_to_string(root.join(CONFIG)) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.to_string()),
    };
    parse(&text)
}

fn parse(text: &str) -> Result<Vec<Generator>, String> {
    let table: Table = toml::from_str(text)
        .map_err(|e| e.to_string().lines().next().unwrap_or_default().to_string())?;
    let Some(raw) = table.get("generators") else {
        return Ok(Vec::new());
    };
    let entries = raw
        .as_array()
        .ok_or_else(|| "`generators` must be an array of tables".to_string())?;
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        let table = entry
            .as_table()
            .ok_or_else(|| format!("generators[{index}] must be a table"))?;
        let id = text_key(table, "id", &format!("generators[{index}]"))?;
        if !seen.insert(id.clone()) {
            return Err(format!("generator {id:?}: duplicate id"));
        }
        for key in table.keys() {
            if !KEYS.contains(&key.as_str()) {
                return Err(format!("generator {id:?}: unknown key `{key}`"));
            }
        }
        let input_globs = list_key(table, "inputs", &id)?;
        let outputs = list_key(table, "outputs", &id)?;
        if let Some(bad) = outputs.iter().find(|o| o.contains(['*', '?', '[', '{'])) {
            return Err(format!(
                "generator {id:?}: `outputs` entry {bad:?} must be a literal path, not a glob"
            ));
        }
        let regenerate = text_key(table, "regenerate", &id)?;
        let check = text_key(table, "check", &id)?;
        let mut builder = GlobSetBuilder::new();
        for glob in &input_globs {
            let built = GlobBuilder::new(glob.trim_start_matches("./"))
                .literal_separator(true)
                .case_insensitive(true)
                .build()
                .map_err(|e| format!("generator {id:?}: `inputs` glob {glob:?}: {e}"))?;
            builder.add(built);
        }
        let inputs = builder
            .build()
            .map_err(|e| format!("generator {id:?}: `inputs`: {e}"))?;
        out.push(Generator {
            id,
            input_globs,
            inputs,
            outputs: outputs.iter().map(|o| normal_path(o)).collect(),
            regenerate,
            check,
        });
    }
    Ok(out)
}

fn text_key(table: &Table, key: &str, who: &str) -> Result<String, String> {
    match table.get(key).and_then(Value::as_str).map(str::trim) {
        Some(text) if !text.is_empty() => Ok(text.to_string()),
        _ => Err(format!("generator {who}: `{key}` is missing or blank")),
    }
}

fn list_key(table: &Table, key: &str, id: &str) -> Result<Vec<String>, String> {
    let blank = || format!("generator {id:?}: `{key}` must be a non-empty array of strings");
    let items = table
        .get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("generator {id:?}: `{key}` is missing"))?;
    let mut out = Vec::new();
    for item in items {
        match item.as_str().map(str::trim) {
            Some(text) if !text.is_empty() => out.push(text.to_string()),
            _ => return Err(blank()),
        }
    }
    if out.is_empty() {
        return Err(blank());
    }
    Ok(out)
}

/// The generators with an input glob matching any of `files`.
pub(crate) fn touched<'a>(generators: &'a [Generator], files: &[String]) -> Vec<&'a Generator> {
    let files: Vec<String> = files.iter().map(|f| normal_path(f)).collect();
    generators
        .iter()
        .filter(|g| files.iter().any(|f| g.inputs.is_match(f)))
        .collect()
}

/// The outputs of every generator `files` touches.
pub(crate) fn outputs_of(generators: &[Generator], files: &[String]) -> BTreeSet<String> {
    touched(generators, files)
        .into_iter()
        .flat_map(|g| g.outputs.iter().cloned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"
[[generators]]
id = "tokens"
inputs = ["src/tokens.css"]
outputs = ["src/tokens.gen.ts"]
regenerate = "bun run tokens"
check = "bun run tokens --check"
"#;

    fn files(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn matching_input_returns_outputs() {
        let gens = parse(GOOD).unwrap();
        let outputs = outputs_of(&gens, &files(&["./src/tokens.css"]));
        assert_eq!(
            outputs.into_iter().collect::<Vec<_>>(),
            ["src/tokens.gen.ts"]
        );
        assert!(outputs_of(&gens, &files(&["src/other.css"])).is_empty());
    }

    #[test]
    fn case_variant_input_still_matches() {
        let gens = parse(GOOD).unwrap();
        assert_eq!(touched(&gens, &files(&["src/Tokens.CSS"])).len(), 1);
    }

    #[test]
    fn missing_config_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load(dir.path()).unwrap().is_empty());
    }

    #[test]
    fn malformed_configs_name_the_generator() {
        let missing = GOOD.replace("check = \"bun run tokens --check\"\n", "");
        let dup = format!("{GOOD}{GOOD}");
        let glob = GOOD.replace("src/tokens.gen.ts", "src/*.gen.ts");
        let unknown = format!("{GOOD}extra = 1\n");
        for (text, needle) in [
            (missing, "`check`"),
            (dup, "duplicate id"),
            (glob, "literal path"),
            (unknown, "unknown key `extra`"),
        ] {
            let err = parse(&text).err().expect("malformed config is refused");
            assert!(err.contains("tokens") && err.contains(needle), "{err}");
        }
    }
}
