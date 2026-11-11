//! `plan probes`: `--emit` prints a Bash script running each probeable `accept`
//! row's controls, and `--record` turns that script's captured stdout into each
//! row's `probe`. Both cover only pending tasks, so re-probing later leaves the
//! verdicts of work already started or finished as they were recorded.
//! `--premises` works on each pending task's premises instead: `base` ones, and
//! `after:<id>` ones once task `<id>` is done. Recording one stores `holds`,
//! `fails` or `broken` as the premise's `probe`. `--gates` works on each
//! milestone's stored gate probes by the same verdict rules. `--milestone` and
//! `--task` narrow any mode to the rows they name; `--gates` takes no `--task`.
//!
//! tomlctl never runs a stored command: `Bash(tomlctl *)` is pre-approved, so a
//! verb executing stored strings would widen that approval to arbitrary
//! commands. A row's `check` and `positive` reach the script only as one
//! single-quoted word, and a skipped row's strings do not reach it at all.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use anyhow::Result;
use serde_json::json;
use sha2::{Digest, Sha256};

use super::store;
use crate::cli::{PlanProbesArgs, ReadIntegrityArgs};
use crate::errors::{ErrorKind, tagged_err};
use crate::integrity::hex_lower;
use crate::output::{print_json_compact, print_text};
use crate::tasks::schema::{
    Accept, KIND_FALSIFIER, KIND_FORWARD, KIND_GUARD, MilestoneSpec, Premise, Probe, RUN_BY_WORKER,
    Status, Store, TaskRow, TaskSpec, VERDICT_HEALTHY, normal_path, premise_target,
};
use crate::tasks::{load_store, mutate_store, resolve_store_path};

const VERDICT_VACUOUS: &str = "vacuous";
const VERDICT_UNSATISFIABLE: &str = "unsatisfiable";
const VERDICT_PERMANENTLY_GREEN: &str = "permanently-green";
const VERDICT_BROKEN: &str = "broken";
const VERDICT_GUARD_ALONE: &str = "guard-alone";
const VERDICT_HOLDS: &str = "holds";
const VERDICT_FAILS: &str = "fails";

const CONTROL_NEG: &str = "neg";
const CONTROL_POS: &str = "pos";
const CONTROL_PREMISE: &str = "premise";
const CONTROL_GATE: &str = "gate";
const DIGEST_PREFIX: &str = "sha256=";
const PREMISE_AT_BASE: &str = "base";
const ROW_ACCEPT: &str = "accept";
const ROW_PREMISE: &str = "premises";
const ROW_GATE: &str = "gate_probes";

/// The stderr markers of a control that never tested its behaviour, whatever it
/// exited with. A syntax error exits non-zero, which would read as a healthy
/// forward's failing negative control.
const BROKEN_MARKERS: [&str; 4] = [
    "command not found",
    "fatal",
    "syntax error",
    "unexpected EOF",
];
const MISSING_PATH_MARKER: &str = "No such file";

/// What a capture row holds, for the refusal of one that holds anything else.
const ROW_SHAPE: &str = "<task>:<index><TAB>neg|pos|premise<TAB>sha256=<digest><TAB>rc=<code>\
                         <TAB><stdout><TAB><stderr>";
const GATE_ROW_SHAPE: &str = "<milestone>:<index><TAB>gate<TAB>sha256=<digest><TAB>rc=<code>\
                              <TAB><stdout><TAB><stderr>";
const QUOTED_CHARS: usize = 80;

/// Every column is folded onto one line with its tabs gone, so a command's
/// output cannot forge a row of the capture. Stderr stays whole, because a
/// breakage marker may sit past any prefix.
const HELPER: &str = r#"p(){ e=$(mktemp); o=$(eval "$4" 2>"$e" </dev/null); r=$?; printf '%s\t%s\tsha256=%s\trc=%s\t%s\t%s\n' "$1" "$2" "$3" "$r" "$(printf %s "$o" | tr '\r\n\t' ' | ' | cut -c1-100)" "$(tr '\r\n\t' ' | ' <"$e")"; rm -f "$e"; }"#;

fn refuse(message: String) -> anyhow::Error {
    tagged_err(ErrorKind::Validation, None, message)
}

/// The rows a run covers: those of one checkpoint, of the listed tasks, or both.
/// An empty `tasks` lists none, so it narrows nothing.
#[derive(Clone, Copy)]
struct Scope<'a> {
    milestone: Option<&'a str>,
    tasks: &'a [u32],
}

