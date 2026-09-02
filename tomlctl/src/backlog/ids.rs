//! Content-derived identity: the `dedup_id` fingerprint and the `B-<hex>` id
//! derived from it.
//!
//! Collision widening is ordered by `dedup_id`, never by insertion order, so
//! two worktrees resolve the same collision the same way. The consequence is
//! that a later-minted, lexicographically smaller `dedup_id` claims the short
//! id an incumbent holds; the caller re-derives the incumbent, and
//! `schema::validate_ids_unique` is what refuses to write the pair otherwise.

// Identity is consumed by the verb leaves, not by this file.
#![allow(dead_code)]

use std::collections::BTreeSet;

use serde_json::Value as JsonValue;
use toml::Value as TomlValue;

use crate::backlog::normalise::normalise;
use crate::backlog::schema::{
    ARRAY_BACKLOG, ARRAY_COMPACTED, FIELD_AREA, FIELD_DEDUP_ID, FIELD_KIND, FIELD_SUMMARY,
};
use crate::convert::str_field_json;
use crate::dedup::fingerprint_hex;
use crate::io::items_array;

/// Values fed to the fingerprint hash, in order. `summary` is hashed through
/// `normalise`, so a rephrasing that differs only in case, punctuation or
/// stopwords folds onto the same id; `kind` and `area` are hashed verbatim.
///
/// Deliberately not `dedup::FINGERPRINTED_FIELDS`: that list is the ledger's
/// and is frozen against pre-existing `dedup_id`s.
pub(crate) const BACKLOG_FINGERPRINT_FIELDS: [&str; 3] =
    [FIELD_KIND, FIELD_AREA, FIELD_SUMMARY];

const ID_PREFIX: &str = "B-";

/// Hex widths an id may take, narrowest first. 8 hex is 32 bits, which
/// collides in a store of this size only by accident; 12 is a backstop, not a
/// tier anything is expected to reach.
const ID_WIDTHS: [usize; 3] = [8, 10, 12];

/// Fingerprint of a whole item. A non-object, or an item missing any of the
/// three fields, hashes those as the empty string, so the function is total
/// and a half-built item still yields a stable id.
pub(crate) fn dedup_id(item: &JsonValue) -> String {
    let empty = serde_json::Map::new();
    let obj = item.as_object().unwrap_or(&empty);
    dedup_id_from_parts(
        str_field_json(obj, FIELD_KIND),
        str_field_json(obj, FIELD_AREA),
        str_field_json(obj, FIELD_SUMMARY),
    )
}

/// Fingerprint from loose parts, for `check`, which has flags rather than a
/// row. `summary` is raw here — normalisation happens inside, so a caller
/// cannot fold it twice or forget to fold it once.
pub(crate) fn dedup_id_from_parts(kind: &str, area: &str, summary: &str) -> String {
    let summary = normalise(summary);
    fingerprint_hex(BACKLOG_FINGERPRINT_FIELDS.map(|field| match field {
        FIELD_KIND => kind,
        FIELD_AREA => area,
        FIELD_SUMMARY => summary.as_str(),
        other => unreachable!("fingerprint field {other} has no input"),
    }))
}

/// The id `dedup_id` takes among the rows whose fingerprints are `existing`.
///
/// Widening is decided by a total order over `dedup_id` — of the fingerprints
/// sharing a prefix, the lexicographically smallest keeps that width and the
/// rest widen — so the answer depends on the set of coexisting rows and not on
/// the order they arrived in. An insertion-order tie-break instead hands two
/// worktrees mirror-image assignments for the same pair.
pub(crate) fn derive_id(dedup_id: &str, existing: &BTreeSet<&str>) -> String {
    let mut chosen = prefix_of(dedup_id, ID_WIDTHS[0]);
    for width in ID_WIDTHS {
        chosen = prefix_of(dedup_id, width);
        let outranked = existing.iter().any(|other| {
            *other != dedup_id && prefix_of(other, width) == chosen && *other < dedup_id
        });
        if !outranked {
            break;
        }
    }
    format!("{ID_PREFIX}{chosen}")
}

/// Fingerprints of every stored row, live and compacted — the input
/// `derive_id` ranks against. Compacted rows count: their ids are still taken.
pub(crate) fn existing_dedup_ids(doc: &TomlValue) -> BTreeSet<&str> {
    let mut out = BTreeSet::new();
    for array in [ARRAY_BACKLOG, ARRAY_COMPACTED] {
        for item in items_array(doc, array) {
            if let Some(id) = item.get(FIELD_DEDUP_ID).and_then(TomlValue::as_str)
                && !id.is_empty()
            {
                out.insert(id);
            }
        }
    }
    out
}

