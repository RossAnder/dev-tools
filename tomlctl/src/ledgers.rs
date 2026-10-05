//! Silent surface over the item ledgers a library consumer browses and
//! triages: the flow-local and flow-less review, optimise and plan-review
//! ledgers, and the repo backlog. Nothing here prints, and every function
//! takes the repo root explicitly rather than resolving it from the process.
//!
//! The writes edit control fields only — status dispositions with their
//! companions, and review/optimise classification — each guarded by a
//! per-row compare-and-set. A stale row is skipped and reported, never
//! overwritten, and no write restamps the ledger's `last_updated`.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde_json::{Map, Value as JsonValue, json};
use sha2::{Digest, Sha256};
use toml::de::{DeTable, DeValue};

use crate::backlog::schema as backlog_schema;
use crate::convert::{devalue_to_json, toml_to_json};
use crate::errors::{ErrorKind, tagged_err};
use crate::flow::validate_slug;
use crate::integrity::{IntegrityOpts, hex_lower};
use crate::io::{
    OnMissing, ensure_process_root, item_id, items_array, mutate_doc_conditional, read_dir_sorted,
    relativise_under,
};
use crate::items::{
    Item, STATUS_COMPANIONS, StaleOp, StalePolicy, compute_apply_mutation_with, status_companions,
};

/// The flow-scoped item ledgers; the backlog is [`LedgerRef::Backlog`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum LedgerKind {
    Review,
    Optimise,
    PlanReview,
}

impl LedgerKind {
    pub const ALL: [LedgerKind; 3] = [Self::Review, Self::Optimise, Self::PlanReview];

    /// The `kind` string [`crate::ledger_read`] and [`crate::ledger_scopes`]
    /// report.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Review => "review",
            Self::Optimise => "optimise",
            Self::PlanReview => "plan-review",
        }
    }

    /// The ledger's file name inside a flow directory.
    pub fn flow_file(self) -> &'static str {
        match self {
            Self::Review => "review-ledger.toml",
            Self::Optimise => "optimise-findings.toml",
            Self::PlanReview => "plan-review-findings.toml",
        }
    }

    /// The directory under `.claude/` holding this kind's flow-less ledgers.
    pub fn scope_dir(self) -> &'static str {
        match self {
            Self::Review => "reviews",
            Self::Optimise => "optimise-findings",
            Self::PlanReview => "plan-review-findings",
        }
    }
}

/// One ledger file. `Flow` and `Scope` names must match the `--slug` regex,
/// so neither can name a path outside its directory; `File` is any path and
/// is read-only.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum LedgerRef {
    Flow { slug: String, kind: LedgerKind },
    Scope { kind: LedgerKind, scope: String },
    Backlog,
    File(PathBuf),
}

impl LedgerRef {
    /// The ledger's file under `root`, refusing a `Flow` slug or `Scope` name
    /// the `--slug` regex rejects.
    pub fn path(&self, root: &Path) -> Result<PathBuf> {
        let claude = root.join(".claude");
        Ok(match self {
            Self::Flow { slug, kind } => {
                validate_slug(slug)?;
                claude.join("flows").join(slug).join(kind.flow_file())
            }
            Self::Scope { kind, scope } => {
                validate_slug(scope)?;
                claude.join(kind.scope_dir()).join(format!("{scope}.toml"))
            }
            Self::Backlog => claude.join("backlog.toml"),
            Self::File(path) => path.clone(),
        })
    }
}

