//! Parses the sources a research note cites: the `toml sources` block that
//! follows its Verdict line when it carries one, and otherwise each
//! `path:line` or `path:line-line` reference in its prose.
//!
//! Nothing here reads a file; whether an inline path exists is the caller's
//! predicate. Every returned path is repo-relative with `/` separators, never
//! absolute and never holding a `..` segment.

use std::collections::HashSet;
use std::sync::OnceLock;

use regex::Regex;
use serde_json::{Map, Value as JsonValue, json};
use toml::Value as TomlValue;

const INFO_STRING: &str = "toml sources";

/// One cited source. `origin` is `"block"` or `"inline"`; an inline source
/// has an empty `claim`, `url` and `read_at` and `dirty = false`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Source {
    pub(crate) path: String,
    pub(crate) line: Option<u32>,
    pub(crate) claim: String,
    pub(crate) url: String,
    pub(crate) read_at: String,
    pub(crate) dirty: bool,
    pub(crate) origin: &'static str,
}

impl Source {
    /// The listing's object; `line` is absent when the source names none.
    pub(crate) fn to_json(&self) -> JsonValue {
        let mut map = Map::new();
        map.insert("path".into(), json!(self.path));
        if let Some(line) = self.line {
            map.insert("line".into(), json!(line));
        }
        map.insert("claim".into(), json!(self.claim));
        map.insert("url".into(), json!(self.url));
        map.insert("read_at".into(), json!(self.read_at));
        map.insert("dirty".into(), json!(self.dirty));
        map.insert("origin".into(), json!(self.origin));
        JsonValue::Object(map)
    }
}

/// A note's sources, with `reason` set when a `toml sources` block was found
/// but could not be read, in which case `sources` are the inline references.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct NoteSources {
    pub(crate) sources: Vec<Source>,
    pub(crate) reason: Option<String>,
}

pub(crate) fn parse_note_sources(text: &str, exists: impl Fn(&str) -> bool) -> NoteSources {
    match find_block(text) {
        None => NoteSources {
            sources: inline_sources(text, &exists),
            reason: None,
        },
        Some(Ok(body)) => match block_sources(&body) {
            Ok(sources) => NoteSources {
                sources,
                reason: None,
            },
            Err(reason) => NoteSources {
                sources: inline_sources(text, &exists),
                reason: Some(reason),
            },
        },
        Some(Err(reason)) => NoteSources {
            sources: inline_sources(text, &exists),
            reason: Some(reason),
        },
    }
}

/// The body of the first fence whose info string is `toml sources`; `Err`
/// when that fence is never closed.
fn find_block(text: &str) -> Option<Result<String, String>> {
    let mut lines = text.lines().map(|l| l.trim_end_matches('\r'));
    let fence_len = loop {
        let line = lines.next()?;
        let ticks = line.len() - line.trim_start_matches('`').len();
        if ticks >= 3 && line[ticks..].trim() == INFO_STRING {
            break ticks;
        }
    };
    let mut body = String::new();
    for line in lines {
        let trimmed = line.trim_end();
        let ticks = trimmed.len() - trimmed.trim_start_matches('`').len();
        if ticks >= fence_len && ticks == trimmed.len() {
            return Some(Ok(body));
        }
        body.push_str(line);
        body.push('\n');
    }
    Some(Err(format!("the `{INFO_STRING}` block is never closed")))
}

fn block_sources(body: &str) -> Result<Vec<Source>, String> {
    let doc: TomlValue = toml::from_str(body).map_err(|e| {
        format!(
            "the `{INFO_STRING}` block is not valid TOML: {}",
            e.message()
        )
    })?;
    let read_at = match doc.get("read_at") {
        None => String::new(),
        Some(TomlValue::String(s)) => s.clone(),
        Some(_) => return Err("`read_at` is not a string".into()),
    };
    let dirty: HashSet<String> = match doc.get("dirty") {
        None => HashSet::new(),
        Some(TomlValue::Array(items)) => items
            .iter()
            .map(|item| match item {
                TomlValue::String(s) => Ok(normalise(s)),
                _ => Err("`dirty` holds a value that is not a string".to_string()),
            })
            .collect::<Result<_, _>>()?,
        Some(_) => return Err("`dirty` is not an array".into()),
    };
    let entries = match doc.get("source") {
        None => return Ok(Vec::new()),
        Some(TomlValue::Array(entries)) => entries,
        Some(_) => return Err("`source` is not an array of tables".into()),
    };
    entries
        .iter()
        .enumerate()
        .map(|(i, entry)| block_source(i + 1, entry, &read_at, &dirty))
        .collect()
}

