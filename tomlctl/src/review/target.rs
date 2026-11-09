//! Record targets for plan-family findings on a v2 flow: the grammar a finding's
//! `target` is written in, resolution of a target to the current value of the
//! record it names, and list-entry matching by value.
//!
//! Values are the JSON form of the stored TOML, so a task field resolves to what
//! `tasks show` prints for it. A stored `spec.accept` entry carries a
//! tool-written `probe` that no finding can supply, so accept entries resolve
//! and match with `probe` removed.

use std::fmt;
use std::path::Path;

use anyhow::Result;
use serde_json::Value as JsonValue;
use toml::Value as TomlValue;

use super::add::section_bodies;
use super::invalid;
use crate::cli::ReadIntegrityArgs;
use crate::convert::toml_to_json;
use crate::tasks::schema::{ELABORATION_UPFRONT, MILESTONE_REF_PREFIX};
use crate::tasks::{Store as TaskStore, load_store, resolve_store_path};

/// Task row fields a finding may edit. `title` is absent: a retitle that moves
/// the derived ref would strand every other finding's `task:<ref>` target.
const TASK_FIELDS: &[&str] = &[
    "action",
    "detail",
    "acceptance",
    "effort",
    "files",
    "needs",
    "coupling",
    "deps_note",
    "checkpoint",
];
const SPEC_FIELDS: &[&str] = &[
    "outcome",
    "constraints",
    "accept",
    "premises",
    "means",
    "design",
    "precedent",
    "executor",
];
/// The `SPEC_FIELDS` that hold lists, and so take an entry target.
const SPEC_LISTS: &[&str] = &[
    "constraints",
    "accept",
    "premises",
    "means",
    "design",
    "precedent",
];
const DECISION_KEYS: &[&str] = &[
    "decision",
    "why",
    "provenance",
    "door",
    "binds",
    "rejected",
    "evidence",
    "if_false",
    "revisit_if",
    "check",
    "surfaced",
    "vetoed",
];
const CONTRACT_KEYS: &[&str] = &["kind", "claim", "check", "tasks", "provides", "consumes"];
/// `goal` is the milestone's `[[checkpoints]]` rationale; the rest live on its
/// `[[milestones]]` row.
const MILESTONE_FIELDS: &[&str] = &["goal", "gates", "elaboration", "defer_reason"];
const HANDOVER_FIELDS: &[&str] = &["command", "source_task", "done"];
const BRIEF_SECTIONS: &[&str] = &[
    "Intent",
    "Non-goals",
    "Scope budget",
    "Success criteria",
    "Background",
    "Unknowns",
];
const BRIEF_FILE: &str = "brief.md";
const DESIGN_DIR: &str = "design/";
const PROBE_KEY: &str = "probe";

/// One record a plan finding edits, parsed from its `target`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum RecordTarget {
    TaskField {
        r#ref: String,
        field: &'static str,
    },
    SpecField {
        r#ref: String,
        field: &'static str,
    },
    /// `index` is a hint only: the entry edited is the one equal to `expect`.
    SpecEntry {
        r#ref: String,
        list: &'static str,
        index: Option<usize>,
        append: bool,
    },
    Decision {
        id: String,
        key: &'static str,
    },
    Contract {
        id: String,
        key: &'static str,
    },
    Milestone {
        id: String,
        field: &'static str,
    },
    Handover {
        id: String,
        field: &'static str,
    },
    Brief {
        section: &'static str,
    },
    /// `file` is relative to the flow's authored directory and starts `design/`.
    Design {
        file: String,
        anchor: String,
    },
}

