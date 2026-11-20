//! `plan milestone add` and `plan milestone update`.
//!
//! A milestone is a `[[checkpoints]]` row (the id and its goal, as `rationale`)
//! plus a `[[milestones]]` row (gates and elaboration), written under one lock
//! so the id `tasks add --checkpoint` validates against always has both. A
//! review edit sets one field through the same write as `update`.

use anyhow::Result;
use serde_json::{Value as JsonValue, json};

use super::store;
use crate::cli::{
    PlanMilestoneAddArgs, PlanMilestoneUpdateArgs, ReadIntegrityArgs, WriteIntegrityArgs,
};
use crate::errors::{ErrorKind, tagged_err};
use crate::output::print_json_compact;
use crate::tasks::schema::{
    BASELINE_FAIL, BASELINE_NULL_CONTROL, BASELINE_PASS, BaselineProof, Change, Checkpoint,
    DEFAULT_ACTOR, ELABORATION_DEFERRED, ELABORATION_ELABORATED, ELABORATION_UPFRONT, GateProbe,
    MILESTONE_REF_PREFIX, MilestoneSpec, Store,
};
use crate::tasks::{
    ReviewEditOutcome, load_store, mutate_store, resolve_store_path, unwritten_outcome,
    written_only,
};

fn refuse(message: String) -> anyhow::Error {
    tagged_err(ErrorKind::Validation, None, message)
}

fn trimmed(raw: Option<&str>) -> &str {
    raw.map(str::trim).unwrap_or_default()
}

fn change(
    id: &str,
    field: &str,
    old: String,
    new: String,
    actor: &str,
    reason: &str,
) -> Result<Change> {
    Ok(Change {
        at: Some(crate::time::now_toml_datetime()?),
        r#ref: format!("{MILESTONE_REF_PREFIX}{id}"),
        field: field.to_string(),
        old,
        new,
        actor: actor.to_string(),
        reason: reason.to_string(),
    })
}

const GATE_PROBE_SEPARATOR: &str = " :: ";

/// Splits each `--gate-probe` value at its first ` :: ` into `(gate, probe)`.
/// A gate's own command is refused as its probe, since running it is no cheaper
/// than the gate.
fn parse_gate_probes(raw: &[String]) -> Result<Vec<(String, String)>> {
    raw.iter()
        .map(|value| {
            let Some((gate, probe)) = value.split_once(GATE_PROBE_SEPARATOR) else {
                return Err(refuse(format!(
                    "`--gate-probe {value}` has no ` :: `: pass `<gate> :: <probe>`"
                )));
            };
            if gate.trim().is_empty() || probe.trim().is_empty() {
                return Err(refuse(format!(
                    "`--gate-probe {value}` needs both a gate and a probe around ` :: `"
                )));
            }
            if gate == probe {
                return Err(refuse(format!(
                    "`--gate-probe {value}` names the gate as its own probe: a probe is a cheap \
                     command, such as the gate's dry run"
                )));
            }
            Ok((gate.to_string(), probe.to_string()))
        })
        .collect()
}

/// The stored probes for `pairs`, each of which must name one of `gates`. A
/// pair already in `old` keeps its recorded verdict.
fn gate_probes_for(
    id: &str,
    pairs: &[(String, String)],
    gates: &[String],
    old: &[GateProbe],
) -> Result<Vec<GateProbe>> {
    pairs
        .iter()
        .map(|(gate, probe)| {
            if !gates.contains(gate) {
                return Err(refuse(format!(
                    "`--gate-probe` names `{gate}`, which is no gate of milestone `{id}`"
                )));
            }
            Ok(old
                .iter()
                .find(|entry| &entry.gate == gate && &entry.probe == probe)
                .cloned()
                .unwrap_or_else(|| GateProbe {
                    gate: gate.clone(),
                    probe: probe.clone(),
                    verdict: String::new(),
                    at: String::new(),
                }))
        })
        .collect()
}

/// The gate list and the external marks for `--gate` and `--external-gate`:
/// each external command is appended after the plain gates unless already
/// there, and each external command is stored and marked once. The plain gates
/// are kept as given.
fn gates_and_marks(gates: &[String], external: &[String]) -> (Vec<String>, Vec<String>) {
    let mut all = gates.to_vec();
    let mut marks: Vec<String> = Vec::new();
    for gate in external {
        if !all.contains(gate) {
            all.push(gate.clone());
        }
        if !marks.contains(gate) {
            marks.push(gate.clone());
        }
    }
    (all, marks)
}

