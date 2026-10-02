//! Ledger rows, their status classes, and the filter, group-by and sort over an item surface,
//! plus the records of the input store.
//!
//! Rows arrive as the `items` of a `tomlctl::ledger_read` document, and input
//! records as the `inputs` of a `tomlctl::inputs_read` one. Each row
//! type reads with `#[serde(default)]`, so a missing field reads as empty and
//! an unknown one is ignored; a field with an unexpected type reads as empty
//! rather than dropping the row.

use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

/// Which ledger a document came from, from its `kind` string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum Kind {
    Review,
    Optimise,
    PlanReview,
    Backlog,
}

impl Kind {
    pub(crate) fn parse(kind: &str) -> Option<Kind> {
        Some(match kind {
            "review" => Self::Review,
            "optimise" => Self::Optimise,
            "plan-review" => Self::PlanReview,
            "backlog" => Self::Backlog,
            _ => return None,
        })
    }
}

/// The glyph and closed-filter bucket of a status. An unknown status is `Live`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum StatusClass {
    #[default]
    Live,
    Parked,
    Done,
    Declined,
}

impl StatusClass {
    pub(crate) fn of(status: &str) -> StatusClass {
        match status {
            "deferred" | "promoted" => Self::Parked,
            "fixed" | "applied" | "merged" | "resolved" | "verified-clean" => Self::Done,
            "wontfix" | "wontapply" | "discarded" | "dismissed" => Self::Declined,
            _ => Self::Live,
        }
    }
}

/// A review or optimise finding; the two ledgers share one row shape.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
struct FindingRow {
    id: String,
    file: String,
    line: u64,
    symbol: String,
    severity: String,
    effort: String,
    category: String,
    summary: String,
    description: String,
    first_flagged: String,
    status: String,
    evidence: Vec<String>,
    instances: Vec<String>,
    resolved: String,
    resolution: String,
    defer_reason: String,
    defer_trigger: String,
    wontfix_rationale: String,
    wontapply_rationale: String,
    verified_note: String,
    reopen_rationale: String,
    rollback_rationale: String,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
struct PlanReviewRow {
    id: String,
    severity: String,
    category: String,
    plan_section: String,
    summary: String,
    description: String,
    status: String,
    evidence: Vec<String>,
    discard_reason: String,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
struct BacklogRow {
    id: String,
    kind: String,
    summary: String,
    area: String,
    tags: Vec<String>,
    status: String,
    created: String,
    context: String,
    evidence: Vec<String>,
    promoted: String,
    promoted_to: String,
    dismissed: String,
    dismiss_reason: String,
    resolved: String,
    resolution: String,
    reopen_rationale: String,
}

/// Where a row points: code for findings, a plan heading for plan-review, an
/// area for backlog.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) enum Anchor {
    Code {
        file: String,
        line: u64,
        symbol: String,
    },
    Section(String),
    Area(String),
    #[default]
    None,
}

impl std::fmt::Display for Anchor {
    /// `file:line:symbol`, leaving out a zero line and an empty symbol.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Code { file, line, symbol } => {
                f.write_str(file)?;
                if *line > 0 {
                    write!(f, ":{line}")?;
                }
                if !symbol.is_empty() {
                    write!(f, ":{symbol}")?;
                }
                Ok(())
            }
            Self::Section(text) | Self::Area(text) => f.write_str(text),
            Self::None => Ok(()),
        }
    }
}

