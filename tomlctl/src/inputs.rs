//! The repo's user-input store, `.claude/inputs.toml`: captures, change
//! requests and notes the user files against the item ledgers, the questions
//! agents post, and the user's answers to them. The array is `inputs`, never
//! `items`, so the items writers' `dedup_id` stamping never reaches it.
//!
//! Every record is untrusted data: `author` is self-declared, and nothing here
//! acts on a record's text. Nothing here prints, every function takes the repo
//! root explicitly, and every write refuses a root that is not the process root.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::{Map, Value as JsonValue, json};
use sha2::{Digest, Sha256};
use toml::Value as TomlValue;
use toml::de::DeValue;
use toml::value::Datetime;

use crate::backlog::schema as backlog_schema;
use crate::convert::{devalue_to_json, json_to_toml};
use crate::errors::{ErrorKind, tagged_err};
use crate::flow::validate_slug;
use crate::integrity::{IntegrityOpts, hex_lower};
use crate::io::{
    OnMissing, ensure_process_root, item_id, items_array_mut, mutate_doc_conditional,
    relativise_under,
};
use crate::ledgers::with_borrowed_doc;

pub(crate) const ARRAY: &str = "inputs";

pub(crate) const KIND_CAPTURE: &str = "capture";
pub(crate) const KIND_REQUEST: &str = "request";
pub(crate) const KIND_NOTE: &str = "note";
pub(crate) const KIND_QUESTION: &str = "question";
pub(crate) const KIND_ANSWER: &str = "answer";
pub(crate) const KINDS: [&str; 5] = [
    KIND_CAPTURE,
    KIND_REQUEST,
    KIND_NOTE,
    KIND_QUESTION,
    KIND_ANSWER,
];

pub(crate) const STATUS_NEW: &str = "new";
pub(crate) const STATUS_ACKNOWLEDGED: &str = "acknowledged";
pub(crate) const STATUS_HANDLED: &str = "handled";
pub(crate) const STATUS_WITHDRAWN: &str = "withdrawn";
pub(crate) const STATUSES: [&str; 4] = [
    STATUS_NEW,
    STATUS_ACKNOWLEDGED,
    STATUS_HANDLED,
    STATUS_WITHDRAWN,
];

pub(crate) const LEDGERS: [&str; 4] = ["review", "optimise", "plan-review", "backlog"];
pub(crate) const CHOICES: [&str; 3] = ["single", "multi", "text"];

const AUTHOR_USER: &str = "user";

/// The fields `add` takes from its caller; the rest are assigned by `add`
/// or written by the lifecycle verbs.
const ADDABLE: [&str; 12] = [
    "kind",
    "author",
    "ledger",
    "flow",
    "scope",
    "items",
    "text",
    "capture_kind",
    "area",
    "prompt",
    "choice",
    "options",
];

const ASSIGNED: [&str; 11] = [
    "id",
    "status",
    "created",
    "answers",
    "picked",
    "acknowledged",
    "acknowledged_by",
    "handled",
    "handled_by",
    "handled_note",
    ANSWERED_BY,
];

const CAPTURE_ONLY: [&str; 2] = ["capture_kind", "area"];
const QUESTION_ONLY: [&str; 3] = ["prompt", "choice", "options"];
const ANSWER_ONLY: [&str; 2] = ["answers", "picked"];
const ACK_FIELDS: [&str; 2] = ["acknowledged", "acknowledged_by"];
const HANDLED_FIELDS: [&str; 3] = ["handled", "handled_by", "handled_note"];
const TARGET_FIELDS: [&str; 4] = ["ledger", "flow", "scope", "items"];
/// The answer that closed a handled question; dropped when that answer is withdrawn.
const ANSWERED_BY: &str = "answered_by";

pub(crate) fn path(root: &Path) -> PathBuf {
    root.join(".claude").join("inputs.toml")
}

/// `list`'s selection; every set criterion must hold. `pending` keeps the
/// records no agent has finished with: `new` and `acknowledged`.
#[derive(Debug, Default, Clone)]
pub(crate) struct Filter {
    pub(crate) pending: bool,
    pub(crate) kinds: Vec<String>,
    pub(crate) ledger: Option<String>,
    pub(crate) flow: Option<String>,
    pub(crate) scope: Option<String>,
    pub(crate) item: Option<String>,
}

impl Filter {
    fn check(&self) -> Result<()> {
        for kind in &self.kinds {
            one_of("kind", kind, &KINDS)?;
        }
        if let Some(ledger) = &self.ledger {
            one_of("ledger", ledger, &LEDGERS)?;
        }
        Ok(())
    }

