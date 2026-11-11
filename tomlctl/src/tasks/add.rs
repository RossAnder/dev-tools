//! The `add` and `add-many` write verbs.
//!
//! Both funnel through `append`, which mints every row first, validates the
//! whole set against a `Graph` over the store *as it would be*, and only then
//! extends `Store::items`. A dangling target or a cycle therefore leaves
//! `store::mutate`'s closure in the `Err` arm, and nothing — file or sidecar —
//! is written.
//!
//! A batch is one candidate graph, so two rows in the same `add-many` may
//! reference each other; a cycle between them is still refused.
//!
//! An `add-many` row may also carry a `spec`, its `[[file_notes]]`,
//! `[[backlog_links]]` and `[[finding_links]]` entries, and the `reason` an
//! audited store requires.
//! Only row shape is checked before the lock; what depends on the store is
//! checked inside `store::mutate`, so a refusal there also persists nothing.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::{Map as JsonMap, Value as JsonValue};

use super::graph::{Node, Tense, build_or_refuse};
use super::schema::{
    ACCEPT_KINDS, Accept, BacklogLink, Change, DEFAULT_ACTOR, DEFAULT_HEADING_DEPTH, Effort,
    FileNote, FindingLink, MEANS_BINDINGS, Means, Premise, RUN_BY_OWNERS, Status, Store, TaskRow,
    TaskSpec, premise_target,
};
use super::{slug, store};
use crate::cli::WriteIntegrityArgs;
use crate::convert::json_type_name;
use crate::errors::{ErrorKind, tagged_err};
use crate::io::read_flag_file;
use crate::items::{parse_ndjson_objects, row_err, row_ids, row_string, row_strings};

/// One row as the caller supplies it. `effort` is still the raw flag string:
/// the schema's vocabulary refusal is what the user should see, not clap's
/// usage prose.
pub(crate) struct NewTask {
    pub(crate) title: String,
    pub(crate) effort: String,
    pub(crate) files: Vec<String>,
    pub(crate) needs: Vec<u32>,
    pub(crate) coupling: Vec<u32>,
    pub(crate) deps_note: String,
    pub(crate) checkpoint: String,
    pub(crate) action: String,
    pub(crate) detail: String,
    pub(crate) acceptance: String,
}

#[derive(Debug)]
pub(crate) struct AddOutcome {
    pub(crate) id: u32,
    pub(crate) r#ref: String,
    /// Zero-based index into `Graph::kahn_rounds` — the round the row lands in
    /// once it is stored.
    pub(crate) batch: usize,
    /// What the row's `ref` cost, when it took over an orphan spec.
    pub(crate) advisory: Option<String>,
}

/// Row fields an NDJSON line may carry. Anything else is refused rather than
/// dropped: a mistyped `neds` would otherwise silently discard an edge.
const ROW_FIELDS: [&str; 19] = [
    "title",
    "effort",
    "files",
    "needs",
    "coupling",
    "deps_note",
    "checkpoint",
    "action",
    "detail",
    "acceptance",
    "spec",
    "reason",
    "actor",
    "file_notes",
    "closes",
    "refs",
    "review_store",
    "fixes",
    "settles",
];

const SPEC_FIELDS: [&str; 8] = [
    "outcome",
    "constraints",
    "accept",
    "premises",
    "means",
    "design",
    "precedent",
    "executor",
];
const ACCEPT_FIELDS: [&str; 7] = [
    "behaviour",
    "check",
    "kind",
    "positive",
    "writes_build",
    "run_by",
    "covered_by",
];
const PREMISE_FIELDS: [&str; 3] = ["claim", "check", "at"];
const MEANS_FIELDS: [&str; 3] = ["text", "binding", "detail"];

const EXECUTORS: [&str; 4] = ["tool", "clerk", "lite", "deep"];

/// What an `add-many` row carries beside its `NewTask`, each landing outside
/// `[[items]]`. `line` is the row's position for a refusal raised under the lock.
#[derive(Default)]
struct RowExtras {
    line: usize,
    spec: Option<SpecPatch>,
    reason: String,
    actor: String,
    file_notes: Vec<(String, String)>,
    closes: Vec<String>,
    refs: Vec<String>,
    /// `None` when the row names no finding.
    finding_link: Option<FindingLink>,
}

/// A row's `spec` key, validated. A field left `None` was not supplied.
#[derive(Clone)]
pub(super) struct SpecPatch {
    outcome: Option<String>,
    constraints: Option<Vec<String>>,
    accept: Option<Vec<Accept>>,
    premises: Option<Vec<Premise>>,
    means: Option<Vec<Means>>,
    design: Option<Vec<String>>,
    precedent: Option<Vec<String>>,
    executor: Option<String>,
}

