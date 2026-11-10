//! `review settle` — what became of a settle task's proposal for a finding
//! blocked on evidence: accepted with its tier and fix, refuted through an
//! `orchestrator` verdict, or deferred with its note. Every outcome takes the
//! finding off `scheduled` and clears its apply link. A replay of an outcome
//! already recorded writes nothing.
//!
//! A deferred proposal ages by the runs of its finding's family started after
//! its `proposal_at`; at [`OVERDUE_AGE`] it is overdue, and [`deferred`] is
//! what `review digest` lists and `review run finish` refuses on.

use std::path::Path;

use anyhow::Result;
use jiff::Timestamp;
use serde_json::{Value as JsonValue, json};
use toml::Value as TomlValue;
use toml::value::Datetime;

use super::fields::{self, BlockedOn, Family, FindingStatus, Record};
use super::invalid;
use super::store::{self, Finding, Store, Target};
use super::verdict;
use crate::cli::{ReadIntegrityArgs, WriteIntegrityArgs};
use crate::io::{read_prose, relativise_under, repo_or_cwd_root};
use crate::output::print_json_compact;

const VERB: &str = "review settle";

/// The number of later runs of its family at which a deferred proposal is
/// overdue.
pub(super) const OVERDUE_AGE: usize = 3;

const REFUTE_RATIONALE: &str = "its settle proposal refuted the finding";

enum Outcome {
    Accept { tier: String, fix: String },
    Refute { citation: String },
    Defer { proposal: String },
}

impl Outcome {
    fn name(&self) -> &'static str {
        match self {
            Self::Accept { .. } => "accepted",
            Self::Refute { .. } => "refuted",
            Self::Defer { .. } => "deferred",
        }
    }
}

/// The `review settle` flags: exactly one of `accept`, `refute` and
/// `defer`, with its companions.
pub(crate) struct Request<'a> {
    pub(crate) id: &'a str,
    pub(crate) accept: bool,
    pub(crate) refute: bool,
    pub(crate) defer: bool,
    pub(crate) tier: Option<&'a str>,
    pub(crate) fix_file: Option<&'a Path>,
    pub(crate) citation: Option<&'a str>,
    pub(crate) proposal: Option<&'a Path>,
}

pub(crate) fn run(
    target: &Target,
    args: Request<'_>,
    integrity: &WriteIntegrityArgs,
) -> Result<()> {
    let outcome = read_outcome(&args)?;
    let id = args.id.trim().to_string();
    if id.is_empty() {
        return Err(invalid(format!("{VERB}: no finding id given")));
    }
    let read = ReadIntegrityArgs {
        verify_integrity: integrity.verify_integrity,
        strict_read: true,
    };
    let now = crate::time::now_toml_datetime()?;
    let slug = super::add::store_slug(target);
    let current = store::load(&target.path, &read)?;
    let mut trial = current.clone();
    let preview = settle(&mut trial, &id, &outcome, now, slug.as_deref())?;
    let unchanged = trial == current;
    let applied = if unchanged {
        preview
    } else {
        store::mutate(&target.path, integrity, |store| {
            settle(store, &id, &outcome, now, slug.as_deref())
        })?
    };
    let mut out = applied;
    out["ok"] = json!(true);
    out["outcome"] = json!(outcome.name());
    out["unchanged"] = json!(unchanged);
    print_json_compact(&out)
}

