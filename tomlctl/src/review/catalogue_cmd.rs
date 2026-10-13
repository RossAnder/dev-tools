//! `review catalogue` — the merged lens catalogue's index, vocabulary,
//! ownership map and validation.

use anyhow::Result;
use serde_json::{Value as JsonValue, json};

use super::catalogue::{Catalogue, IndexFilter, default_builtin_root, project_layer};
use super::fields::Family;
use crate::cli::ReviewCatalogueArgs;
use crate::errors::{ErrorKind, tagged_err};
use crate::io::repo_or_cwd_root;
use crate::output::print_json;

/// Prints the merged index by default, `--vocabulary` the signals with their
/// detectors, `--owners` the generated ownership map; `--check` prints
/// `{"ok":true}` on success and fails listing every validation error. The
/// other modes fail when a catalogue file could not be read.
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
    let project = project_layer(&repo_or_cwd_root()?, args.project)?;
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
    let load_errors = catalogue.load_errors();
    if !load_errors.is_empty() {
        let lines: Vec<String> = load_errors.iter().map(ToString::to_string).collect();
        return Err(tagged_err(
            ErrorKind::Validation,
            None,
            format!(
                "review catalogue: the catalogue could not be read ({} error(s)); `--check` lists every problem:\n{}",
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

    if let Some(name) = args
        .signals
        .iter()
        .find(|name| !catalogue.signals().iter().any(|s| s.name == **name))
    {
        return Err(tagged_err(
            ErrorKind::Validation,
            None,
            format!("review catalogue: --signals `{name}` is not a signal of the vocabulary"),
        ));
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
