//! The `plan request` leaves: a running worker's one request of its run for
//! files it does not hold and corrections to its own spec, and the
//! orchestrator's rulings on it.
//!
//! `add` rules each path item a command can settle inside the plan.toml write
//! lock, so two concurrent posts cannot both be granted one path: a forbidden
//! path, one the task already holds and one another row could edit alongside
//! it are denied, and a declared generated output of an input the task holds
//! is granted. Everything else stays open for `rule`. Nothing here writes the
//! task store; the drain applies grants and then marks them `applied`.

use std::collections::BTreeSet;

use anyhow::{Context, Result};
use serde_json::{Map, Value as JsonValue, json};

use super::holders::{self, Holder, fold_key};
use super::schema::PlanStore;
use super::schema_request::{
    ChangeField, ChangeItem, PathItem, Request, RequestStatus, RuledBy, Ruling,
};
use super::store;
use crate::cli::{
    PlanRequestAddArgs, PlanRequestAppliedArgs, PlanRequestListArgs, PlanRequestRuleArgs,
    ReadIntegrityArgs,
};
use crate::errors::{ErrorKind, tagged_err};
use crate::generated;
use crate::io::{read_json_value_from_arg, read_ndjson_source, repo_or_cwd_root};
use crate::items::{parse_ndjson_batch, row_err, row_string};
use crate::output::{Rows, print_json_compact, print_report};
use crate::tasks::schema::Store;
use crate::tasks::{load_store, resolve_store_path};

type Row = Map<String, JsonValue>;

const PAYLOAD_KEYS: &[&str] = &["paths", "changes"];
const PATH_KEYS: &[&str] = &["path", "why"];
const CHANGE_KEYS: &[&str] = &["field", "what", "evidence"];
const RULE_KEYS: &[&str] = &["item", "ruling", "reason"];
const MAX_PATHS: usize = 16;
const STEP_PREFIX: &str = "implement:";

fn refusal(message: impl Into<String>) -> anyhow::Error {
    tagged_err(ErrorKind::Validation, None, message)
}

struct Payload {
    paths: Vec<(String, String)>,
    changes: Vec<(ChangeField, String, String)>,
}

pub(super) fn add(args: PlanRequestAddArgs) -> Result<()> {
    let PlanRequestAddArgs {
        target,
        task,
        json,
        integrity,
    } = args;
    let slug = target.slug.as_str();
    let payload = parse_payload(&read_json_value_from_arg(&json).context("parsing --json")?)?;
    // A malformed generated-file map must not cost a running worker its one
    // request: without it no output is granted mechanically, so every path
    // stays open for the orchestrator, and the summary names the problem.
    let mut warnings = Vec::new();
    let generators = if payload.paths.is_empty() {
        Vec::new()
    } else {
        generated::load(&repo_or_cwd_root()?).unwrap_or_else(|err| {
            warnings.push(format!(
                "{} is malformed, so no declared output was granted: {err}",
                generated::CONFIG
            ));
            Vec::new()
        })
    };
    let tasks = load_tasks(slug, integrity.verify_integrity)?;
    let at = crate::time::now_toml_datetime()?;

    let request = store::mutate(&store::resolve(slug)?, &integrity, |plan| {
        let (run_id, milestone) = dispatching_run(plan, task)?;
        if let Some(earlier) = plan
            .requests
            .iter()
            .find(|r| r.run_id == run_id && r.task == task)
        {
            return Err(refusal(format!(
                "task {task} already posted request {} in run {run_id}; a task posts one \
                 request per run",
                earlier.id
            )));
        }
        let row = tasks
            .items
            .iter()
            .find(|row| row.id == task)
            .ok_or_else(|| refusal(format!("no task {task} in the task store")))?;
        let held: BTreeSet<String> = row.files.iter().map(|f| fold_key(f)).collect();
        let outputs: BTreeSet<String> = generated::outputs_of(&generators, &row.files)
            .iter()
            .map(|o| fold_key(o))
            .collect();

        let mut paths = Vec::with_capacity(payload.paths.len());
        for (path, why) in &payload.paths {
            let (ruling, by, reason) = rule_path(slug, &tasks, plan, task, path, &held, &outputs)?;
            paths.push(PathItem {
                path: path.clone(),
                why: why.clone(),
                ruling,
                by,
                reason,
            });
        }
        let changes = payload
            .changes
            .iter()
            .map(|(field, what, evidence)| ChangeItem {
                field: *field,
                what: what.clone(),
                evidence: evidence.clone(),
                ruling: Ruling::Open,
                by: RuledBy::Unset,
                reason: String::new(),
            })
            .collect();

        let mut request = Request {
            id: plan.mint_request_id(),
            run_id,
            milestone,
            task,
            at,
            status: RequestStatus::Open,
            ruled_at: None,
            applied: false,
            paths,
            changes,
        };
        settle(&mut request, at);
        plan.requests.push(request.clone());
        Ok(request)
    })?;
    let mut out = summary(&request);
    if !warnings.is_empty() {
        out["warnings"] = json!(warnings);
    }
    print_json_compact(&out)
}

