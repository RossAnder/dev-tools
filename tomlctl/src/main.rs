use anyhow::{Context, Result, anyhow, bail};
use clap::{Parser, Subcommand, ValueEnum};
use serde_json::Value as JsonValue;
use std::fs;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use toml::Value as TomlValue;

#[derive(Parser)]
#[command(
    name = "tomlctl",
    version,
    about = "Read and write TOML files used by Claude Code flows and ledgers"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Parse a TOML file and print the whole document as JSON.
    Parse { file: PathBuf },

    /// Print the value at a dotted key path as JSON (or the whole doc if path is omitted).
    Get {
        file: PathBuf,
        /// Dotted path, e.g. "tasks.total" or "artifacts.optimise_findings". Omit to dump whole file.
        path: Option<String>,
    },

    /// Set a scalar at a dotted key path. Type auto-inferred; override with --type.
    Set {
        file: PathBuf,
        path: String,
        value: String,
        #[arg(long = "type", value_enum)]
        ty: Option<ScalarType>,
    },

    /// Set a JSON-encoded value (array, object, or scalar) at a dotted key path.
    SetJson {
        file: PathBuf,
        path: String,
        #[arg(long)]
        json: String,
    },

    /// Parse-check only. Exit 0 on valid TOML, non-zero otherwise.
    Validate { file: PathBuf },

    /// Operations on [[items]] arrays-of-tables (ledger schema).
    Items {
        #[command(subcommand)]
        op: ItemsOp,
    },
}

#[derive(Subcommand)]
enum ItemsOp {
    /// List items as a JSON array. Optionally filter by status.
    List {
        file: PathBuf,
        #[arg(long)]
        status: Option<String>,
    },

    /// Get a single item by its `id` field.
    Get { file: PathBuf, id: String },

    /// Append a new item. --json is the JSON object payload.
    Add {
        file: PathBuf,
        #[arg(long)]
        json: String,
    },

    /// Merge fields into an existing item (matched by `id`). --json is a patch object.
    Update {
        file: PathBuf,
        id: String,
        #[arg(long)]
        json: String,
    },

    /// Remove an item by id. Fails if no such id exists.
    Remove { file: PathBuf, id: String },

    /// Print the next id string for the given prefix (default R).
    NextId {
        file: PathBuf,
        #[arg(long, default_value = "R")]
        prefix: String,
    },

