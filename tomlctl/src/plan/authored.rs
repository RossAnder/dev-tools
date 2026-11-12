//! Lists and reads the documents under `docs/flows/<slug>/`.
//!
//! Both entry points take the repo root explicitly and never consult the
//! process root, so a poller thread can call them without one set. A file
//! whose canonical path leaves the flow's directory, through a symlink at the
//! file or at any directory above it, is left out of the listing and refused
//! on read.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde_json::{Value as JsonValue, json};

use super::note_sources::parse_note_sources;
use super::run_note;

/// What a listed path is, by where it sits and how it is named.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Kind {
    Brief,
    Design(String),
    Note(String),
    Challenge,
    ColdRead(String),
    Backlog(String),
    RunNote(u32),
    Settle(String),
}

impl Kind {
    fn name(&self) -> &'static str {
        match self {
            Kind::Brief => "brief",
            Kind::Design(_) => "design",
            Kind::Note(_) => "note",
            Kind::Challenge => "challenge",
            Kind::ColdRead(_) => "cold-read",
            Kind::Backlog(_) => "backlog",
            Kind::RunNote(_) => "run-note",
            Kind::Settle(_) => "settle",
        }
    }

    fn id(&self) -> String {
        match self {
            Kind::Brief => "brief".into(),
            Kind::Challenge => "structure-challenge".into(),
            Kind::RunNote(n) => format!("task-{n}"),
            Kind::Design(id)
            | Kind::Note(id)
            | Kind::ColdRead(id)
            | Kind::Backlog(id)
            | Kind::Settle(id) => id.clone(),
        }
    }

    fn carries_sources(&self) -> bool {
        matches!(self, Kind::Note(_) | Kind::Challenge | Kind::ColdRead(_))
    }
}

/// The kind of a `/`-separated path relative to the flow's directory, `None`
/// for any path the listing leaves out.
fn classify(relative: &str) -> Option<Kind> {
    // A `\` is a separator on Windows and a `:` can name a drive or stream, so
    // either could carry a joined path out of the flow's directory.
    if relative.contains(['\\', ':']) {
        return None;
    }
    let parts: Vec<&str> = relative.split('/').collect();
    if parts
        .iter()
        .any(|p| p.is_empty() || *p == "." || *p == "..")
    {
        return None;
    }
    let stem = |name: &str| -> Option<String> {
        let stem = name.strip_suffix(".md")?;
        (!stem.is_empty()).then(|| stem.to_string())
    };
    match parts[..] {
        ["brief.md"] => Some(Kind::Brief),
        ["design", name] => stem(name).map(Kind::Design),
        ["notes", "run", name] => {
            let n = stem(name)?.strip_prefix("task-")?.to_string();
            if n.is_empty() || !n.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            n.parse().ok().map(Kind::RunNote)
        }
        ["notes", "settle", name] => stem(name).map(Kind::Settle),
        ["notes", name] => {
            let id = stem(name)?;
            if id == "structure-challenge" {
                return Some(Kind::Challenge);
            }
            for (prefix, make) in [
                ("cold-read-", Kind::ColdRead as fn(String) -> Kind),
                ("backlog-", Kind::Backlog),
            ] {
                if let Some(rest) = id.strip_prefix(prefix).filter(|r| !r.is_empty()) {
                    return Some(make(rest.to_string()));
                }
            }
            Some(Kind::Note(id))
        }
        _ => None,
    }
}

/// The flow's directory under the canonical root: the boundary every listed
/// file must resolve inside.
fn anchor(root: &Path, slug: &str) -> Result<PathBuf> {
    crate::flow::validate_slug(slug)?;
    Ok(root
        .canonicalize()
        .with_context(|| format!("resolving {}", root.display()))?
        .join("docs")
        .join("flows")
        .join(slug))
}

/// The real path of `relative` under `anchor`, `None` when it is absent, not
/// a file, or resolves outside `anchor`.
fn resolve(anchor: &Path, relative: &str) -> Result<Option<PathBuf>> {
    let path = anchor.join(relative);
    let resolved = match path.canonicalize() {
        Ok(resolved) => resolved,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err).with_context(|| format!("resolving {}", path.display())),
    };
    if !resolved.starts_with(anchor) || !resolved.is_file() {
        return Ok(None);
    }
    Ok(Some(resolved))
}

/// The `.md` file names directly inside `dir`, empty when it does not exist.
fn md_names(dir: &Path) -> Result<Vec<String>> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err).with_context(|| format!("listing {}", dir.display())),
    };
    let mut names = Vec::new();
    for entry in entries {
        let entry = entry.with_context(|| format!("listing {}", dir.display()))?;
        if let Ok(name) = entry.file_name().into_string()
            && name.ends_with(".md")
        {
            names.push(name);
        }
    }
    Ok(names)
}

fn read_text(path: &Path) -> Result<String> {
    let bytes = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    Ok(String::from_utf8_lossy(&bytes).replace("\r\n", "\n"))
}

