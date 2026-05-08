//! T9: `tomlctl flow list [--status <s>] [--branch <b>] [--active-only] [--json]`.
//!
//! Read-only enumeration of every `<root>/.claude/flows/<slug>/context.toml`
//! record. Output is a JSON array of records:
//!
//! ```json
//! [
//!   {"slug": "feature-x", "status": "in-progress", "updated": "2026-05-08",
//!    "plan_path": "docs/plans/feature-x.md", "branch": "feat/x",
//!    "scope": ["src/foo/**"]}
//! ]
//! ```
//!
//! Field contract:
//! - `slug`: directory name (always present).
//! - `status` / `updated` / `plan_path`: pulled from `context.toml`. Empty
//!   string when the field is absent (defensive — preserves shape stability
//!   across malformed inputs).
//! - `branch`: omitted entirely when the source key is absent.
//! - `scope`: always emitted; defaults to `[]` when absent (the field is
//!   structural to flow resolution and downstream readers expect array shape).
//!
//! Filter semantics:
//! - `--status <s>` / `--branch <s>` are exact-string predicates against the
//!   matching field.
//! - `--active-only` cross-references with `.claude/active-flow.toml`'s
//!   `[[active]].slug` set; a missing registry yields `[]` (matches T3's
//!   "missing-registry-is-empty" semantics — no warning emitted because the
//!   read-only `flow active list` already surfaces the legacy-pointer
//!   warning at its own call site).
//!
//! Error tolerance: a malformed `context.toml` in one flow does NOT abort
//! the whole list; we emit a stderr warning of the form
//! `tomlctl: flow <slug>: malformed context.toml — skipped` and continue.
//! Under `--strict-read` a parse error escalates to a tagged `kind=parse`
//! error per the plan's strict-mode contract.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde_json::Value as JsonValue;
use toml::Value as TomlValue;

use crate::cli::ReadIntegrityArgs;
use crate::errors::{ErrorKind, tagged_err};
use crate::flow::schema::ActiveDoc;
use crate::io::repo_or_cwd_root;
use crate::output::print_json;

pub(crate) fn dispatch(
    status: Option<String>,
    branch: Option<String>,
    active_only: bool,
    _json: bool,
    integrity: ReadIntegrityArgs,
) -> Result<()> {
    let root = repo_or_cwd_root()?;
    let flows_dir = root.join(".claude").join("flows");

    // Optional active-only cross-reference. Compute the active-slug set ONCE
    // up front (cheaper than a per-flow lookup) and pass it down as
    // `Option<HashSet<String>>` — `None` means "no filter".
    let active_filter = if active_only {
        Some(load_active_slug_set(&root)?)
    } else {
        None
    };

    let records = enumerate_flows(&flows_dir, integrity.strict_read)?;

    let mut out: Vec<JsonValue> = Vec::with_capacity(records.len());
    for rec in records {
        if let Some(want) = status.as_deref()
            && rec.status.as_deref() != Some(want)
        {
            continue;
        }
        if let Some(want) = branch.as_deref()
            && rec.branch.as_deref() != Some(want)
        {
            continue;
        }
        if let Some(set) = active_filter.as_ref()
            && !set.contains(&rec.slug)
        {
            continue;
        }
        out.push(rec.to_json());
    }

    print_json(&JsonValue::Array(out))
}

/// One flow's listed projection — not a full `context.toml` deserialisation,
/// just the fields the `flow list` contract surfaces. Kept as a struct
/// (rather than building the JSON inline during the walk) so the filter
/// arms above operate on typed fields rather than `JsonValue::get` chains.
struct FlowRecord {
    slug: String,
    status: Option<String>,
    updated: Option<String>,
    plan_path: Option<String>,
    branch: Option<String>,
    scope: Vec<String>,
}

impl FlowRecord {
    fn to_json(&self) -> JsonValue {
        let mut obj = serde_json::Map::new();
        obj.insert("slug".to_string(), JsonValue::String(self.slug.clone()));
        obj.insert(
            "status".to_string(),
            JsonValue::String(self.status.clone().unwrap_or_default()),
        );
        obj.insert(
            "updated".to_string(),
            JsonValue::String(self.updated.clone().unwrap_or_default()),
        );
        obj.insert(
            "plan_path".to_string(),
            JsonValue::String(self.plan_path.clone().unwrap_or_default()),
        );
        // `branch` is omitted entirely when absent — matches the documented
        // record shape (`branch?` in the plan's output schema). Empty string
        // would be ambiguous with "intentionally empty branch", so we skip
        // the key altogether.
        if let Some(b) = self.branch.as_deref() {
            obj.insert("branch".to_string(), JsonValue::String(b.to_string()));
        }
        let scope_arr: Vec<JsonValue> = self
            .scope
            .iter()
            .cloned()
            .map(JsonValue::String)
            .collect();
        obj.insert("scope".to_string(), JsonValue::Array(scope_arr));
        JsonValue::Object(obj)
    }
}

