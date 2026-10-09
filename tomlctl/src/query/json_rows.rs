//! `--where*` filtering over JSON rows, for reports that are not read from a
//! TOML array. Each row is converted to TOML once and evaluated by the same
//! predicate code the list verbs use, so typed comparisons, array fields and
//! dotted keys behave identically. A JSON string that looks like a date stays
//! a string, and a `null` field counts as absent.

use anyhow::Result;
use serde_json::Value as JsonValue;

use super::{Predicate, PreparedPredicates};
use crate::convert::json_to_toml;

/// Keep only the rows that satisfy every predicate, preserving order. A
/// non-object row matches nothing.
pub(crate) fn filter(rows: &mut Vec<JsonValue>, preds: &[Predicate]) -> Result<()> {
    if preds.is_empty() {
        return Ok(());
    }
    let prepared = PreparedPredicates::new(preds)?;
    let mut keep = Vec::with_capacity(rows.len());
    for row in rows.iter() {
        let toml = json_to_toml(&without_nulls(row))?;
        keep.push(prepared.matches(&toml)?);
    }
    let mut keep = keep.into_iter();
    rows.retain(|_| keep.next().unwrap_or(false));
    Ok(())
}

/// TOML has no null, so `json_to_toml` refuses one; dropping null object
/// members and array elements lets such a row convert, with the field read
/// as absent.
fn without_nulls(v: &JsonValue) -> JsonValue {
    match v {
        JsonValue::Object(m) => JsonValue::Object(
            m.iter()
                .filter(|(_, v)| !v.is_null())
                .map(|(k, v)| (k.clone(), without_nulls(v)))
                .collect(),
        ),
        JsonValue::Array(a) => JsonValue::Array(
            a.iter()
                .filter(|v| !v.is_null())
                .map(without_nulls)
                .collect(),
        ),
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ids(rows: &[JsonValue]) -> Vec<&str> {
        rows.iter().filter_map(|r| r["id"].as_str()).collect()
    }

    fn rows() -> Vec<JsonValue> {
        vec![
            json!({"id": "a", "files": ["src/output.rs", "src/lib.rs"], "n": 3, "date": "2026-01-02", "meta": {"tier": "deep"}}),
            json!({"id": "b", "files": ["docs/x.md"], "n": 10, "date": "2026-03-04", "meta": {"tier": "lite"}, "note": null}),
            json!({"id": "c", "files": [], "n": 7}),
        ]
    }

    #[test]
    fn array_field_matches_any_element() {
        let mut r = rows();
        filter(
            &mut r,
            &[Predicate::WhereContains {
                key: "files".into(),
                sub: "output.rs".into(),
            }],
        )
        .unwrap();
        assert_eq!(ids(&r), ["a"]);
    }

    #[test]
    fn where_not_excludes_rows_holding_the_value() {
        let mut r = rows();
        filter(
            &mut r,
            &[Predicate::WhereNot {
                key: "files".into(),
                rhs: "docs/x.md".into(),
            }],
        )
        .unwrap();
        assert_eq!(ids(&r), ["a", "c"]);
    }

    #[test]
    fn integer_comparison_is_typed() {
        let mut r = rows();
        filter(
            &mut r,
            &[Predicate::WhereGt {
                key: "n".into(),
                rhs: "5".into(),
            }],
        )
        .unwrap();
        assert_eq!(ids(&r), ["b", "c"]);
    }

    #[test]
    fn dotted_key_navigates_nested_object() {
        let mut r = rows();
        filter(
            &mut r,
            &[Predicate::Where {
                key: "meta.tier".into(),
                rhs: "lite".into(),
            }],
        )
        .unwrap();
        assert_eq!(ids(&r), ["b"]);
    }

    #[test]
    fn date_string_compares_as_string() {
        let mut r = rows();
        filter(
            &mut r,
            &[Predicate::WhereGte {
                key: "date".into(),
                rhs: "2026-02".into(),
            }],
        )
        .unwrap();
        assert_eq!(ids(&r), ["b"]);
    }

    #[test]
    fn null_field_reads_as_missing() {
        let mut r = rows();
        filter(&mut r, &[Predicate::WhereMissing { key: "note".into() }]).unwrap();
        assert_eq!(ids(&r), ["a", "b", "c"]);
    }

    #[test]
    fn non_object_rows_match_nothing() {
        let mut r = vec![json!("plain"), json!({"id": "x"})];
        filter(&mut r, &[Predicate::WhereHas { key: "id".into() }]).unwrap();
        assert_eq!(r, [json!({"id": "x"})]);
    }

    #[test]
    fn no_predicates_keeps_every_row() {
        let mut r = rows();
        filter(&mut r, &[]).unwrap();
        assert_eq!(r.len(), 3);
    }
}