    /// Apply a batch of add/update/remove operations in a single file rewrite.
    Apply {
        file: PathBuf,
        #[arg(long)]
        ops: String,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum ScalarType {
    Str,
    Int,
    Float,
    Bool,
    Date,
    Datetime,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Parse { file } => {
            let doc = read_toml(&file)?;
            print_json(&toml_to_json(&doc))?;
        }
        Cmd::Get { file, path } => {
            let doc = read_toml(&file)?;
            let out = match path.as_deref() {
                None | Some("") => toml_to_json(&doc),
                Some(p) => toml_to_json(
                    navigate(&doc, p).ok_or_else(|| anyhow!("key path `{}` not found", p))?,
                ),
            };
            print_json(&out)?;
        }
        Cmd::Set {
            file,
            path,
            value,
            ty,
        } => {
            with_exclusive_lock(&file, || {
                let mut doc = read_toml(&file)?;
                let v = parse_scalar(&value, ty)?;
                set_at_path(&mut doc, &path, v)?;
                write_toml(&file, &doc)?;
                Ok(())
            })?;
            println!("{{\"ok\":true}}");
        }
        Cmd::SetJson { file, path, json } => {
            with_exclusive_lock(&file, || {
                let mut doc = read_toml(&file)?;
                let parsed: JsonValue = serde_json::from_str(&json).context("parsing --json")?;
                let last_key = path.rsplit_once('.').map(|(_, k)| k).unwrap_or(path.as_str());
                let v = maybe_date_coerce(last_key, &parsed)?;
                set_at_path(&mut doc, &path, v)?;
                write_toml(&file, &doc)?;
                Ok(())
            })?;
            println!("{{\"ok\":true}}");
        }
        Cmd::Validate { file } => {
            read_toml(&file)?;
            println!("{{\"ok\":true}}");
        }
        Cmd::Items { op } => items_dispatch(op)?,
    }
    Ok(())
}

fn items_dispatch(op: ItemsOp) -> Result<()> {
    match op {
        ItemsOp::List { file, status } => {
            let doc = read_toml(&file)?;
            let list = items_list(&doc, status.as_deref())?;
            print_json(&JsonValue::Array(list))?;
        }
        ItemsOp::Get { file, id } => {
            let doc = read_toml(&file)?;
            print_json(&items_get(&doc, &id)?)?;
        }
        ItemsOp::Add { file, json } => {
            with_exclusive_lock(&file, || {
                let mut doc = read_toml(&file)?;
                items_add(&mut doc, &json)?;
                write_toml(&file, &doc)?;
                Ok(())
            })?;
            println!("{{\"ok\":true}}");
        }
        ItemsOp::Update { file, id, json } => {
            with_exclusive_lock(&file, || {
                let mut doc = read_toml(&file)?;
                items_update(&mut doc, &id, &json)?;
                write_toml(&file, &doc)?;
                Ok(())
            })?;
            println!("{{\"ok\":true}}");
        }
        ItemsOp::Remove { file, id } => {
            with_exclusive_lock(&file, || {
                let mut doc = read_toml(&file)?;
                items_remove(&mut doc, &id)?;
                write_toml(&file, &doc)?;
                Ok(())
            })?;
            println!("{{\"ok\":true}}");
        }
        ItemsOp::Apply { file, ops } => {
            with_exclusive_lock(&file, || {
                let mut doc = read_toml(&file)?;
                items_apply(&mut doc, &ops)?;
                write_toml(&file, &doc)?;
                Ok(())
            })?;
            println!("{{\"ok\":true}}");
        }
        ItemsOp::NextId { file, prefix } => {
            let doc = read_toml(&file)?;
            let id = items_next_id(&doc, &prefix);
            println!("{}", serde_json::to_string(&id)?);
        }
    }
    Ok(())
}

fn read_toml(path: &Path) -> Result<TomlValue> {
    let s = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    toml::from_str::<TomlValue>(&s).with_context(|| format!("parsing {}", path.display()))
}

fn with_exclusive_lock<R>(path: &Path, f: impl FnOnce() -> Result<R>) -> Result<R> {
    use fs4::fs_std::FileExt;
    let lock_path = path.with_extension(match path.extension().and_then(|s| s.to_str()) {
        Some(ext) => format!("{}.lock", ext),
        None => "lock".to_string(),
    });
    let lock_file = std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(&lock_path)
        .with_context(|| format!("opening lock file {}", lock_path.display()))?;
    lock_file
        .lock_exclusive()
        .with_context(|| format!("acquiring exclusive lock on {}", lock_path.display()))?;
    let result = f();
    // Drop releases the lock automatically; no explicit unlock needed.
    let _ = lock_file;
    result
}

fn write_toml(path: &Path, value: &TomlValue) -> Result<()> {
    let parent = path
        .parent()
        .and_then(|p| if p.as_os_str().is_empty() { None } else { Some(p) })
        .unwrap_or(Path::new("."));
    let serialized = toml::to_string_pretty(value).context("serialising TOML")?;
    let mut tmp = tempfile::NamedTempFile::new_in(parent)
        .with_context(|| format!("creating temp file in {}", parent.display()))?;
    std::io::Write::write_all(&mut tmp, serialized.as_bytes())
        .with_context(|| format!("writing temp file for {}", path.display()))?;
    tmp.persist(path)
        .map_err(|e| anyhow!("atomic rename to {} failed: {}", path.display(), e.error))?;
    Ok(())
}

fn print_json(v: &JsonValue) -> Result<()> {
    let stdout = std::io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    serde_json::to_writer_pretty(&mut out, v)?;
    out.write_all(b"\n")?;
    out.flush()?;
    Ok(())
}

fn navigate<'a>(root: &'a TomlValue, path: &str) -> Option<&'a TomlValue> {
    let mut cur = root;
    for part in path.split('.') {
        cur = cur.as_table()?.get(part)?;
    }
    Some(cur)
}

fn set_at_path(root: &mut TomlValue, path: &str, value: TomlValue) -> Result<()> {
    let parts: Vec<&str> = path.split('.').collect();
    let (last, parents) = parts
        .split_last()
        .ok_or_else(|| anyhow!("empty key path"))?;

    let mut cur: &mut TomlValue = root;
    for p in parents {
        let tbl = cur
            .as_table_mut()
            .ok_or_else(|| anyhow!("path segment `{}` has a non-table parent", p))?;
        cur = tbl
            .entry((*p).to_string())
            .or_insert_with(|| TomlValue::Table(toml::Table::new()));
    }
    let tbl = cur
        .as_table_mut()
        .ok_or_else(|| anyhow!("target parent is not a table"))?;
    tbl.insert((*last).to_string(), value);
    Ok(())
}