/// The run id and milestone of the newest open `implement:<id>` run that
/// dispatched `task`.
fn dispatching_run(plan: &PlanStore, task: u32) -> Result<(String, String)> {
    store::open_runs(plan)
        .into_iter()
        .rev()
        .find_map(|run| {
            let milestone = run.step.strip_prefix(STEP_PREFIX)?;
            run.tasks
                .contains(&task)
                .then(|| (run.run_id.clone(), milestone.to_string()))
        })
        .ok_or_else(|| {
            refusal(format!(
                "no open `implement:<milestone>` run lists task {task}; a request is posted \
                 only by a task its milestone's live run dispatched"
            ))
        })
}

/// The mechanical ruling of one path item, in precedence order; an item no
/// rule settles stays open.
fn rule_path(
    slug: &str,
    tasks: &Store,
    plan: &PlanStore,
    task: u32,
    path: &str,
    held: &BTreeSet<String>,
    outputs: &BTreeSet<String>,
) -> Result<(Ruling, RuledBy, String)> {
    let key = fold_key(path);
    if let Some(reason) = holders::forbidden(slug, path) {
        return Ok((Ruling::Denied, RuledBy::Tomlctl, reason));
    }
    if held.contains(&key) {
        return Ok((
            Ruling::Denied,
            RuledBy::Tomlctl,
            "already held by this task".to_string(),
        ));
    }
    if let Some(holder) = holders::holder(tasks, plan, task, path)? {
        return Ok((Ruling::Denied, RuledBy::Tomlctl, held_by(&holder)));
    }
    if outputs.contains(&key) {
        return Ok((
            Ruling::Granted,
            RuledBy::Tomlctl,
            format!(
                "a declared output, in {}, of an input this task holds",
                generated::CONFIG
            ),
        ));
    }
    Ok((Ruling::Open, RuledBy::Unset, String::new()))
}

fn held_by(holder: &Holder) -> String {
    match &holder.grant {
        None => format!("held by task {} ({})", holder.task, holder.status),
        Some(id) => format!(
            "granted to task {} ({}) in request {id}",
            holder.task, holder.status
        ),
    }
}

/// Marks the request ruled once no item is left open.
fn settle(request: &mut Request, at: toml::value::Datetime) {
    let open = request.paths.iter().any(|p| p.ruling == Ruling::Open)
        || request.changes.iter().any(|c| c.ruling == Ruling::Open);
    if !open {
        request.status = RequestStatus::Ruled;
        request.ruled_at = Some(at);
    }
}

