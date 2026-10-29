//! `backlog focus` — the "do next" queue over live backlog rows.
//!
//! The queue is every live (`open` or `promoted`) row carrying `focus`,
//! ordered by `(focus, id)`. A row's position is its index in that order,
//! computed on read and never stored, so `focus` values may have gaps: a
//! removal leaves one, and nothing renumbers the queue except a duplicate
//! value, which [`plan`] settles by numbering the whole queue 1..N first.
//!
//! [`plan`] is pure, so a caller holding a displayed copy of the queue can
//! predict exactly which rows a write will touch. [`apply`] runs the same
//! plan against the stored queue under the store lock, optionally refusing
//! the whole write when the stored queue no longer matches the caller's copy.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::Result;
use serde_json::{Value as JsonValue, json};
use toml::Value as TomlValue;

use super::add::preview_doc;
use super::schema::{self, FIELD_FOCUS, FIELD_LAST_UPDATED};
use crate::cli::{WriteIntegrityArgs, write_integrity_opts};
use crate::convert::toml_to_json;
use crate::errors::{ErrorKind, tagged_err};
use crate::io::{
    item_id, items_array, items_array_mut, mutate_doc_conditional, on_missing_for, warn_if_created,
};
use crate::items::StaleOp;
use crate::output::print_json_compact;

/// One queue edit. Positions are 1-based.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FocusOp {
    /// Queue rows not yet queued, in the order given, at position `at`
    /// (the bottom when `None`). An `at` past the bottom clamps to it.
    Add { ids: Vec<String>, at: Option<usize> },
    /// Drop `focus` from queued rows; the gap they leave stays.
    Remove { ids: Vec<String> },
    /// Swap a queued row with its neighbour above (`up`) or below.
    Move { id: String, up: bool },
}

/// One row whose `focus` a plan changes; `None` is an absent `focus`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FocusChange {
    pub id: String,
    pub before: Option<i64>,
    pub after: Option<i64>,
}

impl FocusChange {
    pub(crate) fn to_json(&self) -> JsonValue {
        json!({"id": self.id, "before": self.before, "after": self.after})
    }
}

fn invalid(msg: impl Into<String>) -> anyhow::Error {
    tagged_err(ErrorKind::Validation, None, msg)
}

fn ensure_distinct(ids: &[String]) -> Result<()> {
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    for id in ids {
        if !seen.insert(id.as_str()) {
            return Err(invalid(format!("{id} is named twice; name each id once")));
        }
    }
    Ok(())
}

/// The queued `(id, focus)` pairs of `state` in queue order.
fn ordered<'a>(state: &BTreeMap<&'a str, Option<i64>>) -> Vec<(&'a str, i64)> {
    let mut queued: Vec<(&str, i64)> = state
        .iter()
        .filter_map(|(id, focus)| focus.map(|f| (*id, f)))
        .collect();
    queued.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(b.0)));
    queued
}

