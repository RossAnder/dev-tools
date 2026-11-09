//! The evidence tier of a plan finding on a v2 flow, which decides what
//! `review apply` may write without asking. Tiers are tried in order:
//!
//! 1. `question` or `user`: the target is a decision, a spec `outcome`, or one
//!    of the four brief sections that state intent, whatever the evidence. An
//!    advisory row or one with no `new` is a `question`, which only the user
//!    answers and `review apply` never writes; one with an edit is `user`.
//! 2. `advisory`: an advisory row, a `handover:*` target, or no `new`.
//! 3. `mechanical`: a `proof` this module verifies from the stores and the
//!    tree, without running a command, over a `new` no wider than the proof.
//! 4. `cited`: the settled verifying call stands, and its citation names a
//!    line range inside the target's own file or a path the target names.
//!
//! A row matching none of them is `advisory`.

use std::collections::BTreeSet;

use anyhow::Result;
use serde_json::Value as JsonValue;
use toml::Value as TomlValue;

use super::fields::{Family, Kind, Route, str_enum};
use super::query::parse_citation;
use super::store::{Finding, Row, Verdict};
use super::target::{self, RecordTarget};
use super::verdict::settle;
use crate::cli::ReadIntegrityArgs;
use crate::convert::{json_from_text, toml_to_json};
use crate::io::repo_or_cwd_root;
use crate::plan::check::REQUIRED_SECTIONS;
use crate::tasks::finding::ERROR;
use crate::tasks::schema::{FileKind, VERDICT_HEALTHY, normal_path};
use crate::tasks::{Slot, Store as TaskStore, load_store, resolve_store_path};

str_enum!(Tier {
    Mechanical => "mechanical",
    Cited => "cited",
    Advisory => "advisory",
    User => "user",
    Question => "question",
});

impl Tier {
    /// Tiers whose open rows are the user's to settle before a plan approves.
    pub(crate) fn is_users(self) -> bool {
        matches!(self, Self::User | Self::Question)
    }
}

const TIER_KEY: &str = "tier";
/// Verdict authors whose settled call a citation can stand on. The checker is
/// not one: its projection hides the target and the edit it would vouch for.
const CITING: &[&str] = &["verifier", "panel"];
const STANDING: &[&str] = &["confirmed", "plausible"];
/// Serialised characters each of `expect` and `new` may hold on a mechanical row.
const MAX_VALUE_CHARS: usize = 800;
/// The accept-entry keys a `probe` fix may change.
const PROBE_FIX_KEYS: &[&str] = &["check", "positive"];
/// The target fields a `check-class` proof may change, per `plan check` class.
/// A class absent here proves nothing, since its fix is a judgement; an
/// unhealthy probe is proved by the narrower `probe` kind instead.
const CLASS_FIELDS: &[(&str, &[&str])] = &[
    ("dag/dangling-ref", &["needs", "coupling"]),
    ("plan/accept-no-check", &["spec.accept"]),
    ("plan/design-anchor-dead", &["spec.design"]),
    ("plan/means-required-no-reason", &["spec.means"]),
    ("plan/premise-task-unknown", &["spec.premises"]),
    ("plan/spec-no-accept", &["spec.accept"]),
    ("plan/spec-no-outcome", &["spec.outcome"]),
];

/// The tier of `row`, a finding in its JSON row form with `expect` and `new`
/// as stored, or `None` for a row outside flow mode: not plan-family, or on a
/// flow with no `plan.toml`. Errs on a `target` that does not parse and on a
/// task or plan store that cannot be read.
pub(super) fn compute(row: &Row, slug: &str, verdicts: &[&Verdict]) -> Result<Option<Tier>> {
    let family = text(row, "family").and_then(Family::parse);
    if family != Some(Family::Plan) || super::add::v2_plan(slug)?.is_none() {
        return Ok(None);
    }
    let target = match text(row, "target").filter(|raw| !raw.is_empty()) {
        Some(raw) => Some(target::parse(raw)?),
        None => None,
    };
    let advisory = text(row, "kind").and_then(Kind::parse) == Some(Kind::Advisory);
    if target.as_ref().is_some_and(is_user) {
        return Ok(Some(if advisory || stored(row, "new").is_none() {
            Tier::Question
        } else {
            Tier::User
        }));
    }
    let (Some(target), Some(new)) = (target, stored(row, "new")) else {
        return Ok(Some(Tier::Advisory));
    };
    if advisory || matches!(target, RecordTarget::Handover { .. }) {
        return Ok(Some(Tier::Advisory));
    }
    let stores = Stores::load(slug)?;
    let expect = stored(row, "expect");
    let proven = row
        .get("proof")
        .and_then(JsonValue::as_object)
        .is_some_and(|proof| {
            stores
                .verify(proof, &target, expect.as_ref(), &new, slug)
                .is_ok()
        });
    if proven {
        return Ok(Some(Tier::Mechanical));
    }
    if cited(row, &target, slug, verdicts) {
        return Ok(Some(Tier::Cited));
    }
    Ok(Some(Tier::Advisory))
}

