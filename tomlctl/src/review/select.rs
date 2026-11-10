//! Which findings may leave triage for apply: the refusal rule an explicit id
//! meets, and what `--all` takes, holds back or refuses. The caller names the
//! family its destination accepts and the verb its errors carry.

use std::collections::HashMap;
use std::path::Path;

use anyhow::Result;
use globset::GlobSet;
use serde_json::{Value as JsonValue, json};
use toml::Value as TomlValue;

use super::catalogue::run_catalogue;
use super::fields::{self, Family, FindingStatus, Kind, Role};
use super::invalid;
use super::store::{Finding, Row, Store, View};

/// The findings a hand-off considers, with each finding's implementer
/// projection, indexed as `stored.findings`.
pub(super) struct Selection {
    pub(super) chosen: Vec<usize>,
    /// `all`'s exclusions that an explicit id still selects.
    pub(super) held: Vec<JsonValue>,
    pub(super) refused: Vec<JsonValue>,
    pub(super) projected: Vec<Row>,
}

impl Selection {
    /// With `all`, every open defect or cleanup of `family` not blocked;
    /// otherwise `ids` in order, blank and repeated ids skipped.
    pub(super) fn new(
        verb: &str,
        stored: &Store,
        root: &Path,
        family: Family,
        all: bool,
        ids: &[String],
    ) -> Result<Self> {
        let view = View::new(stored);
        let projected = (0..stored.findings.len())
            .map(|index| view.project(Role::Implementer, index, false))
            .collect();
        let mut chosen = Vec::new();
        let mut held = Vec::new();
        let mut refused = Vec::new();
        if all {
            let mut tests = TestGlobs::new(verb, root);
            for (index, f) in stored.findings.iter().enumerate() {
                let selectable = f.status() == FindingStatus::Open
                    && f.family == family
                    && matches!(f.kind, Kind::Defect | Kind::Cleanup)
                    && fields::blocked_on(f).is_none();
                if !selectable {
                    continue;
                }
                if let Some(reason) = refusal(f, family) {
                    refused.push(json!({"finding": f.id, "reason": reason}));
                    continue;
                }
                let reason =
                    if f.kind == Kind::Cleanup && latest_verdict(stored, &f.id) == "plausible" {
                        Some("cleanup verified only plausible")
                    } else if tests.is_test_removal(stored, f)? {
                        Some("test removal")
                    } else {
                        None
                    };
                match reason {
                    Some(reason) => held.push(json!({"finding": f.id, "reason": reason})),
                    None => chosen.push(index),
                }
            }
        } else {
            let mut seen: Vec<&str> = Vec::new();
            for id in ids.iter().map(|id| id.trim()).filter(|id| !id.is_empty()) {
                if seen.contains(&id) {
                    continue;
                }
                seen.push(id);
                match stored.findings.iter().position(|f| f.id == id) {
                    None => refused.push(json!({
                        "finding": id,
                        "reason": "no such finding in this store",
                    })),
                    Some(index) => match refusal(&stored.findings[index], family) {
                        Some(reason) => refused.push(json!({"finding": id, "reason": reason})),
                        None => chosen.push(index),
                    },
                }
            }
        }
        Ok(Self {
            chosen,
            held,
            refused,
            projected,
        })
    }
}

/// Why `f` may not be handed to a destination of `family`, if it may not.
pub(super) fn refusal(f: &Finding, family: Family) -> Option<String> {
    if f.family == Family::Plan {
        return Some("plan-family findings merge into the plan, not a v1 ledger".into());
    }
    if f.kind == Kind::Advisory {
        return Some("advisory is parked or picked for the backlog, never exported".into());
    }
    if f.status() != FindingStatus::Open {
        return Some(format!("status is {}, not open", f.status().as_str()));
    }
    if f.family != family {
        return Some(format!(
            "a {} finding does not belong in a {} ledger",
            f.family.as_str(),
            family.as_str()
        ));
    }
    if let Some(blocked) = fields::blocked_on(f) {
        let legacy = if text(f, "blocked_on").trim().is_empty() {
            " (a stored fix_shape = needs-plan)"
        } else {
            ""
        };
        return Some(format!("blocked on {}{legacy}", blocked.as_str()));
    }
    if text(f, "effort") == "large" {
        return Some("effort is large".into());
    }
    None
}

/// The finding's verdicts' latest `verdict`, or `""` when it has none.
fn latest_verdict(stored: &Store, finding: &str) -> String {
    stored
        .verdicts
        .iter()
        .rev()
        .find(|v| v.finding == finding)
        .map(|v| v.verdict.clone())
        .unwrap_or_default()
}

/// The finding's string field `key` from its open fields, or `""`.
pub(super) fn text<'a>(f: &'a Finding, key: &str) -> &'a str {
    f.rest
        .get(key)
        .and_then(TomlValue::as_str)
        .unwrap_or_default()
}

/// The vocabulary's `[routing]` test globs, per run, loaded only for a
/// cleanup that removes something.
struct TestGlobs<'a> {
    verb: &'a str,
    root: &'a Path,
    by_run: HashMap<String, GlobSet>,
}

impl<'a> TestGlobs<'a> {
    fn new(verb: &'a str, root: &'a Path) -> Self {
        Self {
            verb,
            root,
            by_run: HashMap::new(),
        }
    }

    /// Whether `f` is a cleanup removing something at a test path: its edit
    /// target (`target_file`, else `file`) or any of its `instances`.
    fn is_test_removal(&mut self, stored: &Store, f: &Finding) -> Result<bool> {
        if f.kind != Kind::Cleanup || text(f, "direction") != "remove" {
            return Ok(false);
        }
        if !self.by_run.contains_key(&f.run) {
            let run = stored.runs.iter().find(|r| r.id == f.run).ok_or_else(|| {
                invalid(format!(
                    "{}: finding {} names run {}, which is not in this store",
                    self.verb, f.id, f.run
                ))
            })?;
            // Only the tests list: a bad executable glob must not fail selection.
            let globs = fields::compile("tests", &run_catalogue(self.root, run)?.routing().tests)?;
            self.by_run.insert(f.run.clone(), globs);
        }
        let globs = &self.by_run[&f.run];
        let instances = f
            .rest
            .get("instances")
            .and_then(TomlValue::as_array)
            .into_iter()
            .flatten()
            .filter_map(TomlValue::as_str);
        Ok(
            fields::edited_paths(&f.file, Some(text(f, "target_file")), instances)
                .any(|path| globs.is_match(path)),
        )
    }
}
