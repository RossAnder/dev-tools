//! `review verdict` — verdicts upserted on (finding, `by`, `seat`).
//!
//! The store's verdicts carry no ids, so a replayed agent finds its own
//! verdict by key and replaces it; an identical resubmission writes nothing.
//! A finding with no `same_as` takes its status from the verdict at once;
//! one with `same_as` stays `unverified` for `review resolve`. A batch row
//! that fails a check is refused with its reasons while the rest land.
//! A verifier verdict with `unblocks` clears a blocked finding's block and
//! leaves its `fix_sketch` on the finding.

use std::collections::BTreeMap;

use anyhow::Result;
use serde_json::{Map, Value as JsonValue, json};

use super::catalogue::run_catalogue;
use super::fields::{self, FindingStatus, Kind, Record, Route, RowContext, SWEEPER};
use super::invalid;
use super::store::{self, Finding, Store, Target, Verdict};
use crate::cli::{ReadIntegrityArgs, ReviewVerdictArgs};
use crate::io::{read_flag_file, read_ndjson_source, read_text_arg, repo_or_cwd_root};
use crate::output::print_json_compact;

const VERB: &str = "review verdict";

/// One caller-supplied verdict: its NDJSON line (1 for the single form),
/// the row as given, and whether it strikes the finding's `same_as`.
struct Input {
    line: usize,
    row: Map<String, JsonValue>,
    strike_same_as: bool,
}

/// What applying a set of verdicts to a store produced.
struct Applied {
    returns: Vec<JsonValue>,
    refused: Vec<JsonValue>,
}

pub(crate) fn run(target: &Target, args: ReviewVerdictArgs) -> Result<()> {
    let batch = args.ndjson.is_some();
    let inputs = match &args.ndjson {
        Some(src) => crate::items::parse_ndjson_objects(&read_ndjson_source(src)?)?
            .into_iter()
            .map(|(line, row)| Input {
                line,
                row,
                strike_same_as: false,
            })
            .collect(),
        None => vec![single(&args)?],
    };
    if inputs.is_empty() {
        return Err(invalid(format!("{VERB}: the batch holds no verdicts")));
    }
    let read = ReadIntegrityArgs {
        verify_integrity: args.integrity.verify_integrity,
        strict_read: true,
    };

    // A replay of a call that landed finds its own verdicts already in place
    // and writes nothing.
    let current = store::load(&target.path, &read)?;
    let mechanical = mechanical_by_run(&current, &inputs)?;
    let mut trial = current.clone();
    let slug = super::add::store_slug(target);
    let preview = apply(&mut trial, &inputs, slug.as_deref(), &mechanical);
    refuse_if_nothing_landed(&preview, batch)?;
    let unchanged = trial == current;
    let applied = if unchanged {
        preview
    } else {
        store::mutate(&target.path, &args.integrity, |store| {
            let applied = apply(store, &inputs, slug.as_deref(), &mechanical);
            refuse_if_nothing_landed(&applied, batch)?;
            Ok(applied)
        })?
    };
    print_json_compact(&json!({
        "ok": true,
        "verdicts": applied.returns,
        "refused": applied.refused,
        "unchanged": unchanged,
    }))
}

/// The single-verdict form's flags as the row a batch line would carry.
fn single(args: &ReviewVerdictArgs) -> Result<Input> {
    if args.rationale.as_deref() == Some("-") && args.fix_sketch.as_deref() == Some("-") {
        return Err(invalid(format!(
            "{VERB}: --rationale - and --fix-sketch - both read stdin, which holds one value; \
             pass one of them inline or through --rationale-file / --fix-sketch-file"
        )));
    }
    let mut row = Map::new();
    let mut put = |key: &str, value: Option<JsonValue>| {
        if let Some(value) = value {
            row.insert(key.to_string(), value);
        }
    };
    put("finding", args.finding.clone().map(JsonValue::from));
    put("verdict", args.verdict.clone().map(JsonValue::from));
    put("by", args.by.clone().map(JsonValue::from));
    put("seat", args.seat.map(JsonValue::from));
    put("severity", args.severity.clone().map(JsonValue::from));
    put("citation", args.citation.clone().map(JsonValue::from));
    let rationale = match (&args.rationale, &args.rationale_file) {
        (Some(text), _) => Some(read_text_arg(text)?),
        (None, Some(path)) => Some(read_flag_file(path, "rationale-file")?),
        (None, None) => None,
    };
    put(
        "rationale",
        rationale.map(|text| JsonValue::from(text.replace("\r\n", "\n").trim())),
    );
    put("unblocks", args.unblocks.then_some(JsonValue::Bool(true)));
    let sketch = match (&args.fix_sketch, &args.fix_sketch_file) {
        (Some(text), _) => Some(read_text_arg(text)?),
        (None, Some(path)) => Some(read_flag_file(path, "fix-sketch-file")?),
        (None, None) => None,
    };
    put(
        "fix_sketch",
        sketch.map(|text| JsonValue::from(text.replace("\r\n", "\n").trim())),
    );
    Ok(Input {
        line: 1,
        row,
        strike_same_as: args.strike_same_as,
    })
}

