//! T10: `tomlctl flow resolve` — the 5-step (in practice 6-step) flow
//! resolution keystone.
//!
//! Read-only. Composes Phase A primitives via internal Rust calls (NOT
//! subprocesses): the `.claude/active-flow.toml` parser is mirrored from
//! `flow::active`, and the staleness arithmetic mirrors `flow::stale`.
//!
//! # Source enum
//!
//! The plan's envelope sketch lists six `source` strings:
//! `explicit-flag | active-binding | active-latest | branch-match |
//! prompt-required | none` (`prompt-required` is reserved — never emitted
//! by the current resolver). The algorithm body, however, has SIX distinct
//! paths plus the terminal "none" outcome — the scope-glob match (step 2)
//! has no dedicated string in the plan's enum.
//!
//! Resolution: this implementation emits `source = "scope-glob"` for the
//! step-2 hit, treating the plan's enum as illustrative-not-exhaustive.
//! The alternative (folding scope-glob into `branch-match`) would conflate
//! two semantically distinct resolution paths: branch-match scans
//! `.claude/flows/*` and filters by `branch`, whereas scope-glob filters
//! by `--path` against each flow's `scope`. Test fixtures pin the
//! `scope-glob` literal so a future regression that folded the strings
//! together would break the suite.
//!
//! Step-6 ("none") emits literal `source = "none"` with `resolved: false`
//! per the plan body. The `prompt-required` enum string is reserved for
//! future use; this implementation does not emit it.
//!
//! # Read integrity
//!
//! Honours `ReadIntegrityArgs` (`--verify-integrity`, `--strict-read`).
//! `--verify-integrity` is applied to the active-flow registry on the
//! step-3/4 paths and to the resolved `context.toml`. `--strict-read`
//! escalates a missing resolved `context.toml` (under `--flow <slug>`) to
//! a tagged `kind=not_found` rather than falling through to step 6.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use globset::Glob;
use serde_json::{Value as JsonValue, json};
use toml::Value as TomlValue;

use crate::cli::ReadIntegrityArgs;
use crate::errors::{ErrorKind, tagged_err};
use crate::flow::schema::{ActiveDoc, ActiveEntry};
use crate::integrity::{IntegrityOpts, maybe_verify_integrity};
use crate::io::{read_toml, repo_or_cwd_root};
use crate::output::print_json_compact;

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
pub(crate) fn dispatch(
    flow: Option<String>,
    path: Vec<PathBuf>,
    branch: Option<String>,
    worktree: Option<PathBuf>,
    with_staleness: bool,
    _json: bool,
    integrity: ReadIntegrityArgs,
) -> Result<()> {
    let root = repo_or_cwd_root()?;
    let opts = read_integrity_opts(&integrity);

    let envelope = resolve(
        &root,
        flow.as_deref(),
        &path,
        branch.as_deref(),
        worktree.as_deref(),
        with_staleness,
        opts,
        integrity.strict_read,
    )?;
    print_json_compact(&envelope)
}

// ---------------------------------------------------------------------------
// Resolution algorithm
// ---------------------------------------------------------------------------

