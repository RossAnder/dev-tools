//! `review reproduce` and `review reproduce-status` — reproduce verdicts on
//! findings whose fix an apply flow ran, kept apart from `[[verdicts]]`.
//!
//! A reproduction is upserted on (finding, flow, milestone, round, by): a
//! changed row replaces its predecessor at the end of the array, where the
//! finding's current reproduction is read from, and an identical replay keeps
//! its place and writes nothing, so replaying an earlier round never makes it
//! current again. No write here touches a finding's status or its verdicts.
//! A batch row that fails a check is refused with its reasons while the rest
//! land.

use anyhow::Result;
use serde_json::{Map, Value as JsonValue, json};

use super::fields::{ReproduceBy, ReproduceVerdict};
use super::invalid;
use super::store::{self, Reproduction, Store, Target};
use crate::cli::{ReadIntegrityArgs, WriteIntegrityArgs};
use crate::errors::{ErrorKind, tagged_err};
use crate::io::read_ndjson_source;
use crate::output::{print_json_compact, print_json_line};

const RECORD: &str = "review reproduce";
const STATUS: &str = "review reproduce-status";

const KEYS: &[&str] = &[
    "finding",
    "flow",
    "milestone",
    "round",
    "task",
    "verdict",
    "by",
    "citation",
];

struct Applied {
    returns: Vec<JsonValue>,
    refused: Vec<JsonValue>,
}

pub(crate) fn record(target: &Target, src: &str, integrity: &WriteIntegrityArgs) -> Result<()> {
    let rows = crate::items::parse_ndjson_objects(&read_ndjson_source(src)?)?;
    if rows.is_empty() {
        return Err(invalid(format!(
            "{RECORD}: the batch holds no reproductions"
        )));
    }
    let read = ReadIntegrityArgs {
        verify_integrity: integrity.verify_integrity,
        strict_read: true,
    };
    let today = crate::time::today_toml_date()?;

    let current = store::load(&target.path, &read)?;
    let mut trial = current.clone();
    let preview = apply(&mut trial, &rows, today);
    refuse_if_nothing_landed(&preview)?;
    let unchanged = trial == current;
    let applied = if unchanged {
        preview
    } else {
        store::mutate(&target.path, integrity, |store| {
            let applied = apply(store, &rows, today);
            refuse_if_nothing_landed(&applied)?;
            Ok(applied)
        })?
    };
    print_json_compact(&json!({
        "ok": true,
        "reproductions": applied.returns,
        "refused": applied.refused,
        "unchanged": unchanged,
    }))
}

fn refuse_if_nothing_landed(applied: &Applied) -> Result<()> {
    if !applied.returns.is_empty() {
        return Ok(());
    }
    let lines: Vec<String> = applied.refused.iter().map(JsonValue::to_string).collect();
    Err(invalid(format!(
        "{RECORD}: every reproduction row was refused; nothing was written:\n{}",
        lines.join("\n")
    )))
}

fn apply(
    store: &mut Store,
    rows: &[(usize, Map<String, JsonValue>)],
    today: toml::value::Datetime,
) -> Applied {
    let mut applied = Applied {
        returns: Vec::new(),
        refused: Vec::new(),
    };
    for (line, row) in rows {
        match parse_row(store, row, today) {
            Ok(record) => {
                applied.returns.push(json!({
                    "finding": record.finding,
                    "flow": record.flow,
                    "milestone": record.milestone,
                    "round": record.round,
                    "by": record.by.as_str(),
                    "verdict": record.verdict.as_str(),
                }));
                upsert(store, record);
            }
            Err(reasons) => applied.refused.push(json!({
                "line": line,
                "finding": row.get("finding").cloned().unwrap_or(JsonValue::Null),
                "reasons": reasons,
            })),
        }
    }
    applied
}