    fn matches(&self, row: &JsonValue) -> bool {
        let field = |name: &str| row.get(name).and_then(JsonValue::as_str);
        let equals = |name: &str, want: &Option<String>| {
            want.as_deref().is_none_or(|w| field(name) == Some(w))
        };
        (!self.pending || matches!(field("status"), Some(STATUS_NEW | STATUS_ACKNOWLEDGED)))
            && (self.kinds.is_empty() || self.kinds.iter().any(|k| field("kind") == Some(k)))
            && equals("ledger", &self.ledger)
            && equals("flow", &self.flow)
            && equals("scope", &self.scope)
            && self.item.as_deref().is_none_or(|want| {
                row.get("items")
                    .and_then(JsonValue::as_array)
                    .is_some_and(|ids| ids.iter().any(|id| id.as_str() == Some(want)))
            })
    }
}

/// `{"path", "revision", "inputs"}`, where `revision` is the hex sha256 of
/// the bytes read and `inputs` the rows `filter` keeps, unvalidated. A
/// missing store reads as no rows and a null revision.
pub(crate) fn list(root: &Path, filter: &Filter) -> Result<JsonValue> {
    Ok(list_if_changed(root, filter, None)?.expect("no known revision always reads"))
}

/// [`list`], or `None`, parsing nothing, when the store's bytes hash to
/// `known`.
pub(crate) fn list_if_changed(
    root: &Path,
    filter: &Filter,
    known: Option<&str>,
) -> Result<Option<JsonValue>> {
    filter.check()?;
    let path = path(root);
    let shown = relativise_under(root, &path).unwrap_or_else(|| path.display().to_string());
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Some(json!({"path": shown, "revision": null, "inputs": []})));
        }
        Err(e) => {
            return Err(anyhow::Error::new(e).context(format!("reading {}", path.display())));
        }
    };
    let revision = hex_lower(&Sha256::digest(&bytes));
    if known == Some(revision.as_str()) {
        return Ok(None);
    }
    let rows: Vec<JsonValue> = with_borrowed_doc(&path, bytes, |doc| {
        let rows = match doc.get(ARRAY).map(|v| v.get_ref()) {
            Some(DeValue::Array(rows)) => rows.as_ref(),
            _ => &[],
        };
        let mut out = Vec::new();
        for (i, row) in rows.iter().enumerate() {
            let row = devalue_to_json(row.get_ref())
                .with_context(|| format!("{}: `{ARRAY}` row {i}", path.display()))?;
            if filter.matches(&row) {
                out.push(row);
            }
        }
        Ok(out)
    })?;
    Ok(Some(
        json!({"path": shown, "revision": revision, "inputs": rows}),
    ))
}

/// Appends one record as `new`, assigning its `id` and `created`; `author`
/// defaults to `user`. Answers are recorded through [`answer`], which also
/// closes their question. Returns `{"id"}`.
pub(crate) fn add(root: &Path, record: &JsonValue, integrity: IntegrityOpts) -> Result<JsonValue> {
    let Some(fields) = record.as_object() else {
        return Err(invalid("an input record must be a JSON object"));
    };
    for key in fields.keys() {
        if ASSIGNED.contains(&key.as_str()) {
            return Err(invalid(format!(
                "`{key}` is assigned by the store, not by add"
            )));
        }
        if !ADDABLE.contains(&key.as_str()) {
            return Err(invalid(format!("`{key}` is not an input record field")));
        }
    }
    if fields.get("kind").and_then(JsonValue::as_str) == Some(KIND_ANSWER) {
        return Err(invalid(
            "record an answer with `answer`, which also closes its question",
        ));
    }
    let id = write(root, integrity, |rows, now| {
        let id = next_id(rows);
        let mut row = toml::Table::new();
        row.insert("id".into(), TomlValue::String(id.clone()));
        row.insert("kind".into(), toml_field(fields, "kind")?);
        let author = match fields.get("author") {
            Some(value) => json_to_toml(value).context("`author`")?,
            None => TomlValue::String(AUTHOR_USER.into()),
        };
        row.insert("author".into(), author);
        row.insert("status".into(), TomlValue::String(STATUS_NEW.into()));
        row.insert("created".into(), TomlValue::Datetime(now));
        for (key, value) in fields {
            if key != "kind" && key != "author" {
                row.insert(
                    key.clone(),
                    json_to_toml(value).with_context(|| format!("`{key}`"))?,
                );
            }
        }
        rows.push(TomlValue::Table(row));
        Ok(vec![id])
    })?;
    Ok(json!({"id": id[0]}))
}

/// Moves each `new` record of `ids` to `acknowledged`, stamped with `by`.
/// Returns `{"applied", "skipped": [{id, kind, status}]}`; a record past `new`
/// is skipped, and so is a question, which waits on the user — `answer` needs
/// it `new`. An unknown id fails the whole call.
pub(crate) fn ack(
    root: &Path,
    ids: &[String],
    by: &str,
    integrity: IntegrityOpts,
) -> Result<JsonValue> {
    non_empty("--by", by)?;
    lifecycle(
        root,
        ids,
        integrity,
        &[STATUS_NEW],
        &[KIND_QUESTION],
        |row, now| {
            set_str(row, "status", STATUS_ACKNOWLEDGED);
            row.insert("acknowledged".into(), TomlValue::Datetime(now));
            set_str(row, "acknowledged_by", by);
        },
    )
}

