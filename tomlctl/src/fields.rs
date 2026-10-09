// Field flags: `--set KEY=VALUE`, `--set-json KEY=JSON` and `--set-file
// KEY=PATH` fold into one JSON object that merges over an optional `--json`
// base payload. Date coercion of `DATE_KEYS` happens where the merged payload
// becomes TOML, so nothing here coerces.

use std::collections::BTreeSet;
use std::path::Path;

use anyhow::Result;
use serde_json::{Map, Value as JsonValue};

use crate::errors::{ErrorKind, tagged_err};
use crate::io::{read_flag_file, read_text_arg, strip_flag_text};

/// The repeatable field flags shared by every verb that takes them.
#[derive(clap::Args, Debug, Clone, Default)]
pub(crate) struct FieldArgs {
    /// Set a field to a string: KEY=VALUE. A dotted KEY nests (refused on
    /// `items update`, which replaces top-level fields whole).
    #[arg(long = "set", value_name = "KEY=VALUE")]
    pub(crate) set: Vec<String>,

    /// Set a field to any JSON value: KEY=JSON.
    #[arg(long = "set-json", value_name = "KEY=JSON")]
    pub(crate) set_json: Vec<String>,

    /// Set a field to the text of a file (`-` reads stdin): KEY=PATH.
    #[arg(long = "set-file", value_name = "KEY=PATH")]
    pub(crate) set_file: Vec<String>,
}

impl FieldArgs {
    pub(crate) fn is_empty(&self) -> bool {
        self.set.is_empty() && self.set_json.is_empty() && self.set_file.is_empty()
    }
}

/// Splits `--<flag> KEY=VALUE` on its first `=`, refusing a missing `=` or
/// an empty dotted key segment.
pub(crate) fn split_pair<'a>(flag: &str, raw: &'a str) -> Result<(&'a str, &'a str)> {
    let (key, value) = raw.split_once('=').ok_or_else(|| {
        tagged_err(
            ErrorKind::Validation,
            None,
            format!("`--{flag} {raw}` needs the form KEY=VALUE"),
        )
    })?;
    if key.is_empty() || key.split('.').any(str::is_empty) {
        return Err(tagged_err(
            ErrorKind::Validation,
            None,
            format!("`--{flag} {raw}` has an empty key segment"),
        ));
    }
    Ok((key, value))
}

/// Refuses a key given twice, and a key that is a dotted prefix of another
/// (`a` beside `a.b`): either way one flag would silently replace the other,
/// so the refusal must not depend on flag order. `noun` names the key kind.
pub(crate) fn refuse_overlapping_keys(keys: &[&str], noun: &str) -> Result<()> {
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    for &key in keys {
        if !seen.insert(key) {
            return Err(tagged_err(
                ErrorKind::Validation,
                None,
                format!("{noun} `{key}` is given more than once"),
            ));
        }
    }
    for &key in keys {
        for (i, _) in key.match_indices('.') {
            let prefix = &key[..i];
            if seen.contains(prefix) {
                return Err(tagged_err(
                    ErrorKind::Validation,
                    None,
                    format!(
                        "{noun}s `{prefix}` and `{key}` overlap: `{key}` nests under `{prefix}`, which the other flag replaces"
                    ),
                ));
            }
        }
    }
    Ok(())
}

fn read_file_text(path: &str) -> Result<String> {
    if path == "-" {
        Ok(strip_flag_text(read_text_arg("-")?))
    } else {
        read_flag_file(Path::new(path), "set-file")
    }
}

/// Inserts `value` at the dotted `key`, creating intermediate objects.
fn insert_dotted(root: &mut Map<String, JsonValue>, key: &str, value: JsonValue) -> Result<()> {
    let mut parts = key.split('.').peekable();
    let mut cur = root;
    while let Some(part) = parts.next() {
        if parts.peek().is_none() {
            cur.insert(part.to_string(), value);
            return Ok(());
        }
        let slot = cur
            .entry(part.to_string())
            .or_insert_with(|| JsonValue::Object(Map::new()));
        match slot {
            JsonValue::Object(m) => cur = m,
            _ => {
                return Err(tagged_err(
                    ErrorKind::Validation,
                    None,
                    format!(
                        "field `{key}` nests under `{part}`, which another flag sets to a non-object"
                    ),
                ));
            }
        }
    }
    Ok(())
}

fn merge_over(base: &mut Map<String, JsonValue>, over: Map<String, JsonValue>) {
    for (k, v) in over {
        match (base.get_mut(&k), v) {
            (Some(JsonValue::Object(b)), JsonValue::Object(o)) => merge_over(b, o),
            (_, v) => {
                base.insert(k, v);
            }
        }
    }
}

