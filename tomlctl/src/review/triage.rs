//! `review triage` — finding dispositions with a compare-and-set on status.
//!
//! Each call applies one disposition to its ids. Under `--expect-status`, an
//! id no longer at that status lands under `skipped_stale` instead of moving;
//! the check runs inside the locked write. A row already in the requested end
//! state is reported `unchanged`, so a replayed call writes nothing. A move
//! clears every disposition companion before recording its own. `--convert`
//! mints the advisory's replacement through the field catalogue and routing,
//! records an `orchestrator` verdict on it and marks the advisory
//! `converted` in one write; the replacement stays `unverified` for its route.
//! An advisory whose run is finished converts only into the open run `--run`
//! names, which then cannot finish until the replacement is verified.

use std::path::{Path, PathBuf};

use anyhow::Result;
use serde_json::{Map, Value as JsonValue, json};
use toml::Value as TomlValue;
use toml::value::Datetime;

use super::add::{self, str_of};
use super::catalogue::run_catalogue;
use super::fields::{self, FindingStatus, Kind, Route};
use super::invalid;
use super::matching::{self, Site};
use super::store::{self, Finding, Minted, Run, Store, Target, Verdict};
use crate::cli::{ReadIntegrityArgs, ReviewTriageArgs};
use crate::io::{read_ndjson_source, read_prose, repo_or_cwd_root};
use crate::output::print_json_compact;

const VERB: &str = "review triage";
/// The reopenable statuses in the order the `--reopen` refusal names them;
/// a unit test holds it to [`FindingStatus::is_reopenable`].
const REOPENABLE_ORDER: &[FindingStatus] = &[
    FindingStatus::Dismissed,
    FindingStatus::Deferred,
    FindingStatus::Parked,
    FindingStatus::Backlogged,
    FindingStatus::Refuted,
    FindingStatus::Resolved,
];

enum Mode {
    Dismiss(String),
    Defer { reason: String, trigger: String },
    Reopen(String),
    Park(String),
    Resolve(String),
    Backlog(String),
    Convert(Kind),
}

impl Mode {
    fn name(&self) -> &'static str {
        match self {
            Self::Dismiss(_) => "dismiss",
            Self::Defer { .. } => "defer",
            Self::Reopen(_) => "reopen",
            Self::Park(_) => "park",
            Self::Resolve(_) => "resolve",
            Self::Backlog(_) => "backlog-ref",
            Self::Convert(_) => "convert",
        }
    }

    fn status(&self) -> FindingStatus {
        match self {
            Self::Dismiss(_) => FindingStatus::Dismissed,
            Self::Defer { .. } => FindingStatus::Deferred,
            Self::Reopen(_) => FindingStatus::Open,
            Self::Park(_) => FindingStatus::Parked,
            Self::Resolve(_) => FindingStatus::Resolved,
            Self::Backlog(_) => FindingStatus::Backlogged,
            Self::Convert(_) => FindingStatus::Converted,
        }
    }

    /// The text companions this disposition records. `resolved` also stamps
    /// the date, which a replay on a later day does not count as a change.
    fn companions(&self) -> Vec<(&'static str, &str)> {
        match self {
            Self::Dismiss(r) => vec![("dismiss_reason", r)],
            Self::Defer { reason, trigger } => {
                vec![("defer_reason", reason), ("defer_trigger", trigger)]
            }
            Self::Reopen(r) => vec![("reopen_reason", r)],
            Self::Park(r) => vec![("park_reason", r)],
            Self::Resolve(r) => vec![("resolution", r)],
            Self::Backlog(id) => vec![("backlog_ref", id)],
            Self::Convert(_) => Vec::new(),
        }
    }

    fn advisory_only(&self) -> bool {
        matches!(self, Self::Park(_) | Self::Convert(_))
    }
}

/// What one id's disposition comes to against the store as it stands.
enum Step {
    Apply,
    Unchanged,
    Stale(String),
    Refused(Vec<String>),
}