pub(super) fn parse(raw: &str) -> Result<RecordTarget> {
    let refuse = |why: &str| invalid(format!("target `{raw}`: {why}"));
    if let Some(rest) = raw.strip_prefix("task:") {
        let (r#ref, field) = rest
            .split_once('.')
            .ok_or_else(|| refuse("expected `task:<ref>.<field>`"))?;
        if !is_ref(r#ref) {
            return Err(refuse("the task ref is empty or not a derived ref"));
        }
        let r#ref = r#ref.to_string();
        if let Some(spec) = field.strip_prefix("spec.") {
            return match spec.split_once('[') {
                Some((list, slot)) => {
                    let list = SPEC_LISTS
                        .iter()
                        .find(|known| **known == list)
                        .ok_or_else(|| refuse(&expected("a spec list", SPEC_LISTS)))?;
                    let slot = slot
                        .strip_suffix(']')
                        .ok_or_else(|| refuse("an entry index ends with `]`"))?;
                    let (index, append) = match slot {
                        "+" => (None, true),
                        digits
                            if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) =>
                        {
                            let index = digits
                                .parse()
                                .map_err(|_| refuse("the entry index is out of range"))?;
                            (Some(index), false)
                        }
                        _ => return Err(refuse("an entry index is `[<n>]` or `[+]`")),
                    };
                    Ok(RecordTarget::SpecEntry {
                        r#ref,
                        list,
                        index,
                        append,
                    })
                }
                None => Ok(RecordTarget::SpecField {
                    r#ref,
                    field: known(spec, SPEC_FIELDS)
                        .ok_or_else(|| refuse(&expected("a spec field", SPEC_FIELDS)))?,
                }),
            };
        }
        if field == "title" {
            return Err(refuse(
                "`title` is not a target, since a retitle can move the ref other findings name",
            ));
        }
        return Ok(RecordTarget::TaskField {
            r#ref,
            field: known(field, TASK_FIELDS)
                .ok_or_else(|| refuse(&expected("a task field", TASK_FIELDS)))?,
        });
    }
    for (prefix, letter, keys) in [
        ("decision:", 'D', DECISION_KEYS),
        ("contract:", 'C', CONTRACT_KEYS),
        ("handover:", 'H', HANDOVER_FIELDS),
    ] {
        let Some(rest) = raw.strip_prefix(prefix) else {
            continue;
        };
        let (id, key) = rest
            .split_once('.')
            .ok_or_else(|| refuse(&format!("expected `{prefix}{letter}<n>.<key>`")))?;
        let numbered = id
            .strip_prefix(letter)
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
        if !numbered {
            return Err(refuse(&format!("the record id is not `{letter}<n>`")));
        }
        let key = known(key, keys).ok_or_else(|| refuse(&expected("a key", keys)))?;
        let id = id.to_string();
        return Ok(match letter {
            'D' => RecordTarget::Decision { id, key },
            'C' => RecordTarget::Contract { id, key },
            _ => RecordTarget::Handover { id, field: key },
        });
    }
    if let Some(rest) = raw.strip_prefix("milestone:") {
        let (id, field) = rest
            .rsplit_once('.')
            .ok_or_else(|| refuse("expected `milestone:<id>.<field>`"))?;
        if id.trim().is_empty() || id.trim() != id {
            return Err(refuse("the milestone id is empty"));
        }
        return Ok(RecordTarget::Milestone {
            id: id.to_string(),
            field: known(field, MILESTONE_FIELDS)
                .ok_or_else(|| refuse(&expected("a milestone field", MILESTONE_FIELDS)))?,
        });
    }
    if let Some(section) = raw.strip_prefix("brief#") {
        return Ok(RecordTarget::Brief {
            section: known(section, BRIEF_SECTIONS)
                .ok_or_else(|| refuse(&expected("a brief section", BRIEF_SECTIONS)))?,
        });
    }
    if raw.starts_with(DESIGN_DIR) {
        let (file, anchor) = raw
            .split_once('#')
            .ok_or_else(|| refuse("expected `design/<file>.md#<anchor>`"))?;
        let plain = file[DESIGN_DIR.len()..]
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != ".." && !part.contains('\\'));
        if !plain || !file.ends_with(".md") {
            return Err(refuse(
                "the design file is not a plain `design/<file>.md` path",
            ));
        }
        if anchor.is_empty() {
            return Err(refuse("the design anchor is empty"));
        }
        return Ok(RecordTarget::Design {
            file: file.to_string(),
            anchor: anchor.to_string(),
        });
    }
    Err(refuse(
        "expected `task:`, `decision:`, `contract:`, `milestone:`, `handover:`, `brief#` or `design/`",
    ))
}

