//! R17: canonical typed schema for `.claude/active-flow.toml`.
//!
//! Single source of truth for the registry's wire shape. Consumers
//! (`flow::active`, `flow::resolve`, `flow::list`) parse via
//! `ActiveDoc::from_toml_str` (or `from_toml_value` when they already hold
//! a parsed `TomlValue`) rather than each re-walking `TomlValue` chains.
//! The on-disk shape:
//!
//! ```toml
//! schema_version = 1
//! [[active]]
//! slug = "feature-x"
//! last_used = "2026-05-08T14:32:00Z"
//! [active.binding]
//! branch = "feat/x"
//! worktree = "/home/user/dev/repo"
//! scope = ["src/foo/**"]
//! ```
//!
//! Behaviour intentionally mirrors the previous-per-site walks:
//!
//! - `schema_version` defaults to `1` when absent (R17 keeps the existing
//!   `flow::active::list` defaulting convention so the JSON output shape
//!   stays byte-identical).
//! - Entries that are not tables are silently skipped (matches the
//!   defensive `if let Some(tbl) = entry.as_table() else { continue }`
//!   pattern used at every site previously).
//! - Entries that lack a `slug` field are silently skipped (matches
//!   `flow::resolve`'s `let Some(slug) = … else { continue }` chain).
//! - Empty / missing optional fields (`last_used`, `binding.branch`,
//!   `binding.worktree`, `binding.scope`) are preserved as
//!   `String::new()` / `None` / `Vec::new()` exactly as the previous
//!   walks emitted, so all three downstream consumers see the same
//!   defaults they did before consolidation.

use std::path::Path;

use anyhow::{Context, Result};
use toml::Value as TomlValue;

/// Top-level shape of `.claude/active-flow.toml`.
#[derive(Debug, Clone, Default)]
pub(crate) struct ActiveDoc {
    pub(crate) schema_version: i64,
    pub(crate) active: Vec<ActiveEntry>,
}

/// One `[[active]]` table.
#[derive(Debug, Clone, Default)]
pub(crate) struct ActiveEntry {
    pub(crate) slug: String,
    /// RFC3339 timestamp string. Empty when absent — preserves the
    /// pre-consolidation default and lexicographic-comparison contract
    /// `pick_active_latest` relies on.
    pub(crate) last_used: String,
    pub(crate) binding: Binding,
}

/// Optional advisory `[active.binding]` table.
#[derive(Debug, Clone, Default)]
pub(crate) struct Binding {
    pub(crate) branch: Option<String>,
    pub(crate) worktree: Option<String>,
    pub(crate) scope: Vec<String>,
}

impl ActiveDoc {
    /// Parse from raw TOML text. Returns a tagged error on lex/parse
    /// failure; otherwise hands off to `from_toml_value`.
    pub(crate) fn from_toml_str(s: &str) -> Result<Self> {
        let v: TomlValue = toml::from_str(s).context("parsing active-flow.toml")?;
        Ok(Self::from_toml_value(&v))
    }