/// The one outcome the flags carry, with its companion read and checked.
fn read_outcome(args: &Request<'_>) -> Result<Outcome> {
    if args.accept {
        let tier = args.tier.unwrap_or_default().trim();
        if !fields::SETTLE_TIERS.contains(&tier) {
            return Err(invalid(format!(
                "{VERB}: --tier is {tier:?}; expected one of {}",
                fields::SETTLE_TIERS.join(", ")
            )));
        }
        let fix = read_prose(VERB, "fix", None, args.fix_file)?
            .ok_or_else(|| invalid(format!("{VERB}: --accept needs --fix-file")))?;
        let cap = fields::spec(Record::Finding, "fix_sketch", None).and_then(|s| s.cap);
        if let Some(cap) = cap
            && fix.chars().count() > cap
        {
            return Err(invalid(format!(
                "{VERB}: the --fix-file text is {} characters; the cap is {cap}",
                fix.chars().count()
            )));
        }
        return Ok(Outcome::Accept {
            tier: tier.to_string(),
            fix,
        });
    }
    if args.refute {
        let citation = args.citation.unwrap_or_default().trim();
        if citation.is_empty() {
            return Err(invalid(format!("{VERB}: --refute needs --citation")));
        }
        return Ok(Outcome::Refute {
            citation: citation.to_string(),
        });
    }
    if args.defer {
        let path = args
            .proposal
            .ok_or_else(|| invalid(format!("{VERB}: --defer needs --proposal")))?;
        return Ok(Outcome::Defer {
            proposal: proposal_path(path)?,
        });
    }
    Err(invalid(format!("{VERB}: no outcome given")))
}

/// The proposal note as a repo-relative path. It is recorded in a committed
/// store, so a note that is missing or outside the repo is refused.
fn proposal_path(path: &Path) -> Result<String> {
    if !path.is_file() {
        return Err(invalid(format!(
            "{VERB}: --proposal {} is not a file",
            path.display()
        )));
    }
    let root = repo_or_cwd_root()?;
    let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    relativise_under(&root, &absolute).ok_or_else(|| {
        invalid(format!(
            "{VERB}: --proposal {} is outside the repository",
            path.display()
        ))
    })
}

fn text<'f>(finding: &'f Finding, key: &str) -> Option<&'f str> {
    finding.rest.get(key).and_then(TomlValue::as_str)
}

/// Whether `finding` already carries `outcome`, so a replay writes nothing.
fn recorded(store: &Store, finding: &Finding, outcome: &Outcome) -> bool {
    let unlinked = text(finding, "apply_flow").is_none() && text(finding, "apply_task").is_none();
    unlinked
        && match outcome {
            Outcome::Accept { tier, fix } => {
                finding.status() == FindingStatus::Open
                    && fields::blocked_on(finding).is_none()
                    && text(finding, "unblocked_by") == Some("settle")
                    && text(finding, "settle_tier") == Some(tier)
                    && text(finding, "fix_sketch") == Some(fix)
            }
            Outcome::Refute { citation } => {
                finding.status() == FindingStatus::Refuted
                    && store.verdicts.iter().any(|v| {
                        v.finding == finding.id
                            && v.by == "orchestrator"
                            && v.verdict == "refuted"
                            && &v.citation == citation
                    })
            }
            Outcome::Defer { proposal } => {
                finding.status() == FindingStatus::Open
                    && text(finding, "proposal") == Some(proposal)
            }
        }
}

/// Why `outcome` cannot be recorded on `finding`, if it cannot.
fn refusals(finding: &Finding, outcome: &Outcome) -> Vec<String> {
    let id = &finding.id;
    let mut reasons = Vec::new();
    let status = finding.status();
    if !matches!(status, FindingStatus::Open | FindingStatus::Scheduled) {
        reasons.push(format!(
            "{id} is {}; settle records a proposal for an open or scheduled finding",
            status.as_str()
        ));
    }
    match fields::blocked_on(finding) {
        Some(BlockedOn::Evidence) => {}
        Some(other) => reasons.push(format!(
            "{id} is blocked on {}; settle answers a finding blocked on evidence",
            other.as_str()
        )),
        None => reasons.push(format!(
            "{id} is not blocked; settle answers a finding blocked on evidence"
        )),
    }
    if let Outcome::Defer { .. } = outcome
        && let Some(existing) = text(finding, "proposal")
    {
        reasons.push(format!(
            "{id} already carries proposal {existing}; a deferral is not renewed — accept or refute it"
        ));
    }
    reasons
}