#[derive(Default)]
struct Outcome {
    applied: Vec<JsonValue>,
    unchanged: Vec<String>,
    stale: Vec<JsonValue>,
    refused: Vec<JsonValue>,
    minted: Vec<JsonValue>,
}

impl Outcome {
    fn landed_nothing(&self) -> bool {
        self.applied.is_empty() && self.unchanged.is_empty() && self.stale.is_empty()
    }

    fn print(&self, mode: &Mode) -> Result<()> {
        let mut out = json!({
            "ok": true,
            "mode": mode.name(),
            "applied": self.applied,
            "unchanged": self.unchanged,
            "skipped_stale": self.stale,
            "refused": self.refused,
        });
        if matches!(mode, Mode::Convert(_)) {
            out["minted"] = json!(self.minted);
        }
        print_json_compact(&out)
    }
}

pub(crate) fn run(target: &Target, args: ReviewTriageArgs) -> Result<()> {
    let mode = read_mode(&args)?;
    let expect = args.expect_status.as_deref();
    if let Some(status) = expect
        && !fields::FINDING_STATUSES.contains(&status)
    {
        return Err(invalid(format!(
            "{VERB}: --expect-status is {status:?}; expected one of {}",
            fields::FINDING_STATUSES.join(", ")
        )));
    }
    let mut ids: Vec<String> = Vec::new();
    for id in args.ids.iter().map(|i| i.trim()).filter(|i| !i.is_empty()) {
        if !ids.iter().any(|seen| seen == id) {
            ids.push(id.to_string());
        }
    }
    if ids.is_empty() {
        return Err(invalid(format!("{VERB}: no finding ids given")));
    }
    let read = ReadIntegrityArgs {
        verify_integrity: args.integrity.verify_integrity,
        strict_read: true,
    };
    let today = crate::time::today_toml_date()?;
    let current = store::load(&target.path, &read)?;

    if let Mode::Convert(kind) = mode {
        let [id] = ids.as_slice() else {
            return Err(invalid(format!(
                "{VERB}: --convert takes exactly one advisory id; got {}",
                ids.join(", ")
            )));
        };
        let src = args.ndjson.as_deref().ok_or_else(|| {
            invalid(format!(
                "{VERB}: --convert needs the replacement as --ndjson"
            ))
        })?;
        let outcome = convert(target, &args, &current, id, kind, src, expect, today)?;
        return outcome.print(&mode);
    }

    // A replay of a call that landed finds its moves already made and writes
    // nothing. A failed write leaves the TOML as it was, since the sidecar
    // is committed before it.
    let mut trial = current.clone();
    let preview = dispose_all(&mut trial, &ids, &mode, expect, today);
    refuse_if_nothing_landed(&preview)?;
    let outcome = if trial == current {
        preview
    } else {
        store::mutate(&target.path, &args.integrity, |store| {
            let outcome = dispose_all(store, &ids, &mode, expect, today);
            refuse_if_nothing_landed(&outcome)?;
            Ok(outcome)
        })?
    };
    outcome.print(&mode)
}

/// The one disposition the mode group carries, its prose read and checked.
fn read_mode(args: &ReviewTriageArgs) -> Result<Mode> {
    let m = &args.mode;
    if let Some(reason) = prose("dismiss", &m.dismiss, &m.dismiss_file)? {
        return Ok(Mode::Dismiss(reason));
    }
    if let Some(reason) = prose("defer", &m.defer, &m.defer_file)? {
        let trigger = prose("trigger", &args.trigger, &args.trigger_file)?
            .ok_or_else(|| invalid(format!("{VERB}: --defer needs --trigger or --trigger-file")))?;
        return Ok(Mode::Defer { reason, trigger });
    }
    if let Some(reason) = prose("reopen", &m.reopen, &m.reopen_file)? {
        return Ok(Mode::Reopen(reason));
    }
    if let Some(reason) = prose("park", &m.park, &m.park_file)? {
        return Ok(Mode::Park(reason));
    }
    if let Some(resolution) = prose("resolve", &m.resolve, &m.resolve_file)? {
        return Ok(Mode::Resolve(resolution));
    }
    if let Some(reference) = &m.backlog_ref {
        let reference = reference.trim();
        let hex = reference.strip_prefix("B-").unwrap_or_default();
        if hex.is_empty() || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(invalid(format!(
                "{VERB}: --backlog-ref is {reference:?}; expected a backlog id such as B-1a2b3c4d"
            )));
        }
        return Ok(Mode::Backlog(reference.to_string()));
    }
    if let Some(kind) = &m.convert {
        return match Kind::parse(kind) {
            Some(kind @ (Kind::Cleanup | Kind::Defect)) => Ok(Mode::Convert(kind)),
            _ => Err(invalid(format!(
                "{VERB}: --convert is {kind:?}; expected cleanup or defect"
            ))),
        };
    }
    Err(invalid(format!("{VERB}: no disposition given")))
}

