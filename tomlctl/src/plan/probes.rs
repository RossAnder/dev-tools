//! `plan probes`: `--emit` prints a Bash script running each probeable `accept`
//! row's controls, and `--record` turns that script's captured stdout into each
//! row's `probe`.
//!
//! tomlctl never runs a stored command: `Bash(tomlctl *)` is pre-approved, so a
//! verb executing stored strings would widen that approval to arbitrary
//! commands. A row's `check` and `positive` reach the script only as one
//! single-quoted word, and a skipped row's strings do not reach it at all.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde_json::json;

use crate::cli::{PlanProbesArgs, ReadIntegrityArgs};
use crate::errors::{ErrorKind, tagged_err};
use crate::output::{print_json_compact, print_text};
use crate::tasks::schema::{Accept, Probe, Store};
use crate::tasks::{load_store, mutate_store, resolve_store_path};

const KIND_FORWARD: &str = "forward";
const KIND_FALSIFIER: &str = "falsifier";
const KIND_GUARD: &str = "guard";

const VERDICT_HEALTHY: &str = "healthy";
const VERDICT_VACUOUS: &str = "vacuous";
const VERDICT_UNSATISFIABLE: &str = "unsatisfiable";
const VERDICT_PERMANENTLY_GREEN: &str = "permanently-green";
const VERDICT_BROKEN: &str = "broken";
const VERDICT_GUARD_ALONE: &str = "guard-alone";

const CONTROL_NEG: &str = "neg";
const CONTROL_POS: &str = "pos";

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

/// Every column is folded onto one line with its tabs gone, so a command's
/// output cannot forge a row of the capture. Stderr stays whole, because a
/// breakage marker may sit past any prefix.
const HELPER: &str = r#"p(){ e=$(mktemp); o=$(eval "$3" 2>"$e" </dev/null); r=$?; printf '%s\t%s\trc=%s\t%s\t%s\n' "$1" "$2" "$r" "$(printf %s "$o" | tr '\r\n\t' ' | ' | cut -c1-100)" "$(tr '\r\n\t' ' | ' <"$e")"; rm -f "$e"; }"#;

fn refuse(message: String) -> anyhow::Error {
    tagged_err(ErrorKind::Validation, None, message)
}

pub(super) fn run(mut args: PlanProbesArgs) -> Result<()> {
    let path = resolve_store_path(Some(&args.target.slug), None)?;
    let Some(capture) = args.record.take() else {
        let store = load_store(
            &path,
            &ReadIntegrityArgs {
                verify_integrity: args.integrity.verify_integrity,
                strict_read: true,
            },
        )?;
        return print_text(&script(&store)?);
    };
    let capture = read_capture(&capture)?;
    // A capture can only describe rows of a store that already exists, so a
    // mistyped slug must not leave an empty one behind.
    args.integrity.no_create = true;
    let mut recorded = mutate_store(&path, &args.integrity, |store| record(store, capture))?;
    recorded.sort_unstable();
    let recorded: Vec<_> = recorded
        .into_iter()
        .map(
            |(task, accept, verdict)| json!({ "task": task, "accept": accept, "verdict": verdict }),
        )
        .collect();
    print_json_compact(&json!({ "ok": true, "recorded": recorded }))
}

/// Why a row gets no probe line. Only an explicit `writes_build = false` runs,
/// so an unlabelled command, which reads as `true`, never does.
fn skip_reason(accept: &Accept) -> Option<&'static str> {
    if accept.check.trim().is_empty() {
        Some("empty check")
    } else if accept.writes_build {
        Some("writes_build is not false")
    } else {
        None
    }
}

fn has_positive(accept: &Accept) -> bool {
    !accept.positive.trim().is_empty()
}

/// The script. The `cd` is what lets it run from anywhere, since stored commands
/// are repo-root-relative. Nothing sets `errexit`, because a healthy negative
/// control exits non-zero and every later row must still run.
fn script(store: &Store) -> Result<String> {
    let mut out = String::from("#!/usr/bin/env bash\n");
    out.push_str("printf 'head\\t%s\\n' \"$(git rev-parse HEAD)\"\n");
    out.push_str("cd \"$(git rev-parse --show-toplevel)\" || exit 1\n");
    out.push_str(HELPER);
    out.push('\n');
    for row in &store.items {
        let Some(spec) = store.specs.iter().find(|spec| spec.r#ref == row.r#ref) else {
            continue;
        };
        for (index, accept) in spec.accept.iter().enumerate() {
            let key = format!("{}:{index}", row.id);
            if let Some(reason) = skip_reason(accept) {
                writeln!(out, "# skipped {key}: {reason}")?;
                continue;
            }
            writeln!(out, "p {key} {CONTROL_NEG} {}", quote(&key, &accept.check)?)?;
            if has_positive(accept) {
                writeln!(
                    out,
                    "p {key} {CONTROL_POS} {}",
                    quote(&key, &accept.positive)?
                )?;
            }
        }
    }
    Ok(out)
}

/// One single-quoted shell word holding exactly `command`. Bash drops a NUL from
/// a script without failing, which would run a different command than the stored
/// one, so a NUL is refused rather than emitted.
fn quote(key: &str, command: &str) -> Result<String> {
    if command.contains('\0') {
        return Err(refuse(format!(
            "`accept` row {key} holds a NUL, which a Bash script cannot carry"
        )));
    }
    Ok(format!("'{}'", command.replace('\'', r"'\''")))
}