/// One row of any ledger, as an item surface lists and details it.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct ItemRow {
    /// The ledger id, or `#<position>` (from 1) for a row that has none.
    pub(crate) id: String,
    /// Set for a row without a ledger id, which no write can address.
    pub(crate) read_only: bool,
    pub(crate) summary: String,
    pub(crate) description: String,
    pub(crate) status: String,
    pub(crate) class: StatusClass,
    pub(crate) severity: String,
    /// The backlog kind; empty for the other ledgers.
    pub(crate) kind: String,
    pub(crate) category: String,
    pub(crate) effort: String,
    pub(crate) anchor: Anchor,
    /// `first_flagged` or `created`, as `YYYY-MM-DD`.
    pub(crate) created: String,
    /// The date the row left `open`: `resolved`, `dismissed` or `promoted`.
    pub(crate) closed: String,
    /// The non-empty rationale and companion fields, by field name, in a fixed order.
    pub(crate) companions: Vec<(&'static str, String)>,
    pub(crate) tags: Vec<String>,
    pub(crate) evidence: Vec<String>,
    pub(crate) instances: Vec<String>,
    pub(crate) raw: Value,
}

/// One ledger document. `revision` is `None` for a ledger file that does not exist.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Ledger {
    pub(crate) kind: Kind,
    pub(crate) path: String,
    pub(crate) revision: Option<String>,
    pub(crate) rows: Vec<ItemRow>,
}

impl Ledger {
    /// Loads a `tomlctl::ledger_read` document. Fails only on an unknown
    /// `kind` or an `items` that is not an array.
    pub(crate) fn from_value(value: Value) -> Result<Ledger, String> {
        let Value::Object(mut doc) = value else {
            return Err("ledger document is not an object".into());
        };
        let kind_text = doc.get("kind").and_then(Value::as_str).unwrap_or("");
        let kind =
            Kind::parse(kind_text).ok_or_else(|| format!("unknown ledger kind {kind_text:?}"))?;
        let path = doc
            .get("path")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        let revision = doc
            .get("revision")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let items = match doc.remove("items") {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::Array(items)) => items,
            Some(_) => return Err(format!("{path}: `items` is not an array")),
        };
        let rows = items
            .into_iter()
            .enumerate()
            .map(|(index, raw)| ItemRow::from_raw(kind, index, raw))
            .collect();
        Ok(Ledger {
            kind,
            path,
            revision,
            rows,
        })
    }
}

impl ItemRow {
    fn from_raw(kind: Kind, index: usize, raw: Value) -> ItemRow {
        let mut row = match kind {
            Kind::Review | Kind::Optimise => Self::finding(lenient(&raw)),
            Kind::PlanReview => Self::plan_review(lenient(&raw)),
            Kind::Backlog => Self::backlog(lenient(&raw)),
        };
        if row.id.is_empty() {
            row.id = format!("#{}", index + 1);
            row.read_only = true;
        }
        row.class = StatusClass::of(&row.status);
        row.raw = raw;
        row
    }

    fn finding(r: FindingRow) -> ItemRow {
        let closed = r.resolved.clone();
        let companions = companions([
            ("resolution", r.resolution),
            ("defer_reason", r.defer_reason),
            ("defer_trigger", r.defer_trigger),
            ("wontfix_rationale", r.wontfix_rationale),
            ("wontapply_rationale", r.wontapply_rationale),
            ("verified_note", r.verified_note),
            ("reopen_rationale", r.reopen_rationale),
            ("rollback_rationale", r.rollback_rationale),
        ]);
        let anchor = if r.file.is_empty() {
            Anchor::None
        } else {
            Anchor::Code {
                file: r.file,
                line: r.line,
                symbol: r.symbol,
            }
        };
        ItemRow {
            id: r.id,
            summary: r.summary,
            description: r.description,
            status: r.status,
            severity: r.severity,
            category: r.category,
            effort: r.effort,
            anchor,
            created: r.first_flagged,
            closed,
            companions,
            evidence: r.evidence,
            instances: r.instances,
            ..ItemRow::default()
        }
    }

    fn plan_review(r: PlanReviewRow) -> ItemRow {
        let anchor = if r.plan_section.is_empty() {
            Anchor::None
        } else {
            Anchor::Section(r.plan_section)
        };
        ItemRow {
            id: r.id,
            summary: r.summary,
            description: r.description,
            status: r.status,
            severity: r.severity,
            category: r.category,
            anchor,
            companions: companions([("discard_reason", r.discard_reason)]),
            evidence: r.evidence,
            ..ItemRow::default()
        }
    }