/// `{"path", "kind", "revision", "items"}` for one ledger, where `revision`
/// is the hex sha256 of the bytes read. A missing file reads as no items and
/// a null revision. Returns `None`, parsing nothing, when the bytes hash to
/// `known`.
pub(crate) fn read(
    root: &Path,
    ledger: &LedgerRef,
    known: Option<&str>,
) -> Result<Option<JsonValue>> {
    let path = ledger.path(root)?;
    let shown = relativise_under(root, &path).unwrap_or_else(|| path.display().to_string());
    let bytes = match fs::read(&path) {
        Ok(bytes) => Some(bytes),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => {
            return Err(anyhow::Error::new(e).context(format!("reading {}", path.display())));
        }
    };
    let Some(bytes) = bytes else {
        let kind = kind_of(ledger, &path, false)?;
        return Ok(Some(
            json!({"path": shown, "kind": kind, "revision": null, "items": []}),
        ));
    };
    let revision = hex_lower(&Sha256::digest(&bytes));
    if known == Some(revision.as_str()) {
        return Ok(None);
    }
    let (kind, items) = with_borrowed_doc(&path, bytes, |doc| {
        let kind = kind_of(ledger, &path, doc.contains_key("backlog"))?;
        let array = if kind == "backlog" {
            "backlog"
        } else {
            "items"
        };
        let items = match doc.get(array).map(|v| v.get_ref()) {
            None => Vec::new(),
            Some(DeValue::Array(rows)) => rows
                .iter()
                .map(|row| devalue_to_json(row.get_ref()))
                .collect(),
            Some(_) => bail!("{}: `{array}` is not an array", path.display()),
        };
        Ok((kind, items))
    })?;
    Ok(Some(
        json!({"path": shown, "kind": kind, "revision": revision, "items": items}),
    ))
}

/// Parses `bytes`, read from `path`, without the owned `toml::Value` tree,
/// failing a non-UTF-8 or malformed file with the error and `parse` tag
/// `parse_toml_bytes` gives it.
pub(crate) fn with_borrowed_doc<R>(
    path: &Path,
    bytes: Vec<u8>,
    f: impl FnOnce(&DeTable<'_>) -> Result<R>,
) -> Result<R> {
    let source = String::from_utf8(bytes).map_err(|_| {
        anyhow::Error::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "stream did not contain valid UTF-8",
        ))
        .context(format!("reading {}", path.display()))
    })?;
    let doc = DeTable::parse(&source).map_err(|e| {
        tagged_err(
            ErrorKind::Parse,
            Some(path.to_owned()),
            format!("parsing {}: {}", path.display(), e),
        )
    })?;
    f(doc.get_ref())
}

/// A `File` ledger's kind comes from its basename, then its parent directory,
/// then whether it holds a `backlog` key.
fn kind_of(ledger: &LedgerRef, path: &Path, has_backlog: bool) -> Result<&'static str> {
    match ledger {
        LedgerRef::Flow { kind, .. } | LedgerRef::Scope { kind, .. } => return Ok(kind.as_str()),
        LedgerRef::Backlog => return Ok("backlog"),
        LedgerRef::File(_) => {}
    }
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if name == "backlog.toml" {
        return Ok("backlog");
    }
    if let Some(kind) = LedgerKind::ALL.iter().find(|k| k.flow_file() == name) {
        return Ok(kind.as_str());
    }
    let parent = path
        .parent()
        .and_then(Path::file_name)
        .and_then(|n| n.to_str())
        .unwrap_or("");
    if let Some(kind) = LedgerKind::ALL.iter().find(|k| k.scope_dir() == parent) {
        return Ok(kind.as_str());
    }
    if has_backlog {
        return Ok("backlog");
    }
    bail!(
        "cannot infer the ledger kind of {}: name it backlog.toml, review-ledger.toml, \
         optimise-findings.toml or plan-review-findings.toml",
        path.display()
    )
}

