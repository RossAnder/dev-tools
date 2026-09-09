//! Parsers for a plan's `## Execution Policy` bullets and `## Dependency Graph` markers.
//!
//! Both take an already-LF section body (`markdown::Section::body_lf`); neither
//! scans the document itself. `checkpoint_after` rides along with the policy but
//! is only ever compared against the markers' derived maximal elements — the
//! markers are the sole authority for checkpoint membership.
//!
//! Every pattern here uses explicit ASCII classes. The binary resolves `regex`
//! without its unicode features, so a `\d`/`\s`/`\w` shorthand makes
//! `Regex::new` return `Err` at startup there — while a dev-dependency unifies
//! those features back on, so a test run accepts it. That asymmetry is what
//! `every_pattern_compiles` scans the pattern text for.

use crate::io::advise;
use std::sync::OnceLock;

use anyhow::{Result, anyhow};
use regex::Regex;

use super::schema::{POLICY_ORIGIN_DEFAULT, POLICY_ORIGIN_PLAN};

const DEFAULT_CHECKPOINTS: &str = "milestones";
const DEFAULT_MAX_PARALLEL: u32 = 6;
const DEFAULT_COMMIT_GRANULARITY: &str = "per-task";

/// The stored spellings of the vocabulary fields, shared with `check` so the
/// import path and the store gate agree on what is in vocabulary.
pub(crate) const CHECKPOINTS_VALUES: [&str; 3] = ["single", "milestones", "per-batch"];
pub(crate) const GRANULARITY_VALUES: [&str; 3] = ["per-task", "per-checkpoint", "single-commit"];
pub(crate) const ORIGIN_VALUES: [&str; 2] = [POLICY_ORIGIN_PLAN, POLICY_ORIGIN_DEFAULT];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParsedPolicy {
    pub(crate) checkpoints: String,
    pub(crate) max_parallel: u32,
    pub(crate) commit_granularity: String,
    /// The plan carried a `## Execution Policy` section. False leaves every
    /// field above a default the plan never stated, which is a separate fact
    /// from what any of them holds.
    pub(crate) authored: bool,
    /// The clause trailing each bullet's value, kept against the bullet that
    /// carried it: the renderer puts it back on that line, and a single flat
    /// note would attach every one of them to whichever bullet renders last.
    pub(crate) checkpoints_note: String,
    pub(crate) max_parallel_note: String,
    pub(crate) commit_granularity_note: String,
    /// Prose belonging to no bullet, authored only. A parser diagnostic here
    /// would be indistinguishable from an author's own exception once the
    /// renderer writes it back into the document.
    pub(crate) note: String,
    /// The authored `Checkpoint after` bullet, for `checkpoint/marker-mismatch`
    /// only. Membership comes from the markers.
    pub(crate) checkpoint_after: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Marker {
    pub(crate) id: String,
    pub(crate) after: Vec<u32>,
    pub(crate) rationale: String,
}

/// `None` is an absent `## Execution Policy` section, which imports as the
/// house defaults rather than failing a legacy plan.
pub(crate) fn parse_policy(section_body: Option<&str>) -> Result<ParsedPolicy> {
    let mut policy = ParsedPolicy {
        checkpoints: DEFAULT_CHECKPOINTS.to_string(),
        max_parallel: DEFAULT_MAX_PARALLEL,
        commit_granularity: DEFAULT_COMMIT_GRANULARITY.to_string(),
        authored: section_body.is_some(),
        checkpoints_note: String::new(),
        max_parallel_note: String::new(),
        commit_granularity_note: String::new(),
        note: String::new(),
        checkpoint_after: Vec::new(),
    };

    let Some(body) = section_body else {
        return Ok(policy);
    };

    let scanned = scan_bullets(body);

    for (label, value) in &scanned.entries {
        let key = label.trim().trim_end_matches(':').to_ascii_lowercase();
        let (token, rest) = split_first_token(value);
        // The remainder goes to the bullet that carried it, and a repeated
        // bullet overwrites both halves together rather than accumulating a
        // note against a value that no longer stands.
        let note = match key.as_str() {
            "checkpoints" => {
                policy.checkpoints = one_of(label, token, &CHECKPOINTS_VALUES)?;
                &mut policy.checkpoints_note
            }
            "commit granularity" => {
                policy.commit_granularity = one_of(label, token, &GRANULARITY_VALUES)?;
                &mut policy.commit_granularity_note
            }
            "max parallel agents" => {
                // Out-of-range is a `policy/max-parallel-range` finding, not a
                // parse failure, so only a non-numeric token errors here.
                let digits = token.trim_matches(|c: char| !c.is_ascii_digit());
                policy.max_parallel = digits.parse::<u32>().map_err(|_| {
                    anyhow!("execution policy `{label}`: `{token}` is not a whole number")
                })?;
                &mut policy.max_parallel_note
            }
            "checkpoint after" => {
                policy.checkpoint_after = expand_ids(value)?;
                continue;
            }
            _ => continue,
        };
        *note = rest.to_string();
    }

    policy.note = scanned.prose.join("\n").trim().to_string();
    Ok(policy)
}

/// Fed the `## Dependency Graph` section only — markers duplicated inside
/// `## Tasks` are the caller's problem, not this parser's.
pub(crate) fn parse_markers(section_body: &str) -> Result<Vec<Marker>> {
    let mut markers: Vec<Marker> = Vec::new();

    for paragraph in paragraphs(section_body) {
        let flat = strip_marker_furniture(&paragraph);
        let Some(caps) = marker_re().captures(&flat) else {
            continue;
        };
        let whole = caps.get(0).expect("capture group 0 always matches");
        let marker = Marker {
            id: caps[1].to_string(),
            after: expand_ids(caps[2].trim())?,
            rationale: tidy_rationale(&flat[whole.end()..]),
        };

        match markers.iter().find(|m| m.id == marker.id) {
            Some(first) => {
                if first.after != marker.after || first.rationale != marker.rationale {
                    advise!(
                        "tomlctl: checkpoint `{}` is declared twice with different text — keeping the first",
                        marker.id
                    );
                }
            }
            None => markers.push(marker),
        }
    }

    Ok(markers)
}

/// Every `N`, `N-M` and `N–M` in `text`, in document order, ranges expanded.
fn expand_ids(text: &str) -> Result<Vec<u32>> {
    let mut ids = Vec::new();
    for caps in ids_re().captures_iter(text) {
        let low: u32 = caps[1]
            .parse()
            .map_err(|_| anyhow!("task id `{}` does not fit a u32", &caps[1]))?;
        match caps.get(2) {
            None => ids.push(low),
            Some(high) => {
                let high: u32 = high
                    .as_str()
                    .parse()
                    .map_err(|_| anyhow!("task id `{}` does not fit a u32", high.as_str()))?;
                if high < low {
                    return Err(anyhow!("task range `{low}–{high}` runs backwards"));
                }
                ids.extend(low..=high);
            }
        }
    }
    Ok(ids)
}

struct Bullets {
    /// `(label, value)` with wrapped continuation lines folded into the value.
    entries: Vec<(String, String)>,
    prose: Vec<String>,
}

fn scan_bullets(body: &str) -> Bullets {
    let mut entries: Vec<(String, String)> = Vec::new();
    let mut prose: Vec<String> = Vec::new();
    let mut current: Option<(String, String)> = None;

    for raw in body.lines() {
        let line = raw.trim();
        if line.is_empty() {
            entries.extend(current.take());
            continue;
        }
        if let Some(caps) = bullet_re().captures(line) {
            entries.extend(current.take());
            current = Some((caps[1].trim().to_string(), caps[2].trim().to_string()));
            continue;
        }
        if line.starts_with("- ") || line.starts_with("* ") || line.starts_with('#') {
            entries.extend(current.take());
            continue;
        }
        match current.as_mut() {
            Some((_, value)) => {
                value.push(' ');
                value.push_str(line);
            }
            None => prose.push(line.to_string()),
        }
    }

    entries.extend(current);
    Bullets { entries, prose }
}

fn paragraphs(body: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    for raw in body.lines() {
        let line = raw.trim();
        if line.is_empty() {
            if !current.is_empty() {
                out.push(current.join(" "));
                current.clear();
            }
        } else {
            current.push(line);
        }
    }
    if !current.is_empty() {
        out.push(current.join(" "));
    }
    out
}

fn strip_marker_furniture(paragraph: &str) -> String {
    paragraph
        .replace('*', "")
        .trim_start_matches(is_dash_or_space)
        .to_string()
}

/// The closure clause is recomputed on every render, so it must never survive
/// into the stored rationale — nor may `(INVALID CUT)`, which the renderer
/// re-appends and would otherwise accumulate one copy per round trip.
fn tidy_rationale(tail: &str) -> String {
    let mut out = trim_leading_punctuation(tail);
    if let Some(found) = closure_re().find(out) {
        // A parenthetical inside the clause absorbs the sentence's full stop,
        // so the prose that follows needs a second pass.
        out = trim_leading_punctuation(&out[found.end()..]);
    }
    loop {
        let before = out;
        out = out
            .trim()
            .trim_end_matches(is_dash_or_space)
            .trim_end_matches("(INVALID CUT)");
        if out == before {
            return out.to_string();
        }
    }
}

fn trim_leading_punctuation(text: &str) -> &str {
    text.trim_start_matches(|c: char| is_dash_or_space(c) || ":,;.".contains(c))
}

fn is_dash_or_space(c: char) -> bool {
    matches!(c, '—' | '–' | '-' | ' ' | '\t')
}

fn split_first_token(value: &str) -> (&str, &str) {
    let value = value.trim();
    match value.find([' ', '\t']) {
        Some(at) => (&value[..at], value[at..].trim()),
        None => (value, ""),
    }
}

fn one_of(label: &str, token: &str, allowed: &[&str]) -> Result<String> {
    let cleaned = token
        .trim_matches(|c: char| !c.is_ascii_alphanumeric())
        .to_ascii_lowercase();
    allowed
        .iter()
        .find(|candidate| **candidate == cleaned)
        .map(|candidate| (*candidate).to_string())
        .ok_or_else(|| {
            anyhow!(
                "execution policy `{label}`: unknown value `{token}` (expected one of {})",
                allowed.join(", ")
            )
        })
}

fn bullet_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^[-*][ \t]+\*\*([^*]+)\*\*[ \t]*:[ \t]*(.*)$").expect("bullet regex compiles")
    })
}

