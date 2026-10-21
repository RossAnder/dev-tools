//! The `--text-view` block rendering: each object as `key: value` lines, a
//! row report as its header block and then one block per row, blank-line
//! separated.

use serde_json::Value as JsonValue;
use std::collections::HashSet;

use super::template::value_text;

/// One report as text. An array is rendered as header-less rows.
pub(crate) fn render(v: &JsonValue) -> String {
    match v {
        JsonValue::Array(rows) => render_rows(None, rows),
        v => finish(block(v)),
    }
}

/// `header`'s block, when it has any line, then one block per row. Always
/// ends with a newline, even when nothing else is printed.
pub(crate) fn render_rows(header: Option<&JsonValue>, rows: &[JsonValue]) -> String {
    let mut blocks = Vec::with_capacity(rows.len() + 1);
    if let Some(h) = header {
        let b = block(h);
        if !b.is_empty() {
            blocks.push(b);
        }
    }
    blocks.extend(rows.iter().map(block));
    finish(blocks.join("\n"))
}

fn finish(text: String) -> String {
    if text.is_empty() {
        "\n".to_string()
    } else {
        text
    }
}

/// An object's lines; any other value is one bare line. A key already
/// printed is not printed again, so a dotted key a `--select` projection
/// wrote beside the object it came from appears once.
fn block(v: &JsonValue) -> String {
    let mut out = String::new();
    match v {
        JsonValue::Object(map) => {
            let mut seen = HashSet::new();
            for (k, c) in map {
                push_field(&mut out, &mut seen, k, c);
            }
        }
        v => {
            out.push_str(&value_text(v));
            out.push('\n');
        }
    }
    out
}

fn push_field(out: &mut String, seen: &mut HashSet<String>, key: &str, v: &JsonValue) {
    match v {
        JsonValue::Null => {}
        JsonValue::Object(map) if !map.is_empty() => {
            for (k, c) in map {
                push_field(out, seen, &format!("{key}.{k}"), c);
            }
        }
        JsonValue::Array(items) if items.iter().all(is_scalar) => {
            let text = items.iter().map(value_text).collect::<Vec<_>>().join(", ");
            push_text(out, seen, key, &text);
        }
        v => push_text(out, seen, key, &value_text(v)),
    }
}

fn is_scalar(v: &JsonValue) -> bool {
    !matches!(v, JsonValue::Array(_) | JsonValue::Object(_))
}

/// `key: text`, or for text spanning lines `key:` followed by each line
/// indented two spaces (an empty line stays empty).
fn push_text(out: &mut String, seen: &mut HashSet<String>, key: &str, text: &str) {
    if !seen.insert(key.to_string()) {
        return;
    }
    out.push_str(key);
    out.push(':');
    if text.contains('\n') {
        out.push('\n');
        for line in text.lines() {
            if !line.is_empty() {
                out.push_str("  ");
                out.push_str(line);
            }
            out.push('\n');
        }
    } else {
        if !text.is_empty() {
            out.push(' ');
            out.push_str(text);
        }
        out.push('\n');
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_scalar_object_prints_one_line_per_key_with_strings_bare() {
        let v = json!({ "id": 3, "title": "say \"hi\"", "done": false, "gone": null });
        assert_eq!(render(&v), "id: 3\ntitle: say \"hi\"\ndone: false\n");
    }

    #[test]
    fn a_multi_line_string_prints_its_lines_indented_under_the_key() {
        let v = json!({ "id": 1, "body": "first\n\nthird\r\nfourth\n", "after": "x" });
        assert_eq!(
            render(&v),
            "id: 1\nbody:\n  first\n\n  third\n  fourth\nafter: x\n"
        );
    }

    #[test]
    fn an_array_of_scalars_is_comma_joined_and_one_of_objects_is_compact_json() {
        let v = json!({ "files": ["a.rs", "b.rs"], "deps": [2, 3], "rows": [{ "k": 1 }] });
        assert_eq!(
            render(&v),
            "files: a.rs, b.rs\ndeps: 2, 3\nrows: [{\"k\":1}]\n"
        );
    }

    #[test]
    fn a_nested_object_flattens_to_dotted_keys() {
        let v = json!({ "policy": { "mode": "m", "cap": { "n": 2 }, "none": null }, "empty": {} });
        assert_eq!(render(&v), "policy.mode: m\npolicy.cap.n: 2\nempty: {}\n");
    }

    #[test]
    fn a_dotted_key_beside_its_source_object_prints_once() {
        let v = json!({ "a.b": 1, "a": { "b": 1, "c": 2 } });
        assert_eq!(render(&v), "a.b: 1\na.c: 2\n");
    }

    #[test]
    fn a_row_report_prints_its_header_then_blank_line_separated_rows() {
        let header = json!({ "ok": true, "total": 2 });
        let rows = [json!({ "id": 1 }), json!({ "id": 2, "note": "a\nb" })];
        assert_eq!(
            render_rows(Some(&header), &rows),
            "ok: true\ntotal: 2\n\nid: 1\n\nid: 2\nnote:\n  a\n  b\n"
        );
    }

    #[test]
    fn an_empty_header_is_skipped_and_empty_output_is_one_newline() {
        assert_eq!(
            render_rows(Some(&json!({})), &[json!({ "id": 1 })]),
            "id: 1\n"
        );
        assert_eq!(render_rows(None, &[]), "\n");
        assert_eq!(render(&json!([{ "id": 1 }, "bare"])), "id: 1\n\nbare\n");
    }
}
