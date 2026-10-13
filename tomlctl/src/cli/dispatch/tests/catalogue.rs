//! Validation gate over the shipped lens catalogue.

use crate::review::catalogue::Catalogue;
use std::path::Path;

/// The built-in catalogue under `claude/skills/review-v2-lenses/`, merged with
/// this repo's project layer at `.claude/review-lenses/`, must validate clean.
/// A failure lists every error rather than the first, so one run shows the
/// whole repair.
#[test]
fn catalogue_validates() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let repo_root = crate_dir.parent().expect("repo root").to_path_buf();
    let builtin = repo_root
        .join("claude")
        .join("skills")
        .join("review-v2-lenses");
    let project = repo_root.join(".claude").join("review-lenses");

    let catalogue = Catalogue::load(&builtin, Some(&project)).unwrap_or_else(|e| {
        panic!(
            "catalogue_validates: cannot load {}: {e}",
            builtin.display()
        )
    });
    let errors = catalogue.validate();
    if !errors.is_empty() {
        let lines: Vec<String> = errors.iter().map(|e| format!("  {e}")).collect();
        panic!(
            "catalogue_validates: {} validation error(s):\n{}",
            errors.len(),
            lines.join("\n")
        );
    }
}