/// A prose flag or its `-file` twin; refused when it comes to nothing, since
/// a disposition without its reason teaches calibration nothing.
fn prose(flag: &str, literal: &Option<String>, file: &Option<PathBuf>) -> Result<Option<String>> {
    read_prose(VERB, flag, literal.as_deref(), file.as_deref())
}

/// A single id that was refused, or a call whose every id was, is an error;
/// otherwise the refusals are reported beside what landed.
fn refuse_if_nothing_landed(outcome: &Outcome) -> Result<()> {
    if outcome.refused.is_empty() || !outcome.landed_nothing() {
        return Ok(());
    }
    let lines: Vec<String> = outcome.refused.iter().map(JsonValue::to_string).collect();
    Err(invalid(format!(
        "{VERB}: every id was refused; nothing was written:\n{}",
        lines.join("\n")
    )))
}

/// Unchanged first, so a replay never reads as stale; then the
/// compare-and-set; then whether the move is one this row can make.
fn judge(store: &Store, id: &str, mode: &Mode, expect: Option<&str>) -> Step {
    let Some(finding) = store.findings.iter().find(|f| f.id == id) else {
        return Step::Refused(vec![format!("{id} is not in this store")]);
    };
    let current = finding.status();
    let status = current.as_str();
    let recorded = mode
        .companions()
        .into_iter()
        .all(|(key, value)| finding.rest.get(key).and_then(TomlValue::as_str) == Some(value));
    if current == mode.status() && recorded {
        return Step::Unchanged;
    }
    if let Some(expected) = expect
        && status != expected
    {
        return Step::Stale(status.to_string());
    }
    let mut reasons = Vec::new();
    if current.is_settled() {
        reasons.push(format!(
            "{id} is {status}; triage does not move a {status} finding"
        ));
    }
    if mode.advisory_only() && finding.kind != Kind::Advisory {
        reasons.push(format!(
            "--{} applies to advisory findings; {id} is {}",
            mode.name(),
            finding.kind.as_str()
        ));
    }
    if matches!(mode, Mode::Reopen(_)) && !current.is_settled() && !current.is_reopenable() {
        let reopenable: Vec<&str> = REOPENABLE_ORDER.iter().map(|s| s.as_str()).collect();
        reasons.push(format!(
            "{id} is {status}; --reopen moves a finding that is {}",
            reopenable.join(", ")
        ));
    }
    if reasons.is_empty() {
        Step::Apply
    } else {
        Step::Refused(reasons)
    }
}