fn first_line(text: &str) -> &str {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    text.lines().next().unwrap_or_default().trim()
}

fn first_non_blank(text: &str) -> &str {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    text.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or_default()
}

/// `(headline, state, task)` for a file of `kind` holding `text`.
fn headline(kind: &Kind, text: &str) -> (String, String, u32) {
    let line = first_line(text);
    match kind {
        Kind::Brief | Kind::Design(_) => {
            let title = line.strip_prefix("# ").unwrap_or(line).trim();
            (title.to_string(), String::new(), 0)
        }
        Kind::Note(_) | Kind::Challenge | Kind::ColdRead(_) => {
            let verdict = line.strip_prefix("Verdict:").unwrap_or(line).trim();
            (verdict.to_string(), String::new(), 0)
        }
        Kind::Backlog(_) => (first_non_blank(text).to_string(), String::new(), 0),
        Kind::Settle(_) => (line.to_string(), String::new(), 0),
        Kind::RunNote(n) => match run_note::parse(text) {
            Ok(note) => (note.summary, note.status, *n),
            Err(_) => (line.to_string(), String::new(), *n),
        },
    }
}

/// The relative paths the listing could carry, before resolution.
fn candidates(anchor: &Path) -> Result<Vec<String>> {
    let mut paths = vec!["brief.md".to_string()];
    for sub in ["design", "notes", "notes/run", "notes/settle"] {
        for name in md_names(&anchor.join(sub))? {
            paths.push(format!("{sub}/{name}"));
        }
    }
    Ok(paths)
}

/// `{"files": [...]}` for every brief, design file, note, run note and settle
/// note of the flow, sorted by `path`. A missing directory lists no files.
pub(crate) fn list(root: &Path, slug: &str) -> Result<JsonValue> {
    let anchor = anchor(root, slug)?;
    let mut files = Vec::new();
    for relative in candidates(&anchor)? {
        let Some(kind) = classify(&relative) else {
            continue;
        };
        let Some(resolved) = resolve(&anchor, &relative)? else {
            continue;
        };
        let text = read_text(&resolved)?;
        let (headline, state, task) = headline(&kind, &text);
        let sources: Vec<JsonValue> = if kind.carries_sources() {
            parse_note_sources(&text, |p| root.join(p).exists())
                .sources
                .iter()
                .map(|s| s.to_json())
                .collect()
        } else {
            Vec::new()
        };
        files.push(json!({
            "path": relative,
            "kind": kind.name(),
            "id": kind.id(),
            "headline": headline,
            "state": state,
            "task": task,
            "sources": sources,
        }));
    }
    files.sort_by(|a, b| a["path"].as_str().cmp(&b["path"].as_str()));
    Ok(json!({ "files": files }))
}

/// `{"path", "text"}` for one path the listing would carry, the text
/// LF-normalised. Any other path is refused.
pub(crate) fn read(root: &Path, slug: &str, path: &str) -> Result<JsonValue> {
    let anchor = anchor(root, slug)?;
    if classify(path).is_none() {
        bail!("`{path}` is not an authored file of flow `{slug}`");
    }
    let Some(resolved) = resolve(&anchor, path)? else {
        bail!("`{path}` is absent from flow `{slug}` or resolves outside its directory");
    };
    Ok(json!({ "path": path, "text": read_text(&resolved)? }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_classify_by_place_and_name() {
        assert_eq!(classify("brief.md"), Some(Kind::Brief));
        assert_eq!(
            classify("design/records.md"),
            Some(Kind::Design("records".into()))
        );
        assert_eq!(
            classify("notes/structure-challenge.md"),
            Some(Kind::Challenge)
        );
        assert_eq!(
            classify("notes/cold-read-A.md"),
            Some(Kind::ColdRead("A".into()))
        );
        assert_eq!(
            classify("notes/backlog-B12.md"),
            Some(Kind::Backlog("B12".into()))
        );
        assert_eq!(
            classify("notes/surface-nav.md"),
            Some(Kind::Note("surface-nav".into()))
        );
        assert_eq!(classify("notes/run/task-7.md"), Some(Kind::RunNote(7)));
        assert_eq!(
            classify("notes/settle/F3.md"),
            Some(Kind::Settle("F3".into()))
        );
        for refused in [
            "notes/run/task-.md",
            "notes/run/task-x.md",
            "notes/run/other.md",
            "notes/a/b.md",
            "design/x.txt",
            "../brief.md",
            "notes/../brief.md",
            "brief.txt",
            "notes/.md",
            "",
        ] {
            assert_eq!(classify(refused), None, "{refused}");
        }
    }

    #[test]
    fn a_run_note_without_a_status_line_keeps_its_first_line() {
        let (headline, state, task) = headline(&Kind::RunNote(3), "# not a status\n");
        assert_eq!(
            (headline.as_str(), state.as_str(), task),
            ("# not a status", "", 3)
        );
    }
}
