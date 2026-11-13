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
use clap::Args;
use serde_json::{Map, Value as JsonValue, json};

use super::fields::{ReproduceBy, ReproduceVerdict};
use super::invalid;
use super::store::{self, REPRODUCTION_CORE, Reproduction, Store, Target};
use crate::cli::{ReadIntegrityArgs, ReviewStore, WriteIntegrityArgs};
use crate::errors::{ErrorKind, tagged_err};
use crate::io::read_ndjson_source;
use crate::output::{print_json_compact, print_json_line};

#[derive(Args)]
pub(crate) struct ReviewReproduceArgs {
    #[command(flatten)]
    pub(crate) store: ReviewStore,
    #[arg(
        long = "ndjson",
        value_name = "SRC",
        required = true,
        help = "Reproductions, one JSON object per line (finding, flow, milestone, round, task, verdict, by, citation): `-` for stdin, otherwise a file path (a leading `@` is accepted)"
    )]
    pub(crate) ndjson: String,
    #[command(flatten)]
    pub(crate) integrity: WriteIntegrityArgs,
}

#[derive(Args)]
pub(crate) struct ReviewReproduceStatusArgs {
    #[arg(
        value_name = "ID,...",
        required_unless_present = "flow",
        num_args = 1..,
        value_delimiter = ',',
        help = "Finding ids, comma- or space-separated"
    )]
    pub(crate) ids: Vec<String>,
    #[command(flatten)]
    pub(crate) store: ReviewStore,
    #[arg(
        long = "flow",
        value_name = "SLUG",
        conflicts_with = "ids",
        requires = "milestone",
        help = "Select the findings the flow's rows of --milestone fix, from its [[finding_links]] for this store, in place of ids"
    )]
    pub(crate) flow: Option<String>,
    #[arg(
        long = "milestone",
        value_name = "ID",
        requires = "flow",
        help = "The checkpoint whose rows' fixed findings --flow selects"
    )]
    pub(crate) milestone: Option<String>,
    #[arg(
        long = "expect",
        value_name = "VERDICT",
        required = true,
        help = "gone|reproduces|unsettled"
    )]
    pub(crate) expect: String,
    #[command(flatten)]
    pub(crate) integrity: ReadIntegrityArgs,
}

impl super::Verb for ReviewReproduceArgs {
    fn run(self) -> Result<()> {
        record(
            &super::store_target(&self.store)?,
            &self.ndjson,
            &self.integrity,
        )
    }
}

impl super::Verb for ReviewReproduceStatusArgs {
    fn run(self) -> Result<()> {
        let select = match (&self.flow, &self.milestone) {
            (Some(flow), Some(milestone)) => Select::Milestone { flow, milestone },
            _ => Select::Ids(&self.ids),
        };
        status(
            &super::store_target(&self.store)?,
            select,
            &self.expect,
            &self.integrity,
        )
    }
}

