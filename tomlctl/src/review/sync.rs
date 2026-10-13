//! `review sync-v1` — v1 apply outcomes read back onto v2 findings through
//! `v2_ref`.
//!
//! The v1 ledgers read are every distinct `export_ledger` the store's
//! findings record plus the default ledger of each family the store holds
//! (`code` → review, `perf` → optimise) for its slug or scope. Each v1 row
//! whose `v2_ref` names this store moves its finding to the status
//! [`map_status`] gives and upserts one `by = "sync"` verdict citing the row.
//! v1 files are only read; a missing one is skipped. A sync that changes
//! nothing writes nothing.

use std::path::{Path, PathBuf};

use anyhow::Result;
use serde_json::{Value as JsonValue, json};
use toml::Value as TomlValue;
use toml::map::Map;
use toml::value::Datetime;

use super::fields::Family;
use super::store::{self, Finding, Store, Target, Verdict};
use crate::cli::{ReadIntegrityArgs, ReviewSyncArgs};
use crate::io::{read_toml, relativise_under, repo_or_cwd_root};
use crate::ledgers::{LedgerKind, LedgerRef};

/// The v2 statuses a sync may overwrite: the finding as exported (or still
/// `open` when an export's v2 write never landed), or as a sync left it.
const SYNCABLE: &[&str] = &["open", "exported", "resolved", "dismissed", "deferred"];
/// Companions a sync writes, all cleared before the new status's are set.
const SYNC_COMPANIONS: &[&str] = &[
    "resolution",
    "resolved",
    "dismiss_reason",
    "defer_reason",
    "defer_trigger",
];
const RATIONALE_CAP: usize = 600;

/// The v2 status a v1 disposition maps to; `None` for `open` and for any
/// status v1 does not define, which v1 readers treat as `open`.
pub(crate) fn map_status(v1_status: &str) -> Option<&'static str> {
    match v1_status {
        "fixed" | "applied" => Some("resolved"),
        "wontfix" | "wontapply" | "verified-clean" => Some("dismissed"),
        "deferred" => Some("deferred"),
        _ => None,
    }
}

/// One v1 row's disposition: its status, the v2 status it maps to, and the
/// v1 companion prose behind it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct V1Outcome {
    pub(crate) v1_status: String,
    pub(crate) status: &'static str,
    /// `resolution`, `wontfix_rationale`, `wontapply_rationale`,
    /// `verified_note` or `defer_reason`, whichever the status carries.
    pub(crate) resolution: String,
    pub(crate) defer_trigger: String,
    pub(crate) resolved: Option<Datetime>,
}

impl V1Outcome {
    pub(crate) fn from_row(row: &Map<String, TomlValue>) -> Option<Self> {
        let text = |key: &str| {
            row.get(key)
                .and_then(TomlValue::as_str)
                .unwrap_or_default()
                .trim()
                .to_string()
        };
        let v1_status = text("status");
        let status = map_status(&v1_status)?;
        let prose = match v1_status.as_str() {
            "fixed" | "applied" => "resolution",
            "wontfix" => "wontfix_rationale",
            "wontapply" => "wontapply_rationale",
            "verified-clean" => "verified_note",
            _ => "defer_reason",
        };
        let resolved = match row.get("resolved") {
            Some(TomlValue::Datetime(date)) => Some(*date),
            Some(TomlValue::String(raw)) => raw.parse().ok(),
            _ => None,
        };
        Some(Self {
            resolution: text(prose),
            defer_trigger: text("defer_trigger"),
            resolved,
            status,
            v1_status,
        })
    }

    /// `<v1 status>: <prose>`, or the bare status when the row has none.
    pub(crate) fn summary(&self) -> String {
        if self.resolution.is_empty() {
            self.v1_status.clone()
        } else {
            format!("{}: {}", self.v1_status, self.resolution)
        }
    }
}

/// A v1 row whose `v2_ref` names a finding of the store being synced.
#[derive(Debug, Clone)]
pub(crate) struct Linked {
    /// The ledger as cited: repo-relative with `/` when under the root.
    pub(crate) ledger: String,
    pub(crate) v1_id: String,
    pub(crate) finding: String,
    pub(crate) row: Map<String, TomlValue>,
}

