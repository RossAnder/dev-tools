//! Section scanning and in-place section replacement over a plan document.
//!
//! Headings are matched on `^## ` outside fenced code blocks. Every line is
//! matched with one trailing `\r` stripped, so a CRLF plan scans as LF while
//! `body_range` keeps indexing the original bytes — splicing therefore
//! preserves every byte outside the replaced section.

use std::ops::Range;

#[derive(Debug, Clone)]
pub(crate) struct Section {
    pub(crate) title: String,
    /// Offset of the `## ` heading line itself, ahead of `body_range`.
    pub(crate) start: usize,
    /// Bytes after the heading line, up to the next `## ` heading or EOF.
    pub(crate) body_range: Range<usize>,
}

impl Section {
    /// Body with CRLF collapsed to LF, which is what the field parsers match on.
    pub(crate) fn body_lf(&self, src: &str) -> String {
        src[self.body_range.clone()].replace("\r\n", "\n")
    }

    /// 1-based line of the heading in `src`. The body opens on the next one,
    /// so `heading_line + 1` is the document line a body-relative parser's
    /// first line stands on.
    pub(crate) fn heading_line(&self, src: &str) -> usize {
        src[..self.start].matches('\n').count() + 1
    }
}

/// Fenced-block tracker for a line-by-line scan, so every scanner over a plan
/// agrees on which lines can be a heading.
#[derive(Debug, Default)]
pub(crate) struct FenceState {
    open: Option<(char, usize)>,
}

impl FenceState {
    /// Whether the line `consume` is about to read stands inside a block an
    /// earlier line opened, which is what separates an opening marker from the
    /// content and closing marker `consume` reports alike.
    pub(crate) fn is_open(&self) -> bool {
        self.open.is_some()
    }

    /// Advances over one line and reports whether it is fenced — true for the
    /// opening and closing markers too, neither of which is ever a heading.
    pub(crate) fn consume(&mut self, line: &str) -> bool {
        match self.open {
            Some((marker, width)) => {
                if closes_fence(line, marker, width) {
                    self.open = None;
                }
                true
            }
            None => match opens_fence(line) {
                Some(open) => {
                    self.open = Some(open);
                    true
                }
                None => false,
            },
        }
    }
}

pub(crate) fn sections(src: &str) -> Vec<Section> {
    let mut found: Vec<Section> = Vec::new();
    let mut fence = FenceState::default();
    let mut pos = 0usize;

    while pos < src.len() {
        let (content, next) = split_line(src, pos);

        if !fence.consume(content)
            && let Some(heading) = content.strip_prefix("## ")
        {
            if let Some(previous) = found.last_mut() {
                previous.body_range.end = pos;
            }
            found.push(Section {
                title: heading.trim().to_string(),
                start: pos,
                body_range: next..src.len(),
            });
        }

        pos = next;
    }

    found
}

/// Returns `src` unchanged when `title` names no section; callers wanting an
/// insert in that case check `sections` first.
pub(crate) fn replace_section(src: &str, title: &str, new_body: &str) -> String {
    let wanted = needle(title);
    let Some(section) = sections(src).into_iter().find(|s| s.title == wanted) else {
        return src.to_string();
    };

    let eol = dominant_line_ending(src);
    let mut out = String::with_capacity(src.len() + new_body.len());
    out.push_str(&src[..section.body_range.start]);
    out.push_str(&with_line_ending(new_body, eol));
    out.push_str(&src[section.body_range.end..]);
    out
}

/// Appends at EOF when `after_title` names no section.
pub(crate) fn insert_section_after(
    src: &str,
    after_title: &str,
    title: &str,
    body: &str,
) -> String {
    let wanted = needle(after_title);
    let at = sections(src)
        .into_iter()
        .find(|s| s.title == wanted)
        .map_or(src.len(), |s| s.body_range.end);
    splice_section(src, at, title, body)
}

/// The counterpart anchor, for a section whose canonical place is ahead of one
/// that may itself open the document. Appends at EOF when `before_title` names
/// no section.
pub(crate) fn insert_section_before(
    src: &str,
    before_title: &str,
    title: &str,
    body: &str,
) -> String {
    let wanted = needle(before_title);
    let at = sections(src)
        .into_iter()
        .find(|s| s.title == wanted)
        .map_or(src.len(), |s| s.start);
    splice_section(src, at, title, body)
}

