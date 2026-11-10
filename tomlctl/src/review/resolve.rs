//! `review resolve` — identity outcomes for findings that name a `same_as`.
//!
//! A row resolves once its own verdicts are complete and its target is
//! settled and not itself awaiting a `same_as`. Passes repeat until none resolves, so a same-run target always
//! resolves before the rows naming it, whatever order the verdicts arrived
//! in. A `merged` target is followed through `merged_into` to the row it
//! joined, and a `converted` advisory through `converted_to` to its
//! replacement. The outcome by target status and the row's verdict:
//!   open, exported:  confirmed or plausible → repeat; refuted → contested
//!   refuted:         confirmed → reopened; plausible → contested; refuted → repeat
//!   resolved:        confirmed → regression; plausible → contested; refuted → repeat
//!   dismissed, deferred, parked, backlogged → suppressed
//! A plausible verdict never overturns a settled target; it contests it.
//! `--same-as` links two findings of the run first, verified or not, and the
//! pair resolves by the same table.

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

    let links = parse_links(&args.same_as)?;

    // A rerun over an already-resolved store finds nothing waiting and
    // writes nothing, so `last_updated` is not restamped.
    let current = store::load(&target.path, &read)?;
    has_run(&current, &args.run)?;
    let mut trial = current.clone();
    let slug = super::add::store_slug(target);
    let linked = link(&mut trial, &args.run, &links)?;
    let preview = resolve(&mut trial, &args.run, args.wave, slug.as_deref(), &linked);
    let unchanged = trial == current;
    let report = if unchanged {
        preview
    } else {
        store::mutate(&target.path, &args.integrity, |store| {
            has_run(store, &args.run)?;
            let linked = link(store, &args.run, &links)?;
            Ok(resolve(
                store,
                &args.run,
                args.wave,
                slug.as_deref(),
                &linked,
            ))
        })?
    };
    let linked: Vec<serde_json::Value> = linked
        .iter()
        .map(|l| json!({"finding": l.dup, "same_as": l.target}))
        .collect();
    print_json_compact(&json!({
        "ok": true,
        "linked": linked,
        "resolved": report.resolved,
        "pending": report.pending,
        "unchanged": unchanged,
    }))
}

/// One `--same-as` pair: the duplicate and the finding it repeats.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Link {
    dup: String,
    target: String,
}

fn parse_links(raw: &[String]) -> Result<Vec<Link>> {
    raw.iter()
        .map(|pair| match pair.split_once('=') {
            Some((dup, target)) if !dup.trim().is_empty() && !target.trim().is_empty() => {
                Ok(Link {
                    dup: dup.trim().to_string(),
                    target: target.trim().to_string(),
                })
            }
            _ => Err(invalid(format!(
                "{VERB}: --same-as {pair:?} is not DUP=TARGET"
            ))),
        })
        .collect()
}

/// Sets each duplicate's `same_as` to its target and returns the accepted
/// pairs. Every pair is checked before any is set, so one refused pair
/// leaves the store untouched.
fn link(store: &mut Store, run: &str, links: &[Link]) -> Result<Vec<Link>> {
    let mut accepted: Vec<Link> = Vec::new();
    let mut reasons = Vec::new();
    for l in links {
        if accepted.contains(l) {
            continue;
        }
        match check_link(store, run, l, &accepted) {
            Ok(()) => accepted.push(l.clone()),
            Err(reason) => reasons.push(reason),
        }
    }
    if !reasons.is_empty() {
        return Err(invalid(format!(
            "{VERB}: --same-as refused: {}",
            reasons.join("; ")
        )));
    }
    for l in &accepted {
        if let Some(dup) = store.findings.iter_mut().find(|f| f.id == l.dup)
            && dup.status() != FindingStatus::Merged
        {
            dup.same_as.clone_from(&l.target);
        }
    }
    Ok(accepted)
}

