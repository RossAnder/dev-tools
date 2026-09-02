//! `backlog compact` — fold terminal items into the `compacted` array.
//!
//! Age is whole days between a row's terminal date and today, compared
//! strictly: a row dated exactly `today - threshold` stays.
//!
//! A terminal row whose date is missing or unreadable is left in place with a
//! stderr note; the sweep runs unattended, where one bad row must neither
//! vanish nor stop the run.
//!
//! Nothing here touches an evidence directory. A folded row keeps its id, and
//! `evidence::resolve_id` reads both arrays, so the drop-box stays reachable.

use std::path::Path;
use std::time::Duration;

use anyhow::Result;
use jiff::civil::Date;
use serde_json::json;
use toml::Value as TomlValue;
use toml::value::Datetime as TomlDatetime;

use super::schema::{
    self, ARRAY_BACKLOG, ARRAY_COMPACTED, FIELD_COMPACTED_ON, FIELD_ID, FIELD_LAST_UPDATED,
    FIELD_STATUS, FIELD_TERMINAL_DATE, FIELD_TERMINAL_REASON,
};
use crate::cli::{WriteIntegrityArgs, write_integrity_opts};
use crate::io::{
    dry_run_read_opts, items_array, items_array_mut, mutate_doc_conditional, on_missing_for,
    read_doc, relativise, repo_or_cwd_root,
};
use crate::output::print_json_compact;

const SECONDS_PER_DAY: u64 = 86_400;

pub(crate) fn dispatch(
    older_than: String,
    dry_run: bool,
    integrity: WriteIntegrityArgs,
) -> Result<()> {
    let threshold = crate::time::parse_threshold(&older_than)?;
    let today = crate::time::today_utc_date()?;
    let path = schema::backlog_path()?;

    // Ahead of the auto-create policy: a sweep that finds no store must not
    // leave one behind.
    if !path.exists() {
        return if dry_run {
            emit_preview(&[], 0)
        } else {
            emit_result(&path, 0, 0)
        };
    }

    if dry_run {
        let opts = dry_run_read_opts(integrity.verify_integrity);
        let plan = read_doc(&path, opts, |doc| plan_compaction(doc, today, threshold))?;
        return emit_preview(&plan.compacted, plan.remaining);
    }

    let opts = write_integrity_opts(&integrity);
    let on_missing = on_missing_for(&path, integrity.no_create)?;
    let mut counts: Option<(usize, usize)> = None;
    mutate_doc_conditional(&path, integrity.allow_outside, opts, on_missing, |doc| {
        let plan = plan_compaction(doc, today, threshold)?;
        counts = Some((plan.compacted.len(), plan.remaining));
        // Returning false skips the write, so a sweep with nothing to fold
        // leaves the store and its sidecar byte-identical.
        if plan.compacted.is_empty() {
            return Ok(false);
        }
        *doc = plan.new_doc;
        Ok(true)
    })?;
    let (compacted, remaining) = counts.expect("the closure runs on every success path");
    emit_result(&path, compacted, remaining)
}

/// The document as it would stand after the fold, plus what moved. Built
/// whole so the dry-run and live paths share one computation.
struct CompactionPlan {
    new_doc: TomlValue,
    compacted: Vec<String>,
    remaining: usize,
}

fn plan_compaction(doc: &TomlValue, today: Date, threshold: Duration) -> Result<CompactionPlan> {
    let compacted_on = toml_date(today)?;
    let mut kept: Vec<TomlValue> = Vec::new();
    let mut folded: Vec<TomlValue> = Vec::new();
    let mut ids: Vec<String> = Vec::new();

    for item in items_array(doc, ARRAY_BACKLOG) {
        match due(item, today, threshold) {
            Some(terminal) => {
                ids.push(id_of(item).to_string());
                folded.push(compacted_row(item, terminal, compacted_on));
            }
            None => kept.push(item.clone()),
        }
    }

    let remaining = kept.len();
    let mut new_doc = doc.clone();
    if !folded.is_empty() {
        *items_array_mut(&mut new_doc, ARRAY_BACKLOG)? = kept;
        items_array_mut(&mut new_doc, ARRAY_COMPACTED)?.extend(folded);
        if let Some(root) = new_doc.as_table_mut() {
            root.insert(
                FIELD_LAST_UPDATED.to_string(),
                TomlValue::Datetime(compacted_on),
            );
        }
    }
    Ok(CompactionPlan {
        new_doc,
        compacted: ids,
        remaining,
    })
}