/// Top-level resolver. Returns the JSON envelope ready for stdout.
///
/// Steps (per `docs/plans/flow-tracking-overhaul.md` Task 10):
/// 1. `--flow <slug>`     → `source = "explicit-flag"` if `context.toml` exists.
/// 2. scope-glob match     → `source = "scope-glob"` on a unique hit.
/// 3. active-binding match → `source = "active-binding"` on a unique hit.
/// 4. active-latest        → `source = "active-latest"` (registry non-empty).
/// 5. branch-match         → `source = "branch-match"` (registry empty path).
/// 6. none                 → `source = "none"`, `resolved: false`.
#[allow(clippy::too_many_arguments)]
fn resolve(
    root: &Path,
    flow: Option<&str>,
    paths: &[PathBuf],
    branch: Option<&str>,
    worktree: Option<&Path>,
    with_staleness: bool,
    opts: IntegrityOpts,
    strict_read: bool,
) -> Result<JsonValue> {
    let mut warnings: Vec<String> = Vec::new();

    // Step 1: explicit --flow.
    if let Some(slug) = flow {
        let ctx = context_path_for(root, slug);
        if ctx.exists() {
            return build_resolved_envelope(
                root,
                slug,
                "explicit-flag",
                false,
                Vec::new(),
                with_staleness,
                opts,
                &mut warnings,
            );
        }
        if strict_read {
            return Err(tagged_err(
                ErrorKind::NotFound,
                Some(ctx.clone()),
                format!(
                    "explicit --flow {slug}: context.toml does not exist at {}",
                    ctx.display()
                ),
            ));
        }
        warnings.push(format!(
            "explicit --flow {slug} requested but context.toml not found; falling through"
        ));
    }

    // Enumerate all on-disk flows once for steps 2 and 5.
    let flows = enumerate_flows(root)?;

    // Step 2: scope-glob match.
    if !paths.is_empty() {
        let candidates: Vec<&FlowSummary> = flows
            .iter()
            .filter(|f| !f.is_complete())
            .filter(|f| any_path_matches_scope(paths, &f.scope))
            .collect();
        match candidates.len() {
            0 => { /* fall through */ }
            1 => {
                let slug = candidates[0].slug.clone();
                return build_resolved_envelope(
                    root,
                    &slug,
                    "scope-glob",
                    false,
                    Vec::new(),
                    with_staleness,
                    opts,
                    &mut warnings,
                );
            }
            _ => {
                let tie: Vec<String> = candidates.iter().map(|f| f.slug.clone()).collect();
                // Multiple matches → "tie surfaced via tie_candidates". The
                // plan body says "exactly one match → use it; multiple →
                // tie surfaced; zero → next step." We surface the tie as a
                // resolved=false envelope so the caller can present the
                // candidates to the user — a tie is not a resolution.
                return build_unresolved_envelope_with_ties(
                    "scope-glob",
                    tie,
                    warnings,
                );
            }
        }
    }

    // Step 3 + 4: registry consultation.
    let registry_path = active_flow_path(root);
    let registry_entries = if registry_path.exists() {
        // Honour --verify-integrity on the registry.
        if opts.verify_on_read {
            maybe_verify_integrity(&registry_path, opts)
                .with_context(|| format!("verifying {}", registry_path.display()))?;
        }
        load_active_entries(&registry_path)?
    } else {
        Vec::new()
    };

    if !registry_entries.is_empty() {
        // Step 3: best-binding match. When neither --branch nor --worktree is
        // supplied, step-3 has nothing to score against — emit a breadcrumb
        // warning before falling through to step-4 so callers don't wonder
        // why active-binding never fires.
        if branch.is_none() && worktree.is_none() {
            warnings.push(
                "step-3 binding-match skipped: no --branch or --worktree provided".to_string(),
            );
        }
        if let Some(best) = best_binding_match(&registry_entries, branch, worktree) {
            // Confirm the resolved flow's context.toml actually exists; if
            // not, surface a warning and fall through.
            let ctx = context_path_for(root, &best.slug);
            if ctx.exists() {
                return build_resolved_envelope(
                    root,
                    &best.slug,
                    "active-binding",
                    false,
                    Vec::new(),
                    with_staleness,
                    opts,
                    &mut warnings,
                );
            } else {
                warnings.push(format!(
                    "active-binding match {slug} has no context.toml at {path}; falling through",
                    slug = best.slug,
                    path = ctx.display()
                ));
            }
        }

        // Step 4: active-latest by `last_used`.
        if let Some(latest) = pick_active_latest(&registry_entries) {
            let ctx = context_path_for(root, &latest.slug);
            if ctx.exists() {
                return build_resolved_envelope(
                    root,
                    &latest.slug,
                    "active-latest",
                    false,
                    Vec::new(),
                    with_staleness,
                    opts,
                    &mut warnings,
                );
            } else {
                warnings.push(format!(
                    "active-latest candidate {slug} has no context.toml at {path}; falling through",
                    slug = latest.slug,
                    path = ctx.display()
                ));
            }
        }
    }

    // Step 5: branch-match (registry empty OR no usable active entry).
    if let Some(want_branch) = branch {
        let mut branch_hits: Vec<&FlowSummary> = flows
            .iter()
            .filter(|f| !f.is_complete())
            .filter(|f| f.branch.as_deref() == Some(want_branch))
            .collect();
        if !branch_hits.is_empty() {
            // Sort by `updated` descending — newest first.
            branch_hits.sort_by(|a, b| b.updated.cmp(&a.updated));
            // Tie detection: multiple flows share the same most-recent `updated`.
            let top_updated = branch_hits[0].updated.clone();
            let tied: Vec<&&FlowSummary> = branch_hits
                .iter()
                .filter(|f| f.updated == top_updated)
                .collect();
            let chosen_slug = branch_hits[0].slug.clone();
            let tie_slugs: Vec<String> = if tied.len() > 1 {
                tied.iter().map(|f| f.slug.clone()).collect()
            } else {
                Vec::new()
            };
            let ties_broken = tie_slugs.len() > 1;
            if ties_broken {
                eprintln!(
                    "warning: {n} flows tied on branch+updated — picked {chosen}; pass --flow to disambiguate",
                    n = tie_slugs.len(),
                    chosen = chosen_slug,
                );
            }
            return build_resolved_envelope(
                root,
                &chosen_slug,
                "branch-match",
                ties_broken,
                tie_slugs,
                with_staleness,
                opts,
                &mut warnings,
            );
        }
    }

    // Step 6: none.
    warnings.push("no flow resolves; user prompt required".to_string());
    Ok(json!({
        "resolved": false,
        "source": "none",
        "ties_broken": false,
        "tie_candidates": [],
        "warnings": warnings,
    }))
}