/// Why the pair cannot be linked, given the pairs already accepted in this
/// call. A duplicate already merged into its target, or already naming it
/// in `same_as`, is accepted and changes nothing.
fn check_link(
    store: &Store,
    run: &str,
    l: &Link,
    accepted: &[Link],
) -> std::result::Result<(), String> {
    let (dup_id, target_id) = (l.dup.as_str(), l.target.as_str());
    if dup_id == target_id {
        return Err(format!("{dup_id} cannot be linked to itself"));
    }
    if let Some(other) = accepted.iter().find(|a| a.dup == dup_id) {
        return Err(format!(
            "{dup_id} is linked to both {} and {target_id}",
            other.target
        ));
    }
    let find = |id: &str| {
        store
            .findings
            .iter()
            .find(|f| f.id == id)
            .ok_or_else(|| format!("no finding {id} in this store"))
    };
    let dup = find(dup_id)?;
    let target = find(target_id)?;
    for f in [dup, target] {
        if f.run != run {
            return Err(format!("{} is from run {}, not {run}", f.id, f.run));
        }
        if f.status() == FindingStatus::Superseded {
            return Err(format!("{} is superseded", f.id));
        }
    }
    if dup.family != target.family {
        return Err(format!(
            "{dup_id} is {} and {target_id} is {}; a pair shares its family",
            dup.family.as_str(),
            target.family.as_str()
        ));
    }
    if dup.status() == FindingStatus::Merged {
        let into = dup.rest.get("merged_into").and_then(TomlValue::as_str);
        return if into == Some(target_id) {
            Ok(())
        } else {
            Err(format!(
                "{dup_id} is already merged into {}",
                into.unwrap_or("an unnamed row")
            ))
        };
    }
    if !dup.same_as.is_empty() && dup.same_as != target_id {
        return Err(format!(
            "{dup_id} already names {} in `same_as`; strike it with `review verdict --strike-same-as` first",
            dup.same_as
        ));
    }
    if dup.same_as.is_empty() && !dup.status().decided_by_verdict() {
        return Err(format!(
            "{dup_id} is {}; only an unverified, open or refuted row can be linked",
            dup.status().as_str()
        ));
    }
    if leads_to(store, target_id, dup_id, accepted) {
        return Err(format!(
            "linking {dup_id} to {target_id} makes a cycle: {target_id} already leads to {dup_id}"
        ));
    }
    Ok(())
}

/// Whether the identity chain from `from` reaches `to`, following the pairs
/// accepted in this call before each row's stored `same_as`, `merged_into`
/// or `converted_to`.
fn leads_to(store: &Store, from: &str, to: &str, accepted: &[Link]) -> bool {
    let mut visited: Vec<&str> = Vec::new();
    let mut id = from;
    loop {
        if id == to {
            return true;
        }
        if visited.contains(&id) {
            return false;
        }
        visited.push(id);
        let next = accepted
            .iter()
            .find(|a| a.dup == id)
            .map(|a| a.target.as_str())
            .or_else(|| {
                store
                    .findings
                    .iter()
                    .find(|f| f.id == id)?
                    .identity_links()
                    .next()
            });
        match next {
            Some(n) => id = n,
            None => return false,
        }
    }
}

/// A row whose `same_as` has not yet been resolved: its verdict decides its
/// status and no outcome has been recorded.
pub(super) fn awaits_resolution(f: &Finding) -> bool {
    !f.same_as.is_empty() && f.outcome.is_empty() && f.status().decided_by_verdict()
}

fn has_run(store: &Store, run: &str) -> Result<()> {
    if store.runs.iter().any(|r| r.id == run) {
        Ok(())
    } else {
        Err(invalid(format!("{VERB}: no run {run} in this store")))
    }
}

/// Resolves the run's rows awaiting their `same_as`. A pair linked in this
/// call is resolved whatever wave its duplicate came from.
fn resolve(
    store: &mut Store,
    run: &str,
    wave: Option<u32>,
    slug: Option<&str>,
    linked: &[Link],
) -> Report {
    let in_wave = |f: &Finding| {
        linked.iter().any(|l| l.dup == f.id)
            || wave.is_none_or(|n| {
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
        .filter(|(_, f)| f.run == run && awaits_resolution(f) && in_wave(f))
        .map(|(i, _)| i)
        .collect();

    let mut resolved = Vec::new();
    loop {
        let before = waiting.len();
        waiting.retain(|&i| match classify(store, i) {
            Ok((target, decision)) => match apply(store, i, target, decision, slug) {
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
            _ if awaits_resolution(target) => {
                return Err(format!("target {id} awaits its own `same_as`"));
            }
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
fn apply(
    store: &mut Store,
    i: usize,
    t: usize,
    decision: Decision,
    slug: Option<&str>,
) -> Option<serde_json::Value> {
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
            stamp_tier(store, t, slug);
        }
        Outcome::Regression | Outcome::Suppressed => {}
    }
    let resolved = &mut store.findings[i];
    resolved.outcome = outcome.as_str().into();
    if outcome == Outcome::Regression {
        resolved.set_status(FindingStatus::Open, &[]);
        resolved.related = vec![target_id.clone()];
        stamp_tier(store, i, slug);
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

/// Stamps the tier of the finding at `at` as it opens. A store outside flow
/// mode, or a tier that cannot be computed, leaves the finding unstamped so
/// the resolve still lands.
fn stamp_tier(store: &mut Store, at: usize, slug: Option<&str>) {
    let Some(slug) = slug else { return };
    let id = store.findings[at].id.clone();
    let mine: Vec<&Verdict> = store.verdicts.iter().filter(|v| v.finding == id).collect();
    let _ = super::tier::stamp(&mut store.findings[at], slug, &mine);
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