/// Sets `tier` on `finding` when it carries none, so a tier once stamped is
/// frozen. A `target` that does not parse is an error; a store that cannot be
/// read leaves `tier` absent rather than stamping a default.
pub(super) fn stamp(finding: &mut Finding, slug: &str, verdicts: &[&Verdict]) -> Result<()> {
    if finding.rest.contains_key(TIER_KEY) {
        return Ok(());
    }
    let mut row = Row::new();
    row.insert("family".into(), finding.family.as_str().into());
    row.insert("kind".into(), finding.kind.as_str().into());
    if let Some(route) = finding.route {
        row.insert("route".into(), route.as_str().into());
    }
    for (key, value) in &finding.rest {
        row.insert(key.clone(), toml_to_json(value));
    }
    if let Some(raw) = text(&row, "target").filter(|raw| !raw.is_empty()) {
        target::parse(raw)?;
    }
    if let Ok(Some(tier)) = compute(&row, slug, verdicts) {
        finding.rest.insert(
            TIER_KEY.to_string(),
            TomlValue::String(tier.as_str().to_string()),
        );
    }
    Ok(())
}

/// Whether `proof` holds for `target` in flow `slug` and `new` changes no
/// more than it covers. `expect` and `new` are decoded values. The `Err` is
/// the reason the row cannot be mechanical.
pub(super) fn verify_proof(
    proof: &serde_json::Map<String, JsonValue>,
    target: &RecordTarget,
    expect: Option<&JsonValue>,
    new: &JsonValue,
    slug: &str,
) -> Result<(), String> {
    let stores = Stores::load(slug).map_err(|err| format!("{err:#}"))?;
    stores.verify(proof, target, expect, new, slug)
}

fn is_user(target: &RecordTarget) -> bool {
    match target {
        RecordTarget::Decision { .. } => true,
        RecordTarget::SpecField { field, .. } => *field == "outcome",
        // The sections `plan check` requires state intent, which only the user may change.
        RecordTarget::Brief { section } => REQUIRED_SECTIONS.contains(section),
        _ => false,
    }
}

fn text<'r>(row: &'r Row, key: &str) -> Option<&'r str> {
    row.get(key).and_then(JsonValue::as_str)
}

/// The stored `expect` or `new` decoded, `None` when the row carries none. The
/// row is already JSON, so a non-string value is taken as converted.
fn stored(row: &Row, key: &str) -> Option<JsonValue> {
    match row.get(key)? {
        JsonValue::String(raw) => Some(json_from_text(raw)),
        other => Some(other.clone()),
    }
}

fn integrity() -> ReadIntegrityArgs {
    ReadIntegrityArgs {
        verify_integrity: false,
        strict_read: false,
    }
}

/// The flow's task store, read once per tier decision. The plan store is read
/// too, only so that an unreadable one stops the decision.
struct Stores {
    tasks: TaskStore,
}

impl Stores {
    fn load(slug: &str) -> Result<Self> {
        let tasks = load_store(&resolve_store_path(Some(slug), None)?, &integrity())?;
        crate::plan::store::load(&crate::plan::store::resolve(slug)?, &integrity())?;
        Ok(Self { tasks })
    }

    fn verify(
        &self,
        proof: &serde_json::Map<String, JsonValue>,
        target: &RecordTarget,
        expect: Option<&JsonValue>,
        new: &JsonValue,
        slug: &str,
    ) -> Result<(), String> {
        for (name, value) in [("expect", expect), ("new", Some(new))] {
            let size = value.map_or(0, |v| v.to_string().chars().count());
            if size > MAX_VALUE_CHARS {
                return Err(format!(
                    "`{name}` is {size} characters, over the {MAX_VALUE_CHARS} a mechanical fix may hold"
                ));
            }
        }
        let kind = proof
            .get("kind")
            .and_then(JsonValue::as_str)
            .ok_or("a proof names its `kind`")?;
        match kind {
            "probe" => self.probe(target, expect, new, slug),
            "check-class" => self.check_class(proof, target, expect, new, slug),
            "path-missing" => self.path_missing(proof, target, expect, new),
            "ref-unknown" => self.ref_unknown(proof, target, expect, new, slug),
            other => Err(format!(
                "proof kind `{other}` is not one of probe, check-class, path-missing, ref-unknown"
            )),
        }
    }