/// A single verdict that was refused, or a batch whose every row was, is an
/// error; a partly refused batch lands the rest.
fn refuse_if_nothing_landed(applied: &Applied, batch: bool) -> Result<()> {
    if applied.refused.is_empty() || (batch && !applied.returns.is_empty()) {
        return Ok(());
    }
    let lines: Vec<String> = applied.refused.iter().map(JsonValue::to_string).collect();
    let what = if batch {
        "every verdict row was refused; nothing was written"
    } else {
        "the verdict was refused"
    };
    Err(invalid(format!("{VERB}: {what}:\n{}", lines.join("\n"))))
}

/// Upserts each input in order, refusing those that fail a check, and
/// returns the verdict-return object of each that landed.
fn apply(
    store: &mut Store,
    inputs: &[Input],
    slug: Option<&str>,
    mechanical: &Mechanical,
) -> Applied {
    let mut applied = Applied {
        returns: Vec::new(),
        refused: Vec::new(),
    };
    for input in inputs {
        match apply_one(store, input, slug, mechanical) {
            Ok(entry) => applied.returns.push(entry),
            Err(reasons) => {
                let finding = input.row.get("finding").cloned().unwrap_or(JsonValue::Null);
                applied.refused.push(json!({
                    "line": input.line,
                    "finding": finding,
                    "reasons": reasons,
                }));
            }
        }
    }
    applied
}