pub(super) fn run(mut args: PlanProbesArgs) -> Result<()> {
    let path = resolve_store_path(Some(&args.target.slug), None)?;
    let Some(capture) = args.record.take() else {
        // A flow with tasks and no `plan.toml` can still be probed, so only a
        // missing task store asks whether the flow was ever opened.
        if !path.exists() {
            store::require_open(&store::resolve(&args.target.slug)?)?;
        }
        let store = load_store(
            &path,
            &ReadIntegrityArgs {
                verify_integrity: args.integrity.verify_integrity,
                strict_read: true,
            },
        )?;
        let scope = Scope {
            milestone: args.milestone.as_deref(),
            tasks: &args.task,
        };
        if args.gates {
            return print_text(&gate_script(&store, scope)?);
        }
        let lines = if args.premises {
            premise_lines
        } else {
            accept_lines
        };
        return print_text(&script(&store, scope, lines)?);
    };
    let text = read_capture(&capture)?;
    // A capture can only describe rows of a store that already exists, so a
    // mistyped slug must not leave an empty one behind.
    args.integrity.no_create = true;
    let scope = Scope {
        milestone: args.milestone.as_deref(),
        tasks: &args.task,
    };
    if args.gates {
        let capture = parse_capture_keyed(&text, GATE_ROW_SHAPE, gate_key)?;
        let recorded = mutate_store(&path, &args.integrity, |store| {
            record_gates(store, capture, scope)
        })?;
        let recorded: Vec<_> = recorded
            .into_iter()
            .map(|(milestone, index, verdict)| {
                json!({ "milestone": milestone, "gate_probe": index, "verdict": verdict })
            })
            .collect();
        return print_json_compact(&json!({ "ok": true, "recorded": recorded }));
    }
    let capture = parse_capture(&text)?;
    if args.premises {
        let mut recorded = mutate_store(&path, &args.integrity, |store| {
            record_premises(store, capture, scope)
        })?;
        recorded.sort_unstable_by_key(|(task, premise, _)| (*task, *premise));
        let recorded: Vec<_> = recorded
            .into_iter()
            .map(|(task, premise, verdict)| {
                json!({ "task": task, "premise": premise, "verdict": verdict })
            })
            .collect();
        return print_json_compact(&json!({ "ok": true, "recorded": recorded }));
    }
    let mut recorded = mutate_store(&path, &args.integrity, |store| {
        record(store, capture, scope)
    })?;
    recorded.sort_unstable_by_key(|(task, accept, _)| (*task, *accept));
    let recorded: Vec<_> = recorded
        .into_iter()
        .map(|(task, accept, verdict)| {
            json!({
                "task": task,
                "accept": accept,
                "verdict": verdict.name,
                "reason": verdict.reason,
            })
        })
        .collect();
    print_json_compact(&json!({ "ok": true, "recorded": recorded }))
}

/// Why a row gets no probe line. Only a worker-owned check with an explicit
/// `writes_build = false` runs, so an unlabelled command, which reads as `true`,
/// never does, and neither does one a gate owns.
pub(super) fn skip_reason(accept: &Accept) -> Option<&'static str> {
    if accept.check.trim().is_empty() {
        Some("empty check")
    } else if accept.writes_build {
        Some("writes_build is not false")
    } else if accept.owner() != RUN_BY_WORKER {
        Some("run_by is not worker")
    } else {
        None
    }
}

fn has_positive(accept: &Accept) -> bool {
    !accept.positive.trim().is_empty()
}

/// Why a premise gets no line. An `after:` premise holds only once its task
/// lands, so it is checked only when that task is done. The reason reaches the
/// script, so it holds no stored string.
fn premise_skip_reason(store: &Store, premise: &Premise) -> Option<String> {
    if premise.check.trim().is_empty() {
        return Some("empty check".to_string());
    }
    if premise.at == PREMISE_AT_BASE {
        return None;
    }
    let Some(id) = premise_target(&premise.at) else {
        return Some("`at` is neither `base` nor `after:<number>`".to_string());
    };
    match store.items.iter().find(|row| row.id == id) {
        Some(row) if row.status == Status::Done => None,
        Some(_) => Some(format!("task {id} is not done")),
        None => Some(format!("no task {id} in the store")),
    }
}