    /// Project an already-parsed `TomlValue` into the canonical typed
    /// shape. Defensive against malformed entries — non-table rows and
    /// rows missing `slug` are silently skipped (matches the per-site
    /// behaviour the consolidation replaces).
    pub(crate) fn from_toml_value(doc: &TomlValue) -> Self {
        let schema_version = doc
            .get("schema_version")
            .and_then(|v| v.as_integer())
            .unwrap_or(1);
        let arr = doc.get("active").and_then(|v| v.as_array());
        let active = arr
            .map(|a| {
                a.iter()
                    .filter_map(ActiveEntry::from_toml_value)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        ActiveDoc {
            schema_version,
            active,
        }
    }
}

impl ActiveEntry {
    /// Project a single `[[active]]` `TomlValue` into the typed shape.
    /// Returns `None` for non-table entries or entries missing `slug` —
    /// silent-skip matches the previous-per-site contract.
    pub(crate) fn from_toml_value(entry: &TomlValue) -> Option<Self> {
        let tbl = entry.as_table()?;
        let slug = tbl.get("slug").and_then(|v| v.as_str())?.to_string();
        let last_used = tbl
            .get("last_used")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let binding = tbl
            .get("binding")
            .and_then(|v| v.as_table())
            .map(|bt| {
                let branch = bt.get("branch").and_then(|v| v.as_str()).map(str::to_string);
                let worktree = bt
                    .get("worktree")
                    .and_then(|v| v.as_str())
                    .map(str::to_string);
                let scope = bt
                    .get("scope")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(str::to_string))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                Binding {
                    branch,
                    worktree,
                    scope,
                }
            })
            .unwrap_or_default();
        Some(ActiveEntry {
            slug,
            last_used,
            binding,
        })
    }
}

impl Binding {
    /// True when none of the optional fields are populated. Used by
    /// active-flow writers to decide whether to emit `[active.binding]`.
    pub(crate) fn is_empty(&self) -> bool {
        self.branch.is_none() && self.worktree.is_none() && self.scope.is_empty()
    }

    /// Convenience: the worktree as a `Path`-comparable view, for
    /// resolve.rs's binding-match scoring.
    pub(crate) fn worktree_path(&self) -> Option<&Path> {
        self.worktree.as_deref().map(Path::new)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_canonical_doc() {
        let body = r#"schema_version = 1
[[active]]
slug = "feature-x"
last_used = "2026-05-08T14:32:00Z"
[active.binding]
branch = "feat/x"
worktree = "/home/user/dev/repo"
scope = ["src/foo/**", "src/bar/**"]
"#;
        let doc = ActiveDoc::from_toml_str(body).expect("parse ok");
        assert_eq!(doc.schema_version, 1);
        assert_eq!(doc.active.len(), 1);
        let e = &doc.active[0];
        assert_eq!(e.slug, "feature-x");
        assert_eq!(e.last_used, "2026-05-08T14:32:00Z");
        assert_eq!(e.binding.branch.as_deref(), Some("feat/x"));
        assert_eq!(e.binding.worktree.as_deref(), Some("/home/user/dev/repo"));
        assert_eq!(e.binding.scope, vec!["src/foo/**", "src/bar/**"]);
    }

    #[test]
    fn missing_schema_version_defaults_to_1() {
        let body = r#"[[active]]
slug = "x"
"#;
        let doc = ActiveDoc::from_toml_str(body).expect("parse ok");
        assert_eq!(doc.schema_version, 1);
        assert_eq!(doc.active.len(), 1);
        assert_eq!(doc.active[0].slug, "x");
        assert!(doc.active[0].last_used.is_empty());
        assert!(doc.active[0].binding.is_empty());
    }

    #[test]
    fn entry_without_slug_is_skipped() {
        let body = r#"[[active]]
last_used = "2026-05-08T00:00:00Z"
[[active]]
slug = "ok"
"#;
        let doc = ActiveDoc::from_toml_str(body).expect("parse ok");
        assert_eq!(doc.active.len(), 1);
        assert_eq!(doc.active[0].slug, "ok");
    }

    #[test]
    fn empty_doc_yields_default() {
        let doc = ActiveDoc::from_toml_str("").expect("parse ok");
        assert_eq!(doc.schema_version, 1);
        assert!(doc.active.is_empty());
    }

    #[test]
    fn binding_partial_populates_only_present_fields() {
        let body = r#"[[active]]
slug = "x"
[active.binding]
branch = "feat/x"
"#;
        let doc = ActiveDoc::from_toml_str(body).expect("parse ok");
        let b = &doc.active[0].binding;
        assert_eq!(b.branch.as_deref(), Some("feat/x"));
        assert!(b.worktree.is_none());
        assert!(b.scope.is_empty());
        assert!(!b.is_empty());
    }
}