fn parse_scalar(input: &str, explicit: Option<ScalarType>) -> Result<TomlValue> {
    let ty = explicit.unwrap_or_else(|| infer_type(input));
    match ty {
        ScalarType::Str => Ok(TomlValue::String(input.to_string())),
        ScalarType::Int => Ok(TomlValue::Integer(
            input
                .parse::<i64>()
                .with_context(|| format!("`{}` is not a valid int", input))?,
        )),
        ScalarType::Float => Ok(TomlValue::Float(
            input
                .parse::<f64>()
                .with_context(|| format!("`{}` is not a valid float", input))?,
        )),
        ScalarType::Bool => Ok(TomlValue::Boolean(
            input
                .parse::<bool>()
                .with_context(|| format!("`{}` is not a valid bool", input))?,
        )),
        ScalarType::Date | ScalarType::Datetime => {
            let dt: toml::value::Datetime = input
                .parse()
                .with_context(|| format!("`{}` is not a valid TOML datetime", input))?;
            Ok(TomlValue::Datetime(dt))
        }
    }
}

fn infer_type(s: &str) -> ScalarType {
    if s == "true" || s == "false" {
        ScalarType::Bool
    } else if looks_like_date(s) {
        ScalarType::Date
    } else if s.parse::<i64>().is_ok() {
        ScalarType::Int
    } else {
        ScalarType::Str
    }
}

fn looks_like_date(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b[..4].iter().all(|c| c.is_ascii_digit())
        && b[5..7].iter().all(|c| c.is_ascii_digit())
        && b[8..10].iter().all(|c| c.is_ascii_digit())
}

fn toml_to_json(v: &TomlValue) -> JsonValue {
    match v {
        TomlValue::String(s) => JsonValue::String(s.clone()),
        TomlValue::Integer(i) => JsonValue::from(*i),
        TomlValue::Float(f) => serde_json::Number::from_f64(*f)
            .map(JsonValue::Number)
            .unwrap_or(JsonValue::Null),
        TomlValue::Boolean(b) => JsonValue::Bool(*b),
        TomlValue::Datetime(dt) => JsonValue::String(dt.to_string()),
        TomlValue::Array(a) => JsonValue::Array(a.iter().map(toml_to_json).collect()),
        TomlValue::Table(t) => {
            let mut m = serde_json::Map::new();
            for (k, v) in t.iter() {
                m.insert(k.clone(), toml_to_json(v));
            }
            JsonValue::Object(m)
        }
    }
}

fn json_to_toml(v: &JsonValue) -> Result<TomlValue> {
    match v {
        JsonValue::Null => bail!("TOML has no null type"),
        JsonValue::Bool(b) => Ok(TomlValue::Boolean(*b)),
        JsonValue::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(TomlValue::Integer(i))
            } else if let Some(f) = n.as_f64() {
                Ok(TomlValue::Float(f))
            } else {
                bail!("number `{}` is not representable in TOML", n)
            }
        }
        JsonValue::String(s) => Ok(TomlValue::String(s.clone())),
        JsonValue::Array(a) => {
            let items: Result<Vec<_>> = a.iter().map(json_to_toml).collect();
            Ok(TomlValue::Array(items?))
        }
        JsonValue::Object(m) => {
            let mut t = toml::Table::new();
            for (k, v) in m.iter() {
                t.insert(k.clone(), json_to_toml(v)?);
            }
            Ok(TomlValue::Table(t))
        }
    }
}

fn items_array<'a>(doc: &'a TomlValue) -> Result<&'a Vec<TomlValue>> {
    doc.get("items")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow!("no [[items]] array in file"))
}

fn items_array_mut<'a>(doc: &'a mut TomlValue) -> Result<&'a mut Vec<TomlValue>> {
    let root = doc
        .as_table_mut()
        .ok_or_else(|| anyhow!("root is not a table"))?;
    let entry = root
        .entry("items".to_string())
        .or_insert_with(|| TomlValue::Array(Vec::new()));
    entry
        .as_array_mut()
        .ok_or_else(|| anyhow!("`items` is not an array"))
}

fn item_id(item: &TomlValue) -> Option<&str> {
    item.as_table()?.get("id")?.as_str()
}

