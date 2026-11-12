//! The `snapshot` verb — one consistent read of everything a flow viewer
//! renders: the task rows, the graph products, the execution record joined to
//! the rows, and the hook-written agent records.
//!
//! Read-only: every file besides `tasks.toml` is optional, and an absent one
//! reads as empty rather than being created. `revision` fingerprints the raw
//! bytes, so a poller can skip a snapshot whose inputs did not change.

use std::io::ErrorKind as IoErrorKind;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::{Map as JsonMap, Value as JsonValue, json};
use sha2::{Digest, Sha256};
use toml::Value as TomlValue;

use super::graph::{Tense, build_or_refuse, nodes_of};
use super::schema::{Change, MilestoneSpec, Status, Store, TaskRow, TaskSpec};
use super::snapshot_record::{checkpoint_facts, record_view};
use super::{edges, ready, schema, show, store};
use crate::cli::{ReadIntegrityArgs, read_integrity_opts};
use crate::convert::toml_to_json;
use crate::integrity::hex_lower;
use crate::io::{items_array, parse_toml_bytes, read_doc_owned};
use crate::plan::probes;

const SCHEMA: u32 = 1;
// The store's siblings, named once in `SNAPSHOT_INPUTS` so a viewer's
// fingerprint covers the same files this reads.
const RECORD_FILE: &str = crate::SNAPSHOT_INPUTS[1];
const AGENTS_FILE: &str = crate::SNAPSHOT_INPUTS[2];
const CONTEXT_FILE: &str = crate::SNAPSHOT_INPUTS[3];
const PLAN_FILE: &str = crate::SNAPSHOT_INPUTS[4];
const REVISION_HEX_LEN: usize = 16;