/// Moves each `new` or `acknowledged` record of `ids` to `handled`, with
/// `by` and a `note` saying what was done or why it was declined. Returns
/// what [`ack`] does.
pub(crate) fn handle(
    root: &Path,
    ids: &[String],
    by: &str,
    note: &str,
    integrity: IntegrityOpts,
) -> Result<JsonValue> {
    non_empty("--by", by)?;
    non_empty("--note", note)?;
    lifecycle(
        root,
        ids,
        integrity,
        &[STATUS_NEW, STATUS_ACKNOWLEDGED],
        &[],
        |row, now| {
            set_str(row, "status", STATUS_HANDLED);
            row.insert("handled".into(), TomlValue::Datetime(now));
            set_str(row, "handled_by", by);
            set_str(row, "handled_note", note);
        },
    )
}

/// Withdraws `ids`, refusing the whole call unless every one is `new`.
/// Withdrawing an answer returns the question it closed to `new`. Returns
/// `{"applied", "reopened"}`.
pub(crate) fn withdraw(root: &Path, ids: &[String], integrity: IntegrityOpts) -> Result<JsonValue> {
    let ids = unique(ids);
    let mut reopened: Vec<String> = Vec::new();
    write(root, integrity, |rows, _| {
        let mut questions = Vec::new();
        for id in &ids {
            let row = find(rows, id)?;
            let status = str_of(row, "status");
            if status != STATUS_NEW {
                return Err(invalid(format!(
                    "{id} is {status}; only a new record can be withdrawn"
                )));
            }
            if str_of(row, "kind") == KIND_ANSWER {
                questions.push((str_of(row, "answers").to_string(), id.clone()));
            }
        }
        for id in &ids {
            set_str(find_mut(rows, id)?, "status", STATUS_WITHDRAWN);
        }
        let mut touched = ids.clone();
        for (question, answer) in questions {
            let Ok(row) = find_mut(rows, &question) else {
                continue;
            };
            // A question closed before `answered_by` existed carries only the note.
            let names_it = match row.get(ANSWERED_BY) {
                Some(by) => by.as_str() == Some(answer.as_str()),
                None => str_of(row, "handled_note") == answered_note(&answer),
            };
            let closed_by_it = str_of(row, "status") == STATUS_HANDLED
                && str_of(row, "handled_by") == AUTHOR_USER
                && names_it;
            if closed_by_it {
                for field in HANDLED_FIELDS.iter().chain(&[ANSWERED_BY]) {
                    row.remove(*field);
                }
                set_str(row, "status", STATUS_NEW);
                reopened.push(question.clone());
                touched.push(question);
            }
        }
        Ok(touched)
    })?;
    Ok(json!({"applied": ids, "reopened": reopened}))
}

/// Records the user's answer to the `new` question `question` — `picked`
/// options, free `text`, or both — and moves the question to `handled`. The
/// answer inherits the question's target, so the carrier that asked finds it.
/// Returns `{"id", "question"}`.
pub(crate) fn answer(
    root: &Path,
    question: &str,
    picked: &[String],
    text: Option<&str>,
    integrity: IntegrityOpts,
) -> Result<JsonValue> {
    let text = text.filter(|t| !t.trim().is_empty());
    if picked.is_empty() && text.is_none() {
        return Err(invalid("an answer needs a picked option or text"));
    }
    let id = write(root, integrity, |rows, now| {
        let asked = find(rows, question)?;
        if str_of(asked, "kind") != KIND_QUESTION {
            return Err(invalid(format!("{question} is not a question")));
        }
        let status = str_of(asked, "status");
        if status != STATUS_NEW {
            return Err(invalid(format!(
                "{question} is {status}; only a new question can be answered"
            )));
        }
        check_picks(question, asked, picked)?;
        let id = next_id(rows);
        let mut row = toml::Table::new();
        row.insert("id".into(), TomlValue::String(id.clone()));
        set_str(&mut row, "kind", KIND_ANSWER);
        set_str(&mut row, "author", AUTHOR_USER);
        set_str(&mut row, "status", STATUS_NEW);
        row.insert("created".into(), TomlValue::Datetime(now));
        for field in TARGET_FIELDS {
            if let Some(value) = asked.get(field) {
                row.insert(field.into(), value.clone());
            }
        }
        set_str(&mut row, "answers", question);
        if !picked.is_empty() {
            let options = picked.iter().cloned().map(TomlValue::String).collect();
            row.insert("picked".into(), TomlValue::Array(options));
        }
        if let Some(text) = text {
            set_str(&mut row, "text", text);
        }
        let asked = find_mut(rows, question)?;
        set_str(asked, "status", STATUS_HANDLED);
        asked.insert("handled".into(), TomlValue::Datetime(now));
        set_str(asked, "handled_by", AUTHOR_USER);
        set_str(asked, "handled_note", &answered_note(&id));
        set_str(asked, ANSWERED_BY, &id);
        rows.push(TomlValue::Table(row));
        Ok(vec![id, question.to_string()])
    })?;
    Ok(json!({"id": id[0], "question": question}))
}

