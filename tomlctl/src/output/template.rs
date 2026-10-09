//! The `--template` mini-language: literal text with `{path}` placeholders,
//! `{{` and `}}` for literal braces. A path uses the dotted syntax of
//! `convert::navigate_json`. Strings render bare, numbers and bools as literals,
//! arrays and objects as compact JSON, and null or a missing path as nothing.
//! A path with a `*` segment renders its matches comma-joined. `{path:N}`
//! truncates the rendered text to N characters with `truncate_text`.

use std::borrow::Cow;

use anyhow::Result;
use serde_json::Value as JsonValue;

use crate::convert::{is_wildcard_path, navigate_json, navigate_json_all};
use crate::errors::{ErrorKind, tagged_err};

#[derive(Debug, PartialEq)]
enum Segment {
    Literal(String),
    Placeholder { path: String, width: Option<usize> },
}

/// A parsed template, ready to render once per row.
#[derive(Debug)]
pub(crate) struct Template {
    segments: Vec<Segment>,
}

fn parse_err(msg: String) -> anyhow::Error {
    tagged_err(ErrorKind::Validation, None, format!("--template: {msg}"))
}

impl Template {
    /// Columns in errors are 1-based character positions.
    pub(crate) fn parse(src: &str) -> Result<Self> {
        let mut segments = Vec::new();
        let mut literal = String::new();
        let mut chars = src.chars().enumerate().peekable();
        while let Some((col, c)) = chars.next() {
            match c {
                '{' if chars.peek().is_some_and(|&(_, n)| n == '{') => {
                    chars.next();
                    literal.push('{');
                }
                '}' if chars.peek().is_some_and(|&(_, n)| n == '}') => {
                    chars.next();
                    literal.push('}');
                }
                '{' => {
                    let mut path = String::new();
                    let mut closed = false;
                    for (_, p) in chars.by_ref() {
                        if p == '}' {
                            closed = true;
                            break;
                        }
                        path.push(p);
                    }
                    if !closed {
                        return Err(parse_err(format!("unclosed `{{` at column {}", col + 1)));
                    }
                    if path.is_empty() {
                        return Err(parse_err(format!(
                            "empty placeholder `{{}}` at column {}",
                            col + 1
                        )));
                    }
                    if path.contains('{') {
                        return Err(parse_err(format!("unclosed `{{` at column {}", col + 1)));
                    }
                    let (path, width) = split_width(path, col + 1)?;
                    if !literal.is_empty() {
                        segments.push(Segment::Literal(std::mem::take(&mut literal)));
                    }
                    segments.push(Segment::Placeholder { path, width });
                }
                '}' => {
                    return Err(parse_err(format!(
                        "unmatched `}}` at column {} (write `}}}}` for a literal brace)",
                        col + 1
                    )));
                }
                c => literal.push(c),
            }
        }
        if !literal.is_empty() {
            segments.push(Segment::Literal(literal));
        }
        Ok(Self { segments })
    }

    /// The placeholder paths, in template order.
    pub(crate) fn paths(&self) -> impl Iterator<Item = &str> {
        self.segments.iter().filter_map(|s| match s {
            Segment::Placeholder { path, .. } => Some(path.as_str()),
            Segment::Literal(_) => None,
        })
    }

    pub(crate) fn render(&self, row: &JsonValue) -> String {
        let mut out = String::new();
        for seg in &self.segments {
            match seg {
                Segment::Literal(s) => out.push_str(s),
                Segment::Placeholder { path, width } => {
                    let text = placeholder_text(row, path);
                    match width {
                        Some(n) => out.push_str(&truncate_text(&text, *n)),
                        None => out.push_str(&text),
                    }
                }
            }
        }
        out
    }
}

/// Split a placeholder body on its last `:` into path and width. `col` is the
/// placeholder's 1-based column, named in the error for a non-numeric width.
fn split_width(body: String, col: usize) -> Result<(String, Option<usize>)> {
    let Some((path, w)) = body.rsplit_once(':') else {
        return Ok((body, None));
    };
    let width = w.parse::<usize>().map_err(|_| {
        parse_err(format!(
            "invalid width `{w}` in placeholder at column {col} (expected `{{path:N}}`)"
        ))
    })?;
    if path.is_empty() {
        return Err(parse_err(format!("empty placeholder path at column {col}")));
    }
    Ok((path.to_string(), Some(width)))
}

/// The rendered text of one placeholder: a wildcard path joins its matches
/// with `,`; any other path renders its value, or nothing when missing.
fn placeholder_text(row: &JsonValue, path: &str) -> String {
    if is_wildcard_path(path) {
        navigate_json_all(row, path)
            .into_iter()
            .map(value_text)
            .collect::<Vec<_>>()
            .join(",")
    } else {
        navigate_json(row, path).map(value_text).unwrap_or_default()
    }
}

/// `s` cut to its first `n` Unicode scalars, followed by `…(+K)` where K is
/// the number of scalars cut; `s` unchanged when it is no longer than `n`.
pub(crate) fn truncate_text(s: &str, n: usize) -> Cow<'_, str> {
    match s.char_indices().nth(n) {
        None => Cow::Borrowed(s),
        Some((cut, _)) => {
            let rest = s[cut..].chars().count();
            Cow::Owned(format!("{}…(+{rest})", &s[..cut]))
        }
    }
}