/// The date/companion field pair a row's own `status` names. `open` and any
/// unrecognised status yield `None`, which is what keeps them out of the fold
/// no matter how old they are.
fn terminal_cluster(item: &TomlValue) -> Option<(&str, &'static str, &'static str)> {
    let status = item.get(FIELD_STATUS).and_then(TomlValue::as_str)?;
    let (date_field, reason_field) = schema::terminal_pair(status)?;
    Some((status, date_field, reason_field))
}

/// The terminal date of a row old enough to fold, or `None`.
fn due(item: &TomlValue, today: Date, threshold: Duration) -> Option<TomlDatetime> {
    let (status, date_field, _) = terminal_cluster(item)?;
    let Some((stored, civil)) = read_date(item, date_field) else {
        eprintln!(
            "tomlctl: backlog item {} is status=\"{status}\" with no readable `{date_field}` date — leaving it in place",
            id_of(item)
        );
        return None;
    };
    let age = crate::time::age_days(civil, today).saturating_mul(SECONDS_PER_DAY);
    (age > threshold.as_secs()).then_some(stored)
}

/// A date field in both the forms a store can hold it: native TOML, and the
/// quoted string carried by rows written before `promoted` / `dismissed` joined
/// the `items add` ISO-string promotion set. Both shapes stay readable.
fn read_date(item: &TomlValue, field: &str) -> Option<(TomlDatetime, Date)> {
    let stored = match item.get(field)? {
        TomlValue::Datetime(dt) => *dt,
        TomlValue::String(s) => s.parse::<TomlDatetime>().ok()?,
        _ => return None,
    };
    let date = stored.date?;
    let civil = Date::new(date.year as i16, date.month as i8, date.day as i8).ok()?;
    Some((stored, civil))
}

/// Project a live row onto `COMPACTED_FIELDS` — driven by that constant, so
/// the row carries every pinned key and nothing else. Absent source fields
/// land as empty strings; every original field outside the projection is
/// dropped.
fn compacted_row(
    item: &TomlValue,
    terminal_date: TomlDatetime,
    compacted_on: TomlDatetime,
) -> TomlValue {
    let reason_field = terminal_cluster(item).map_or("", |(_, _, reason)| reason);
    let mut row = toml::map::Map::new();
    for field in schema::COMPACTED_FIELDS {
        let value = match *field {
            FIELD_TERMINAL_DATE => TomlValue::Datetime(terminal_date),
            FIELD_COMPACTED_ON => TomlValue::Datetime(compacted_on),
            FIELD_TERMINAL_REASON => TomlValue::String(text(item, reason_field)),
            other => TomlValue::String(text(item, other)),
        };
        row.insert((*field).to_string(), value);
    }
    TomlValue::Table(row)
}

fn text(item: &TomlValue, field: &str) -> String {
    item.get(field)
        .and_then(TomlValue::as_str)
        .unwrap_or_default()
        .to_string()
}

fn id_of(item: &TomlValue) -> &str {
    item.get(FIELD_ID)
        .and_then(TomlValue::as_str)
        .unwrap_or("<no id>")
}

fn toml_date(date: Date) -> Result<TomlDatetime> {
    let iso = date.to_string();
    iso.parse::<TomlDatetime>()
        .map_err(|e| anyhow::anyhow!("converting {iso} to a TOML date: {e}"))
}

fn emit_result(path: &Path, compacted: usize, remaining: usize) -> Result<()> {
    print_json_compact(&json!({
        "ok": true,
        "compacted": compacted,
        "remaining": remaining,
        "path": relativise(&repo_or_cwd_root()?, path),
    }))
}

