//! The execution-record entry contract as pure functions: type vocabulary,
//! required fields, enums, integer and array types, length caps, `files[]`
//! hygiene and the flow-scope check. The canonical statement of the rules is
//! the `flow-contract-execution-record-schema` skill; this module enforces
//! them on one JSON entry before it is written.

use anyhow::Result;
use serde_json::{Map as JsonMap, Value as JsonValue};

use super::resolve::compile_scope_globset;
use crate::errors::{ErrorKind, tagged_err};

/// The always-required fields every entry carries regardless of `type`.
/// `id` is minted by the write itself, so it is not checked here.
const ALWAYS_REQUIRED: &[&str] = &["type", "date", "agent", "summary"];

const TYPES: &[&str] = &[
    "task-completion",
    "verification",
    "deviation",
    "deferral",
    "reconcile",
    "status-transition",
    "checkpoint",
];

/// Per-type required fields beyond `ALWAYS_REQUIRED`. `commits` is optional
/// on both `task-completion` and `deviation`: under milestone checkpoints the
/// SHA lands later on the commit-train `checkpoint` entry.
fn type_required(ty: &str) -> &'static [&'static str] {
    match ty {
        "task-completion" => &[
            "task_ref",
            "status",
            "files",
            "dispatch_tier",
            "dispatch_agent",
            "vet",
            "retries",
        ],
        "verification" => &["command", "outcome"],
        "deviation" => &["original_intent", "rationale"],
        "deferral" => &["task_ref", "reason", "reevaluate_when"],
        "reconcile" => &["direction", "findings_count", "commits_checked"],
        "status-transition" => &["from_status", "to_status"],
        _ => &[],
    }
}

/// Per-type enum fields and their allowed values. An enum field on a type
/// that does not define it passes through unchecked, like any extra key.
fn type_enums(ty: &str) -> &'static [(&'static str, &'static [&'static str])] {
    match ty {
        "task-completion" => &[
            ("status", &["done", "failed", "skipped"]),
            ("dispatch_tier", &["lite", "deep"]),
            ("dispatch_agent", &["implement-lite", "implement-deep"]),
            (
                "vet",
                &[
                    "skipped",
                    "sampled-pass",
                    "sampled-fail",
                    "flagged-pass",
                    "flagged-fail",
                ],
            ),
        ],
        "verification" => &[("outcome", &["pass", "fail", "timeout", "flaky"])],
        "reconcile" => &[("direction", &["forward", "reverse"])],
        _ => &[],
    }
}

const INTEGER_FIELDS: &[&str] = &["retries", "duration_s"];
const ARRAY_FIELDS: &[&str] = &["files", "commits", "failed_ids"];

/// Byte caps, measured on the UTF-8 encoding.
const TEXT_CAPS: &[(&str, usize)] = &[
    ("summary", 1024),
    ("description", 8192),
    ("rationale", 8192),
    ("original_intent", 8192),
    ("reason", 8192),
    ("reevaluate_when", 8192),
];

const TRUNCATION_MARKER: &str = " (truncated)";
const FAILED_IDS_CAP: usize = 20;

/// What `normalise` changed or flagged without refusing the entry.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct RecordReport {
    /// Fields shortened to their cap.
    pub(crate) truncated: Vec<String>,
    /// `files[]` entries removed as absolute, home-relative or escaping.
    pub(crate) dropped_files: Vec<String>,
    /// Kept `files[]` entries that match none of the flow's scope globs.
    pub(crate) scope_warnings: Vec<String>,
}

fn validation(msg: String) -> anyhow::Error {
    tagged_err(ErrorKind::Validation, None, msg)
}