/// Why a task's rows are out of this run. The reason reaches the script, so it
/// holds no stored string.
fn task_skip_reason(row: &TaskRow, scope: Scope) -> Option<&'static str> {
    if row.status != Status::Pending {
        Some("status is not pending")
    } else if scope.milestone.is_some_and(|id| row.checkpoint != id) {
        Some("not in the requested milestone")
    } else if !scope.tasks.is_empty() && !scope.tasks.contains(&row.id) {
        Some("not a requested task")
    } else {
        None
    }
}

fn refuse_out_of_store(store: &Store, scope: Scope) -> Result<()> {
    if let Some(id) = scope.milestone {
        let declared = store.checkpoints.iter().any(|c| c.id == id)
            || store.milestones.iter().any(|m| m.id == id);
        if !declared {
            return Err(refuse(format!(
                "`--milestone {id}` names no checkpoint or milestone in the store"
            )));
        }
    }
    if let Some(id) = scope
        .tasks
        .iter()
        .find(|id| !store.items.iter().any(|row| row.id == **id))
    {
        return Err(refuse(format!("`--task {id}` names no task in the store")));
    }
    Ok(())
}

/// Binds a recorded verdict to the command that produced it. It catches a stale
/// capture, not a forged one: whoever can edit a capture can edit its digests.
fn command_digest(command: &str) -> String {
    hex_lower(&Sha256::digest(command.as_bytes()))
}

/// The script. The `cd` is what lets it run from anywhere, since stored commands
/// are repo-root-relative. Nothing sets `errexit`, because a healthy negative
/// control exits non-zero and every later row must still run.
fn script(
    store: &Store,
    scope: Scope,
    lines: fn(&mut String, &Store, u32, &TaskSpec) -> Result<()>,
) -> Result<String> {
    refuse_out_of_store(store, scope)?;
    let mut out = script_header();
    for row in &store.items {
        let Some(spec) = store.specs.iter().find(|spec| spec.r#ref == row.r#ref) else {
            continue;
        };
        if let Some(reason) = task_skip_reason(row, scope) {
            writeln!(out, "# skipped task {}: {reason}", row.id)?;
            continue;
        }
        lines(&mut out, store, row.id, spec)?;
    }
    Ok(out)
}

fn script_header() -> String {
    let mut out = String::from("#!/usr/bin/env bash\n");
    out.push_str("printf 'head\\t%s\\n' \"$(git rev-parse HEAD)\"\n");
    out.push_str("cd \"$(git rev-parse --show-toplevel)\" || exit 1\n");
    out.push_str(HELPER);
    out.push('\n');
    out
}

/// The milestones a `--gates` run covers: every one, or the one `--milestone`
/// names.
fn gate_milestones<'a>(
    store: &'a Store,
    scope: Scope<'a>,
) -> impl Iterator<Item = &'a MilestoneSpec> + 'a {
    store
        .milestones
        .iter()
        .filter(move |milestone| scope.milestone.is_none_or(|id| milestone.id == id))
}

/// A milestone id reaches the script bare, as the start of its row key, so one
/// holding anything a shell word or a capture column could misread is refused.
fn gate_row_key(milestone: &str, index: usize) -> Result<String> {
    let plain = !milestone.is_empty()
        && milestone
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || "._-".contains(ch));
    if !plain {
        return Err(refuse(format!(
            "milestone {} holds gate probes, and its id must be ASCII letters, digits, `.`, `_` \
             or `-` to key a probe row",
            quoted(milestone)
        )));
    }
    Ok(format!("{milestone}:{index}"))
}

/// One line per stored gate probe, whose exit code is the probe's own: 0 when
/// the gate can run here.
fn gate_script(store: &Store, scope: Scope) -> Result<String> {
    refuse_out_of_store(store, scope)?;
    let mut out = script_header();
    for milestone in gate_milestones(store, scope) {
        for (index, gate_probe) in milestone.gate_probes.iter().enumerate() {
            let key = gate_row_key(&milestone.id, index)?;
            if gate_probe.probe.trim().is_empty() {
                writeln!(out, "# skipped {key}: empty probe")?;
                continue;
            }
            writeln!(
                out,
                "p {key} {CONTROL_GATE} {} {}",
                command_digest(&gate_probe.probe),
                quote(ROW_GATE, &key, &gate_probe.probe)?
            )?;
        }
    }
    Ok(out)
}

/// A gate row's `<milestone>:<index>` key. The index follows the last `:`.
fn gate_key(raw: &str) -> Option<(String, usize)> {
    let (milestone, index) = raw.rsplit_once(':')?;
    let index = index.parse::<usize>().ok()?;
    (!milestone.is_empty()).then(|| (milestone.to_string(), index))
}

