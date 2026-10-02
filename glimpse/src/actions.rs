//! The status transitions and classify edits a ledger offers, built into forms and turned into write requests.
//!
//! Every request carries the values glimpse displayed as its compare-and-set guard, and every
//! edit records how to put each row back, so undo restores exactly the fields a write touched.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value};
use tomlctl::{BacklogTriage, LedgerKind, LedgerRef};
use tui_input::Input;

use crate::form::{Field, FieldValue, Form};
use crate::ledger::{InputRow, ItemRow, Kind};
use crate::surface::Surface;
use crate::writer::{RequestId, WriteRequest};

/// One companion text field of a transition's form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Companion {
    pub(crate) field: &'static str,
    pub(crate) required: bool,
}

/// A status a row can be moved to, with the fields its form asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Transition {
    pub(crate) to: &'static str,
    pub(crate) fields: &'static [Companion],
}

const fn required(field: &'static str) -> Companion {
    Companion {
        field,
        required: true,
    }
}

const DEFER: Transition = Transition {
    to: "deferred",
    fields: &[required("defer_reason"), required("defer_trigger")],
};
const WONTFIX: Transition = Transition {
    to: "wontfix",
    fields: &[required("wontfix_rationale")],
};
const VERIFIED: Transition = Transition {
    to: "verified-clean",
    fields: &[required("verified_note")],
};
const WONTAPPLY: Transition = Transition {
    to: "wontapply",
    fields: &[required("wontapply_rationale")],
};
const DISCARD: Transition = Transition {
    to: "discarded",
    fields: &[Companion {
        field: "discard_reason",
        required: false,
    }],
};
const REOPEN: Transition = Transition {
    to: "open",
    fields: &[required("reopen_rationale")],
};
const DISMISS: Transition = Transition {
    to: "dismissed",
    fields: &[required("dismiss_reason")],
};
const RESOLVE: Transition = Transition {
    to: "resolved",
    fields: &[required("resolution")],
};

/// The transitions glimpse offers away from `from` on a `kind` ledger. The apply flows and
/// the plan merge own `fixed`, `applied` and `merged`.
pub(crate) fn offered(kind: Kind, from: &str) -> &'static [Transition] {
    match (kind, from) {
        (Kind::Review, "open") => &[DEFER, WONTFIX, VERIFIED],
        (Kind::Optimise, "open") => &[DEFER, WONTAPPLY],
        (Kind::Review | Kind::Optimise, "deferred") => &[REOPEN],
        (Kind::PlanReview, "open") => &[DISCARD],
        (Kind::Backlog, "open") => &[DISMISS, RESOLVE],
        (Kind::Backlog, "dismissed" | "resolved") => &[REOPEN],
        _ => &[],
    }
}

/// The transitions offered from every one of `statuses`, in the order the first offers them.
pub(crate) fn common<'a>(
    kind: Kind,
    statuses: impl IntoIterator<Item = &'a str>,
) -> Vec<Transition> {
    let mut statuses = statuses.into_iter();
    let Some(first) = statuses.next() else {
        return Vec::new();
    };
    let mut shared = offered(kind, first).to_vec();
    for status in statuses {
        let here = offered(kind, status);
        shared.retain(|t| here.contains(t));
    }
    shared
}

/// The statuses whose move away asks for confirmation when a target is `critical`.
const DECLINED: [&str; 4] = ["wontfix", "wontapply", "discarded", "dismissed"];

/// The companion fields a transition away from `status` removes, as the facade drops them,
/// so undo knows every field a transition can take away.
fn leaves(status: &str) -> &'static [&'static str] {
    match status {
        "deferred" => &["defer_reason", "defer_trigger"],
        "wontfix" => &["wontfix_rationale"],
        "wontapply" => &["wontapply_rationale"],
        "verified-clean" => &["verified_note"],
        "discarded" => &["discard_reason"],
        _ => &[],
    }
}

/// Every field a backlog triage rewrites; a move clears the ones the new status does not own.
const BACKLOG_MANAGED: [&str; 10] = [
    "promoted",
    "promoted_to",
    "dismissed",
    "dismiss_reason",
    "resolved",
    "resolution",
    "reopen_rationale",
    "resolved_flow",
    "resolved_tasks",
    "resolved_commits",
];

/// The terminal date a backlog move stamps, which glimpse cannot predict and so never guards.
fn backlog_stamp(to: &str) -> Option<&'static str> {
    match to {
        "dismissed" => Some("dismissed"),
        "resolved" => Some("resolved"),
        _ => None,
    }
}