/// Moves `finding` to `status` through [`Finding::set_status`], recording the
/// text `companions` and, for a resolve, the `resolved` date.
fn dispose(
    finding: &mut Finding,
    status: FindingStatus,
    companions: &[(&'static str, &str)],
    resolved: Option<Datetime>,
) {
    let mut values: Vec<(&str, TomlValue)> = companions
        .iter()
        .map(|(key, value)| (*key, TomlValue::String((*value).to_string())))
        .collect();
    if let Some(date) = resolved {
        values.push(("resolved", TomlValue::Datetime(date)));
    }
    finding.set_status(status, &values);
}

fn dispose_all(
    store: &mut Store,
    ids: &[String],
    mode: &Mode,
    expect: Option<&str>,
    today: Datetime,
) -> Outcome {
    let mut outcome = Outcome::default();
    for id in ids {
        match judge(store, id, mode, expect) {
            Step::Apply => {
                let finding = store
                    .findings
                    .iter_mut()
                    .find(|f| &f.id == id)
                    .expect("judge applies only to a stored finding");
                let from = finding.status().as_str();
                let resolved = matches!(mode, Mode::Resolve(_)).then_some(today);
                // A reopened finding no longer follows its v1 row: sync would
                // undo the reopen, and a re-export mints a regression instead.
                if matches!(mode, Mode::Reopen(_)) {
                    finding.retire_link();
                }
                dispose(finding, mode.status(), &mode.companions(), resolved);
                outcome
                    .applied
                    .push(json!({"id": id, "from": from, "status": mode.status().as_str()}));
            }
            Step::Unchanged => outcome.unchanged.push(id.clone()),
            Step::Stale(found) => outcome.stale.push(stale_entry(id, expect, &found)),
            Step::Refused(reasons) => outcome.refused.push(json!({"id": id, "reasons": reasons})),
        }
    }
    outcome
}

fn stale_entry(id: &str, expect: Option<&str>, found: &str) -> JsonValue {
    json!({"id": id, "field": "status", "expected": expect, "found": found})
}

/// `--convert`: the advisory's replacement is built and checked before the
/// lock; the compare-and-set, the mint, the `converted` move and the
/// `orchestrator` verdict happen under it in one write.
#[allow(clippy::too_many_arguments)]
fn convert(
    target: &Target,
    args: &ReviewTriageArgs,
    current: &Store,
    id: &str,
    kind: Kind,
    src: &str,
    expect: Option<&str>,
    today: Datetime,
) -> Result<Outcome> {
    let mode = Mode::Convert(kind);
    let mut outcome = Outcome::default();
    match judge(current, id, &mode, expect) {
        Step::Apply => {}
        Step::Unchanged => {
            outcome.unchanged.push(id.to_string());
            outcome.minted.extend(replacement_of(current, id));
            return Ok(outcome);
        }
        Step::Stale(found) => {
            outcome.stale.push(stale_entry(id, expect, &found));
            return Ok(outcome);
        }
        Step::Refused(reasons) => {
            outcome.refused.push(json!({"id": id, "reasons": reasons}));
            refuse_if_nothing_landed(&outcome)?;
            return Ok(outcome);
        }
    }
    let advisory = current
        .findings
        .iter()
        .find(|f| f.id == id)
        .expect("judge applies only to a stored finding");
    let named = args.run.as_deref();
    let run = target_run(current, advisory, named)?;
    let root = repo_or_cwd_root()?;
    let row = replacement_row(&root, current, advisory, run, kind, src)?;
    let run_id = run.id.clone();
    let today_iso = today.to_string();

    store::mutate_minting(
        &target.path,
        &args.integrity,
        |doc| {
            let locked = store::from_toml(doc)?;
            let step = judge(&locked, id, &mode, expect);
            if !matches!(step, Step::Apply) {
                return Ok((step, None));
            }
            let advisory = locked
                .findings
                .iter()
                .find(|f| f.id == id)
                .expect("judge applies only to a stored finding");
            target_run(&locked, advisory, named)?;
            let row = finish_row(row.clone(), advisory, &run_id, &locked.findings, &today_iso);
            let minted = store::mint(doc, Minted::Findings, std::slice::from_ref(&row))?;
            let new_id = minted.into_iter().next().expect("one row mints one id");
            Ok((Step::Apply, Some((new_id, row))))
        },
        |store, (step, minted)| {
            match step {
                Step::Apply => {}
                Step::Unchanged => {
                    outcome.unchanged.push(id.to_string());
                    outcome.minted.extend(replacement_of(store, id));
                    return Ok(outcome);
                }
                Step::Stale(found) => {
                    outcome.stale.push(stale_entry(id, expect, &found));
                    return Ok(outcome);
                }
                Step::Refused(reasons) => {
                    outcome.refused.push(json!({"id": id, "reasons": reasons}));
                    refuse_if_nothing_landed(&outcome)?;
                    return Ok(outcome);
                }
            }
            let (new_id, row) = minted.expect("an applied conversion minted its row");
            let advisory = store
                .findings
                .iter_mut()
                .find(|f| f.id == id)
                .expect("judge applies only to a stored finding");
            let from = advisory.status().as_str();
            dispose(
                advisory,
                FindingStatus::Converted,
                &[("converted_to", &new_id)],
                None,
            );
            let file = row["file"].as_str().unwrap_or_default();
            let citation = match row.get("line").and_then(JsonValue::as_u64) {
                Some(line) => format!("{file}:{line}"),
                None => file.to_string(),
            };
            store.verdicts.push(Verdict {
                finding: new_id.clone(),
                run: run_id.clone(),
                by: "orchestrator".to_string(),
                seat: None,
                verdict: "confirmed".to_string(),
                severity: String::new(),
                citation,
                rationale: format!("converted from advisory {id} at triage"),
                rest: toml::map::Map::new(),
            });
            outcome.applied.push(json!({
                "id": id,
                "from": from,
                "status": "converted",
                "converted_to": new_id,
            }));
            outcome.minted.push(json!({
                "id": new_id,
                "kind": row["kind"],
                "severity": row.get("severity").cloned().unwrap_or(JsonValue::Null),
                "route": row["route"],
                "status": row["status"],
            }));
            Ok(outcome)
        },
    )
}

/// The `{id, kind, severity, route, status}` of the finding an already
/// converted advisory names, for a replayed `--convert`.
fn replacement_of(store: &Store, advisory: &str) -> Option<JsonValue> {
    let converted_to = store
        .findings
        .iter()
        .find(|f| f.id == advisory)?
        .rest
        .get("converted_to")?
        .as_str()?;
    let f = store.findings.iter().find(|f| f.id == converted_to)?;
    Some(json!({
        "id": f.id,
        "kind": f.kind.as_str(),
        "severity": f.rest.get("severity").and_then(TomlValue::as_str),
        "route": f.route.unwrap_or(Route::Unrouted).as_str(),
        "status": f.status().as_str(),
    }))
}

/// The open run the replacement is minted in: the advisory's own while it is
/// open, else the `--run` named, which must be open and of the advisory's
/// family. A finished run cannot verify the replacement, so it never hosts one.
fn target_run<'a>(store: &'a Store, advisory: &Finding, named: Option<&str>) -> Result<&'a Run> {
    let own = store
        .runs
        .iter()
        .find(|r| r.id == advisory.run)
        .ok_or_else(|| {
            invalid(format!(
                "{VERB}: {} names run {}, which is not in this store",
                advisory.id, advisory.run
            ))
        })?;
    if own.finished.is_empty() {
        return match named {
            Some(named) if named != own.id => Err(invalid(format!(
                "{VERB}: --run is {named}, but {} belongs to run {}, which is open; its replacement is minted there",
                advisory.id, own.id
            ))),
            _ => Ok(own),
        };
    }
    let Some(named) = named else {
        return Err(invalid(format!(
            "{VERB}: {} belongs to run {}, which is finished; its replacement could not be verified there — name an open run with --run",
            advisory.id, own.id
        )));
    };
    let run = store
        .runs
        .iter()
        .find(|r| r.id == named)
        .ok_or_else(|| invalid(format!("{VERB}: --run {named} is not in this store")))?;
    if !run.finished.is_empty() {
        return Err(invalid(format!(
            "{VERB}: --run {named} is finished; the replacement needs an open run"
        )));
    }
    if run.family != advisory.family {
        return Err(invalid(format!(
            "{VERB}: --run {named} is a {} run; {} is a {} finding",
            run.family.as_str(),
            advisory.id,
            advisory.family.as_str()
        )));
    }
    Ok(run)
}