const RECORD: &str = "review reproduce";
const STATUS: &str = "review reproduce-status";

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
    // `at` is stamped by the verb, never supplied.
    let supplied = |k: &str| REPRODUCTION_CORE.contains(&k) && k != "at";
    for key in row.keys().filter(|k| !supplied(k)) {
        let keys: Vec<&str> = REPRODUCTION_CORE
            .iter()
            .copied()
            .filter(|k| supplied(k))
            .collect();
        reasons.push(format!(
            "unknown key `{key}`; a reproduction carries {}",
            keys.join(", ")
        ));
    }
    let mut text = |key: &str, needed: bool| -> String {
        match row.get(key) {
            Some(JsonValue::String(s)) if !s.trim().is_empty() => s.trim().to_string(),
            Some(JsonValue::String(_)) | None | Some(JsonValue::Null) if !needed => String::new(),
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

/// The findings `reproduce-status` reads: ids given, or every finding a
/// flow's rows of one milestone fix in this store.
pub(crate) enum Select<'a> {
    Ids(&'a [String]),
    Milestone { flow: &'a str, milestone: &'a str },
}

pub(crate) fn status(
    target: &Target,
    select: Select<'_>,
    expect: &str,
    integrity: &ReadIntegrityArgs,
) -> Result<()> {
    let Some(expect) = ReproduceVerdict::parse(expect.trim()) else {
        return Err(invalid(format!(
            "{STATUS}: --expect {expect:?} is not one of {}",
            ReproduceVerdict::VALUES.join("|")
        )));
    };
    let mut wanted: Vec<String> = Vec::new();
    let mut push = |id: &str| {
        let id = id.trim();
        if !id.is_empty() && !wanted.iter().any(|w| w == id) {
            wanted.push(id.to_string());
        }
    };
    match select {
        Select::Ids(ids) => {
            ids.iter().for_each(|id| push(id));
            if wanted.is_empty() {
                return Err(invalid(format!("{STATUS}: no finding ids given")));
            }
        }
        Select::Milestone { flow, milestone } => {
            milestone_fixes(target, flow, milestone, integrity)?
                .iter()
                .for_each(|id| push(id));
        }
    }

    let stored = store::load(&target.path, integrity)?;
    let mut findings = Vec::new();
    // An empty milestone selection fails: a fix row carries this check only
    // when its milestone has findings to prove, so none means lost links.
    let mut ok = !wanted.is_empty();
    for id in wanted.iter().map(String::as_str) {
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
        return Err(crate::invocation::ExitWith(1).into());
    }
    Ok(())
}

/// The `fixes` of every `[[finding_links]]` entry for this review store on a
/// row of `flow` whose checkpoint is `milestone`, in row order.
fn milestone_fixes(
    target: &Target,
    flow: &str,
    milestone: &str,
    integrity: &ReadIntegrityArgs,
) -> Result<Vec<String>> {
    let tasks_path = crate::tasks::resolve_store_path(Some(flow), None)?;
    if !tasks_path.is_file() {
        return Err(tagged_err(
            ErrorKind::NotFound,
            Some(tasks_path),
            format!("{STATUS}: flow {flow} has no task store"),
        ));
    }
    let read = ReadIntegrityArgs {
        verify_integrity: integrity.verify_integrity,
        strict_read: false,
    };
    let tasks = crate::tasks::load_store(&tasks_path, &read)?;
    let store_rel = target.rel();
    let milestone = milestone.trim();
    let mut fixes = Vec::new();
    for row in tasks.items.iter().filter(|r| r.checkpoint == milestone) {
        for link in tasks
            .finding_links
            .iter()
            .filter(|l| l.r#ref == row.r#ref && l.store == store_rel)
        {
            fixes.extend(link.fixes.iter().cloned());
        }
    }
    Ok(fixes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> Store {
        let doc: toml::Value = toml::from_str(
            "schema_version = 1\n\n[[findings]]\nid = \"F1\"\nfamily = \"code\"\nkind = \"defect\"\n\
             status = \"open\"\n",
        )
        .expect("the fixture parses");
        store::from_toml(&doc).expect("the fixture reads")
    }

    fn row(extra: JsonValue) -> Map<String, JsonValue> {
        let mut row = json!({
            "finding": "F1", "flow": "x", "milestone": "a", "round": 1,
            "verdict": "gone", "by": "verifier", "citation": "src/a.rs:1",
        });
        row.as_object_mut()
            .expect("an object")
            .extend(extra.as_object().expect("an object").clone());
        row.as_object().expect("an object").clone()
    }

    #[test]
    fn a_blank_optional_task_is_recorded_as_none() {
        let today = "2026-11-11".parse().expect("a date");
        let record = parse_row(&store(), &row(json!({"task": "  "})), today).expect("lands");
        assert_eq!(record.task, "");
        let reasons = parse_row(&store(), &row(json!({"citation": ""})), today).unwrap_err();
        assert_eq!(reasons, ["`citation` is required"]);
    }

    #[test]
    fn at_is_not_a_key_a_batch_supplies() {
        let today = "2026-11-11".parse().expect("a date");
        let reasons = parse_row(&store(), &row(json!({"at": "2026-01-01"})), today).unwrap_err();
        assert_eq!(
            reasons,
            [
                "unknown key `at`; a reproduction carries finding, flow, milestone, round, task, verdict, by, citation"
            ]
        );
    }
}