fn parse_payload(value: &JsonValue) -> Result<Payload> {
    let object = value
        .as_object()
        .ok_or_else(|| refusal("`--json` must be a JSON object with `paths`, `changes` or both"))?;
    check_keys(object, PAYLOAD_KEYS, "the payload")?;

    let mut seen = BTreeSet::new();
    let mut paths = Vec::new();
    for (index, entry) in entries(object, "paths")?.into_iter().enumerate() {
        let at = format!("paths[{index}]");
        check_keys(entry, PATH_KEYS, &at)?;
        let raw = text(entry, "path", &at)?;
        let path = generated::canonical_path(&raw);
        if path.is_empty() {
            return Err(refusal(format!("{at}: `{raw}` names no file")));
        }
        if !seen.insert(fold_key(&path)) {
            return Err(refusal(format!(
                "{at}: `{path}` is named twice; name each path once"
            )));
        }
        paths.push((path, text(entry, "why", &at)?));
    }
    if paths.len() > MAX_PATHS {
        return Err(refusal(format!(
            "a request names at most {MAX_PATHS} paths, not {}",
            paths.len()
        )));
    }

    let mut changes = Vec::new();
    for (index, entry) in entries(object, "changes")?.into_iter().enumerate() {
        let at = format!("changes[{index}]");
        check_keys(entry, CHANGE_KEYS, &at)?;
        let raw = text(entry, "field", &at)?;
        let field = ChangeField::parse(&raw).ok_or_else(|| {
            refusal(format!(
                "{at}: unknown field `{raw}` — expected one of {}",
                ChangeField::VOCABULARY.join(", ")
            ))
        })?;
        changes.push((
            field,
            text(entry, "what", &at)?,
            text(entry, "evidence", &at)?,
        ));
    }

    if paths.is_empty() && changes.is_empty() {
        return Err(refusal(
            "a request needs at least one item in `paths` or `changes`",
        ));
    }
    Ok(Payload { paths, changes })
}

fn check_keys(object: &Row, allowed: &[&str], at: &str) -> Result<()> {
    match object.keys().find(|key| !allowed.contains(&key.as_str())) {
        Some(key) => Err(refusal(format!(
            "{at}: unknown key `{key}` — expected one of {}",
            allowed.join(", ")
        ))),
        None => Ok(()),
    }
}

fn entries<'a>(object: &'a Row, key: &str) -> Result<Vec<&'a Row>> {
    let Some(value) = object.get(key) else {
        return Ok(Vec::new());
    };
    let array = value
        .as_array()
        .ok_or_else(|| refusal(format!("`{key}` must be an array of objects")))?;
    array
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            entry
                .as_object()
                .ok_or_else(|| refusal(format!("{key}[{index}] must be an object")))
        })
        .collect()
}

fn text(object: &Row, key: &str, at: &str) -> Result<String> {
    match object.get(key).and_then(JsonValue::as_str).map(str::trim) {
        Some(value) if !value.is_empty() => Ok(value.to_string()),
        _ => Err(refusal(format!("{at}: `{key}` must be a non-empty string"))),
    }
}

/// One `rule` row: the item it names and the ruling to write.
struct RuleRow {
    line: usize,
    item: String,
    ruling: Ruling,
    reason: String,
}

enum ItemRef {
    Path(usize),
    Change(usize),
}