/// Records `outcome` on finding `id` and returns `{id, from, status}`.
fn settle(
    store: &mut Store,
    id: &str,
    outcome: &Outcome,
    now: Datetime,
    slug: Option<&str>,
) -> Result<JsonValue> {
    let index = store
        .findings
        .iter()
        .position(|f| f.id == id)
        .ok_or_else(|| invalid(format!("{VERB}: {id} is not in this store")))?;
    let from = store.findings[index].status();
    if recorded(store, &store.findings[index], outcome) {
        return Ok(json!({"id": id, "from": from.as_str(), "status": from.as_str()}));
    }
    let reasons = refusals(&store.findings[index], outcome);
    if !reasons.is_empty() {
        return Err(refused(id, reasons));
    }

    let finding = &mut store.findings[index];
    finding.set_status(FindingStatus::Open, &[]);
    finding.rest.remove("apply_flow");
    finding.rest.remove("apply_task");
    match outcome {
        Outcome::Accept { tier, fix } => {
            fields::clear_block(finding, &[]);
            let put = |f: &mut Finding, key: &str, value: &str| {
                f.rest
                    .insert(key.to_string(), TomlValue::String(value.to_string()));
            };
            put(finding, "fix_sketch", fix);
            put(finding, "settle_tier", tier);
            put(finding, "unblocked_by", "settle");
            // The proposal is taken up, so it no longer ages.
            finding.rest.remove("proposal");
            finding.rest.remove("proposal_at");
        }
        Outcome::Refute { citation } => {
            verdict::orchestrator_verdict(store, id, "refuted", citation, REFUTE_RATIONALE, slug)
                .map_err(|reasons| refused(id, reasons))?;
        }
        Outcome::Defer { proposal } => {
            finding
                .rest
                .insert("proposal".into(), TomlValue::String(proposal.clone()));
            finding
                .rest
                .insert("proposal_at".into(), TomlValue::Datetime(now));
        }
    }
    let status = store.findings[index].status();
    Ok(json!({"id": id, "from": from.as_str(), "status": status.as_str()}))
}

fn refused(id: &str, reasons: Vec<String>) -> anyhow::Error {
    let refused = json!({"id": id, "reasons": reasons});
    invalid(format!(
        "{VERB}: {id} was refused; nothing was written:\n{refused}"
    ))
}

/// A deferred proposal and its age in runs.
pub(super) struct Deferred<'s> {
    pub(super) finding: &'s Finding,
    pub(super) age: usize,
}

impl Deferred<'_> {
    pub(super) fn overdue(&self) -> bool {
        self.age >= OVERDUE_AGE
    }
}

/// The open findings of `family` carrying a deferred proposal, in store
/// order, each with the number of the family's runs started after its
/// `proposal_at`. A `proposal_at` that is missing or unreadable counts every
/// run of the family, so a damaged row is pressed for a disposition rather
/// than left to age forever.
pub(super) fn deferred(store: &Store, family: Family) -> Vec<Deferred<'_>> {
    store
        .findings
        .iter()
        .filter(|f| {
            f.family == family
                && f.status() == FindingStatus::Open
                && text(f, "proposal").is_some_and(|p| !p.trim().is_empty())
        })
        .map(|finding| {
            let at = proposal_at(finding);
            let age = store
                .runs
                .iter()
                .filter(|r| r.family == family)
                .filter(|r| match at {
                    Some(at) => r.started.parse::<Timestamp>().is_ok_and(|s| s > at),
                    None => true,
                })
                .count();
            Deferred { finding, age }
        })
        .collect()
}

fn proposal_at(finding: &Finding) -> Option<Timestamp> {
    let raw = match finding.rest.get("proposal_at")? {
        TomlValue::Datetime(at) => at.to_string(),
        TomlValue::String(at) => at.clone(),
        _ => return None,
    };
    raw.parse().ok()
}
