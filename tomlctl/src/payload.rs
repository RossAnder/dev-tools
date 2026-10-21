// TOML record files: a `.toml` path given to a payload flag that already takes
// a file is converted here to the JSON text the flag's parser consumes, so
// every verb behind `--ndjson`, `--json '@…'` and `--ops '@…'` accepts one.
//
// Shape: a top-level `record` array is the payload's rows, and no other
// top-level key may sit beside it; any other document is one object. Every
// multi-line string (its source token opens `'''` or `"""`) has CRLF read as
// LF and exactly one trailing `\n` removed, matching `io::read_flag_file`, so a
// block whose closing delimiter sits on its own line reads back as typed.
// Single-line strings are left as written. Every error is `kind=validation`.

use std::path::Path;

use anyhow::Result;
use serde_json::Value as JsonValue;
use toml::Spanned;
use toml::de::{DeTable, DeValue};

use crate::errors::{ErrorKind, tagged_err};

/// The top-level key whose array holds the payload's rows.
const ROWS_KEY: &str = "record";

/// Whether `path` names a TOML record file: its extension is `toml`, in any
/// case.
pub(crate) fn is_toml_path(path: &str) -> bool {
    Path::new(path)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("toml"))
}

/// `text`, read from `path`, as compact JSON: one line per row (or one line
/// for a single object) when `as_ndjson`, else the JSON array or object.
pub(crate) fn toml_payload(path: &Path, text: &str, as_ndjson: bool) -> Result<String> {
    let payload = toml_payload_value(path, text)?;
    match (&payload, as_ndjson) {
        (JsonValue::Array(rows), true) => {
            let lines: Vec<String> = rows.iter().map(JsonValue::to_string).collect();
            Ok(lines.join("\n"))
        }
        _ => Ok(payload.to_string()),
    }
}

/// `text`, read from `path`, as a JSON array of rows or a single object.
pub(crate) fn toml_payload_value(path: &Path, text: &str) -> Result<JsonValue> {
    let source = text.strip_prefix('\u{feff}').unwrap_or(text);
    let doc =
        DeTable::parse(source).map_err(|e| refuse(path, format!("{}: {e}", path.display())))?;
    let doc = doc.get_ref();
    match doc.get(ROWS_KEY).map(Spanned::get_ref) {
        Some(DeValue::Array(rows)) => {
            let mut stray: Vec<String> = doc
                .keys()
                .map(|k| k.get_ref().to_string())
                .filter(|k| k != ROWS_KEY)
                .collect();
            if !stray.is_empty() {
                stray.sort();
                let named: Vec<String> = stray.iter().map(|k| format!("`{k}`")).collect();
                return Err(refuse(
                    path,
                    format!(
                        "{}: top-level key(s) {} beside the `[[{ROWS_KEY}]]` rows; `{ROWS_KEY}` is \
                         reserved for rows, so put shared values in each row or pass them with \
                         --defaults-json / --set",
                        path.display(),
                        named.join(", ")
                    ),
                ));
            }
            let mut out = Vec::with_capacity(rows.len());
            for (i, row) in rows.iter().enumerate() {
                let DeValue::Table(table) = row.get_ref() else {
                    return Err(refuse(
                        path,
                        format!(
                            "{}: {ROWS_KEY} {} (line {}): expected a table, found {}",
                            path.display(),
                            i + 1,
                            line_of(source, row.span().start),
                            kind_name(row.get_ref())
                        ),
                    ));
                };
                out.push(table_to_json(path, source, table)?);
            }
            Ok(JsonValue::Array(out))
        }
        _ => table_to_json(path, source, doc),
    }
}

fn table_to_json(path: &Path, source: &str, table: &DeTable<'_>) -> Result<JsonValue> {
    let mut json = crate::convert::detable_to_json(table)
        .map_err(|e| refuse(path, format!("{}: {e:#}", path.display())))?;
    normalise_table(source, table, &mut json);
    Ok(json)
}

fn normalise_table(source: &str, table: &DeTable<'_>, json: &mut JsonValue) {
    for (key, value) in table.iter() {
        if let Some(slot) = json.get_mut(key.get_ref().as_ref() as &str) {
            normalise_value(source, value, slot);
        }
    }
}

fn normalise_value(source: &str, value: &Spanned<DeValue<'_>>, json: &mut JsonValue) {
    match value.get_ref() {
        DeValue::String(_) if is_multi_line(source, value.span().start) => {
            if let JsonValue::String(s) = json {
                *s = trim_multi_line(s);
            }
        }
        DeValue::Array(items) => {
            if let JsonValue::Array(slots) = json {
                for (item, slot) in items.iter().zip(slots.iter_mut()) {
                    normalise_value(source, item, slot);
                }
            }
        }
        DeValue::Table(table) => normalise_table(source, table, json),
        _ => {}
    }
}

/// A string value's span covers its raw token, delimiters included.
fn is_multi_line(source: &str, start: usize) -> bool {
    source
        .get(start..)
        .is_some_and(|raw| raw.starts_with("'''") || raw.starts_with("\"\"\""))
}