fn answered_note(answer: &str) -> String {
    format!("answered by {answer}")
}

fn check_picks(id: &str, question: &toml::Table, picked: &[String]) -> Result<()> {
    let choice = str_of(question, "choice");
    if choice == "text" && !picked.is_empty() {
        return Err(invalid(format!(
            "question {id} takes a text answer, not options"
        )));
    }
    if choice == "single" && picked.len() > 1 {
        return Err(invalid(format!("question {id} takes a single option")));
    }
    let options: Vec<&str> = question
        .get("options")
        .and_then(TomlValue::as_array)
        .map(|o| o.iter().filter_map(TomlValue::as_str).collect())
        .unwrap_or_default();
    let mut seen = BTreeSet::new();
    for pick in picked {
        if !options.contains(&pick.as_str()) {
            return Err(invalid(format!(
                "`{pick}` is not an option of question {id}; options: {}",
                options.join(", ")
            )));
        }
        if !seen.insert(pick.as_str()) {
            return Err(invalid(format!("`{pick}` is picked twice")));
        }
    }
    Ok(())
}

/// Applies `edit` to each record of `ids` whose status is in `from` and whose
/// kind is not in `exempt`, skipping the rest and failing on an unknown id.
fn lifecycle(
    root: &Path,
    ids: &[String],
    integrity: IntegrityOpts,
    from: &[&str],
    exempt: &[&str],
    edit: impl Fn(&mut toml::Table, Datetime),
) -> Result<JsonValue> {
    let ids = unique(ids);
    let mut skipped: Vec<JsonValue> = Vec::new();
    let applied = write(root, integrity, |rows, now| {
        let mut applied = Vec::new();
        for id in &ids {
            let row = find_mut(rows, id)?;
            let status = str_of(row, "status").to_string();
            let kind = str_of(row, "kind").to_string();
            if from.contains(&status.as_str()) && !exempt.contains(&kind.as_str()) {
                edit(row, now);
                applied.push(id.clone());
            } else {
                skipped.push(json!({"id": id, "kind": kind, "status": status}));
            }
        }
        Ok(applied)
    })?;
    Ok(json!({"applied": applied, "skipped": skipped}))
}

/// One locked read-modify-write of the store, seeding it when missing. `f`
/// returns the ids of the rows it changed, each validated before anything is
/// written; when it changed none, nothing is written.
fn write(
    root: &Path,
    integrity: IntegrityOpts,
    f: impl FnOnce(&mut Vec<TomlValue>, Datetime) -> Result<Vec<String>>,
) -> Result<Vec<String>> {
    ensure_process_root(root)?;
    let path = path(root);
    let now = now()?;
    let today = crate::time::today_toml_date()?;
    let mut seed = toml::Table::new();
    seed.insert("schema_version".into(), TomlValue::Integer(1));
    seed.insert("last_updated".into(), TomlValue::Datetime(today));
    let mut touched = Vec::new();
    let on_missing = OnMissing::Create(TomlValue::Table(seed));
    mutate_doc_conditional(&path, false, integrity, on_missing, |doc| {
        touched = f(items_array_mut(doc, ARRAY)?, now)?;
        if touched.is_empty() {
            return Ok(false);
        }
        for id in &touched {
            validate(find(items_array_mut(doc, ARRAY)?, id)?)
                .with_context(|| format!("{id} would be invalid"))?;
        }
        if let Some(table) = doc.as_table_mut() {
            table.insert("last_updated".into(), TomlValue::Datetime(today));
        }
        Ok(true)
    })?;
    Ok(touched)
}