/// The replacement as given, defaulted from the advisory's lens, anchor and
/// summary, validated through the field catalogue and routed by its edit
/// target under `run`'s catalogue and depth. The fields that need the store
/// under the lock come later.
fn replacement_row(
    root: &Path,
    store: &Store,
    advisory: &Finding,
    run: &Run,
    kind: Kind,
    src: &str,
) -> Result<Map<String, JsonValue>> {
    let rows = crate::items::parse_ndjson_objects(&read_ndjson_source(src)?)?;
    let mut rows = rows.into_iter();
    let (Some((_, mut row)), None) = (rows.next(), rows.next()) else {
        return Err(invalid(format!(
            "{VERB}: --ndjson must hold exactly one replacement object"
        )));
    };

    let mut reasons = Vec::new();
    match row.get("kind").and_then(JsonValue::as_str) {
        None => {
            row.insert("kind".into(), json!(kind.as_str()));
        }
        Some(given) if given == kind.as_str() => {}
        Some(given) => reasons.push(format!(
            "`kind` is {given:?}; --convert asks for {}",
            kind.as_str()
        )),
    }
    if row.contains_key("same_as") {
        reasons.push(format!(
            "`same_as` cannot be supplied; the replacement is linked to {} through `related`",
            advisory.id
        ));
    }
    let defaults = [
        ("lens", json!(advisory.lens)),
        ("file", json!(advisory.file)),
        ("line", advisory.line.map_or(JsonValue::Null, |l| json!(l))),
        ("symbol", json!(advisory.symbol)),
        ("summary", json!(advisory.summary)),
    ];
    for (key, value) in defaults {
        let empty = value.is_null() || value.as_str().is_some_and(str::is_empty);
        if !empty && !row.contains_key(key) {
            row.insert(key.into(), value);
        }
    }
    let cell = store.cells.iter().find(|c| c.id == advisory.cell);
    reasons.extend(add::field_refusals(root, &row, advisory.family, cell));
    if reasons.is_empty() {
        reasons.extend(add::text_refusals(root, &row, kind, advisory.family));
    }
    if !reasons.is_empty() {
        let refused = json!({"id": advisory.id, "reasons": reasons});
        return Err(invalid(format!(
            "{VERB}: the replacement for {} was refused; nothing was written:\n{refused}",
            advisory.id
        )));
    }

    let globs = run_catalogue(root, run)?.routing().compile()?;
    let route = add::route_of(&row, kind, run, &globs);
    row.insert("route".into(), json!(route.as_str()));
    Ok(row)
}