// ---------------------------------------------------------------------------
// Envelope construction
// ---------------------------------------------------------------------------

/// Render the full resolved envelope. Reads the resolved flow's
/// `context.toml` to populate the projection (status, branch, scope,
/// plan_path, artifacts). Surfaces missing artifacts in `warnings`.
#[allow(clippy::too_many_arguments)]
fn build_resolved_envelope(
    root: &Path,
    slug: &str,
    source: &str,
    ties_broken: bool,
    tie_candidates: Vec<String>,
    with_staleness: bool,
    opts: IntegrityOpts,
    warnings: &mut Vec<String>,
) -> Result<JsonValue> {
    let context_path = context_path_for(root, slug);

    // Read the resolved context.toml. Honour --verify-integrity here so
    // the caller catches a tampered context before downstream consumption.
    if opts.verify_on_read && context_path.exists() {
        maybe_verify_integrity(&context_path, opts)
            .with_context(|| format!("verifying {}", context_path.display()))?;
    }
    let doc = read_toml(&context_path)?;
    let table = doc.as_table().ok_or_else(|| {
        tagged_err(
            ErrorKind::Parse,
            Some(context_path.clone()),
            format!(
                "context.toml at {} root is not a table",
                context_path.display()
            ),
        )
    })?;

    // Project the surface fields.
    let plan_path_v = table
        .get("plan_path")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let branch_v = table
        .get("branch")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let status_v = table
        .get("status")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let scope_v: Vec<String> = table
        .get("scope")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();

    // Artifacts: prefer the explicit [artifacts] block; fall back to the
    // canonical computation when absent.
    let artifacts = read_or_compute_artifacts(table, slug);

    // Warn on artifact files that are referenced but absent from disk.
    for (key, rel) in &[
        ("review_ledger", &artifacts.review_ledger),
        ("optimise_findings", &artifacts.optimise_findings),
        ("execution_record", &artifacts.execution_record),
        ("plan_review_findings", &artifacts.plan_review_findings),
    ] {
        let abs = root.join(rel);
        if !abs.exists() {
            warnings.push(format!("artifact missing: {key} at {rel}"));
        }
    }

    // Optional staleness annotation.
    let stale_v = if with_staleness {
        compute_staleness(table)
    } else {
        JsonValue::Null
    };

    // Render context_path relative to root (forward slashes).
    let context_path_rel = relativise(root, &context_path);
    let tie_candidates_json: Vec<JsonValue> = tie_candidates
        .into_iter()
        .map(JsonValue::String)
        .collect();

    let mut obj = serde_json::Map::new();
    obj.insert("resolved".to_string(), JsonValue::Bool(true));
    obj.insert("slug".to_string(), JsonValue::String(slug.to_string()));
    obj.insert("source".to_string(), JsonValue::String(source.to_string()));
    obj.insert("ties_broken".to_string(), JsonValue::Bool(ties_broken));
    obj.insert(
        "tie_candidates".to_string(),
        JsonValue::Array(tie_candidates_json),
    );
    obj.insert(
        "context_path".to_string(),
        JsonValue::String(context_path_rel),
    );
    obj.insert("artifacts".to_string(), artifacts.to_json());
    obj.insert(
        "plan_path".to_string(),
        plan_path_v
            .map(JsonValue::String)
            .unwrap_or(JsonValue::Null),
    );
    let scope_json: Vec<JsonValue> = scope_v.into_iter().map(JsonValue::String).collect();
    obj.insert("scope".to_string(), JsonValue::Array(scope_json));
    obj.insert(
        "branch".to_string(),
        branch_v.map(JsonValue::String).unwrap_or(JsonValue::Null),
    );
    obj.insert(
        "status".to_string(),
        status_v.map(JsonValue::String).unwrap_or(JsonValue::Null),
    );
    obj.insert("stale".to_string(), stale_v);
    let warnings_json: Vec<JsonValue> =
        warnings.iter().cloned().map(JsonValue::String).collect();
    obj.insert("warnings".to_string(), JsonValue::Array(warnings_json));

    Ok(JsonValue::Object(obj))
}