pub(super) fn rule(args: PlanRequestRuleArgs) -> Result<()> {
    let PlanRequestRuleArgs {
        id,
        target,
        ndjson,
        integrity,
    } = args;
    let slug = target.slug.as_str();
    let rows = parse_rule_rows(&read_ndjson_source(&ndjson)?)?;
    let tasks = load_tasks(slug, integrity.verify_integrity)?;
    let at = crate::time::now_toml_datetime()?;

    let request = store::mutate(&store::resolve(slug)?, &integrity, |plan| {
        let index = plan
            .requests
            .iter()
            .position(|r| r.id == id)
            .ok_or_else(|| refusal(format!("no request `{id}`")))?;
        let request = &plan.requests[index];
        if request.status != RequestStatus::Open {
            return Err(refusal(format!(
                "request {id} is {}; only an open request is ruled",
                request.status.as_str()
            )));
        }

        let mut named = BTreeSet::new();
        let mut edits = Vec::with_capacity(rows.len());
        for row in &rows {
            let item = item_ref(&row.item)
                .filter(|item| match item {
                    ItemRef::Path(i) => *i < request.paths.len(),
                    ItemRef::Change(i) => *i < request.changes.len(),
                })
                .ok_or_else(|| {
                    row_err(row.line, format!("request {id} has no item `{}`", row.item))
                })?;
            let key = match item {
                ItemRef::Path(i) => (true, i),
                ItemRef::Change(i) => (false, i),
            };
            if !named.insert(key) {
                return Err(row_err(
                    row.line,
                    format!("item `{}` is ruled twice", row.item),
                ));
            }
            let current = match item {
                ItemRef::Path(i) => request.paths[i].ruling,
                ItemRef::Change(i) => request.changes[i].ruling,
            };
            if current != Ruling::Open {
                return Err(row_err(
                    row.line,
                    format!(
                        "item `{}` of request {id} is already ruled {}",
                        row.item,
                        current.as_str()
                    ),
                ));
            }
            if let (ItemRef::Path(i), Ruling::Granted) = (&item, row.ruling) {
                let path = &request.paths[*i].path;
                if let Some(holder) = holders::holder(&tasks, plan, request.task, path)? {
                    return Err(row_err(
                        row.line,
                        format!("cannot grant `{path}`: {}", held_by(&holder)),
                    ));
                }
            }
            edits.push((item, row.ruling, row.reason.clone()));
        }

        let request = &mut plan.requests[index];
        for (item, ruling, reason) in edits {
            let (slot, by, why) = match item {
                ItemRef::Path(i) => {
                    let p = &mut request.paths[i];
                    (&mut p.ruling, &mut p.by, &mut p.reason)
                }
                ItemRef::Change(i) => {
                    let c = &mut request.changes[i];
                    (&mut c.ruling, &mut c.by, &mut c.reason)
                }
            };
            *slot = ruling;
            *by = RuledBy::Orchestrator;
            *why = reason;
        }
        settle(request, at);
        Ok(request.clone())
    })?;
    print_json_compact(&summary(&request))
}

/// `p<n>` or `c<n>`, numbered from 1, as a zero-based index. `<n>` is plain
/// digits with no leading zero, so each item has exactly one spelling.
fn item_ref(item: &str) -> Option<ItemRef> {
    let (kind, number) = item.split_at_checked(1)?;
    if number.starts_with('0') || !number.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let number: usize = number.parse().ok()?;
    let index = number.checked_sub(1)?;
    match kind {
        "p" => Some(ItemRef::Path(index)),
        "c" => Some(ItemRef::Change(index)),
        _ => None,
    }
}

fn parse_rule_rows(text: &str) -> Result<Vec<RuleRow>> {
    parse_ndjson_batch(text)?
        .iter()
        .map(|(line, row)| {
            let line = *line;
            if let Some(key) = row.keys().find(|key| !RULE_KEYS.contains(&key.as_str())) {
                return Err(row_err(
                    line,
                    format!(
                        "unknown key `{key}` — expected one of {}",
                        RULE_KEYS.join(", ")
                    ),
                ));
            }
            let item = row_string(line, "item", row.get("item"))?
                .filter(|item| !item.trim().is_empty())
                .ok_or_else(|| row_err(line, "a row needs `item`"))?;
            let raw = row_string(line, "ruling", row.get("ruling"))?
                .ok_or_else(|| row_err(line, "a row needs `ruling`"))?;
            let ruling = Ruling::parse(&raw)
                .filter(|ruling| *ruling != Ruling::Open)
                .ok_or_else(|| {
                    row_err(
                        line,
                        format!(
                            "unknown ruling `{raw}` — expected one of granted, denied, needs-user"
                        ),
                    )
                })?;
            let reason = row_string(line, "reason", row.get("reason"))?
                .map(|reason| reason.trim().to_string())
                .filter(|reason| !reason.is_empty())
                .ok_or_else(|| row_err(line, "a row needs a non-blank `reason`"))?;
            Ok(RuleRow {
                line,
                item: item.trim().to_string(),
                ruling,
                reason,
            })
        })
        .collect()
}