fn is_ref(candidate: &str) -> bool {
    !candidate.is_empty()
        && candidate
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

fn known(word: &str, vocabulary: &[&'static str]) -> Option<&'static str> {
    vocabulary.iter().copied().find(|entry| *entry == word)
}

fn expected(what: &str, vocabulary: &[&str]) -> String {
    format!("expected {what}, one of {}", vocabulary.join(", "))
}

impl RecordTarget {
    /// The repo-relative file holding the record.
    pub(super) fn file(&self, slug: &str) -> String {
        match self {
            Self::TaskField { .. }
            | Self::SpecField { .. }
            | Self::SpecEntry { .. }
            | Self::Milestone { .. } => format!(".claude/flows/{slug}/tasks.toml"),
            Self::Decision { .. } | Self::Contract { .. } | Self::Handover { .. } => {
                format!(".claude/flows/{slug}/plan.toml")
            }
            Self::Brief { .. } => format!("docs/flows/{slug}/brief.md"),
            Self::Design { file, .. } => format!("docs/flows/{slug}/{file}"),
        }
    }

    /// The record within [`Self::file`]: a task ref, `milestone:<id>`, a plan
    /// record id, a brief section or a design anchor.
    pub(super) fn symbol(&self) -> String {
        match self {
            Self::TaskField { r#ref, .. }
            | Self::SpecField { r#ref, .. }
            | Self::SpecEntry { r#ref, .. } => r#ref.clone(),
            Self::Milestone { id, .. } => format!("{MILESTONE_REF_PREFIX}{id}"),
            Self::Decision { id, .. } | Self::Contract { id, .. } | Self::Handover { id, .. } => {
                id.clone()
            }
            Self::Brief { section } => (*section).to_string(),
            Self::Design { anchor, .. } => anchor.clone(),
        }
    }

    /// The field the target edits as a `[[changes]]` row names it, so a revert
    /// joins on (`symbol`, `field_name`). Prose has no field and reads `text`.
    pub(super) fn field_name(&self) -> String {
        match self {
            Self::TaskField { field, .. }
            | Self::Milestone { field, .. }
            | Self::Handover { field, .. } => (*field).to_string(),
            Self::SpecField { field, .. } => format!("spec.{field}"),
            Self::SpecEntry { list, .. } => format!("spec.{list}"),
            Self::Decision { key, .. } | Self::Contract { key, .. } => (*key).to_string(),
            Self::Brief { .. } | Self::Design { .. } => "text".to_string(),
        }
    }

    fn reads_tasks(&self) -> bool {
        matches!(
            self,
            Self::TaskField { .. }
                | Self::SpecField { .. }
                | Self::SpecEntry { .. }
                | Self::Milestone { .. }
        )
    }

    fn reads_plan(&self) -> bool {
        matches!(
            self,
            Self::Decision { .. } | Self::Contract { .. } | Self::Handover { .. }
        )
    }
}

impl fmt::Display for RecordTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TaskField { r#ref, field } => write!(f, "task:{}.{field}", r#ref),
            Self::SpecField { r#ref, field } => write!(f, "task:{}.spec.{field}", r#ref),
            Self::SpecEntry {
                r#ref,
                list,
                index,
                append,
            } => match (append, index) {
                (true, _) => write!(f, "task:{}.spec.{list}[+]", r#ref),
                (false, Some(index)) => write!(f, "task:{}.spec.{list}[{index}]", r#ref),
                (false, None) => write!(f, "task:{}.spec.{list}", r#ref),
            },
            Self::Decision { id, key } => write!(f, "decision:{id}.{key}"),
            Self::Contract { id, key } => write!(f, "contract:{id}.{key}"),
            Self::Milestone { id, field } => write!(f, "milestone:{id}.{field}"),
            Self::Handover { id, field } => write!(f, "handover:{id}.{field}"),
            Self::Brief { section } => write!(f, "brief#{section}"),
            Self::Design { file, anchor } => write!(f, "{file}#{anchor}"),
        }
    }
}

/// The current value of the record `target` names in flow `slug`. A
/// `SpecEntry` resolves to its whole list, which [`match_entry`] then searches;
/// a brief section to its body; a design anchor to its section, heading line
/// included.
pub(super) fn resolve(slug: &str, target: &RecordTarget) -> Result<JsonValue> {
    let integrity = ReadIntegrityArgs {
        verify_integrity: false,
        strict_read: false,
    };
    if target.reads_tasks() {
        let store = load_store(&resolve_store_path(Some(slug), None)?, &integrity)?;
        return task_value(&store, target);
    }
    if target.reads_plan() {
        let path = crate::plan::store::resolve(slug)?;
        let plan = crate::plan::store::load(&path, &integrity)?;
        return plan_value(&crate::plan::schema::to_toml(&plan), target);
    }
    let authored = crate::plan::store::authored_dir(slug)?;
    match target {
        RecordTarget::Brief { section } => brief_value(&authored, section),
        RecordTarget::Design { file, anchor } => {
            crate::plan::check::design_section(&authored, &format!("{file}#{anchor}"))
                .map(JsonValue::String)
                .ok_or_else(|| invalid(format!("target `{target}` names no design section")))
        }
        _ => unreachable!("every record target reads tasks, plan or prose"),
    }
}

fn task_value(store: &TaskStore, target: &RecordTarget) -> Result<JsonValue> {
    let doc = crate::tasks::schema::to_toml(store);
    let not_found = |what: String| invalid(format!("target `{target}`: {what}"));
    match target {
        RecordTarget::TaskField { r#ref, field } => {
            let row = keyed(&doc, "items", "ref", r#ref)
                .ok_or_else(|| not_found(format!("no task with ref `{}`", r#ref)))?;
            Ok(row.get(*field).map(toml_to_json).unwrap_or(JsonValue::Null))
        }
        RecordTarget::SpecField { r#ref, field }
        | RecordTarget::SpecEntry {
            r#ref, list: field, ..
        } => {
            if keyed(&doc, "items", "ref", r#ref).is_none() {
                return Err(not_found(format!("no task with ref `{}`", r#ref)));
            }
            let value = keyed(&doc, "specs", "ref", r#ref)
                .and_then(|spec| spec.get(*field))
                .map(toml_to_json)
                .unwrap_or_else(|| empty_spec_field(field));
            Ok(without_probes(value))
        }
        RecordTarget::Milestone { id, field } => {
            let checkpoint = keyed(&doc, "checkpoints", "id", id)
                .ok_or_else(|| not_found(format!("no milestone `{id}`")))?;
            if *field == "goal" {
                return Ok(checkpoint
                    .get("rationale")
                    .map(toml_to_json)
                    .unwrap_or_else(|| JsonValue::String(String::new())));
            }
            Ok(keyed(&doc, "milestones", "id", id)
                .and_then(|spec| spec.get(*field))
                .map(toml_to_json)
                .unwrap_or_else(|| match *field {
                    "gates" => JsonValue::Array(Vec::new()),
                    "elaboration" => JsonValue::String(ELABORATION_UPFRONT.to_string()),
                    _ => JsonValue::String(String::new()),
                }))
        }
        _ => unreachable!("task_value is called only for a task-store target"),
    }
}

/// A row with no `[[specs]]` entry reads as an empty spec, which is what the
/// task update path starts from when it writes the first field.
fn empty_spec_field(field: &str) -> JsonValue {
    if SPEC_LISTS.contains(&field) {
        JsonValue::Array(Vec::new())
    } else {
        JsonValue::String(String::new())
    }
}

fn plan_value(doc: &TomlValue, target: &RecordTarget) -> Result<JsonValue> {
    let (array, id, key) = match target {
        RecordTarget::Decision { id, key } => ("decision", id, *key),
        RecordTarget::Contract { id, key } => ("contract", id, *key),
        RecordTarget::Handover { id, field } => ("handover", id, *field),
        _ => unreachable!("plan_value is called only for a plan-store target"),
    };
    let record = keyed(doc, array, "id", id)
        .ok_or_else(|| invalid(format!("target `{target}`: no {array} `{id}`")))?;
    Ok(record.get(key).map(toml_to_json).unwrap_or(JsonValue::Null))
}

fn keyed<'a>(doc: &'a TomlValue, array: &str, key: &str, wanted: &str) -> Option<&'a TomlValue> {
    doc.get(array)?
        .as_array()?
        .iter()
        .find(|entry| entry.get(key).and_then(TomlValue::as_str) == Some(wanted))
}

/// The brief is read with `\r\n` as `\n`, and only when it resolves inside the
/// flow's authored directory, so a symlinked brief cannot surface another file.
fn brief_value(authored: &Path, section: &str) -> Result<JsonValue> {
    let path = authored.join(BRIEF_FILE);
    let refuse = |why: String| invalid(format!("target `brief#{section}`: {why}"));
    let resolved = path
        .canonicalize()
        .map_err(|_| refuse("the flow has no brief.md".to_string()))?;
    let contained = authored
        .canonicalize()
        .is_ok_and(|anchor| resolved.starts_with(anchor));
    if !contained {
        return Err(refuse(
            "brief.md resolves outside the flow's directory".to_string(),
        ));
    }
    let bytes = std::fs::read(&resolved)?;
    let text = String::from_utf8(bytes)
        .map_err(|_| refuse("brief.md is not UTF-8".to_string()))?
        .replace("\r\n", "\n");
    let mut bodies = section_bodies(&text, section);
    match bodies.len() {
        1 => Ok(JsonValue::String(bodies.remove(0))),
        0 => Err(refuse(format!("brief.md has no `{section}` section"))),
        n => Err(refuse(format!("brief.md has {n} `{section}` sections"))),
    }
}

/// The index of the one entry of `list` equal to `expect`, comparing objects
/// with any `probe` key removed on both sides. No match and several matches are
/// both refused, since the target's index is only a hint.
pub(super) fn match_entry(list: &[JsonValue], expect: &JsonValue) -> Result<usize> {
    let expect = without_probe(expect.clone());
    let found: Vec<usize> = list
        .iter()
        .enumerate()
        .filter(|(_, entry)| without_probe((*entry).clone()) == expect)
        .map(|(at, _)| at)
        .collect();
    match found.as_slice() {
        [one] => Ok(*one),
        [] => Err(invalid("no list entry equals `expect`")),
        many => Err(invalid(format!(
            "{} list entries equal `expect`, so the entry is ambiguous",
            many.len()
        ))),
    }
}

fn without_probe(mut entry: JsonValue) -> JsonValue {
    if let Some(map) = entry.as_object_mut() {
        map.remove(PROBE_KEY);
    }
    entry
}

fn without_probes(value: JsonValue) -> JsonValue {
    match value {
        JsonValue::Array(entries) => {
            JsonValue::Array(entries.into_iter().map(without_probe).collect())
        }
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const TASKS: &str = r#"
schema_version = 1

[[checkpoints]]
id = "A"
rationale = "the parser lands"

[[milestones]]
id = "A"
gates = ["cargo test"]
elaboration = "upfront"
defer_reason = ""

[[specs]]
ref = "parse-it"
outcome = "targets parse"
constraints = ["no new deps", "no new deps"]
executor = "deep"

[[specs.accept]]
behaviour = "parses"
check = "cargo test parse"
kind = "test"
positive = ""
writes_build = false

[specs.accept.probe]
at = "2026-11-09"
neg_rc = 1
pos_rc = 0
verdict = "healthy"

[[items]]
id = 1
ref = "parse-it"
title = "Parse it"
effort = "S"
status = "pending"
files = ["src/a.rs"]
needs = []
action = "parse"

[[items]]
id = 2
ref = "bare"
title = "Bare"
effort = "M"
status = "pending"
"#;

    const PLAN: &str = r#"
schema_version = 1

[[decision]]
id = "D1"
decision = "one carrier"
why = "fewer moving parts"
provenance = "user-stated"
door = "two-way"

[[contract]]
id = "C2"
kind = "interface"
claim = "targets resolve"
tasks = [1]

[[handover]]
id = "H1"
command = "cargo install --path tomlctl"
source_task = 1
done = false
"#;

    fn tasks() -> TaskStore {
        let doc: TomlValue = toml::from_str(TASKS).expect("fixture parses");
        crate::tasks::schema::from_toml(&doc).expect("fixture is a task store")
    }

    fn plan() -> TomlValue {
        let doc: TomlValue = toml::from_str(PLAN).expect("fixture parses");
        let store = crate::plan::schema::from_toml(&doc).expect("fixture is a plan store");
        crate::plan::schema::to_toml(&store)
    }

    fn task(raw: &str) -> JsonValue {
        task_value(&tasks(), &parse(raw).expect(raw)).expect(raw)
    }

    fn refused(raw: &str) -> String {
        format!("{:#}", parse(raw).expect_err(raw))
    }

    #[test]
    fn every_target_kind_parses_and_prints_back() {
        for raw in [
            "task:parse-it.files",
            "task:parse-it.spec.outcome",
            "task:parse-it.spec.accept[0]",
            "task:parse-it.spec.constraints[+]",
            "decision:D1.why",
            "contract:C2.tasks",
            "milestone:A.gates",
            "handover:H1.command",
            "brief#Scope budget",
            "design/flow/api.md#record-targets",
        ] {
            let target = parse(raw).expect(raw);
            assert_eq!(target.to_string(), raw);
        }
    }

    #[test]
    fn file_symbol_and_field_name_follow_the_record() {
        let entry = parse("task:parse-it.spec.accept[3]").unwrap();
        assert_eq!(entry.file("s"), ".claude/flows/s/tasks.toml");
        assert_eq!(entry.symbol(), "parse-it");
        assert_eq!(entry.field_name(), "spec.accept");
        let milestone = parse("milestone:A.goal").unwrap();
        assert_eq!(milestone.symbol(), "milestone:A");
        assert_eq!(milestone.field_name(), "goal");
        let decision = parse("decision:D3.why").unwrap();
        assert_eq!(decision.file("s"), ".claude/flows/s/plan.toml");
        assert_eq!(decision.symbol(), "D3");
        let brief = parse("brief#Intent").unwrap();
        assert_eq!(brief.file("s"), "docs/flows/s/brief.md");
        assert_eq!(brief.symbol(), "Intent");
        let design = parse("design/api.md#grammar").unwrap();
        assert_eq!(design.file("s"), "docs/flows/s/design/api.md");
        assert_eq!(design.symbol(), "grammar");
    }

    #[test]
    fn an_entry_target_carries_its_hint_or_append() {
        assert_eq!(
            parse("task:x.spec.accept[2]").unwrap(),
            RecordTarget::SpecEntry {
                r#ref: "x".to_string(),
                list: "accept",
                index: Some(2),
                append: false,
            }
        );
        assert_eq!(
            parse("task:x.spec.premises[+]").unwrap(),
            RecordTarget::SpecEntry {
                r#ref: "x".to_string(),
                list: "premises",
                index: None,
                append: true,
            }
        );
    }

    #[test]
    fn malformed_targets_are_refused() {
        assert!(refused("task:parse-it.priority").contains("expected a task field"));
        assert!(refused("task:parse-it.title").contains("`title` is not a target"));
        assert!(refused("task:.action").contains("ref is empty"));
        assert!(refused("task:parse-it.spec.outcome[0]").contains("a spec list"));
        assert!(refused("task:parse-it.spec.accept[x]").contains("`[<n>]` or `[+]`"));
        assert!(refused("decision:3.why").contains("`D<n>`"));
        assert!(refused("decision:D3.id").contains("expected a key"));
        assert!(refused("brief#Summary").contains("a brief section"));
        assert!(refused("design/../x.md#a").contains("plain"));
        assert!(refused("design/x.md#").contains("anchor is empty"));
        assert!(refused("plan#Approach").contains("expected `task:`"));
    }

    #[test]
    fn task_targets_resolve_to_stored_values() {
        assert_eq!(task("task:parse-it.files"), json!(["src/a.rs"]));
        assert_eq!(task("task:parse-it.action"), json!("parse"));
        assert_eq!(task("task:parse-it.spec.outcome"), json!("targets parse"));
        assert_eq!(task("task:bare.spec.accept"), json!([]));
        assert_eq!(task("task:bare.spec.executor"), json!(""));
        assert_eq!(task("milestone:A.goal"), json!("the parser lands"));
        assert_eq!(task("milestone:A.gates"), json!(["cargo test"]));
        let err = task_value(&tasks(), &parse("task:gone.action").unwrap()).unwrap_err();
        assert!(format!("{err:#}").contains("no task with ref `gone`"));
    }

    #[test]
    fn an_accept_entry_resolves_without_its_probe() {
        let list = task("task:parse-it.spec.accept[0]");
        assert!(list[0].get("probe").is_none(), "{list}");
        assert_eq!(list[0]["check"], "cargo test parse");
    }

    #[test]
    fn a_probed_entry_matches_an_expect_without_probe() {
        let stored = vec![json!({
            "behaviour": "parses",
            "check": "cargo test parse",
            "probe": {"verdict": "healthy"},
        })];
        let expect = json!({"behaviour": "parses", "check": "cargo test parse"});
        assert_eq!(match_entry(&stored, &expect).unwrap(), 0);
    }

    #[test]
    fn duplicate_and_missing_entries_are_refused() {
        let list = task("task:parse-it.spec.constraints[0]");
        let list = list.as_array().unwrap();
        let err = match_entry(list, &json!("no new deps")).unwrap_err();
        assert!(format!("{err:#}").contains("2 list entries"), "{err:#}");
        let err = match_entry(list, &json!("absent")).unwrap_err();
        assert!(format!("{err:#}").contains("no list entry"), "{err:#}");
    }

    #[test]
    fn plan_records_resolve_by_id_and_key() {
        let doc = plan();
        let value = |raw: &str| plan_value(&doc, &parse(raw).unwrap());
        assert_eq!(
            value("decision:D1.why").unwrap(),
            json!("fewer moving parts")
        );
        assert_eq!(value("contract:C2.tasks").unwrap(), json!([1]));
        assert_eq!(value("handover:H1.done").unwrap(), json!(false));
        assert!(value("decision:D9.why").is_err());
    }

    #[test]
    fn a_brief_section_resolves_to_its_body() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(BRIEF_FILE),
            "# x\r\n\r\n## Intent\r\n\r\nShip it.\r\n\r\n## Non-goals\r\n\r\nNone.\r\n",
        )
        .unwrap();
        assert_eq!(
            brief_value(dir.path(), "Intent").unwrap(),
            json!("\nShip it.\n\n")
        );
        assert!(brief_value(dir.path(), "Unknowns").is_err());
    }
}