/// Tie-detection envelope for the step-2 multi-match case. The plan body
/// says "multiple → tie surfaced": we surface as a `resolved=false`
/// envelope carrying the tied slugs in `tie_candidates`.
fn build_unresolved_envelope_with_ties(
    source: &str,
    tie_candidates: Vec<String>,
    mut warnings: Vec<String>,
) -> Result<JsonValue> {
    warnings.push(format!(
        "{source} match has multiple candidates; tie_candidates surfaced"
    ));
    let tie_json: Vec<JsonValue> = tie_candidates.into_iter().map(JsonValue::String).collect();
    Ok(json!({
        "resolved": false,
        "source": source,
        "ties_broken": true,
        "tie_candidates": tie_json,
        "warnings": warnings,
    }))
}

// ---------------------------------------------------------------------------
// Artifacts projection
// ---------------------------------------------------------------------------

/// Four canonical artifact paths for a flow.
struct Artifacts {
    review_ledger: String,
    optimise_findings: String,
    execution_record: String,
    plan_review_findings: String,
}

impl Artifacts {
    fn to_json(&self) -> JsonValue {
        json!({
            "review_ledger": self.review_ledger,
            "optimise_findings": self.optimise_findings,
            "execution_record": self.execution_record,
            "plan_review_findings": self.plan_review_findings,
        })
    }
}

/// Canonical-path computation: `.claude/flows/<slug>/<key-with-dashes>.toml`.
/// Mirrors the logic in `flow::init::artifacts_for`.
fn canonical_artifacts(slug: &str) -> Artifacts {
    Artifacts {
        review_ledger: format!(".claude/flows/{slug}/review-ledger.toml"),
        optimise_findings: format!(".claude/flows/{slug}/optimise-findings.toml"),
        execution_record: format!(".claude/flows/{slug}/execution-record.toml"),
        plan_review_findings: format!(".claude/flows/{slug}/plan-review-findings.toml"),
    }
}

/// Prefer explicit `[artifacts]` when present and well-shaped; fall back to
/// the canonical computation otherwise. A partial `[artifacts]` block (some
/// keys present, some absent) gets the missing keys filled from the
/// canonical computation — that's the most defensible behaviour for a
/// hand-edited file that left out a key.
fn read_or_compute_artifacts(table: &toml::map::Map<String, TomlValue>, slug: &str) -> Artifacts {
    let canonical = canonical_artifacts(slug);
    let Some(arts_tbl) = table.get("artifacts").and_then(|v| v.as_table()) else {
        return canonical;
    };
    let pluck = |key: &str, fallback: &str| -> String {
        arts_tbl
            .get(key)
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .unwrap_or_else(|| fallback.to_string())
    };
    Artifacts {
        review_ledger: pluck("review_ledger", &canonical.review_ledger),
        optimise_findings: pluck("optimise_findings", &canonical.optimise_findings),
        execution_record: pluck("execution_record", &canonical.execution_record),
        plan_review_findings: pluck(
            "plan_review_findings",
            &canonical.plan_review_findings,
        ),
    }
}