/// The `focus` changes `op` makes to `queue`: the live rows as `(id, focus)`,
/// any order, `None` for unqueued. A duplicate `focus` value first renumbers
/// the whole queue 1..N, those rows reported too. Sorted by position after
/// the change, removals last.
pub fn plan(queue: &[(String, Option<i64>)], op: &FocusOp) -> Result<Vec<FocusChange>> {
    let mut original: BTreeMap<&str, Option<i64>> = BTreeMap::new();
    for (id, focus) in queue {
        original.entry(id.as_str()).or_insert(*focus);
    }
    let mut state = original.clone();

    let live_focus = |id: &str| -> Result<Option<i64>> {
        original
            .get(id)
            .copied()
            .ok_or_else(|| invalid(format!("{id} is not a live backlog row")))
    };
    match op {
        FocusOp::Add { ids, at } => {
            ensure_distinct(ids)?;
            if *at == Some(0) {
                return Err(invalid("queue positions start at 1"));
            }
            for id in ids {
                if live_focus(id)?.is_some() {
                    return Err(invalid(format!(
                        "{id} is already queued; move it rather than add it"
                    )));
                }
            }
        }
        FocusOp::Remove { ids } => {
            ensure_distinct(ids)?;
            for id in ids {
                if live_focus(id)?.is_none() {
                    return Err(invalid(format!("{id} is not queued")));
                }
            }
        }
        FocusOp::Move { id, .. } => {
            if live_focus(id)?.is_none() {
                return Err(invalid(format!("{id} is not queued")));
            }
        }
    }

    let current = ordered(&state);
    let has_duplicate = current.windows(2).any(|w| w[0].1 == w[1].1);
    if has_duplicate {
        for (index, (id, _)) in current.iter().enumerate() {
            state.insert(id, Some(index as i64 + 1));
        }
    }

    let current = ordered(&state);
    match op {
        FocusOp::Add { ids, at } => {
            let n = current.len();
            let at = at.map_or(n + 1, |k| k.min(n + 1));
            let start = match current.get(at - 1) {
                Some(&(_, f)) => f,
                None => current.last().map_or(1, |&(_, f)| f + 1),
            };
            let len = ids.len() as i64;
            for &(id, f) in &current[at - 1..] {
                state.insert(id, Some(f + len));
            }
            for (offset, id) in ids.iter().enumerate() {
                state.insert(id.as_str(), Some(start + offset as i64));
            }
        }
        FocusOp::Remove { ids } => {
            for id in ids {
                state.insert(id.as_str(), None);
            }
        }
        FocusOp::Move { id, up } => {
            let p = current
                .iter()
                .position(|&(q, _)| q == id.as_str())
                .expect("a queued id is in the queue");
            let neighbour = if *up { p.checked_sub(1) } else { Some(p + 1) };
            if let Some(&(other, other_focus)) = neighbour.and_then(|q| current.get(q)) {
                let own_focus = current[p].1;
                state.insert(id.as_str(), Some(other_focus));
                state.insert(other, Some(own_focus));
            }
        }
    }

    let mut changes: Vec<FocusChange> = state
        .iter()
        .filter(|(id, after)| original.get(*id) != Some(after))
        .map(|(id, after)| FocusChange {
            id: (*id).to_string(),
            before: original[id],
            after: *after,
        })
        .collect();
    changes.sort_by(|a, b| {
        (a.after.is_none(), a.after, &a.id).cmp(&(b.after.is_none(), b.after, &b.id))
    });
    Ok(changes)
}

fn row_focus(row: &TomlValue) -> Option<i64> {
    row.get(FIELD_FOCUS).and_then(TomlValue::as_integer)
}

fn row_status(row: &TomlValue) -> Option<&str> {
    row.get(schema::FIELD_STATUS).and_then(TomlValue::as_str)
}

fn row_is_live(row: &TomlValue) -> bool {
    row_status(row).is_some_and(schema::is_live)
}

/// The stored live rows as the `(id, focus)` pairs [`plan`] takes.
fn live_queue(doc: &TomlValue) -> Vec<(String, Option<i64>)> {
    items_array(doc, schema::ARRAY_BACKLOG)
        .iter()
        .filter(|row| row_is_live(row))
        .filter_map(|row| item_id(row).map(|id| (id.to_string(), row_focus(row))))
        .collect()
}

/// The stored queue's ids in queue order.
pub(crate) fn queue_order(doc: &TomlValue) -> Vec<String> {
    let mut queued: Vec<(i64, String)> = live_queue(doc)
        .into_iter()
        .filter_map(|(id, focus)| focus.map(|f| (f, id)))
        .collect();
    queued.sort();
    queued.into_iter().map(|(_, id)| id).collect()
}