/// The v1 ledgers a store's findings can be linked from, deduplicated, in
/// the order recorded `export_ledger`s first, then the family defaults.
pub(crate) fn ledger_paths(
    root: &Path,
    store: &Store,
    slug: Option<&str>,
    scope: Option<&str>,
) -> Result<Vec<PathBuf>> {
    let mut paths: Vec<PathBuf> = Vec::new();
    let mut push = |path: PathBuf| {
        if !paths.contains(&path) {
            paths.push(path);
        }
    };
    for finding in &store.findings {
        if let Some(ledger) = finding
            .rest
            .get("export_ledger")
            .and_then(TomlValue::as_str)
            && !ledger.is_empty()
        {
            push(under_root(root, ledger));
        }
    }
    let held = |family: Family| {
        store.runs.iter().any(|r| r.family == family)
            || store.findings.iter().any(|f| f.family == family)
    };
    let kinds = [
        (Family::Code, LedgerKind::Review),
        (Family::Perf, LedgerKind::Optimise),
    ];
    for (_, kind) in kinds.into_iter().filter(|(family, _)| held(*family)) {
        let ledger = match (slug, scope) {
            (Some(slug), _) => LedgerRef::Flow {
                slug: slug.to_string(),
                kind,
            },
            (None, Some(scope)) => LedgerRef::Scope {
                kind,
                scope: scope.to_string(),
            },
            (None, None) => continue,
        };
        push(ledger.path(root)?);
    }
    Ok(paths)
}

/// Every row of `ledgers` whose `v2_ref` names `store_path`, in ledger then
/// row order. A missing ledger contributes nothing.
pub(crate) fn linked_rows(
    root: &Path,
    store_path: &Path,
    ledgers: &[PathBuf],
) -> Result<Vec<Linked>> {
    let store_key = slashed(store_path);
    let mut linked = Vec::new();
    for path in ledgers {
        if !path.is_file() {
            continue;
        }
        let doc = read_toml(path)?;
        let ledger = relativise_under(root, path).unwrap_or_else(|| slashed(path));
        let rows = doc.get("items").and_then(TomlValue::as_array);
        for row in rows.into_iter().flatten().filter_map(TomlValue::as_table) {
            let text = |key: &str| row.get(key).and_then(TomlValue::as_str).unwrap_or_default();
            let (v1_id, v2_ref) = (text("id"), text("v2_ref"));
            let Some((ref_path, finding)) = v2_ref.rsplit_once('#') else {
                continue;
            };
            if v1_id.is_empty() || slashed(&under_root(root, ref_path)) != store_key {
                continue;
            }
            linked.push(Linked {
                ledger: ledger.clone(),
                v1_id: v1_id.to_string(),
                finding: finding.to_string(),
                row: row.clone(),
            });
        }
    }
    Ok(linked)
}

fn under_root(root: &Path, path: &str) -> PathBuf {
    let path = Path::new(path);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}