pub(super) fn list(args: PlanRequestListArgs) -> Result<()> {
    let PlanRequestListArgs { target, integrity } = args;
    let plan = store::load(&store::resolve(&target.slug)?, &integrity)?;
    let rows = plan.requests.iter().map(list_row).collect();
    print_report(JsonValue::Array(rows), Rows::Top)
}

fn list_row(request: &Request) -> JsonValue {
    json!({
        "id": request.id,
        "run_id": request.run_id,
        "milestone": request.milestone,
        "task": request.task,
        "at": request.at.to_string(),
        "status": request.status.as_str(),
        "ruled_at": request.ruled_at.map(|at| at.to_string()).unwrap_or_default(),
        "applied": request.applied,
        "paths": request.paths.iter().map(|p| json!({
            "path": p.path,
            "why": p.why,
            "ruling": p.ruling.as_str(),
            "by": p.by.as_str(),
            "reason": p.reason,
        })).collect::<Vec<_>>(),
        "changes": request.changes.iter().map(|c| json!({
            "field": c.field.as_str(),
            "what": c.what,
            "evidence": c.evidence,
            "ruling": c.ruling.as_str(),
            "by": c.by.as_str(),
            "reason": c.reason,
        })).collect::<Vec<_>>(),
    })
}

pub(super) fn applied(args: PlanRequestAppliedArgs) -> Result<()> {
    let PlanRequestAppliedArgs {
        ids,
        target,
        integrity,
    } = args;
    let mut unique: Vec<String> = Vec::with_capacity(ids.len());
    for id in ids.into_iter().map(|id| id.trim().to_string()) {
        if !id.is_empty() && !unique.contains(&id) {
            unique.push(id);
        }
    }
    if unique.is_empty() {
        return Err(refusal("name at least one request id"));
    }
    store::mutate(&store::resolve(&target.slug)?, &integrity, |plan| {
        for id in &unique {
            let request = plan
                .requests
                .iter()
                .find(|r| &r.id == id)
                .ok_or_else(|| refusal(format!("no request `{id}`")))?;
            if request.status == RequestStatus::Open {
                return Err(refusal(format!(
                    "request {id} is still open; rule every item before marking it applied"
                )));
            }
        }
        for request in plan.requests.iter_mut() {
            if unique.contains(&request.id) {
                request.applied = true;
            }
        }
        Ok(())
    })?;
    print_json_compact(&json!({"ok": true, "ids": unique}))
}

fn summary(request: &Request) -> JsonValue {
    let paths = request.paths.iter().enumerate().map(|(i, p)| {
        json!({
            "item": format!("p{}", i + 1),
            "ruling": p.ruling.as_str(),
            "by": p.by.as_str(),
            "reason": p.reason,
        })
    });
    let changes = request.changes.iter().enumerate().map(|(i, c)| {
        json!({
            "item": format!("c{}", i + 1),
            "ruling": c.ruling.as_str(),
            "by": c.by.as_str(),
            "reason": c.reason,
        })
    });
    json!({
        "ok": true,
        "id": request.id,
        "status": request.status.as_str(),
        "items": paths.chain(changes).collect::<Vec<_>>(),
    })
}