fn parse_row(
    store: &Store,
    row: &Map<String, JsonValue>,
    today: toml::value::Datetime,
) -> std::result::Result<Reproduction, Vec<String>> {
    let mut reasons = Vec::new();
    for key in row.keys().filter(|k| !KEYS.contains(&k.as_str())) {
        reasons.push(format!(
            "unknown key `{key}`; a reproduction carries {}",
            KEYS.join(", ")
        ));
    }
    let mut text = |key: &str, needed: bool| -> String {
        match row.get(key) {
            Some(JsonValue::String(s)) if !s.trim().is_empty() => s.trim().to_string(),
            None | Some(JsonValue::Null) if !needed => String::new(),
            Some(JsonValue::String(_)) | None | Some(JsonValue::Null) => {
                reasons.push(format!("`{key}` is required"));
                String::new()
            }
            Some(_) => {
                reasons.push(format!("`{key}` must be a string"));
                String::new()
            }
        }
    };
    let finding = text("finding", true);
    let flow = text("flow", true);
    let milestone = text("milestone", true);
    let task = text("task", false);
    let citation = text("citation", true);
    let verdict_text = text("verdict", true);
    let by_text = text("by", true);

    let verdict = ReproduceVerdict::parse(&verdict_text);
    if verdict.is_none() && !verdict_text.is_empty() {
        reasons.push(format!(
            "`verdict` {verdict_text:?} is not one of {}",
            ReproduceVerdict::VALUES.join("|")
        ));
    }
    let by = ReproduceBy::parse(&by_text);
    if by.is_none() && !by_text.is_empty() {
        reasons.push(format!(
            "`by` {by_text:?} is not one of {}",
            ReproduceBy::VALUES.join("|")
        ));
    }
    let round = match row.get("round") {
        Some(value) => match value.as_u64().and_then(|n| u32::try_from(n).ok()) {
            Some(n) if n >= 1 => Some(n),
            _ => {
                reasons.push(format!("`round` {value} is not a positive integer"));
                None
            }
        },
        None => {
            reasons.push("`round` is required".to_string());
            None
        }
    };
    if !finding.is_empty() && !store.findings.iter().any(|f| f.id == finding) {
        reasons.push(format!("`finding` {finding:?} is not in this store"));
    }

    match (verdict, by, round) {
        (Some(verdict), Some(by), Some(round)) if reasons.is_empty() => Ok(Reproduction {
            finding,
            flow,
            milestone,
            round,
            task,
            verdict,
            by,
            citation,
            at: Some(today),
            rest: toml::map::Map::new(),
        }),
        _ => Err(reasons),
    }
}

/// `at` is left out of the comparison, so a replay on a later day is still
/// identical and keeps the earlier row.
fn upsert(store: &mut Store, record: Reproduction) {
    let key = |r: &Reproduction| {
        (
            r.finding.clone(),
            r.flow.clone(),
            r.milestone.clone(),
            r.round,
            r.by,
        )
    };
    let same = |r: &Reproduction| {
        r.task == record.task
            && r.verdict == record.verdict
            && r.citation == record.citation
            && r.rest == record.rest
    };
    match store
        .reproductions
        .iter()
        .position(|r| key(r) == key(&record))
    {
        Some(at) if same(&store.reproductions[at]) => {}
        Some(at) => {
            store.reproductions.remove(at);
            store.reproductions.push(record);
        }
        None => store.reproductions.push(record),
    }
}

pub(crate) fn status(
    target: &Target,
    ids: &[String],
    expect: &str,
    integrity: &ReadIntegrityArgs,
) -> Result<()> {
    let Some(expect) = ReproduceVerdict::parse(expect.trim()) else {
        return Err(invalid(format!(
            "{STATUS}: --expect {expect:?} is not one of {}",
            ReproduceVerdict::VALUES.join("|")
        )));
    };
    let mut wanted: Vec<&str> = Vec::new();
    for id in ids.iter().map(|i| i.trim()).filter(|i| !i.is_empty()) {
        if !wanted.contains(&id) {
            wanted.push(id);
        }
    }
    if wanted.is_empty() {
        return Err(invalid(format!("{STATUS}: no finding ids given")));
    }

    let stored = store::load(&target.path, integrity)?;
    let mut findings = Vec::new();
    let mut ok = true;
    for id in wanted {
        if !stored.findings.iter().any(|f| f.id == id) {
            return Err(tagged_err(
                ErrorKind::NotFound,
                Some(target.path.clone()),
                format!("{STATUS}: no finding {id} in this store"),
            ));
        }
        let current = stored.reproductions.iter().rfind(|r| r.finding == id);
        let matches = current.is_some_and(|r| r.verdict == expect);
        ok &= matches;
        findings.push(match current {
            Some(r) => json!({
                "id": id,
                "matches": matches,
                "verdict": r.verdict.as_str(),
                "flow": r.flow,
                "milestone": r.milestone,
                "round": r.round,
                "by": r.by.as_str(),
            }),
            None => json!({"id": id, "matches": false, "verdict": JsonValue::Null}),
        });
    }
    print_json_line(&json!({
        "ok": ok,
        "expect": expect.as_str(),
        "findings": findings,
    }))?;
    if !ok {
        // stdout is flushed by the print above, so skipping destructors loses nothing.
        std::process::exit(1);
    }
    Ok(())
}