    fn backlog(r: BacklogRow) -> ItemRow {
        let closed = [&r.resolved, &r.dismissed, &r.promoted]
            .into_iter()
            .find(|date| !date.is_empty())
            .cloned()
            .unwrap_or_default();
        let anchor = if r.area.is_empty() {
            Anchor::None
        } else {
            Anchor::Area(r.area)
        };
        ItemRow {
            id: r.id,
            summary: r.summary,
            description: r.context,
            status: r.status,
            kind: r.kind,
            anchor,
            created: r.created,
            closed,
            companions: companions([
                ("resolution", r.resolution),
                ("dismiss_reason", r.dismiss_reason),
                ("promoted_to", r.promoted_to),
                ("reopen_rationale", r.reopen_rationale),
            ]),
            tags: r.tags,
            evidence: r.evidence,
            ..ItemRow::default()
        }
    }
}

/// One record of the input store. A row without an `id` reads as `#<position>`
/// (from 1) and is `read_only`.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub(crate) struct InputRow {
    pub(crate) id: String,
    #[serde(skip)]
    pub(crate) read_only: bool,
    pub(crate) kind: String,
    pub(crate) author: String,
    pub(crate) status: String,
    /// An RFC 3339 datetime.
    pub(crate) created: String,
    pub(crate) ledger: String,
    pub(crate) flow: String,
    pub(crate) scope: String,
    pub(crate) items: Vec<String>,
    pub(crate) text: String,
    pub(crate) capture_kind: String,
    pub(crate) area: String,
    pub(crate) prompt: String,
    pub(crate) choice: String,
    pub(crate) options: Vec<String>,
    /// The id of the question an `answer` answers.
    pub(crate) answers: String,
    pub(crate) picked: Vec<String>,
    pub(crate) acknowledged: String,
    pub(crate) acknowledged_by: String,
    pub(crate) handled: String,
    pub(crate) handled_by: String,
    pub(crate) handled_note: String,
}

impl InputRow {
    /// `new` or `acknowledged`: no agent has finished with it.
    pub(crate) fn is_pending(&self) -> bool {
        matches!(self.status.as_str(), "new" | "acknowledged")
    }

    pub(crate) fn is_unanswered_question(&self) -> bool {
        self.kind == "question" && self.status == "new"
    }

    pub(crate) fn ledger_kind(&self) -> Option<Kind> {
        Kind::parse(&self.ledger)
    }
}

/// The input store document. `revision` is `None` when the store does not exist.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Inputs {
    pub(crate) path: String,
    pub(crate) revision: Option<String>,
    pub(crate) rows: Vec<InputRow>,
}

impl Inputs {
    /// Loads a `tomlctl::inputs_read` document. Fails only on an `inputs` that
    /// is not an array.
    pub(crate) fn from_value(value: Value) -> Result<Inputs, String> {
        let Value::Object(mut doc) = value else {
            return Err("inputs document is not an object".into());
        };
        let path = doc
            .get("path")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        let revision = doc
            .get("revision")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let records = match doc.remove("inputs") {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::Array(records)) => records,
            Some(_) => return Err(format!("{path}: `inputs` is not an array")),
        };
        let rows = records
            .iter()
            .enumerate()
            .map(|(index, raw)| {
                let mut row: InputRow = lenient(raw);
                if row.id.is_empty() {
                    row.id = format!("#{}", index + 1);
                    row.read_only = true;
                }
                row
            })
            .collect();
        Ok(Inputs {
            path,
            revision,
            rows,
        })
    }
}