/// The flow's task store, read once before the plan.toml lock is taken, as
/// the other `plan` writers that read it do.
fn load_tasks(slug: &str, verify_integrity: bool) -> Result<Store> {
    let path = resolve_store_path(Some(slug), None)?;
    if !path.exists() {
        store::require_open(&store::resolve(slug)?)?;
    }
    load_store(
        &path,
        &ReadIntegrityArgs {
            verify_integrity,
            strict_read: true,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload(raw: &str) -> Result<Payload> {
        parse_payload(&serde_json::from_str(raw).expect("valid JSON"))
    }

    #[test]
    fn a_payload_with_both_item_kinds_parses_with_normalised_paths() {
        let parsed = payload(
            r#"{"paths":[{"path":".\\src\\a.rs","why":"A caller."}],
                "changes":[{"field":"accept","what":"Fix it.","evidence":"tests/a.rs"}]}"#,
        )
        .expect("parses");
        assert_eq!(
            parsed.paths,
            [("src/a.rs".to_string(), "A caller.".to_string())]
        );
        assert_eq!(parsed.changes.len(), 1);
        assert_eq!(parsed.changes[0].0, ChangeField::Accept);
    }

    #[test]
    fn paths_are_stored_canonical_and_one_naming_no_file_is_refused() {
        let parsed = payload(r#"{"paths":[{"path":"src/./a//b.rs","why":"w"}]}"#).expect("parses");
        assert_eq!(parsed.paths[0].0, "src/a/b.rs");
        for raw in ["./", ".", "././", "./."] {
            let body = format!(r#"{{"paths":[{{"path":"{raw}","why":"w"}}]}}"#);
            assert!(payload(&body).is_err(), "{raw}");
        }
        assert!(
            payload(r#"{"paths":[{"path":"src/a.rs","why":"b"},{"path":"src/./A.rs","why":"c"}]}"#)
                .is_err()
        );
    }

    #[test]
    fn an_unknown_key_is_refused_naming_it_at_every_level() {
        for (raw, key) in [
            (r#"{"paths":[],"extra":1}"#, "extra"),
            (r#"{"paths":[{"path":"a","why":"b","stray":1}]}"#, "stray"),
            (
                r#"{"changes":[{"field":"means","what":"a","evidence":"b","odd":1}]}"#,
                "odd",
            ),
        ] {
            let message = payload(raw).err().expect(key).to_string();
            assert!(message.contains(key), "{message}");
        }
    }

    #[test]
    fn an_empty_or_oversized_or_duplicated_payload_is_refused() {
        assert!(payload(r#"{}"#).is_err());
        assert!(payload(r#"{"paths":[{"path":"a","why":"b"},{"path":"./A","why":"c"}]}"#).is_err());
        let many: Vec<String> = (0..=MAX_PATHS)
            .map(|i| format!(r#"{{"path":"f{i}","why":"w"}}"#))
            .collect();
        assert!(payload(&format!(r#"{{"paths":[{}]}}"#, many.join(","))).is_err());
    }

    #[test]
    fn item_refs_number_from_one() {
        assert!(matches!(item_ref("p1"), Some(ItemRef::Path(0))));
        assert!(matches!(item_ref("c3"), Some(ItemRef::Change(2))));
        assert!(matches!(item_ref("p10"), Some(ItemRef::Path(9))));
        for bad in ["p0", "x1", "p", "", "c-1", "p01", "p+1", "c 1"] {
            assert!(item_ref(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn a_rule_row_needs_a_known_ruling_and_a_reason() {
        assert!(parse_rule_rows(r#"{"item":"p1","ruling":"granted","reason":"ok"}"#).is_ok());
        for bad in [
            r#"{"item":"p1","ruling":"open","reason":"ok"}"#,
            r#"{"item":"p1","ruling":"granted","reason":" "}"#,
            r#"{"item":"p1","ruling":"granted","reason":"ok","by":"me"}"#,
        ] {
            assert!(parse_rule_rows(bad).is_err(), "{bad}");
        }
    }
}
