//! Record targets as review findings carry them: the finding `file` and
//! `symbol` a target implies, resolution of a target to its current value,
//! and list-entry matching by value. The grammar and the store readers are
//! the plan module's ([`crate::plan::target`]); prose (brief and design
//! sections) is read here.
//!
//! A stored `spec.accept` entry carries a tool-written `probe` that no finding
//! can supply, so accept entries resolve and match with `probe` removed.

use std::path::{Path, PathBuf};

use anyhow::Result;
use serde_json::Value as JsonValue;

use super::add::section_spans;
use super::invalid;
use crate::plan::store::BriefPath;
pub(super) use crate::plan::target::{RecordTarget, parse};
use crate::tasks::schema::MILESTONE_REF_PREFIX;

const PROBE_KEY: &str = "probe";

impl RecordTarget {
    /// The repo-relative file holding the record.
    pub(super) fn file(&self, slug: &str) -> String {
        match self {
            Self::TaskField { .. }
            | Self::SpecField { .. }
            | Self::SpecEntry { .. }
            | Self::Milestone { .. } => format!(".claude/flows/{slug}/tasks.toml"),
            Self::Decision { .. } | Self::Contract { .. } | Self::Handover { .. } => {
                format!(".claude/flows/{slug}/plan.toml")
            }
            Self::Brief { .. } => format!("docs/flows/{slug}/brief.md"),
            Self::Design { file, .. } => format!("docs/flows/{slug}/{file}"),
        }
    }