fn trim_multi_line(s: &str) -> String {
    let mut out = s.replace("\r\n", "\n");
    if out.ends_with('\n') {
        out.pop();
    }
    out
}

fn line_of(source: &str, offset: usize) -> usize {
    let end = offset.min(source.len());
    source.as_bytes()[..end]
        .iter()
        .filter(|&&b| b == b'\n')
        .count()
        + 1
}

fn kind_name(value: &DeValue<'_>) -> &'static str {
    match value {
        DeValue::String(_) => "a string",
        DeValue::Integer(_) => "an integer",
        DeValue::Float(_) => "a float",
        DeValue::Boolean(_) => "a boolean",
        DeValue::Datetime(_) => "a datetime",
        DeValue::Array(_) => "an array",
        DeValue::Table(_) => "a table",
    }
}

fn refuse(path: &Path, msg: String) -> anyhow::Error {
    tagged_err(ErrorKind::Validation, Some(path.to_path_buf()), msg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::errors::TaggedError;
    use serde_json::json;

    fn path() -> &'static Path {
        Path::new("payload.toml")
    }

    fn lines(text: &str) -> Vec<JsonValue> {
        let out = toml_payload(path(), text, true).unwrap();
        out.lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    fn refusal(text: &str) -> (String, String) {
        let err = toml_payload(path(), text, true).unwrap_err();
        let kind = err
            .downcast_ref::<TaggedError>()
            .map(|t| t.kind.as_str().to_string())
            .unwrap_or_default();
        (kind, format!("{err:#}"))
    }

    #[test]
    fn record_rows_become_one_ndjson_line_each() {
        let rows = lines("[[record]]\nid = 1\nname = \"a\"\n\n[[record]]\nid = 2\nname = \"b\"\n");
        assert_eq!(
            rows,
            vec![json!({"id": 1, "name": "a"}), json!({"id": 2, "name": "b"})]
        );
    }

    #[test]
    fn record_rows_become_a_json_array_for_a_json_flag() {
        let out = toml_payload(path(), "[[record]]\nid = 1\n[[record]]\nid = 2\n", false).unwrap();
        assert_eq!(out, r#"[{"id":1},{"id":2}]"#);
    }

    #[test]
    fn a_plain_table_is_one_object() {
        let text = "a = 1\nb = \"x\"\n[sub]\nc = true\n";
        assert_eq!(
            lines(text),
            vec![json!({"a": 1, "b": "x", "sub": {"c": true}})]
        );
        assert_eq!(
            toml_payload(path(), text, false).unwrap(),
            r#"{"a":1,"b":"x","sub":{"c":true}}"#
        );
    }

    #[test]
    fn literal_prose_round_trips_less_one_trailing_newline() {
        let text = "[[record]]\nrationale = '''\nSend `commitId` by \"generationId\" with $HOME\nsecond line\n'''\n";
        assert_eq!(
            lines(text)[0]["rationale"],
            json!("Send `commitId` by \"generationId\" with $HOME\nsecond line")
        );
    }

    #[test]
    fn multi_line_strings_are_trimmed_inside_arrays_and_tables() {
        let text = "notes = ['''\nfirst\n''', \"plain\\n\"]\n[sub]\nbody = \"\"\"\nbasic\n\"\"\"\n";
        assert_eq!(
            lines(text)[0],
            json!({"notes": ["first", "plain\n"], "sub": {"body": "basic"}})
        );
    }

    #[test]
    fn a_single_line_string_keeps_its_escaped_newline() {
        assert_eq!(lines("s = \"ends\\n\"\n")[0]["s"], json!("ends\n"));
    }

    #[test]
    fn crlf_input_reads_as_lf() {
        let text = "[[record]]\r\nbody = '''\r\none\r\ntwo\r\n'''\r\n";
        assert_eq!(lines(text)[0]["body"], json!("one\ntwo"));
    }

    #[test]
    fn a_key_beside_record_rows_is_refused_by_name() {
        let (kind, msg) = refusal("shared = 1\n[[record]]\na = 1\n");
        assert_eq!(kind, "validation");
        assert!(msg.contains("`shared`"), "{msg}");
        assert!(msg.contains("--defaults-json"), "{msg}");
    }

    #[test]
    fn a_syntax_error_carries_line_and_column() {
        let (kind, msg) = refusal("a = 1\nb = \n");
        assert_eq!(kind, "validation");
        assert!(msg.starts_with("payload.toml: "), "{msg}");
        assert!(msg.contains("line 2"), "{msg}");
        assert!(msg.contains("column"), "{msg}");
    }

    #[test]
    fn a_non_table_row_names_its_record_and_line() {
        let (kind, msg) = refusal("record = [\n  { a = 1 },\n  2,\n]\n");
        assert_eq!(kind, "validation");
        assert!(msg.contains("record 2 (line 3)"), "{msg}");
    }
}