const SEVERITIES: [&str; 3] = ["critical", "warning", "suggestion"];
const EFFORTS: [&str; 3] = ["trivial", "small", "medium"];
const REVIEW_CATEGORIES: [&str; 7] = [
    "quality",
    "security",
    "architecture",
    "completeness",
    "db",
    "testability",
    "package-quality",
];
const OPTIMISE_CATEGORIES: [&str; 5] = [
    "memory",
    "serialization",
    "query",
    "algorithm",
    "concurrency",
];
pub(crate) const CLASSIFY_FIELDS: [&str; 3] = ["severity", "effort", "category"];
/// Offered first by a classify field the targets disagree on, so submitting leaves each as it is.
pub(crate) const KEEP: &str = "(unchanged)";

/// The ledger behind a displayed `path`, or `None` for one named by path (`--once`), which
/// is read-only. Writes go to exactly the file whose rows are on screen.
pub(crate) fn ledger_ref(kind: Kind, path: &str) -> Option<LedgerRef> {
    let kind = match kind {
        Kind::Backlog => return (path == ".claude/backlog.toml").then_some(LedgerRef::Backlog),
        Kind::Review => LedgerKind::Review,
        Kind::Optimise => LedgerKind::Optimise,
        Kind::PlanReview => LedgerKind::PlanReview,
    };
    let (flow_file, scope_dir) = match kind {
        LedgerKind::Review => ("review-ledger.toml", "reviews"),
        LedgerKind::Optimise => ("optimise-findings.toml", "optimise-findings"),
        LedgerKind::PlanReview => ("plan-review-findings.toml", "plan-review-findings"),
    };
    if let Some(rest) = path.strip_prefix(".claude/flows/") {
        let (slug, file) = rest.split_once('/')?;
        return (file == flow_file).then(|| LedgerRef::Flow {
            slug: slug.to_owned(),
            kind,
        });
    }
    let scope = path
        .strip_prefix(".claude/")?
        .strip_prefix(scope_dir)?
        .strip_prefix('/')?
        .strip_suffix(".toml")?;
    (!scope.is_empty() && !scope.contains('/')).then(|| LedgerRef::Scope {
        kind,
        scope: scope.to_owned(),
    })
}

/// The rows an action acts on, copied when it opened, so the guard holds what was on screen then.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Selection {
    pub(crate) surface: Surface,
    pub(crate) kind: Kind,
    pub(crate) ledger: LedgerRef,
    pub(crate) rows: Vec<ItemRow>,
}

impl Selection {
    fn label(&self) -> String {
        match self.rows.as_slice() {
            [row] => row.id.clone(),
            rows => format!("{} items", rows.len()),
        }
    }

    /// The distinct displayed statuses, in order.
    pub(crate) fn statuses(&self) -> BTreeSet<&str> {
        self.rows.iter().map(|r| r.status.as_str()).collect()
    }

    pub(crate) fn transitions(&self) -> Vec<Transition> {
        common(self.kind, self.statuses())
    }

    /// The `critical` targets a move to `to` would decline, which ask for confirmation first.
    pub(crate) fn critical_declines(&self, to: &str) -> Vec<String> {
        if !DECLINED.contains(&to) {
            return Vec::new();
        }
        self.rows
            .iter()
            .filter(|r| r.severity == "critical")
            .map(|r| r.id.clone())
            .collect()
    }
}

/// What the open form will do once submitted.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Purpose {
    /// A yes/no before a critical row is declined; yes opens the transition's form.
    Confirm(Selection, Transition),
    Transition(Selection, Transition),
    Classify(Selection),
}

/// The modal on top of the surfaces. While one is open it takes every key.
#[derive(Debug, Clone)]
pub(crate) enum Overlay {
    /// The action menu: a lone open select whose options are `choices`, by `to`.
    Menu {
        selection: Selection,
        choices: Vec<Transition>,
        form: Form,
    },
    Form {
        purpose: Purpose,
        form: Form,
    },
    /// The live filter; `before` is the filter to restore on cancel.
    Prompt {
        input: Input,
        before: String,
    },
}

impl Overlay {
    /// The form a view draws, `None` for the filter prompt.
    pub(crate) fn form(&self) -> Option<&Form> {
        match self {
            Overlay::Menu { form, .. } | Overlay::Form { form, .. } => Some(form),
            Overlay::Prompt { .. } => None,
        }
    }
}

pub(crate) fn menu(selection: Selection) -> Overlay {
    let choices = selection.transitions();
    let options = choices.iter().map(|t| t.to.to_owned()).collect();
    let form = Form::new(
        format!("{}: {}", selection.label(), status_line(&selection)),
        vec![Field::select("move to", options).opened()],
    );
    Overlay::Menu {
        selection,
        choices,
        form,
    }
}

fn status_line(selection: &Selection) -> String {
    selection
        .statuses()
        .into_iter()
        .collect::<Vec<_>>()
        .join(", ")
}