fn emit_preview(compacted: &[String], remaining: usize) -> Result<()> {
    print_json_compact(&json!({
        "ok": true,
        "dry_run": true,
        "would_change": {
            "kind": "compact",
            "compacted": compacted.len(),
            "remaining": remaining,
            "ids": compacted,
        },
    }))
}

#[cfg(test)]
mod tests {
    use super::super::evidence;
    use super::*;
    use crate::test_support::with_root;
    use std::collections::BTreeSet;
    use std::fs;
    use std::path::PathBuf;

    fn args() -> WriteIntegrityArgs {
        WriteIntegrityArgs {
            allow_outside: false,
            no_write_integrity: false,
            verify_integrity: false,
            strict_integrity: false,
            no_create: false,
        }
    }

    fn kind_of(err: &anyhow::Error) -> &'static str {
        err.downcast_ref::<crate::errors::TaggedError>()
            .map_or("other", |tagged| tagged.kind.as_str())
    }

    fn today() -> Date {
        "2026-09-01".parse().unwrap()
    }

    fn ago(days: i64) -> String {
        today()
            .checked_sub(jiff::Span::new().days(days))
            .unwrap()
            .to_string()
    }

    fn ninety_days() -> Duration {
        Duration::from_secs(90 * SECONDS_PER_DAY)
    }

    fn doc(body: &str) -> TomlValue {
        toml::from_str(body).unwrap()
    }

    /// One resolved row dated `date`, carrying a value in every field the
    /// projection reads plus two it must drop.
    fn resolved_store(date: &str) -> TomlValue {
        doc(&format!(
            r#"schema_version = 1
last_updated = 2026-01-01

[[backlog]]
id = "B-7f0e2d91"
kind = "bug"
summary = "checkout total overlaps the confirm button"
area = "web/src/Checkout.vue"
status = "resolved"
resolved = {date}
resolution = "fixed in abc123"
context = "Only below 1400px."
dedup_id = "7f0e2d91aabbccdd"
created = 2026-01-01
last_seen = 2026-01-02
seen_count = 3
tags = ["ui"]
"#
        ))
    }

    fn ids_in(doc: &TomlValue, array: &str) -> Vec<String> {
        items_array(doc, array)
            .iter()
            .map(|item| id_of(item).to_string())
            .collect()
    }

    #[test]
    fn an_open_row_is_never_folded_however_old() {
        let d = doc(&format!(
            r#"
[[backlog]]
id = "B-a1b2c3d4"
summary = "five years old and still open"
status = "open"
created = {}
last_seen = {}
"#,
            ago(1825),
            ago(1825)
        ));
        let plan = plan_compaction(&d, today(), Duration::from_secs(SECONDS_PER_DAY)).unwrap();
        assert!(plan.compacted.is_empty());
        assert_eq!(plan.remaining, 1);
        assert!(items_array(&plan.new_doc, ARRAY_COMPACTED).is_empty());
    }

    #[test]
    fn older_than_excludes_the_boundary_and_includes_the_day_past_it() {
        // Pin the fixture arithmetic itself, so a wrong `ago` would fail here
        // rather than silently shift what "the boundary" means below.
        assert_eq!(ago(90), "2026-06-03");
        assert_eq!(ago(91), "2026-06-02");

        let at = plan_compaction(&resolved_store(&ago(90)), today(), ninety_days()).unwrap();
        assert!(
            at.compacted.is_empty(),
            "exactly at the threshold must stay"
        );
        assert_eq!(at.remaining, 1);
        assert!(items_array(&at.new_doc, ARRAY_COMPACTED).is_empty());

        let past = plan_compaction(&resolved_store(&ago(91)), today(), ninety_days()).unwrap();
        assert_eq!(past.compacted, vec!["B-7f0e2d91".to_string()]);
        assert_eq!(past.remaining, 0);
        assert_eq!(ids_in(&past.new_doc, ARRAY_COMPACTED), ["B-7f0e2d91"]);
        assert!(items_array(&past.new_doc, ARRAY_BACKLOG).is_empty());
    }

    #[test]
    fn a_future_dated_row_reads_as_brand_new() {
        let plan = plan_compaction(
            &resolved_store("2027-01-01"),
            today(),
            Duration::from_secs(SECONDS_PER_DAY),
        )
        .unwrap();
        assert!(plan.compacted.is_empty());
    }

    #[test]
    fn the_folded_row_carries_the_pinned_fields_and_nothing_else() {
        let plan = plan_compaction(&resolved_store(&ago(200)), today(), ninety_days()).unwrap();
        let row = &items_array(&plan.new_doc, ARRAY_COMPACTED)[0];
        let table = row.as_table().unwrap();

        let got: BTreeSet<&str> = table.keys().map(String::as_str).collect();
        let want: BTreeSet<&str> = schema::COMPACTED_FIELDS.iter().copied().collect();
        assert_eq!(got, want);

        assert_eq!(table["id"].as_str(), Some("B-7f0e2d91"));
        assert_eq!(table["dedup_id"].as_str(), Some("7f0e2d91aabbccdd"));
        assert_eq!(table["kind"].as_str(), Some("bug"));
        assert_eq!(table["area"].as_str(), Some("web/src/Checkout.vue"));
        assert_eq!(table["status"].as_str(), Some("resolved"));
        assert_eq!(table["context"].as_str(), Some("Only below 1400px."));
        assert_eq!(table["terminal_reason"].as_str(), Some("fixed in abc123"));
        assert_eq!(
            table["terminal_date"].as_datetime().unwrap().to_string(),
            ago(200)
        );
        assert_eq!(
            table["compacted_on"].as_datetime().unwrap().to_string(),
            "2026-09-01"
        );
    }

    #[test]
    fn an_absent_context_folds_to_an_empty_string() {
        let mut d = resolved_store(&ago(200));
        d["backlog"][0]
            .as_table_mut()
            .unwrap()
            .remove(schema::FIELD_CONTEXT);
        let plan = plan_compaction(&d, today(), ninety_days()).unwrap();
        let row = &items_array(&plan.new_doc, ARRAY_COMPACTED)[0];
        assert_eq!(row["context"].as_str(), Some(""));
    }

    #[test]
    fn a_terminal_row_without_a_readable_date_is_skipped_not_rejected() {
        for body in [
            r#"
[[backlog]]
id = "B-7f0e2d91"
summary = "no date at all"
status = "resolved"
resolution = "fixed in abc123"
"#,
            r#"
[[backlog]]
id = "B-7f0e2d91"
summary = "unparseable date"
status = "resolved"
resolved = "last tuesday"
resolution = "fixed in abc123"
"#,
        ] {
            let plan =
                plan_compaction(&doc(body), today(), Duration::from_secs(SECONDS_PER_DAY)).unwrap();
            assert!(plan.compacted.is_empty(), "{body}");
            assert_eq!(plan.remaining, 1, "{body}");
        }
    }

    #[test]
    fn a_string_shaped_terminal_date_still_folds() {
        let d = doc(&format!(
            r#"
[[backlog]]
id = "B-7f0e2d91"
summary = "dismissed with a quoted date"
status = "dismissed"
dismissed = "{}"
dismiss_reason = "not reproducible"
"#,
            ago(200)
        ));
        let plan = plan_compaction(&d, today(), ninety_days()).unwrap();
        assert_eq!(plan.compacted, vec!["B-7f0e2d91".to_string()]);
        let row = &items_array(&plan.new_doc, ARRAY_COMPACTED)[0];
        assert_eq!(
            row["terminal_date"].as_datetime().unwrap().to_string(),
            ago(200)
        );
        assert_eq!(row["terminal_reason"].as_str(), Some("not reproducible"));
    }

    /// Ancient enough that the live paths below never depend on the wall
    /// clock: any threshold a test passes is far short of this row's age.
    ///
    /// The leading comment is load-bearing. A round-trip through the TOML
    /// serialiser drops it, so a byte-identity assertion over this fixture
    /// distinguishes "did not write" from "wrote the same bytes back".
    const STORE: &str = r#"# hand-edited capture log