/// `{"flows": [{"slug", "has_tasks", "ledgers"}], "scopes": [{"kind", "scope"}]}`:
/// every flow directory holding a task store or a ledger, and every flow-less
/// ledger file whose stem is a valid scope name.
pub(crate) fn scopes(root: &Path) -> Result<JsonValue> {
    let claude = root.join(".claude");
    let mut flows = Vec::new();
    let flows_dir = claude.join("flows");
    if flows_dir.is_dir() {
        for entry in read_dir_sorted(&flows_dir)? {
            let dir = entry.path();
            let Some(slug) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if validate_slug(&slug).is_err() || !dir.is_dir() {
                continue;
            }
            let has_tasks = dir.join("tasks.toml").is_file();
            let ledgers: Vec<&str> = LedgerKind::ALL
                .iter()
                .filter(|k| dir.join(k.flow_file()).is_file())
                .map(|k| k.as_str())
                .collect();
            if has_tasks || !ledgers.is_empty() {
                flows.push(json!({"slug": slug, "has_tasks": has_tasks, "ledgers": ledgers}));
            }
        }
    }
    let mut scopes = Vec::new();
    for kind in LedgerKind::ALL {
        let dir = claude.join(kind.scope_dir());
        if !dir.is_dir() {
            continue;
        }
        for entry in read_dir_sorted(&dir)? {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("toml") || !path.is_file() {
                continue;
            }
            let Some(scope) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            if validate_slug(scope).is_ok() {
                scopes.push(json!({"kind": kind.as_str(), "scope": scope}));
            }
        }
    }
    Ok(json!({"flows": flows, "scopes": scopes}))
}

/// The form fields of a transition the write facade offers, or `None` for any
/// other. `fixed`, `applied` and `merged` belong to the apply flows and the
/// plan merge, and backlog moves go through backlog triage.
fn offered(kind: LedgerKind, from: &str, to: &str) -> Option<Vec<&'static str>> {
    use LedgerKind::{Optimise, PlanReview, Review};
    match (kind, from, to) {
        (Review | Optimise, "open", "deferred")
        | (Review, "open", "wontfix" | "verified-clean")
        | (Optimise, "open", "wontapply")
        | (PlanReview, "open", "discarded") => Some(companions(to).collect()),
        (Review | Optimise, "deferred", "open") => Some(vec!["reopen_rationale"]),
        _ => None,
    }
}

/// The companion fields a status owns, removed when an item leaves it.
fn companions(status: &str) -> impl Iterator<Item = &'static str> {
    status_companions(status).iter().map(|(field, _)| *field)
}

const CLASSIFY_FIELDS: [&str; 3] = ["severity", "effort", "category"];
const SEVERITIES: [&str; 3] = ["critical", "warning", "suggestion"];
const EFFORTS: [&str; 3] = ["trivial", "small", "medium"];

/// Every facade write keeps the sidecar current and never verifies it.
pub(crate) const FACADE_WRITE: IntegrityOpts = IntegrityOpts {
    write_sidecar: true,
    verify_on_read: false,
    strict: false,
};

/// What a restore may write on each ledger: the status values glimpse can
/// move an item away from, and the fields its writes touch.
fn restorable(ledger: &LedgerRef) -> Result<(&'static [&'static str], Vec<&'static str>)> {
    Ok(match ledger {
        LedgerRef::Backlog => (
            &[
                backlog_schema::STATUS_OPEN,
                backlog_schema::STATUS_DISMISSED,
                backlog_schema::STATUS_RESOLVED,
            ],
            backlog_schema::MANAGED_FIELDS.to_vec(),
        ),
        LedgerRef::Flow { kind, .. } | LedgerRef::Scope { kind, .. } => {
            let statuses: &[&str] = match kind {
                LedgerKind::PlanReview => &["open"],
                LedgerKind::Review | LedgerKind::Optimise => &["open", "deferred"],
            };
            let mut fields: Vec<&str> = Vec::new();
            if *kind != LedgerKind::PlanReview {
                fields.extend(CLASSIFY_FIELDS);
            }
            let forms = STATUS_COMPANIONS
                .iter()
                .filter_map(|(to, _)| offered(*kind, "open", to))
                .chain(offered(*kind, "deferred", "open"));
            for field in forms.flatten() {
                if !fields.contains(&field) {
                    fields.push(field);
                }
            }
            (statuses, fields)
        }
        LedgerRef::File(_) => bail!("a ledger named by path is read-only"),
    })
}