fn companions<const N: usize>(fields: [(&'static str, String); N]) -> Vec<(&'static str, String)> {
    fields
        .into_iter()
        .filter(|(_, value)| !value.is_empty())
        .collect()
}

/// Reads `raw` as `T`, dropping any field whose value does not read as that
/// field's type. Every field of `T` defaults, so a one-field object reads
/// exactly when that field's type matches.
fn lenient<T: DeserializeOwned + Default>(raw: &Value) -> T {
    T::deserialize(raw).unwrap_or_else(|_| {
        let fitting: Map<String, Value> = raw
            .as_object()
            .into_iter()
            .flatten()
            .filter(|(key, value)| {
                let one = Map::from_iter([((*key).clone(), (*value).clone())]);
                T::deserialize(Value::Object(one)).is_ok()
            })
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        T::deserialize(Value::Object(fitting)).unwrap_or_default()
    })
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use serde_json::json;
    use tomlctl::LedgerRef;

    use super::*;

    fn fixture(name: &str) -> Ledger {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let path: PathBuf = root.join("tests").join("fixtures").join(name);
        let value = tomlctl::ledger_read(root, &LedgerRef::File(path)).expect("fixture reads");
        Ledger::from_value(value).expect("fixture loads")
    }

    fn row<'a>(ledger: &'a Ledger, id: &str) -> &'a ItemRow {
        ledger
            .rows
            .iter()
            .find(|row| row.id == id)
            .unwrap_or_else(|| panic!("no row {id}"))
    }

    #[test]
    fn review_fixture_maps_every_status_to_its_class() {
        let ledger = fixture("review-ledger.toml");
        assert_eq!(ledger.kind, Kind::Review);
        assert!(ledger.revision.as_deref().is_some_and(|r| r.len() == 64));
        let classes: Vec<(&str, &str, StatusClass)> = ledger
            .rows
            .iter()
            .map(|row| (row.id.as_str(), row.status.as_str(), row.class))
            .collect();
        assert_eq!(
            classes,
            [
                ("R1", "open", StatusClass::Live),
                ("R2", "deferred", StatusClass::Parked),
                ("R3", "fixed", StatusClass::Done),
                ("R4", "wontfix", StatusClass::Declined),
                ("R5", "verified-clean", StatusClass::Done),
                ("R6", "triaged", StatusClass::Live),
            ]
        );
    }

    #[test]
    fn review_rows_carry_anchor_dates_and_companions() {
        let ledger = fixture("review-ledger.toml");
        let deferred = row(&ledger, "R2");
        assert_eq!(deferred.anchor.to_string(), "src/cache.rs:42:Cache::get");
        assert_eq!(deferred.created, "2026-09-20");
        assert_eq!(
            deferred.companions,
            [
                ("defer_reason", "waits on the cache rewrite".to_owned()),
                ("defer_trigger", "the LRU lands".to_owned()),
            ]
        );
        let fixed = row(&ledger, "R3");
        assert_eq!(fixed.closed, "2026-09-25");
        assert_eq!(fixed.instances, ["src/io.rs:read_all", "src/net.rs:fetch"]);
        assert_eq!(fixed.raw["rounds"], json!(2));
        assert_eq!(row(&ledger, "R5").anchor.to_string(), "src/lib.rs");
    }

    #[test]
    fn a_plan_review_row_without_an_id_is_read_only() {
        let value = json!({
            "path": "x/plan-review-findings.toml",
            "kind": "plan-review",
            "revision": null,
            "items": [
                {"id": "P1", "status": "open", "plan_section": "### Forms", "summary": "a"},
                {"status": "merged", "severity": "warning", "summary": "b"},
            ],
        });
        let ledger = Ledger::from_value(value).expect("loads");
        assert_eq!(ledger.revision, None);
        let [first, second] = ledger.rows.as_slice() else {
            panic!("two rows expected");
        };
        assert_eq!((first.id.as_str(), first.read_only), ("P1", false));
        assert_eq!(first.anchor, Anchor::Section("### Forms".into()));
        assert_eq!((second.id.as_str(), second.read_only), ("#2", true));
        assert_eq!(second.class, StatusClass::Done);
    }

    #[test]
    fn backlog_rows_carry_kind_and_area() {
        let ledger = fixture("backlog.toml");
        assert_eq!(ledger.kind, Kind::Backlog);
        let rows: Vec<(&str, &str, String, StatusClass)> = ledger
            .rows
            .iter()
            .map(|row| {
                (
                    row.id.as_str(),
                    row.kind.as_str(),
                    row.anchor.to_string(),
                    row.class,
                )
            })
            .collect();
        assert_eq!(
            rows,
            [
                (
                    "B-0a1b2c3d",
                    "bug",
                    "glimpse/src/watch.rs".to_owned(),
                    StatusClass::Live
                ),
                (
                    "B-1b2c3d4e",
                    "debt",
                    "tomlctl/src/items.rs".to_owned(),
                    StatusClass::Parked
                ),
                (
                    "B-2c3d4e5f",
                    "annoyance",
                    "glimpse".to_owned(),
                    StatusClass::Declined
                ),
                (
                    "B-3d4e5f60",
                    "flaky-test",
                    "tomlctl/tests".to_owned(),
                    StatusClass::Done
                ),
            ]
        );
        let dismissed = row(&ledger, "B-2c3d4e5f");
        assert_eq!(dismissed.closed, "2026-09-30");
        assert_eq!(dismissed.tags, ["ui"]);
        assert_eq!(
            dismissed.companions,
            [("dismiss_reason", "works as intended".to_owned())]
        );
    }

    #[test]
    fn a_mistyped_field_reads_as_empty_and_keeps_the_row() {
        let value = json!({
            "path": "review-ledger.toml",
            "kind": "review",
            "revision": "ab",
            "items": [{"id": "R9", "status": "wontfix", "line": "ten", "summary": "s"}],
        });
        let ledger = Ledger::from_value(value).expect("loads");
        let row = &ledger.rows[0];
        assert_eq!((row.id.as_str(), row.summary.as_str()), ("R9", "s"));
        assert_eq!(row.anchor, Anchor::None);
        assert_eq!(row.class, StatusClass::Declined);
        assert!(!row.read_only);
    }

    #[test]
    fn input_records_read_leniently() {
        let value = json!({
            "path": ".claude/inputs.toml",
            "revision": "cd",
            "inputs": [
                {"id": "I1", "kind": "question", "author": "review", "status": "new",
                 "created": "2026-10-02T09:00:00Z", "ledger": "review", "items": ["R3"],
                 "prompt": "Defer?", "choice": "multi", "options": ["a", "b"]},
                {"kind": "note", "status": "new", "items": "R4", "text": "t"},
            ],
        });
        let inputs = Inputs::from_value(value).expect("loads");
        assert_eq!(inputs.revision.as_deref(), Some("cd"));
        let [question, note] = inputs.rows.as_slice() else {
            panic!("two rows expected");
        };
        assert!(question.is_unanswered_question() && question.is_pending());
        assert_eq!(question.ledger_kind(), Some(Kind::Review));
        assert_eq!(question.options, ["a", "b"]);
        assert_eq!(question.created, "2026-10-02T09:00:00Z");
        assert!(!question.read_only);
        assert_eq!((note.id.as_str(), note.read_only), ("#2", true));
        assert!(note.items.is_empty(), "a mistyped field reads as empty");
        assert_eq!(note.text, "t");
        let missing = json!({"path": "p", "revision": null, "inputs": []});
        assert_eq!(Inputs::from_value(missing).expect("loads").revision, None);
    }

    #[test]
    fn an_unknown_kind_is_an_error() {
        let value = json!({"path": "p", "kind": "tasks", "revision": null, "items": []});
        assert!(Ledger::from_value(value).is_err());
    }
}
