//! The `ref` slug rule and the normalised matcher record reconciliation compares on.
//!
//! The precedent is `github-slugger` (the GitHub heading-anchor rule), with two
//! deliberate divergences: runs of whitespace and hyphens collapse to a single
//! hyphen (`Setup & Config` slugs to `setup-config`, not `setup--config`), and
//! underscores are kept rather than stripped, because execution records already
//! on disk carry `task_ref`s containing them.

use std::collections::HashMap;

/// Slugs a task heading title. The caller strips the leading number and the
/// trailing effort tag first.
pub(crate) fn derive_ref(title: &str) -> String {
    let mut out = String::with_capacity(title.len());
    let mut pending_separator = false;

    for ch in title.chars().flat_map(char::to_lowercase) {
        let ch = if ch.is_whitespace() { ' ' } else { ch };
        match ch {
            ' ' | '-' => pending_separator = true,
            'a'..='z' | '0'..='9' | '_' => {
                if pending_separator && !out.is_empty() {
                    out.push('-');
                }
                pending_separator = false;
                out.push(ch);
            }
            _ => {}
        }
    }

    out
}

/// Suffixes repeats in document order: the first keeps the bare ref, later ones
/// become `<ref>-2`, `<ref>-3`.
pub(crate) fn dedupe_refs(refs: &mut [String]) {
    let mut seen: HashMap<String, usize> = HashMap::new();
    for slot in refs.iter_mut() {
        let count = seen.entry(slot.clone()).or_insert(0);
        *count += 1;
        if *count > 1 {
            slot.push_str(&format!("-{}", count));
        }
    }
}

/// Collapses a ref to `[a-z0-9]` so `count_distinct` and `count-distinct` match,
/// which is what lets import adopt a pre-existing execution-record `task_ref`.
pub(crate) fn normalise_for_match(value: &str) -> String {
    value
        .chars()
        .flat_map(char::to_lowercase)
        .filter(|ch| ch.is_ascii_alphanumeric())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_a_live_execution_record_ref() {
        assert_eq!(
            derive_ref("Arm the sort-engaged `count_distinct` test with a filter"),
            "arm-the-sort-engaged-count_distinct-test-with-a-filter"
        );
    }

    #[test]
    fn collapses_runs_left_by_dropped_characters() {
        assert_eq!(derive_ref("Setup & Config"), "setup-config");
    }

    #[test]
    fn numbers_repeats_in_document_order() {
        let mut refs = vec!["x".to_string(), "x".to_string()];
        dedupe_refs(&mut refs);
        assert_eq!(refs, vec!["x".to_string(), "x-2".to_string()]);
    }

    #[test]
    fn matcher_ignores_separators() {
        assert_eq!(
            normalise_for_match("count_distinct"),
            normalise_for_match("count-distinct")
        );
    }
}