/// Folds the field flags into one JSON object laid over `base` (the flags
/// win). Returns `base` unchanged when no flag is given.
pub(crate) fn build(args: &FieldArgs, base: Option<JsonValue>) -> Result<Option<JsonValue>> {
    if args.is_empty() {
        return Ok(base);
    }
    let sources = [
        ("set", &args.set),
        ("set-json", &args.set_json),
        ("set-file", &args.set_file),
    ];
    let mut pairs = Vec::new();
    for (flag, values) in sources {
        for raw in values {
            let (key, value) = split_pair(flag, raw)?;
            pairs.push((flag, key, value));
        }
    }
    let keys: Vec<&str> = pairs.iter().map(|&(_, key, _)| key).collect();
    refuse_overlapping_keys(&keys, "field")?;
    let mut flags = Map::new();
    for (flag, key, value) in pairs {
        let parsed = match flag {
            "set" => JsonValue::String(value.to_string()),
            "set-json" => serde_json::from_str(value).map_err(|e| {
                tagged_err(
                    ErrorKind::Validation,
                    None,
                    format!("`--set-json {key}=…` is not valid JSON: {e}"),
                )
            })?,
            _ => JsonValue::String(read_file_text(value)?),
        };
        insert_dotted(&mut flags, key, parsed)?;
    }
    let mut out = match base {
        None => Map::new(),
        Some(JsonValue::Object(m)) => m,
        Some(_) => {
            return Err(tagged_err(
                ErrorKind::Validation,
                None,
                "field flags need the `--json` payload to be an object".to_string(),
            ));
        }
    };
    merge_over(&mut out, flags);
    Ok(Some(JsonValue::Object(out)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn args(set: &[&str], set_json: &[&str], set_file: &[&str]) -> FieldArgs {
        let own = |s: &[&str]| s.iter().map(|x| x.to_string()).collect();
        FieldArgs {
            set: own(set),
            set_json: own(set_json),
            set_file: own(set_file),
        }
    }

    #[test]
    fn no_flags_returns_base() {
        assert_eq!(build(&FieldArgs::default(), None).unwrap(), None);
        let base = json!({"a": 1});
        assert_eq!(
            build(&FieldArgs::default(), Some(base.clone())).unwrap(),
            Some(base)
        );
    }

    #[test]
    fn set_stays_a_string_and_splits_on_first_equals() {
        let out = build(&args(&["retries=3", "note=a=b"], &[], &[]), None)
            .unwrap()
            .unwrap();
        assert_eq!(out, json!({"retries": "3", "note": "a=b"}));
    }

    #[test]
    fn set_json_keeps_types_and_flags_win_over_base() {
        let out = build(
            &args(&[], &["retries=3", "files=[\"a\",\"b\"]"], &[]),
            Some(json!({"retries": 9, "keep": true})),
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            out,
            json!({"retries": 3, "files": ["a", "b"], "keep": true})
        );
    }

    #[test]
    fn set_file_strips_bom_and_one_trailing_newline() {
        let dir = std::env::temp_dir().join(format!("tomlctl-fields-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let bom = dir.join("bom.txt");
        std::fs::write(&bom, "\u{feff}hello\r\nworld\r\n").unwrap();
        let lf = dir.join("lf.txt");
        std::fs::write(&lf, "line\n\n").unwrap();
        let out = build(
            &args(
                &[],
                &[],
                &[
                    &format!("a={}", bom.display()),
                    &format!("b={}", lf.display()),
                ],
            ),
            None,
        )
        .unwrap()
        .unwrap();
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(out, json!({"a": "hello\r\nworld", "b": "line\n"}));
    }

    #[test]
    fn dotted_keys_nest() {
        let out = build(&args(&["a.b=1", "a.c=2"], &["a.d={\"x\":true}"], &[]), None)
            .unwrap()
            .unwrap();
        assert_eq!(out, json!({"a": {"b": "1", "c": "2", "d": {"x": true}}}));
    }

    #[test]
    fn duplicate_key_is_a_validation_error() {
        let err = build(&args(&["a=1"], &["a=2"], &[]), None).unwrap_err();
        let tagged = err.downcast_ref::<crate::errors::TaggedError>().unwrap();
        assert_eq!(tagged.kind.as_str(), "validation");
    }

    fn validation_message(err: anyhow::Error) -> String {
        let tagged = err.downcast_ref::<crate::errors::TaggedError>().unwrap();
        assert_eq!(tagged.kind.as_str(), "validation");
        err.to_string()
    }

    #[test]
    fn dotted_prefix_key_is_refused_in_either_order() {
        for set in [["a.b=1", "a=2"], ["a=2", "a.b=1"]] {
            let msg = validation_message(build(&args(&set, &[], &[]), None).unwrap_err());
            assert!(
                msg.contains("`a`") && msg.contains("`a.b`"),
                "{set:?}: {msg}"
            );
        }
        let msg =
            validation_message(build(&args(&["a.b.c=1"], &["a.b={}"], &[]), None).unwrap_err());
        assert!(msg.contains("`a.b`") && msg.contains("`a.b.c`"), "{msg}");
    }

    #[test]
    fn prefix_check_respects_segment_boundaries() {
        let out = build(&args(&["ab=1", "a.b=2"], &[], &[]), None)
            .unwrap()
            .unwrap();
        assert_eq!(out, json!({"ab": "1", "a": {"b": "2"}}));
    }

    #[test]
    fn missing_equals_is_refused() {
        assert!(build(&args(&["nokey"], &[], &[]), None).is_err());
    }
}