/// `--ui yes|no` as the stored call.
fn parse_ui(raw: Option<&str>) -> Result<Option<bool>> {
    match raw.map(str::trim) {
        None => Ok(None),
        Some("yes") => Ok(Some(true)),
        Some("no") => Ok(Some(false)),
        Some(other) => Err(refuse(format!(
            "`--ui {other}` is not a UI call: pass `yes` or `no`"
        ))),
    }
}

/// The baseline gates for `--baseline-gate`, each stored once and each one of
/// `gates`, which holds the external gates too.
fn baseline_gates_for(id: &str, named: &[String], gates: &[String]) -> Result<Vec<String>> {
    let mut kept: Vec<String> = Vec::new();
    for gate in named {
        if !gates.contains(gate) {
            return Err(refuse(format!(
                "`--baseline-gate {gate}` names no gate of milestone `{id}`: a baseline gate must \
                 also be a `--gate` or `--external-gate`"
            )));
        }
        if !kept.contains(gate) {
            kept.push(gate.clone());
        }
    }
    Ok(kept)
}

/// One `--baseline-proof`, parsed but not yet checked against the store.
#[derive(Debug, Clone)]
struct ProofArg {
    raw: String,
    gate: String,
    verdict: String,
    task: Option<u32>,
}

/// Splits each `--baseline-proof` value at its first ` :: ` into the gate and
/// the verdict; a `null-control` verdict takes a third part, the task id.
fn parse_baseline_proofs(raw: &[String]) -> Result<Vec<ProofArg>> {
    raw.iter()
        .map(|value| {
            let Some((gate, rest)) = value.split_once(GATE_PROBE_SEPARATOR) else {
                return Err(refuse(format!(
                    "`--baseline-proof {value}` has no ` :: `: pass `<gate> :: <verdict>`"
                )));
            };
            if gate.trim().is_empty() {
                return Err(refuse(format!(
                    "`--baseline-proof {value}` needs a gate before ` :: `"
                )));
            }
            let (verdict, task) = match rest.split_once(GATE_PROBE_SEPARATOR) {
                Some((verdict, task)) => (verdict.trim(), Some(task.trim())),
                None => (rest.trim(), None),
            };
            let task = match (verdict, task) {
                (BASELINE_NULL_CONTROL, Some(task)) => Some(task.parse::<u32>().map_err(|_| {
                    refuse(format!(
                        "`--baseline-proof {value}` names task `{task}`, which is not a task id"
                    ))
                })?),
                (BASELINE_NULL_CONTROL, None) => {
                    return Err(refuse(format!(
                        "`--baseline-proof {value}` is a null-control proof and needs the task \
                         whose accept runs it: pass `<gate> :: null-control :: <task id>`"
                    )));
                }
                (BASELINE_PASS | BASELINE_FAIL, None) => None,
                (BASELINE_PASS | BASELINE_FAIL, Some(_)) => {
                    return Err(refuse(format!(
                        "`--baseline-proof {value}`: a `{verdict}` verdict takes no third part"
                    )));
                }
                _ => {
                    return Err(refuse(format!(
                        "`--baseline-proof {value}` has verdict `{verdict}`: expected one of \
                         {BASELINE_PASS}, {BASELINE_FAIL}, {BASELINE_NULL_CONTROL}"
                    )));
                }
            };
            Ok(ProofArg {
                raw: value.clone(),
                gate: gate.to_string(),
                verdict: verdict.to_string(),
                task,
            })
        })
        .collect()
}

