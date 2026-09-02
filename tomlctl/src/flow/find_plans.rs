//! `tomlctl flow find-plans` — discover plan markdown files under
//! configured directories and cross-reference each plan with its flow's
//! `context.toml` (when present).
//!
//! Resolution order for the plan directories:
//!
//! 1. `--dirs` argument (CLI; possibly multiple).
//! 2. `tomlctl.plansDirectories` array in `.claude/settings.json` (the
//!    tomlctl-namespaced multi-path opt-in — takes precedence when present).
//! 3. `plansDirectory` string in `.claude/settings.json` (the upstream
//!    Claude Code setting; schema is string-only).
//! 4. Default `["docs/plans/"]`.
//!
//! At either settings key the literal string `"__DONT_ASK__"` is the sentinel
//! for "explicitly unset"; it is treated as if the key were absent and
//! resolution falls through to the next step.
//!
//! Output is a JSON array (always, regardless of `--json` flag — read-side
//! tomlctl idiom is JSON-by-default) of records:
//!
//! ```json
//! [
//!   {"path":"docs/plans/feature-x.md","slug":"feature-x","has_flow":true,
//!    "status":"in-progress","updated":"2026-05-08","branch":"main"},
//!   {"path":"docs/plans/feature-y.md","slug":"feature-y","has_flow":false}
//! ]
//! ```
//!
//! `--strict-read` errors with `kind=not_found` when a configured
//! `plansDirectory` (any source — `--dirs`, `tomlctl.plansDirectories`, or
//! `plansDirectory`) does not exist on disk. When using the implicit default
//! and the dir is missing, we return an empty array rather than erroring,
//! since a fresh clone with no plans yet is normal.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::Value as JsonValue;

use crate::cli::ReadIntegrityArgs;
use crate::errors::{ErrorKind, tagged_err};
use crate::io::{read_dir_sorted, relativise, repo_or_cwd_root};
use crate::output::print_json;

/// A settings value of this literal string is treated as "explicitly unset"
/// and falls through to the next resolution step.
const SENTINEL_UNSET: &str = "__DONT_ASK__";

pub(crate) fn dispatch(dirs: Vec<PathBuf>, integrity: ReadIntegrityArgs) -> Result<()> {
    let root = repo_or_cwd_root()?;

    // Step 1–4: resolve plan directories. We track whether the source was
    // "configured" (CLI / settings.json) vs "default" so `--strict-read`
    // can distinguish the two cases.
    let (plan_dirs, source_is_configured) = resolve_plan_dirs(&dirs, &root)?;

    let mut records: Vec<JsonValue> = Vec::new();
    let mut seen_slugs: std::collections::HashSet<String> = std::collections::HashSet::new();
    for dir in &plan_dirs {
        let abs_dir = if dir.is_absolute() {
            dir.clone()
        } else {
            root.join(dir)
        };
        if !abs_dir.exists() {
            if integrity.strict_read && source_is_configured {
                return Err(tagged_err(
                    ErrorKind::NotFound,
                    Some(abs_dir.clone()),
                    format!(
                        "configured plan directory does not exist: {}",
                        abs_dir.display()
                    ),
                ));
            }
            continue;
        }
        walk_plans(&abs_dir, &root, &mut records, &mut seen_slugs)?;
    }

    let arr = JsonValue::Array(records);
    print_json(&arr)
}

/// Resolution-order driver. Returns the resolved list of plan directories
/// (relative to `root` unless absolute) plus a flag indicating whether the
/// source was an explicit configuration (CLI or settings.json) — needed so
/// `--strict-read` can distinguish "configured-but-missing" from
/// "default-fallback-missing".
fn resolve_plan_dirs(cli_dirs: &[PathBuf], root: &Path) -> Result<(Vec<PathBuf>, bool)> {
    // Step 1: --dirs.
    if !cli_dirs.is_empty() {
        return Ok((cli_dirs.to_vec(), true));
    }

    // Steps 2 + 3: read .claude/settings.json (if present). Missing is fine,
    // we fall through to default. Parse errors propagate (a malformed
    // settings.json should be reported clearly, not silently masked).
    let settings_path = root.join(".claude").join("settings.json");
    let settings_doc = read_settings_json(&settings_path)?;

    if let Some(doc) = settings_doc.as_ref() {
        // Step 2: tomlctl.plansDirectories (array of strings; sentinel-aware).
        if let Some(dirs) = read_tomlctl_plans_directories(doc) {
            return Ok((dirs, true));
        }
        // Step 3: plansDirectory (string; sentinel-aware).
        if let Some(dirs) = read_plans_directory(doc) {
            return Ok((dirs, true));
        }
    }

    // Step 4: default.
    Ok((vec![PathBuf::from("docs/plans/")], false))
}