/// Checks one record against the schema: the shared fields, the fields its
/// kind requires and admits, and the lifecycle fields its status implies.
pub(crate) fn validate(row: &toml::Table) -> Result<()> {
    for key in row.keys() {
        if !ADDABLE.contains(&key.as_str()) && !ASSIGNED.contains(&key.as_str()) {
            return Err(invalid(format!("`{key}` is not an input record field")));
        }
    }
    let id = str_of(row, "id");
    if !is_input_id(id) {
        return Err(invalid(format!("`id` must look like I1, not `{id}`")));
    }
    let kind = required_str(row, "kind")?;
    one_of("kind", kind, &KINDS)?;
    let status = required_str(row, "status")?;
    one_of("status", status, &STATUSES)?;
    let author = required_str(row, "author")?;
    if kind == KIND_QUESTION && author == AUTHOR_USER {
        return Err(invalid(
            "a question's `author` must be the command that posted it, not `user`",
        ));
    }
    datetime(row, "created", true)?;
    if let Some(ledger) = optional_str(row, "ledger")? {
        one_of("ledger", ledger, &LEDGERS)?;
    }
    for name in ["flow", "scope"] {
        if let Some(value) = optional_str(row, name)? {
            validate_slug(value).with_context(|| format!("`{name}`"))?;
        }
    }
    if row.contains_key("items") {
        string_list(row, "items")?;
    }
    if row.contains_key("flow") && row.contains_key("scope") {
        return Err(invalid("a record targets a `flow` or a `scope`, not both"));
    }
    if !row.contains_key("ledger")
        && let Some(field) = ["flow", "scope", "items"]
            .into_iter()
            .find(|f| row.contains_key(*f))
    {
        return Err(invalid(format!("`{field}` needs a `ledger` to target")));
    }
    optional_str(row, "text")?;

    let admitted: &[&str] = match kind {
        KIND_CAPTURE => &CAPTURE_ONLY,
        KIND_QUESTION => &QUESTION_ONLY,
        KIND_ANSWER => &ANSWER_ONLY,
        _ => &[],
    };
    for field in CAPTURE_ONLY
        .iter()
        .chain(&QUESTION_ONLY)
        .chain(&ANSWER_ONLY)
    {
        if row.contains_key(*field) && !admitted.contains(field) {
            return Err(invalid(format!(
                "`{field}` does not belong on a {kind} record"
            )));
        }
    }
    match kind {
        KIND_QUESTION => {
            required_str(row, "prompt")?;
            let choice = required_str(row, "choice")?;
            one_of("choice", choice, &CHOICES)?;
            if choice == "text" {
                if row.contains_key("options") {
                    return Err(invalid("a text question takes no `options`"));
                }
            } else {
                let options = string_list(row, "options")
                    .with_context(|| format!("a {choice} question needs `options`"))?;
                let distinct: BTreeSet<&str> = options.iter().copied().collect();
                if distinct.len() != options.len() {
                    return Err(invalid("`options` repeats an option"));
                }
            }
        }
        KIND_ANSWER => {
            let answers = required_str(row, "answers")?;
            if !is_input_id(answers) {
                return Err(invalid(format!(
                    "`answers` must name a question id, not `{answers}`"
                )));
            }
            let picked = row.contains_key("picked");
            if picked {
                string_list(row, "picked")?;
            }
            if !picked && optional_str(row, "text")?.is_none_or(|t| t.trim().is_empty()) {
                return Err(invalid("an answer needs `picked` or `text`"));
            }
        }
        _ => {
            required_str(row, "text")?;
            if kind == KIND_CAPTURE {
                if let Some(hint) = optional_str(row, "capture_kind")? {
                    one_of("capture_kind", hint, backlog_schema::KINDS)?;
                }
                optional_str(row, "area")?;
            }
        }
    }

    let acknowledged = matches!(status, STATUS_ACKNOWLEDGED | STATUS_HANDLED);
    for field in ACK_FIELDS {
        if row.contains_key(field) && !acknowledged {
            return Err(invalid(format!("a {status} record carries no `{field}`")));
        }
    }
    datetime(row, "acknowledged", status == STATUS_ACKNOWLEDGED)?;
    let ack_by = optional_str(row, "acknowledged_by")?;
    if status == STATUS_ACKNOWLEDGED && ack_by.is_none() {
        return Err(invalid("an acknowledged record needs `acknowledged_by`"));
    }
    for field in HANDLED_FIELDS {
        if row.contains_key(field) && status != STATUS_HANDLED {
            return Err(invalid(format!("a {status} record carries no `{field}`")));
        }
    }
    if status == STATUS_HANDLED {
        datetime(row, "handled", true)?;
        required_str(row, "handled_by")?;
        required_str(row, "handled_note")?;
    }
    if row.contains_key(ANSWERED_BY) {
        if kind != KIND_QUESTION {
            return Err(invalid(format!(
                "`{ANSWERED_BY}` does not belong on a {kind} record"
            )));
        }
        if status != STATUS_HANDLED {
            return Err(invalid(format!(
                "a {status} record carries no `{ANSWERED_BY}`"
            )));
        }
        let answer = required_str(row, ANSWERED_BY)?;
        if !is_input_id(answer) {
            return Err(invalid(format!(
                "`{ANSWERED_BY}` must name an answer id, not `{answer}`"
            )));
        }
    }
    Ok(())
}

fn invalid(msg: impl Into<String>) -> anyhow::Error {
    tagged_err(ErrorKind::Validation, None, msg)
}

fn one_of(field: &str, value: &str, allowed: &[&str]) -> Result<()> {
    if allowed.contains(&value) {
        return Ok(());
    }
    Err(invalid(format!(
        "`{field}` must be one of {}, not `{value}`",
        allowed.join(", ")
    )))
}

fn non_empty(flag: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Err(invalid(format!("{flag} must not be empty")));
    }
    Ok(())
}

fn optional_str<'a>(row: &'a toml::Table, field: &str) -> Result<Option<&'a str>> {
    match row.get(field) {
        None => Ok(None),
        Some(TomlValue::String(s)) => Ok(Some(s)),
        Some(_) => Err(invalid(format!("`{field}` must be a string"))),
    }
}

fn required_str<'a>(row: &'a toml::Table, field: &str) -> Result<&'a str> {
    match optional_str(row, field)? {
        Some(s) if !s.trim().is_empty() => Ok(s),
        _ => Err(invalid(format!("`{field}` is required"))),
    }
}

