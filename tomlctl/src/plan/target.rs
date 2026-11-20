//! Record targets: the grammar a plan-family finding on a v2 flow names the
//! record it edits in, and the current value of that record, read through the
//! owning store's review reader so it is the value a review edit compares.
//!
//! The fields a target may name are each store's own review fields, so a
//! target parses exactly when the store's review edit would accept its field.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde_json::Value as JsonValue;

use super::schema::{PlanStore, RecordKind};
use super::{check, milestone, open, records, store};
use crate::cli::ReadIntegrityArgs;
use crate::errors::{ErrorKind, tagged_err};
use crate::integrity::sha256_hex_crlf_folded;
use crate::tasks::markdown::FenceState;
use crate::tasks::schema::Store;
use crate::tasks::slug::{dedupe_refs, derive_ref};
use crate::tasks::{
    REVIEW_ROW_FIELDS, REVIEW_SPEC_FIELDS, REVIEW_SPEC_LISTS, Slot, load_store, resolve_store_path,
    review_current,
};

const DESIGN_DIR: &str = "design/";

/// One record a plan finding edits, parsed from its `target`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RecordTarget {
    TaskField {
        r#ref: String,
        field: &'static str,
    },
    SpecField {
        r#ref: String,
        field: &'static str,
    },
    /// A `Slot::At` index is a hint only: the entry edited is the one equal to
    /// `expect`.
    SpecEntry {
        r#ref: String,
        list: &'static str,
        slot: Slot,
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

fn invalid(message: String) -> anyhow::Error {
    tagged_err(ErrorKind::Validation, None, message)
}

pub(crate) fn parse(raw: &str) -> Result<RecordTarget> {
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
                    let list = known(list, &REVIEW_SPEC_LISTS)
                        .ok_or_else(|| refuse(&expected("a spec list", &REVIEW_SPEC_LISTS)))?;
                    let slot = slot
                        .strip_suffix(']')
                        .ok_or_else(|| refuse("an entry index ends with `]`"))?;
                    let slot = match slot {
                        "+" => Slot::Append,
                        digits
                            if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) =>
                        {
                            Slot::At(
                                digits
                                    .parse()
                                    .map_err(|_| refuse("the entry index is out of range"))?,
                            )
                        }
                        _ => return Err(refuse("an entry index is `[<n>]` or `[+]`")),
                    };
                    Ok(RecordTarget::SpecEntry { r#ref, list, slot })
                }
                None => Ok(RecordTarget::SpecField {
                    r#ref,
                    field: known(spec, &REVIEW_SPEC_FIELDS)
                        .ok_or_else(|| refuse(&expected("a spec field", &REVIEW_SPEC_FIELDS)))?,
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
            field: known(field, &REVIEW_ROW_FIELDS)
                .ok_or_else(|| refuse(&expected("a task field", &REVIEW_ROW_FIELDS)))?,
        });
    }
    for (prefix, letter, kind) in [
        ("decision:", 'D', RecordKind::Decision),
        ("contract:", 'C', RecordKind::Contract),
        ("handover:", 'H', RecordKind::Handover),
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
        let keys = records::review_keys(kind);
        let key = known(key, keys).ok_or_else(|| refuse(&expected("a key", keys)))?;
        let id = id.to_string();
        return Ok(match kind {
            RecordKind::Decision => RecordTarget::Decision { id, key },
            RecordKind::Contract => RecordTarget::Contract { id, key },
            RecordKind::Handover => RecordTarget::Handover { id, field: key },
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
            field: known(field, &milestone::REVIEW_FIELDS)
                .ok_or_else(|| refuse(&expected("a milestone field", &milestone::REVIEW_FIELDS)))?,
        });
    }
    if let Some(section) = raw.strip_prefix("brief#") {
        return Ok(RecordTarget::Brief {
            section: known(section, &open::BRIEF_SECTIONS)
                .ok_or_else(|| refuse(&expected("a brief section", &open::BRIEF_SECTIONS)))?,
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
    /// The field the target edits as a `[[changes]]` row names it, so a revert
    /// joins on (`symbol`, `field_name`). Prose has no field and reads `text`.
    pub(crate) fn field_name(&self) -> String {
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
}

impl fmt::Display for RecordTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TaskField { r#ref, field } => write!(f, "task:{}.{field}", r#ref),
            Self::SpecField { r#ref, field } => write!(f, "task:{}.spec.{field}", r#ref),
            Self::SpecEntry { r#ref, list, slot } => match slot {
                Slot::Append => write!(f, "task:{}.spec.{list}[+]", r#ref),
                Slot::At(index) => write!(f, "task:{}.spec.{list}[{index}]", r#ref),
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

/// The current value of the store record `target` names in flow `slug`, or
/// `None` for a brief or design target, which is prose. A `SpecEntry` reads as
/// its whole list.
pub(crate) fn current(slug: &str, target: &RecordTarget) -> Result<Option<JsonValue>> {
    let integrity = ReadIntegrityArgs {
        verify_integrity: false,
        strict_read: false,
    };
    let missing = |what: String| invalid(format!("target `{target}`: {what}"));
    let value = match target {
        RecordTarget::TaskField { r#ref, .. }
        | RecordTarget::SpecField { r#ref, .. }
        | RecordTarget::SpecEntry { r#ref, .. } => {
            let tasks = load_store(&resolve_store_path(Some(slug), None)?, &integrity)?;
            review_current(&tasks, r#ref, &target.field_name())
                .map_err(|err| err.context(format!("target `{target}`")))?
        }
        RecordTarget::Milestone { id, field } => {
            let tasks = load_store(&resolve_store_path(Some(slug), None)?, &integrity)?;
            milestone::milestone_value(&tasks, id, field)
                .ok_or_else(|| missing(format!("no milestone `{id}`")))?
        }
        RecordTarget::Decision { id, key }
        | RecordTarget::Contract { id, key }
        | RecordTarget::Handover { id, field: key } => {
            let kind = match target {
                RecordTarget::Decision { .. } => RecordKind::Decision,
                RecordTarget::Contract { .. } => RecordKind::Contract,
                _ => RecordKind::Handover,
            };
            let plan = store::load(&store::resolve(slug)?, &integrity)?;
            records::record_value(&plan, kind, id, key)
                .ok_or_else(|| missing(format!("no {} `{id}`", kind_name(kind))))?
        }
        RecordTarget::Brief { .. } | RecordTarget::Design { .. } => return Ok(None),
    };
    Ok(Some(value))
}

fn kind_name(kind: RecordKind) -> &'static str {
    match kind {
        RecordKind::Decision => "decision",
        RecordKind::Contract => "contract",
        RecordKind::Handover => "handover",
    }
}

/// Each field-level plan target, keyed as [`digest_key`] spells it, to the
/// SHA-256 of its current value.
pub(crate) type PlanDigest = BTreeMap<String, String>;

/// The digest of every target flow `slug` holds now. A brief or design file
/// that is absent, or resolves outside the flow's authored directory, adds none.
pub(crate) fn plan_digest(slug: &str) -> Result<PlanDigest> {
    let integrity = ReadIntegrityArgs {
        verify_integrity: false,
        strict_read: false,
    };
    let tasks = load_store(&resolve_store_path(Some(slug), None)?, &integrity)?;
    let plan = store::load(&store::resolve(slug)?, &integrity)?;
    digest_of(&plan, &tasks, &store::authored_dir(slug)?)
}

fn digest_of(plan: &PlanStore, tasks: &Store, authored: &Path) -> Result<PlanDigest> {
    let mut digest = PlanDigest::new();
    let mut put = |target: RecordTarget, value: &JsonValue| {
        digest.insert(target.to_string(), hash_json(value));
    };
    for row in &tasks.items {
        for field in REVIEW_ROW_FIELDS {
            let value = review_current(tasks, &row.r#ref, field)?;
            let r#ref = row.r#ref.clone();
            put(RecordTarget::TaskField { r#ref, field }, &value);
        }
        for field in REVIEW_SPEC_FIELDS {
            let value = review_current(tasks, &row.r#ref, &format!("spec.{field}"))?;
            let r#ref = row.r#ref.clone();
            put(RecordTarget::SpecField { r#ref, field }, &value);
        }
    }
    for checkpoint in &tasks.checkpoints {
        for field in milestone::REVIEW_FIELDS {
            if let Some(value) = milestone::milestone_value(tasks, &checkpoint.id, field) {
                let id = checkpoint.id.clone();
                put(RecordTarget::Milestone { id, field }, &value);
            }
        }
    }
    let ids = |kind: RecordKind| -> Vec<String> {
        match kind {
            RecordKind::Decision => plan.decisions.iter().map(|d| d.id.clone()).collect(),
            RecordKind::Contract => plan.contracts.iter().map(|c| c.id.clone()).collect(),
            RecordKind::Handover => plan.handovers.iter().map(|h| h.id.clone()).collect(),
        }
    };
    for kind in [
        RecordKind::Decision,
        RecordKind::Contract,
        RecordKind::Handover,
    ] {
        for id in ids(kind) {
            for &key in records::review_keys(kind) {
                let Some(value) = records::record_value(plan, kind, &id, key) else {
                    continue;
                };
                let id = id.clone();
                let target = match kind {
                    RecordKind::Decision => RecordTarget::Decision { id, key },
                    RecordKind::Contract => RecordTarget::Contract { id, key },
                    RecordKind::Handover => RecordTarget::Handover { id, field: key },
                };
                put(target, &value);
            }
        }
    }
    if let Some(bytes) = store::read_brief(authored)? {
        let text = String::from_utf8_lossy(&bytes).replace("\r\n", "\n");
        for section in open::BRIEF_SECTIONS {
            if let Some((_, body)) = check::design_section_in(&text, &derive_ref(section)) {
                digest.insert(
                    RecordTarget::Brief { section }.to_string(),
                    sha256_hex_crlf_folded(body.as_bytes()),
                );
            }
        }
    }
    for file in design_files(authored)? {
        let Some(path) = check::design_path(authored, &file) else {
            continue;
        };
        let source = String::from_utf8_lossy(
            &fs::read(&path).with_context(|| format!("reading {}", path.display()))?,
        )
        .replace("\r\n", "\n");
        for anchor in heading_anchors(&source) {
            if let Some((_, body)) = check::design_section_in(&source, &anchor) {
                digest.insert(
                    format!("{file}#{anchor}"),
                    sha256_hex_crlf_folded(body.as_bytes()),
                );
            }
        }
    }
    Ok(digest)
}

/// `preserve_order` keeps a store reader's key order, which is fixed per field.
fn hash_json(value: &JsonValue) -> String {
    sha256_hex_crlf_folded(value.to_string().as_bytes())
}

/// Every `design/**/*.md` path under `authored`, `/`-separated. A symlinked
/// directory is not entered; `check::design_path` holds each file inside.
fn design_files(authored: &Path) -> Result<Vec<String>> {
    let mut found = Vec::new();
    let mut pending = vec![DESIGN_DIR.trim_end_matches('/').to_string()];
    while let Some(relative) = pending.pop() {
        let dir = authored.join(&relative);
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => return Err(err).with_context(|| format!("listing {}", dir.display())),
        };
        for entry in entries {
            let entry = entry.with_context(|| format!("listing {}", dir.display()))?;
            let Ok(name) = entry.file_name().into_string() else {
                continue;
            };
            let path = format!("{relative}/{name}");
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                pending.push(path);
            } else if name.ends_with(".md") {
                found.push(path);
            }
        }
    }
    found.sort();
    Ok(found)
}

/// The heading anchors of `source` as `check::design_section_in` resolves them.
fn heading_anchors(source: &str) -> Vec<String> {
    let mut fence = FenceState::default();
    let mut anchors: Vec<String> = source
        .lines()
        .filter(|line| !fence.consume(line))
        .filter_map(|line| check::atx_heading(line).map(|(_, title)| derive_ref(title)))
        .collect();
    dedupe_refs(&mut anchors);
    anchors
}

/// The digest key a target falls under: an entry target is its whole list, and
/// a string that is no target stands for itself.
pub(crate) fn digest_key(raw: &str) -> String {
    match parse(raw) {
        Ok(RecordTarget::SpecEntry { r#ref, list, .. }) => {
            RecordTarget::SpecField { r#ref, field: list }.to_string()
        }
        Ok(target) => target.to_string(),
        Err(_) => raw.to_string(),
    }
}

/// The keys of `current` whose digest differs from, or is absent in, the
/// latest of `finished` holding that key. `finished` runs oldest first; with
/// none, nothing was ever digested and the delta is empty.
pub(crate) fn review_delta(current: &PlanDigest, finished: &[&PlanDigest]) -> Vec<String> {
    if finished.is_empty() {
        return Vec::new();
    }
    current
        .iter()
        .filter(|(key, value)| finished.iter().rev().find_map(|d| d.get(*key)) != Some(*value))
        .map(|(key, _)| key.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
            "handover:H1.action",
            "brief#Scope budget",
            "design/flow/api.md#record-targets",
        ] {
            let target = parse(raw).expect(raw);
            assert_eq!(target.to_string(), raw);
        }
    }

    #[test]
    fn the_field_name_follows_the_record() {
        let name = |raw: &str| parse(raw).unwrap().field_name();
        assert_eq!(name("task:parse-it.spec.accept[3]"), "spec.accept");
        assert_eq!(name("task:parse-it.spec.outcome"), "spec.outcome");
        assert_eq!(name("milestone:A.goal"), "goal");
        assert_eq!(name("decision:D3.why"), "why");
        assert_eq!(name("brief#Intent"), "text");
    }

    #[test]
    fn an_entry_target_carries_its_hint_or_append() {
        assert_eq!(
            parse("task:x.spec.accept[2]").unwrap(),
            RecordTarget::SpecEntry {
                r#ref: "x".to_string(),
                list: "accept",
                slot: Slot::At(2),
            }
        );
        assert_eq!(
            parse("task:x.spec.premises[+]").unwrap(),
            RecordTarget::SpecEntry {
                r#ref: "x".to_string(),
                list: "premises",
                slot: Slot::Append,
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
        assert!(refused("contract:C3.id").contains("expected a key"));
        assert!(refused("handover:H1.id").contains("expected a key"));
        assert!(refused("milestone:A.status").contains("a milestone field"));
        assert!(refused("brief#Summary").contains("a brief section"));
        assert!(refused("design/../x.md#a").contains("plain"));
        assert!(refused("design/x.md#").contains("anchor is empty"));
        assert!(refused("plan#Approach").contains("expected `task:`"));
    }

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
kind = "forward"
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
action = "cargo install --path tomlctl"
source_task = 1
done = false
"#;

    fn tasks() -> crate::tasks::Store {
        let doc: toml::Value = toml::from_str(TASKS).expect("fixture parses");
        crate::tasks::schema::from_toml(&doc).expect("fixture is a task store")
    }

    /// What the task store's reader reports for the record `raw` names.
    fn task(raw: &str) -> JsonValue {
        let store = tasks();
        let target = parse(raw).expect(raw);
        match &target {
            RecordTarget::Milestone { id, field } => milestone::milestone_value(&store, id, field),
            RecordTarget::TaskField { r#ref, .. }
            | RecordTarget::SpecField { r#ref, .. }
            | RecordTarget::SpecEntry { r#ref, .. } => {
                review_current(&store, r#ref, &target.field_name()).ok()
            }
            other => panic!("{other} is not a task-store target"),
        }
        .expect(raw)
    }

    #[test]
    fn task_targets_read_through_the_task_store() {
        assert_eq!(task("task:parse-it.files"), json!(["src/a.rs"]));
        assert_eq!(task("task:parse-it.action"), json!("parse"));
        assert_eq!(task("task:parse-it.spec.outcome"), json!("targets parse"));
        assert_eq!(task("task:bare.spec.accept"), json!([]));
        assert_eq!(task("task:bare.spec.executor"), json!(""));
        assert_eq!(task("task:bare.detail"), json!(""));
        assert_eq!(task("milestone:A.goal"), json!("the parser lands"));
        assert_eq!(task("milestone:A.gates"), json!(["cargo test"]));
        let err = review_current(&tasks(), "gone", "action").unwrap_err();
        assert!(format!("{err:#}").contains("no task with ref `gone`"));
    }

    #[test]
    fn an_accept_entry_reads_without_its_probe() {
        let list = task("task:parse-it.spec.accept[0]");
        assert!(list[0].get("probe").is_none(), "{list}");
        assert_eq!(list[0]["check"], "cargo test parse");
    }

    #[test]
    fn plan_records_read_by_id_and_key() {
        let doc: toml::Value = toml::from_str(PLAN).expect("fixture parses");
        let plan = super::super::schema::from_toml(&doc).expect("fixture is a plan store");
        let value = |raw: &str| match parse(raw).unwrap() {
            RecordTarget::Decision { id, key } => {
                records::record_value(&plan, RecordKind::Decision, &id, key)
            }
            RecordTarget::Contract { id, key } => {
                records::record_value(&plan, RecordKind::Contract, &id, key)
            }
            RecordTarget::Handover { id, field } => {
                records::record_value(&plan, RecordKind::Handover, &id, field)
            }
            other => panic!("{other} is not a plan-store target"),
        };
        assert_eq!(value("decision:D1.why"), Some(json!("fewer moving parts")));
        assert_eq!(value("contract:C2.tasks"), Some(json!([1])));
        assert_eq!(value("handover:H1.done"), Some(json!(false)));
        assert_eq!(value("decision:D9.why"), None);
    }

    fn plan() -> PlanStore {
        let doc: toml::Value = toml::from_str(PLAN).expect("fixture parses");
        super::super::schema::from_toml(&doc).expect("fixture is a plan store")
    }

    #[test]
    fn the_digest_holds_one_key_per_field_level_target() {
        let authored = tempfile::tempdir().expect("tempdir");
        fs::write(
            authored.path().join("brief.md"),
            "## Intent\nStop it.\n\n## Non-goals\nNone.\n",
        )
        .expect("brief");
        fs::create_dir_all(authored.path().join("design/flow")).expect("design dir");
        fs::write(
            authored.path().join("design/flow/api.md"),
            "# API\n\n## Record targets\nOne.\n\n```\n## not a heading\n```\n",
        )
        .expect("design file");
        let digest = digest_of(&plan(), &tasks(), authored.path()).expect("the digest");
        for key in [
            "task:parse-it.files",
            "task:parse-it.spec.accept",
            "task:bare.spec.outcome",
            "milestone:A.gates",
            "decision:D1.why",
            "contract:C2.tasks",
            "handover:H1.done",
            "brief#Intent",
            "brief#Non-goals",
            "design/flow/api.md#api",
            "design/flow/api.md#record-targets",
        ] {
            assert!(digest.contains_key(key), "{key} missing from {digest:?}");
        }
        assert!(!digest.keys().any(|key| key.contains('[')), "{digest:?}");
        assert!(!digest.contains_key("brief#Unknowns"), "an absent section");
        assert!(!digest.contains_key("design/flow/api.md#not-a-heading"));
        assert!(!digest.contains_key("task:parse-it.title"));
    }

    #[test]
    fn an_edited_value_changes_only_its_own_digest() {
        let authored = tempfile::tempdir().expect("tempdir");
        let before = digest_of(&plan(), &tasks(), authored.path()).expect("the digest");
        let mut edited = plan();
        edited.decisions[0].why = "a sharper reason".to_string();
        let after = digest_of(&edited, &tasks(), authored.path()).expect("the digest");
        let changed: Vec<&String> = after
            .iter()
            .filter(|(key, value)| before.get(*key) != Some(*value))
            .map(|(key, _)| key)
            .collect();
        assert_eq!(changed, ["decision:D1.why"]);
    }

    #[test]
    fn an_entry_target_digests_under_its_whole_list() {
        assert_eq!(
            digest_key("task:x.spec.accept[2]"),
            "task:x.spec.accept".to_string()
        );
        assert_eq!(
            digest_key("task:x.spec.premises[+]"),
            "task:x.spec.premises"
        );
        assert_eq!(digest_key("decision:D1.why"), "decision:D1.why");
        assert_eq!(digest_key("not a target"), "not a target");
    }

    #[test]
    fn the_delta_compares_each_target_with_the_latest_run_holding_it() {
        let digest = |pairs: &[(&str, &str)]| -> PlanDigest {
            pairs
                .iter()
                .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                .collect()
        };
        let current = digest(&[("a", "1"), ("b", "2"), ("c", "3")]);
        assert!(review_delta(&current, &[]).is_empty(), "no digested run");

        let full = digest(&[("a", "1"), ("b", "0")]);
        assert_eq!(review_delta(&current, &[&full]), ["b", "c"]);

        let recheck = digest(&[("b", "2")]);
        assert_eq!(review_delta(&current, &[&full, &recheck]), ["c"]);

        let stale = digest(&[("a", "0")]);
        assert_eq!(review_delta(&current, &[&full, &stale]), ["a", "b", "c"]);
    }
}