/// The kind a review, optimise or plan-review write targets, refusing the
/// backlog and path-named ledgers.
fn item_kind(ledger: &LedgerRef, verb: &str) -> Result<LedgerKind> {
    match ledger {
        LedgerRef::Flow { kind, .. } | LedgerRef::Scope { kind, .. } => Ok(*kind),
        LedgerRef::Backlog => bail!("{verb} does not edit the backlog; use backlog triage"),
        LedgerRef::File(_) => bail!("a ledger named by path is read-only"),
    }
}

fn is_empty(value: &JsonValue) -> bool {
    match value {
        JsonValue::Null => true,
        JsonValue::String(s) => s.is_empty(),
        JsonValue::Array(a) => a.is_empty(),
        _ => false,
    }
}

/// One row's guarded edit: `set` merges, `unset` removes, and the whole edit
/// is skipped when any `expect` field no longer holds its value.
struct RowEdit {
    id: String,
    set: Map<String, JsonValue>,
    unset: Vec<String>,
    expect: Map<String, JsonValue>,
}

/// Applies `edits` in one locked read-modify-write, validating every row it
/// changes with `validate`. Returns `{"applied", "skipped_stale"}`; a row
/// that is gone is stale on `id`, found `null`. Nothing is written when no
/// edit lands.
fn write_guarded(
    root: &Path,
    ledger: &LedgerRef,
    edits: Vec<RowEdit>,
    validate: impl Fn(&JsonValue) -> Result<()>,
) -> Result<JsonValue> {
    ensure_process_root(root)?;
    let path = ledger.path(root)?;
    if !path.is_file() {
        bail!("no ledger at {}", path.display());
    }
    let array = if *ledger == LedgerRef::Backlog {
        backlog_schema::ARRAY_BACKLOG
    } else {
        "items"
    };
    let mut applied: Vec<String> = Vec::new();
    let mut stale: Vec<StaleOp> = Vec::new();
    mutate_doc_conditional(&path, false, FACADE_WRITE, OnMissing::Error, |doc| {
        let present: BTreeSet<&str> = items_array(doc, array).iter().filter_map(item_id).collect();
        let mut ops = Vec::new();
        for edit in edits {
            if !present.contains(edit.id.as_str()) {
                stale.push(StaleOp {
                    field: "id".into(),
                    expected: JsonValue::String(edit.id.clone()),
                    found: JsonValue::Null,
                    id: edit.id,
                });
                continue;
            }
            ops.push(json!({
                "op": "update",
                "id": edit.id,
                "json": edit.set,
                "unset": edit.unset,
                "expect": edit.expect,
            }));
        }
        if ops.is_empty() {
            return Ok(false);
        }
        let guarded = compute_apply_mutation_with(
            doc,
            array,
            &JsonValue::Array(ops),
            true,
            StalePolicy::Skip,
        )?;
        stale.extend(guarded.skipped_stale);
        let plan = guarded.plan;
        for id in &plan.updated {
            let row = items_array(&plan.new_doc, array)
                .iter()
                .find(|row| item_id(row) == Some(id.as_str()))
                .expect("an updated row is present");
            validate(&toml_to_json(row)).with_context(|| format!("{id} would be invalid"))?;
        }
        if plan.updated.is_empty() {
            return Ok(false);
        }
        applied = plan.updated;
        *doc = plan.new_doc;
        Ok(true)
    })?;
    let skipped: Vec<JsonValue> = stale.iter().map(StaleOp::to_json).collect();
    Ok(json!({"applied": applied, "skipped_stale": skipped}))
}

fn unique(ids: &[String]) -> Vec<String> {
    let mut seen = BTreeSet::new();
    ids.iter()
        .filter(|id| seen.insert(id.as_str()))
        .cloned()
        .collect()
}