/// Read and parse `.claude/settings.json`. Returns `Ok(None)` when the
/// file does not exist (fresh clone — fall through to default).
fn read_settings_json(path: &Path) -> Result<Option<JsonValue>> {
    match fs::read_to_string(path) {
        Ok(s) => {
            let v: JsonValue = serde_json::from_str(&s).map_err(|e| {
                tagged_err(
                    ErrorKind::Parse,
                    Some(path.to_path_buf()),
                    format!("parsing {}: {}", path.display(), e),
                )
            })?;
            Ok(Some(v))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(anyhow::Error::new(e)).with_context(|| format!("reading {}", path.display())),
    }
}

/// Read `tomlctl.plansDirectories` from a parsed settings doc. Returns
/// `Some(dirs)` when the key is present, is an array of strings, and is
/// not entirely sentinel-only. Returns `None` when the key is absent OR
/// when every entry is the `__DONT_ASK__` sentinel.
fn read_tomlctl_plans_directories(doc: &JsonValue) -> Option<Vec<PathBuf>> {
    let arr = doc
        .get("tomlctl")
        .and_then(|t| t.get("plansDirectories"))
        .and_then(|v| v.as_array())?;
    let dirs: Vec<PathBuf> = arr
        .iter()
        .filter_map(|el| el.as_str())
        .filter(|s| *s != SENTINEL_UNSET)
        .map(PathBuf::from)
        .collect();
    if dirs.is_empty() { None } else { Some(dirs) }
}

/// Read `plansDirectory` from a parsed settings doc. Accepts either the
/// upstream string form OR an array-of-strings form (CLAUDE.md prose
/// describes both shapes; the upstream JSON schema is string-only, but
/// users who edit `.claude/settings.json` by hand routinely write arrays
/// and we should honour them at the canonical key rather than silently
/// ignoring them and forcing the namespaced `tomlctl.plansDirectories`
/// workaround). Returns `None` when the key is absent, when a string
/// value is the sentinel, or when an array is empty / sentinel-only.
fn read_plans_directory(doc: &JsonValue) -> Option<Vec<PathBuf>> {
    let v = doc.get("plansDirectory")?;
    // Array form: pattern-match `read_tomlctl_plans_directories` above.
    if let Some(arr) = v.as_array() {
        let dirs: Vec<PathBuf> = arr
            .iter()
            .filter_map(|el| el.as_str())
            .filter(|s| *s != SENTINEL_UNSET)
            .map(PathBuf::from)
            .collect();
        return if dirs.is_empty() { None } else { Some(dirs) };
    }
    // String form (upstream schema).
    let s = v.as_str()?;
    if s == SENTINEL_UNSET {
        return None;
    }
    Some(vec![PathBuf::from(s)])
}

/// Walk one plan directory: top-level `*.md` plus one level of subdirectories
/// for the multi-file `<feature>/00-outline.md` convention. Each discovered
/// plan is appended to `records` once; the `seen_slugs` set guards against
/// the cross-dir collision case (e.g. the same slug surfacing from two
/// configured dirs).
fn walk_plans(
    dir: &Path,
    root: &Path,
    records: &mut Vec<JsonValue>,
    seen_slugs: &mut std::collections::HashSet<String>,
) -> Result<()> {
    // Pass 1: top-level `*.md` files. Track which parent-dir-slugs have a
    // top-level `<slug>.md` so the "subdirectory has the same slug" collision
    // resolves to the multi-file outline (per the spec) rather than emitting
    // both the top-level file and the per-feature outline.
    let mut top_level_md_slugs: std::collections::HashSet<String> = Default::default();
    let entries = read_dir_sorted(dir)?;
    for entry in &entries {
        let path = entry.path();
        if path.is_file()
            && is_markdown(&path)
            && let Some(slug) = slug_from_md_filename(&path)
        {
            top_level_md_slugs.insert(slug);
        }
    }

    // Emit top-level plans first.
    for entry in &entries {
        let path = entry.path();
        if !path.is_file() || !is_markdown(&path) {
            continue;
        }
        let Some(slug) = slug_from_md_filename(&path) else {
            continue;
        };
        if seen_slugs.insert(slug.clone()) {
            records.push(build_plan_record(&path, &slug, root));
        }
    }

    // Pass 2: subdirectories. For each subdir, look for an outline file
    // (00-outline.md → index.md → README.md → lexicographically-first .md).
    for entry in &entries {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(parent_slug) = path
            .file_name()
            .and_then(|n| n.to_str())
            .map(str::to_string)
        else {
            continue;
        };
        // If the parent dir name ALSO matches a top-level <slug>.md we already
        // emitted, the spec says: prefer the multi-file convention. Detect
        // by re-emitting under the same slug (the seen-slugs guard would
        // otherwise drop us; we explicitly remove the prior entry first).
        let outline = pick_outline_md(&path)?;
        if let Some(outline_path) = outline {
            // Spec: when the same slug appeared at top-level and as a
            // subdirectory with an outline, prefer the subdirectory outline.
            if top_level_md_slugs.contains(&parent_slug) {
                // Remove the top-level record we already pushed under this
                // slug, then re-insert with the outline path.
                if let Some(pos) = records.iter().position(|r| {
                    r.get("slug").and_then(|s| s.as_str()) == Some(parent_slug.as_str())
                }) {
                    records.remove(pos);
                }
                seen_slugs.remove(&parent_slug);
            }
            if seen_slugs.insert(parent_slug.clone()) {
                records.push(build_plan_record(&outline_path, &parent_slug, root));
            }
        }
    }

    Ok(())
}

fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("md"))
        .unwrap_or(false)
}