/// The snapshot of the flow whose store is `store_path`; its siblings are
/// read from the same directory. `None` when the inputs hash to `known`, so a
/// poller skips the parse and graph build; `known: None` always builds.
/// Errors when the store is missing or its graph has a dangling edge or a cycle.
pub(crate) fn snapshot(
    slug: &str,
    store_path: &Path,
    read_opts: &ReadIntegrityArgs,
    known: Option<&str>,
) -> Result<Option<JsonValue>> {
    let record_path = sibling(store_path, RECORD_FILE);
    let agents_path = sibling(store_path, AGENTS_FILE);
    let context_path = sibling(store_path, CONTEXT_FILE);
    let plan_path = sibling(store_path, PLAN_FILE);

    let paths = [
        store_path,
        &record_path,
        &agents_path,
        &context_path,
        &plan_path,
    ];
    let [store_raw, record_raw, agents_raw, context_raw, plan_raw] = paths.map(read_raw);
    let (store_raw, record_raw, agents_raw, context_raw, plan_raw) = (
        store_raw?,
        record_raw?,
        agents_raw?,
        context_raw?,
        plan_raw?,
    );
    let revision = revision(&[
        &store_raw,
        &record_raw,
        &agents_raw,
        &context_raw,
        &plan_raw,
    ]);
    if known == Some(revision.as_str()) {
        return Ok(None);
    }

    let (store, record, agents, context, plan) =
        if read_opts.verify_integrity || read_opts.strict_read {
            // The sidecar check and the strict-read gate need the file and its
            // lock, so these re-read. Hashed first: a write landing in between
            // leaves a stale revision over fresh content, which the next poll
            // corrects, never a fresh revision over stale content, which it
            // would skip.
            (
                store::load(store_path, read_opts)?,
                read_optional(&record_path, read_opts)?,
                read_optional(&agents_path, read_opts)?,
                read_optional(&context_path, read_opts)?,
                read_optional(&plan_path, read_opts)?,
            )
        } else {
            // Parsed from the hashed bytes, so revision and content agree.
            let store = match store_raw {
                Some(bytes) => schema::from_toml(&parse_toml_bytes(store_path, bytes)?)?,
                // Absent: `load` raises the missing-store error.
                None => store::load(store_path, read_opts)?,
            };
            (
                store,
                parse_optional(&record_path, record_raw)?,
                parse_optional(&agents_path, agents_raw)?,
                parse_optional(&context_path, context_raw)?,
                parse_optional(&plan_path, plan_raw)?,
            )
        };
    let plan_store = match &plan {
        Some(doc) => Some(
            crate::plan::schema::from_toml(doc)
                .with_context(|| format!("parsing {}", plan_path.display()))?,
        ),
        None => None,
    };
    let running_milestone = plan_store
        .as_ref()
        .map_or_else(String::new, crate::plan::running_milestone);
    let plan_json = plan_store.as_ref().map_or(JsonValue::Null, |parsed| {
        toml_to_json(&crate::plan::schema::to_toml(parsed))
    });

    let nodes = nodes_of(&store.items);
    let graph = build_or_refuse(&nodes, "the snapshot", Tense::Stored)?;
    let layers = graph.kahn_rounds();
    let order: Vec<String> = store.checkpoints.iter().map(|c| c.id.clone()).collect();
    let groups = graph.groups(&order).map_err(super::graph::refuse)?;

    let in_progress: Vec<u32> = store
        .items
        .iter()
        .filter(|row| row.status == Status::InProgress)
        .map(|row| row.id)
        .collect();
    let frontier = ready::ready_with(&graph, &in_progress)?;
    let edge_list = edges::edge_list_with(&store, &graph, None)?;

    let record_json = record_view(record.as_ref(), &store);
    let mut facts = checkpoint_facts(&record_json, &store);
    let checkpoints: Vec<JsonValue> = groups
        .into_iter()
        .map(|group| {
            let rationale = store
                .checkpoints
                .iter()
                .find(|c| c.id == group.id)
                .map_or("", |c| c.rationale.as_str());
            let (commits, verification) = facts.remove(&group.id).unwrap_or_default();
            json!({
                "id": group.id,
                "rationale": rationale,
                "members": group.members,
                "maximal": group.maximal,
                "valid_cut": group.valid_cut,
                "commits": commits,
                "verification": verification,
            })
        })
        .collect();

    let flow_status = context
        .as_ref()
        .and_then(|doc| doc.get("status"))
        .and_then(TomlValue::as_str)
        .unwrap_or("");
    let agents_json: Vec<JsonValue> = agents
        .as_ref()
        .map(|doc| {
            items_array(doc, "agents")
                .iter()
                .map(toml_to_json)
                .collect()
        })
        .unwrap_or_default();

    let mut out = JsonMap::new();
    out.insert("schema".into(), json!(SCHEMA));
    out.insert("revision".into(), json!(revision));
    out.insert("slug".into(), json!(slug));
    out.insert("plan_path".into(), json!(store.plan_path));
    out.insert("flow_status".into(), json!(flow_status));
    out.insert(
        "policy".into(),
        json!({
            "checkpoints": store.policy.checkpoints,
            "max_parallel": store.policy.max_parallel,
            "commit_granularity": store.policy.commit_granularity,
        }),
    );
    out.insert("running_milestone".into(), json!(running_milestone));
    out.insert(
        "tasks".into(),
        JsonValue::Array(
            store
                .items
                .iter()
                .map(|row| task_json(&store, row))
                .collect(),
        ),
    );
    out.insert("layers".into(), json!(layers));
    out.insert("frontier".into(), frontier);
    out.insert("edges".into(), edge_list);
    out.insert("checkpoints".into(), JsonValue::Array(checkpoints));
    out.insert("record".into(), record_json);
    out.insert("agents".into(), JsonValue::Array(agents_json));
    out.insert(
        "milestones".into(),
        JsonValue::Array(store.milestones.iter().map(milestone_json).collect()),
    );
    out.insert(
        "changes".into(),
        JsonValue::Array(store.changes.iter().map(change_json).collect()),
    );
    out.insert("plan".into(), plan_json);
    Ok(Some(JsonValue::Object(out)))
}

fn sibling(store_path: &Path, name: &str) -> PathBuf {
    store_path.with_file_name(name)
}

/// `read_doc` errors on a missing file; here absence is a valid state.
fn read_optional(path: &Path, read_opts: &ReadIntegrityArgs) -> Result<Option<TomlValue>> {
    if !path.exists() {
        return Ok(None);
    }
    read_doc_owned(path, read_integrity_opts(read_opts)).map(Some)
}

fn parse_optional(path: &Path, bytes: Option<Vec<u8>>) -> Result<Option<TomlValue>> {
    bytes.map(|bytes| parse_toml_bytes(path, bytes)).transpose()
}