pub(crate) fn confirm_form(selection: &Selection, to: &str) -> Form {
    let critical = selection.critical_declines(to).join(", ");
    Form::new(
        format!("{} → {to}", selection.label()),
        vec![Field::select("move anyway?", vec!["no".into(), "yes".into()]).opened()],
    )
    .with_prompt(format!("{critical} is critical"))
}

pub(crate) fn transition_form(selection: &Selection, transition: Transition) -> Form {
    let fields = transition
        .fields
        .iter()
        .map(|c| {
            let field = Field::text(c.field);
            if c.required { field.required() } else { field }
        })
        .collect();
    Form::new(format!("{} → {}", selection.label(), transition.to), fields)
}

/// Prefilled with the value every target shares; a field they disagree on starts at [`KEEP`].
pub(crate) fn classify_form(selection: &Selection) -> Form {
    let categories: &[&str] = match selection.kind {
        Kind::Optimise => &OPTIMISE_CATEGORIES,
        _ => &REVIEW_CATEGORIES,
    };
    let vocab: [(&str, &[&str]); 3] = [
        ("severity", &SEVERITIES),
        ("effort", &EFFORTS),
        ("category", categories),
    ];
    let fields = vocab
        .into_iter()
        .map(|(name, options)| {
            let shared = shared_value(selection, name);
            let mut options: Vec<String> = options.iter().map(|o| (*o).to_owned()).collect();
            if shared.is_none() {
                options.insert(0, KEEP.to_owned());
            }
            let mut field = Field::select(name, options);
            if name == "category" {
                field = field.with_other();
            }
            match shared {
                Some(value) => field.with_value(&[value.as_str()]),
                None => field,
            }
            .required()
        })
        .collect();
    Form::new(format!("classify {}", selection.label()), fields)
}

fn raw_str(row: &ItemRow, field: &str) -> String {
    row.raw
        .get(field)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned()
}

fn shared_value(selection: &Selection, field: &str) -> Option<String> {
    let mut values = selection.rows.iter().map(|r| raw_str(r, field));
    let first = values.next()?;
    (!first.is_empty() && values.all(|v| v == first)).then_some(first)
}

/// How to put one row back as it was: `set` restores, `unset` removes what the write added,
/// guarded on the row still holding `expect`, the values the write left.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RowUndo {
    pub(crate) id: String,
    pub(crate) set: Map<String, Value>,
    pub(crate) unset: Vec<String>,
    pub(crate) expect: Map<String, Value>,
}

impl RowUndo {
    /// Restores each of `touched` from `row`'s raw values: a non-empty one is set back, and
    /// an absent or empty one the write added (`written`) is removed. `status` comes back as
    /// the status displayed, which the write was guarded on.
    fn of(row: &ItemRow, touched: &[&str], written: &Map<String, Value>) -> RowUndo {
        let mut set = Map::new();
        let mut unset = Vec::new();
        for field in touched {
            if *field == "status" {
                set.insert("status".into(), Value::String(row.status.clone()));
                continue;
            }
            match row.raw.get(*field).filter(|v| !is_empty(v)) {
                Some(before) => {
                    set.insert((*field).to_owned(), before.clone());
                }
                None if written.contains_key(*field) => unset.push((*field).to_owned()),
                None => {}
            }
        }
        RowUndo {
            id: row.id.clone(),
            set,
            unset,
            expect: written.clone(),
        }
    }

    pub(crate) fn restore(&self, request: RequestId, ledger: &LedgerRef) -> WriteRequest {
        WriteRequest::Restore {
            request,
            ledger: ledger.clone(),
            id: self.id.clone(),
            set: self.set.clone(),
            unset: self.unset.clone(),
            expect: self.expect.clone(),
        }
    }
}

fn is_empty(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::String(s) => s.is_empty(),
        Value::Array(a) => a.is_empty(),
        _ => false,
    }
}

/// The requests one submitted edit makes, each paired with the ids it targets, and the undo
/// of every row it targets.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Plan {
    pub(crate) requests: Vec<(WriteRequest, Vec<String>)>,
    pub(crate) undo: Vec<RowUndo>,
}

fn take(next: &mut RequestId) -> RequestId {
    let id = *next;
    *next += 1;
    id
}

/// The companion values a transition form submitted, by field name, leaving out empty ones.
pub(crate) fn companions(transition: Transition, values: &[FieldValue]) -> Map<String, Value> {
    transition
        .fields
        .iter()
        .zip(values)
        .filter_map(|(c, value)| match value {
            FieldValue::Text(text) if !text.is_empty() => {
                Some((c.field.to_owned(), Value::String(text.clone())))
            }
            _ => None,
        })
        .collect()
}