// ---------------------------------------------------------------------------
// Active-flow registry parsing (delegates to flow::schema for the canonical
// typed projection; R17 consolidated the three per-site walks).
// ---------------------------------------------------------------------------

fn active_flow_path(root: &Path) -> PathBuf {
    root.join(".claude").join("active-flow.toml")
}

fn load_active_entries(file: &Path) -> Result<Vec<ActiveEntry>> {
    // Align with `flow::doctor`'s silent-zero behaviour on a malformed
    // registry: surface zero entries (with a stderr breadcrumb) so resolve
    // falls through to step-5 instead of hard-erroring on a corrupt file.
    let doc = match read_toml(file) {
        Ok(d) => d,
        Err(e) => {
            eprintln!(
                "warning: active-flow.toml unreadable — falling through to step-5; {e}"
            );
            return Ok(Vec::new());
        }
    };
    Ok(ActiveDoc::from_toml_value(&doc).active)
}

/// Pick the registry entry whose binding best matches the caller's
/// `--branch` / `--worktree` filters. Returns `None` when no entry matches
/// or when there's a tie at the top score (so the caller can fall through
/// to step 4). Scoring: +1 for each filter that's both supplied AND matches
/// the entry's binding. An entry with no binding scores 0. The caller's
/// `--scope` is not threaded here (resolve has no `--scope` arg of its own;
/// scope-glob is step 2's surface).
fn best_binding_match(
    entries: &[ActiveEntry],
    want_branch: Option<&str>,
    want_worktree: Option<&Path>,
) -> Option<ActiveEntry> {
    if want_branch.is_none() && want_worktree.is_none() {
        return None;
    }
    let mut scored: Vec<(u32, &ActiveEntry)> = Vec::with_capacity(entries.len());
    for entry in entries {
        let mut score: u32 = 0;
        if let Some(want) = want_branch
            && entry.binding.branch.as_deref() == Some(want)
        {
            score += 1;
        }
        if let Some(want) = want_worktree
            && let Some(wt) = entry.binding.worktree_path()
            && wt == want
        {
            score += 1;
        }
        if score > 0 {
            scored.push((score, entry));
        }
    }
    if scored.is_empty() {
        return None;
    }
    let max = scored.iter().map(|(s, _)| *s).max().unwrap_or(0);
    let top: Vec<&ActiveEntry> = scored
        .iter()
        .filter(|(s, _)| *s == max)
        .map(|(_, e)| *e)
        .collect();
    if top.len() == 1 {
        Some(top[0].clone())
    } else {
        // Tie: per plan "Multiple ties → fall through". Caller picks
        // step-4 active-latest instead.
        None
    }
}

/// Step-4 fallback: most-recent `last_used` wins. Empty `last_used`
/// strings sort to the bottom (treated as ancient).
fn pick_active_latest(entries: &[ActiveEntry]) -> Option<ActiveEntry> {
    entries
        .iter()
        .max_by(|a, b| a.last_used.cmp(&b.last_used))
        .cloned()
}

// ---------------------------------------------------------------------------
// On-disk flow enumeration (mirrors flow::list)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct FlowSummary {
    slug: String,
    status: Option<String>,
    branch: Option<String>,
    scope: Vec<String>,
    /// `updated` rendered as the on-disk display form (`YYYY-MM-DD` for a
    /// TOML date, the raw datetime string otherwise). Lexicographic
    /// comparison is used for "latest" picks, which is correct under
    /// ISO-8601 dates.
    updated: String,
}

impl FlowSummary {
    fn is_complete(&self) -> bool {
        self.status.as_deref() == Some("complete")
    }
}

fn context_path_for(root: &Path, slug: &str) -> PathBuf {
    root.join(".claude")
        .join("flows")
        .join(slug)
        .join("context.toml")
}