/// Stamps the bookkeeping `review add` computes: the hosting `run`, the
/// advisory's cell as provenance, the anchor, and `related` naming the
/// advisory first, then the replacement's other anchor candidates.
fn finish_row(
    mut row: Map<String, JsonValue>,
    advisory: &Finding,
    run: &str,
    pool: &[Finding],
    today: &str,
) -> JsonValue {
    let site = Site {
        family: advisory.family,
        file: str_of(&row, "file").unwrap_or_default(),
        symbol: str_of(&row, "symbol").unwrap_or_default(),
        line: add::line_of(&row),
    };
    let anchor = matching::anchor(&site);
    let mut related = vec![advisory.id.clone()];
    for candidate in matching::candidates(pool, &site) {
        if !related.contains(&candidate.id) {
            related.push(candidate.id.clone());
        }
    }
    row.insert("run".into(), json!(run));
    row.insert("cell".into(), json!(advisory.cell));
    row.insert("seen_in".into(), json!([run]));
    row.insert("family".into(), json!(advisory.family.as_str()));
    row.insert("anchor".into(), json!(anchor));
    row.insert("related".into(), json!(related));
    row.insert("status".into(), json!(FindingStatus::Unverified.as_str()));
    row.insert("first_seen".into(), json!(today));
    JsonValue::Object(row)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reopen_order_names_exactly_the_reopenable_statuses() {
        let mut ordered: Vec<&str> = REOPENABLE_ORDER.iter().map(|s| s.as_str()).collect();
        let mut owned: Vec<&str> = FindingStatus::ALL
            .iter()
            .filter(|s| s.is_reopenable())
            .map(|s| s.as_str())
            .collect();
        ordered.sort_unstable();
        owned.sort_unstable();
        assert_eq!(ordered, owned);
    }
}