/// The text form of one value, shared by `--template` and `--get`.
pub(crate) fn value_text(v: &JsonValue) -> String {
    match v {
        JsonValue::String(s) => s.clone(),
        JsonValue::Null => String::new(),
        JsonValue::Number(n) => n.to_string(),
        JsonValue::Bool(b) => b.to_string(),
        JsonValue::Array(_) | JsonValue::Object(_) => v.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn render(t: &str, row: JsonValue) -> String {
        Template::parse(t).unwrap().render(&row)
    }

    #[test]
    fn doubled_braces_render_as_literal_braces() {
        assert_eq!(render("{{{id}}}", json!({ "id": 7 })), "{7}");
        assert_eq!(render("a {{b}} c", json!({})), "a {b} c");
    }

    #[test]
    fn values_render_by_type() {
        let row = json!({
            "s": "text", "n": 3, "f": 1.5, "b": true, "z": null,
            "a": [1, "x"], "o": { "k": 1 }
        });
        assert_eq!(
            render("{s}|{n}|{f}|{b}|{z}|{a}|{o}|{missing}", row),
            r#"text|3|1.5|true||[1,"x"]|{"k":1}|"#
        );
    }

    #[test]
    fn placeholders_take_dotted_and_indexed_paths() {
        let row = json!({ "policy": { "checkpoints": "milestones" }, "files": ["a.rs", "b.rs"] });
        assert_eq!(
            render("{policy.checkpoints} {files.1}", row),
            "milestones b.rs"
        );
    }

    #[test]
    fn paths_lists_placeholders_in_order() {
        let t = Template::parse("{a}-{b.c}{{x}}").unwrap();
        assert_eq!(t.paths().collect::<Vec<_>>(), ["a", "b.c"]);
    }

    #[test]
    fn unclosed_brace_names_its_column() {
        let err = Template::parse("ab {id").unwrap_err();
        assert_eq!(format!("{err:#}"), "--template: unclosed `{` at column 4");
        let err = Template::parse("{a{b}").unwrap_err();
        assert_eq!(format!("{err:#}"), "--template: unclosed `{` at column 1");
    }

    #[test]
    fn stray_close_and_empty_placeholder_are_refused() {
        let err = Template::parse("a}b").unwrap_err();
        assert!(format!("{err:#}").contains("unmatched `}` at column 2"));
        let err = Template::parse("x{}").unwrap_err();
        assert!(format!("{err:#}").contains("empty placeholder `{}` at column 2"));
    }

    #[test]
    fn width_truncates_with_a_count_of_the_cut() {
        assert_eq!(
            render("{detail:5}", json!({ "detail": "abcdefgh" })),
            "abcde…(+3)"
        );
        assert_eq!(render("{n:1}", json!({ "n": 1234 })), "1…(+3)");
    }

    #[test]
    fn width_leaves_short_and_missing_values_alone() {
        assert_eq!(render("{s:5}", json!({ "s": "abcde" })), "abcde");
        assert_eq!(render("[{gone:3}]", json!({})), "[]");
    }

    #[test]
    fn width_counts_unicode_scalars() {
        assert_eq!(render("{s:2}", json!({ "s": "éàü" })), "éà…(+1)");
        assert_eq!(truncate_text("日本語テキスト", 3), "日本語…(+4)");
    }

    #[test]
    fn width_zero_keeps_only_the_marker() {
        assert_eq!(truncate_text("abc", 0), "…(+3)");
        assert_eq!(truncate_text("", 0), "");
    }

    #[test]
    fn width_returns_borrowed_text_when_nothing_is_cut() {
        assert!(matches!(truncate_text("abc", 3), Cow::Borrowed("abc")));
    }

    #[test]
    fn width_paths_list_the_bare_path() {
        let t = Template::parse("{a.b:4} {c}").unwrap();
        assert_eq!(t.paths().collect::<Vec<_>>(), ["a.b", "c"]);
    }

    #[test]
    fn width_non_numeric_is_a_parse_error_naming_the_column() {
        let err = Template::parse("ab {id:x}").unwrap_err();
        assert_eq!(
            format!("{err:#}"),
            "--template: invalid width `x` in placeholder at column 4 (expected `{path:N}`)"
        );
        let err = Template::parse("{id:}").unwrap_err();
        assert!(format!("{err:#}").contains("invalid width `` in placeholder at column 1"));
        let err = Template::parse("{id:-1}").unwrap_err();
        assert!(format!("{err:#}").contains("invalid width `-1`"));
        let tag = err.downcast_ref::<crate::errors::TaggedError>().unwrap();
        assert_eq!(tag.kind.as_str(), "validation");
    }

    #[test]
    fn width_without_a_path_is_refused() {
        let err = Template::parse("{:3}").unwrap_err();
        assert!(format!("{err:#}").contains("empty placeholder path at column 1"));
    }

    #[test]
    fn wildcard_placeholder_joins_matches_with_commas() {
        let row = json!({ "deps": [{ "ref": "a" }, { "ref": "b" }, { "id": 3 }] });
        assert_eq!(render("deps={deps.*.ref}", row), "deps=a,b");
    }

    #[test]
    fn wildcard_placeholder_renders_values_by_type() {
        let row = json!({ "m": { "x": 1, "y": [2], "z": "s" } });
        assert_eq!(render("{m.*}", row), "1,[2],s");
    }

    #[test]
    fn wildcard_placeholder_with_no_matches_renders_nothing() {
        assert_eq!(render("[{deps.*.ref}]", json!({ "deps": [] })), "[]");
        assert_eq!(render("[{deps.*.ref}]", json!({})), "[]");
    }

    #[test]
    fn wildcard_placeholder_width_truncates_the_joined_text() {
        let row = json!({ "files": ["a.rs", "b.rs", "c.rs"] });
        assert_eq!(render("{files.*:6}", row), "a.rs,b…(+8)");
    }

    #[test]
    fn parse_errors_are_tagged_validation() {
        let err = Template::parse("{").unwrap_err();
        let tag = err.downcast_ref::<crate::errors::TaggedError>().unwrap();
        assert_eq!(tag.kind.as_str(), "validation");
    }
}