/// One stale record per difference from `expect`: an expected id gone (on
/// `id`) or no longer live (on `status`), a differing `focus`, or a queued
/// row `expect` omits. An omitted unqueued row is no difference, so a
/// concurrent capture never makes a write stale.
fn stale_against(doc: &TomlValue, expect: &[(String, Option<i64>)]) -> Vec<StaleOp> {
    let rows = items_array(doc, schema::ARRAY_BACKLOG);
    let mut stale: Vec<StaleOp> = Vec::new();
    let mut named: BTreeSet<&str> = BTreeSet::new();
    for (id, expected) in expect {
        if !named.insert(id.as_str()) {
            continue;
        }
        let matching: Vec<&TomlValue> = rows
            .iter()
            .filter(|row| item_id(row) == Some(id.as_str()))
            .collect();
        if matching.is_empty() {
            stale.push(StaleOp {
                id: id.clone(),
                field: schema::FIELD_ID.into(),
                expected: json!(id),
                found: JsonValue::Null,
            });
        } else if let Some(row) = matching.iter().find(|row| !row_is_live(row)) {
            stale.push(StaleOp {
                id: id.clone(),
                field: schema::FIELD_STATUS.into(),
                expected: json!(schema::LIVE_STATUSES),
                found: row_status(row).map_or(JsonValue::Null, |s| json!(s)),
            });
        } else if let Some(found) = matching
            .iter()
            .map(|row| row_focus(row))
            .find(|found| found != expected)
        {
            stale.push(StaleOp {
                id: id.clone(),
                field: FIELD_FOCUS.into(),
                expected: json!(expected),
                found: json!(found),
            });
        }
    }
    for row in rows.iter().filter(|row| row_is_live(row)) {
        if let (Some(id), Some(found)) = (item_id(row), row_focus(row))
            && !named.contains(id)
        {
            stale.push(StaleOp {
                id: id.to_string(),
                field: FIELD_FOCUS.into(),
                expected: JsonValue::Null,
                found: json!(found),
            });
        }
    }
    stale
}

fn unknown(doc: &TomlValue, id: &str) -> anyhow::Error {
    let compacted = items_array(doc, schema::ARRAY_COMPACTED)
        .iter()
        .any(|item| item_id(item) == Some(id));
    if compacted {
        invalid(format!(
            "backlog id \"{id}\" has been compacted; the queue holds only live `[[{}]]` rows",
            schema::ARRAY_BACKLOG
        ))
    } else {
        invalid(format!("no backlog item with id \"{id}\""))
    }
}

/// What one [`apply_doc`] call did.
struct Outcome {
    changed: Vec<FocusChange>,
    stale: Vec<StaleOp>,
    queue: Vec<String>,
}

/// Plan `op` against `doc` and write the changes into it, every changed row
/// validated before the first is stored. With `expect` naming a queue other
/// than the stored one, `doc` is left untouched and the differences are
/// returned as stale records.
fn apply_doc(
    doc: &mut TomlValue,
    op: &FocusOp,
    expect: Option<&[(String, Option<i64>)]>,
    today: toml::value::Datetime,
) -> Result<Outcome> {
    if let Some(expect) = expect {
        let stale = stale_against(doc, expect);
        if !stale.is_empty() {
            return Ok(Outcome {
                changed: Vec::new(),
                stale,
                queue: queue_order(doc),
            });
        }
    }

    let rows = items_array(doc, schema::ARRAY_BACKLOG);
    let find = |id: &str| -> Result<&TomlValue> {
        rows.iter()
            .find(|row| item_id(row) == Some(id))
            .ok_or_else(|| unknown(doc, id))
    };
    let require_live = |id: &str| -> Result<()> {
        let row = find(id)?;
        if row_is_live(row) {
            Ok(())
        } else {
            Err(invalid(format!(
                "{id} is `{}`; only open or promoted rows join the queue",
                row_status(row).unwrap_or("unknown")
            )))
        }
    };

    // A terminal row can still carry `focus` written by an older binary;
    // removing it is planned here, since `plan` sees only live rows.
    let mut strays: Vec<FocusChange> = Vec::new();
    let planned = match op {
        FocusOp::Add { ids, .. } => {
            for id in ids {
                require_live(id)?;
            }
            op.clone()
        }
        FocusOp::Move { id, .. } => {
            require_live(id)?;
            op.clone()
        }
        FocusOp::Remove { ids } => {
            ensure_distinct(ids)?;
            let mut live: Vec<String> = Vec::new();
            for id in ids {
                let row = find(id)?;
                if row_is_live(row) {
                    live.push(id.clone());
                } else if let Some(before) = row_focus(row) {
                    strays.push(FocusChange {
                        id: id.clone(),
                        before: Some(before),
                        after: None,
                    });
                } else {
                    return Err(invalid(format!("{id} is not queued")));
                }
            }
            FocusOp::Remove { ids: live }
        }
    };

    let stray_ids: BTreeSet<String> = strays.iter().map(|c| c.id.clone()).collect();
    let mut changed = plan(&live_queue(doc), &planned)?;
    changed.extend(strays);

    let mut staged: Vec<(usize, TomlValue)> = Vec::new();
    for change in &changed {
        let live_target = !stray_ids.contains(&change.id);
        for (index, row) in rows.iter().enumerate() {
            if item_id(row) != Some(change.id.as_str())
                || row_focus(row) != change.before
                || row_is_live(row) != live_target
            {
                continue;
            }
            let mut row = row.clone();
            let table = row.as_table_mut().ok_or_else(|| {
                invalid(format!(
                    "`[[{}]]` entry is not a table",
                    schema::ARRAY_BACKLOG
                ))
            })?;
            match change.after {
                Some(after) => {
                    table.insert(FIELD_FOCUS.to_string(), TomlValue::Integer(after));
                }
                None => {
                    table.remove(FIELD_FOCUS);
                }
            }
            schema::validate(&toml_to_json(&row)).map_err(|e| e.into_tagged(None))?;
            staged.push((index, row));
        }
    }

    if !staged.is_empty() {
        let rows = items_array_mut(doc, schema::ARRAY_BACKLOG)?;
        for (index, row) in staged {
            rows[index] = row;
        }
        if let Some(table) = doc.as_table_mut() {
            table.insert(FIELD_LAST_UPDATED.to_string(), TomlValue::Datetime(today));
        }
    }
    Ok(Outcome {
        changed,
        stale: Vec::new(),
        queue: queue_order(doc),
    })
}