/// Moves each of `ids` whose status is still `expect_status` to `to`, writing
/// `fields` and dropping the companions of the status it leaves; a reopen
/// keeps `defer_reason` as the audit trail. Only the offered transitions are
/// accepted, and only their form fields.
pub(crate) fn transition(
    root: &Path,
    ledger: &LedgerRef,
    ids: &[String],
    to: &str,
    fields: Map<String, JsonValue>,
    expect_status: &str,
) -> Result<JsonValue> {
    let kind = item_kind(ledger, "a transition")?;
    let Some(form) = offered(kind, expect_status, to) else {
        bail!(
            "the {} → {to} transition is not offered on a {} ledger",
            expect_status,
            kind.as_str()
        );
    };
    for (field, value) in &fields {
        if !form.contains(&field.as_str()) {
            bail!(
                "`{field}` is not a field of the {expect_status} → {to} transition (expected one of {})",
                form.join(", ")
            );
        }
        if !value.is_string() {
            bail!("`{field}` must be a string");
        }
    }
    let unset: Vec<String> = companions(expect_status)
        .filter(|f| !(to == "open" && *f == "defer_reason"))
        .filter(|f| !fields.contains_key(*f))
        .map(|f| f.to_string())
        .collect();
    let mut set = fields;
    set.insert("status".into(), JsonValue::String(to.into()));
    let mut expect = Map::new();
    expect.insert("status".into(), JsonValue::String(expect_status.into()));
    let edits = unique(ids)
        .into_iter()
        .map(|id| RowEdit {
            id,
            set: set.clone(),
            unset: unset.clone(),
            expect: expect.clone(),
        })
        .collect();
    write_guarded(root, ledger, edits, |row| Ok(Item::validate(row)?))
}

/// Sets `severity`, `effort` and `category` from `fields` on each of `ids`
/// whose fields still match `expect`, on review and optimise ledgers only.
/// The row's `dedup_id` is recomputed as `items update` does.
pub(crate) fn classify(
    root: &Path,
    ledger: &LedgerRef,
    ids: &[String],
    fields: Map<String, JsonValue>,
    expect: Map<String, JsonValue>,
) -> Result<JsonValue> {
    let kind = item_kind(ledger, "classify")?;
    if kind == LedgerKind::PlanReview {
        bail!("classify edits review and optimise ledgers only, not plan-review");
    }
    if fields.is_empty() {
        bail!("classify needs at least one of severity, effort, category");
    }
    for (field, value) in &fields {
        if !CLASSIFY_FIELDS.contains(&field.as_str()) {
            bail!("classify sets only severity, effort and category, not `{field}`");
        }
        let Some(value) = value.as_str().filter(|s| !s.trim().is_empty()) else {
            bail!("`{field}` must be a non-empty string");
        };
        let allowed: &[&str] = match field.as_str() {
            "severity" => &SEVERITIES,
            "effort" => &EFFORTS,
            _ => continue,
        };
        if !allowed.contains(&value) {
            bail!("`{field}` must be one of {}", allowed.join(", "));
        }
    }
    if expect.is_empty() {
        bail!("classify needs an `expect` precondition");
    }
    let edits = unique(ids)
        .into_iter()
        .map(|id| RowEdit {
            id,
            set: fields.clone(),
            unset: Vec::new(),
            expect: expect.clone(),
        })
        .collect();
    write_guarded(root, ledger, edits, |_| Ok(()))
}

/// What to put back on one row: `set` restores values and `unset` removes
/// fields the write added, provided the row still holds `expect`.
#[derive(Debug, Clone, PartialEq)]
pub struct RestoreRow {
    pub id: String,
    pub set: Map<String, JsonValue>,
    pub unset: Vec<String>,
    pub expect: Map<String, JsonValue>,
}