/// A file's bytes, or `None` when it does not exist.
fn read_raw(path: &Path) -> Result<Option<Vec<u8>>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(err) if err.kind() == IoErrorKind::NotFound => Ok(None),
        Err(err) => Err(err).with_context(|| format!("reading {}", path.display())),
    }
}

/// Each file contributes its byte length (u64, little-endian) and then its
/// bytes, so moving bytes from one file to the next changes the digest. An
/// absent file contributes a zero length.
fn revision(files: &[&Option<Vec<u8>>]) -> String {
    let mut hasher = Sha256::new();
    for file in files {
        let bytes = file.as_deref().unwrap_or_default();
        hasher.update((bytes.len() as u64).to_le_bytes());
        hasher.update(bytes);
    }
    let mut hex = hex_lower(&hasher.finalize());
    hex.truncate(REVISION_HEX_LEN);
    hex
}

/// The row's own keys, then the spec and links the store keys on its `ref`.
fn task_json(store: &Store, row: &TaskRow) -> JsonValue {
    let mut out = row_json(row);
    let JsonValue::Object(map) = &mut out else {
        unreachable!("json! of an object literal is an object");
    };
    if let Some(spec) = store.spec_for(&row.r#ref) {
        map.insert("spec".into(), spec_json(store, spec));
    }
    let notes: JsonMap<String, JsonValue> = store
        .file_notes
        .iter()
        .filter(|entry| entry.r#ref == row.r#ref)
        .map(|entry| (entry.file.clone(), json!(entry.note)))
        .collect();
    map.insert("file_notes".into(), JsonValue::Object(notes));
    let link = store.links_for(&row.r#ref);
    map.insert("closes".into(), json!(link.map_or(&[][..], |l| &l.closes)));
    map.insert("refs".into(), json!(link.map_or(&[][..], |l| &l.refs)));
    if let Some(link) = store
        .finding_links
        .iter()
        .find(|link| link.r#ref == row.r#ref)
    {
        map.insert(
            "finding".into(),
            json!({"store": link.store, "fixes": link.fixes, "settles": link.settles}),
        );
    }
    out
}

/// Unlike `tasks show`, every accept entry carries `run_by` and `covered_by`
/// even when empty, plus its derived `owner`; `probeable` is the probe
/// emitter's own skip rule, so a viewer never restates it.
fn spec_json(store: &Store, spec: &TaskSpec) -> JsonValue {
    let accept: Vec<JsonValue> = spec
        .accept
        .iter()
        .map(|entry| {
            let mut map = JsonMap::new();
            map.insert("behaviour".into(), json!(entry.behaviour));
            map.insert("check".into(), json!(entry.check));
            map.insert("kind".into(), json!(entry.kind));
            map.insert("positive".into(), json!(entry.positive));
            map.insert("writes_build".into(), json!(entry.writes_build));
            map.insert("run_by".into(), json!(entry.run_by));
            map.insert("covered_by".into(), json!(entry.covered_by));
            map.insert("owner".into(), json!(entry.owner()));
            map.insert(
                "probeable".into(),
                json!(probes::skip_reason(entry).is_none()),
            );
            if let Some(probe) = &entry.probe {
                map.insert("probe".into(), show::accept_probe_json(probe));
            }
            JsonValue::Object(map)
        })
        .collect();
    let premises: Vec<JsonValue> = spec
        .premises
        .iter()
        .map(|premise| {
            let mut map = JsonMap::new();
            map.insert("claim".into(), json!(premise.claim));
            map.insert("check".into(), json!(premise.check));
            map.insert("at".into(), json!(premise.at));
            map.insert(
                "probeable".into(),
                json!(probes::premise_skip_reason(store, premise).is_none()),
            );
            if let Some(probe) = &premise.probe {
                map.insert("probe".into(), show::premise_probe_json(probe));
            }
            JsonValue::Object(map)
        })
        .collect();
    json!({
        "outcome": spec.outcome,
        "constraints": spec.constraints,
        "means": show::means_json(&spec.means),
        "executor": spec.executor,
        "design": spec.design,
        "precedent": spec.precedent,
        "accept": accept,
        "premises": premises,
    })
}

fn milestone_json(milestone: &MilestoneSpec) -> JsonValue {
    let gate_probes: Vec<JsonValue> = milestone
        .gate_probes
        .iter()
        .map(|entry| {
            json!({
                "gate": entry.gate,
                "probe": entry.probe,
                "verdict": entry.verdict,
                "at": entry.at,
            })
        })
        .collect();
    json!({
        "id": milestone.id,
        "gates": milestone.gates,
        "elaboration": milestone.elaboration,
        "defer_reason": milestone.defer_reason,
        "gate_probes": gate_probes,
    })
}

/// `at` is `""` for a change stored without a timestamp.
fn change_json(change: &Change) -> JsonValue {
    json!({
        "at": change.at.map(|at| at.to_string()).unwrap_or_default(),
        "ref": change.r#ref,
        "field": change.field,
        "old": change.old,
        "new": change.new,
        "actor": change.actor,
        "reason": change.reason,
    })
}

fn row_json(row: &TaskRow) -> JsonValue {
    json!({
        "id": row.id,
        "ref": row.r#ref,
        "title": row.title,
        "effort": row.effort.as_str(),
        "status": row.status.as_str(),
        "checkpoint": row.checkpoint,
        "phase": row.phase,
        "files": row.files,
        "needs": row.needs,
        "coupling": row.coupling,
        "deps_note": row.deps_note,
        "action": row.action,
        "detail": row.detail,
        "acceptance": row.acceptance,
        "agent": row.agent,
        "commit": row.commit,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    const STORE: &str = r#"
schema_version = 1
plan_path = "docs/plans/demo.md"

[policy]
checkpoints = "milestones"
max_parallel = 4
commit_granularity = "per-checkpoint"

[[checkpoints]]
id = "A"
rationale = "foundation"

[[items]]
id = 1
ref = "lay-the-base"
title = "Lay the base"
effort = "S"
status = "in-progress"
checkpoint = "A"
files = ["src/a.rs"]

[[items]]
id = 2
ref = "build-on-it"
title = "Build on it"
effort = "M"
status = "pending"
checkpoint = "A"
files = ["src/b.rs"]
needs = [1]
"#;

    fn args() -> ReadIntegrityArgs {
        ReadIntegrityArgs {
            verify_integrity: false,
            strict_read: false,
        }
    }

    fn flow_dir() -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let store = tmp.path().join("tasks.toml");
        fs::write(&store, STORE).unwrap();
        (tmp, store)
    }

    fn build(store: &Path) -> JsonValue {
        snapshot("demo", store, &args(), None)
            .unwrap()
            .expect("no known revision always builds")
    }

    fn keys(value: &JsonValue) -> Vec<&str> {
        value
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect()
    }

    #[test]
    fn top_level_keys_follow_the_contract_order() {
        let (_tmp, store) = flow_dir();
        let snap = build(&store);
        assert_eq!(
            keys(&snap),
            [
                "schema",
                "revision",
                "slug",
                "plan_path",
                "flow_status",
                "policy",
                "running_milestone",
                "tasks",
                "layers",
                "frontier",
                "edges",
                "checkpoints",
                "record",
                "agents",
                "milestones",
                "changes",
                "plan",
            ]
        );
        assert_eq!(snap["slug"], json!("demo"));
        assert_eq!(snap["plan_path"], json!("docs/plans/demo.md"));
        assert_eq!(snap["layers"], json!([[1], [2]]));
        assert_eq!(
            keys(&snap["tasks"][0]),
            [
                "id",
                "ref",
                "title",
                "effort",
                "status",
                "checkpoint",
                "phase",
                "files",
                "needs",
                "coupling",
                "deps_note",
                "action",
                "detail",
                "acceptance",
                "agent",
                "commit",
                "file_notes",
                "closes",
                "refs",
            ]
        );
        assert_eq!(snap["tasks"][0]["file_notes"], json!({}));
        assert_eq!(snap["milestones"], json!([]));
        assert_eq!(snap["changes"], json!([]));
        assert_eq!(
            snap["checkpoints"][0],
            json!({
                "id": "A",
                "rationale": "foundation",
                "members": [1, 2],
                "maximal": [2],
                "valid_cut": true,
                "commits": [],
                "verification": null,
            })
        );
    }

    #[test]
    fn revision_is_stable_until_an_input_file_changes() {
        let (tmp, store) = flow_dir();
        let first = build(&store)["revision"].clone();
        let again = build(&store)["revision"].clone();
        assert_eq!(first, again);
        assert_eq!(first.as_str().unwrap().len(), REVISION_HEX_LEN);

        fs::write(tmp.path().join(CONTEXT_FILE), "status = \"in-progress\"\n").unwrap();
        let changed = build(&store);
        assert_ne!(changed["revision"], first);
        assert_eq!(changed["flow_status"], json!("in-progress"));
    }

    #[test]
    fn a_known_revision_skips_the_build_and_any_other_builds() {
        let (_tmp, store) = flow_dir();
        let revision = build(&store)["revision"].as_str().unwrap().to_string();
        let same = snapshot("demo", &store, &args(), Some(&revision)).unwrap();
        assert!(same.is_none(), "matching revision yields nothing");
        let other = snapshot("demo", &store, &args(), Some("0000000000000000")).unwrap();
        assert_eq!(other.expect("built")["revision"], json!(revision));
    }

    #[test]
    fn absent_optional_files_read_as_empty_and_are_not_created() {
        let (tmp, store) = flow_dir();
        let snap = build(&store);
        assert_eq!(snap["flow_status"], json!(""));
        assert_eq!(snap["record"], json!([]));
        assert_eq!(snap["agents"], json!([]));
        let mut names: Vec<String> = fs::read_dir(tmp.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names, ["tasks.toml"]);
    }

    #[test]
    fn agent_rows_pass_through_verbatim_including_unknown_keys() {
        let (tmp, store) = flow_dir();
        fs::write(
            tmp.path().join(AGENTS_FILE),
            "schema_version = 1\n[[agents]]\nid = \"A1\"\nagent_id = \"x\"\nfuture_key = 7\n",
        )
        .unwrap();
        let snap = build(&store);
        assert_eq!(
            snap["agents"],
            json!([{ "id": "A1", "agent_id": "x", "future_key": 7 }])
        );
    }

    #[test]
    fn the_first_snapshot_input_is_the_store_a_slug_resolves_to() {
        crate::test_support::with_root(|_root| {
            let store = store::resolve_store_path(Some("demo"), None).unwrap();
            let name = store.file_name().unwrap().to_str().unwrap();
            assert_eq!(name, crate::SNAPSHOT_INPUTS[0]);
        });
    }

    const PLAN_RUNS: &str = r#"
[[run]]
step = "implement:A"
run_id = "wf_1"
started = 2026-11-11T10:00:00Z
finished = 2026-11-11T10:30:00Z
outcome = "ok"

[[run]]
step = "review"
run_id = ""
started = 2026-11-11T11:00:00Z
finished = ""

[[run]]
step = "implement:B"
run_id = "wf_2"
started = 2026-11-11T12:00:00Z
finished = ""

[[run]]
step = "implement:C"
run_id = "wf_3"
started = 2026-11-11T13:00:00Z
finished = ""
"#;

    #[test]
    fn running_milestone_is_empty_without_a_plan_store_or_an_open_implement_run() {
        let (tmp, store) = flow_dir();
        assert_eq!(build(&store)["running_milestone"], json!(""));

        let closed = PLAN_RUNS
            .split("[[run]]\nstep = \"implement:B\"")
            .next()
            .unwrap();
        fs::write(tmp.path().join(PLAN_FILE), closed).unwrap();
        assert_eq!(build(&store)["running_milestone"], json!(""));
    }

    #[test]
    fn running_milestone_names_the_oldest_open_implement_run() {
        let (tmp, store) = flow_dir();
        let before = build(&store)["revision"].clone();
        fs::write(tmp.path().join(PLAN_FILE), PLAN_RUNS).unwrap();
        let snap = build(&store);
        assert_eq!(snap["running_milestone"], json!("B"));
        assert_ne!(
            snap["revision"], before,
            "a run opening in plan.toml moves the revision"
        );

        let strict = ReadIntegrityArgs {
            verify_integrity: false,
            strict_read: true,
        };
        let reread = snapshot("demo", &store, &strict, None).unwrap().unwrap();
        assert_eq!(reread["running_milestone"], json!("B"));
    }

    #[test]
    fn an_in_progress_row_keeps_its_dependents_in_next() {
        let (_tmp, store) = flow_dir();
        let snap = build(&store);
        assert_eq!(snap["frontier"]["next"], json!([2]));
        assert_eq!(snap["frontier"]["blocked"], json!([]));
        assert_eq!(snap["frontier"]["ready"], json!([]));
    }
}