fn apply_one(
    store: &mut Store,
    input: &Input,
    slug: Option<&str>,
    mechanical: &Mechanical,
) -> std::result::Result<JsonValue, Vec<String>> {
    let row = &input.row;
    let text = |key: &str| row.get(key).and_then(JsonValue::as_str).unwrap_or_default();
    let id = text("finding");
    let index = store.findings.iter().position(|f| f.id == id);
    let family = index.map_or(fields::Family::Code, |i| store.findings[i].family);
    let no_paths = |_: &str| false;
    let ctx = RowContext {
        family,
        target_exists: &no_paths,
    };
    let mut reasons: Vec<String> = match fields::validate_row(Record::Verdict, row, &ctx) {
        Ok(()) => Vec::new(),
        Err(refusals) => refusals.iter().map(ToString::to_string).collect(),
    };
    let Some(index) = index else {
        if !id.is_empty() {
            reasons.push(format!("`finding` {id:?} is not in this store"));
        }
        return Err(reasons);
    };
    let finding = &store.findings[index];
    let (by, verdict) = (text("by"), text("verdict"));
    let seat = row
        .get("seat")
        .and_then(JsonValue::as_u64)
        .and_then(|n| u32::try_from(n).ok());

    match finding.kind {
        Kind::Advisory => reasons.push(format!(
            "{id} is advisory; advisory findings are triaged, not verified"
        )),
        _ if finding.status() == FindingStatus::Superseded => reasons.push(format!(
            "{id} is superseded by a later `review add` of cell {}",
            finding.cell
        )),
        _ => {}
    }
    match (by == "panel", seat) {
        (true, None) => reasons.push("`seat` is required for a panel verdict".to_string()),
        (false, Some(_)) => reasons.push("`seat` applies only to a panel verdict".to_string()),
        _ => {}
    }
    // A finding settles under its route's rule alone, so a verdict that rule
    // would never read is refused rather than recorded to no effect.
    let panel_routed = finding.route == Some(Route::Panel);
    match by {
        "panel" if !panel_routed => reasons.push(format!(
            "{id} is not routed to the panel; a panel verdict needs a panel-routed finding"
        )),
        "panel" | "audit" | "" => {}
        _ if panel_routed => reasons.push(format!(
            "{id} is routed to the panel; only panel seats and the audit give its verdict, not {by:?}"
        )),
        _ => {}
    }
    if row.contains_key("severity") && finding.kind != Kind::Defect {
        reasons.push(format!(
            "`severity` reassesses a defect; {id} is {}",
            finding.kind.as_str()
        ));
    }
    if by == "checker" {
        match verdict {
            "plausible" => {
                reasons.push("a checker returns confirmed, unsettled or refuted".to_string());
            }
            "refuted" => match checker_may_refute(store, finding, mechanical) {
                Ok(true) => {}
                Ok(false) => reasons.push(format!(
                    "a checker may refute only a row a sweeper wrote or a cleanup under a mechanical lens; {id} is a {} under lens {:?} from cell {}, which is not a {SWEEPER} cell",
                    finding.kind.as_str(),
                    finding.lens,
                    finding.cell
                )),
                Err(err) => reasons.push(format!(
                    "run {}'s catalogue, which says whether lens {:?} is mechanical, could not be read: {err}",
                    finding.run, finding.lens
                )),
            },
            _ => {}
        }
    }
    let unblocks = fields::unblocks(row);
    let mut rest = toml::map::Map::new();
    if unblocks {
        rest.insert("unblocks".into(), toml::Value::Boolean(true));
        rest.insert(
            "fix_sketch".into(),
            toml::Value::String(text("fix_sketch").to_string()),
        );
    }
    let record = Verdict {
        finding: id.to_string(),
        run: finding.run.clone(),
        by: by.to_string(),
        seat,
        verdict: verdict.to_string(),
        severity: text("severity").to_string(),
        citation: text("citation").to_string(),
        rationale: text("rationale").to_string(),
        rest,
    };
    if unblocks {
        if by != "verifier" {
            reasons.push(format!(
                "`unblocks` answers a blocked finding from the verifier, not {by:?}"
            ));
        }
        if verdict == "refuted" {
            reasons.push(
                "a refuted finding has no block to answer; `unblocks` needs a standing verdict"
                    .to_string(),
            );
        }
        // A replay finds the block it cleared already gone.
        if fields::blocked_on(finding).is_none() && !store.verdicts.contains(&record) {
            reasons.push(format!(
                "{id} is not blocked; `unblocks` answers a finding with `blocked_on`"
            ));
        }
    }
    if !reasons.is_empty() {
        return Err(reasons);
    }

    store.upsert_verdict(record);

    let reroute = verdict == "unsettled";
    let decided = decide(store, index, by, verdict);
    let finding = &mut store.findings[index];
    if input.strike_same_as {
        finding.same_as.clear();
    }
    if reroute {
        finding.route = Some(Route::Verifier);
    }
    if unblocks {
        if let Some(block) = fields::blocked_on(finding) {
            finding.rest.insert(
                "unblocked_from".into(),
                toml::Value::String(block.as_str().into()),
            );
        }
        fields::clear_block(finding, &[]);
        let sketch = toml::Value::String(text("fix_sketch").to_string());
        finding.rest.insert("fix_sketch".into(), sketch);
        finding.rest.insert(
            "unblocked_by".into(),
            toml::Value::String("verifier".into()),
        );
    }
    if finding.same_as.is_empty() && finding.status().decided_by_verdict() {
        finding.set_status(decided, &[]);
        if decided == FindingStatus::Open
            && let Some(slug) = slug
        {
            let mine: Vec<&Verdict> = store.verdicts.iter().filter(|v| v.finding == id).collect();
            // A target that does not parse leaves the tier absent; the verdict still lands.
            let _ = super::tier::stamp(&mut store.findings[index], slug, &mine);
        }
    }

    let mut entry = json!({"finding": id, "verdict": verdict});
    if reroute {
        entry["reroute"] = json!(Route::Verifier.as_str());
    }
    if unblocks {
        entry["unblocks"] = json!(true);
        entry["fix_sketch"] = json!(text("fix_sketch"));
    }
    Ok(entry)
}

/// Records one `orchestrator` verdict on `finding` through the checks and
/// status decision `review verdict` applies; `Err` carries the refusals.
pub(super) fn orchestrator_verdict(
    store: &mut Store,
    finding: &str,
    verdict: &str,
    citation: &str,
    rationale: &str,
    slug: Option<&str>,
) -> std::result::Result<JsonValue, Vec<String>> {
    let row = Map::from_iter([
        ("finding".to_string(), json!(finding)),
        ("by".to_string(), json!("orchestrator")),
        ("verdict".to_string(), json!(verdict)),
        ("citation".to_string(), json!(citation)),
        ("rationale".to_string(), json!(rationale)),
    ]);
    let input = Input {
        line: 1,
        row,
        strike_same_as: false,
    };
    apply_one(store, &input, slug, &Mechanical::new())
}

fn written_by_sweeper(store: &Store, cell: &str) -> bool {
    store
        .cells
        .iter()
        .any(|c| c.id == cell && c.agent == SWEEPER)
}

/// Per run id, the ids of its catalogue's mechanical lenses for the run's
/// family, or why the catalogue could not be read.
type Mechanical = BTreeMap<String, std::result::Result<Vec<String>, String>>;

