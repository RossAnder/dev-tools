//! `review catalogue` — the merged lens catalogue's index, vocabulary,
//! ownership map and validation.

use std::path::PathBuf;

use anyhow::Result;
use serde_json::{Value as JsonValue, json};

use super::catalogue::{Catalogue, IndexFilter, default_builtin_root};
use super::fields::Family;
use crate::cli::ReviewCatalogueArgs;
use crate::errors::{ErrorKind, tagged_err};
use crate::io::repo_or_cwd_root;
use crate::output::print_json;

/// Prints the merged index by default, `--vocabulary` the signals with their
/// detectors, `--owners` the generated ownership map; `--check` prints
/// `{"ok":true}` on success and fails listing every validation error.
pub(crate) fn run(args: ReviewCatalogueArgs) -> Result<()> {
    let builtin = match args.builtin {
        Some(dir) => dir,
        None => default_builtin_root().ok_or_else(|| {
            tagged_err(
                ErrorKind::NotFound,
                None,
                "review catalogue: no built-in catalogue root; pass --builtin or set CLAUDE_CONFIG_DIR",
            )
        })?,
    };
    let project: Option<PathBuf> = match args.project {
        Some(dir) => Some(dir),
        None => Some(repo_or_cwd_root()?.join(".claude").join("review-lenses")),
    };
    let family = match args.family.as_deref() {
        None => None,
        Some(text) => Some(Family::parse(text).ok_or_else(|| {
            tagged_err(
                ErrorKind::Validation,
                None,
                format!(
                    "review catalogue: --family `{text}` is not one of {}",
                    Family::VALUES.join(", ")
                ),
            )
        })?),
    };
    let catalogue = Catalogue::load(&builtin, project.as_deref())?;

    if args.check {
        let errors = catalogue.validate();
        if errors.is_empty() {
            return print_json(&json!({"ok": true}));
        }
        let lines: Vec<String> = errors.iter().map(ToString::to_string).collect();
        return Err(tagged_err(
            ErrorKind::Validation,
            None,
            format!(
                "review catalogue: {} validation error(s):\n{}",
                lines.len(),
                lines.join("\n")
            ),
        ));
    }
    if args.vocabulary {
        return print_json(&vocabulary(&catalogue));
    }
    if args.owners {
        return print_json(&owners(&catalogue));
    }

    let entries = catalogue.index(&IndexFilter {
        family,
        signals: args.signals,
    });
    print_json(&json!({
        "tree_sha": catalogue.tree_sha(),
        "config_sha": catalogue.config_sha(),
        "entries": entries,
    }))
}

fn vocabulary(catalogue: &Catalogue) -> JsonValue {
    let signals: Vec<JsonValue> = catalogue
        .signals()
        .iter()
        .map(|s| {
            json!({
                "name": s.name,
                "kind": s.kind.as_str(),
                "meaning": s.meaning,
                "detect": s.detect,
                "hint": s.hint,
                "layer": s.layer.as_str(),
            })
        })
        .collect();
    json!({ "signals": signals })
}

fn owners(catalogue: &Catalogue) -> JsonValue {
    let rows: Vec<JsonValue> = catalogue
        .owners()
        .into_iter()
        .map(|((family, tag), owners)| json!({"family": family, "tag": tag, "owners": owners}))
        .collect();
    json!({ "owners": rows })
}