pub(crate) fn add(
    path: &Path,
    integrity: &WriteIntegrityArgs,
    task: NewTask,
) -> Result<AddOutcome> {
    let mut outcomes = store::mutate(path, integrity, |store| {
        if store.audit {
            return Err(tagged_err(
                ErrorKind::Validation,
                None,
                format!(
                    "task `{}`: `tasks add` has no reason channel and this store is audited — \
                     use `tasks add-many` with a `reason` on the row",
                    task.title
                ),
            ));
        }
        append(store, vec![(task, RowExtras::default())])
    })?;
    Ok(outcomes.remove(0))
}

pub(crate) fn add_many(
    path: &Path,
    integrity: &WriteIntegrityArgs,
    ndjson: &str,
) -> Result<Vec<AddOutcome>> {
    let tasks = parse_rows(ndjson)?;
    store::mutate(path, integrity, |store| append(store, tasks))
}

impl Store {
    /// `add-many` over a store the caller already holds inside
    /// `store::mutate`, for a verb that reads the store to compose its rows.
    /// The rows meet the same parse, refusals and audit as `add-many`'s.
    pub(crate) fn add_many_locked(&mut self, ndjson: &str) -> Result<Vec<AddOutcome>> {
        let tasks = parse_rows(ndjson)?;
        append(self, tasks)
    }
}

fn parse_rows(ndjson: &str) -> Result<Vec<(NewTask, RowExtras)>> {
    parse_ndjson_objects(ndjson)?
        .iter()
        .map(|(line, row)| {
            let task = task_from_row(*line, row)?;
            let extras = extras_from_row(*line, row, &task.files)?;
            Ok((task, extras))
        })
        .collect()
}

/// Resolve a `--<body>` / `--<body>-file` pair. Both absent is a blank body,
/// which is what an incrementally-filled row starts as.
pub(crate) fn body(literal: Option<String>, file: Option<PathBuf>) -> Result<String> {
    match (literal, file) {
        (Some(text), _) => Ok(text),
        (None, Some(path)) => fs::read_to_string(&path)
            .with_context(|| format!("reading body file `{}`", path.display())),
        (None, None) => Ok(String::new()),
    }
}

/// Resolve a `--<field>` / `--<field>-file` pair for a one-line value. The
/// file form is read through `read_flag_file`, which drops a leading BOM and
/// one trailing newline.
pub(crate) fn literal_or_file(
    literal: Option<String>,
    file: Option<PathBuf>,
    flag: &str,
) -> Result<Option<String>> {
    let (Some(path), None) = (file, literal.as_ref()) else {
        return Ok(literal);
    };
    read_flag_file(&path, &format!("{flag}-file")).map(Some)
}