fn splice_section(src: &str, at: usize, title: &str, body: &str) -> String {
    let eol = dominant_line_ending(src);
    let mut out = String::with_capacity(src.len() + body.len() + title.len() + 8);
    out.push_str(&src[..at]);
    if !out.is_empty() && !out.ends_with('\n') {
        out.push_str(eol);
    }
    out.push_str("## ");
    out.push_str(needle(title));
    out.push_str(eol);
    out.push_str(&with_line_ending(body, eol));
    out.push_str(&src[at..]);
    out
}

pub(crate) fn dominant_line_ending(src: &str) -> &'static str {
    let crlf = src.matches("\r\n").count();
    let bare_lf = src.matches('\n').count() - crlf;
    if crlf > bare_lf { "\r\n" } else { "\n" }
}

/// Content with its terminator and one trailing `\r` stripped, plus the start
/// offset of the next line.
fn split_line(src: &str, start: usize) -> (&str, usize) {
    let (end, next) = match src[start..].find('\n') {
        Some(offset) => (start + offset, start + offset + 1),
        None => (src.len(), src.len()),
    };
    let content = &src[start..end];
    (content.strip_suffix('\r').unwrap_or(content), next)
}

fn opens_fence(line: &str) -> Option<(char, usize)> {
    let trimmed = line.trim_start();
    let marker = trimmed.chars().next()?;
    if marker != '`' && marker != '~' {
        return None;
    }
    let width = trimmed.chars().take_while(|&c| c == marker).count();
    // A backtick fence's info string may not itself contain a backtick.
    if width < 3 || (marker == '`' && trimmed[width..].contains('`')) {
        return None;
    }
    Some((marker, width))
}

fn closes_fence(line: &str, marker: char, width: usize) -> bool {
    let trimmed = line.trim_start();
    let run = trimmed.chars().take_while(|&c| c == marker).count();
    run >= width && trimmed[run..].trim().is_empty()
}

fn needle(title: &str) -> &str {
    title.trim().trim_start_matches('#').trim()
}