/// One request per displayed status, each guarded on that status, since a facade call takes
/// a single from-status.
pub(crate) fn transition_plan(
    selection: &Selection,
    transition: Transition,
    fields: &Map<String, Value>,
    next: &mut RequestId,
) -> Plan {
    let mut by_status: BTreeMap<&str, Vec<&ItemRow>> = BTreeMap::new();
    for row in &selection.rows {
        by_status.entry(row.status.as_str()).or_default().push(row);
    }
    let mut plan = Plan::default();
    for (status, rows) in by_status {
        let ids: Vec<String> = rows.iter().map(|r| r.id.clone()).collect();
        let mut written = fields.clone();
        written.insert("status".into(), Value::String(transition.to.into()));
        let request = match selection.ledger {
            LedgerRef::Backlog => {
                let text = |field: &str| {
                    fields
                        .get(field)
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_owned()
                };
                let triage = match transition.to {
                    "dismissed" => BacklogTriage::Dismiss {
                        reason: text("dismiss_reason"),
                    },
                    "resolved" => BacklogTriage::Resolve {
                        resolution: text("resolution"),
                    },
                    _ => BacklogTriage::Reopen {
                        rationale: text("reopen_rationale"),
                    },
                };
                WriteRequest::BacklogTriage {
                    request: take(next),
                    ids: ids.clone(),
                    triage,
                    expect_status: status.to_owned(),
                }
            }
            _ => WriteRequest::Transition {
                request: take(next),
                ledger: selection.ledger.clone(),
                ids: ids.clone(),
                to: transition.to.to_owned(),
                fields: fields.clone(),
                expect_status: status.to_owned(),
            },
        };
        let touched: Vec<&str> = if selection.ledger == LedgerRef::Backlog {
            let mut all: Vec<&str> = BACKLOG_MANAGED.to_vec();
            all.push("status");
            all
        } else {
            let mut all: Vec<&str> = vec!["status"];
            all.extend(fields.keys().map(String::as_str));
            for field in leaves(status) {
                if !all.contains(field) {
                    all.push(field);
                }
            }
            all
        };
        // The stamped date is written but unguarded, so it counts as written for removal only.
        let mut added = written.clone();
        if selection.ledger == LedgerRef::Backlog
            && let Some(stamp) = backlog_stamp(transition.to)
        {
            added.insert(stamp.into(), Value::Null);
        }
        for row in rows {
            let mut undo = RowUndo::of(row, &touched, &added);
            undo.expect = written.clone();
            plan.undo.push(undo);
        }
        plan.requests.push((request, ids));
    }
    plan
}

/// One request per distinct edit: each row writes only the submitted values that differ from
/// its own, guarded on its current values of those fields. Rows already matching are left out.
pub(crate) fn classify_plan(
    selection: &Selection,
    values: &[FieldValue],
    next: &mut RequestId,
) -> Plan {
    let submitted: Vec<(&str, &str)> = CLASSIFY_FIELDS
        .iter()
        .zip(values)
        .filter_map(|(field, value)| match value {
            FieldValue::One(v) if v != KEEP && !v.trim().is_empty() => Some((*field, v.as_str())),
            _ => None,
        })
        .collect();
    struct Edit<'a> {
        fields: Map<String, Value>,
        expect: Map<String, Value>,
        rows: Vec<&'a ItemRow>,
    }
    let mut groups: Vec<Edit> = Vec::new();
    for row in &selection.rows {
        let mut fields = Map::new();
        let mut expect = Map::new();
        for (field, value) in &submitted {
            let before = row.raw.get(*field).cloned().unwrap_or(Value::Null);
            if before.as_str() != Some(value) {
                fields.insert((*field).to_owned(), Value::String((*value).to_owned()));
                expect.insert((*field).to_owned(), before);
            }
        }
        if fields.is_empty() {
            continue;
        }
        match groups
            .iter_mut()
            .find(|g| g.fields == fields && g.expect == expect)
        {
            Some(group) => group.rows.push(row),
            None => groups.push(Edit {
                fields,
                expect,
                rows: vec![row],
            }),
        }
    }
    let mut plan = Plan::default();
    for Edit {
        fields,
        expect,
        rows,
    } in groups
    {
        let ids: Vec<String> = rows.iter().map(|r| r.id.clone()).collect();
        let touched: Vec<&str> = fields.keys().map(String::as_str).collect();
        for row in rows {
            plan.undo.push(RowUndo::of(row, &touched, &fields));
        }
        plan.requests.push((
            WriteRequest::Classify {
                request: take(next),
                ledger: selection.ledger.clone(),
                ids: ids.clone(),
                fields,
                expect,
            },
            ids,
        ));
    }
    plan
}