fn enumerate_flows(root: &Path) -> Result<Vec<FlowSummary>> {
    let flows_dir = root.join(".claude").join("flows");
    if !flows_dir.exists() {
        return Ok(Vec::new());
    }
    let mut entries: Vec<std::fs::DirEntry> = std::fs::read_dir(&flows_dir)
        .with_context(|| format!("reading {}", flows_dir.display()))?
        .filter_map(|e| e.ok())
        .collect();
    entries.sort_by_key(|e| e.file_name());
    let mut out: Vec<FlowSummary> = Vec::with_capacity(entries.len());
    for entry in entries {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(slug) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let ctx = path.join("context.toml");
        if !ctx.exists() {
            continue;
        }
        let raw = match std::fs::read_to_string(&ctx) {
            Ok(s) => s,
            // A racing delete or unreadable flow is best-effort skipped here
            // — this enumeration drives candidate filtering, not strict
            // verification (`flow doctor` is the proper invariant check).
            Err(_) => continue,
        };
        let doc: TomlValue = match toml::from_str(&raw) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let Some(table) = doc.as_table() else { continue };
        let status = table
            .get("status")
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
        let updated = table
            .get("updated")
            .map(|v| match v {
                TomlValue::Datetime(dt) => dt.to_string(),
                TomlValue::String(s) => s.clone(),
                other => other.to_string(),
            })
            .unwrap_or_default();
        out.push(FlowSummary {
            slug: slug.to_string(),
            status,
            branch,
            scope,
            updated,
        });
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Glob matching (step 2)
// ---------------------------------------------------------------------------

/// True when ANY of the caller's `--path` args matches ANY of `scope`'s
/// globs. We compile each scope glob ad-hoc (the `scope` array is small —
/// typically 1-3 patterns per flow). Compile failures on a malformed
/// pattern are treated as non-matches (defensive — a hand-edited bad glob
/// should not crash the resolver).
fn any_path_matches_scope(paths: &[PathBuf], scope: &[String]) -> bool {
    if scope.is_empty() {
        return false;
    }
    // Pre-compile the scope globs once, then test each path against each.
    let mut matchers: Vec<globset::GlobMatcher> = Vec::with_capacity(scope.len());
    for pat in scope {
        if let Ok(g) = Glob::new(pat) {
            matchers.push(g.compile_matcher());
        }
    }
    if matchers.is_empty() {
        return false;
    }
    for p in paths {
        for m in &matchers {
            if m.is_match(p) {
                return true;
            }
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Staleness annotation (mirrors flow::stale)
// ---------------------------------------------------------------------------

/// Compute `{stale, age_seconds, reason}` for the resolved flow, given its
/// context.toml table. Mirrors the simple-date arithmetic in
/// `flow::stale::verdict_from_iso_string`. The threshold is hardcoded to
/// 7 days — `flow resolve --with-staleness` doesn't expose a custom
/// threshold (the caller can call `flow stale --threshold` directly for
/// that, per Phase A leaf separation). Returns JSON `null` semantics in
/// JSON form when `updated` is missing/unparseable.
fn compute_staleness(table: &toml::map::Map<String, TomlValue>) -> JsonValue {
    let updated = table.get("updated");
    let iso: String = match updated {
        Some(TomlValue::Datetime(dt)) => dt.to_string(),
        Some(TomlValue::String(s)) => s.clone(),
        _ => {
            return json!({
                "stale": true,
                "age_seconds": JsonValue::Null,
                "reason": "updated field missing",
            });
        }
    };

    use jiff::Timestamp;
    use jiff::civil::Date;

    let updated_date: Date = if iso.len() == 10 {
        match iso.parse::<Date>() {
            Ok(d) => d,
            Err(_) => {
                return json!({
                    "stale": true,
                    "age_seconds": JsonValue::Null,
                    "reason": "updated field unparseable",
                });
            }
        }
    } else {
        let z = match iso.parse::<Timestamp>().and_then(|t| t.in_tz("UTC")) {
            Ok(z) => z,
            Err(_) => {
                return json!({
                    "stale": true,
                    "age_seconds": JsonValue::Null,
                    "reason": "updated field unparseable",
                });
            }
        };
        z.date()
    };

    let today: Date = match Timestamp::now().in_tz("UTC") {
        Ok(z) => z.date(),
        Err(_) => {
            return json!({
                "stale": true,
                "age_seconds": JsonValue::Null,
                "reason": "could not resolve today's UTC date",
            });
        }
    };

    let span = match updated_date.until(today) {
        Ok(s) => s,
        Err(_) => {
            return json!({
                "stale": true,
                "age_seconds": JsonValue::Null,
                "reason": "could not compute span",
            });
        }
    };
    let signed_days: i64 = span.get_days() as i64;
    let age_days: u64 = if signed_days > 0 { signed_days as u64 } else { 0 };
    let age_seconds: u64 = age_days.saturating_mul(86_400);
    let threshold = Duration::from_secs(7 * 86_400);
    let stale = Duration::from_secs(age_seconds) > threshold;
    let reason = if stale {
        "updated > 7d ago".to_string()
    } else {
        "updated within threshold".to_string()
    };
    json!({
        "stale": stale,
        "age_seconds": age_seconds,
        "reason": reason,
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn read_integrity_opts(args: &ReadIntegrityArgs) -> IntegrityOpts {
    IntegrityOpts {
        write_sidecar: true,
        verify_on_read: args.verify_integrity,
        strict: false,
    }
}

fn relativise(root: &Path, path: &Path) -> String {
    let rel = path.strip_prefix(root).unwrap_or(path);
    rel.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_artifacts_yields_four_canonical_paths() {
        let a = canonical_artifacts("feature-x");
        assert_eq!(
            a.review_ledger,
            ".claude/flows/feature-x/review-ledger.toml"
        );
        assert_eq!(
            a.optimise_findings,
            ".claude/flows/feature-x/optimise-findings.toml"
        );
        assert_eq!(
            a.execution_record,
            ".claude/flows/feature-x/execution-record.toml"
        );
        assert_eq!(
            a.plan_review_findings,
            ".claude/flows/feature-x/plan-review-findings.toml"
        );
    }

    #[test]
    fn glob_matcher_smoke() {
        let scope = vec!["src/foo/**".to_string()];
        assert!(any_path_matches_scope(
            &[PathBuf::from("src/foo/bar.rs")],
            &scope
        ));
        assert!(!any_path_matches_scope(
            &[PathBuf::from("src/baz/bar.rs")],
            &scope
        ));
    }

    #[test]
    fn glob_matcher_handles_malformed_patterns_as_non_match() {
        // A truly-malformed glob (unbalanced bracket) shouldn't panic and
        // shouldn't accidentally match.
        let scope = vec!["[unbalanced".to_string()];
        assert!(!any_path_matches_scope(
            &[PathBuf::from("src/foo/bar.rs")],
            &scope
        ));
    }

    use crate::flow::schema::Binding;

    #[test]
    fn best_binding_match_returns_unique_top_score() {
        let entries = vec![
            ActiveEntry {
                slug: "a".to_string(),
                last_used: "2026-05-08T00:00:00Z".to_string(),
                binding: Binding {
                    branch: Some("feat/x".to_string()),
                    ..Default::default()
                },
            },
            ActiveEntry {
                slug: "b".to_string(),
                last_used: "2026-05-09T00:00:00Z".to_string(),
                binding: Binding {
                    branch: Some("feat/y".to_string()),
                    ..Default::default()
                },
            },
        ];
        let best = best_binding_match(&entries, Some("feat/y"), None);
        assert!(best.is_some());
        assert_eq!(best.unwrap().slug, "b");
    }

    #[test]
    fn best_binding_match_falls_through_on_tie() {
        let entries = vec![
            ActiveEntry {
                slug: "a".to_string(),
                last_used: "2026-05-08T00:00:00Z".to_string(),
                binding: Binding {
                    branch: Some("feat/x".to_string()),
                    ..Default::default()
                },
            },
            ActiveEntry {
                slug: "b".to_string(),
                last_used: "2026-05-09T00:00:00Z".to_string(),
                binding: Binding {
                    branch: Some("feat/x".to_string()),
                    ..Default::default()
                },
            },
        ];
        let best = best_binding_match(&entries, Some("feat/x"), None);
        assert!(best.is_none(), "tie at top score must fall through");
    }

    #[test]
    fn pick_active_latest_picks_highest_last_used() {
        let entries = vec![
            ActiveEntry {
                slug: "a".to_string(),
                last_used: "2026-05-08T00:00:00Z".to_string(),
                binding: Binding::default(),
            },
            ActiveEntry {
                slug: "b".to_string(),
                last_used: "2026-05-09T00:00:00Z".to_string(),
                binding: Binding::default(),
            },
        ];
        let latest = pick_active_latest(&entries);
        assert_eq!(latest.unwrap().slug, "b");
    }

}