/// Refuses a null-control proof whose task is no row of milestone `id`, or a
/// row whose spec holds no accept entry to run the gate's tool.
fn check_proof_tasks(store: &Store, id: &str, proofs: &[ProofArg]) -> Result<()> {
    for proof in proofs {
        let Some(task) = proof.task else {
            continue;
        };
        let Some(row) = store
            .items
            .iter()
            .find(|row| row.id == task && row.checkpoint == id)
        else {
            return Err(refuse(format!(
                "`--baseline-proof {}` names task {task}, which is no task of milestone `{id}`",
                proof.raw
            )));
        };
        let has_accept = store
            .specs
            .iter()
            .any(|spec| spec.r#ref == row.r#ref && !spec.accept.is_empty());
        if !has_accept {
            return Err(refuse(format!(
                "`--baseline-proof {}` names task {task}, which holds no accept entry to run the \
                 gate's tool on the base against itself",
                proof.raw
            )));
        }
    }
    Ok(())
}

fn baseline_proofs_json(proofs: &[BaselineProof]) -> String {
    let entries: Vec<JsonValue> = proofs
        .iter()
        .map(|entry| match entry.task {
            Some(task) => json!({ "gate": entry.gate, "verdict": entry.verdict, "task": task }),
            None => json!({ "gate": entry.gate, "verdict": entry.verdict }),
        })
        .collect();
    JsonValue::Array(entries).to_string()
}

fn ui_text(ui: Option<bool>) -> String {
    ui.map(|call| call.to_string()).unwrap_or_default()
}

fn gate_probes_json(probes: &[GateProbe]) -> String {
    let pairs: Vec<JsonValue> = probes
        .iter()
        .map(|entry| json!({ "gate": entry.gate, "probe": entry.probe }))
        .collect();
    JsonValue::Array(pairs).to_string()
}

/// Refuses a capture-state name outside `^[a-z0-9][a-z0-9-]{0,63}$`, or one
/// given twice to `flag`.
fn check_ui_names(flag: &str, names: &[String]) -> Result<()> {
    for (index, name) in names.iter().enumerate() {
        let mut chars = name.chars();
        let well_formed = name.len() <= 64
            && chars
                .next()
                .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
            && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
        if !well_formed {
            return Err(refuse(format!(
                "`{flag} {name}` is not a state name: lowercase letters, digits and `-`, \
                 starting with a letter or digit, at most 64 characters"
            )));
        }
        if names[..index].contains(name) {
            return Err(refuse(format!("`{flag} {name}` is given twice")));
        }
    }
    Ok(())
}

/// Refuses a name that is both a state the visual pass judges and a control
/// the inspector must fail.
fn check_ui_disjoint(id: &str, states: &[String], controls: &[String]) -> Result<()> {
    match states.iter().find(|name| controls.contains(name)) {
        Some(name) => Err(refuse(format!(
            "`{name}` is both a ui state and a ui control of milestone `{id}`"
        ))),
        None => Ok(()),
    }
}

/// Refuses an `after` id that is not stored before `id` in `[[checkpoints]]`;
/// an id not yet stored counts as last.
fn check_after(store: &Store, id: &str, after: &[String]) -> Result<()> {
    let own = store
        .checkpoints
        .iter()
        .position(|entry| entry.id == id)
        .unwrap_or(store.checkpoints.len());
    for target in after {
        let earlier = store
            .checkpoints
            .iter()
            .position(|entry| &entry.id == target)
            .is_some_and(|index| index < own);
        if !earlier {
            return Err(refuse(format!(
                "`--after {target}` names no milestone stored before `{id}`"
            )));
        }
    }
    Ok(())
}

pub(super) fn add(args: PlanMilestoneAddArgs) -> Result<()> {
    let id = args.id.trim().to_string();
    if id.is_empty() {
        return Err(refuse("`--id` must not be empty".to_string()));
    }
    let (gates, external_gates) = gates_and_marks(&args.gate, &args.external_gate);
    let gate_probes = gate_probes_for(&id, &parse_gate_probes(&args.gate_probe)?, &gates, &[])?;
    let baseline_gates = baseline_gates_for(&id, &args.baseline_gate, &gates)?;
    let ui = parse_ui(args.ui.as_deref())?;
    check_ui_names("--ui-state", &args.ui_state)?;
    check_ui_names("--ui-control", &args.ui_control)?;
    check_ui_disjoint(&id, &args.ui_state, &args.ui_control)?;
    let reason = trimmed(args.reason.as_deref()).to_string();
    if args.defer && reason.is_empty() {
        return Err(refuse("`--defer` needs `--reason <text>`".to_string()));
    }
    store::require_open(&store::resolve(&args.target.slug)?)?;
    let path = resolve_store_path(Some(&args.target.slug), None)?;
    mutate_store(&path, &args.integrity, |store: &mut Store| {
        if store.audit && reason.is_empty() {
            return Err(refuse(
                "this store is audited: pass --reason <text> to add a milestone".to_string(),
            ));
        }
        if store.checkpoints.iter().any(|entry| entry.id == id)
            || store.milestones.iter().any(|entry| entry.id == id)
        {
            return Err(refuse(format!("milestone `{id}` already exists")));
        }
        check_after(store, &id, &args.after)?;
        store.checkpoints.push(Checkpoint {
            id: id.clone(),
            rationale: args.goal.clone(),
        });
        store.milestones.push(MilestoneSpec {
            id: id.clone(),
            gates: gates.clone(),
            elaboration: match args.defer {
                true => ELABORATION_DEFERRED.to_string(),
                false => ELABORATION_UPFRONT.to_string(),
            },
            defer_reason: match args.defer {
                true => reason.clone(),
                false => String::new(),
            },
            gate_probes: gate_probes.clone(),
            after: args.after.clone(),
            ui_states: args.ui_state.clone(),
            ui_controls: args.ui_control.clone(),
            external_gates: external_gates.clone(),
            baseline_gates: baseline_gates.clone(),
            baseline_proofs: Vec::new(),
            ui,
        });
        if store.audit {
            store.changes.push(change(
                &id,
                "*",
                String::new(),
                args.goal.clone(),
                DEFAULT_ACTOR,
                &reason,
            )?);
        }
        Ok(())
    })?;
    print_json_compact(&json!({ "ok": true, "id": id }))
}

pub(super) fn update(args: PlanMilestoneUpdateArgs) -> Result<()> {
    let id = args.id.trim().to_string();
    if args.goal.is_none()
        && args.gate.is_empty()
        && args.external_gate.is_empty()
        && !args.clear_gate
        && args.gate_probe.is_empty()
        && args.baseline_gate.is_empty()
        && args.baseline_proof.is_empty()
        && args.ui.is_none()
        && args.ui_state.is_empty()
        && args.ui_control.is_empty()
        && !args.clear_ui
        && args.after.is_empty()
        && !args.clear_after
        && !args.defer
        && !args.elaborated
    {
        return Err(refuse(
            "nothing to update: pass `--goal`, `--gate`, `--external-gate`, `--clear-gate`, \
             `--gate-probe`, `--baseline-gate`, `--baseline-proof`, `--ui`, \
             `--ui-state`, `--ui-control`, `--clear-ui`, `--after`, `--clear-after`, \
             `--defer` or `--elaborated`"
                .to_string(),
        ));
    }
    let ui = parse_ui(args.ui.as_deref())?;
    let baseline_proofs = parse_baseline_proofs(&args.baseline_proof)?;
    if args.clear_ui && (!args.ui_state.is_empty() || !args.ui_control.is_empty()) {
        return Err(refuse(
            "`--clear-ui` and `--ui-state` or `--ui-control` set the ui lists two different ways"
                .to_string(),
        ));
    }
    check_ui_names("--ui-state", &args.ui_state)?;
    check_ui_names("--ui-control", &args.ui_control)?;
    if args.clear_after && !args.after.is_empty() {
        return Err(refuse(
            "`--clear-after` and `--after` set the after list two different ways".to_string(),
        ));
    }
    let gate_probes = parse_gate_probes(&args.gate_probe)?;
    if args.clear_gate && !args.gate.is_empty() {
        return Err(refuse(
            "`--clear-gate` and `--gate` set the gate list two different ways".to_string(),
        ));
    }
    if args.clear_gate && !args.external_gate.is_empty() {
        return Err(refuse(
            "`--clear-gate` and `--external-gate` set the gate list two different ways".to_string(),
        ));
    }
    let gates_given = !args.gate.is_empty() || !args.external_gate.is_empty() || args.clear_gate;
    let (gates, external_gates) = gates_and_marks(&args.gate, &args.external_gate);
    if args.defer && args.elaborated {
        return Err(refuse(
            "`--defer` and `--elaborated` move the milestone in opposite directions".to_string(),
        ));
    }
    let reason = trimmed(args.reason.as_deref()).to_string();
    if args.defer && reason.is_empty() {
        return Err(refuse("`--defer` needs `--reason <text>`".to_string()));
    }
    if args.elaborated && reason.is_empty() {
        return Err(refuse("`--elaborated` needs `--reason <text>`".to_string()));
    }
    store::require_open(&store::resolve(&args.target.slug)?)?;
    let path = resolve_store_path(Some(&args.target.slug), None)?;
    let patch = MilestonePatch {
        goal: args.goal.clone(),
        gates: gates_given.then_some(gates),
        external_gates: gates_given.then_some(external_gates),
        gate_probes: (!gate_probes.is_empty()).then_some(gate_probes),
        baseline_gates: (!args.baseline_gate.is_empty()).then(|| args.baseline_gate.clone()),
        baseline_proofs,
        ui,
        ui_states: (!args.ui_state.is_empty() || args.clear_ui).then(|| args.ui_state.clone()),
        ui_controls: (!args.ui_control.is_empty() || args.clear_ui)
            .then(|| args.ui_control.clone()),
        after: (!args.after.is_empty() || args.clear_after).then(|| args.after.clone()),
        elaboration: match (args.defer, args.elaborated) {
            (true, _) => Some(ELABORATION_DEFERRED.to_string()),
            (_, true) => Some(ELABORATION_ELABORATED.to_string()),
            _ => None,
        },
        defer_reason: args.defer.then(|| reason.clone()),
    };
    mutate_store(&path, &args.integrity, |store: &mut Store| {
        patch_milestone(store, &id, &patch, DEFAULT_ACTOR, &reason)
    })?;
    print_json_compact(&json!({ "ok": true, "id": id }))
}

/// The fields one milestone write sets; `None` leaves a field as it is.
#[derive(Default)]
struct MilestonePatch {
    goal: Option<String>,
    gates: Option<Vec<String>>,
    /// The external marks replacing the whole mark list. `None` with `gates`
    /// set keeps only the marks whose command is still a gate.
    external_gates: Option<Vec<String>>,
    /// `(gate, probe)` pairs replacing the whole probe list, each checked
    /// against the gates the patch leaves.
    gate_probes: Option<Vec<(String, String)>>,
    /// The baseline gates replacing the whole list, each checked against the
    /// gates the patch leaves. `None` with `gates` set keeps only the baseline
    /// gates still a gate.
    baseline_gates: Option<Vec<String>>,
    /// Proofs upserted by gate, each naming a baseline gate the patch leaves.
    baseline_proofs: Vec<ProofArg>,
    ui: Option<bool>,
    ui_states: Option<Vec<String>>,
    ui_controls: Option<Vec<String>>,
    after: Option<Vec<String>>,
    elaboration: Option<String>,
    defer_reason: Option<String>,
}

/// Writes `patch` to milestone `id`, adding its `[[milestones]]` row when only
/// the checkpoint exists, and audits each field that changed under `actor`.
/// Only a deferred milestone can become `elaborated`, and a milestone left
/// deferred by the patch needs a `defer_reason`.
fn patch_milestone(
    store: &mut Store,
    id: &str,
    patch: &MilestonePatch,
    actor: &str,
    reason: &str,
) -> Result<()> {
    if store.audit && reason.is_empty() {
        return Err(refuse(
            "this store is audited: pass --reason <text> to update a milestone".to_string(),
        ));
    }
    if !store.checkpoints.iter().any(|entry| entry.id == id) {
        return Err(tagged_err(
            ErrorKind::NotFound,
            None,
            format!("no milestone `{id}` in the store"),
        ));
    }
    if let Some(after) = &patch.after {
        check_after(store, id, after)?;
    }
    check_proof_tasks(store, id, &patch.baseline_proofs)?;
    let checkpoint = store
        .checkpoints
        .iter_mut()
        .find(|entry| entry.id == id)
        .expect("checkpoint presence checked above");
    let mut changes: Vec<(&str, String, String)> = Vec::new();
    if let Some(goal) = &patch.goal {
        let old = std::mem::replace(&mut checkpoint.rationale, goal.clone());
        changes.push(("goal", old, goal.clone()));
    }
    let index = match store.milestones.iter().position(|entry| entry.id == id) {
        Some(index) => index,
        None => {
            store.milestones.push(MilestoneSpec {
                id: id.to_string(),
                gates: Vec::new(),
                elaboration: ELABORATION_UPFRONT.to_string(),
                defer_reason: String::new(),
                gate_probes: Vec::new(),
                after: Vec::new(),
                ui_states: Vec::new(),
                ui_controls: Vec::new(),
                external_gates: Vec::new(),
                baseline_gates: Vec::new(),
                baseline_proofs: Vec::new(),
                ui: None,
            });
            store.milestones.len() - 1
        }
    };
    let spec = &mut store.milestones[index];
    if let Some(gates) = &patch.gates {
        let old = std::mem::replace(&mut spec.gates, gates.clone());
        changes.push(("gates", json!(old).to_string(), json!(gates).to_string()));
    }
    let marks = match (&patch.external_gates, &patch.gates) {
        (Some(marks), _) => Some(marks.clone()),
        (None, Some(_)) => Some(
            spec.external_gates
                .iter()
                .filter(|mark| spec.gates.contains(mark))
                .cloned()
                .collect(),
        ),
        (None, None) => None,
    };
    if let Some(marks) = marks {
        let old = std::mem::replace(&mut spec.external_gates, marks);
        changes.push((
            "external_gates",
            json!(old).to_string(),
            json!(spec.external_gates).to_string(),
        ));
    }
    if let Some(pairs) = &patch.gate_probes {
        let probes = gate_probes_for(id, pairs, &spec.gates, &spec.gate_probes)?;
        let old = std::mem::replace(&mut spec.gate_probes, probes);
        changes.push((
            "gate_probes",
            gate_probes_json(&old),
            gate_probes_json(&spec.gate_probes),
        ));
    }
    let baseline = match (&patch.baseline_gates, &patch.gates) {
        (Some(named), _) => Some(baseline_gates_for(id, named, &spec.gates)?),
        (None, Some(_)) => Some(
            spec.baseline_gates
                .iter()
                .filter(|gate| spec.gates.contains(gate))
                .cloned()
                .collect(),
        ),
        (None, None) => None,
    };
    if let Some(baseline) = baseline {
        let old = std::mem::replace(&mut spec.baseline_gates, baseline);
        changes.push((
            "baseline_gates",
            json!(old).to_string(),
            json!(spec.baseline_gates).to_string(),
        ));
    }
    let old_proofs = baseline_proofs_json(&spec.baseline_proofs);
    let gates_left = spec.baseline_gates.clone();
    spec.baseline_proofs
        .retain(|proof| gates_left.contains(&proof.gate));
    if !patch.baseline_proofs.is_empty() {
        let at = crate::time::now_toml_datetime()?;
        for proof in &patch.baseline_proofs {
            if !spec.baseline_gates.contains(&proof.gate) {
                return Err(refuse(format!(
                    "`--baseline-proof {}` names `{}`, which is no baseline gate of milestone \
                     `{id}`",
                    proof.raw, proof.gate
                )));
            }
            let entry = BaselineProof {
                gate: proof.gate.clone(),
                verdict: proof.verdict.clone(),
                at: Some(at),
                task: proof.task,
            };
            match spec
                .baseline_proofs
                .iter()
                .position(|stored| stored.gate == proof.gate)
            {
                Some(slot) => spec.baseline_proofs[slot] = entry,
                None => spec.baseline_proofs.push(entry),
            }
        }
    }
    changes.push((
        "baseline_proofs",
        old_proofs,
        baseline_proofs_json(&spec.baseline_proofs),
    ));
    if let Some(ui) = patch.ui {
        let old = spec.ui.replace(ui);
        changes.push(("ui", ui_text(old), ui_text(spec.ui)));
    }
    if patch.ui_states.is_some() || patch.ui_controls.is_some() {
        check_ui_disjoint(
            id,
            patch.ui_states.as_deref().unwrap_or(&spec.ui_states),
            patch.ui_controls.as_deref().unwrap_or(&spec.ui_controls),
        )?;
    }
    if let Some(states) = &patch.ui_states {
        let old = std::mem::replace(&mut spec.ui_states, states.clone());
        changes.push((
            "ui_states",
            json!(old).to_string(),
            json!(states).to_string(),
        ));
    }
    if let Some(controls) = &patch.ui_controls {
        let old = std::mem::replace(&mut spec.ui_controls, controls.clone());
        changes.push((
            "ui_controls",
            json!(old).to_string(),
            json!(controls).to_string(),
        ));
    }
    if let Some(after) = &patch.after {
        let old = std::mem::replace(&mut spec.after, after.clone());
        changes.push(("after", json!(old).to_string(), json!(after).to_string()));
    }
    if let Some(elaboration) = &patch.elaboration {
        if !ELABORATIONS.contains(&elaboration.as_str()) {
            return Err(refuse(format!(
                "unknown elaboration `{elaboration}` — expected one of {}",
                ELABORATIONS.join(", ")
            )));
        }
        if elaboration == ELABORATION_ELABORATED && !spec.is_deferred() {
            return Err(refuse(format!(
                "milestone `{id}` is `{}`, and only a deferred milestone can be elaborated",
                spec.elaboration
            )));
        }
        let old = std::mem::replace(&mut spec.elaboration, elaboration.clone());
        changes.push(("elaboration", old, elaboration.clone()));
    }
    if let Some(defer_reason) = &patch.defer_reason {
        let old = std::mem::replace(&mut spec.defer_reason, defer_reason.clone());
        changes.push(("defer_reason", old, defer_reason.clone()));
    }
    if (patch.elaboration.is_some() || patch.defer_reason.is_some())
        && spec.is_deferred()
        && spec.defer_reason.trim().is_empty()
    {
        return Err(refuse(format!(
            "milestone `{id}` is deferred and needs a `defer_reason`"
        )));
    }
    if store.audit {
        for (field, old, new) in changes.into_iter().filter(|(_, old, new)| old != new) {
            store
                .changes
                .push(change(id, field, old, new, actor, reason)?);
        }
    }
    Ok(())
}

const ELABORATIONS: [&str; 3] = [
    ELABORATION_UPFRONT,
    ELABORATION_DEFERRED,
    ELABORATION_ELABORATED,
];

/// `goal` is the milestone's `[[checkpoints]]` rationale; the rest live on its
/// `[[milestones]]` row.
pub(super) const REVIEW_FIELDS: [&str; 4] = ["goal", "gates", "elaboration", "defer_reason"];

/// One compare-and-set edit to a milestone field. `expect` is required and
/// `new` is the field's whole value: a string, or an array of strings for
/// `gates`.
pub(crate) struct MilestoneEdit<'a> {
    pub(crate) id: &'a str,
    pub(crate) field: &'a str,
    pub(crate) expect: Option<&'a JsonValue>,
    pub(crate) new: &'a JsonValue,
    pub(crate) actor: &'a str,
    pub(crate) reason: &'a str,
}

