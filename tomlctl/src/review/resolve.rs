//! `review resolve` — identity outcomes for findings that name a `same_as`.
//!
//! A row resolves once its own verdicts are complete and its target is
//! settled. Passes repeat until none resolves, so a same-run target always
//! resolves before the rows naming it, whatever order the verdicts arrived
//! in. A `merged` target is followed through `merged_into` to the row it
//! joined, and a `converted` advisory through `converted_to` to its
//! replacement. The outcome by target status and the row's verdict:
//!   open, exported:  confirmed or plausible → repeat; refuted → contested
//!   refuted:         confirmed → reopened; plausible → contested; refuted → repeat
//!   resolved:        confirmed → regression; plausible → contested; refuted → repeat
//!   dismissed, deferred, parked, backlogged → suppressed
//! A plausible verdict never overturns a settled target; it contests it.

use anyhow::Result;
use serde_json::json;
use toml::Value as TomlValue;

use super::fields::FindingStatus;
use super::invalid;
use super::store::{self, Finding, Store, Target, Verdict};
use super::verdict::settle;
use crate::cli::{ReadIntegrityArgs, ReviewResolveArgs};
use crate::output::print_json_compact;

const VERB: &str = "review resolve";

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
    fn for_target(status: FindingStatus, call: Call) -> Option<Self> {
        use FindingStatus::{Exported, Open, Refuted, Resolved};
        Some(match (status, call) {
            (s, _) if s.is_suppressing() => Self::Suppressed,
            (Open | Exported, Call::Refuted) => Self::Contested,
            (Open | Exported, _) => Self::Repeat,
            (Refuted | Resolved, Call::Plausible) => Self::Contested,
            (Refuted | Resolved, Call::Refuted) => Self::Repeat,
            (Refuted, Call::Confirmed) => Self::Reopened,
            (Resolved, Call::Confirmed) => Self::Regression,
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
        Err(invalid(format!("{VERB}: no run {run} in this store")))
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
            f.run == run
                && f.status() == FindingStatus::Unverified
                && !f.same_as.is_empty()
                && in_wave(f)
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

/// The row's verdict once complete, settled by the rule `review verdict`
/// applies; every non-panel author counts.
fn decide(store: &Store, row: &Finding) -> Option<Decision> {
    let (indices, mine): (Vec<usize>, Vec<&Verdict>) = store
        .verdicts
        .iter()
        .enumerate()
        .filter(|(_, v)| v.finding == row.id)
        .unzip();
    let settled = settle(row.route, &mine, |_| true)?;
    Some(Decision {
        call: Call::parse(settled.verdict)?,
        deciding: settled.deciding.iter().map(|&k| indices[k]).collect(),
    })
}

/// Follows `same_as`, then `merged_into` through merged rows and
/// `converted_to` through converted ones, to a target whose status an
/// outcome can read.
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
        let link = match target.status() {
            FindingStatus::Merged => Some("merged_into"),
            FindingStatus::Converted => Some("converted_to"),
            _ => None,
        };
        if let Some(link) = link {
            visited.push(id);
            id = target
                .rest
                .get(link)
                .and_then(TomlValue::as_str)
                .ok_or_else(|| {
                    format!(
                        "target {id} is {} but names no `{link}`",
                        target.status().as_str()
                    )
                })?;
            continue;
        }
        match target.status() {
            FindingStatus::Unverified => return Err(format!("target {id} is unverified")),
            FindingStatus::Superseded => {
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
    let outcome = Outcome::for_target(store.findings[t].status(), decision.call)?;
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
            store.findings[t].set_status(FindingStatus::Open, &[]);
        }
        Outcome::Regression | Outcome::Suppressed => {}
    }
    let resolved = &mut store.findings[i];
    resolved.outcome = outcome.as_str().into();
    if outcome == Outcome::Regression {
        resolved.set_status(FindingStatus::Open, &[]);
        resolved.related = vec![target_id.clone()];
    } else {
        resolved.set_status(
            FindingStatus::Merged,
            &[("merged_into", TomlValue::String(target_id.clone()))],
        );
    }
    Some(json!({
        "finding": row.id,
        "outcome": outcome.as_str(),
        "target": target_id,
        "status": store.findings[i].status().as_str(),
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