/// Derive the slug from a top-level plan markdown filename (filename minus
/// `.md`). Returns `None` when the filename has no stem (impossible for an
/// `is_markdown`-passing path, but handled defensively).
fn slug_from_md_filename(path: &Path) -> Option<String> {
    path.file_stem()
        .and_then(|s| s.to_str())
        .map(str::to_string)
}

/// Pick the canonical outline file from a multi-file plan subdirectory.
/// Priority: `00-outline.md` → `index.md` → `README.md` → lexicographically
/// first `*.md`. Returns `Ok(None)` when the directory contains no `*.md`
/// at all (then the subdir contributes no record).
fn pick_outline_md(dir: &Path) -> Result<Option<PathBuf>> {
    let entries = read_dir_sorted(dir)?;
    let mds: Vec<PathBuf> = entries
        .iter()
        .map(|e| e.path())
        .filter(|p| p.is_file() && is_markdown(p))
        .collect();
    if mds.is_empty() {
        return Ok(None);
    }
    for preferred in ["00-outline.md", "index.md", "README.md"] {
        if let Some(p) = mds.iter().find(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.eq_ignore_ascii_case(preferred))
                .unwrap_or(false)
        }) {
            return Ok(Some(p.clone()));
        }
    }
    // Lexicographic-first fallback. `entries` was sorted, so the first md
    // in `mds` already is the lex-first.
    Ok(Some(mds[0].clone()))
}

/// Build the JSON record for one discovered plan. Emits the path relative
/// to `root` whenever possible (purely cosmetic — keeps output stable
/// across machines whose absolute paths differ).
fn build_plan_record(plan_path: &Path, slug: &str, root: &Path) -> JsonValue {
    let display_path = relativise(root, plan_path);

    let mut obj = serde_json::Map::new();
    obj.insert("path".to_string(), JsonValue::String(display_path));
    obj.insert("slug".to_string(), JsonValue::String(slug.to_string()));

    // Cross-reference: .claude/flows/<slug>/context.toml.
    let ctx_path = root
        .join(".claude")
        .join("flows")
        .join(slug)
        .join("context.toml");

    match read_context_toml(&ctx_path) {
        Some(ctx) => {
            obj.insert("has_flow".to_string(), JsonValue::Bool(true));
            if let Some(status) = ctx.status {
                obj.insert("status".to_string(), JsonValue::String(status));
            }
            if let Some(updated) = ctx.updated {
                obj.insert("updated".to_string(), JsonValue::String(updated));
            }
            if let Some(branch) = ctx.branch {
                obj.insert("branch".to_string(), JsonValue::String(branch));
            }
        }
        None => {
            obj.insert("has_flow".to_string(), JsonValue::Bool(false));
        }
    }

    JsonValue::Object(obj)
}