/// Leading `width` bytes, or the whole string when it is shorter. `get`
/// rather than a slice so a fingerprint that is somehow not ASCII hex cannot
/// panic on a char boundary.
fn prefix_of(dedup_id: &str, width: usize) -> &str {
    dedup_id.get(..width).unwrap_or(dedup_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Digests computed outside this crate with
    /// `printf '<kind>|<area>|<normalised summary>' | sha256sum | cut -c1-16`
    /// (2026-09-01). This is the pin that stops a change to the field order,
    /// the separator or the truncation silently re-keying the store.
    #[test]
    fn dedup_id_matches_a_digest_pinned_outside_the_crate() {
        let item = json!({
            "kind": "flaky-test",
            "area": "lumina/server/tests/pty_readiness_probe.rs",
            "summary": "PTY readiness probe flakes on slow CI",
        });
        assert_eq!(dedup_id(&item), "882a6bb55c6f8378");
        assert_eq!(dedup_id(&item).len(), 16);
    }

    /// An accented letter is the case that separates the byte pipeline from a
    /// char pipeline: `normalise` yields "caf latte", not "café latte". A
    /// curly apostrophe does not discriminate the two.
    #[test]
    fn dedup_id_is_taken_over_the_byte_normalised_summary() {
        let item = json!({"kind": "bug", "area": "tomlctl/src", "summary": "Café latte"});
        assert_eq!(dedup_id(&item), "765021c7ef4c1bae");
        assert_ne!(dedup_id(&item), "4218c4e1e381314d");
    }

    #[test]
    fn punctuation_and_case_variants_share_an_id() {
        let plain = json!({
            "kind": "bug",
            "area": "tomlctl/src/io.rs",
            "summary": "guard_write_path refuses a symlinked leaf",
        });
        let rephrased = json!({
            "kind": "bug",
            "area": "tomlctl/src/io.rs",
            "summary": "  GUARD_WRITE_PATH refuses, a symlinked leaf!! ",
        });
        assert_eq!(dedup_id(&plain), dedup_id(&rephrased));
        let existing = BTreeSet::new();
        assert_eq!(
            derive_id(&dedup_id(&plain), &existing),
            derive_id(&dedup_id(&rephrased), &existing)
        );
    }

    #[test]
    fn missing_fields_hash_as_empty_strings() {
        assert_eq!(dedup_id(&json!({})), dedup_id(&JsonValue::Null));
        assert_eq!(
            dedup_id(&json!({"kind": "bug"})),
            dedup_id(&json!({"kind": "bug", "area": "", "summary": ""}))
        );
    }

    #[test]
    fn an_uncontested_id_is_the_eight_hex_prefix() {
        let existing = BTreeSet::from(["ffffffffffffffff"]);
        assert_eq!(derive_id("0123456789abcdef", &existing), "B-01234567");
    }

    /// Re-deriving a row already in the store must not widen: the row's own
    /// fingerprint is not a competitor with itself.
    #[test]
    fn a_row_does_not_collide_with_itself() {
        let existing = BTreeSet::from(["0123456789abcdef"]);
        assert_eq!(derive_id("0123456789abcdef", &existing), "B-01234567");
    }

    /// The load-bearing property: which of a colliding pair widens is fixed by
    /// the fingerprint order, so both insertion orders reach the same
    /// assignment. An insertion-order tie-break inverts the second block.
    #[test]
    fn the_larger_fingerprint_widens_whichever_is_stored_first() {
        let smaller = "0123456789abcdef";
        let larger = "01234567ffffffff";

        // Smaller minted first.
        assert_eq!(derive_id(smaller, &BTreeSet::new()), "B-01234567");
        assert_eq!(
            derive_id(larger, &BTreeSet::from([smaller])),
            "B-01234567ff"
        );

        // Larger minted first: it takes the short id alone, and the smaller
        // one arriving after still ranks ahead of it.
        assert_eq!(derive_id(larger, &BTreeSet::new()), "B-01234567");
        assert_eq!(derive_id(smaller, &BTreeSet::from([larger])), "B-01234567");

        // Both stored: one assignment, reached from either direction.
        let both = BTreeSet::from([smaller, larger]);
        assert_eq!(derive_id(smaller, &both), "B-01234567");
        assert_eq!(derive_id(larger, &both), "B-01234567ff");
    }

    #[test]
    fn a_ten_hex_collision_widens_to_twelve() {
        let smaller = "0123456789abcdef";
        let larger = "0123456789ffffff";
        assert_eq!(derive_id(smaller, &BTreeSet::from([larger])), "B-01234567");
        assert_eq!(
            derive_id(larger, &BTreeSet::from([smaller])),
            "B-0123456789ff"
        );
    }

    #[test]
    fn existing_dedup_ids_spans_both_arrays() {
        let doc: TomlValue = toml::from_str(
            r#"
[[backlog]]
id = "B-01234567"
dedup_id = "0123456789abcdef"

[[backlog]]
id = "B-89abcdef"

[[compacted]]
id = "B-fedcba98"
dedup_id = "fedcba9876543210"
"#,
        )
        .unwrap();
        assert_eq!(
            existing_dedup_ids(&doc),
            BTreeSet::from(["0123456789abcdef", "fedcba9876543210"])
        );
    }
}