/// Stores each in-scope gate probe's verdict by the premise rules, returning
/// `(milestone, index, verdict)` for each. As with `record_premises`, the
/// capture must cover exactly the probes the emitted script runs, each with the
/// digest of the stored probe, or nothing is written.
fn record_gates(
    store: &mut Store,
    mut capture: Capture<(String, usize)>,
    scope: Scope,
) -> Result<Vec<(String, usize, &'static str)>> {
    refuse_out_of_store(store, scope)?;
    if let Some((milestone, index)) = capture
        .controls
        .iter()
        .find(|(_, controls)| {
            controls.gate.is_none() || controls.has_accept() || controls.premise.is_some()
        })
        .map(|(key, _)| key)
    {
        return Err(refuse(format!(
            "the capture's {milestone}:{index} is not a gate probe row: record it without \
             `--gates`"
        )));
    }
    let checked: Vec<(String, usize)> = gate_milestones(store, scope)
        .flat_map(|milestone| {
            milestone
                .gate_probes
                .iter()
                .enumerate()
                .filter(|(_, gate_probe)| !gate_probe.probe.trim().is_empty())
                .map(|(index, _)| (milestone.id.clone(), index))
        })
        .collect();
    if capture.controls.len() < checked.len() {
        return Err(refuse(format!(
            "the capture covers {} of the {} gate probes in scope, as a script cut short by a \
             timeout leaves it: re-run the emitted script under a longer limit, or `plan probes \
             --emit --gates` again if the store has changed",
            capture.controls.len(),
            checked.len()
        )));
    }
    let mut recorded = Vec::new();
    for (milestone, index) in checked {
        let key = format!("{milestone}:{index}");
        let control = capture
            .controls
            .remove(&(milestone.clone(), index))
            .and_then(|controls| controls.gate)
            .ok_or_else(|| stale(&key, CONTROL_GATE))?;
        let gate_probe = store
            .milestones
            .iter_mut()
            .find(|entry| entry.id == milestone)
            .and_then(|entry| entry.gate_probes.get_mut(index))
            .expect("a gate probe collected from this store");
        if control.digest != command_digest(&gate_probe.probe) {
            return Err(refuse(format!(
                "the capture's gate control for milestone `{milestone}`'s `gate_probes[{index}]` \
                 ran a different command than the store holds: re-run `plan probes --emit --gates`"
            )));
        }
        let verdict = premise_verdict(&control);
        gate_probe.verdict = verdict.to_string();
        gate_probe.at = capture.head.clone();
        recorded.push((milestone, index, verdict));
    }
    if let Some((milestone, index)) = capture.controls.keys().next() {
        return Err(refuse(format!(
            "the capture names {milestone}:{index}, which is no stored gate probe in scope: \
             re-run `plan probes --emit --gates`"
        )));
    }
    Ok(recorded)
}

fn accept_lines(out: &mut String, _store: &Store, task: u32, spec: &TaskSpec) -> Result<()> {
    for (index, accept) in spec.accept.iter().enumerate() {
        let key = format!("{task}:{index}");
        if let Some(reason) = skip_reason(accept) {
            writeln!(out, "# skipped {key}: {reason}")?;
            continue;
        }
        writeln!(
            out,
            "p {key} {CONTROL_NEG} {} {}",
            command_digest(&accept.check),
            quote(ROW_ACCEPT, &key, &accept.check)?
        )?;
        if has_positive(accept) {
            writeln!(
                out,
                "p {key} {CONTROL_POS} {} {}",
                command_digest(&accept.positive),
                quote(ROW_ACCEPT, &key, &accept.positive)?
            )?;
        }
    }
    Ok(())
}

/// A premise's line exits 0 when its claim holds.
fn premise_lines(out: &mut String, store: &Store, task: u32, spec: &TaskSpec) -> Result<()> {
    for (index, premise) in spec.premises.iter().enumerate() {
        let key = format!("{task}:{index}");
        if let Some(reason) = premise_skip_reason(store, premise) {
            writeln!(out, "# skipped {key}: {reason}")?;
            continue;
        }
        writeln!(
            out,
            "p {key} {CONTROL_PREMISE} {} {}",
            command_digest(&premise.check),
            quote(ROW_PREMISE, &key, &premise.check)?
        )?;
    }
    Ok(())
}