fn quoted_list(values: &[&str]) -> String {
    values
        .iter()
        .map(|v| format!("`{v}`"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Validate and normalise one execution-record entry in place against the
/// contract, given the flow's `context.toml` `scope` globs. A missing field,
/// an out-of-vocabulary value or a wrongly typed field is `kind=validation`;
/// over-cap text is truncated, never refused. Unknown keys pass through.
pub(crate) fn normalise(
    entry: &mut JsonMap<String, JsonValue>,
    scope: &[String],
) -> Result<RecordReport> {
    let ty = match entry.get("type") {
        Some(JsonValue::String(t)) if TYPES.contains(&t.as_str()) => t.clone(),
        Some(JsonValue::String(t)) => {
            return Err(validation(format!(
                "execution-record field `type` is `{t}`; allowed values: {}",
                quoted_list(TYPES)
            )));
        }
        Some(JsonValue::Null) | None => {
            return Err(validation(format!(
                "execution-record entry is missing required field `type`; allowed values: {}",
                quoted_list(TYPES)
            )));
        }
        Some(other) => {
            return Err(validation(format!(
                "execution-record field `type` must be a string, got {other}; allowed values: {}",
                quoted_list(TYPES)
            )));
        }
    };

    for field in ALWAYS_REQUIRED.iter().chain(type_required(&ty)) {
        if matches!(entry.get(*field), None | Some(JsonValue::Null)) {
            return Err(validation(format!(
                "execution-record entry of type `{ty}` is missing required field `{field}`"
            )));
        }
    }

    for (field, allowed) in type_enums(&ty) {
        match entry.get(*field) {
            None | Some(JsonValue::Null) => {}
            Some(JsonValue::String(v)) if allowed.contains(&v.as_str()) => {}
            Some(v) => {
                return Err(validation(format!(
                    "execution-record field `{field}` is {v} on type `{ty}`; allowed values: {}",
                    quoted_list(allowed)
                )));
            }
        }
    }

    for field in INTEGER_FIELDS {
        coerce_integer(entry, field)?;
    }
    for field in ARRAY_FIELDS {
        check_array(entry, field)?;
    }

    let mut report = RecordReport::default();
    for (field, cap) in TEXT_CAPS {
        if let Some(JsonValue::String(s)) = entry.get_mut(*field)
            && let Some(cut) = truncate_to_cap(s, *cap)
        {
            *s = cut;
            report.truncated.push((*field).to_string());
        }
    }
    if let Some(JsonValue::Array(ids)) = entry.get_mut("failed_ids")
        && ids.len() > FAILED_IDS_CAP
    {
        ids.truncate(FAILED_IDS_CAP);
        report.truncated.push("failed_ids".to_string());
    }

    normalise_files(entry, scope, &mut report)?;
    Ok(report)
}

/// An integer field holding a digit string becomes that integer; any other
/// non-integer value is refused.
fn coerce_integer(entry: &mut JsonMap<String, JsonValue>, field: &str) -> Result<()> {
    let Some(value) = entry.get_mut(field) else {
        return Ok(());
    };
    match value {
        JsonValue::Null => Ok(()),
        JsonValue::Number(n) if n.is_i64() || n.is_u64() => Ok(()),
        JsonValue::String(s) if !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) => {
            let n: u64 = s.parse().map_err(|_| {
                validation(format!(
                    "execution-record field `{field}` is out of range: `{s}`"
                ))
            })?;
            *value = JsonValue::from(n);
            Ok(())
        }
        other => Err(validation(format!(
            "execution-record field `{field}` must be an integer, got {other}"
        ))),
    }
}

fn check_array(entry: &JsonMap<String, JsonValue>, field: &str) -> Result<()> {
    match entry.get(field) {
        None | Some(JsonValue::Null) | Some(JsonValue::Array(_)) => Ok(()),
        Some(JsonValue::String(_)) => Err(validation(format!(
            "execution-record field `{field}` must be an array, got a string; \
             pass it with `--set-json {field}='[\"…\"]'` rather than `--set`"
        ))),
        Some(other) => Err(validation(format!(
            "execution-record field `{field}` must be an array, got {other}; \
             pass it with `--set-json {field}='[…]'`"
        ))),
    }
}

/// `Some(shortened)` when `s` exceeds `cap` bytes: the longest char-boundary
/// prefix that still fits with the marker appended.
fn truncate_to_cap(s: &str, cap: usize) -> Option<String> {
    if s.len() <= cap {
        return None;
    }
    let mut end = cap.saturating_sub(TRUNCATION_MARKER.len());
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    Some(format!("{}{TRUNCATION_MARKER}", &s[..end]))
}

/// A path that is absolute, home-relative, drive-qualified or climbs out
/// with `..` cannot name a repo file. Expects `/` separators.
fn is_unsafe_path(path: &str) -> bool {
    let bytes = path.as_bytes();
    path.starts_with('/')
        || path.starts_with('~')
        || (bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':')
        || path.split('/').any(|seg| seg == "..")
}

fn normalise_files(
    entry: &mut JsonMap<String, JsonValue>,
    scope: &[String],
    report: &mut RecordReport,
) -> Result<()> {
    let Some(JsonValue::Array(files)) = entry.get_mut("files") else {
        return Ok(());
    };
    let was_empty = files.is_empty();
    let mut kept = Vec::with_capacity(files.len());
    for item in files.drain(..) {
        let JsonValue::String(raw) = item else {
            return Err(validation(format!(
                "execution-record field `files` must hold strings, got {item}"
            )));
        };
        let path = raw.replace('\\', "/");
        if is_unsafe_path(&path) {
            report.dropped_files.push(raw);
        } else {
            kept.push(path);
        }
    }
    if !was_empty && kept.is_empty() {
        return Err(validation(format!(
            "execution-record field `files` holds no repo-relative path once absolute, `~` \
             and `..` entries are dropped (dropped: {})",
            report.dropped_files.join(", ")
        )));
    }
    if let Some(set) = compile_scope_globset(scope) {
        report.scope_warnings = kept
            .iter()
            .filter(|p| !set.is_match(p.as_str()))
            .cloned()
            .collect();
    }
    *files = kept.into_iter().map(JsonValue::String).collect();
    if !report.scope_warnings.is_empty() {
        entry.insert("scope_warning".to_string(), JsonValue::Bool(true));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::errors::TaggedError;
    use serde_json::json;

    fn map(v: JsonValue) -> JsonMap<String, JsonValue> {
        match v {
            JsonValue::Object(m) => m,
            _ => panic!("fixture must be an object"),
        }
    }

    fn base(ty: &str) -> JsonMap<String, JsonValue> {
        map(json!({
            "type": ty,
            "date": "2026-10-09",
            "agent": "implement",
            "summary": "did a thing",
        }))
    }

    fn task_completion() -> JsonMap<String, JsonValue> {
        let mut m = base("task-completion");
        m.extend(map(json!({
            "task_ref": "add-retry-logic",
            "status": "done",
            "files": ["src/retry.rs"],
            "dispatch_tier": "lite",
            "dispatch_agent": "implement-lite",
            "vet": "skipped",
            "retries": 0,
        })));
        m
    }

    fn validation_message(err: anyhow::Error) -> String {
        let tag = err
            .downcast_ref::<TaggedError>()
            .expect("error must be tagged");
        assert_eq!(tag.kind.as_str(), "validation");
        tag.message.clone()
    }

    #[test]
    fn task_completion_valid_passes_unchanged() {
        let mut m = task_completion();
        let before = m.clone();
        let report = normalise(&mut m, &[]).unwrap();
        assert_eq!(report, RecordReport::default());
        assert_eq!(m, before);
    }

    #[test]
    fn task_completion_missing_required_field_names_it() {
        for field in [
            "task_ref",
            "status",
            "files",
            "dispatch_tier",
            "dispatch_agent",
            "vet",
            "retries",
        ] {
            let mut m = task_completion();
            m.remove(field);
            let msg = validation_message(normalise(&mut m, &[]).unwrap_err());
            assert!(msg.contains(&format!("`{field}`")), "{field}: {msg}");
        }
    }

    #[test]
    fn always_required_fields_are_enforced() {
        for field in ["date", "agent", "summary"] {
            let mut m = base("checkpoint");
            m.insert(field.to_string(), JsonValue::Null);
            let msg = validation_message(normalise(&mut m, &[]).unwrap_err());
            assert!(msg.contains(&format!("`{field}`")), "{field}: {msg}");
        }
    }

    #[test]
    fn missing_or_unknown_type_lists_the_vocabulary() {
        let mut m = base("checkpoint");
        m.remove("type");
        let msg = validation_message(normalise(&mut m, &[]).unwrap_err());
        assert!(msg.contains("`task-completion`") && msg.contains("`checkpoint`"));

        let mut m = base("progress");
        let msg = validation_message(normalise(&mut m, &[]).unwrap_err());
        assert!(msg.contains("`progress`") && msg.contains("`status-transition`"));
    }

    #[test]
    fn task_completion_enums_reject_out_of_vocabulary_values() {
        for (field, bad, allowed) in [
            ("status", "complete", "`done`"),
            ("dispatch_tier", "medium", "`deep`"),
            ("dispatch_agent", "general-purpose", "`implement-deep`"),
            ("vet", "passed", "`sampled-pass`"),
        ] {
            let mut m = task_completion();
            m.insert(field.to_string(), json!(bad));
            let msg = validation_message(normalise(&mut m, &[]).unwrap_err());
            assert!(
                msg.contains(&format!("`{field}`")) && msg.contains(allowed),
                "{field}: {msg}"
            );
        }
    }

    #[test]
    fn verification_requires_command_and_outcome_enum() {
        let mut m = base("verification");
        m.insert("command".into(), json!("cargo test"));
        m.insert("outcome".into(), json!("flaky"));
        m.insert("duration_s".into(), json!("42"));
        normalise(&mut m, &[]).unwrap();
        assert_eq!(m["duration_s"], json!(42));

        m.insert("outcome".into(), json!("green"));
        let msg = validation_message(normalise(&mut m, &[]).unwrap_err());
        assert!(msg.contains("`outcome`") && msg.contains("`timeout`"));

        let mut m = base("verification");
        m.insert("outcome".into(), json!("pass"));
        let msg = validation_message(normalise(&mut m, &[]).unwrap_err());
        assert!(msg.contains("`command`"));
    }

    #[test]
    fn verification_failed_ids_keeps_first_twenty() {
        let mut m = base("verification");
        m.insert("command".into(), json!("cargo test"));
        m.insert("outcome".into(), json!("fail"));
        let ids: Vec<String> = (0..25).map(|i| format!("t{i}")).collect();
        m.insert("failed_ids".into(), json!(ids));
        let report = normalise(&mut m, &[]).unwrap();
        assert_eq!(report.truncated, vec!["failed_ids"]);
        let kept = m["failed_ids"].as_array().unwrap();
        assert_eq!(kept.len(), 20);
        assert_eq!(kept[0], json!("t0"));
        assert_eq!(kept[19], json!("t19"));
    }

    #[test]
    fn deviation_requires_intent_and_rationale_but_not_commits() {
        let mut m = base("deviation");
        m.insert("original_intent".into(), json!("add redis"));
        m.insert("rationale".into(), json!("cache exists"));
        normalise(&mut m, &[]).unwrap();

        m.remove("rationale");
        let msg = validation_message(normalise(&mut m, &[]).unwrap_err());
        assert!(msg.contains("`rationale`"));
    }

    #[test]
    fn deferral_requires_its_three_fields() {
        for field in ["task_ref", "reason", "reevaluate_when"] {
            let mut m = base("deferral");
            m.insert("task_ref".into(), json!("x"));
            m.insert("reason".into(), json!("blocked"));
            m.insert("reevaluate_when".into(), json!("after 0.16"));
            normalise(&mut m.clone(), &[]).unwrap();
            m.remove(field);
            let msg = validation_message(normalise(&mut m, &[]).unwrap_err());
            assert!(msg.contains(&format!("`{field}`")), "{field}: {msg}");
        }
    }

    #[test]
    fn reconcile_direction_enum() {
        let mut m = base("reconcile");
        m.insert("direction".into(), json!("forward"));
        m.insert("findings_count".into(), json!(3));
        m.insert("commits_checked".into(), json!(["abc"]));
        normalise(&mut m, &[]).unwrap();

        m.insert("direction".into(), json!("sideways"));
        let msg = validation_message(normalise(&mut m, &[]).unwrap_err());
        assert!(msg.contains("`direction`") && msg.contains("`reverse`"));
    }

    #[test]
    fn status_transition_requires_both_ends() {
        let mut m = base("status-transition");
        m.insert("from_status".into(), json!("draft"));
        let msg = validation_message(normalise(&mut m, &[]).unwrap_err());
        assert!(msg.contains("`to_status`"));
        m.insert("to_status".into(), json!("in-progress"));
        normalise(&mut m, &[]).unwrap();
    }

    #[test]
    fn checkpoint_is_freeform_and_unknown_keys_pass_through() {
        let mut m = base("checkpoint");
        m.insert("kind".into(), json!("commit-train"));
        m.insert("anything_else".into(), json!({"nested": [1, 2]}));
        let before = m.clone();
        normalise(&mut m, &[]).unwrap();
        assert_eq!(m, before);
    }

    #[test]
    fn retries_digit_string_is_coerced_and_other_strings_refused() {
        let mut m = task_completion();
        m.insert("retries".into(), json!("2"));
        normalise(&mut m, &[]).unwrap();
        assert_eq!(m["retries"], json!(2));

        for bad in [json!("two"), json!("-1"), json!(""), json!(1.5)] {
            let mut m = task_completion();
            m.insert("retries".into(), bad.clone());
            let msg = validation_message(normalise(&mut m, &[]).unwrap_err());
            assert!(msg.contains("`retries`"), "{bad}: {msg}");
        }
    }

    #[test]
    fn array_field_given_as_string_points_at_set_json() {
        for field in ["files", "commits", "failed_ids"] {
            let mut m = task_completion();
            m.insert(field.to_string(), json!("src/a.rs"));
            let msg = validation_message(normalise(&mut m, &[]).unwrap_err());
            assert!(
                msg.contains(&format!("`{field}`")) && msg.contains("--set-json"),
                "{field}: {msg}"
            );
        }
    }

    #[test]
    fn summary_over_cap_truncates_at_char_boundary() {
        let mut m = task_completion();
        // 3-byte chars, so the cut point falls mid-character unless aligned.
        m.insert("summary".into(), json!("é€".repeat(400)));
        let report = normalise(&mut m, &[]).unwrap();
        assert_eq!(report.truncated, vec!["summary"]);
        let s = m["summary"].as_str().unwrap();
        assert!(s.len() <= 1024, "{} bytes", s.len());
        assert!(s.ends_with(" (truncated)"));
        assert!(s.len() > 1024 - 12 - 3);
    }

    #[test]
    fn long_text_fields_cap_at_eight_kib_and_short_ones_are_kept() {
        let mut m = base("deviation");
        m.insert("original_intent".into(), json!("short"));
        m.insert("rationale".into(), json!("r".repeat(9000)));
        m.insert("description".into(), json!("d".repeat(8192)));
        let report = normalise(&mut m, &[]).unwrap();
        assert_eq!(report.truncated, vec!["rationale"]);
        assert_eq!(m["rationale"].as_str().unwrap().len(), 8192);
        assert_eq!(m["description"].as_str().unwrap().len(), 8192);
        assert_eq!(m["original_intent"], json!("short"));
    }

    #[test]
    fn files_backslashes_normalise_and_unsafe_entries_drop() {
        let mut m = task_completion();
        m.insert(
            "files".into(),
            json!([
                "src\\a.rs",
                "/etc/passwd",
                "\\abs\\x.rs",
                "~/notes.md",
                "C:\\Users\\x.rs",
                "c:/x.rs",
                "../x.rs",
                "src/../../x.rs",
                "src/..hidden/ok.rs",
            ]),
        );
        let report = normalise(&mut m, &[]).unwrap();
        assert_eq!(m["files"], json!(["src/a.rs", "src/..hidden/ok.rs"]));
        assert_eq!(
            report.dropped_files,
            vec![
                "/etc/passwd",
                "\\abs\\x.rs",
                "~/notes.md",
                "C:\\Users\\x.rs",
                "c:/x.rs",
                "../x.rs",
                "src/../../x.rs",
            ]
        );
    }

    #[test]
    fn files_emptied_by_dropping_is_refused_but_empty_list_is_kept() {
        let mut m = task_completion();
        m.insert("files".into(), json!(["../x.rs"]));
        let msg = validation_message(normalise(&mut m, &[]).unwrap_err());
        assert!(msg.contains("`files`") && msg.contains("../x.rs"));

        let mut m = task_completion();
        m.insert("files".into(), json!([]));
        normalise(&mut m, &[]).unwrap();
        assert_eq!(m["files"], json!([]));
    }

    #[test]
    fn files_outside_scope_warn_and_mark_the_entry() {
        let scope = vec!["tomlctl/src/**".to_string()];
        let mut m = task_completion();
        m.insert(
            "files".into(),
            json!(["tomlctl\\src\\flow\\mod.rs", "claude/skills/x.md"]),
        );
        let report = normalise(&mut m, &scope).unwrap();
        assert_eq!(report.scope_warnings, vec!["claude/skills/x.md"]);
        assert_eq!(m["scope_warning"], json!(true));

        let mut m = task_completion();
        m.insert("files".into(), json!(["tomlctl/src/lib.rs"]));
        let report = normalise(&mut m, &scope).unwrap();
        assert!(report.scope_warnings.is_empty());
        assert!(!m.contains_key("scope_warning"));
    }

    #[test]
    fn empty_absent_or_invalid_scope_warns_on_nothing() {
        for scope in [vec![], vec!["[".to_string()]] {
            let mut m = task_completion();
            m.insert("files".into(), json!(["anywhere/x.rs"]));
            let report = normalise(&mut m, &scope).unwrap();
            assert!(report.scope_warnings.is_empty());
            assert!(!m.contains_key("scope_warning"));
        }
    }
}
