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
    /// Bytes after the heading line, up to the next `## ` heading or EOF.
    pub(crate) body_range: Range<usize>,
}

impl Section {
    /// Body with CRLF collapsed to LF, which is what the field parsers match on.
    pub(crate) fn body_lf(&self, src: &str) -> String {
        src[self.body_range.clone()].replace("\r\n", "\n")
    }
}

pub(crate) fn sections(src: &str) -> Vec<Section> {
    let mut found: Vec<Section> = Vec::new();
    let mut fence: Option<(char, usize)> = None;
    let mut pos = 0usize;

    while pos < src.len() {
        let (content, next) = split_line(src, pos);

        match fence {
            Some((marker, width)) => {
                if closes_fence(content, marker, width) {
                    fence = None;
                }
            }
            None => {
                if let Some(open) = opens_fence(content) {
                    fence = Some(open);
                } else if let Some(heading) = content.strip_prefix("## ") {
                    if let Some(previous) = found.last_mut() {
                        previous.body_range.end = pos;
                    }
                    found.push(Section {
                        title: heading.trim().to_string(),
                        body_range: next..src.len(),
                    });
                }
            }
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
    fn insert_uses_the_source_line_ending() {
        let src = "## Tasks\r\n\r\n### 1. One\r\n";
        let out = insert_section_after(src, "Tasks", "Dependency Graph", "marker\n");
        assert!(out.ends_with("## Dependency Graph\r\nmarker\r\n"));
        assert!(out.matches('\n').count() == out.matches("\r\n").count());
    }
}
