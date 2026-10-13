//! `review verdict` — verdicts upserted on (finding, `by`, `seat`).
//!
//! The store's verdicts carry no ids, so a replayed agent finds its own
//! verdict by key and replaces it; an identical resubmission writes nothing.
//! A finding with no `same_as` takes its status from the verdict at once;
//! one with `same_as` stays `unverified` for `review resolve`. A batch row
//! that fails a check is refused with its reasons while the rest land.

use anyhow::Result;
use serde_json::{Map, Value as JsonValue, json};

use super::fields::{self, Kind, Record, Route, RowContext, SWEEPER};
use super::store::{self, Store, Target, Verdict};
use crate::cli::{ReadIntegrityArgs, ReviewVerdictArgs};
use crate::errors::{ErrorKind, tagged_err};
use crate::io::{read_flag_file, read_ndjson_source, read_text_arg};
use crate::output::print_json_compact;

const VERB: &str = "review verdict";
/// The finding statuses a verdict decides; every other status is a later
/// disposition that a verdict is recorded beside but never overrides.
const DECIDED_BY_VERDICT: &[&str] = &["unverified", "open", "refuted"];

fn invalid(msg: impl Into<String>) -> anyhow::Error {
    tagged_err(ErrorKind::Validation, None, msg)
}

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

    // A retry after the sidecar rename failed finds its own verdicts already
    // in place and writes nothing.
    let current = store::load(&target.path, &read)?;
    let mut trial = current.clone();
    let preview = apply(&mut trial, &inputs);
    refuse_if_nothing_landed(&preview, batch)?;
    let unchanged = trial == current;
    let applied = if unchanged {
        preview
    } else {
        store::mutate(&target.path, &args.integrity, |store| {
            let applied = apply(store, &inputs);
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
fn apply(store: &mut Store, inputs: &[Input]) -> Applied {
    let mut applied = Applied {
        returns: Vec::new(),
        refused: Vec::new(),
    };
    for input in inputs {
        match apply_one(store, input) {
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

fn apply_one(store: &mut Store, input: &Input) -> std::result::Result<JsonValue, Vec<String>> {
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
        _ if finding.status == "superseded" => reasons.push(format!(
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
            "refuted" if !written_by_sweeper(store, &finding.cell) => reasons.push(format!(
                "a checker may refute only a row a sweeper wrote; {id} is from cell {}, which is not a {SWEEPER} cell",
                finding.cell
            )),
            _ => {}
        }
    }
    if !reasons.is_empty() {
        return Err(reasons);
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
        rest: toml::map::Map::new(),
    };
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
    if finding.same_as.is_empty() && DECIDED_BY_VERDICT.contains(&finding.status.as_str()) {
        finding.status = decided.to_string();
    }

    let mut entry = json!({"finding": id, "verdict": verdict});
    if reroute {
        entry["reroute"] = json!(Route::Verifier.as_str());
    }
    Ok(entry)
}

fn written_by_sweeper(store: &Store, cell: &str) -> bool {
    store
        .cells
        .iter()
        .any(|c| c.id == cell && c.agent == SWEEPER)
}

/// The status the verdict just written gives its finding. An audit verdict
/// decides at once on any route; a checker's `unsettled` leaves the finding
/// for the verifier; every other verdict decides through [`settle`].
fn decide(store: &Store, index: usize, by: &str, verdict: &str) -> &'static str {
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
        Some("refuted") => "refuted",
        Some(_) => "open",
        None => "unverified",
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