    /// The entry `expect` names carries a probe verdict other than healthy; a
    /// missing verdict proves nothing.
    fn probe(
        &self,
        target: &RecordTarget,
        expect: Option<&JsonValue>,
        new: &JsonValue,
        slug: &str,
    ) -> Result<(), String> {
        let RecordTarget::SpecEntry {
            r#ref,
            list: "accept",
            slot: Slot::At(_),
            ..
        } = target
        else {
            return Err("a `probe` proof needs a `task:<ref>.spec.accept[<n>]` target".into());
        };
        let expect = expect.ok_or("a `probe` proof needs `expect`, the entry it fixes")?;
        let expect = &as_stored(target, true, expect);
        let new = &as_stored(target, true, new);
        let current = target::resolve(slug, target).map_err(|err| format!("{err:#}"))?;
        let list = current.as_array().map(Vec::as_slice).unwrap_or_default();
        let at = target::match_entry(list, expect).map_err(|err| format!("{err:#}"))?;
        let probe = self
            .tasks
            .spec_for(r#ref)
            .and_then(|spec| spec.accept.get(at))
            .and_then(|entry| entry.probe.as_ref());
        match probe {
            None => return Err("the entry has no probe verdict, which proves nothing".into()),
            Some(probe) if probe.verdict == VERDICT_HEALTHY => {
                return Err("the entry's probe verdict is healthy".into());
            }
            Some(_) => {}
        }
        let (Some(old), Some(fixed)) = (expect.as_object(), new.as_object()) else {
            return Err("a `probe` fix replaces the entry with another entry".into());
        };
        if old.contains_key("probe") || fixed.contains_key("probe") {
            return Err(
                "`probe` is tool-written, so neither `expect` nor `new` may carry it".into(),
            );
        }
        let keys: BTreeSet<&String> = old.keys().chain(fixed.keys()).collect();
        match keys
            .into_iter()
            .find(|key| !PROBE_FIX_KEYS.contains(&key.as_str()) && old.get(*key) != fixed.get(*key))
        {
            Some(key) => Err(format!(
                "a `probe` fix may change only `check` and `positive`, not `{key}`"
            )),
            None => Ok(()),
        }
    }

    /// `plan check` raises the error class on the target's task, and the class
    /// binds the target's field. Its findings name tasks only, so the target is
    /// a task's. The class names the field but not the fix, so on a list `new`
    /// may only append entries or fill an entry's empty `check`.
    fn check_class(
        &self,
        proof: &serde_json::Map<String, JsonValue>,
        target: &RecordTarget,
        expect: Option<&JsonValue>,
        new: &JsonValue,
        slug: &str,
    ) -> Result<(), String> {
        let class = proof
            .get("class")
            .and_then(JsonValue::as_str)
            .ok_or("a `check-class` proof names its `class`")?;
        let r#ref = match target {
            RecordTarget::TaskField { r#ref, .. }
            | RecordTarget::SpecField { r#ref, .. }
            | RecordTarget::SpecEntry { r#ref, .. } => r#ref,
            _ => {
                return Err(
                    "`plan check` findings name only tasks, so a `check-class` proof needs a \
                     `task:` target"
                        .into(),
                );
            }
        };
        let bound = CLASS_FIELDS
            .iter()
            .find(|(known, _)| *known == class)
            .map(|(_, fields)| *fields)
            .ok_or_else(|| format!("`{class}` binds no field a mechanical fix may change"))?;
        let field = target.field_name();
        if !bound.contains(&field.as_str()) {
            return Err(format!("`{class}` does not bind `{field}`"));
        }
        let current = match target::resolve(slug, target).map_err(|err| format!("{err:#}"))? {
            JsonValue::Null => Some(Vec::new()),
            JsonValue::Array(list) => Some(list),
            _ => None,
        };
        if let Some(list) = current {
            let keeps = match target {
                RecordTarget::SpecEntry {
                    slot: Slot::Append, ..
                } => true,
                RecordTarget::SpecEntry { .. } => {
                    let expect = expect.ok_or("a `check-class` fix to one entry needs `expect`")?;
                    let expect = as_stored(target, true, expect);
                    let at =
                        target::match_entry(&list, &expect).map_err(|err| format!("{err:#}"))?;
                    entry_kept(&list[at], &as_stored(target, true, new))
                }
                _ => as_stored(target, false, new)
                    .as_array()
                    .is_some_and(|fixed| list_kept(&list, fixed)),
            };
            if !keeps {
                return Err(
                    "a `check-class` fix to a list may only append entries or fill an empty \
                     `check`"
                        .into(),
                );
            }
        }
        let id = self
            .tasks
            .find_ref(r#ref)
            .ok_or_else(|| format!("no task with ref `{}`", r#ref))?
            .id;
        let found =
            crate::plan::check::findings(slug, &integrity()).map_err(|err| format!("{err:#}"))?;
        let raised = found
            .iter()
            .any(|f| f.class == class && f.severity == ERROR && f.ids.contains(&id));
        if raised {
            Ok(())
        } else {
            Err(format!(
                "`plan check` raises no `{class}` error on task {id}"
            ))
        }
    }

    /// The target task's `files` claims the path, the tree lacks it, and no
    /// `(new)` file note declares it.
    fn path_missing(
        &self,
        proof: &serde_json::Map<String, JsonValue>,
        target: &RecordTarget,
        expect: Option<&JsonValue>,
        new: &JsonValue,
    ) -> Result<(), String> {
        let RecordTarget::TaskField {
            r#ref,
            field: "files",
        } = target
        else {
            return Err("a `path-missing` proof needs a `task:<ref>.files` target".into());
        };
        let path = proof
            .get("path")
            .and_then(JsonValue::as_str)
            .ok_or("a `path-missing` proof names its `path`")?;
        if let Some(problem) = super::fields::path_problem(path, "paths") {
            return Err(format!("proof path `{path}` {problem}"));
        }
        let wanted = normal_path(path);
        let row = self
            .tasks
            .find_ref(r#ref)
            .ok_or_else(|| format!("no task with ref `{}`", r#ref))?;
        if !row.files.iter().any(|file| normal_path(file) == wanted) {
            return Err(format!("task `{}` does not claim `{path}`", r#ref));
        }
        let root = repo_or_cwd_root().map_err(|err| format!("{err:#}"))?;
        if root.join(&wanted).exists() {
            return Err(format!("`{path}` exists"));
        }
        let declared_new = self.tasks.file_notes.iter().any(|note| {
            note.r#ref == *r#ref
                && normal_path(&note.file) == wanted
                && FileKind::of_note(&note.note) == Some(FileKind::New)
        });
        if declared_new {
            return Err(format!("`{path}` is a `(new)` file"));
        }
        one_entry_fix(expect, new, |entry| {
            entry.as_str().map(normal_path).as_deref() == Some(wanted.as_str())
        })
    }

    /// The target is a `needs` or a contract's `tasks`, its value names the
    /// task id, and no row carries that id.
    fn ref_unknown(
        &self,
        proof: &serde_json::Map<String, JsonValue>,
        target: &RecordTarget,
        expect: Option<&JsonValue>,
        new: &JsonValue,
        slug: &str,
    ) -> Result<(), String> {
        let names_ids = matches!(
            target,
            RecordTarget::TaskField { field: "needs", .. }
                | RecordTarget::Contract { key: "tasks", .. }
        );
        if !names_ids {
            return Err(
                "a `ref-unknown` proof needs a `task:<ref>.needs` or `contract:C<n>.tasks` target"
                    .into(),
            );
        }
        let id = match proof.get("ref") {
            Some(JsonValue::Number(n)) => n.as_u64(),
            Some(JsonValue::String(raw)) => raw.parse().ok(),
            _ => None,
        }
        .and_then(|n| u32::try_from(n).ok())
        .ok_or("a `ref-unknown` proof names its `ref`, a task id")?;
        let current = target::resolve(slug, target).map_err(|err| format!("{err:#}"))?;
        let is_id = |entry: &JsonValue| entry.as_u64() == Some(u64::from(id));
        if !current
            .as_array()
            .is_some_and(|list| list.iter().any(is_id))
        {
            return Err(format!("`{target}` does not name task {id}"));
        }
        if self.tasks.find(id).is_some() {
            return Err(format!("task {id} exists"));
        }
        one_entry_fix(expect, new, is_id)
    }
}

/// `new` is `expect` with the one entry `proved` matches dropped, or replaced
/// by one value of the same JSON type that `proved` does not match.
fn one_entry_fix(
    expect: Option<&JsonValue>,
    new: &JsonValue,
    proved: impl Fn(&JsonValue) -> bool,
) -> Result<(), String> {
    let old = expect
        .and_then(JsonValue::as_array)
        .ok_or("the proof needs `expect`, the whole list")?;
    let fixed = new.as_array().ok_or("`new` must be the whole list")?;
    let at = old
        .iter()
        .position(&proved)
        .ok_or("`expect` does not hold the entry the proof names")?;
    let others = |list: &[JsonValue]| -> Vec<JsonValue> {
        list.iter()
            .enumerate()
            .filter(|(i, _)| *i != at)
            .map(|(_, v)| v.clone())
            .collect()
    };
    let dropped = fixed.len() + 1 == old.len() && others(old) == *fixed;
    let replaced = fixed.len() == old.len()
        && others(old) == others(fixed)
        && !proved(&fixed[at])
        && same_type(&old[at], &fixed[at]);
    if dropped || replaced {
        Ok(())
    } else {
        Err("`new` may only drop or replace the entry the proof names".into())
    }
}

/// `fixed` holds every entry of `stored` in place, each kept by [`entry_kept`],
/// and may append more after them.
fn list_kept(stored: &[JsonValue], fixed: &[JsonValue]) -> bool {
    fixed.len() >= stored.len()
        && stored
            .iter()
            .zip(fixed)
            .all(|(old, new)| entry_kept(old, new))
}

/// `new` equals `old`, or `old` is an object with an empty or absent `check`
/// and `new` is the same object with a non-empty `check`.
fn entry_kept(old: &JsonValue, new: &JsonValue) -> bool {
    fn check(entry: &serde_json::Map<String, JsonValue>) -> Option<&str> {
        entry
            .get("check")
            .map_or(Some(""), JsonValue::as_str)
            .map(str::trim)
    }
    if old == new {
        return true;
    }
    let (Some(old), Some(new)) = (old.as_object(), new.as_object()) else {
        return false;
    };
    check(old) == Some("")
        && check(new).is_some_and(|filled| !filled.is_empty())
        && old
            .keys()
            .chain(new.keys())
            .all(|key| key == "check" || old.get(key) == new.get(key))
}

/// `value` as the target's store would hold it, so an entry leaving defaults
/// unstated matches as `review add` and `review apply` judge it. A value the
/// spec parser refuses, a supplied `probe` among them, is kept as written.
fn as_stored(target: &RecordTarget, entry: bool, value: &JsonValue) -> JsonValue {
    crate::tasks::review_stored_form(&target.field_name(), entry, value)
        .unwrap_or_else(|_| value.clone())
}

fn same_type(a: &JsonValue, b: &JsonValue) -> bool {
    std::mem::discriminant(a) == std::mem::discriminant(b)
}

/// The settled verifying call stands, and the citation of its latest deciding
/// verdict holds for the target.
fn cited(row: &Row, target: &RecordTarget, slug: &str, verdicts: &[&Verdict]) -> bool {
    let route = text(row, "route").and_then(Route::parse);
    let Some(settled) = settle(route, verdicts, |v| CITING.contains(&v.by.as_str())) else {
        return false;
    };
    if !STANDING.contains(&settled.verdict) {
        return false;
    }
    settled
        .deciding
        .iter()
        .max()
        .is_some_and(|&at| citation_holds(&verdicts[at].citation, target, slug))
}

/// The citation is a `path:line` range inside the file it names, and that
/// file is the target's own or one the target's current value names.
fn citation_holds(citation: &str, target: &RecordTarget, slug: &str) -> bool {
    let Some((path, _, to)) = parse_citation(citation) else {
        return false;
    };
    let path = normal_path(path);
    if !named_paths(target, slug).contains(&path) {
        return false;
    }
    let Ok(root) = repo_or_cwd_root() else {
        return false;
    };
    let Ok(bytes) = std::fs::read(root.join(&path)) else {
        return false;
    };
    String::from_utf8_lossy(&bytes).lines().count() >= to
}

/// The target's file, plus every string in its current value that is a path or
/// a `design/<file>.md#<anchor>` citation, in repo-relative form.
fn named_paths(target: &RecordTarget, slug: &str) -> BTreeSet<String> {
    let mut paths = BTreeSet::from([normal_path(&target.file(slug))]);
    let mut pending = target::resolve(slug, target)
        .ok()
        .into_iter()
        .collect::<Vec<_>>();
    while let Some(value) = pending.pop() {
        match value {
            JsonValue::String(raw) => {
                let path = match raw.split_once('#') {
                    Some((file, _)) if file.starts_with("design/") => {
                        format!("docs/flows/{slug}/{file}")
                    }
                    _ => raw,
                };
                paths.insert(normal_path(&path));
            }
            JsonValue::Array(entries) => pending.extend(entries),
            JsonValue::Object(map) => pending.extend(map.into_iter().map(|(_, v)| v)),
            _ => {}
        }
    }
    paths
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::with_root;
    use serde_json::json;
    use std::path::Path;

    const SLUG: &str = "tiers";

    const TASKS: &str = r#"
schema_version = 1

[[specs]]
ref = "parse-it"
outcome = "targets parse"
executor = "deep"

[[specs.accept]]
behaviour = "parses"
check = "cargo test parse"
kind = "forward"
positive = ""
writes_build = false

[specs.accept.probe]
at = "2026-11-09"
neg_rc = 0
pos_rc = 0
verdict = "inverted"

[[specs.accept]]
behaviour = "renders"
check = "cargo test render"
kind = "forward"
positive = ""
writes_build = false

[[specs]]
ref = "bare"
outcome = ""
executor = "lite"

[[items]]
id = 1
ref = "parse-it"
title = "Parse it"
effort = "S"
status = "pending"
files = ["src/a.rs", "src/gone.rs"]
needs = [7]
action = "parse"

[[items]]
id = 2
ref = "bare"
title = "Bare"
effort = "S"
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

[[handover]]
id = "H1"
command = "cargo install --path tomlctl"
source_task = 1
done = false
"#;

    fn seed(root: &Path) {
        let flow = root.join(".claude").join("flows").join(SLUG);
        std::fs::create_dir_all(&flow).unwrap();
        std::fs::write(flow.join("tasks.toml"), TASKS).unwrap();
        std::fs::write(flow.join("plan.toml"), PLAN).unwrap();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src").join("a.rs"), "fn a() {}\n").unwrap();
    }

    fn entry(check: &str, writes_build: bool) -> JsonValue {
        json!({
            "behaviour": "parses",
            "check": check,
            "kind": "forward",
            "positive": "",
            "writes_build": writes_build,
        })
    }

    /// A flow-mode defect row, `expect` and `new` stored as JSON text.
    fn row(target: &str, expect: Option<JsonValue>, new: JsonValue, proof: JsonValue) -> Row {
        let mut row = json!({
            "id": "F1",
            "family": "plan",
            "kind": "defect",
            "route": "verifier",
            "target": target,
            "new": new.to_string(),
        });
        if let Some(expect) = expect {
            row["expect"] = expect.to_string().into();
        }
        if !proof.is_null() {
            row["proof"] = proof;
        }
        row.as_object().unwrap().clone()
    }

    fn verdict(verdict: &str, citation: &str) -> Verdict {
        Verdict {
            finding: "F1".into(),
            run: "U1".into(),
            by: "verifier".into(),
            seat: None,
            verdict: verdict.into(),
            severity: String::new(),
            citation: citation.into(),
            rationale: String::new(),
            rest: toml::map::Map::new(),
        }
    }

    fn tier(row: &Row, verdicts: &[&Verdict]) -> Option<Tier> {
        compute(row, SLUG, verdicts).expect("the tier computes")
    }

    fn proof_err(row: &Row) -> String {
        let target = target::parse(text(row, "target").unwrap()).unwrap();
        let proof = row["proof"].as_object().unwrap();
        verify_proof(
            proof,
            &target,
            stored(row, "expect").as_ref(),
            &stored(row, "new").unwrap(),
            SLUG,
        )
        .expect_err("the proof must not hold")
    }

    #[test]
    fn the_user_table_beats_a_valid_proof() {
        with_root(|root| {
            seed(root);
            let row = row(
                "task:bare.spec.outcome",
                Some(json!("")),
                json!("bare things render"),
                json!({"kind": "check-class", "class": "plan/spec-no-outcome"}),
            );
            let target = target::parse("task:bare.spec.outcome").unwrap();
            verify_proof(
                row["proof"].as_object().unwrap(),
                &target,
                Some(&json!("")),
                &json!("bare things render"),
                SLUG,
            )
            .expect("the proof holds");
            assert_eq!(tier(&row, &[]), Some(Tier::User));
            let decision = self::row(
                "decision:D1.why",
                Some(json!("fewer moving parts")),
                json!("x"),
                JsonValue::Null,
            );
            assert_eq!(tier(&decision, &[]), Some(Tier::User));
            for section in ["Intent", "Non-goals", "Scope budget", "Success criteria"] {
                let brief = self::row(
                    &format!("brief#{section}"),
                    Some(json!("old")),
                    json!("new"),
                    JsonValue::Null,
                );
                assert_eq!(tier(&brief, &[]), Some(Tier::User), "brief#{section}");
            }
        });
    }

    #[test]
    fn a_user_owned_row_apply_cannot_write_is_a_question() {
        with_root(|root| {
            seed(root);
            let edit = row(
                "decision:D1.why",
                Some(json!("fewer moving parts")),
                json!("x"),
                JsonValue::Null,
            );
            assert_eq!(tier(&edit, &[]), Some(Tier::User));
            let mut bare = edit.clone();
            bare.remove("new");
            assert_eq!(tier(&bare, &[]), Some(Tier::Question));
            let mut advisory = edit;
            advisory["kind"] = "advisory".into();
            assert_eq!(tier(&advisory, &[]), Some(Tier::Question));
            let mut outcome = row("task:bare.spec.outcome", None, json!("x"), JsonValue::Null);
            outcome.remove("new");
            assert_eq!(tier(&outcome, &[]), Some(Tier::Question));
        });
    }

    #[test]
    fn a_checker_call_cannot_cite_a_row_into_cited() {
        with_root(|root| {
            seed(root);
            let row = row(
                "task:parse-it.files",
                Some(json!(["src/a.rs", "src/gone.rs"])),
                json!(["src/a.rs"]),
                JsonValue::Null,
            );
            let mut checked = verdict("confirmed", ".claude/flows/tiers/tasks.toml:2-4");
            checked.by = "checker".into();
            assert_eq!(tier(&row, &[&checked]), Some(Tier::Advisory));
        });
    }

    #[test]
    fn a_handover_target_caps_at_advisory() {
        with_root(|root| {
            seed(root);
            let row = row(
                "handover:H1.done",
                Some(json!(false)),
                json!(true),
                JsonValue::Null,
            );
            let standing = verdict("confirmed", ".claude/flows/tiers/plan.toml:1");
            assert_eq!(tier(&row, &[&standing]), Some(Tier::Advisory));
        });
    }

    #[test]
    fn a_proof_that_no_longer_holds_is_not_mechanical() {
        with_root(|root| {
            seed(root);
            let row = row(
                "task:parse-it.files",
                Some(json!(["src/a.rs", "src/gone.rs"])),
                json!(["src/gone.rs"]),
                json!({"kind": "path-missing", "path": "src/a.rs"}),
            );
            assert!(proof_err(&row).contains("exists"));
            assert_eq!(tier(&row, &[]), Some(Tier::Advisory));

            let held = self::row(
                "task:parse-it.files",
                Some(json!(["src/a.rs", "src/gone.rs"])),
                json!(["src/a.rs"]),
                json!({"kind": "path-missing", "path": "src/gone.rs"}),
            );
            assert_eq!(tier(&held, &[]), Some(Tier::Mechanical));
        });
    }

    #[test]
    fn a_probe_fix_that_flips_writes_build_caps_at_cited() {
        with_root(|root| {
            seed(root);
            let proof = json!({"kind": "probe"});
            let target = "task:parse-it.spec.accept[0]";
            let fixed = row(
                target,
                Some(entry("cargo test parse", false)),
                entry("cargo test parse --lib", false),
                proof.clone(),
            );
            assert_eq!(tier(&fixed, &[]), Some(Tier::Mechanical));

            let flipped = row(
                target,
                Some(entry("cargo test parse", false)),
                entry("cargo test parse --lib", true),
                proof,
            );
            assert!(proof_err(&flipped).contains("`writes_build`"));
            let standing = verdict("confirmed", ".claude/flows/tiers/tasks.toml:2-4");
            assert_eq!(tier(&flipped, &[&standing]), Some(Tier::Cited));
            let refuted = verdict("refuted", ".claude/flows/tiers/tasks.toml:2-4");
            assert_eq!(tier(&flipped, &[&refuted]), Some(Tier::Advisory));
            let elsewhere = verdict("confirmed", "src/a.rs:1");
            assert_eq!(tier(&flipped, &[&elsewhere]), Some(Tier::Advisory));
            let past_end = verdict("confirmed", ".claude/flows/tiers/tasks.toml:9999");
            assert_eq!(tier(&flipped, &[&past_end]), Some(Tier::Advisory));
        });
    }

    #[test]
    fn a_short_form_accept_expect_add_accepts_is_proved_mechanical() {
        with_root(|root| {
            seed(root);
            let target = "task:parse-it.spec.accept[0]";
            let short = |check: &str| {
                json!({
                    "behaviour": "parses",
                    "check": check,
                    "kind": "forward",
                    "writes_build": false,
                })
            };
            let (expect, new) = (short("cargo test parse"), short("cargo test parse --lib"));
            assert_eq!(
                super::super::apply::validate_target_edit(
                    SLUG,
                    &target::parse(target).unwrap(),
                    Some(&expect),
                    &new
                )
                .unwrap(),
                crate::tasks::ReviewEditOutcome::Applied
            );
            let proof = json!({"kind": "probe"});
            let fixed = row(target, Some(expect.clone()), new, proof.clone());
            assert_eq!(tier(&fixed, &[]), Some(Tier::Mechanical));

            let defaulted = json!({"behaviour": "parses", "check": "x", "kind": "forward"});
            let flipped = row(target, Some(expect), defaulted, proof);
            assert!(proof_err(&flipped).contains("`writes_build`"));
        });
    }

    #[test]
    fn a_citation_of_a_path_the_value_names_holds() {
        with_root(|root| {
            seed(root);
            let row = row(
                "task:parse-it.files",
                Some(json!(["src/a.rs", "src/gone.rs"])),
                json!(["src/a.rs"]),
                JsonValue::Null,
            );
            let named = verdict("confirmed", "src/a.rs:1");
            assert_eq!(tier(&row, &[&named]), Some(Tier::Cited));
        });
    }

    #[test]
    fn a_check_class_fix_may_only_append_or_fill_a_check() {
        with_root(|root| {
            seed(root);
            std::fs::write(
                root.join(".claude/flows/tiers/tasks.toml"),
                TASKS.replace("check = \"cargo test render\"", "check = \"\""),
            )
            .unwrap();
            let proof = json!({"kind": "check-class", "class": "plan/accept-no-check"});
            let target = "task:parse-it.spec.accept[1]";
            let mut expect = entry("", false);
            expect["behaviour"] = "renders".into();
            let mut filled = expect.clone();
            filled["check"] = "cargo test render".into();
            let fill = row(target, Some(expect.clone()), filled.clone(), proof.clone());
            assert_eq!(tier(&fill, &[]), Some(Tier::Mechanical));

            let mut flipped = filled.clone();
            flipped["writes_build"] = true.into();
            let flip = row(target, Some(expect.clone()), flipped, proof.clone());
            assert!(proof_err(&flip).contains("only append entries or fill an empty `check`"));
            assert_eq!(tier(&flip, &[]), Some(Tier::Advisory));
            let standing = verdict("confirmed", ".claude/flows/tiers/tasks.toml:2-4");
            assert_eq!(tier(&flip, &[&standing]), Some(Tier::Cited));

            let dropped = row(
                "task:parse-it.spec.accept",
                Some(json!([entry("cargo test parse", false), expect])),
                json!([filled]),
                proof,
            );
            assert!(proof_err(&dropped).contains("only append entries"));

            let unhealthy = row(
                "task:parse-it.spec.accept[0]",
                Some(entry("cargo test parse", false)),
                entry("cargo test parse --lib", false),
                json!({"kind": "check-class", "class": "plan/probe-unhealthy"}),
            );
            assert!(proof_err(&unhealthy).contains("binds no field"));
        });
    }

    #[test]
    fn a_probe_absent_entry_is_no_proof() {
        with_root(|root| {
            seed(root);
            let mut expect = entry("cargo test render", false);
            expect["behaviour"] = "renders".into();
            let mut new = expect.clone();
            new["check"] = "cargo test render --lib".into();
            let row = row(
                "task:parse-it.spec.accept[1]",
                Some(expect),
                new,
                json!({"kind": "probe"}),
            );
            assert!(proof_err(&row).contains("no probe verdict"));
            assert_eq!(tier(&row, &[]), Some(Tier::Advisory));
        });
    }

    #[test]
    fn ref_unknown_proves_a_dangling_need_and_check_class_does_not() {
        with_root(|root| {
            seed(root);
            let unknown = row(
                "task:parse-it.needs",
                Some(json!([7])),
                json!([]),
                json!({"kind": "ref-unknown", "ref": 7}),
            );
            assert_eq!(tier(&unknown, &[]), Some(Tier::Mechanical));
            let class = row(
                "task:parse-it.needs",
                Some(json!([7])),
                json!([2]),
                json!({"kind": "check-class", "class": "dag/dangling-ref"}),
            );
            assert!(proof_err(&class).contains("only append entries"));
            assert_eq!(tier(&class, &[]), Some(Tier::Advisory));
            let unbound = row(
                "task:parse-it.files",
                Some(json!(["src/a.rs", "src/gone.rs"])),
                json!([]),
                json!({"kind": "check-class", "class": "dag/dangling-ref"}),
            );
            assert!(proof_err(&unbound).contains("does not bind `files`"));
        });
    }

    #[test]
    fn rows_outside_flow_mode_have_no_tier() {
        with_root(|root| {
            seed(root);
            let mut code = row("task:parse-it.action", None, json!("x"), JsonValue::Null);
            code["family"] = "code".into();
            assert_eq!(tier(&code, &[]), None);
            std::fs::remove_file(root.join(".claude/flows/tiers/plan.toml")).unwrap();
            let plan = row("task:parse-it.action", None, json!("x"), JsonValue::Null);
            assert_eq!(tier(&plan, &[]), None);
        });
    }

    #[test]
    fn stamp_freezes_a_tier_and_skips_an_unreadable_store() {
        with_root(|root| {
            seed(root);
            let doc: TomlValue = toml::from_str(
                "[[findings]]\nid = \"F1\"\nfamily = \"plan\"\nkind = \"defect\"\nstatus = \"open\"\ntarget = \"decision:D1.why\"\nnew = '\"x\"'\n",
            )
            .unwrap();
            let fresh = super::super::store::from_toml(&doc)
                .unwrap()
                .findings
                .remove(0);

            let mut finding = fresh.clone();
            stamp(&mut finding, SLUG, &[]).unwrap();
            assert_eq!(
                finding.rest.get(TIER_KEY).and_then(TomlValue::as_str),
                Some("user")
            );

            let mut frozen = fresh.clone();
            frozen
                .rest
                .insert(TIER_KEY.into(), TomlValue::String("cited".into()));
            stamp(&mut frozen, SLUG, &[]).unwrap();
            assert_eq!(
                frozen.rest.get(TIER_KEY).and_then(TomlValue::as_str),
                Some("cited")
            );

            std::fs::write(root.join(".claude/flows/tiers/tasks.toml"), "items = 3").unwrap();
            let mut unread = fresh;
            unread.rest.insert(
                "target".into(),
                TomlValue::String("task:parse-it.action".into()),
            );
            stamp(&mut unread, SLUG, &[]).unwrap();
            assert!(!unread.rest.contains_key(TIER_KEY), "{:?}", unread.rest);
        });
    }
}