fn block_source(
    n: usize,
    entry: &TomlValue,
    read_at: &str,
    dirty: &HashSet<String>,
) -> Result<Source, String> {
    let table = entry
        .as_table()
        .ok_or_else(|| format!("source {n} is not a table"))?;
    let string = |key: &str| -> Result<String, String> {
        match table.get(key) {
            None => Ok(String::new()),
            Some(TomlValue::String(s)) => Ok(s.clone()),
            Some(_) => Err(format!("source {n}: `{key}` is not a string")),
        }
    };
    let path = normalise(&string("path")?);
    let url = string("url")?;
    match (path.is_empty(), url.is_empty()) {
        (true, true) => return Err(format!("source {n} has neither `path` nor `url`")),
        (false, false) => return Err(format!("source {n} has both `path` and `url`")),
        _ => {}
    }
    if !path.is_empty() && !is_safe_relative(&path) {
        return Err(format!("source {n}: `{path}` is not a repo-relative path"));
    }
    let line = match table.get("line") {
        None => None,
        Some(TomlValue::Integer(v)) => Some(
            u32::try_from(*v)
                .ok()
                .filter(|v| *v > 0)
                .ok_or_else(|| format!("source {n}: `line` {v} is not a line number"))?,
        ),
        Some(_) => return Err(format!("source {n}: `line` is not an integer")),
    };
    let dirty = !path.is_empty() && dirty.contains(&path);
    Ok(Source {
        path,
        line,
        claim: string("claim")?,
        url,
        read_at: read_at.to_string(),
        dirty,
        origin: "block",
    })
}

fn inline_sources(text: &str, exists: &impl Fn(&str) -> bool) -> Vec<Source> {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    // `[0-9]` rather than `\d`: line numbers are ASCII digits.
    let pattern = PATTERN.get_or_init(|| {
        Regex::new(r"([A-Za-z0-9_.\-/\\]+\.[A-Za-z0-9]+):([0-9]+)(?:-[0-9]+)?")
            .expect("inline source pattern compiles")
    });
    let mut seen = HashSet::new();
    let mut sources = Vec::new();
    for caps in pattern.captures_iter(text) {
        let path = normalise(&caps[1]);
        let Some(line) = caps[2].parse::<u32>().ok().filter(|v| *v > 0) else {
            continue;
        };
        if !path.contains('/') || !is_safe_relative(&path) || !exists(&path) {
            continue;
        }
        if seen.insert((path.clone(), line)) {
            sources.push(Source {
                path,
                line: Some(line),
                claim: String::new(),
                url: String::new(),
                read_at: String::new(),
                dirty: false,
                origin: "inline",
            });
        }
    }
    sources
}

/// `\` to `/`, and a leading `./` dropped.
fn normalise(path: &str) -> String {
    let path = path.trim().replace('\\', "/");
    let mut rest = path.as_str();
    while let Some(stripped) = rest.strip_prefix("./") {
        rest = stripped;
    }
    rest.to_string()
}