fn slashed(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

pub(crate) fn run(target: &Target, args: ReviewSyncArgs) -> Result<()> {
    // Sync runs before any run starts, so an absent store is the common
    // first case and is neither an error nor created.
    if !target.path.is_file() {
        return print(&[], &[], &[], true);
    }
    let root = repo_or_cwd_root()?;
    let read = ReadIntegrityArgs {
        verify_integrity: args.integrity.verify_integrity,
        strict_read: true,
    };
    let current = store::load(&target.path, &read)?;
    let ledgers = ledger_paths(
        &root,
        &current,
        args.store.slug.as_deref(),
        args.store.scope.as_deref(),
    )?;
    let linked = linked_rows(&root, &target.path, &ledgers)?;
    let today = crate::time::today_toml_date()?;

    let mut trial = current.clone();
    let (synced, skipped) = apply(&mut trial, &linked, today);
    let unchanged = trial == current;
    if !unchanged {
        store::mutate(&target.path, &args.integrity, |store| {
            apply(store, &linked, today);
            Ok(())
        })?;
    }
    let shown: Vec<JsonValue> = ledgers
        .iter()
        .map(|path| {
            json!({
                "path": relativise_under(&root, path).unwrap_or_else(|| slashed(path)),
                "missing": !path.is_file(),
            })
        })
        .collect();
    print(&shown, &synced, &skipped, unchanged)
}

fn print(
    ledgers: &[JsonValue],
    synced: &[JsonValue],
    skipped: &[JsonValue],
    unchanged: bool,
) -> Result<()> {
    crate::output::print_json_compact(&json!({
        "ok": true,
        "ledgers": ledgers,
        "synced": synced,
        "skipped": skipped,
        "unchanged": unchanged,
    }))
}

/// Applies every linked row to `store`, returning the synced and skipped
/// entries. A v1 row still `open` (or of an unknown status) is neither.
fn apply(
    store: &mut Store,
    linked: &[Linked],
    today: Datetime,
) -> (Vec<JsonValue>, Vec<JsonValue>) {
    let (mut synced, mut skipped) = (Vec::new(), Vec::new());
    for link in linked {
        let Some(outcome) = V1Outcome::from_row(&link.row) else {
            continue;
        };
        let cited = format!("{}#{}", link.ledger, link.v1_id);
        let Some(index) = store.findings.iter().position(|f| f.id == link.finding) else {
            skipped.push(json!({
                "v1": cited,
                "finding": link.finding,
                "reason": "not in this store",
            }));
            continue;
        };
        let finding = &mut store.findings[index];
        if !SYNCABLE.contains(&finding.status.as_str()) {
            skipped.push(json!({
                "v1": cited,
                "finding": link.finding,
                "reason": format!("finding is {}", finding.status),
            }));
            continue;
        }
        set_status(finding, &outcome, today);
        let record = Verdict {
            finding: finding.id.clone(),
            run: finding.run.clone(),
            by: "sync".to_string(),
            seat: None,
            verdict: "confirmed".to_string(),
            severity: String::new(),
            citation: cited.clone(),
            rationale: capped(&outcome.summary()),
            rest: Map::new(),
        };
        upsert(store, record);
        synced.push(json!({
            "finding": link.finding,
            "v1": cited,
            "v1_status": outcome.v1_status,
            "status": outcome.status,
        }));
    }
    (synced, skipped)
}

fn set_status(finding: &mut Finding, outcome: &V1Outcome, today: Datetime) {
    let kept_date = match (finding.status.as_str(), finding.rest.get("resolved")) {
        ("resolved", Some(TomlValue::Datetime(date))) => Some(*date),
        _ => None,
    };
    for key in SYNC_COMPANIONS {
        finding.rest.remove(*key);
    }
    let mut put = |key: &str, value: &str| {
        if !value.is_empty() {
            finding
                .rest
                .insert(key.to_string(), TomlValue::String(value.to_string()));
        }
    };
    match outcome.status {
        "resolved" => {
            put("resolution", &format!("v1: {}", outcome.summary()));
            let date = outcome.resolved.or(kept_date).unwrap_or(today);
            finding
                .rest
                .insert("resolved".to_string(), TomlValue::Datetime(date));
        }
        "dismissed" => put("dismiss_reason", &format!("v1: {}", outcome.summary())),
        _ => {
            put("defer_reason", &outcome.resolution);
            put("defer_trigger", &outcome.defer_trigger);
        }
    }
    finding.status = outcome.status.to_string();
}

/// Replaces the verdict on `record`'s (finding, by, seat), or appends it; a
/// changed verdict moves to the end, where readers take the latest from.
fn upsert(store: &mut Store, record: Verdict) {
    let key = |v: &Verdict| (v.finding.clone(), v.by.clone(), v.seat);
    match store.verdicts.iter().position(|v| key(v) == key(&record)) {
        Some(at) if store.verdicts[at] == record => {}
        Some(at) => {
            store.verdicts.remove(at);
            store.verdicts.push(record);
        }
        None => store.verdicts.push(record),
    }
}

fn capped(text: &str) -> String {
    text.chars().take(RATIONALE_CAP).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_status_covers_every_v1_disposition() {
        for (v1, v2) in [
            ("fixed", Some("resolved")),
            ("applied", Some("resolved")),
            ("wontfix", Some("dismissed")),
            ("wontapply", Some("dismissed")),
            ("verified-clean", Some("dismissed")),
            ("deferred", Some("deferred")),
            ("open", None),
            ("bogus", None),
        ] {
            assert_eq!(map_status(v1), v2, "v1 status {v1}");
        }
    }
}