/// Subset of fields we extract from `<flow>/context.toml`. We deliberately
/// don't go through `crate::io::read_toml` here — that path layers the
/// tagged-error envelope and shared-lock plumbing, and a malformed flow
/// context.toml (rare; not user-facing in this command's contract) should
/// degrade gracefully to "no flow data" rather than aborting the whole
/// `find-plans` call.
struct ContextSummary {
    status: Option<String>,
    updated: Option<String>,
    branch: Option<String>,
}

fn read_context_toml(path: &Path) -> Option<ContextSummary> {
    let s = fs::read_to_string(path).ok()?;
    let doc: toml::Value = toml::from_str(&s).ok()?;
    let table = doc.as_table()?;
    let status = table
        .get("status")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    // `updated` is normally a TOML date literal (parsed as Datetime). Render
    // its `Display` form (YYYY-MM-DD) so the JSON output is a plain string,
    // not a typed-date object. Strings are also accepted for forward compat.
    let updated = table.get("updated").map(|v| match v {
        toml::Value::Datetime(dt) => dt.to_string(),
        toml::Value::String(s) => s.clone(),
        other => other.to_string(),
    });
    let branch = table
        .get("branch")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    Some(ContextSummary {
        status,
        updated,
        branch,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sentinel_string_falls_through() {
        let doc: JsonValue = serde_json::json!({"plansDirectory": "__DONT_ASK__"});
        assert!(read_plans_directory(&doc).is_none());
    }

    #[test]
    fn sentinel_array_falls_through() {
        let doc: JsonValue = serde_json::json!({"tomlctl": {"plansDirectories": ["__DONT_ASK__"]}});
        assert!(read_tomlctl_plans_directories(&doc).is_none());
    }

    #[test]
    fn array_filters_sentinel_entries() {
        let doc: JsonValue = serde_json::json!({
            "tomlctl": {"plansDirectories": ["docs/plans/", "__DONT_ASK__", "other/"]}
        });
        let dirs = read_tomlctl_plans_directories(&doc).unwrap();
        assert_eq!(
            dirs,
            vec![PathBuf::from("docs/plans/"), PathBuf::from("other/")]
        );
    }

    #[test]
    fn plans_directory_accepts_string() {
        let doc: JsonValue = serde_json::json!({"plansDirectory": "docs/plans/"});
        let dirs = read_plans_directory(&doc).unwrap();
        assert_eq!(dirs, vec![PathBuf::from("docs/plans/")]);
    }

    #[test]
    fn plans_directory_accepts_array() {
        let doc: JsonValue = serde_json::json!({
            "plansDirectory": ["docs/plans/", ".claude/plans/"]
        });
        let dirs = read_plans_directory(&doc).unwrap();
        assert_eq!(
            dirs,
            vec![
                PathBuf::from("docs/plans/"),
                PathBuf::from(".claude/plans/")
            ]
        );
    }

    #[test]
    fn plans_directory_array_filters_sentinel_entries() {
        let doc: JsonValue = serde_json::json!({
            "plansDirectory": ["docs/plans/", "__DONT_ASK__", "other/"]
        });
        let dirs = read_plans_directory(&doc).unwrap();
        assert_eq!(
            dirs,
            vec![PathBuf::from("docs/plans/"), PathBuf::from("other/")]
        );
    }

    #[test]
    fn plans_directory_array_all_sentinel_falls_through() {
        let doc: JsonValue = serde_json::json!({
            "plansDirectory": ["__DONT_ASK__"]
        });
        assert!(read_plans_directory(&doc).is_none());
    }

    #[test]
    fn plans_directory_empty_array_falls_through() {
        let doc: JsonValue = serde_json::json!({"plansDirectory": []});
        assert!(read_plans_directory(&doc).is_none());
    }
}