/// One submitted edit on the undo stack. `outstanding` holds the requests not yet reported;
/// `applied` the ids the reported ones changed, the only rows undo puts back.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct UndoEntry {
    pub(crate) surface: Surface,
    pub(crate) ledger: LedgerRef,
    pub(crate) rows: Vec<RowUndo>,
    pub(crate) outstanding: BTreeSet<RequestId>,
    pub(crate) applied: BTreeSet<String>,
}

/// The backlog kinds a capture can hint at; `/backlog` settles the real kind and area.
const BACKLOG_KINDS: [&str; 7] = [
    "bug",
    "flaky-test",
    "debt",
    "direction",
    "annoyance",
    "question",
    "other",
];
/// The capture kind option that records no hint.
const NO_HINT: &str = "(no hint)";
const NOTE_KINDS: [&str; 2] = ["request", "note"];

/// A form whose submission appends to, answers in or withdraws from the input store.
#[derive(Debug, Clone, PartialEq)]
#[allow(
    dead_code,
    reason = "the item surfaces and Inbox open these once their keys are wired in"
)]
pub(crate) enum InputForm {
    /// A new backlog item in the user's words.
    Capture,
    /// A request or note on the selected rows.
    Request(Selection),
    /// The answer to a `question` record.
    Answer(InputRow),
    /// A yes/no before withdrawing the user's own `new` record.
    Withdraw(InputRow),
}

#[allow(
    dead_code,
    reason = "the item surfaces and Inbox open these once their keys are wired in"
)]
impl InputForm {
    pub(crate) fn form(&self) -> Form {
        match self {
            InputForm::Capture => {
                let mut kinds = vec![NO_HINT.to_owned()];
                kinds.extend(BACKLOG_KINDS.iter().map(|k| (*k).to_owned()));
                Form::new(
                    "capture a new item",
                    vec![
                        Field::text("summary").required(),
                        Field::select("kind", kinds),
                        Field::text("area"),
                        Field::text("note"),
                    ],
                )
            }
            InputForm::Request(selection) => Form::new(
                format!("request or note on {}", selection.label()),
                vec![
                    Field::select("kind", NOTE_KINDS.iter().map(|k| (*k).to_owned()).collect())
                        .required(),
                    Field::text("text").required(),
                ],
            ),
            InputForm::Answer(question) => {
                let options = question.options.clone();
                let fields = match question.choice.as_str() {
                    "single" => vec![
                        Field::select("answer", options).required(),
                        Field::text("note"),
                    ],
                    "multi" => vec![Field::multi("answer", options), Field::text("note")],
                    _ => vec![Field::text("answer").required()],
                };
                Form::new(format!("answer {}", question.id), fields).with_prompt(&question.prompt)
            }
            InputForm::Withdraw(record) => Form::new(
                format!("withdraw {}", record.id),
                vec![Field::select("withdraw?", vec!["no".into(), "yes".into()]).opened()],
            )
            .with_prompt(&record.text),
        }
    }

    /// The write the submitted `values` make: `Ok(None)` for a declined withdrawal, and an
    /// error for the form to show when the values cannot make a valid record.
    pub(crate) fn submit(
        &self,
        values: &[FieldValue],
        next: &mut RequestId,
    ) -> Result<Option<WriteRequest>, String> {
        let text = |at: usize| match values.get(at) {
            Some(FieldValue::Text(t)) => t.trim().to_owned(),
            _ => String::new(),
        };
        let one = |at: usize| match values.get(at) {
            Some(FieldValue::One(v)) => v.clone(),
            _ => String::new(),
        };
        let record = match self {
            InputForm::Capture => {
                let summary = text(0);
                if summary.is_empty() {
                    return Err("summary is required".into());
                }
                let mut record = Map::new();
                record.insert("kind".into(), "capture".into());
                let note = text(3);
                let body = if note.is_empty() {
                    summary
                } else {
                    format!("{summary}\n\n{note}")
                };
                record.insert("text".into(), body.into());
                let kind = one(1);
                if BACKLOG_KINDS.contains(&kind.as_str()) {
                    record.insert("capture_kind".into(), kind.into());
                }
                let area = text(2);
                if !area.is_empty() {
                    record.insert("area".into(), area.into());
                }
                record
            }
            InputForm::Request(selection) => {
                let kind = one(0);
                let body = text(1);
                if !NOTE_KINDS.contains(&kind.as_str()) || body.is_empty() {
                    return Err("a request or note needs its kind and text".into());
                }
                let mut record = input_target(&selection.ledger)
                    .ok_or_else(|| "this ledger is read-only".to_owned())?;
                record.insert("kind".into(), kind.into());
                record.insert("text".into(), body.into());
                let items = selection.rows.iter().map(|r| Value::from(r.id.clone()));
                record.insert("items".into(), Value::Array(items.collect()));
                record
            }
            InputForm::Answer(question) => {
                let (picked, note) = match values {
                    [FieldValue::One(pick), FieldValue::Text(note)] => {
                        (vec![pick.clone()], note.trim())
                    }
                    [FieldValue::Many(picks), FieldValue::Text(note)] => {
                        (picks.clone(), note.trim())
                    }
                    [FieldValue::Text(answer)] => (Vec::new(), answer.trim()),
                    _ => (Vec::new(), ""),
                };
                let picked: Vec<String> = picked.into_iter().filter(|p| !p.is_empty()).collect();
                if picked.is_empty() && note.is_empty() {
                    return Err("pick an option or write an answer".into());
                }
                return Ok(Some(WriteRequest::InputAnswer {
                    request: take(next),
                    question: question.id.clone(),
                    picked,
                    text: (!note.is_empty()).then(|| note.to_owned()),
                }));
            }
            InputForm::Withdraw(record) => {
                return Ok((one(0) == "yes").then(|| withdraw(vec![record.id.clone()], next)));
            }
        };
        Ok(Some(WriteRequest::InputAdd {
            request: take(next),
            record: Value::Object(record),
        }))
    }
}