fn is_safe_relative(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains(':')
        && path
            .split('/')
            .all(|seg| !seg.is_empty() && seg != "." && seg != "..")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn any(_: &str) -> bool {
        true
    }

    fn inline(path: &str, line: u32) -> Source {
        Source {
            path: path.into(),
            line: Some(line),
            claim: String::new(),
            url: String::new(),
            read_at: String::new(),
            dirty: false,
            origin: "inline",
        }
    }

    const BLOCK_NOTE: &str = "Verdict: the cursor stays put\n\n\
```toml sources\n\
read_at = \"207a03a1\"\n\
dirty = [\"docs/ideas/v2-pipeline.md\"]\n\
\n\
[[source]]\n\
path = \"glimpse/src/app.rs\"\n\
line = 902\n\
claim = \"SwitchSurface leaves the cursor\"\n\
\n\
[[source]]\n\
path = \"docs/ideas/v2-pipeline.md\"\n\
\n\
[[source]]\n\
url = \"https://docs.rs/pulldown-cmark/\"\n\
claim = \"tables need ENABLE_TABLES\"\n\
```\n\
\n\
Prose citing glimpse/src/other.rs:4 is ignored when the block is read.\n";

    #[test]
    fn a_block_yields_path_and_url_sources() {
        let parsed = parse_note_sources(BLOCK_NOTE, any);
        assert_eq!(parsed.reason, None);
        assert_eq!(parsed.sources.len(), 3);
        assert_eq!(
            parsed.sources[0],
            Source {
                path: "glimpse/src/app.rs".into(),
                line: Some(902),
                claim: "SwitchSurface leaves the cursor".into(),
                url: String::new(),
                read_at: "207a03a1".into(),
                dirty: false,
                origin: "block",
            }
        );
        assert_eq!(parsed.sources[1].line, None);
        assert!(parsed.sources[1].dirty);
        assert_eq!(parsed.sources[2].path, "");
        assert_eq!(parsed.sources[2].url, "https://docs.rs/pulldown-cmark/");
        assert!(!parsed.sources[2].dirty);
        assert_eq!(parsed.sources[2].read_at, "207a03a1");
    }

    #[test]
    fn a_source_without_a_line_omits_it_from_json() {
        let parsed = parse_note_sources(BLOCK_NOTE, any);
        let json = parsed.sources[1].to_json();
        assert!(json.get("line").is_none());
        assert_eq!(json["origin"], "block");
        assert_eq!(parsed.sources[0].to_json()["line"], 902);
    }

    #[test]
    fn inline_references_in_backticks_and_tables_are_found() {
        let text = "Verdict: ok\n\nSee `glimpse/src/surface.rs:16` and\n\n\
| a | b |\n|---|---|\n| tomlctl/src/plan/mod.rs:40-52 | x |\n";
        let parsed = parse_note_sources(text, any);
        assert_eq!(parsed.reason, None);
        assert_eq!(
            parsed.sources,
            vec![
                inline("glimpse/src/surface.rs", 16),
                inline("tomlctl/src/plan/mod.rs", 40)
            ]
        );
    }

    #[test]
    fn duplicates_are_listed_once_in_first_seen_order() {
        let text = "b/y.rs:2 then a/x.rs:1 then b/y.rs:2 and b/y.rs:2-9, a/x.rs:3\n";
        let parsed = parse_note_sources(text, any);
        assert_eq!(
            parsed.sources,
            vec![
                inline("b/y.rs", 2),
                inline("a/x.rs", 1),
                inline("a/x.rs", 3)
            ]
        );
    }

    #[test]
    fn a_windows_style_path_is_normalised() {
        let parsed = parse_note_sources("at glimpse\\src\\app.rs:12\n", any);
        assert_eq!(parsed.sources, vec![inline("glimpse/src/app.rs", 12)]);
    }

    #[test]
    fn a_rejected_or_unsafe_path_is_dropped() {
        let text = "gone/file.rs:3 kept/file.rs:4 ../up/x.rs:5 /abs/x.rs:6 \
C:\\Users\\x\\y.rs:7 https://host.example/a/b.rs:8 noslash.rs:9\n";
        let parsed = parse_note_sources(text, |p| p != "gone/file.rs");
        assert_eq!(parsed.sources, vec![inline("kept/file.rs", 4)]);
    }

    #[test]
    fn a_malformed_block_yields_inline_sources_and_a_reason() {
        let text = "Verdict: x\n\n```toml sources\n[[source]]\npath = \"a/b.rs\"\n\
url = \"https://x\"\n```\n\nsee a/c.rs:3\n";
        let parsed = parse_note_sources(text, any);
        assert!(parsed.reason.as_deref().unwrap().contains("both"));
        assert_eq!(parsed.sources, vec![inline("a/c.rs", 3)]);

        let unclosed = "Verdict: x\n\n```toml sources\nread_at = \"1\"\nsee a/c.rs:3\n";
        let parsed = parse_note_sources(unclosed, any);
        assert!(parsed.reason.is_some());
        assert_eq!(parsed.sources, vec![inline("a/c.rs", 3)]);

        let bad_toml = "```toml sources\nread_at = \n```\nsee a/c.rs:3\n";
        assert!(parse_note_sources(bad_toml, any).reason.is_some());

        let escaping = "```toml sources\n[[source]]\npath = \"../x.rs\"\n```\n";
        assert!(parse_note_sources(escaping, any).reason.is_some());
    }

    #[test]
    fn a_crlf_block_is_read() {
        let text = BLOCK_NOTE.replace('\n', "\r\n");
        let parsed = parse_note_sources(&text, any);
        assert_eq!(parsed.reason, None);
        assert_eq!(parsed.sources.len(), 3);
        assert_eq!(parsed.sources[0].claim, "SwitchSurface leaves the cursor");
    }
}