/// Applies `edit` to the flow's task store, re-reading the field and checking
/// `expect` inside the store lock, through the write `plan milestone update`
/// makes. Only `Applied` writes; a refusal is an error and writes nothing.
pub(crate) fn apply_review_edit(
    slug: &str,
    integrity: &WriteIntegrityArgs,
    edit: &MilestoneEdit<'_>,
) -> Result<ReviewEditOutcome> {
    store::require_open(&store::resolve(slug)?)?;
    let path = resolve_store_path(Some(slug), None)?;
    let integrity = WriteIntegrityArgs {
        no_create: true,
        ..integrity.clone()
    };
    unwritten_outcome(mutate_store(&path, &integrity, |store: &mut Store| {
        written_only(milestone_edit(store, edit)?)
    }))
}

/// What `apply_review_edit` would return against the store as it stands,
/// writing nothing.
pub(crate) fn validate_review_edit(
    slug: &str,
    integrity: &WriteIntegrityArgs,
    edit: &MilestoneEdit<'_>,
) -> Result<ReviewEditOutcome> {
    store::require_open(&store::resolve(slug)?)?;
    let path = resolve_store_path(Some(slug), None)?;
    let mut store = load_store(
        &path,
        &ReadIntegrityArgs {
            verify_integrity: integrity.verify_integrity,
            strict_read: true,
        },
    )?;
    milestone_edit(&mut store, edit)
}