fn items_list(doc: &TomlValue, status_filter: Option<&str>) -> Result<Vec<JsonValue>> {
    let items = items_array(doc)?;
    let mut out = Vec::new();
    for item in items {
        if let Some(want) = status_filter {
            let cur = item
                .as_table()
                .and_then(|t| t.get("status"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if cur != want {
                continue;
            }
        }
        out.push(toml_to_json(item));
    }
    Ok(out)
}

fn items_get(doc: &TomlValue, id: &str) -> Result<JsonValue> {
    for item in items_array(doc)? {
        if item_id(item) == Some(id) {
            return Ok(toml_to_json(item));
        }
    }
    bail!("no item with id = {}", id)
}

const DATE_KEYS: &[&str] = &[
    "created",
    "updated",
    "first_flagged",
    "last_updated",
    "resolved",
    "date",
];

fn maybe_date_coerce(key: &str, v: &JsonValue) -> Result<TomlValue> {
    if DATE_KEYS.contains(&key) {
        if let JsonValue::String(s) = v {
            if let Ok(dt) = s.parse::<toml::value::Datetime>() {
                return Ok(TomlValue::Datetime(dt));
            }
        }
    }
    json_to_toml(v)
}

fn items_add(doc: &mut TomlValue, json: &str) -> Result<()> {
    let patch: JsonValue = serde_json::from_str(json).context("parsing --json")?;
    items_add_value(doc, &patch)
}

fn items_add_value(doc: &mut TomlValue, patch: &JsonValue) -> Result<()> {
    let obj = patch
        .as_object()
        .ok_or_else(|| anyhow!("--json must be a JSON object"))?;
    let mut tbl = toml::Table::new();
    for (k, v) in obj.iter() {
        tbl.insert(k.clone(), maybe_date_coerce(k, v)?);
    }
    let arr = items_array_mut(doc)?;
    arr.push(TomlValue::Table(tbl));
    Ok(())
}

fn items_update(doc: &mut TomlValue, id: &str, json: &str) -> Result<()> {
    let patch: JsonValue = serde_json::from_str(json).context("parsing --json")?;
    items_update_value(doc, id, &patch)
}

fn items_update_value(doc: &mut TomlValue, id: &str, patch: &JsonValue) -> Result<()> {
    let patch_obj = patch
        .as_object()
        .ok_or_else(|| anyhow!("--json must be a JSON object"))?;

    let arr = items_array_mut(doc)?;
    for item in arr.iter_mut() {
        let tbl = match item.as_table_mut() {
            Some(t) => t,
            None => continue,
        };
        let matches = tbl.get("id").and_then(|v| v.as_str()) == Some(id);
        if !matches {
            continue;
        }
        for (k, v) in patch_obj.iter() {
            tbl.insert(k.clone(), maybe_date_coerce(k, v)?);
        }
        return Ok(());
    }
    bail!("no item with id = {}", id)
}

fn items_apply(doc: &mut TomlValue, ops_json: &str) -> Result<()> {
    let ops: JsonValue = serde_json::from_str(ops_json).context("parsing --ops")?;
    let arr = ops
        .as_array()
        .ok_or_else(|| anyhow!("--ops must be a JSON array"))?;
    for (i, op) in arr.iter().enumerate() {
        apply_single_op(doc, op).with_context(|| format!("op[{}] failed", i))?;
    }
    Ok(())
}

fn apply_single_op(doc: &mut TomlValue, op: &JsonValue) -> Result<()> {
    let obj = op
        .as_object()
        .ok_or_else(|| anyhow!("op must be a JSON object"))?;
    let op_name = obj
        .get("op")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("op missing `op` field"))?;
    match op_name {
        "add" => {
            let json = obj
                .get("json")
                .ok_or_else(|| anyhow!("add op missing `json` field"))?;
            items_add_value(doc, json)
        }
        "update" => {
            let id = obj
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("update op missing `id` field"))?;
            let json = obj
                .get("json")
                .ok_or_else(|| anyhow!("update op missing `json` field"))?;
            items_update_value(doc, id, json)
        }
        "remove" => {
            let id = obj
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("remove op missing `id` field"))?;
            items_remove(doc, id)
        }
        other => bail!("unknown op `{}`", other),
    }
}

fn items_remove(doc: &mut TomlValue, id: &str) -> Result<()> {
    let arr = items_array_mut(doc)?;
    let before = arr.len();
    arr.retain(|item| item_id(item) != Some(id));
    if arr.len() == before {
        bail!("no item with id = {}", id);
    }
    Ok(())
}