struct Control {
    rc: i64,
    stderr: String,
}

#[derive(Default)]
struct Controls {
    neg: Option<Control>,
    pos: Option<Control>,
}

struct Capture {
    head: String,
    controls: BTreeMap<(u32, usize), Controls>,
}

/// Non-UTF-8 is read lossily: a tool's stderr may be in any encoding, and only
/// the ASCII markers are read out of it.
fn read_capture(path: &Path) -> Result<Capture> {
    let bytes = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    parse_capture(&String::from_utf8_lossy(&bytes))
}

fn parse_capture(text: &str) -> Result<Capture> {
    let mut lines = text
        .lines()
        .enumerate()
        .filter(|(_, line)| !line.is_empty());
    let head = lines
        .next()
        .and_then(|(_, line)| line.strip_prefix("head\t"))
        .filter(|sha| !sha.is_empty() && sha.chars().all(|ch| ch.is_ascii_hexdigit()))
        .ok_or_else(|| {
            refuse(
                "the capture does not open with `head<TAB><sha>`: pass the script's own stdout, \
                 from a repository with a commit"
                    .to_string(),
            )
        })?
        .to_string();
    let mut controls: BTreeMap<(u32, usize), Controls> = BTreeMap::new();
    for (number, line) in lines {
        let malformed = || refuse(format!("capture line {} is not a probe row", number + 1));
        let columns: Vec<&str> = line.splitn(5, '\t').collect();
        let &[key, control, rc, _stdout, stderr] = columns.as_slice() else {
            return Err(malformed());
        };
        let (task, index) = key.split_once(':').ok_or_else(malformed)?;
        let key = (
            task.parse::<u32>().map_err(|_| malformed())?,
            index.parse::<usize>().map_err(|_| malformed())?,
        );
        let parsed = Control {
            rc: rc
                .strip_prefix("rc=")
                .and_then(|raw| raw.parse::<i64>().ok())
                .ok_or_else(malformed)?,
            stderr: stderr.to_string(),
        };
        let entry = controls.entry(key).or_default();
        let slot = match control {
            CONTROL_NEG => &mut entry.neg,
            CONTROL_POS => &mut entry.pos,
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

/// Stores the capture's probes, returning `(task, accept, verdict)` for each. A
/// capture must cover exactly the store's probeable controls: anything else is a
/// truncated run or a store changed since `--emit`, and writes nothing.
///
/// Guards go after the rest of their task, since whether one stands alone
/// depends on the verdicts beside it.
fn record(store: &mut Store, mut capture: Capture) -> Result<Vec<(u32, usize, &'static str)>> {
    let declared: Vec<String> = store
        .items
        .iter()
        .flat_map(|row| row.files.iter().cloned())
        .collect();
    let mut recorded = Vec::new();
    for row in &store.items {
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
                    verdict: verdict.to_string(),
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

fn stale(key: &str, control: &str) -> anyhow::Error {
    refuse(format!(
        "the capture and the store disagree on {key}'s {control} control: re-run `plan probes \
         --emit`"
    ))
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
) -> &'static str {
    let is_broken = |control: &Control| stderr_names_breakage(&control.stderr, declared);
    if is_broken(neg) || pos.is_some_and(is_broken) {
        return VERDICT_BROKEN;
    }
    let holds = neg.rc == 0;
    match (kind, holds) {
        (KIND_FORWARD, true) => VERDICT_VACUOUS,
        (KIND_FORWARD, false) => match pos {
            Some(pos) if pos.rc != 0 => VERDICT_UNSATISFIABLE,
            _ => VERDICT_HEALTHY,
        },
        (KIND_FALSIFIER, true) => VERDICT_HEALTHY,
        (KIND_FALSIFIER, false) => VERDICT_PERMANENTLY_GREEN,
        (KIND_GUARD, true) if discriminated => VERDICT_HEALTHY,
        (KIND_GUARD, true) => VERDICT_GUARD_ALONE,
        _ => VERDICT_BROKEN,
    }
}

/// A missing path a row declares is what the task will create, so only an
/// undeclared one is a broken command. `|` is the helper's folded newline.
fn stderr_names_breakage(stderr: &str, declared: &[String]) -> bool {
    BROKEN_MARKERS.iter().any(|marker| stderr.contains(marker))
        || stderr.split('|').any(|line| {
            line.split_once(MISSING_PATH_MARKER)
                .is_some_and(|(before, _)| !names_declared_path(before, declared))
        })
}

/// Whether a token of `text` is a declared path, or either is the other's tail
/// at a component boundary: stored commands may `cd` into a subdirectory, and a
/// tool may print an absolute path.
fn names_declared_path(text: &str, declared: &[String]) -> bool {
    text.split(|ch: char| ch.is_whitespace() || "':\"`(),;".contains(ch))
        .map(|token| token.replace('\\', "/"))
        .map(|token| token.trim_start_matches("./").to_string())
        .filter(|token| !token.is_empty())
        .any(|token| {
            declared.iter().any(|path| {
                path == &token
                    || path.ends_with(&format!("/{token}"))
                    || token.ends_with(&format!("/{path}"))
            })
        })
}