fn milestone_edit(store: &mut Store, edit: &MilestoneEdit<'_>) -> Result<ReviewEditOutcome> {
    let id = edit.id;
    let refuse_edit =
        |message: &str| refuse(format!("milestone `{id}` `{}`: {message}", edit.field));
    if !REVIEW_FIELDS.contains(&edit.field) {
        return Err(refuse_edit(&format!(
            "a review edit targets one of {}",
            REVIEW_FIELDS.join(", ")
        )));
    }
    let Some(expect) = edit.expect else {
        return Err(refuse_edit("the edit needs `expect`"));
    };
    let mut patch = MilestonePatch::default();
    match edit.field {
        "gates" => {
            let gates = edit
                .new
                .as_array()
                .and_then(|entries| {
                    entries
                        .iter()
                        .map(|entry| entry.as_str().map(str::to_string))
                        .collect::<Option<Vec<_>>>()
                })
                .ok_or_else(|| refuse_edit("`new` must be an array of strings"))?;
            patch.gates = Some(gates);
        }
        field => {
            let text = edit
                .new
                .as_str()
                .ok_or_else(|| refuse_edit("`new` must be a string"))?
                .to_string();
            match field {
                "goal" => patch.goal = Some(text),
                "elaboration" => patch.elaboration = Some(text),
                _ => patch.defer_reason = Some(text),
            }
        }
    }
    let current = milestone_value(store, id, edit.field).ok_or_else(|| {
        tagged_err(
            ErrorKind::NotFound,
            None,
            format!("no milestone `{id}` in the store"),
        )
    })?;
    if &current == edit.new {
        return Ok(ReviewEditOutcome::Already);
    }
    if expect != &current {
        return Ok(ReviewEditOutcome::Stale { found: current });
    }
    patch_milestone(store, id, &patch, edit.actor, edit.reason)?;
    Ok(ReviewEditOutcome::Applied)
}