/// Applies `op` to the store under `root` in one locked write and returns the
/// envelope the CLI prints. `applied` names every changed id, shifted and
/// renumbered neighbours included. With `expect`, nothing is written unless
/// the stored queue matches it.
pub(crate) fn apply(
    root: &Path,
    op: &FocusOp,
    expect: Option<&[(String, Option<i64>)]>,
    integrity: &WriteIntegrityArgs,
    dry_run: bool,
) -> Result<JsonValue> {
    let path = root.join(".claude").join("backlog.toml");
    let today = crate::time::today_toml_date()?;
    let outcome = if dry_run {
        apply_doc(&mut preview_doc(&path, integrity)?, op, expect, today)?
    } else {
        let opts = write_integrity_opts(integrity);
        let on_missing = on_missing_for(&path, integrity.no_create)?;
        let mut outcome = None;
        let created =
            mutate_doc_conditional(&path, integrity.allow_outside, opts, on_missing, |doc| {
                let result = apply_doc(doc, op, expect, today)?;
                let wrote = !result.changed.is_empty();
                outcome = Some(result);
                Ok(wrote)
            })?;
        warn_if_created(&path, created);
        outcome.expect("the closure runs on every success path")
    };
    let applied: Vec<&str> = outcome.changed.iter().map(|c| c.id.as_str()).collect();
    let mut out = json!({
        "ok": true,
        "applied": applied,
        "skipped_stale": outcome.stale.iter().map(StaleOp::to_json).collect::<Vec<_>>(),
        "changed": outcome.changed.iter().map(FocusChange::to_json).collect::<Vec<_>>(),
        "queue": outcome.queue,
    });
    if dry_run {
        out["dry_run"] = json!(true);
    }
    Ok(out)
}