fn append(store: &mut Store, tasks: Vec<(NewTask, RowExtras)>) -> Result<Vec<AddOutcome>> {
    let mut taken: BTreeSet<String> = store
        .items
        .iter()
        .map(|row| row.r#ref.as_str())
        .chain(store.trail_refs())
        .map(str::to_string)
        .collect();

    let mut next = store.next_id();
    let mut minted = Vec::with_capacity(tasks.len());
    let mut extras = Vec::with_capacity(tasks.len());
    for (task, row_extras) in tasks {
        if store.audit && row_extras.reason.trim().is_empty() {
            return Err(row_err(
                row_extras.line,
                format!(
                    "task `{}`: this store is audited, so adding a task needs a `reason`",
                    task.title
                ),
            ));
        }
        extras.push(row_extras);
        reject_undeclared_checkpoint(store, &task.checkpoint, &format!("task `{}`", task.title))?;
        let row = build_row(task, next, &mut taken)?;
        next = next.checked_add(1).ok_or_else(|| {
            tagged_err(
                ErrorKind::Validation,
                None,
                "task ids are exhausted".to_string(),
            )
        })?;
        minted.push(row);
    }

    let nodes: Vec<Node> = store
        .items
        .iter()
        .chain(minted.iter())
        .map(Node::from)
        .collect();
    // `Graph::build` is the dangling-target check: it names the referring task
    // and the absent one.
    let graph = build_or_refuse(&nodes, "to add", Tense::Candidate)?;

    let rounds = graph.kahn_rounds();
    let outcomes = minted
        .iter()
        .map(|row| AddOutcome {
            id: row.id,
            r#ref: row.r#ref.clone(),
            batch: rounds
                .iter()
                .position(|round| round.contains(&row.id))
                .expect("an acyclic graph emits every task in a round"),
            advisory: store.spec_for(&row.r#ref).and_then(discarded_spec_advisory),
        })
        .collect();

    let known: BTreeSet<u32> = store
        .items
        .iter()
        .chain(minted.iter())
        .map(|row| row.id)
        .collect();
    for row_extras in &extras {
        if let Some(premises) = row_extras
            .spec
            .as_ref()
            .and_then(|spec| spec.premises.as_deref())
        {
            reject_unknown_premise_targets(row_extras.line, premises, &known)?;
        }
    }

    // Each minted `ref` is free of rows, so anything keyed on it is a removed
    // row's leftover.
    for row in &minted {
        store.drop_ref_entries(&row.r#ref);
    }
    for (row, row_extras) in minted.iter().zip(extras) {
        row_extras.land(row, store)?;
    }
    // A store no import has written has no ref set to be absent from.
    if !store.last_import_refs.is_empty() {
        store
            .minted_refs
            .extend(minted.iter().map(|row| row.r#ref.clone()));
    }
    store.items.extend(minted);
    Ok(outcomes)
}

/// No row holds a minted `ref`, so a spec keyed on it is an orphan. Its outcome
/// and accept text are quoted because this line is their last copy.
fn discarded_spec_advisory(spec: &TaskSpec) -> Option<String> {
    let mut discarded = Vec::new();
    if !spec.outcome.trim().is_empty() {
        discarded.push(format!("outcome \"{}\"", spec.outcome));
    }
    if !spec.accept.is_empty() {
        let behaviours: Vec<String> = spec
            .accept
            .iter()
            .map(|accept| format!("\"{}\"", accept.behaviour))
            .collect();
        discarded.push(format!("accept {}", behaviours.join(", ")));
    }
    for (field, empty) in [
        ("constraints", spec.constraints.is_empty()),
        ("premises", spec.premises.is_empty()),
        ("means", spec.means.is_empty()),
        ("design", spec.design.is_empty()),
        ("precedent", spec.precedent.is_empty()),
        ("executor", spec.executor.is_empty()),
    ] {
        if !empty {
            discarded.push(field.to_string());
        }
    }
    if discarded.is_empty() {
        return None;
    }
    Some(format!(
        "`{}` took over an orphan spec and discarded its {}",
        spec.r#ref,
        discarded.join("; ")
    ))
}

/// A premise's `after:<id>` must name a row. Ids only exist once a batch is
/// minted, so this runs under the lock where `premise_entry`'s format check
/// cannot.
pub(super) fn reject_unknown_premise_targets(
    line: usize,
    premises: &[Premise],
    known: &BTreeSet<u32>,
) -> Result<()> {
    for premise in premises {
        if let Some(id) = premise_target(&premise.at)
            && !known.contains(&id)
        {
            return Err(row_err(
                line,
                format!(
                    "`spec.premises` entry `at = \"{}\"` names no row in the store or this batch",
                    premise.at
                ),
            ));
        }
    }
    Ok(())
}

impl RowExtras {
    /// Runs after `drop_ref_entries`, which would otherwise take these with
    /// the leftovers under the same `ref`.
    fn land(self, row: &TaskRow, store: &mut Store) -> Result<()> {
        let Self {
            line: _,
            spec,
            reason,
            actor,
            file_notes,
            closes,
            refs,
            finding_link,
        } = self;

        if let Some(patch) = spec {
            let mut entry = TaskSpec {
                r#ref: row.r#ref.clone(),
                outcome: String::new(),
                constraints: Vec::new(),
                accept: Vec::new(),
                premises: Vec::new(),
                means: Vec::new(),
                design: Vec::new(),
                precedent: Vec::new(),
                executor: String::new(),
            };
            patch.apply_to(&mut entry);
            store.specs.push(entry);
        }
        for (file, note) in file_notes {
            store.file_notes.push(FileNote {
                r#ref: row.r#ref.clone(),
                file,
                note,
            });
        }
        if !closes.is_empty() || !refs.is_empty() {
            store.backlog_links.push(BacklogLink {
                r#ref: row.r#ref.clone(),
                closes,
                refs,
            });
        }
        if let Some(link) = finding_link {
            store.finding_links.push(FindingLink {
                r#ref: row.r#ref.clone(),
                ..link
            });
        }
        if store.audit {
            store.changes.push(Change {
                at: Some(crate::time::now_toml_datetime()?),
                r#ref: row.r#ref.clone(),
                field: "*".to_string(),
                old: String::new(),
                new: row.title.clone(),
                actor: match actor.trim().is_empty() {
                    true => DEFAULT_ACTOR.to_string(),
                    false => actor,
                },
                reason,
            });
        }
        Ok(())
    }
}

impl SpecPatch {
    pub(super) fn parse(line: usize, raw: &JsonValue) -> Result<Self> {
        let spec = SpecObject::of(line, "spec".to_string(), raw)?;
        spec.refuse_unknown(&SPEC_FIELDS)?;
        Ok(Self {
            outcome: spec.text("outcome")?,
            constraints: spec.strings("constraints")?,
            accept: spec.entries("accept", accept_entry)?,
            premises: spec.entries("premises", premise_entry)?,
            means: spec.entries("means", means_entry)?,
            design: spec.strings("design")?,
            precedent: spec.strings("precedent")?,
            executor: spec.one_of("executor", &EXECUTORS)?,
        })
    }

    /// Each supplied field replaces `spec`'s; the rest stay as they are.
    pub(super) fn apply_to(self, spec: &mut TaskSpec) {
        let Self {
            outcome,
            constraints,
            accept,
            premises,
            means,
            design,
            precedent,
            executor,
        } = self;
        replace(&mut spec.outcome, outcome);
        replace(&mut spec.constraints, constraints);
        replace(&mut spec.accept, accept);
        replace(&mut spec.premises, premises);
        replace(&mut spec.means, means);
        replace(&mut spec.design, design);
        replace(&mut spec.precedent, precedent);
        replace(&mut spec.executor, executor);
    }
}

fn replace<T>(slot: &mut T, value: Option<T>) {
    if let Some(value) = value {
        *slot = value;
    }
}

/// A table inside a row's `spec`, carrying its row and its path for a refusal.
/// `null` reads as absent, as it does on the row.
struct SpecObject<'a> {
    line: usize,
    label: String,
    map: &'a JsonMap<String, JsonValue>,
}

impl<'a> SpecObject<'a> {
    fn of(line: usize, label: String, raw: &'a JsonValue) -> Result<Self> {
        let map = raw.as_object().ok_or_else(|| {
            row_err(
                line,
                format!("`{label}` must be a table, got {}", json_type_name(raw)),
            )
        })?;
        Ok(Self { line, label, map })
    }

    fn get(&self, key: &str) -> Option<&'a JsonValue> {
        self.map.get(key).filter(|value| !value.is_null())
    }

    fn path(&self, key: &str) -> String {
        format!("{}.{key}", self.label)
    }

    fn refuse(&self, message: String) -> anyhow::Error {
        row_err(self.line, message)
    }

    fn refuse_unknown(&self, accepted: &[&str]) -> Result<()> {
        match self
            .map
            .keys()
            .find(|key| !accepted.contains(&key.as_str()))
        {
            None => Ok(()),
            Some(key) => Err(self.refuse(format!(
                "unknown field `{key}` in `{}` — accepted fields are {}",
                self.label,
                accepted.join(", ")
            ))),
        }
    }

    fn text(&self, key: &str) -> Result<Option<String>> {
        row_string(self.line, &self.path(key), self.get(key))
    }

    fn strings(&self, key: &str) -> Result<Option<Vec<String>>> {
        row_strings(self.line, &self.path(key), self.get(key))
    }

    fn one_of(&self, key: &str, vocabulary: &[&str]) -> Result<Option<String>> {
        let value = self.text(key)?;
        match &value {
            Some(raw) if !vocabulary.contains(&raw.as_str()) => Err(self.refuse(format!(
                "`{}` is `{raw}` — expected one of {}",
                self.path(key),
                vocabulary.join(", ")
            ))),
            _ => Ok(value),
        }
    }

    fn entries<T>(
        &self,
        key: &str,
        parse: impl Fn(&SpecObject<'a>) -> Result<T>,
    ) -> Result<Option<Vec<T>>> {
        let Some(raw) = self.get(key) else {
            return Ok(None);
        };
        let entries = raw.as_array().ok_or_else(|| {
            self.refuse(format!(
                "`{}` must be an array of tables, got {}",
                self.path(key),
                json_type_name(raw)
            ))
        })?;
        entries
            .iter()
            .enumerate()
            .map(|(index, entry)| {
                let label = format!("{}[{index}]", self.path(key));
                parse(&SpecObject::of(self.line, label, entry)?)
            })
            .collect::<Result<Vec<_>>>()
            .map(Some)
    }
}

/// A `kind` may stay off only while `check` is empty: a deferred milestone's
/// row states its behaviour before anything exists to label.
fn accept_entry(entry: &SpecObject<'_>) -> Result<Accept> {
    if entry.map.contains_key("probe") {
        return Err(entry.refuse(format!(
            "`{}` cannot be supplied — only `plan probes --record` writes it",
            entry.path("probe")
        )));
    }
    entry.refuse_unknown(&ACCEPT_FIELDS)?;
    let check = entry.text("check")?.unwrap_or_default();
    // A check-less entry is stored, and shown, as `kind = ""`; that form reads
    // back as an absent kind so a stored list round-trips through the parser.
    let unlabelled = check.trim().is_empty() && entry.text("kind")?.as_deref() == Some("");
    let kind = match (!unlabelled)
        .then(|| entry.one_of("kind", &ACCEPT_KINDS))
        .transpose()?
        .flatten()
    {
        Some(kind) => kind,
        None if check.trim().is_empty() => String::new(),
        None => {
            return Err(entry.refuse(format!(
                "`{}` has a `check` but no `kind` — expected one of {}",
                entry.label,
                ACCEPT_KINDS.join(", ")
            )));
        }
    };
    let writes_build = match entry.get("writes_build") {
        None => true,
        Some(JsonValue::Bool(flag)) => *flag,
        Some(other) => {
            return Err(entry.refuse(format!(
                "`{}` must be a boolean, got {}",
                entry.path("writes_build"),
                json_type_name(other)
            )));
        }
    };
    let accept = Accept {
        behaviour: entry.text("behaviour")?.unwrap_or_default(),
        check,
        kind,
        positive: entry.text("positive")?.unwrap_or_default(),
        writes_build,
        run_by: entry.one_of("run_by", &RUN_BY_OWNERS)?.unwrap_or_default(),
        covered_by: entry.text("covered_by")?.unwrap_or_default(),
        probe: None,
    };
    match accept.owner_problem() {
        Some(problem) => Err(entry.refuse(format!("`{}`: {problem}", entry.label))),
        None => Ok(accept),
    }
}

fn premise_entry(entry: &SpecObject<'_>) -> Result<Premise> {
    if entry.map.contains_key("probe") {
        return Err(entry.refuse(format!(
            "`{}` cannot be supplied — only `plan probes --record --premises` writes it",
            entry.path("probe")
        )));
    }
    entry.refuse_unknown(&PREMISE_FIELDS)?;
    let at = entry.text("at")?.unwrap_or_default();
    if at != "base" && premise_target(&at).is_none() {
        return Err(entry.refuse(format!(
            "`{}` is `{at}` — expected `base` or `after:<id>`",
            entry.path("at")
        )));
    }
    Ok(Premise {
        claim: entry.text("claim")?.unwrap_or_default(),
        check: entry.text("check")?.unwrap_or_default(),
        at,
        probe: None,
    })
}

fn means_entry(entry: &SpecObject<'_>) -> Result<Means> {
    entry.refuse_unknown(&MEANS_FIELDS)?;
    let Some(binding) = entry.one_of("binding", &MEANS_BINDINGS)? else {
        return Err(entry.refuse(format!(
            "`{}` has no `binding` — expected one of {}",
            entry.label,
            MEANS_BINDINGS.join(", ")
        )));
    };
    Ok(Means {
        text: entry.text("text")?.unwrap_or_default(),
        binding,
        detail: entry.text("detail")?.unwrap_or_default(),
    })
}

/// A grouping key must denote a group that exists. Every checkpoint product
/// selects members by exact match, so a key no `[[checkpoints]]` entry declares
/// puts the row in no closure and no drain, with nothing downstream reporting
/// it missing. An empty key is the "no group" spelling and stays legal.
pub(super) fn reject_undeclared_checkpoint(
    store: &Store,
    checkpoint: &str,
    subject: &str,
) -> Result<()> {
    if checkpoint.is_empty()
        || store
            .checkpoints
            .iter()
            .any(|declared| declared.id == checkpoint)
    {
        return Ok(());
    }
    let declared: Vec<&str> = store
        .checkpoints
        .iter()
        .map(|checkpoint| checkpoint.id.as_str())
        .collect();
    let known = if declared.is_empty() {
        "the store declares no checkpoint group".to_string()
    } else {
        format!("declared groups are {}", declared.join(", "))
    };
    Err(tagged_err(
        ErrorKind::Validation,
        None,
        format!("{subject}: unknown checkpoint group `{checkpoint}` — {known}"),
    ))
}

fn build_row(task: NewTask, id: u32, taken: &mut BTreeSet<String>) -> Result<TaskRow> {
    let NewTask {
        title,
        effort,
        files,
        needs,
        coupling,
        deps_note,
        checkpoint,
        action,
        detail,
        acceptance,
    } = task;

    if title.trim().is_empty() {
        return Err(tagged_err(
            ErrorKind::Validation,
            None,
            "a task needs a non-empty `title`".to_string(),
        ));
    }
    let effort = Effort::parse(&effort).ok_or_else(|| {
        tagged_err(
            ErrorKind::Validation,
            None,
            format!(
                "task `{title}`: unknown effort `{effort}` — expected one of {}",
                Effort::VOCABULARY.join(", ")
            ),
        )
    })?;
    let derived = slug::derive_ref(&title);
    if derived.is_empty() {
        return Err(tagged_err(
            ErrorKind::Validation,
            None,
            format!("task `{title}`: the title carries no slug characters, so `ref` is empty"),
        ));
    }
    let r#ref = unique_ref(derived, taken);
    taken.insert(r#ref.clone());

    Ok(TaskRow {
        id,
        r#ref,
        title,
        effort,
        status: Status::default(),
        checkpoint,
        // Heading structure is authored in the plan and re-read by every
        // import, so a value set here would not outlive the next one.
        phase: String::new(),
        phase_depth: 0,
        heading_depth: DEFAULT_HEADING_DEPTH,
        files,
        needs,
        coupling,
        deps_note,
        action,
        detail,
        acceptance,
        agent: String::new(),
        commit: String::new(),
    })
}

/// `slug::dedupe_refs` numbers a whole document in one pass and so cannot see
/// what an append is landing beside — replaying it over `existing ++ new`
/// re-derives a suffix another row already holds. Scanning for the first free
/// suffix keeps that function's `<ref>-2` spelling without the collision.
fn unique_ref(derived: String, taken: &BTreeSet<String>) -> String {
    if !taken.contains(&derived) {
        return derived;
    }
    for n in 2u32.. {
        let candidate = format!("{derived}-{n}");
        if !taken.contains(&candidate) {
            return candidate;
        }
    }
    unreachable!("the suffix search is unbounded")
}

fn task_from_row(line: usize, row: &JsonMap<String, JsonValue>) -> Result<NewTask> {
    for key in row.keys() {
        if !ROW_FIELDS.contains(&key.as_str()) {
            return Err(row_err(
                line,
                format!(
                    "unknown field `{key}` — accepted fields are {}",
                    ROW_FIELDS.join(", ")
                ),
            ));
        }
    }

    // A row reads `null` as an absent field.
    let present = |key: &str| row.get(key).filter(|v| !v.is_null());
    let text = |key: &str| row_string(line, key, present(key)).map(Option::unwrap_or_default);
    let ids = |key: &str| row_ids(line, key, present(key)).map(Option::unwrap_or_default);
    Ok(NewTask {
        title: text("title")?,
        effort: text("effort")?,
        files: row_strings(line, "files", present("files"))?.unwrap_or_default(),
        needs: ids("needs")?,
        coupling: ids("coupling")?,
        deps_note: text("deps_note")?,
        checkpoint: text("checkpoint")?,
        action: text("action")?,
        detail: text("detail")?,
        acceptance: text("acceptance")?,
    })
}

fn extras_from_row(
    line: usize,
    row: &JsonMap<String, JsonValue>,
    files: &[String],
) -> Result<RowExtras> {
    let present = |key: &str| row.get(key).filter(|v| !v.is_null());
    let text = |key: &str| row_string(line, key, present(key)).map(Option::unwrap_or_default);
    let strings = |key: &str| row_strings(line, key, present(key)).map(Option::unwrap_or_default);
    let file_notes = file_notes_from_row(line, present("file_notes"))?;
    reject_unclaimed_notes(line, &file_notes, files)?;
    Ok(RowExtras {
        line,
        spec: present("spec")
            .map(|raw| SpecPatch::parse(line, raw))
            .transpose()?,
        reason: text("reason")?,
        actor: text("actor")?,
        file_notes,
        closes: strings("closes")?,
        refs: strings("refs")?,
        finding_link: finding_link_from_row(
            line,
            text("review_store")?,
            strings("fixes")?,
            strings("settles")?,
        )?,
    })
}

/// Finding ids mean nothing without the store that holds them, so ids without
/// a `review_store` are refused. A `review_store` naming no finding links
/// nothing and is not stored.
fn finding_link_from_row(
    line: usize,
    review_store: String,
    fixes: Vec<String>,
    settles: Vec<String>,
) -> Result<Option<FindingLink>> {
    if fixes.is_empty() && settles.is_empty() {
        return Ok(None);
    }
    if review_store.trim().is_empty() {
        return Err(row_err(
            line,
            "`fixes` and `settles` name findings in a review store — the row needs a \
             `review_store`",
        ));
    }
    for (key, ids) in [("fixes", &fixes), ("settles", &settles)] {
        if ids.iter().any(|id| id.trim().is_empty()) {
            return Err(row_err(line, format!("`{key}` holds a blank finding id")));
        }
    }
    Ok(Some(FindingLink {
        r#ref: String::new(),
        store: review_store,
        fixes,
        settles,
    }))
}

/// A blank annotation is refused rather than stored, because the store drops
/// such an entry on write and the caller would never learn it was lost.
pub(super) fn file_notes_from_row(
    line: usize,
    value: Option<&JsonValue>,
) -> Result<Vec<(String, String)>> {
    let Some(raw) = value else {
        return Ok(Vec::new());
    };
    let notes = raw.as_object().ok_or_else(|| {
        row_err(
            line,
            format!(
                "`file_notes` must map a path to its annotation, got {}",
                json_type_name(raw)
            ),
        )
    })?;
    notes
        .iter()
        .map(|(file, note)| {
            let note = row_string(line, "file_notes", Some(note))?.unwrap_or_default();
            if note.trim().is_empty() {
                return Err(row_err(
                    line,
                    format!("`file_notes` gives `{file}` a blank annotation"),
                ));
            }
            Ok((file.clone(), note))
        })
        .collect()
}

pub(super) fn reject_unclaimed_notes(
    line: usize,
    notes: &[(String, String)],
    files: &[String],
) -> Result<()> {
    match notes.iter().find(|(file, _)| !files.contains(file)) {
        Some((file, _)) => Err(row_err(
            line,
            format!("`file_notes` names `{file}`, which is not on the row's `files`"),
        )),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::with_root;

    fn write_args() -> WriteIntegrityArgs {
        WriteIntegrityArgs {
            allow_outside: false,
            no_write_integrity: false,
            verify_integrity: false,
            strict_integrity: false,
            no_create: false,
        }
    }

    fn store_path(root: &Path) -> PathBuf {
        root.join(".claude")
            .join("flows")
            .join("whimsical-hugging-puppy")
            .join("tasks.toml")
    }

    fn task(title: &str, needs: &[u32]) -> NewTask {
        NewTask {
            title: title.to_string(),
            effort: "S".to_string(),
            files: Vec::new(),
            needs: needs.to_vec(),
            coupling: Vec::new(),
            deps_note: String::new(),
            checkpoint: String::new(),
            action: String::new(),
            detail: String::new(),
            acceptance: String::new(),
        }
    }

    /// Two rows, ids 1 and 2, with 2 depending on 1.
    fn seeded(path: &Path) {
        add(path, &write_args(), task("Seed the store", &[])).expect("first row lands");
        add(path, &write_args(), task("Wire the renderer", &[1])).expect("second row lands");
    }

    #[test]
    fn an_absent_dependency_target_is_named() {
        with_root(|root| {
            let path = store_path(root);
            let message = add(&path, &write_args(), task("Dangling", &[99]))
                .expect_err("99 does not exist")
                .to_string();
            assert!(message.contains("99"), "{message}");
            assert!(!path.exists(), "a refused add must persist nothing");
        });
    }

    #[test]
    fn a_cycle_is_refused_and_the_file_is_untouched() {
        with_root(|root| {
            let path = store_path(root);
            seeded(&path);
            let before = fs::read(&path).expect("the seeded store is on disk");

            // The next id is 3, so `--needs 3` closes a loop onto the row
            // being added.
            let message = add(&path, &write_args(), task("Self referential", &[3]))
                .expect_err("a self-edge is a cycle")
                .to_string();
            assert!(message.contains("cycle"), "{message}");
            assert!(message.contains('3'), "{message}");

            assert_eq!(
                fs::read(&path).expect("the store is still on disk"),
                before,
                "validation must run before the write"
            );
        });
    }

    #[test]
    fn a_batch_closing_a_cycle_between_its_own_rows_is_refused() {
        with_root(|root| {
            let path = store_path(root);
            seeded(&path);
            let before = fs::read(&path).expect("the seeded store is on disk");

            let ndjson = "{\"title\":\"Alpha\",\"effort\":\"S\",\"needs\":[4]}\n\
                          {\"title\":\"Beta\",\"effort\":\"S\",\"needs\":[3]}\n";
            let message = add_many(&path, &write_args(), ndjson)
                .expect_err("3 and 4 depend on each other")
                .to_string();
            assert!(message.contains("cycle"), "{message}");
            assert!(message.contains("3, 4"), "{message}");

            assert_eq!(
                fs::read(&path).expect("the store is still on disk"),
                before,
                "an all-or-nothing batch must not half-land"
            );
        });
    }

    #[test]
    fn a_batch_lands_once_and_reports_each_rows_round() {
        with_root(|root| {
            let path = store_path(root);
            let ndjson = "{\"title\":\"Alpha\",\"effort\":\"S\"}\n\
                          {\"title\":\"Beta\",\"effort\":\"M\",\"needs\":[1]}\n";
            let outcomes = add_many(&path, &write_args(), ndjson).expect("the batch lands");

            assert_eq!(
                outcomes
                    .iter()
                    .map(|o| (o.id, o.r#ref.as_str(), o.batch))
                    .collect::<Vec<_>>(),
                vec![(1, "alpha", 0), (2, "beta", 1)]
            );

            let store = store::load(
                &path,
                &crate::cli::ReadIntegrityArgs {
                    verify_integrity: true,
                    strict_read: false,
                },
            )
            .expect("the store loads");
            assert_eq!(store.items.len(), 2);
            assert_eq!(store.items[1].effort, Effort::M);
            assert_eq!(store.items[1].status, Status::Pending);
        });
    }

    #[test]
    fn a_repeated_title_takes_the_next_free_suffix() {
        with_root(|root| {
            let path = store_path(root);
            let first = add(&path, &write_args(), task("Same title", &[])).expect("first lands");
            let second = add(&path, &write_args(), task("Same title", &[])).expect("second lands");
            let third = add(&path, &write_args(), task("Same title", &[])).expect("third lands");
            assert_eq!(
                (
                    first.r#ref.as_str(),
                    second.r#ref.as_str(),
                    third.r#ref.as_str()
                ),
                ("same-title", "same-title-2", "same-title-3")
            );
        });
    }

    /// A removed row's notes and links outlive it, so a new row minted onto
    /// its `ref` must not inherit them.
    #[test]
    fn a_new_row_drops_entries_left_under_its_ref() {
        use super::super::schema::{BacklogLink, FileNote};

        with_root(|root| {
            let path = store_path(root);
            seeded(&path);
            store::mutate(&path, &write_args(), |store| {
                for r#ref in ["seed-the-store", "same-title"] {
                    store.file_notes.push(FileNote {
                        r#ref: r#ref.to_string(),
                        file: "src/a.rs".to_string(),
                        note: "(new)".to_string(),
                    });
                    store.backlog_links.push(BacklogLink {
                        r#ref: r#ref.to_string(),
                        closes: vec!["B1".to_string()],
                        refs: Vec::new(),
                    });
                }
                Ok(())
            })
            .expect("the leftovers land");

            let added = add(&path, &write_args(), task("Same title", &[])).expect("it lands");
            assert_eq!(added.r#ref, "same-title");

            let store = store::load(
                &path,
                &crate::cli::ReadIntegrityArgs {
                    verify_integrity: true,
                    strict_read: false,
                },
            )
            .expect("the store loads");
            let note_refs: Vec<&str> = store.file_notes.iter().map(|n| n.r#ref.as_str()).collect();
            let link_refs: Vec<&str> = store
                .backlog_links
                .iter()
                .map(|l| l.r#ref.as_str())
                .collect();
            assert_eq!(note_refs, ["seed-the-store"]);
            assert_eq!(link_refs, ["seed-the-store"]);
        });
    }

    #[test]
    fn an_unknown_ndjson_field_is_refused_by_line() {
        with_root(|root| {
            let path = store_path(root);
            let message = add_many(
                &path,
                &write_args(),
                "{\"title\":\"Alpha\",\"effort\":\"S\"}\n{\"title\":\"Beta\",\"effort\":\"S\",\"neds\":[1]}\n",
            )
            .expect_err("`neds` is not a row field")
            .to_string();
            assert!(message.starts_with("line 2: "), "{message}");
            assert!(message.contains("neds"), "{message}");
            assert!(!path.exists(), "a refused batch must persist nothing");
        });
    }

    #[test]
    fn a_body_flag_wins_over_its_file_and_neither_is_blank() {
        let dir = tempfile::tempdir().expect("a scratch dir");
        let file = dir.path().join("action.md");
        fs::write(&file, "from the file").expect("the body file is written");

        assert_eq!(
            body(Some("literal".to_string()), Some(file.clone())).expect("the literal wins"),
            "literal"
        );
        assert_eq!(
            body(None, Some(file)).expect("the file is read"),
            "from the file"
        );
        assert_eq!(body(None, None).expect("both absent"), "");
        assert!(body(None, Some(dir.path().join("absent.md"))).is_err());
    }

    #[test]
    fn a_checkpoint_group_the_store_does_not_declare_is_refused() {
        with_root(|root| {
            let path = store_path(root);
            let mut stray = task("Into a group that is not there", &[]);
            stray.checkpoint = "Z".to_string();
            let message = add(&path, &write_args(), stray)
                .expect_err("`Z` is declared nowhere")
                .to_string();
            assert!(message.contains('Z'), "{message}");
            assert!(!path.exists(), "a refused add must persist nothing");
        });
    }

    #[test]
    fn an_unknown_effort_names_the_vocabulary() {
        with_root(|root| {
            let path = store_path(root);
            let mut bad = task("Lowercase effort", &[]);
            bad.effort = "s".to_string();
            let message = add(&path, &write_args(), bad)
                .expect_err("effort is case-sensitive")
                .to_string();
            assert!(message.contains("S, M, L"), "{message}");
        });
    }
}
