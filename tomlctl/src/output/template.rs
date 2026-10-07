//! The `--template` mini-language: literal text with `{path}` placeholders,
//! `{{` and `}}` for literal braces. A path uses the dotted syntax of
//! `convert::navigate_json`. Strings render bare, numbers and bools as literals,
//! arrays and objects as compact JSON, and null or a missing path as nothing.

use anyhow::Result;
use serde_json::Value as JsonValue;

use crate::convert::navigate_json;
use crate::errors::{ErrorKind, tagged_err};

#[derive(Debug, PartialEq)]
enum Segment {
    Literal(String),
    Placeholder(String),
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
                    if !literal.is_empty() {
                        segments.push(Segment::Literal(std::mem::take(&mut literal)));
                    }
                    segments.push(Segment::Placeholder(path));
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
            Segment::Placeholder(p) => Some(p.as_str()),
            Segment::Literal(_) => None,
        })
    }

    pub(crate) fn render(&self, row: &JsonValue) -> String {
        let mut out = String::new();
        for seg in &self.segments {
            match seg {
                Segment::Literal(s) => out.push_str(s),
                Segment::Placeholder(p) => {
                    if let Some(v) = navigate_json(row, p) {
                        out.push_str(&value_text(v));
                    }
                }
            }
        }
        out
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
    fn parse_errors_are_tagged_validation() {
        let err = Template::parse("{").unwrap_err();
        let tag = err.downcast_ref::<crate::errors::TaggedError>().unwrap();
        assert_eq!(tag.kind.as_str(), "validation");
    }
}