fn marker_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"CHECKPOINT ([A-Za-z0-9]+) after tasks? ([0-9][0-9, \-–]*)")
            .expect("marker regex compiles")
    })
}

fn ids_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"([0-9]+)(?:[ ]*[\-–][ ]*([0-9]+))?").expect("id-range regex compiles")
    })
}

fn closure_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^[(]?(?:dependency )?closure[^.)]*[.)]?").expect("closure regex compiles")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one_marker(body: &str) -> Marker {
        let mut found = parse_markers(body).expect("markers parse");
        assert_eq!(found.len(), 1, "expected one marker in {body:?}: {found:?}");
        found.remove(0)
    }

    fn id_and_after(body: &str) -> (String, Vec<u32>) {
        let marker = one_marker(body);
        (marker.id, marker.after)
    }

    #[test]
    fn every_pattern_compiles() {
        let patterns = [bullet_re(), marker_re(), ids_re(), closure_re()];

        // A dev-dependency unifies `regex/unicode` on, so a shorthand class is
        // accepted here and rejected by the dependency-free release build, where
        // `Regex::new` returns `Err` and the `expect` panics. Only a textual scan
        // of the pattern catches that from a test.
        for re in patterns {
            for shorthand in [r"\d", r"\D", r"\s", r"\S", r"\w", r"\W", r"\b", r"\B"] {
                assert!(
                    !re.as_str().contains(shorthand),
                    "`{shorthand}` in `{}` — spell the class out in ASCII",
                    re.as_str()
                );
            }
        }

        // Each accessor `expect`s, so a unicode-class regression fails here
        // rather than inside whichever verb happens to run first.
        assert!(bullet_re().is_match("- **Checkpoints**: milestones"));
        assert!(marker_re().is_match("CHECKPOINT A after task 12"));
        assert!(ids_re().is_match("1–12"));
        assert!(closure_re().is_match("closure: tasks 1–12."));
    }

    #[test]
    fn shape_em_dash_with_closure_sentence() {
        assert_eq!(
            id_and_after("— CHECKPOINT A after task 12 — closure: tasks 1–12."),
            ("A".to_string(), vec![12])
        );
    }

    #[test]
    fn shape_hyphen_range_before_a_colon() {
        assert_eq!(
            id_and_after("— CHECKPOINT A after tasks 1-5: the shell kit lands first"),
            ("A".to_string(), vec![1, 2, 3, 4, 5])
        );
    }

    #[test]
    fn shape_parenthesised_closure_annotation() {
        let marker = one_marker("— CHECKPOINT B after task 8 (closure: 1–8)");
        assert_eq!((marker.id.as_str(), &marker.after[..]), ("B", &[8][..]));
        assert_eq!(marker.rationale, "");
    }

    #[test]
    fn shape_parenthesised_dependency_closure() {
        let marker = one_marker("— CHECKPOINT C after task 14 (dependency closure: tasks 1–14)");
        assert_eq!((marker.id.as_str(), &marker.after[..]), ("C", &[14][..]));
        assert_eq!(marker.rationale, "");
    }

    #[test]
    fn shape_bolded_marker() {
        assert_eq!(
            id_and_after("— **CHECKPOINT A after task 11** —"),
            ("A".to_string(), vec![11])
        );
    }

    #[test]
    fn shape_triple_hyphen_rule_with_numeric_id() {
        let marker = one_marker("--- CHECKPOINT 1 after tasks 6, 10: closes the shell work ---");
        assert_eq!((marker.id.as_str(), &marker.after[..]), ("1", &[6, 10][..]));
        assert_eq!(marker.rationale, "closes the shell work");
    }

    #[test]
    fn brace_set_closure_leaves_no_rationale() {
        let marker = one_marker("— CHECKPOINT D after task 6 — closure {6, 1}");
        assert_eq!((marker.id.as_str(), &marker.after[..]), ("D", &[6][..]));
        assert_eq!(marker.rationale, "");
    }

    #[test]
    fn en_dash_range_in_the_after_list_expands() {
        assert_eq!(
            id_and_after("— CHECKPOINT A after tasks 1–12 —"),
            ("A".to_string(), (1..=12).collect::<Vec<u32>>())
        );
    }

    #[test]
    fn invalid_cut_never_reaches_the_rationale() {
        let marker = one_marker(
            "— CHECKPOINT B after tasks 4, 9 — closure: tasks 1–9. The panel and its tests. (INVALID CUT)",
        );
        assert_eq!(marker.after, vec![4, 9]);
        assert_eq!(marker.rationale, "The panel and its tests.");
    }

    #[test]
    fn a_parenthetical_inside_the_closure_clause_leaves_no_leading_stop() {
        let marker = one_marker(
            "— CHECKPOINT B after tasks 25, 26 — closure: tasks 14–19 and 24–26 (plus A). The markdown scanner and both parsers.",
        );
        assert_eq!(marker.after, vec![25, 26]);
        assert_eq!(marker.rationale, "The markdown scanner and both parsers.");
    }

    #[test]
    fn a_preamble_paragraph_is_not_a_marker() {
        let body = "\nPer-task `Depends on` lines are authoritative; this section states only the cuts.\n\n— CHECKPOINT A after task 3 — closure: tasks 1–3.\n";
        let found = parse_markers(body).expect("markers parse");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, "A");
    }

    #[test]
    fn a_repeated_marker_block_keeps_the_first() {
        let body = "— CHECKPOINT A after task 3 — first wins.\n\n— CHECKPOINT A after tasks 3, 4 — second loses.\n";
        let found = parse_markers(body).expect("markers parse");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].after, vec![3]);
        assert_eq!(found[0].rationale, "first wins.");
    }

    #[test]
    fn checkpoint_after_flattens_every_clause() {
        let body = "- **Checkpoint after**: tasks 2, 4, 5, 27, 28; task 14\n";
        let policy = parse_policy(Some(body)).expect("policy parses");
        assert_eq!(policy.checkpoint_after, vec![2, 4, 5, 27, 28, 14]);
    }

    #[test]
    fn the_four_bullets_parse() {
        let body = "\n- **Checkpoints**: milestones\n- **Checkpoint after**: tasks 12, 13, 22\n- **Max parallel agents**: 6\n- **Commit granularity**: per-task\n";
        let policy = parse_policy(Some(body)).expect("policy parses");
        assert_eq!(policy.checkpoints, "milestones");
        assert_eq!(policy.max_parallel, 6);
        assert_eq!(policy.commit_granularity, "per-task");
        assert_eq!(policy.checkpoint_after, vec![12, 13, 22]);
        assert_eq!(policy.note, "");
        assert!(policy.authored);
    }

    #[test]
    fn a_wrapped_remainder_becomes_the_note() {
        let body = "- **Checkpoints**: milestones — one commit train per\n  milestone group\n- **Max parallel agents**: 6\n";
        let policy = parse_policy(Some(body)).expect("policy parses");
        assert_eq!(policy.checkpoints, "milestones");
        assert_eq!(
            policy.checkpoints_note,
            "— one commit train per milestone group"
        );
        assert_eq!(policy.note, "");
    }

    /// Two bullets each carrying a clause: a flat note could not say which
    /// bullet either belongs to, and the renderer would put both on the last.
    #[test]
    fn each_bullet_keeps_its_own_remainder() {
        let body = "- **Checkpoints**: milestones — one commit train per milestone group\n\
            - **Max parallel agents**: 6 — 8 while the tree is quiet\n\
            - **Commit granularity**: per-task — tasks 5 and 6 land in one commit\n";
        let policy = parse_policy(Some(body)).expect("policy parses");
        assert_eq!(
            policy.checkpoints_note,
            "— one commit train per milestone group"
        );
        assert_eq!(policy.max_parallel_note, "— 8 while the tree is quiet");
        assert_eq!(
            policy.commit_granularity_note,
            "— tasks 5 and 6 land in one commit"
        );
        assert_eq!(policy.note, "");
    }

    /// Prose under the bullets belongs to no bullet, so it stays in the flat
    /// note rather than being attached to whichever bullet came last.
    #[test]
    fn free_prose_stays_out_of_every_bullet_note() {
        let body = "- **Checkpoints**: milestones\n- **Commit granularity**: per-task\n\n\
            The trains are cut by hand while the store is young.\n";
        let policy = parse_policy(Some(body)).expect("policy parses");
        assert_eq!(policy.checkpoints_note, "");
        assert_eq!(policy.commit_granularity_note, "");
        assert_eq!(
            policy.note,
            "The trains are cut by hand while the store is young."
        );
    }

    /// The absence is carried by `authored`, and `note` stays empty: the
    /// renderer writes a note back into the plan as the author's own prose,
    /// so a diagnostic placed there returns as authored content.
    #[test]
    fn an_absent_section_takes_the_defaults_and_authors_no_note() {
        let policy = parse_policy(None).expect("policy parses");
        assert_eq!(policy.checkpoints, "milestones");
        assert_eq!(policy.max_parallel, 6);
        assert_eq!(policy.commit_granularity, "per-task");
        assert!(!policy.authored);
        assert_eq!(policy.note, "");
        assert_eq!(policy.checkpoints_note, "");
        assert_eq!(policy.max_parallel_note, "");
        assert_eq!(policy.commit_granularity_note, "");
        assert!(policy.checkpoint_after.is_empty());
    }

    #[test]
    fn an_out_of_range_max_parallel_survives_for_the_check_to_flag() {
        let policy = parse_policy(Some("- **Max parallel agents**: 12\n")).expect("policy parses");
        assert_eq!(policy.max_parallel, 12);
    }

    #[test]
    fn an_unknown_enum_names_the_label() {
        let err = parse_policy(Some("- **Checkpoints**: hourly\n")).expect_err("unknown value");
        let text = err.to_string();
        assert!(text.contains("Checkpoints"), "{text}");
        assert!(text.contains("hourly"), "{text}");
    }
}