/// Puts back what one glimpse write changed on each of `rows`, in one locked
/// write; each row is guarded on its own `expect`. Only the fields and
/// from-statuses glimpse writes are accepted, and one refused row refuses the
/// call. The result is not re-validated, so a row that was malformed before
/// the write can still be returned to exactly that state.
pub(crate) fn restore(root: &Path, ledger: &LedgerRef, rows: Vec<RestoreRow>) -> Result<JsonValue> {
    let (statuses, fields) = restorable(ledger)?;
    let mut ids = BTreeSet::new();
    for row in &rows {
        let RestoreRow {
            id,
            set,
            unset,
            expect,
        } = row;
        if !ids.insert(id.as_str()) {
            bail!("restore names `{id}` more than once");
        }
        if expect.is_empty() {
            bail!("restore needs an `expect` precondition");
        }
        for (field, value) in set {
            if field == "status" {
                if !value.as_str().is_some_and(|s| statuses.contains(&s)) {
                    bail!(
                        "restore cannot set status {value}; expected one of {}",
                        statuses.join(", ")
                    );
                }
            } else if !fields.contains(&field.as_str()) {
                bail!("restore does not write `{field}`");
            } else if is_empty(value) {
                bail!("restore cannot set `{field}` to an empty value; unset it instead");
            }
        }
        for field in unset {
            if !fields.contains(&field.as_str()) {
                bail!("restore does not remove `{field}`");
            }
            if set.contains_key(field) {
                bail!("restore both sets and removes `{field}`");
            }
        }
    }
    let edits = rows
        .into_iter()
        .map(|row| RowEdit {
            id: row.id,
            set: row.set,
            unset: row.unset,
            expect: row.expect,
        })
        .collect();
    write_guarded(root, ledger, edits, |_| Ok(()))
}

#[cfg(test)]
mod tests {
    use crate::test_support::with_root;

    #[test]
    fn a_matching_known_revision_reads_nothing_and_another_reads_in_full() {
        with_root(|root| {
            let ledger = super::LedgerRef::Backlog;
            let path = ledger.path(root).unwrap();
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, "[[backlog]]\nid = \"B1\"\n").unwrap();
            let full = crate::ledger_read(root, &ledger).unwrap();
            let revision = full["revision"].as_str().unwrap().to_string();
            assert_eq!(full["items"].as_array().unwrap().len(), 1);

            let same = crate::ledger_read_if_changed(root, &ledger, Some(&revision)).unwrap();
            assert!(same.is_none());

            let other = crate::ledger_read_if_changed(root, &ledger, Some("0000")).unwrap();
            assert_eq!(other.unwrap(), full);
        });
    }

    #[test]
    fn items_read_as_the_owned_toml_conversion_renders_them() {
        with_root(|root| {
            let ledger = super::LedgerRef::Scope {
                kind: super::LedgerKind::Review,
                scope: "parity".into(),
            };
            let path = ledger.path(root).unwrap();
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            let source = r#"schema_version = 1
last_updated = 2026-10-02

[[items]]
id = "R1"
first_flagged = 2026-10-02
seen_at = 2026-10-02T08:30:00Z
local = 2026-10-02T08:30:00.125
time = 08:30:00
count = 42
hex = 0xff
ratio = 0.5
escaped = "a \"quoted\" é line\n"
nested = [[1, 2], ["a"], []]
inline = { k = "v", n = -7 }

[[items.vet_events]]
at = 2026-10-02
verdict = "kept"
"#;
            std::fs::write(&path, source).unwrap();
            let doc = toml::from_str::<toml::Value>(source).unwrap();
            let expected = crate::convert::toml_to_json(&doc)["items"].clone();
            let read = crate::ledger_read(root, &ledger).unwrap();
            assert_eq!(read["items"], expected);
            assert_eq!(
                serde_json::to_string(&read["items"]).unwrap(),
                serde_json::to_string(&expected).unwrap()
            );

            std::fs::write(&path, "[[items]]\nid = \n").unwrap();
            let err = crate::ledger_read(root, &ledger).unwrap_err();
            let shown = format!("{err:#}");
            assert!(
                shown.starts_with(&format!("parsing {}:", path.display())),
                "{shown}"
            );
        });
    }
}
