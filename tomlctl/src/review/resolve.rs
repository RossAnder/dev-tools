//! `review resolve` — identity outcomes for findings that name a `same_as`.
//!
//! A row resolves once its own verdicts are complete and its target is
//! settled. Passes repeat until none resolves, so a same-run target always
//! resolves before the rows naming it, whatever order the verdicts arrived
//! in. A `merged` target is followed through `merged_into` to the row it
//! joined. The outcome by target status and the row's verdict:
//!   open, exported:  confirmed or plausible → repeat; refuted → contested
//!   refuted:         confirmed → reopened; plausible → contested; refuted → repeat
//!   resolved:        confirmed → regression; plausible → contested; refuted → repeat
//!   dismissed, deferred, parked, backlogged, converted → suppressed
//! A plausible verdict never overturns a settled target; it contests it.

use anyhow::Result;
use serde_json::json;
use toml::Value as TomlValue;

use super::fields::Route;
use super::store::{self, Finding, Store, Target, Verdict};
use crate::cli::{ReadIntegrityArgs, ReviewResolveArgs};
use crate::errors::{ErrorKind, tagged_err};
use crate::output::print_json_compact;

const VERB: &str = "review resolve";
/// Target statuses that suppress any row naming them in `same_as`.
const SUPPRESSING: &[&str] = &["dismissed", "deferred", "parked", "backlogged", "converted"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Call {
    Confirmed,
    Plausible,
    Refuted,
}

impl Call {
    fn parse(verdict: &str) -> Option<Self> {
        match verdict {
            "confirmed" => Some(Self::Confirmed),
            "plausible" => Some(Self::Plausible),
            "refuted" => Some(Self::Refuted),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    Repeat,
    Contested,
    Reopened,
    Regression,
    Suppressed,
}

impl Outcome {
    fn for_target(status: &str, call: Call) -> Option<Self> {
        Some(match (status, call) {
            (s, _) if SUPPRESSING.contains(&s) => Self::Suppressed,
            ("open" | "exported", Call::Refuted) => Self::Contested,
            ("open" | "exported", _) => Self::Repeat,
            ("refuted" | "resolved", Call::Plausible) => Self::Contested,
            ("refuted" | "resolved", Call::Refuted) => Self::Repeat,
            ("refuted", Call::Confirmed) => Self::Reopened,
            ("resolved", Call::Confirmed) => Self::Regression,
            _ => return None,
        })
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Repeat => "repeat",
            Self::Contested => "contested",
            Self::Reopened => "reopened",
            Self::Regression => "regression",
            Self::Suppressed => "suppressed",
        }
    }
}

/// A row's complete verdict: the call and the indices of the verdicts that
/// made it, which a contested or reopened target gains.
struct Decision {
    call: Call,
    deciding: Vec<usize>,
}

/// What one resolve over a store did and left waiting.
struct Report {
    resolved: Vec<serde_json::Value>,
    pending: Vec<serde_json::Value>,
}

pub(crate) fn run(target: &Target, args: ReviewResolveArgs) -> Result<()> {
    if !target.path.is_file() {
        return Err(tagged_err(
            ErrorKind::NotFound,
            Some(target.path.clone()),
            format!("review store not found at {}", target.path.display()),
        ));
    }
    let read = ReadIntegrityArgs {
        verify_integrity: args.integrity.verify_integrity,
        strict_read: true,
    };

    // A rerun over an already-resolved store finds nothing waiting and
    // writes nothing, so `last_updated` is not restamped.
    let current = store::load(&target.path, &read)?;
    has_run(&current, &args.run)?;
    let mut trial = current.clone();
    let preview = resolve(&mut trial, &args.run, args.wave);
    let unchanged = trial == current;
    let report = if unchanged {
        preview
    } else {
        store::mutate(&target.path, &args.integrity, |store| {
            has_run(store, &args.run)?;
            Ok(resolve(store, &args.run, args.wave))
        })?
    };
    print_json_compact(&json!({
        "ok": true,
        "resolved": report.resolved,
        "pending": report.pending,
        "unchanged": unchanged,
    }))
}

fn has_run(store: &Store, run: &str) -> Result<()> {
    if store.runs.iter().any(|r| r.id == run) {
        Ok(())
    } else {
        Err(tagged_err(
            ErrorKind::Validation,
            None,
            format!("{VERB}: no run {run} in this store"),
        ))
    }
}

fn resolve(store: &mut Store, run: &str, wave: Option<u32>) -> Report {
    let in_wave = |f: &Finding| {
        wave.is_none_or(|n| {
            store
                .cells
                .iter()
                .any(|c| c.id == f.cell && c.wave == Some(n))
        })
    };
    let mut waiting: Vec<usize> = store
        .findings
        .iter()
        .enumerate()
        .filter(|(_, f)| {
            f.run == run && f.status == "unverified" && !f.same_as.is_empty() && in_wave(f)
        })
        .map(|(i, _)| i)
        .collect();

    let mut resolved = Vec::new();
    loop {
        let before = waiting.len();
        waiting.retain(|&i| match classify(store, i) {
            Ok((target, decision)) => match apply(store, i, target, decision) {
                Some(entry) => {
                    resolved.push(entry);
                    false
                }
                None => true,
            },
            Err(_) => true,
        });
        if waiting.len() == before {
            break;
        }
    }

    let pending = waiting
        .iter()
        .map(|&i| {
            let row = &store.findings[i];
            let reason = match classify(store, i) {
                Err(reason) => reason,
                Ok((t, _)) => format!(
                    "target {} has status {:?}, which no outcome covers",
                    store.findings[t].id, store.findings[t].status
                ),
            };
            json!({"finding": row.id, "same_as": row.same_as, "reason": reason})
        })
        .collect();
    Report { resolved, pending }
}

/// The row's settled target and complete verdict, or why it must wait.
fn classify(store: &Store, i: usize) -> std::result::Result<(usize, Decision), String> {
    let row = &store.findings[i];
    let decision = decide(store, row).ok_or_else(|| "verdicts incomplete".to_string())?;
    let target = settled_target(store, row)?;
    Ok((target, decision))
}

/// The row's verdict once complete. A panel row decides as `review verdict`
/// decides a panel: two non-refuted seats stand it (confirmed when two of
/// them confirm), two refuting seats refute it. Any other route takes the
/// latest non-panel verdict that is not `unsettled`.
fn decide(store: &Store, row: &Finding) -> Option<Decision> {
    let mut mine = store
        .verdicts
        .iter()
        .enumerate()
        .filter(|(_, v)| v.finding == row.id);
    if row.route == Some(Route::Panel) {
        let mut seats: Vec<(Option<u32>, usize)> = Vec::new();
        for (k, vote) in mine.filter(|(_, v)| v.by == "panel") {
            seats.retain(|(seat, _)| *seat != vote.seat);
            seats.push((vote.seat, k));
        }
        let deciding: Vec<usize> = seats.iter().map(|&(_, k)| k).collect();
        let count = |call: Call| {
            deciding
                .iter()
                .filter(|&&k| Call::parse(&store.verdicts[k].verdict) == Some(call))
                .count()
        };
        let (confirmed, plausible) = (count(Call::Confirmed), count(Call::Plausible));
        let call = if confirmed + plausible >= 2 {
            if confirmed >= 2 {
                Call::Confirmed
            } else {
                Call::Plausible
            }
        } else if count(Call::Refuted) >= 2 {
            Call::Refuted
        } else {
            return None;
        };
        return Some(Decision { call, deciding });
    }
    let (k, verdict) = mine.rfind(|(_, v)| v.by != "panel" && v.verdict != "unsettled")?;
    Some(Decision {
        call: Call::parse(&verdict.verdict)?,
        deciding: vec![k],
    })
}

/// Follows `same_as`, then `merged_into` through merged rows, to a target
/// whose status an outcome can read.
fn settled_target(store: &Store, row: &Finding) -> std::result::Result<usize, String> {
    let mut visited = vec![row.id.as_str()];
    let mut id = row.same_as.as_str();
    loop {
        if visited.contains(&id) {
            return Err(format!("the `same_as` chain from {} loops at {id}", row.id));
        }
        let Some(t) = store.findings.iter().position(|f| f.id == id) else {
            return Err(format!("target {id} is not in this store"));
        };
        let target = &store.findings[t];
        match target.status.as_str() {
            "merged" => {
                visited.push(id);
                id = target
                    .rest
                    .get("merged_into")
                    .and_then(TomlValue::as_str)
                    .ok_or_else(|| format!("target {id} is merged but names no `merged_into`"))?;
            }
            "unverified" => return Err(format!("target {id} is unverified")),
            "superseded" => {
                return Err(format!(
                    "target {id} is superseded; strike the identity with `review verdict --strike-same-as`"
                ));
            }
            _ => return Ok(t),
        }
    }
}

/// Applies the outcome to the row at `i` and its target at `t`, returning
/// the report entry, or `None` when no outcome covers the target's status.
fn apply(store: &mut Store, i: usize, t: usize, decision: Decision) -> Option<serde_json::Value> {
    let outcome = Outcome::for_target(&store.findings[t].status, decision.call)?;
    let target_id = store.findings[t].id.clone();
    let row = store.findings[i].clone();
    match outcome {
        Outcome::Repeat => {
            let target = &mut store.findings[t];
            if !target.seen_in.contains(&row.run) {
                target.seen_in.push(row.run.clone());
            }
            // A refuted row's anchor and evidence are the claims that failed.
            if decision.call != Call::Refuted {
                if row.line.is_some() {
                    target.line = row.line;
                }
                if row.kind == target.kind
                    && let Some(evidence) = row.rest.get("evidence")
                {
                    target.rest.insert("evidence".into(), evidence.clone());
                }
            }
        }
        Outcome::Contested => gain(store, &target_id, &decision.deciding),
        Outcome::Reopened => {
            gain(store, &target_id, &decision.deciding);
            store.findings[t].status = "open".into();
        }
        Outcome::Regression | Outcome::Suppressed => {}
    }
    let resolved = &mut store.findings[i];
    resolved.outcome = outcome.as_str().into();
    if outcome == Outcome::Regression {
        resolved.status = "open".into();
        resolved.related = vec![target_id.clone()];
    } else {
        resolved.status = "merged".into();
        resolved
            .rest
            .insert("merged_into".into(), TomlValue::String(target_id.clone()));
    }
    Some(json!({
        "finding": row.id,
        "outcome": outcome.as_str(),
        "target": target_id,
        "status": store.findings[i].status,
    }))
}

/// Copies the row's deciding verdicts onto the target. They are appended
/// rather than upserted, so the target's own verdicts under the same key
/// stay readable beside them; an identical copy is not appended twice.
fn gain(store: &mut Store, target: &str, deciding: &[usize]) {
    let gained: Vec<Verdict> = deciding
        .iter()
        .map(|&k| Verdict {
            finding: target.to_string(),
            ..store.verdicts[k].clone()
        })
        .collect();
    for verdict in gained {
        if !store.verdicts.contains(&verdict) {
            store.verdicts.push(verdict);
        }
    }
}