fn string_list<'a>(row: &'a toml::Table, field: &str) -> Result<Vec<&'a str>> {
    let values = row
        .get(field)
        .and_then(TomlValue::as_array)
        .filter(|a| !a.is_empty())
        .ok_or_else(|| invalid(format!("`{field}` must be a non-empty array of strings")))?;
    values
        .iter()
        .map(|v| {
            v.as_str()
                .filter(|s| !s.trim().is_empty())
                .ok_or_else(|| invalid(format!("`{field}` must hold non-empty strings")))
        })
        .collect()
}

fn datetime(row: &toml::Table, field: &str, required: bool) -> Result<()> {
    match row.get(field) {
        Some(TomlValue::Datetime(_)) => Ok(()),
        None if !required => Ok(()),
        _ => Err(invalid(format!("`{field}` must be a datetime"))),
    }
}

fn is_input_id(id: &str) -> bool {
    id.strip_prefix('I')
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// One past the highest `I{n}` ever assigned; withdrawn records keep theirs,
/// so an id is never reused.
fn next_id(rows: &[TomlValue]) -> String {
    let max = rows
        .iter()
        .filter_map(item_id)
        .filter_map(|id| id.strip_prefix('I')?.parse::<u64>().ok())
        .max()
        .unwrap_or(0);
    format!("I{}", max + 1)
}

/// The current instant to whole seconds, as a TOML offset datetime.
fn now() -> Result<Datetime> {
    let stamp = jiff::Timestamp::from_second(crate::time::now().as_second())
        .context("truncating the current time")?;
    let text = stamp.to_string();
    text.parse::<Datetime>()
        .with_context(|| format!("converting {text} to a TOML datetime"))
}

fn toml_field(fields: &Map<String, JsonValue>, key: &str) -> Result<TomlValue> {
    match fields.get(key) {
        Some(value) => json_to_toml(value).with_context(|| format!("`{key}`")),
        None => Err(invalid(format!("`{key}` is required"))),
    }
}

fn set_str(row: &mut toml::Table, field: &str, value: &str) {
    row.insert(field.into(), TomlValue::String(value.into()));
}

fn str_of<'a>(row: &'a toml::Table, field: &str) -> &'a str {
    row.get(field).and_then(TomlValue::as_str).unwrap_or("")
}

fn find<'a>(rows: &'a [TomlValue], id: &str) -> Result<&'a toml::Table> {
    rows.iter()
        .find(|row| item_id(row) == Some(id))
        .and_then(TomlValue::as_table)
        .ok_or_else(|| invalid(format!("no input record {id}")))
}

fn find_mut<'a>(rows: &'a mut [TomlValue], id: &str) -> Result<&'a mut toml::Table> {
    rows.iter_mut()
        .find(|row| item_id(row) == Some(id))
        .and_then(TomlValue::as_table_mut)
        .ok_or_else(|| invalid(format!("no input record {id}")))
}