/// Rewrites `body`'s line endings to `eol` and guarantees a trailing one, so
/// whatever follows the splice still starts its own line.
fn with_line_ending(body: &str, eol: &str) -> String {
    let lf = body.replace("\r\n", "\n");
    let mut out = if eol == "\r\n" {
        lf.replace('\n', "\r\n")
    } else {
        lf
    };
    if !out.is_empty() && !out.ends_with(eol) {
        out.push_str(eol);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const FENCED: &str =
        "# Plan\n\n## Alpha\n\n```toml\n## X = 1\nkey = 1\n```\n\n## Beta\n\ntail\n";

    fn titles(src: &str) -> Vec<String> {
        sections(src).into_iter().map(|s| s.title).collect()
    }

    #[test]
    fn heading_inside_a_fence_is_not_a_section() {
        assert_eq!(titles(FENCED), vec!["Alpha", "Beta"]);
    }

    #[test]
    fn a_heading_line_indexes_the_document_line_the_heading_stands_on() {
        for src in [FENCED.to_string(), FENCED.replace('\n', "\r\n")] {
            let beta = sections(&src)
                .into_iter()
                .find(|s| s.title == "Beta")
                .expect("Beta section");
            let line = beta.heading_line(&src);
            assert_eq!(src.lines().nth(line - 1), Some("## Beta"), "{src:?}");
            assert_eq!(
                src.lines().nth(line),
                Some(""),
                "the body opens on the line after the heading: {src:?}"
            );
        }
    }

    #[test]
    fn tilde_fence_ignores_a_backtick_line() {
        let src = "## A\n\n~~~\n## X\n```\n~~~\n\n## B\n";
        assert_eq!(titles(src), vec!["A", "B"]);
    }

    #[test]
    fn replacing_a_section_preserves_every_byte_around_it() {
        let src = "# Plan\n\n## Approach\n\nprose\n\n## Tasks\n\n### 1. Old\n\n## Risks\n\nrisk\n";
        let tasks = sections(src)
            .into_iter()
            .find(|s| s.title == "Tasks")
            .expect("Tasks section");
        let out = replace_section(src, "## Tasks", "\n### 1. New\n\n");

        assert_eq!(
            &out[..tasks.body_range.start],
            &src[..tasks.body_range.start]
        );
        let after = &src[tasks.body_range.end..];
        assert_eq!(&out[out.len() - after.len()..], after);
        assert_eq!(
            &out[tasks.body_range.start..out.len() - after.len()],
            "\n### 1. New\n\n"
        );
    }

    #[test]
    fn unknown_title_leaves_the_document_alone() {
        assert_eq!(replace_section(FENCED, "Tasks", "x"), FENCED);
    }

    #[test]
    fn lf_source_stays_free_of_carriage_returns() {
        let src = "# Plan\n\n## Tasks\n\nold\n\n## Risks\n\nrisk\n";
        assert_eq!(dominant_line_ending(src), "\n");
        assert!(!sections(src)[0].body_lf(src).contains('\r'));

        let out = replace_section(src, "Tasks", "one\r\ntwo\n");
        assert!(!out.contains('\r'), "LF source gained a CR: {out:?}");
        assert!(out.contains("## Tasks\none\ntwo\n## Risks"));
    }

    #[test]
    fn crlf_source_keeps_its_endings() {
        let lf = "# Plan\n\n## Tasks\n\nold\n\n## Risks\n\nrisk\n";
        let src = lf.replace('\n', "\r\n");
        assert_eq!(dominant_line_ending(&src), "\r\n");

        let body = sections(&src)[0].body_lf(&src);
        assert!(!body.contains('\r'), "body was not normalised: {body:?}");

        let out = replace_section(&src, "Tasks", "one\ntwo\n");
        assert!(out.contains("## Tasks\r\none\r\ntwo\r\n## Risks"));
        assert!(
            out.matches('\n').count() == out.matches("\r\n").count(),
            "CRLF source gained a bare LF: {out:?}"
        );
    }

    #[test]
    fn insert_places_the_section_after_its_anchor() {
        let src = "## Execution Policy\n\npolicy\n\n## Tasks\n\n### 1. One\n\n## Risks\n\nrisk\n";
        let out = insert_section_after(src, "Tasks", "Dependency Graph", "— CHECKPOINT A\n");
        assert_eq!(
            titles(&out),
            vec!["Execution Policy", "Tasks", "Dependency Graph", "Risks"]
        );
        assert!(out.contains("### 1. One\n\n## Dependency Graph\n— CHECKPOINT A\n## Risks"));
    }

    #[test]
    fn insert_appends_when_the_anchor_is_missing() {
        let src = "## Tasks\n\n### 1. One";
        let out = insert_section_after(src, "Nowhere", "Dependency Graph", "body");
        assert_eq!(out, "## Tasks\n\n### 1. One\n## Dependency Graph\nbody\n");
    }

    #[test]
    fn insert_before_places_the_section_ahead_of_an_opening_anchor() {
        let src = "## Tasks\n\n### 1. One\n\n## Risks\n\nrisk\n";
        let out = insert_section_before(src, "Tasks", "Execution Policy", "\npolicy\n\n");
        assert_eq!(
            titles(&out),
            vec!["Execution Policy", "Tasks", "Risks"],
            "{out}"
        );
        assert!(
            out.starts_with("## Execution Policy\n\npolicy\n\n## Tasks\n"),
            "{out}"
        );
    }

    #[test]
    fn insert_before_keeps_the_bytes_ahead_of_the_anchor() {
        let src = "# Plan\n\n## Approach\n\nprose\n\n## Tasks\n\n### 1. One\n";
        let out = insert_section_before(src, "Tasks", "Execution Policy", "policy\n");
        assert!(
            out.starts_with("# Plan\n\n## Approach\n\nprose\n\n"),
            "{out}"
        );
        assert!(
            out.ends_with("## Execution Policy\npolicy\n## Tasks\n\n### 1. One\n"),
            "{out}"
        );
    }

    #[test]
    fn insert_before_appends_when_the_anchor_is_missing() {
        let src = "## Tasks\n\n### 1. One";
        let out = insert_section_before(src, "Nowhere", "Dependency Graph", "body");
        assert_eq!(out, "## Tasks\n\n### 1. One\n## Dependency Graph\nbody\n");
    }

    #[test]
    fn insert_uses_the_source_line_ending() {
        let src = "## Tasks\r\n\r\n### 1. One\r\n";
        let out = insert_section_after(src, "Tasks", "Dependency Graph", "marker\n");
        assert!(out.ends_with("## Dependency Graph\r\nmarker\r\n"));
        assert!(out.matches('\n').count() == out.matches("\r\n").count());
    }
}