/// One single-quoted shell word holding exactly `command`. Bash drops a NUL from
/// a script without failing, which would run a different command than the stored
/// one, so a NUL is refused rather than emitted.
fn quote(row_kind: &str, key: &str, command: &str) -> Result<String> {
    if command.contains('\0') {
        return Err(refuse(format!(
            "`{row_kind}` row {key} holds a NUL, which a Bash script cannot carry"
        )));
    }
    Ok(format!("'{}'", command.replace('\'', r"'\''")))
}

struct Control {
    digest: String,
    rc: i64,
    stderr: String,
}

/// The controls a capture holds under one `<task>:<index>` key. An `accept`
/// row's are `neg` and `pos`; a premise's is `premise`, its index counting
/// premises rather than `accept` rows. A gate probe's is `gate`, under a
/// `<milestone>:<index>` key.
#[derive(Default)]
struct Controls {
    neg: Option<Control>,
    pos: Option<Control>,
    premise: Option<Control>,
    gate: Option<Control>,
}

impl Controls {
    fn has_accept(&self) -> bool {
        self.neg.is_some() || self.pos.is_some()
    }
}

struct Capture<K = (u32, usize)> {
    head: String,
    controls: BTreeMap<K, Controls>,
}

/// Non-UTF-8 is read lossily: a tool's stderr may be in any encoding, and only
/// the ASCII markers are read out of it.
fn read_capture(path: &Path) -> Result<String> {
    let bytes = fs::read(path).map_err(|err| {
        let kind = match err.kind() {
            std::io::ErrorKind::NotFound => ErrorKind::NotFound,
            _ => ErrorKind::Other,
        };
        tagged_err(
            kind,
            Some(path.to_path_buf()),
            format!("reading capture {}: {err}", path.display()),
        )
    })?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// A line as a refusal quotes it: cut short, with its tabs and control characters
/// escaped so that the columns show.
fn quoted(line: &str) -> String {
    let kept: String = line.chars().take(QUOTED_CHARS).collect();
    let ellipsis = if kept.len() < line.len() { "..." } else { "" };
    format!("{kept:?}{ellipsis}")
}

fn parse_capture(text: &str) -> Result<Capture> {
    parse_capture_keyed(text, ROW_SHAPE, |raw| {
        let (task, index) = raw.split_once(':')?;
        Some((task.parse::<u32>().ok()?, index.parse::<usize>().ok()?))
    })
}

/// A capture whose row keys `key` parses, `shape` naming the row a refusal
/// expects.
fn parse_capture_keyed<K: Ord>(
    text: &str,
    shape: &str,
    key: fn(&str) -> Option<K>,
) -> Result<Capture<K>> {
    let mut lines = text
        .lines()
        .enumerate()
        .filter(|(_, line)| !line.is_empty());
    let first = lines.next().map(|(_, line)| line);
    let head = first
        .and_then(|line| line.strip_prefix("head\t"))
        .filter(|sha| !sha.is_empty() && sha.chars().all(|ch| ch.is_ascii_hexdigit()))
        .ok_or_else(|| {
            refuse(format!(
                "the capture does not open with `head<TAB><sha>`, got {}: pass the script's own \
                 stdout, from a repository with a commit",
                first.map_or_else(|| "nothing".to_string(), quoted)
            ))
        })?
        .to_string();
    let mut controls: BTreeMap<K, Controls> = BTreeMap::new();
    for (number, line) in lines {
        let malformed = || {
            refuse(format!(
                "capture line {} is not a probe row: expected `{shape}`, got {}",
                number + 1,
                quoted(line)
            ))
        };
        let no_digest = |key: &str| {
            refuse(format!(
                "capture line {} for {key} has no command digest: re-run `plan probes --emit`",
                number + 1
            ))
        };
        let columns: Vec<&str> = line.splitn(6, '\t').collect();
        if columns
            .get(2)
            .is_some_and(|column| column.starts_with("rc="))
        {
            return Err(no_digest(columns[0]));
        }
        let &[raw_key, control, digest, rc, _stdout, stderr] = columns.as_slice() else {
            return Err(malformed());
        };
        let row_key = key(raw_key).ok_or_else(malformed)?;
        let parsed = Control {
            digest: digest
                .strip_prefix(DIGEST_PREFIX)
                .filter(|digest| !digest.is_empty())
                .ok_or_else(|| no_digest(raw_key))?
                .to_string(),
            rc: rc
                .strip_prefix("rc=")
                .and_then(|raw| raw.parse::<i64>().ok())
                .ok_or_else(malformed)?,
            stderr: stderr.to_string(),
        };
        let entry = controls.entry(row_key).or_default();
        let slot = match control {
            CONTROL_NEG => &mut entry.neg,
            CONTROL_POS => &mut entry.pos,
            CONTROL_PREMISE => &mut entry.premise,
            CONTROL_GATE => &mut entry.gate,
            _ => return Err(malformed()),
        };
        if slot.replace(parsed).is_some() {
            return Err(refuse(format!(
                "capture line {} repeats a {control} control already seen",
                number + 1
            )));
        }
    }
    Ok(Capture { head, controls })
}

/// A probe's verdict and, unless it is healthy, why: the control or field read and
/// the rule it broke. Only the name reaches the store.
struct Verdict {
    name: &'static str,
    reason: String,
}

impl Verdict {
    fn new(name: &'static str, reason: String) -> Self {
        Self { name, reason }
    }

    fn healthy() -> Self {
        Self::new(VERDICT_HEALTHY, String::new())
    }
}

/// Stores the capture's probes, returning `(task, accept, verdict)` for each. A
/// capture must cover exactly the probeable controls of the tasks in scope, each
/// carrying the digest of the command the store holds now: anything else is a
/// truncated run or a store changed since `--emit`, and writes nothing.
///
/// Guards go after the rest of their task, since whether one stands alone
/// depends on the verdicts beside it.
fn record(
    store: &mut Store,
    mut capture: Capture,
    scope: Scope,
) -> Result<Vec<(u32, usize, Verdict)>> {
    refuse_out_of_store(store, scope)?;
    refuse_gate_rows(&capture)?;
    if let Some((task, index)) = capture
        .controls
        .iter()
        .find(|(_, controls)| controls.premise.is_some())
        .map(|(key, _)| *key)
    {
        return Err(refuse(format!(
            "the capture's {task}:{index} is a premise row: record a premise capture with \
             `plan probes --record <capture> --premises`"
        )));
    }
    let declared: Vec<String> = store
        .items
        .iter()
        .flat_map(|row| row.files.iter().cloned())
        .collect();
    let expected: usize = store
        .items
        .iter()
        .filter(|row| task_skip_reason(row, scope).is_none())
        .filter_map(|row| store.specs.iter().find(|spec| spec.r#ref == row.r#ref))
        .map(|spec| {
            spec.accept
                .iter()
                .filter(|a| skip_reason(a).is_none())
                .count()
        })
        .sum();
    if capture.controls.len() < expected {
        return Err(refuse(format!(
            "the capture covers {} of the {expected} probeable `accept` rows in scope, as a probe \
             script cut short by a timeout leaves it: re-run the emitted script under a longer \
             limit, or `plan probes --emit` again if the store has changed",
            capture.controls.len()
        )));
    }
    let mut recorded = Vec::new();
    for row in &store.items {
        if task_skip_reason(row, scope).is_some() {
            continue;
        }
        let Some(spec) = store.specs.iter_mut().find(|spec| spec.r#ref == row.r#ref) else {
            continue;
        };
        for guards in [false, true] {
            for index in 0..spec.accept.len() {
                if skip_reason(&spec.accept[index]).is_some()
                    || (spec.accept[index].kind == KIND_GUARD) != guards
                {
                    continue;
                }
                let key = format!("{}:{index}", row.id);
                let controls = capture
                    .controls
                    .remove(&(row.id, index))
                    .unwrap_or_default();
                let accept = &spec.accept[index];
                let neg = controls.neg.ok_or_else(|| stale(&key, CONTROL_NEG))?;
                if has_positive(accept) != controls.pos.is_some() {
                    return Err(stale(&key, CONTROL_POS));
                }
                refuse_edited(row.id, index, CONTROL_NEG, &neg, &accept.check)?;
                if let Some(pos) = &controls.pos {
                    refuse_edited(row.id, index, CONTROL_POS, pos, &accept.positive)?;
                }
                let verdict = verdict(
                    &accept.kind,
                    &neg,
                    controls.pos.as_ref(),
                    &declared,
                    spec.accept.iter().any(discriminates),
                );
                spec.accept[index].probe = Some(Probe {
                    at: capture.head.clone(),
                    neg_rc: Some(neg.rc),
                    pos_rc: controls.pos.map(|pos| pos.rc),
                    verdict: verdict.name.to_string(),
                });
                recorded.push((row.id, index, verdict));
            }
        }
    }
    if let Some((task, index)) = capture.controls.keys().next() {
        return Err(refuse(format!(
            "the capture names {task}:{index}, which is not a probeable `accept` row: re-run \
             `plan probes --emit`"
        )));
    }
    Ok(recorded)
}

/// Stores each in-scope premise's verdict, returning `(task, premise, verdict)`
/// for each. As with `record`, the capture must cover exactly the premises the
/// emitted script checks, each with the digest of the stored check, or nothing
/// is written.
fn record_premises(
    store: &mut Store,
    mut capture: Capture,
    scope: Scope,
) -> Result<Vec<(u32, usize, &'static str)>> {
    refuse_out_of_store(store, scope)?;
    refuse_gate_rows(&capture)?;
    if let Some((task, index)) = capture
        .controls
        .iter()
        .find(|(_, controls)| controls.has_accept())
        .map(|(key, _)| *key)
    {
        return Err(refuse(format!(
            "the capture's {task}:{index} is an `accept` row, not a premise: record an accept \
             capture without `--premises`"
        )));
    }
    let checked: Vec<(u32, String, usize)> = store
        .items
        .iter()
        .filter(|row| task_skip_reason(row, scope).is_none())
        .filter_map(|row| {
            let spec = store.specs.iter().find(|spec| spec.r#ref == row.r#ref)?;
            Some((row, spec))
        })
        .flat_map(|(row, spec)| {
            spec.premises
                .iter()
                .enumerate()
                .filter(|(_, premise)| premise_skip_reason(store, premise).is_none())
                .map(|(index, _)| (row.id, row.r#ref.clone(), index))
        })
        .collect();
    if capture.controls.len() < checked.len() {
        return Err(refuse(format!(
            "the capture covers {} of the {} checkable premises in scope, as a script cut short \
             by a timeout leaves it: re-run the emitted script under a longer limit, or `plan \
             probes --emit --premises` again if the store has changed",
            capture.controls.len(),
            checked.len()
        )));
    }
    let mut recorded = Vec::new();
    for (task, r#ref, index) in checked {
        let key = format!("{task}:{index}");
        let control = capture
            .controls
            .remove(&(task, index))
            .and_then(|controls| controls.premise)
            .ok_or_else(|| stale(&key, CONTROL_PREMISE))?;
        let premise = store
            .specs
            .iter_mut()
            .find(|spec| spec.r#ref == r#ref)
            .and_then(|spec| spec.premises.get_mut(index))
            .expect("a premise collected from this store");
        if control.digest != command_digest(&premise.check) {
            return Err(refuse(format!(
                "the capture's premise control for task {task}'s `premises[{index}]` ran a \
                 different command than the store holds: re-run `plan probes --emit --premises`"
            )));
        }
        let verdict = premise_verdict(&control);
        premise.probe = Some(Probe {
            at: capture.head.clone(),
            neg_rc: Some(control.rc),
            pos_rc: None,
            verdict: verdict.to_string(),
        });
        recorded.push((task, index, verdict));
    }
    if let Some((task, index)) = capture.controls.keys().next() {
        return Err(refuse(format!(
            "the capture names {task}:{index}, which is not a checkable premise: re-run `plan \
             probes --emit --premises`"
        )));
    }
    Ok(recorded)
}

/// A gate row whose milestone id is a number parses under a task key too.
fn refuse_gate_rows(capture: &Capture) -> Result<()> {
    match capture
        .controls
        .iter()
        .find(|(_, controls)| controls.gate.is_some())
    {
        Some(((milestone, index), _)) => Err(refuse(format!(
            "the capture's {milestone}:{index} is a gate probe row: record a gate capture with \
             `plan probes --record <capture> --gates`"
        ))),
        None => Ok(()),
    }
}

/// A premise's check exits 0 when its claim holds. A breakage marker means the
/// check never tested the claim, whatever it exited with.
fn premise_verdict(control: &Control) -> &'static str {
    if BROKEN_MARKERS
        .iter()
        .any(|marker| control.stderr.contains(marker))
    {
        VERDICT_BROKEN
    } else if control.rc == 0 {
        VERDICT_HOLDS
    } else {
        VERDICT_FAILS
    }
}

fn stale(key: &str, control: &str) -> anyhow::Error {
    refuse(format!(
        "the capture and the store disagree on {key}'s {control} control: re-run `plan probes \
         --emit`"
    ))
}

fn refuse_edited(
    task: u32,
    index: usize,
    name: &str,
    control: &Control,
    command: &str,
) -> Result<()> {
    if control.digest == command_digest(command) {
        return Ok(());
    }
    Err(refuse(format!(
        "the capture's {name} control for task {task}'s `accept[{index}]` ran a different command \
         than the store holds: re-run `plan probes --emit`"
    )))
}

/// A forward or falsifier with a check, unless a probe has shown it does not
/// bind. One not yet probed, such as a build-writing test, still counts.
fn discriminates(accept: &Accept) -> bool {
    matches!(accept.kind.as_str(), KIND_FORWARD | KIND_FALSIFIER)
        && !accept.check.trim().is_empty()
        && accept
            .probe
            .as_ref()
            .is_none_or(|probe| probe.verdict == VERDICT_HEALTHY)
}

/// `check` exits non-zero when its behaviour does not hold, so a zero `neg` says
/// the behaviour holds now. A guard asserts the present as a falsifier does, and
/// one that fails is a false claim about the tree rather than a weak check, so
/// it reads `broken`.
fn verdict(
    kind: &str,
    neg: &Control,
    pos: Option<&Control>,
    declared: &[String],
    discriminated: bool,
) -> Verdict {
    let controls = [(CONTROL_NEG, Some(neg)), (CONTROL_POS, pos)];
    for (name, control) in controls {
        let breakage = control.and_then(|control| stderr_breakage(&control.stderr, declared));
        if let Some(rule) = breakage {
            return Verdict::new(VERDICT_BROKEN, format!("{name}: {rule}"));
        }
    }
    let rc = neg.rc;
    match (kind, rc == 0) {
        (KIND_FORWARD, true) => Verdict::new(
            VERDICT_VACUOUS,
            format!("{CONTROL_NEG}: a forward check must fail before the work, and this exited 0"),
        ),
        (KIND_FORWARD, false) => match pos {
            Some(pos) if pos.rc != 0 => Verdict::new(
                VERDICT_UNSATISFIABLE,
                format!(
                    "{CONTROL_POS}: the positive control must pass, and this exited {}",
                    pos.rc
                ),
            ),
            _ => Verdict::healthy(),
        },
        (KIND_FALSIFIER, true) => Verdict::healthy(),
        (KIND_FALSIFIER, false) => Verdict::new(
            VERDICT_PERMANENTLY_GREEN,
            format!("{CONTROL_NEG}: a falsifier check must pass today, and this exited {rc}"),
        ),
        (KIND_GUARD, true) if discriminated => Verdict::healthy(),
        (KIND_GUARD, true) => Verdict::new(
            VERDICT_GUARD_ALONE,
            format!(
                "{CONTROL_NEG}: this guard passes, and no forward or falsifier row beside it \
                 discriminates"
            ),
        ),
        (KIND_GUARD, false) => Verdict::new(
            VERDICT_BROKEN,
            format!("{CONTROL_NEG}: a guard must pass today, and this exited {rc}"),
        ),
        _ => Verdict::new(
            VERDICT_BROKEN,
            format!("kind: `{kind}` is not an accept kind"),
        ),
    }
}

/// The rule a control's stderr breaks, if any. A missing path a row declares is
/// what the task will create, so only an undeclared one is a broken command. `|` is
/// the helper's folded newline.
fn stderr_breakage(stderr: &str, declared: &[String]) -> Option<String> {
    if let Some(marker) = BROKEN_MARKERS
        .iter()
        .find(|marker| stderr.contains(*marker))
    {
        return Some(format!("stderr holds the marker `{marker}`"));
    }
    stderr
        .split('|')
        .find(|line| {
            line.split_once(MISSING_PATH_MARKER)
                .is_some_and(|(before, _)| !names_declared_path(before, declared))
        })
        .map(|line| {
            format!(
                "stderr names a missing path no row declares: {}",
                quoted(line)
            )
        })
}

/// Whether a token of `text` is a declared path, or either is the other's tail
/// at a component boundary: stored commands may `cd` into a subdirectory, and a
/// tool may print an absolute path.
fn names_declared_path(text: &str, declared: &[String]) -> bool {
    let declared: Vec<String> = declared.iter().map(|path| normal_path(path)).collect();
    text.split(|ch: char| ch.is_whitespace() || "':\"`(),;".contains(ch))
        .map(normal_path)
        .filter(|token| !token.is_empty())
        .any(|token| {
            declared.iter().any(|path| {
                path == &token
                    || path.ends_with(&format!("/{token}"))
                    || token.ends_with(&format!("/{path}"))
            })
        })
}