fn items_next_id(doc: &TomlValue, prefix: &str) -> String {
    let mut max_n: u64 = 0;
    if let Ok(arr) = items_array(doc) {
        for item in arr {
            if let Some(id) = item_id(item) {
                if let Some(rest) = id.strip_prefix(prefix) {
                    if let Ok(n) = rest.parse::<u64>() {
                        if n > max_n {
                            max_n = n;
                        }
                    }
                }
            }
        }
    }
    format!("{}{}", prefix, max_n + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONTEXT: &str = r#"slug = "x"
plan_path = "docs/plans/x.md"
status = "draft"
created = 2026-04-08
updated = 2026-04-08
scope = ["src/**"]

[tasks]
total = 3
completed = 0
in_progress = 0

[artifacts]
review_ledger = ".claude/flows/x/review-ledger.toml"
optimise_findings = ".claude/flows/x/optimise-findings.toml"
"#;

    const LEDGER: &str = r#"schema_version = 1
last_updated = 2026-04-16

[[items]]
id = "R1"
file = "src/a.rs"
line = 10
severity = "warning"
effort = "small"
category = "quality"
summary = "foo"
first_flagged = 2026-04-08
rounds = 1
status = "open"

[[items]]
id = "R4"
file = "src/b.rs"
line = 20
severity = "critical"
effort = "small"
category = "quality"
summary = "bar"
first_flagged = 2026-04-08
rounds = 1
status = "fixed"
resolved = 2026-04-08
resolution = "fix in abc123"
"#;

    fn ctx() -> TomlValue {
        toml::from_str(CONTEXT).unwrap()
    }
    fn led() -> TomlValue {
        toml::from_str(LEDGER).unwrap()
    }

    #[test]
    fn navigate_finds_nested_value() {
        let doc = ctx();
        assert_eq!(
            navigate(&doc, "tasks.total").and_then(|v| v.as_integer()),
            Some(3)
        );
        assert_eq!(
            navigate(&doc, "artifacts.review_ledger").and_then(|v| v.as_str()),
            Some(".claude/flows/x/review-ledger.toml")
        );
        assert!(navigate(&doc, "missing.path").is_none());
    }

    #[test]
    fn set_at_path_preserves_unrelated_fields_and_created() {
        let mut doc = ctx();
        set_at_path(&mut doc, "status", TomlValue::String("review".into())).unwrap();
        set_at_path(&mut doc, "tasks.completed", TomlValue::Integer(2)).unwrap();
        assert_eq!(
            navigate(&doc, "status").and_then(|v| v.as_str()),
            Some("review")
        );
        assert_eq!(
            navigate(&doc, "tasks.completed").and_then(|v| v.as_integer()),
            Some(2)
        );
        assert_eq!(
            navigate(&doc, "created").and_then(|v| v.as_datetime()).map(|d| d.to_string()),
            Some("2026-04-08".into())
        );
        assert_eq!(
            navigate(&doc, "slug").and_then(|v| v.as_str()),
            Some("x")
        );
    }

    #[test]
    fn set_json_replaces_array() {
        let mut doc = ctx();
        let patch: JsonValue = serde_json::from_str(r#"["a/**", "b/**"]"#).unwrap();
        let v = json_to_toml(&patch).unwrap();
        set_at_path(&mut doc, "scope", v).unwrap();
        let scope: Vec<&str> = navigate(&doc, "scope")
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(scope, vec!["a/**", "b/**"]);
    }

    #[test]
    fn infer_type_distinguishes_date_int_bool_string() {
        assert!(matches!(infer_type("2026-04-17"), ScalarType::Date));
        assert!(matches!(infer_type("42"), ScalarType::Int));
        assert!(matches!(infer_type("true"), ScalarType::Bool));
        assert!(matches!(infer_type("false"), ScalarType::Bool));
        assert!(matches!(infer_type("review"), ScalarType::Str));
        assert!(matches!(infer_type("2026-4-1"), ScalarType::Str));
    }

    #[test]
    fn items_list_filters_by_status() {
        let doc = led();
        let open = items_list(&doc, Some("open")).unwrap();
        assert_eq!(open.len(), 1);
        assert_eq!(open[0]["id"], "R1");
        let fixed = items_list(&doc, Some("fixed")).unwrap();
        assert_eq!(fixed.len(), 1);
        assert_eq!(fixed[0]["id"], "R4");
    }

    #[test]
    fn items_add_promotes_iso_date_strings_to_datetime() {
        let mut doc = led();
        items_add(
            &mut doc,
            r#"{"id":"R5","file":"src/c.rs","line":1,"severity":"suggestion","effort":"trivial","category":"quality","summary":"baz","first_flagged":"2026-04-17","rounds":1,"status":"open"}"#,
        )
        .unwrap();
        let item = items_get(&doc, "R5").unwrap();
        assert_eq!(item["first_flagged"], "2026-04-17");
        let serialised = toml::to_string_pretty(&doc).unwrap();
        assert!(
            serialised.contains("first_flagged = 2026-04-17"),
            "expected raw TOML date literal, got:\n{serialised}"
        );
    }

    #[test]
    fn items_update_merges_patch() {
        let mut doc = led();
        items_update(
            &mut doc,
            "R1",
            r#"{"status":"fixed","resolved":"2026-04-17","resolution":"fix in def456","rounds":2}"#,
        )
        .unwrap();
        let item = items_get(&doc, "R1").unwrap();
        assert_eq!(item["status"], "fixed");
        assert_eq!(item["rounds"], 2);
        assert_eq!(item["resolved"], "2026-04-17");
        assert_eq!(item["summary"], "foo", "unrelated field must be preserved");
    }

    #[test]
    fn items_remove_drops_matching_item() {
        let mut doc = led();
        items_remove(&mut doc, "R1").unwrap();
        assert!(items_get(&doc, "R1").is_err());
        assert!(items_get(&doc, "R4").is_ok());
        assert!(items_remove(&mut doc, "R999").is_err());
    }

    #[test]
    fn items_next_id_respects_max_and_prefix() {
        let doc = led();
        assert_eq!(items_next_id(&doc, "R"), "R5");
        assert_eq!(items_next_id(&doc, "O"), "O1");
    }

    #[test]
    fn roundtrip_preserves_datetime_and_key_order() {
        let doc = ctx();
        let s = toml::to_string_pretty(&doc).unwrap();
        assert!(s.contains("created = 2026-04-08"));
        let slug_pos = s.find("slug =").unwrap();
        let status_pos = s.find("status =").unwrap();
        assert!(slug_pos < status_pos);
    }

    #[test]
    fn json_to_toml_rejects_null() {
        let v: JsonValue = serde_json::from_str("null").unwrap();
        assert!(json_to_toml(&v).is_err());
    }

    #[test]
    fn items_add_does_not_coerce_non_date_keys() {
        let mut doc = led();
        items_add(
            &mut doc,
            r#"{"id":"R99","file":"2026-04-17","line":1,"severity":"suggestion","effort":"trivial","category":"quality","summary":"file name shaped like a date","first_flagged":"2026-04-17","rounds":1,"status":"open"}"#,
        )
        .unwrap();
        let item = items_get(&doc, "R99").unwrap();
        assert_eq!(item["file"], "2026-04-17");
        let serialised = toml::to_string_pretty(&doc).unwrap();
        assert!(
            serialised.contains(r#"file = "2026-04-17""#),
            "expected quoted string for non-date key, got:\n{serialised}"
        );
        assert!(
            serialised.contains("first_flagged = 2026-04-17"),
            "expected date literal for date key, got:\n{serialised}"
        );
    }

    #[test]
    fn items_apply_runs_batch_atomically() {
        let batch_ops = r#"[
            {"op":"add","json":{"id":"R5","file":"src/c.rs","line":1,"severity":"suggestion","effort":"trivial","category":"quality","summary":"baz","first_flagged":"2026-04-17","rounds":1,"status":"open"}},
            {"op":"update","id":"R1","json":{"status":"fixed","resolved":"2026-04-17","resolution":"fix in def456","rounds":2}},
            {"op":"remove","id":"R4"}
        ]"#;

        let mut doc_batch = led();
        items_apply(&mut doc_batch, batch_ops).unwrap();

        let mut doc_seq = led();
        items_add(
            &mut doc_seq,
            r#"{"id":"R5","file":"src/c.rs","line":1,"severity":"suggestion","effort":"trivial","category":"quality","summary":"baz","first_flagged":"2026-04-17","rounds":1,"status":"open"}"#,
        )
        .unwrap();
        items_update(
            &mut doc_seq,
            "R1",
            r#"{"status":"fixed","resolved":"2026-04-17","resolution":"fix in def456","rounds":2}"#,
        )
        .unwrap();
        items_remove(&mut doc_seq, "R4").unwrap();

        let s_batch = toml::to_string_pretty(&doc_batch).unwrap();
        let s_seq = toml::to_string_pretty(&doc_seq).unwrap();
        assert_eq!(s_batch, s_seq);
    }
}