/// The field as a review edit sees it; a milestone with no `[[milestones]]`
/// row reads as the row `patch_milestone` would add.
pub(super) fn milestone_value(store: &Store, id: &str, field: &str) -> Option<JsonValue> {
    let checkpoint = store.checkpoints.iter().find(|entry| entry.id == id)?;
    let spec = store.milestones.iter().find(|entry| entry.id == id);
    Some(match field {
        "goal" => json!(checkpoint.rationale),
        "gates" => json!(spec.map(|s| s.gates.clone()).unwrap_or_default()),
        "elaboration" => json!(spec.map_or(ELABORATION_UPFRONT, |s| s.elaboration.as_str())),
        "defer_reason" => json!(spec.map(|s| s.defer_reason.as_str()).unwrap_or_default()),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seeded() -> Store {
        let mut store = Store {
            audit: true,
            ..Store::default()
        };
        store.checkpoints.push(Checkpoint {
            id: "A".to_string(),
            rationale: "old goal".to_string(),
        });
        store
    }

    fn edit<'a>(
        field: &'a str,
        expect: Option<&'a JsonValue>,
        new: &'a JsonValue,
    ) -> MilestoneEdit<'a> {
        MilestoneEdit {
            id: "A",
            field,
            expect,
            new,
            actor: "review",
            reason: "review r1 F2",
        }
    }

    #[test]
    fn a_milestone_edit_sets_audits_then_reports_already_and_stale() {
        let mut store = seeded();
        let (old, new) = (json!([]), json!(["cargo test"]));
        assert_eq!(
            milestone_edit(&mut store, &edit("gates", Some(&old), &new)).expect("applies"),
            ReviewEditOutcome::Applied
        );
        assert_eq!(store.milestones[0].gates, vec!["cargo test".to_string()]);
        let audit = store.changes.last().expect("an audit row");
        assert_eq!(
            (
                audit.r#ref.as_str(),
                audit.field.as_str(),
                audit.actor.as_str()
            ),
            ("milestone:A", "gates", "review")
        );
        assert_eq!(
            milestone_edit(&mut store, &edit("gates", Some(&old), &new)).expect("no error"),
            ReviewEditOutcome::Already
        );
        let (stale_expect, goal) = (json!("not the goal"), json!("new goal"));
        assert_eq!(
            milestone_edit(&mut store, &edit("goal", Some(&stale_expect), &goal))
                .expect("no error"),
            ReviewEditOutcome::Stale {
                found: json!("old goal")
            }
        );
        assert_eq!(store.changes.len(), 1);
    }

    #[test]
    fn a_milestone_edit_refuses_what_the_update_path_refuses() {
        let mut store = seeded();
        let upfront = json!("upfront");
        let values = [
            json!("elaborated"),
            json!("deferred"),
            json!("later"),
            json!("cargo test"),
            json!("new goal"),
            json!("B"),
        ];
        let cases = [
            edit("elaboration", Some(&upfront), &values[0]),
            edit("elaboration", Some(&upfront), &values[1]),
            edit("elaboration", Some(&upfront), &values[2]),
            edit("gates", Some(&upfront), &values[3]),
            edit("goal", None, &values[4]),
            edit("id", Some(&upfront), &values[5]),
        ];
        for case in &cases {
            assert!(
                milestone_edit(&mut store.clone(), case).is_err(),
                "{} {} should be refused",
                case.field,
                case.new
            );
        }
        store.checkpoints.clear();
        let (goal, new) = (json!("old goal"), json!("x"));
        assert!(milestone_edit(&mut store, &edit("goal", Some(&goal), &new)).is_err());
    }
}