fn unique(ids: &[String]) -> Vec<String> {
    let mut seen = BTreeSet::new();
    ids.iter()
        .filter(|id| seen.insert(id.as_str()))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::with_root;

    const OPTS: IntegrityOpts = IntegrityOpts {
        write_sidecar: true,
        verify_on_read: false,
        strict: false,
    };

    fn add_ok(root: &Path, record: JsonValue) -> String {
        let out = add(root, &record, OPTS).unwrap();
        out["id"].as_str().unwrap().to_string()
    }

    fn rows(root: &Path, filter: &Filter) -> Vec<JsonValue> {
        list(root, filter).unwrap()["inputs"]
            .as_array()
            .unwrap()
            .clone()
    }

    fn row(root: &Path, id: &str) -> JsonValue {
        rows(root, &Filter::default())
            .into_iter()
            .find(|r| r["id"] == id)
            .unwrap()
    }

    fn ids(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_string()).collect()
    }

    fn question(root: &Path) -> String {
        add_ok(
            root,
            json!({
                "kind": "question",
                "author": "review",
                "ledger": "review",
                "flow": "some-flow",
                "items": ["R3"],
                "prompt": "Defer R3?",
                "choice": "single",
                "options": ["yes", "no"],
            }),
        )
    }

    #[test]
    fn add_assigns_sequential_ids() {
        with_root(|root| {
            let first = add_ok(root, json!({"kind": "capture", "text": "flaky test"}));
            let second = add_ok(root, json!({"kind": "note", "text": "look at R3"}));
            assert_eq!((first.as_str(), second.as_str()), ("I1", "I2"));
            withdraw(root, &ids(&["I2"]), OPTS).unwrap();
            let third = add_ok(root, json!({"kind": "request", "text": "re-scope"}));
            assert_eq!(third, "I3", "a withdrawn id is never reused");
            let stored = row(root, "I1");
            assert_eq!(stored["status"], "new");
            assert_eq!(stored["author"], "user");
            assert!(stored["created"].as_str().is_some_and(|c| c.contains('T')));
            assert!(path(root).with_extension("toml.sha256").is_file());
        });
    }

    #[test]
    fn add_refuses_store_assigned_and_unknown_fields() {
        with_root(|root| {
            for record in [
                json!({"kind": "note", "text": "x", "status": "handled"}),
                json!({"kind": "note", "text": "x", "id": "I9"}),
                json!({"kind": "note", "text": "x", "colour": "red"}),
                json!({"kind": "answer", "answers": "I1", "text": "x"}),
                json!({"kind": "note", "text": "x", "prompt": "?"}),
                json!({"kind": "capture"}),
            ] {
                assert!(
                    add(root, &record, OPTS).is_err(),
                    "{record} must be refused"
                );
            }
            assert!(!path(root).exists(), "a refused add writes nothing");
        });
    }

    #[test]
    fn answer_handles_its_question() {
        with_root(|root| {
            let asked = question(root);
            let out = answer(root, &asked, &ids(&["yes"]), None, OPTS).unwrap();
            assert_eq!(out["id"], "I2");
            let reply = row(root, "I2");
            assert_eq!(reply["kind"], "answer");
            assert_eq!(reply["answers"], asked);
            assert_eq!(reply["picked"], json!(["yes"]));
            assert_eq!(reply["ledger"], "review", "the answer inherits the target");
            assert_eq!(reply["flow"], "some-flow");
            let closed = row(root, &asked);
            assert_eq!(closed["status"], "handled");
            assert_eq!(closed["handled_by"], "user");
            assert!(
                answer(root, &asked, &ids(&["no"]), None, OPTS).is_err(),
                "a handled question cannot be answered again"
            );
        });
    }

    #[test]
    fn answer_refuses_a_pick_outside_the_options() {
        with_root(|root| {
            let asked = question(root);
            let err = answer(root, &asked, &ids(&["maybe"]), None, OPTS).unwrap_err();
            assert!(
                format!("{err:#}").contains(&format!(
                    "`maybe` is not an option of question {asked}; options: yes, no"
                )),
                "{err:#}"
            );
            let err = answer(root, &asked, &ids(&["yes", "no"]), None, OPTS).unwrap_err();
            assert!(format!("{err:#}").contains(&format!("question {asked}")));
            assert!(answer(root, &asked, &[], None, OPTS).is_err());
            assert_eq!(row(root, &asked)["status"], "new");
        });
    }

    #[test]
    fn withdrawing_an_answer_reopens_its_question() {
        with_root(|root| {
            let asked = question(root);
            answer(root, &asked, &[], Some("only after the release"), OPTS).unwrap();
            assert_eq!(row(root, &asked)["answered_by"], "I2");
            let out = withdraw(root, &ids(&["I2"]), OPTS).unwrap();
            assert_eq!(out["reopened"], json!([asked]));
            let reopened = row(root, &asked);
            assert_eq!(reopened["status"], "new");
            assert!(reopened.get("handled_by").is_none());
            assert!(reopened.get("answered_by").is_none());
        });
    }

    #[test]
    fn withdrawing_an_answer_reopens_a_question_closed_by_note_alone() {
        with_root(|root| {
            let store = path(root);
            fs::create_dir_all(store.parent().unwrap()).unwrap();
            fs::write(
                &store,
                r#"schema_version = 1
last_updated = 2026-10-02

[[inputs]]
id = "I1"
kind = "question"
author = "review"
status = "handled"
created = 2026-10-02T08:00:00Z
prompt = "Why?"
choice = "text"
handled = 2026-10-02T09:00:00Z
handled_by = "user"
handled_note = "answered by I2"

[[inputs]]
id = "I2"
kind = "answer"
author = "user"
status = "new"
created = 2026-10-02T09:00:00Z
answers = "I1"
text = "because"
"#,
            )
            .unwrap();
            let out = withdraw(root, &ids(&["I2"]), OPTS).unwrap();
            assert_eq!(out["reopened"], json!(["I1"]));
            assert_eq!(row(root, "I1")["status"], "new");
        });
    }

    #[test]
    fn ack_skips_a_question() {
        with_root(|root| {
            let asked = question(root);
            let out = ack(root, &ids(&[&asked]), "review", OPTS).unwrap();
            assert_eq!(out["applied"], json!([]));
            assert_eq!(
                out["skipped"],
                json!([{"id": asked, "kind": "question", "status": "new"}])
            );
            assert_eq!(row(root, &asked)["status"], "new");
            assert!(answer(root, &asked, &ids(&["yes"]), None, OPTS).is_ok());
        });
    }

    #[test]
    fn a_target_needs_its_ledger_and_one_of_flow_or_scope() {
        with_root(|root| {
            for record in [
                json!({"kind": "note", "text": "x", "flow": "f", "items": ["R1"]}),
                json!({"kind": "note", "text": "x", "scope": "s"}),
                json!({"kind": "note", "text": "x", "items": ["R1"]}),
                json!({"kind": "note", "text": "x", "ledger": "review", "flow": "f", "scope": "s"}),
            ] {
                assert!(
                    add(root, &record, OPTS).is_err(),
                    "{record} must be refused"
                );
            }
            assert!(!path(root).exists(), "a refused add writes nothing");
        });
    }

    #[test]
    fn answered_by_belongs_only_on_a_handled_question() {
        let mut closed = toml::Table::new();
        for (key, value) in [
            ("id", "I1"),
            ("kind", "question"),
            ("author", "review"),
            ("status", "handled"),
            ("prompt", "Why?"),
            ("choice", "text"),
            ("handled_by", "user"),
            ("handled_note", "answered by I2"),
            ("answered_by", "I2"),
        ] {
            set_str(&mut closed, key, value);
        }
        let at: Datetime = "2026-10-02T08:00:00Z".parse().unwrap();
        closed.insert("created".into(), TomlValue::Datetime(at));
        closed.insert("handled".into(), TomlValue::Datetime(at));
        validate(&closed).unwrap();

        let mut missing_note = closed.clone();
        missing_note.remove("handled_note");
        assert!(
            validate(&missing_note).is_err(),
            "a handled question requires a note"
        );

        let mut open = closed.clone();
        set_str(&mut open, "status", "new");
        for field in HANDLED_FIELDS {
            open.remove(field);
        }
        assert!(validate(&open).is_err(), "a new question carries no answer");

        let mut note = closed.clone();
        for field in QUESTION_ONLY {
            note.remove(field);
        }
        set_str(&mut note, "kind", "note");
        set_str(&mut note, "text", "x");
        assert!(validate(&note).is_err(), "only a question is answered");
    }

    #[test]
    fn withdraw_refuses_an_acknowledged_record() {
        with_root(|root| {
            let id = add_ok(root, json!({"kind": "capture", "text": "x"}));
            ack(root, &ids(&[&id]), "backlog", OPTS).unwrap();
            let err = withdraw(root, &ids(&[&id]), OPTS).unwrap_err();
            assert!(
                format!("{err:#}").contains("only a new record can be withdrawn"),
                "refused for its status, not by the schema backstop: {err:#}"
            );
            assert_eq!(row(root, &id)["status"], "acknowledged");
        });
    }

    #[test]
    fn ack_and_handle_follow_the_lifecycle() {
        with_root(|root| {
            let id = add_ok(root, json!({"kind": "request", "text": "split R3"}));
            let out = ack(root, &ids(&[&id]), "review", OPTS).unwrap();
            assert_eq!(out["applied"], json!([id]));
            let again = ack(root, &ids(&[&id]), "review", OPTS).unwrap();
            assert_eq!(
                again["skipped"],
                json!([{"id": id, "kind": "request", "status": "acknowledged"}])
            );
            handle(
                root,
                &ids(&[&id]),
                "review-apply",
                "split into R3, R9",
                OPTS,
            )
            .unwrap();
            let done = row(root, &id);
            assert_eq!(done["status"], "handled");
            assert_eq!(done["acknowledged_by"], "review");
            assert_eq!(done["handled_note"], "split into R3, R9");
            assert!(ack(root, &ids(&["I99"]), "review", OPTS).is_err());
        });
    }

    #[test]
    fn a_question_without_options_is_invalid() {
        with_root(|root| {
            let mut record = json!({
                "kind": "question",
                "author": "review",
                "prompt": "Which?",
                "choice": "multi",
            });
            assert!(add(root, &record, OPTS).is_err());
            record["choice"] = json!("text");
            assert!(
                add(root, &record, OPTS).is_ok(),
                "a text question needs no options"
            );
        });
    }

    #[test]
    fn list_filters_compose() {
        with_root(|root| {
            question(root);
            add_ok(
                root,
                json!({"kind": "note", "text": "x", "ledger": "optimise", "items": ["O1"]}),
            );
            let done = add_ok(
                root,
                json!({"kind": "note", "text": "y", "ledger": "review"}),
            );
            handle(root, &ids(&[&done]), "review", "noted", OPTS).unwrap();
            let pick = |filter: Filter| -> Vec<String> {
                rows(root, &filter)
                    .iter()
                    .map(|r| r["id"].as_str().unwrap().to_string())
                    .collect()
            };
            let review = Filter {
                ledger: Some("review".into()),
                ..Filter::default()
            };
            assert_eq!(pick(review.clone()), ids(&["I1", "I3"]));
            let pending = Filter {
                pending: true,
                ..review
            };
            assert_eq!(pick(pending), ids(&["I1"]));
            let item = Filter {
                item: Some("O1".into()),
                ..Filter::default()
            };
            assert_eq!(pick(item), ids(&["I2"]));
            let notes = Filter {
                kinds: ids(&["note"]),
                ..Filter::default()
            };
            assert_eq!(pick(notes), ids(&["I2", "I3"]));
        });
    }

    #[test]
    fn a_missing_store_lists_empty() {
        with_root(|root| {
            let out = list(root, &Filter::default()).unwrap();
            assert_eq!(out["inputs"], json!([]));
            assert!(out["revision"].is_null());
        });
    }
}