schema_version = 1
last_updated = 2020-01-02

[[backlog]]
id = "B-a1b2c3d4"
kind = "flaky-test"
summary = "readiness probe flakes on slow CI"
area = "lumina/server/tests/probe.rs"
status = "open"
dedup_id = "a1b2c3d4e5f60718"
created = 2020-01-01
last_seen = 2020-01-01
seen_count = 1

[[backlog]]
id = "B-7f0e2d91"
kind = "bug"
summary = "checkout total overlaps the confirm button"
area = "web/src/Checkout.vue"
status = "resolved"
resolved = 2020-01-02
resolution = "fixed in abc123"
context = "Only below 1400px."
dedup_id = "7f0e2d91aabbccdd"
created = 2020-01-01
last_seen = 2020-01-01
seen_count = 3
"#;

    fn seed_store(root: &Path) -> PathBuf {
        let path = root.join(".claude").join("backlog.toml");
        fs::write(&path, STORE).unwrap();
        crate::io::write_sidecar_for(&path, STORE.as_bytes()).unwrap();
        path
    }

    fn bytes_of(path: &Path) -> (Vec<u8>, Vec<u8>) {
        (
            fs::read(path).unwrap(),
            fs::read(crate::integrity::sidecar_path(path)).unwrap(),
        )
    }

    #[test]
    fn dry_run_leaves_the_store_and_its_sidecar_byte_identical() {
        with_root(|root| {
            let path = seed_store(root);
            let before = bytes_of(&path);
            dispatch("90d".to_string(), true, args()).unwrap();
            assert_eq!(bytes_of(&path), before);
        });
    }

    #[test]
    fn a_live_sweep_moves_the_terminal_row_and_bumps_last_updated() {
        with_root(|root| {
            let path = seed_store(root);
            dispatch("90d".to_string(), false, args()).unwrap();
            let after = crate::io::read_toml(&path).unwrap();

            assert_eq!(ids_in(&after, ARRAY_BACKLOG), ["B-a1b2c3d4"]);
            assert_eq!(ids_in(&after, ARRAY_COMPACTED), ["B-7f0e2d91"]);
            assert_eq!(
                after[FIELD_LAST_UPDATED].as_datetime().unwrap().to_string(),
                crate::time::today_utc_iso().unwrap()
            );
            assert_eq!(schema::validate_ids_unique(&after), Ok(()));
        });
    }

    #[test]
    fn a_sweep_with_nothing_to_fold_leaves_the_store_untouched() {
        with_root(|root| {
            let path = seed_store(root);
            let before = bytes_of(&path);
            dispatch("9999d".to_string(), false, args()).unwrap();
            assert_eq!(bytes_of(&path), before);
        });
    }

    #[test]
    fn a_missing_store_stays_missing() {
        with_root(|root| {
            let path = root.join(".claude").join("backlog.toml");
            dispatch("90d".to_string(), false, args()).unwrap();
            assert!(!path.exists());
            dispatch("90d".to_string(), true, args()).unwrap();
            assert!(!path.exists());
        });
    }

    #[test]
    fn a_folded_rows_evidence_directory_survives_intact() {
        with_root(|root| {
            seed_store(root);
            let dir = root
                .join(".claude")
                .join(evidence::EVIDENCE_ROOT_NAME)
                .join("B-7f0e2d91");
            fs::create_dir_all(&dir).unwrap();
            fs::write(
                dir.join(evidence::MARKER_NAME),
                evidence::marker_text("B-7f0e2d91", "checkout total overlaps", Some(true)),
            )
            .unwrap();
            fs::write(dir.join("shot.png"), b"1234").unwrap();

            dispatch("90d".to_string(), false, args()).unwrap();

            assert_eq!(
                evidence::list_dir(&dir).unwrap(),
                Some(vec![("shot.png".to_string(), 4)])
            );
        });
    }

    #[test]
    fn an_unparseable_older_than_is_a_validation_error() {
        for input in ["9 days", "90", "d", ""] {
            let err = dispatch(input.to_string(), false, args()).unwrap_err();
            assert_eq!(kind_of(&err), "validation", "{input}");
        }
    }
}