    /// The record within [`Self::file`]: a task ref, `milestone:<id>`, a plan
    /// record id, a brief section or a design anchor.
    pub(super) fn symbol(&self) -> String {
        match self {
            Self::TaskField { r#ref, .. }
            | Self::SpecField { r#ref, .. }
            | Self::SpecEntry { r#ref, .. } => r#ref.clone(),
            Self::Milestone { id, .. } => format!("{MILESTONE_REF_PREFIX}{id}"),
            Self::Decision { id, .. } | Self::Contract { id, .. } | Self::Handover { id, .. } => {
                id.clone()
            }
            Self::Brief { section } => (*section).to_string(),
            Self::Design { anchor, .. } => anchor.clone(),
        }
    }
}

/// The current value of the record `target` names in flow `slug`: a store
/// record as its store's review reader reports it, with a `SpecEntry` read as
/// its whole list for [`match_entry`] to search; a brief section as its body;
/// a design anchor as its section, heading line included.
pub(super) fn resolve(slug: &str, target: &RecordTarget) -> Result<JsonValue> {
    if let Some(value) = crate::plan::target::current(slug, target)? {
        return Ok(value);
    }
    let authored = crate::plan::store::authored_dir(slug)?;
    match target {
        RecordTarget::Brief { section } => brief_value(&authored, section),
        RecordTarget::Design { file, anchor } => {
            crate::plan::check::design_section(&authored, &format!("{file}#{anchor}"))
                .map(JsonValue::String)
                .ok_or_else(|| invalid(format!("target `{target}` names no design section")))
        }
        _ => unreachable!("every store target resolves through its store"),
    }
}

/// The brief is read with `\r\n` as `\n`, and only when it resolves inside the
/// flow's authored directory, so a symlinked brief cannot surface another file.
fn brief_value(authored: &Path, section: &str) -> Result<JsonValue> {
    let resolved = brief_path(authored, section)?;
    let bytes = std::fs::read(&resolved)?;
    let text = String::from_utf8(bytes)
        .map_err(|_| invalid(format!("target `brief#{section}`: brief.md is not UTF-8")))?
        .replace("\r\n", "\n");
    brief_section(&text, section).map(|(_, body)| JsonValue::String(body))
}

/// The real path of the flow's brief, refused when it is absent or resolves
/// outside the authored directory's anchor.
pub(super) fn brief_path(authored: &Path, section: &str) -> Result<PathBuf> {
    let refuse = |why: &str| invalid(format!("target `brief#{section}`: {why}"));
    match crate::plan::store::locate_brief(authored)? {
        BriefPath::At(resolved) => Ok(resolved),
        BriefPath::Absent => Err(refuse("the flow has no brief.md")),
        BriefPath::Outside => Err(refuse("brief.md resolves outside the flow's directory")),
    }
}

/// The one `section` of the brief `text`, already in its `\n` form, with the
/// byte offset in `text` at which its body starts.
pub(super) fn brief_section(text: &str, section: &str) -> Result<(usize, String)> {
    let refuse = |why: String| invalid(format!("target `brief#{section}`: {why}"));
    let mut spans = section_spans(text, section);
    match spans.len() {
        1 => Ok(spans.remove(0)),
        0 => Err(refuse(format!("brief.md has no `{section}` section"))),
        n => Err(refuse(format!("brief.md has {n} `{section}` sections"))),
    }
}

/// The index of the one entry of `list` equal to `expect`, comparing objects
/// with any `probe` key removed on both sides. No match and several matches are
/// both refused, since the target's index is only a hint.
pub(super) fn match_entry(list: &[JsonValue], expect: &JsonValue) -> Result<usize> {
    let expect = without_probe(expect.clone());
    let found: Vec<usize> = list
        .iter()
        .enumerate()
        .filter(|(_, entry)| without_probe((*entry).clone()) == expect)
        .map(|(at, _)| at)
        .collect();
    match found.as_slice() {
        [one] => Ok(*one),
        [] => Err(invalid("no list entry equals `expect`")),
        many => Err(invalid(format!(
            "{} list entries equal `expect`, so the entry is ambiguous",
            many.len()
        ))),
    }
}

fn without_probe(mut entry: JsonValue) -> JsonValue {
    if let Some(map) = entry.as_object_mut() {
        map.remove(PROBE_KEY);
    }
    entry
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn file_and_symbol_follow_the_record() {
        let entry = parse("task:parse-it.spec.accept[3]").unwrap();
        assert_eq!(entry.file("s"), ".claude/flows/s/tasks.toml");
        assert_eq!(entry.symbol(), "parse-it");
        let milestone = parse("milestone:A.goal").unwrap();
        assert_eq!(milestone.symbol(), "milestone:A");
        let decision = parse("decision:D3.why").unwrap();
        assert_eq!(decision.file("s"), ".claude/flows/s/plan.toml");
        assert_eq!(decision.symbol(), "D3");
        let brief = parse("brief#Intent").unwrap();
        assert_eq!(brief.file("s"), "docs/flows/s/brief.md");
        assert_eq!(brief.symbol(), "Intent");
        let design = parse("design/api.md#grammar").unwrap();
        assert_eq!(design.file("s"), "docs/flows/s/design/api.md");
        assert_eq!(design.symbol(), "grammar");
    }

    #[test]
    fn a_probed_entry_matches_an_expect_without_probe() {
        let stored = vec![json!({
            "behaviour": "parses",
            "check": "cargo test parse",
            "probe": {"verdict": "healthy"},
        })];
        let expect = json!({"behaviour": "parses", "check": "cargo test parse"});
        assert_eq!(match_entry(&stored, &expect).unwrap(), 0);
    }

    #[test]
    fn duplicate_and_missing_entries_are_refused() {
        let list = [json!("no new deps"), json!("no new deps")];
        let err = match_entry(&list, &json!("no new deps")).unwrap_err();
        assert!(format!("{err:#}").contains("2 list entries"), "{err:#}");
        let err = match_entry(&list, &json!("absent")).unwrap_err();
        assert!(format!("{err:#}").contains("no list entry"), "{err:#}");
    }

    #[test]
    fn a_brief_section_resolves_to_its_body() {
        crate::test_support::with_root(|root| {
            let authored = root.join("docs").join("flows").join("x");
            std::fs::create_dir_all(&authored).unwrap();
            std::fs::write(
                authored.join(crate::plan::store::BRIEF_FILE),
                "# x\r\n\r\n## Intent\r\n\r\nShip it.\r\n\r\n## Non-goals\r\n\r\nNone.\r\n",
            )
            .unwrap();
            assert_eq!(
                brief_value(&authored, "Intent").unwrap(),
                json!("\nShip it.\n\n")
            );
            assert!(brief_value(&authored, "Unknowns").is_err());
        });
    }

    #[test]
    fn a_brief_section_carries_its_offset() {
        let text = "# x\n\n## Intent\n\nNone.\n\n## Unknowns\n\nNone.\n";
        let (at, body) = brief_section(text, "Unknowns").unwrap();
        assert_eq!(body, "\nNone.\n");
        assert_eq!(&text[at..], body);
    }
}