/// `backlog focus`: applies `op` against the current store and prints the
/// envelope.
pub(crate) fn run_focus(
    root: &Path,
    op: &FocusOp,
    integrity: &WriteIntegrityArgs,
    dry_run: bool,
) -> Result<()> {
    let out = apply(root, op, None, integrity, dry_run)?;
    print_json_compact(&out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::with_root;
    use std::path::PathBuf;

    fn q(rows: &[(&str, Option<i64>)]) -> Vec<(String, Option<i64>)> {
        rows.iter().map(|(id, f)| (id.to_string(), *f)).collect()
    }

    fn ids(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    fn ch(id: &str, before: Option<i64>, after: Option<i64>) -> FocusChange {
        FocusChange {
            id: id.to_string(),
            before,
            after,
        }
    }

    fn kind_of(err: &anyhow::Error) -> &'static str {
        err.downcast_ref::<crate::errors::TaggedError>()
            .map_or("other", |tagged| tagged.kind.as_str())
    }

    #[test]
    fn add_at_the_bottom_takes_the_next_value() {
        let queue = q(&[("a", Some(1)), ("b", Some(2)), ("c", None), ("d", None)]);
        let op = FocusOp::Add {
            ids: ids(&["d", "c"]),
            at: None,
        };
        assert_eq!(
            plan(&queue, &op).unwrap(),
            vec![ch("d", None, Some(3)), ch("c", None, Some(4))]
        );
    }

    #[test]
    fn add_into_an_empty_queue_starts_at_one() {
        let queue = q(&[("a", None)]);
        let op = FocusOp::Add {
            ids: ids(&["a"]),
            at: None,
        };
        assert_eq!(plan(&queue, &op).unwrap(), vec![ch("a", None, Some(1))]);
    }

    #[test]
    fn insert_at_k_shifts_the_rows_from_k_down() {
        let queue = q(&[("a", Some(1)), ("b", Some(2)), ("c", Some(3)), ("x", None)]);
        let op = FocusOp::Add {
            ids: ids(&["x"]),
            at: Some(2),
        };
        assert_eq!(
            plan(&queue, &op).unwrap(),
            vec![
                ch("x", None, Some(2)),
                ch("b", Some(2), Some(3)),
                ch("c", Some(3), Some(4)),
            ]
        );
    }

    #[test]
    fn add_into_a_gapped_queue_follows_the_highest_value() {
        let queue = q(&[("a", Some(1)), ("b", Some(2)), ("c", Some(5)), ("x", None)]);
        let op = FocusOp::Add {
            ids: ids(&["x"]),
            at: None,
        };
        assert_eq!(plan(&queue, &op).unwrap(), vec![ch("x", None, Some(6))]);
    }

    #[test]
    fn insert_into_a_gap_takes_the_displaced_rows_value() {
        let queue = q(&[("a", Some(1)), ("b", Some(2)), ("c", Some(5)), ("x", None)]);
        let op = FocusOp::Add {
            ids: ids(&["x"]),
            at: Some(3),
        };
        assert_eq!(
            plan(&queue, &op).unwrap(),
            vec![ch("x", None, Some(5)), ch("c", Some(5), Some(6))]
        );
    }

    #[test]
    fn an_at_beyond_the_bottom_clamps() {
        let queue = q(&[("a", Some(1)), ("x", None)]);
        let op = FocusOp::Add {
            ids: ids(&["x"]),
            at: Some(9),
        };
        assert_eq!(plan(&queue, &op).unwrap(), vec![ch("x", None, Some(2))]);
    }

    #[test]
    fn invalid_adds_are_refused() {
        let queue = q(&[("a", Some(1)), ("x", None)]);
        for op in [
            FocusOp::Add {
                ids: ids(&["x"]),
                at: Some(0),
            },
            FocusOp::Add {
                ids: ids(&["a"]),
                at: None,
            },
            FocusOp::Add {
                ids: ids(&["nope"]),
                at: None,
            },
            FocusOp::Add {
                ids: ids(&["x", "x"]),
                at: None,
            },
        ] {
            let err = plan(&queue, &op).unwrap_err();
            assert_eq!(kind_of(&err), "validation", "{op:?}: {err:#}");
        }
    }

    #[test]
    fn remove_leaves_a_gap() {
        let queue = q(&[("a", Some(1)), ("b", Some(2)), ("c", Some(3))]);
        let op = FocusOp::Remove { ids: ids(&["b"]) };
        assert_eq!(plan(&queue, &op).unwrap(), vec![ch("b", Some(2), None)]);
    }

    #[test]
    fn remove_or_move_of_an_unqueued_row_is_refused() {
        let queue = q(&[("a", Some(1)), ("x", None)]);
        for op in [
            FocusOp::Remove { ids: ids(&["x"]) },
            FocusOp::Move {
                id: "x".into(),
                up: true,
            },
        ] {
            let err = plan(&queue, &op).unwrap_err();
            assert_eq!(kind_of(&err), "validation", "{op:?}: {err:#}");
        }
    }

    #[test]
    fn move_swaps_with_the_neighbour() {
        let queue = q(&[("a", Some(1)), ("b", Some(2)), ("c", Some(5))]);
        let up = FocusOp::Move {
            id: "c".into(),
            up: true,
        };
        assert_eq!(
            plan(&queue, &up).unwrap(),
            vec![ch("c", Some(5), Some(2)), ch("b", Some(2), Some(5))]
        );
        let down = FocusOp::Move {
            id: "a".into(),
            up: false,
        };
        assert_eq!(
            plan(&queue, &down).unwrap(),
            vec![ch("b", Some(2), Some(1)), ch("a", Some(1), Some(2))]
        );
    }

    #[test]
    fn a_move_past_either_end_changes_nothing() {
        let queue = q(&[("a", Some(1)), ("b", Some(2))]);
        for (id, up) in [("a", true), ("b", false)] {
            let op = FocusOp::Move { id: id.into(), up };
            assert_eq!(plan(&queue, &op).unwrap(), vec![]);
        }
    }

    #[test]
    fn duplicate_values_renumber_the_queue_first() {
        let queue = q(&[("b", Some(3)), ("a", Some(3)), ("c", Some(7))]);
        let op = FocusOp::Move {
            id: "c".into(),
            up: false,
        };
        assert_eq!(
            plan(&queue, &op).unwrap(),
            vec![
                ch("a", Some(3), Some(1)),
                ch("b", Some(3), Some(2)),
                ch("c", Some(7), Some(3)),
            ]
        );
    }

    const STORE: &str = r#"schema_version = 1
last_updated = 2026-08-01

[[backlog]]
id = "B-aaaaaaaa"
kind = "bug"
summary = "first"
area = ""
status = "open"
created = 2026-08-01
last_seen = 2026-08-01
seen_count = 1
dedup_id = "aaaaaaaaaaaaaaaa"
focus = 1

[[backlog]]
id = "B-bbbbbbbb"
kind = "debt"
summary = "second"
area = ""
status = "open"
created = 2026-08-01
last_seen = 2026-08-01
seen_count = 1
dedup_id = "bbbbbbbbbbbbbbbb"
focus = 2

[[backlog]]
id = "B-cccccccc"
kind = "question"
summary = "third"
area = ""
status = "open"
created = 2026-08-01
last_seen = 2026-08-01
seen_count = 1
dedup_id = "cccccccccccccccc"

[[backlog]]
id = "B-eeeeeeee"
kind = "bug"
summary = "done"
area = ""
status = "resolved"
created = 2026-08-01
last_seen = 2026-08-01
seen_count = 1
dedup_id = "eeeeeeeeeeeeeeee"
resolved = 2026-08-02
resolution = "fixed"
focus = 3
"#;

    fn seed(root: &Path) -> PathBuf {
        let file = root.join(".claude").join("backlog.toml");
        std::fs::write(&file, STORE).unwrap();
        file
    }

    fn write_args() -> WriteIntegrityArgs {
        WriteIntegrityArgs {
            allow_outside: false,
            no_write_integrity: false,
            verify_integrity: false,
            strict_integrity: false,
            no_create: true,
        }
    }

    fn stored_focus(file: &Path, id: &str) -> Option<i64> {
        let doc: TomlValue = toml::from_str(&std::fs::read_to_string(file).unwrap()).unwrap();
        items_array(&doc, schema::ARRAY_BACKLOG)
            .iter()
            .find(|row| item_id(row) == Some(id))
            .and_then(row_focus)
    }

    #[test]
    fn apply_writes_the_plan_and_reports_the_queue() {
        let (out, focus) = with_root(|root| {
            let file = seed(root);
            let op = FocusOp::Add {
                ids: ids(&["B-cccccccc"]),
                at: Some(1),
            };
            let out = apply(root, &op, None, &write_args(), false).unwrap();
            let focus: Vec<Option<i64>> = ["B-aaaaaaaa", "B-bbbbbbbb", "B-cccccccc"]
                .iter()
                .map(|id| stored_focus(&file, id))
                .collect();
            (out, focus)
        });
        assert_eq!(focus, vec![Some(2), Some(3), Some(1)]);
        assert_eq!(
            out["queue"],
            json!(["B-cccccccc", "B-aaaaaaaa", "B-bbbbbbbb"])
        );
        assert_eq!(
            out["applied"],
            json!(["B-cccccccc", "B-aaaaaaaa", "B-bbbbbbbb"])
        );
        assert_eq!(out["skipped_stale"], json!([]));
    }

    #[test]
    fn remove_drops_a_terminal_rows_stray_focus() {
        let (out, focus) = with_root(|root| {
            let file = seed(root);
            let op = FocusOp::Remove {
                ids: ids(&["B-eeeeeeee"]),
            };
            let out = apply(root, &op, None, &write_args(), false).unwrap();
            (out, stored_focus(&file, "B-eeeeeeee"))
        });
        assert_eq!(focus, None);
        assert_eq!(
            out["changed"],
            json!([{"id": "B-eeeeeeee", "before": 3, "after": null}])
        );
    }

    #[test]
    fn add_of_a_terminal_or_unknown_row_is_refused() {
        let results = with_root(|root| {
            let file = seed(root);
            let errs: Vec<&'static str> = ["B-eeeeeeee", "B-nosuchid"]
                .iter()
                .map(|id| {
                    let op = FocusOp::Add {
                        ids: ids(&[id]),
                        at: None,
                    };
                    kind_of(&apply(root, &op, None, &write_args(), false).unwrap_err())
                })
                .collect();
            (errs, std::fs::read(&file).unwrap())
        });
        assert_eq!(results.0, vec!["validation", "validation"]);
        assert_eq!(results.1, STORE.as_bytes());
    }

    #[test]
    fn a_stale_expect_writes_nothing() {
        let (out, bytes) = with_root(|root| {
            let file = seed(root);
            let op = FocusOp::Move {
                id: "B-bbbbbbbb".into(),
                up: true,
            };
            let expect = q(&[("B-aaaaaaaa", Some(1)), ("B-bbbbbbbb", Some(4))]);
            let out = apply(root, &op, Some(&expect), &write_args(), false).unwrap();
            (out, std::fs::read(&file).unwrap())
        });
        assert_eq!(bytes, STORE.as_bytes());
        assert_eq!(out["applied"], json!([]));
        assert_eq!(
            out["skipped_stale"],
            json!([{"id": "B-bbbbbbbb", "field": "focus", "expected": 4, "found": 2}])
        );
    }

    #[test]
    fn an_unnamed_queued_row_makes_expect_stale() {
        let out = with_root(|root| {
            seed(root);
            let op = FocusOp::Remove {
                ids: ids(&["B-aaaaaaaa"]),
            };
            let expect = q(&[("B-aaaaaaaa", Some(1))]);
            apply(root, &op, Some(&expect), &write_args(), false).unwrap()
        });
        assert_eq!(
            out["skipped_stale"],
            json!([{"id": "B-bbbbbbbb", "field": "focus", "expected": null, "found": 2}])
        );
    }

    #[test]
    fn a_concurrent_unranked_capture_is_not_stale() {
        let (out, focus) = with_root(|root| {
            let file = seed(root);
            let op = FocusOp::Move {
                id: "B-bbbbbbbb".into(),
                up: true,
            };
            // `B-cccccccc` is live and unranked, and absent from `expect`.
            let expect = q(&[("B-aaaaaaaa", Some(1)), ("B-bbbbbbbb", Some(2))]);
            let out = apply(root, &op, Some(&expect), &write_args(), false).unwrap();
            (out, stored_focus(&file, "B-bbbbbbbb"))
        });
        assert_eq!(out["skipped_stale"], json!([]));
        assert_eq!(focus, Some(1));
        assert_eq!(out["queue"], json!(["B-bbbbbbbb", "B-aaaaaaaa"]));
    }

    #[test]
    fn dry_run_reports_without_writing() {
        let (out, bytes) = with_root(|root| {
            let file = seed(root);
            let op = FocusOp::Remove {
                ids: ids(&["B-aaaaaaaa"]),
            };
            let out = apply(root, &op, None, &write_args(), true).unwrap();
            (out, std::fs::read(&file).unwrap())
        });
        assert_eq!(bytes, STORE.as_bytes());
        assert_eq!(out["dry_run"], json!(true));
        assert_eq!(out["queue"], json!(["B-bbbbbbbb"]));
    }
}