/// Walk `<root>/.claude/flows/*/context.toml` (one level deep) and emit one
/// `FlowRecord` per readable context.toml. A missing flows dir yields the
/// empty list (a fresh clone with no flows yet is not an error). Per-flow
/// parse failures emit a stderr warning and skip the flow, unless
/// `strict_read` is set in which case the failure escalates to a tagged
/// `kind=parse` error.
fn enumerate_flows(flows_dir: &Path, strict_read: bool) -> Result<Vec<FlowRecord>> {
    if !flows_dir.exists() {
        return Ok(Vec::new());
    }
    let mut records: Vec<FlowRecord> = Vec::new();
    let entries = read_dir_sorted(flows_dir)?;
    for entry in entries {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(slug) = path.file_name().and_then(|n| n.to_str()).map(str::to_string) else {
            continue;
        };
        let ctx_path = path.join("context.toml");
        if !ctx_path.exists() {
            // A flow directory without a context.toml is structurally
            // incomplete (no record to surface) but not an error — `flow
            // doctor` is the proper check for that invariant. Skip silently.
            continue;
        }
        match read_context_record(&ctx_path, &slug) {
            Ok(rec) => records.push(rec),
            Err(e) => {
                if strict_read {
                    return Err(tagged_err(
                        ErrorKind::Parse,
                        Some(ctx_path.clone()),
                        format!("parsing {}: {}", ctx_path.display(), e),
                    ));
                }
                eprintln!(
                    "tomlctl: flow {}: malformed context.toml — skipped",
                    slug
                );
            }
        }
    }
    Ok(records)
}

/// Read and project a single flow's `context.toml`. Returns the typed
/// record; on parse failure returns `Err` so the caller (`enumerate_flows`)
/// can decide between warn-and-skip and `--strict-read` escalation.
///
/// Deliberately does NOT funnel through `crate::io::read_toml`: that path
/// layers tagged-error envelopes that downstream callers would consume
/// verbatim, but the per-flow tolerance contract here calls for a plain
/// `Result<_>` whose inner error we project into either a stderr warning
/// or a `kind=parse` re-tag at the `enumerate_flows` boundary.
fn read_context_record(path: &Path, slug: &str) -> Result<FlowRecord> {
    let s = fs::read_to_string(path)
        .with_context(|| format!("reading {}", path.display()))?;
    let doc: TomlValue = toml::from_str(&s)
        .with_context(|| format!("parsing {}", path.display()))?;
    let table = doc
        .as_table()
        .ok_or_else(|| anyhow::anyhow!("context.toml root is not a table"))?;

    let status = table
        .get("status")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    // `updated` is normally a TOML date literal (parsed as Datetime). Render
    // its `Display` form (YYYY-MM-DD) so JSON output stays a plain string;
    // accept a TOML string too for forward-compat with hand-edited files.
    let updated = table.get("updated").map(|v| match v {
        TomlValue::Datetime(dt) => dt.to_string(),
        TomlValue::String(s) => s.clone(),
        other => other.to_string(),
    });
    let plan_path = table
        .get("plan_path")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let branch = table
        .get("branch")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let scope: Vec<String> = table
        .get("scope")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();

    Ok(FlowRecord {
        slug: slug.to_string(),
        status,
        updated,
        plan_path,
        branch,
        scope,
    })
}

/// Load the set of slugs from `<root>/.claude/active-flow.toml` for the
/// `--active-only` cross-reference. Missing file → empty set (matches T3's
/// "missing registry behaves like empty registry" contract). A malformed
/// registry is tolerated as empty — `flow active list` is the proper
/// surface for the legacy/parse-warning paths, and silently treating
/// breakage as "no active flows" here keeps `flow list --active-only` from
/// becoming a second warning channel for the same condition.
fn load_active_slug_set(root: &Path) -> Result<std::collections::HashSet<String>> {
    let registry = root.join(".claude").join("active-flow.toml");
    let mut set = std::collections::HashSet::new();
    if !registry.exists() {
        return Ok(set);
    }
    let s = match fs::read_to_string(&registry) {
        Ok(s) => s,
        // Race: the file existed at the `.exists()` check but disappeared
        // before we could read it. Treat as empty — matching the missing-file
        // branch above — so the cross-reference degrades gracefully rather
        // than aborting `flow list --active-only`.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(set),
        Err(e) => {
            return Err(anyhow::Error::new(e))
                .with_context(|| format!("reading {}", registry.display()));
        }
    };
    // R17: route through the canonical schema parser so list.rs and
    // resolve.rs share one source of truth for the registry's wire shape.
    // Malformed TOML degrades to "no active flows" — same defensive
    // contract the previous TomlValue walk maintained.
    let doc = match ActiveDoc::from_toml_str(&s) {
        Ok(d) => d,
        Err(_) => return Ok(set),
    };
    for entry in doc.active {
        set.insert(entry.slug);
    }
    Ok(set)
}

/// Sorted directory listing — keeps test output deterministic across
/// platforms (POSIX and NTFS make no guarantee about `read_dir` order).
/// Mirrors `find_plans::read_dir_sorted` rather than reusing it directly:
/// keeping each flow leaf module self-contained avoids cross-leaf coupling
/// during the parallel B-phase rollout.
fn read_dir_sorted(dir: &Path) -> Result<Vec<fs::DirEntry>> {
    let mut entries: Vec<fs::DirEntry> = fs::read_dir(dir)
        .with_context(|| format!("reading directory {}", dir.display()))?
        .filter_map(|e| e.ok())
        .collect();
    entries.sort_by_key(|e| e.file_name());
    Ok(entries)
}