/// Withdraws `ids`: the `w` action, and the undo of an input write, whose outcome names the
/// record it created.
#[allow(
    dead_code,
    reason = "the Inbox and undo submit it once their keys are wired in"
)]
pub(crate) fn withdraw(ids: Vec<String>, next: &mut RequestId) -> WriteRequest {
    WriteRequest::InputWithdraw {
        request: take(next),
        ids,
    }
}

/// The `ledger` and `flow` or `scope` naming `ledger` as an input record's target; `None`
/// for a ledger named only by path.
fn input_target(ledger: &LedgerRef) -> Option<Map<String, Value>> {
    let name = |kind: &LedgerKind| match kind {
        LedgerKind::Review => "review",
        LedgerKind::Optimise => "optimise",
        LedgerKind::PlanReview => "plan-review",
    };
    let mut target = Map::new();
    match ledger {
        LedgerRef::Backlog => {
            target.insert("ledger".into(), "backlog".into());
        }
        LedgerRef::Flow { slug, kind } => {
            target.insert("ledger".into(), name(kind).into());
            target.insert("flow".into(), slug.clone().into());
        }
        LedgerRef::Scope { kind, scope } => {
            target.insert("ledger".into(), name(kind).into());
            target.insert("scope".into(), scope.clone().into());
        }
        LedgerRef::File(_) => return None,
    }
    Some(target)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::form::FormOutcome;
    use crate::ledger::StatusClass;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use serde_json::json;

    fn row(id: &str, status: &str, raw: Value) -> ItemRow {
        ItemRow {
            id: id.into(),
            status: status.into(),
            class: StatusClass::of(status),
            severity: raw["severity"].as_str().unwrap_or("").into(),
            raw,
            ..ItemRow::default()
        }
    }

    fn selection(kind: Kind, ledger: LedgerRef, rows: Vec<ItemRow>) -> Selection {
        Selection {
            surface: Surface::Review,
            kind,
            ledger,
            rows,
        }
    }

    fn review_ledger() -> LedgerRef {
        LedgerRef::Flow {
            slug: "demo".into(),
            kind: LedgerKind::Review,
        }
    }

    #[test]
    fn ledger_paths_map_to_the_file_on_screen() {
        assert_eq!(
            ledger_ref(Kind::Review, ".claude/flows/demo/review-ledger.toml"),
            Some(review_ledger())
        );
        assert_eq!(
            ledger_ref(Kind::Optimise, ".claude/optimise-findings/core.toml"),
            Some(LedgerRef::Scope {
                kind: LedgerKind::Optimise,
                scope: "core".into()
            })
        );
        assert_eq!(
            ledger_ref(Kind::Backlog, ".claude/backlog.toml"),
            Some(LedgerRef::Backlog)
        );
        assert_eq!(
            ledger_ref(Kind::Review, "elsewhere/review-ledger.toml"),
            None
        );
        assert_eq!(
            ledger_ref(Kind::Review, ".claude/flows/demo/optimise-findings.toml"),
            None,
            "a file of another kind is not this ledger"
        );
    }

    #[test]
    fn backlog_reopen_from_two_statuses_splits_per_status() {
        let sel = selection(
            Kind::Backlog,
            LedgerRef::Backlog,
            vec![
                row(
                    "B-1",
                    "dismissed",
                    json!({"id": "B-1", "status": "dismissed", "dismissed": "2026-09-01", "dismiss_reason": "dup"}),
                ),
                row(
                    "B-2",
                    "resolved",
                    json!({"id": "B-2", "status": "resolved", "resolved": "2026-09-02", "resolution": "done"}),
                ),
            ],
        );
        assert_eq!(sel.transitions(), vec![REOPEN]);
        let fields = companions(REOPEN, &[FieldValue::Text("again".into())]);
        let mut next = 5;
        let plan = transition_plan(&sel, REOPEN, &fields, &mut next);
        let statuses: Vec<_> = plan
            .requests
            .iter()
            .map(|(r, _)| match r {
                WriteRequest::BacklogTriage {
                    expect_status,
                    triage,
                    ..
                } => {
                    assert_eq!(
                        triage,
                        &BacklogTriage::Reopen {
                            rationale: "again".into()
                        }
                    );
                    expect_status.as_str()
                }
                other => panic!("expected a backlog triage, got {other:?}"),
            })
            .collect();
        assert_eq!(statuses, vec!["dismissed", "resolved"]);
        assert_eq!(next, 7);
        let undo = &plan.undo[0];
        assert_eq!(undo.set["status"], "dismissed");
        assert_eq!(undo.set["dismiss_reason"], "dup");
        assert_eq!(undo.unset, vec!["reopen_rationale".to_string()]);
        assert_eq!(undo.expect["status"], "open");
    }

    #[test]
    fn a_reopen_undo_puts_back_the_dropped_trigger() {
        let sel = selection(
            Kind::Review,
            review_ledger(),
            vec![row(
                "R1",
                "deferred",
                json!({"id": "R1", "status": "deferred", "defer_reason": "later", "defer_trigger": "v2"}),
            )],
        );
        let fields = companions(REOPEN, &[FieldValue::Text("v2 shipped".into())]);
        let plan = transition_plan(&sel, REOPEN, &fields, &mut 0);
        let undo = &plan.undo[0];
        assert_eq!(undo.set["status"], "deferred");
        assert_eq!(undo.set["defer_trigger"], "v2");
        assert_eq!(undo.unset, vec!["reopen_rationale".to_string()]);
        assert_eq!(undo.expect["reopen_rationale"], "v2 shipped");
    }

    #[test]
    fn classify_writes_only_what_changes_per_row() {
        let sel = selection(
            Kind::Review,
            review_ledger(),
            vec![
                row(
                    "R1",
                    "open",
                    json!({"id": "R1", "severity": "warning", "effort": "small", "category": "quality"}),
                ),
                row(
                    "R2",
                    "open",
                    json!({"id": "R2", "severity": "critical", "effort": "small", "category": "quality"}),
                ),
                row(
                    "R3",
                    "open",
                    json!({"id": "R3", "severity": "suggestion", "effort": "small", "category": "quality"}),
                ),
            ],
        );
        let form = classify_form(&sel);
        assert_eq!(form.fields[0].value(), FieldValue::One(KEEP.into()));
        assert_eq!(form.fields[1].value(), FieldValue::One("small".into()));
        let values = [
            FieldValue::One("critical".into()),
            FieldValue::One("small".into()),
            FieldValue::One("security".into()),
        ];
        let plan = classify_plan(&sel, &values, &mut 0);
        assert_eq!(
            plan.requests.len(),
            3,
            "each row is guarded on its own severity"
        );
        let (WriteRequest::Classify { fields, expect, .. }, ids) = &plan.requests[1] else {
            panic!("expected a classify");
        };
        assert_eq!(ids, &vec!["R2".to_string()]);
        assert_eq!(fields, json!({"category": "security"}).as_object().unwrap());
        assert_eq!(expect, json!({"category": "quality"}).as_object().unwrap());
    }

    #[test]
    fn only_a_critical_decline_needs_confirmation() {
        let sel = selection(
            Kind::Review,
            review_ledger(),
            vec![
                row("R1", "open", json!({"severity": "critical"})),
                row("R2", "open", json!({"severity": "warning"})),
            ],
        );
        assert_eq!(sel.critical_declines("wontfix"), vec!["R1".to_string()]);
        assert!(sel.critical_declines("deferred").is_empty());
    }

    fn press(form: &mut Form, code: KeyCode) -> FormOutcome {
        form.handle_key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn type_str(form: &mut Form, s: &str) {
        for c in s.chars() {
            press(form, KeyCode::Char(c));
        }
    }

    fn submitted(form: &mut Form) -> Vec<FieldValue> {
        match press(form, KeyCode::Enter) {
            FormOutcome::Submit(values) => values,
            other => panic!("expected a submit, got {other:?} ({:?})", form.error),
        }
    }

    #[test]
    fn a_capture_form_submits_an_input_add() {
        let mut form = InputForm::Capture.form();
        type_str(&mut form, "watch leaks a handle");
        press(&mut form, KeyCode::Tab);
        press(&mut form, KeyCode::Enter);
        press(&mut form, KeyCode::Down);
        press(&mut form, KeyCode::Enter);
        press(&mut form, KeyCode::Tab);
        type_str(&mut form, "glimpse/src/watch.rs");
        press(&mut form, KeyCode::Tab);
        type_str(&mut form, "seen twice on Windows");
        let values = submitted(&mut form);
        let mut next = 4;
        let request = InputForm::Capture.submit(&values, &mut next);
        assert_eq!(
            request,
            Ok(Some(WriteRequest::InputAdd {
                request: 4,
                record: json!({
                    "kind": "capture",
                    "text": "watch leaks a handle\n\nseen twice on Windows",
                    "capture_kind": "bug",
                    "area": "glimpse/src/watch.rs",
                }),
            }))
        );
        assert_eq!(next, 5);
    }

    #[test]
    fn a_capture_without_a_hint_leaves_kind_and_area_out() {
        let values = [
            FieldValue::Text("tidy the legend".into()),
            FieldValue::One(NO_HINT.into()),
            FieldValue::Text(String::new()),
            FieldValue::Text(String::new()),
        ];
        let Ok(Some(WriteRequest::InputAdd { record, .. })) =
            InputForm::Capture.submit(&values, &mut 0)
        else {
            panic!("expected an input add");
        };
        assert_eq!(
            record,
            json!({"kind": "capture", "text": "tidy the legend"})
        );
    }

    #[test]
    fn a_request_targets_the_selected_rows_in_their_ledger() {
        let sel = selection(
            Kind::Review,
            review_ledger(),
            vec![row("R3", "open", json!({})), row("R7", "open", json!({}))],
        );
        let input = InputForm::Request(sel);
        let mut form = input.form();
        press(&mut form, KeyCode::Tab);
        type_str(&mut form, "defer both");
        let values = submitted(&mut form);
        let Ok(Some(WriteRequest::InputAdd { record, .. })) = input.submit(&values, &mut 0) else {
            panic!("expected an input add");
        };
        assert_eq!(
            record,
            json!({
                "kind": "request",
                "text": "defer both",
                "ledger": "review",
                "flow": "demo",
                "items": ["R3", "R7"],
            })
        );
    }

    fn question(choice: &str, options: &[&str]) -> InputRow {
        InputRow {
            id: "I3".into(),
            kind: "question".into(),
            author: "review".into(),
            status: "new".into(),
            prompt: "which first?".into(),
            choice: choice.into(),
            options: options.iter().map(|o| (*o).to_owned()).collect(),
            ..InputRow::default()
        }
    }

    #[test]
    fn answering_a_multi_question_sends_the_picked_options() {
        let input = InputForm::Answer(question("multi", &["R1", "R2", "R3"]));
        let mut form = input.form();
        press(&mut form, KeyCode::Char(' '));
        press(&mut form, KeyCode::Down);
        press(&mut form, KeyCode::Down);
        press(&mut form, KeyCode::Char(' '));
        let values = submitted(&mut form);
        assert_eq!(
            input.submit(&values, &mut 9),
            Ok(Some(WriteRequest::InputAnswer {
                request: 9,
                question: "I3".into(),
                picked: vec!["R1".into(), "R3".into()],
                text: None,
            }))
        );
    }

    #[test]
    fn an_answer_with_nothing_picked_or_written_is_refused() {
        let input = InputForm::Answer(question("multi", &["R1"]));
        let values = [FieldValue::Many(Vec::new()), FieldValue::Text(" ".into())];
        assert!(input.submit(&values, &mut 0).is_err());
        let text = InputForm::Answer(question("text", &[]));
        assert_eq!(
            text.submit(&[FieldValue::Text("after the release".into())], &mut 0),
            Ok(Some(WriteRequest::InputAnswer {
                request: 0,
                question: "I3".into(),
                picked: Vec::new(),
                text: Some("after the release".into()),
            }))
        );
    }

    #[test]
    fn withdrawing_needs_a_yes() {
        let record = InputRow {
            id: "I5".into(),
            ..InputRow::default()
        };
        let input = InputForm::Withdraw(record);
        assert_eq!(
            input.submit(&[FieldValue::One("no".into())], &mut 0),
            Ok(None)
        );
        assert_eq!(
            input.submit(&[FieldValue::One("yes".into())], &mut 2),
            Ok(Some(WriteRequest::InputWithdraw {
                request: 2,
                ids: vec!["I5".into()],
            }))
        );
    }
}