/// The mechanical lenses of each run that a checker's `refuted` on a cleanup
/// row outside a sweeper cell needs; no other verdict reads a catalogue.
fn mechanical_by_run(store: &Store, inputs: &[Input]) -> Result<Mechanical> {
    let mut mechanical = Mechanical::new();
    let mut root = None;
    for input in inputs {
        let text = |key: &str| input.row.get(key).and_then(JsonValue::as_str);
        if text("by") != Some("checker") || text("verdict") != Some("refuted") {
            continue;
        }
        let Some(finding) = store
            .findings
            .iter()
            .find(|f| Some(f.id.as_str()) == text("finding"))
        else {
            continue;
        };
        if finding.kind != Kind::Cleanup
            || written_by_sweeper(store, &finding.cell)
            || mechanical.contains_key(&finding.run)
        {
            continue;
        }
        let Some(run) = store.runs.iter().find(|r| r.id == finding.run) else {
            continue;
        };
        let root = match &root {
            Some(root) => root,
            None => root.insert(repo_or_cwd_root()?),
        };
        let lenses = run_catalogue(root, run)
            .map(|catalogue| catalogue.mechanical_lenses(run.family))
            .map_err(|err| format!("{err:#}"));
        mechanical.insert(run.id.clone(), lenses);
    }
    Ok(mechanical)
}

/// Whether a checker may refute `finding`: a sweeper cell wrote it, or it is
/// a cleanup under one of its run's mechanical lenses. `Err` carries the
/// catalogue failure that left the second question unanswered.
fn checker_may_refute(
    store: &Store,
    finding: &Finding,
    mechanical: &Mechanical,
) -> std::result::Result<bool, String> {
    if written_by_sweeper(store, &finding.cell) {
        return Ok(true);
    }
    if finding.kind != Kind::Cleanup {
        return Ok(false);
    }
    match mechanical.get(&finding.run) {
        Some(Ok(lenses)) => Ok(lenses.contains(&finding.lens)),
        Some(Err(err)) => Err(err.clone()),
        None => Ok(false),
    }
}

/// The status the verdict just written gives its finding. An audit verdict
/// decides at once on any route; a checker's `unsettled` leaves the finding
/// for the verifier; every other verdict decides through [`settle`].
fn decide(store: &Store, index: usize, by: &str, verdict: &str) -> FindingStatus {
    let finding = &store.findings[index];
    let settled = match (by, verdict) {
        ("audit", _) => Some(verdict),
        (_, "unsettled") => None,
        _ => {
            let mine: Vec<&Verdict> = store
                .verdicts
                .iter()
                .filter(|v| v.finding == finding.id)
                .collect();
            settle(finding.route, &mine, |_| true).map(|s| s.verdict)
        }
    };
    match settled {
        Some("refuted") => FindingStatus::Refuted,
        Some(_) => FindingStatus::Open,
        None => FindingStatus::Unverified,
    }
}

/// A finding's settled call — `confirmed`, `plausible` or `refuted` — and the
/// positions, in the verdicts it was settled from, of those that made it.
pub(crate) struct Settled<'v> {
    pub(crate) verdict: &'v str,
    pub(crate) deciding: Vec<usize>,
}

/// Settles a finding from its verdicts in store order, the one rule
/// `review verdict`, `review resolve` and `review stats` share. A panel-routed
/// finding settles on its panel seats alone, each by its latest vote, so
/// copied or repeated rows never make a second seat: two non-refuted seats
/// stand it (`confirmed` when two confirm, else `plausible`), two refuting
/// seats refute it. Any other route takes the latest non-panel verdict that
/// `counts` admits and that is not `unsettled`. `None` is undecided.
pub(crate) fn settle<'v>(
    route: Option<Route>,
    verdicts: &[&'v Verdict],
    counts: impl Fn(&Verdict) -> bool,
) -> Option<Settled<'v>> {
    if route == Some(Route::Panel) {
        let mut seats: Vec<(Option<u32>, usize)> = Vec::new();
        for (k, vote) in verdicts.iter().enumerate().filter(|(_, v)| v.by == "panel") {
            seats.retain(|(seat, _)| *seat != vote.seat);
            seats.push((vote.seat, k));
        }
        let deciding: Vec<usize> = seats.iter().map(|&(_, k)| k).collect();
        let count = |pred: &dyn Fn(&str) -> bool| {
            deciding
                .iter()
                .filter(|&&k| pred(&verdicts[k].verdict))
                .count()
        };
        let refuting = count(&|v| v == "refuted");
        let verdict = if deciding.len() - refuting >= 2 {
            if count(&|v| v == "confirmed") >= 2 {
                "confirmed"
            } else {
                "plausible"
            }
        } else if refuting >= 2 {
            "refuted"
        } else {
            return None;
        };
        return Some(Settled { verdict, deciding });
    }
    let (k, latest) = verdicts
        .iter()
        .enumerate()
        .rfind(|(_, v)| v.by != "panel" && v.verdict != "unsettled" && counts(v))?;
    Some(Settled {
        verdict: latest.verdict.as_str(),
        deciding: vec![k],
    })
}
