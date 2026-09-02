//! `backlog check` — the graded already-known verdict an agent reads before
//! deciding whether to mint.
//!
//! The probe's `dedup_id` is derived exactly the way `add` derives a row's,
//! so a `duplicate` verdict predicts the id `add` would land on. Any drift
//! between the two derivations turns the gate into advisory noise.
//!
//! Read-only, and a missing store is a `novel` answer rather than an error:
//! the first capture in a repo runs this before anything exists to read.
//!
//! Evidence counts are read from the directory at call time, for the returned
//! candidates only. The store holds no evidence field, so a count is only
//! true at the moment it is taken.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::Result;
use serde_json::{Value as JsonValue, json};
use toml::Value as TomlValue;

use super::evidence;
use super::ids::dedup_id_from_parts;
use super::normalise::{
    SIMILARITY_RELATED, SIMILARITY_STRONG, char_trigrams, jaccard, word_tokens,
};
use super::schema::{
    self, ARRAY_BACKLOG, ARRAY_COMPACTED, FIELD_AREA, FIELD_CONTEXT, FIELD_DEDUP_ID, FIELD_ID,
    FIELD_SEEN_COUNT, FIELD_STATUS, FIELD_SUMMARY, FIELD_TAGS, KIND_OTHER, coerce_kind,
};
use crate::cli::{ReadIntegrityArgs, read_integrity_opts};
use crate::errors::{ErrorKind, tagged_err};
use crate::integrity::maybe_verify_integrity;
use crate::io::{items_array, read_toml};
use crate::output::print_json;

/// Number of shared leading `area` components, or shared tags, at which a
/// candidate is proposed as `related` on structure alone. One is the
/// top-level crate directory, which every row under a crate shares.
const SHARED_STRUCTURE_MIN: usize = 2;

/// Why a candidate qualified. Declaration order is the verdict ladder: the
/// first reason that applies to a row is the one reported, and the strongest
/// reason across all candidates is the overall verdict.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Reason {
    DedupId,
    Compacted,
    DuplicateId,
    Trigram,
    Words,
    Area,
    Tags,
}

impl Reason {
    fn as_str(self) -> &'static str {
        match self {
            Self::DedupId => "dedup_id",
            Self::Compacted => "compacted",
            Self::DuplicateId => "duplicate-id",
            Self::Trigram => "trigram",
            Self::Words => "words",
            Self::Area => "area",
            Self::Tags => "tags",
        }
    }

    fn verdict(self) -> &'static str {
        match self {
            Self::DedupId => "duplicate",
            Self::Compacted => "previously-resolved",
            Self::DuplicateId => "duplicate-id",
            Self::Trigram => "likely-duplicate",
            Self::Words | Self::Area | Self::Tags => "related",
        }
    }
}

const VERDICT_NOVEL: &str = "novel";

struct Thresholds {
    strong: f64,
    related: f64,
}

/// The discovery being weighed, with its comparison sets folded once rather
/// than per candidate.
struct Probe {
    dedup_id: String,
    trigrams: BTreeSet<String>,
    words: BTreeSet<String>,
    area: Vec<String>,
    tags: BTreeSet<String>,
}

impl Probe {
    fn new(summary: &str, area: Option<&str>, kind: Option<&str>, tags: &[String]) -> Self {
        let area = area.unwrap_or_default();
        Self {
            dedup_id: dedup_id_from_parts(coerce_kind(kind.unwrap_or(KIND_OTHER)), area, summary),
            trigrams: char_trigrams(summary),
            words: word_tokens(summary),
            area: components(area),
            tags: tags.iter().cloned().collect(),
        }
    }

    /// The four similarity rungs, in ladder order, for one row. `None` when
    /// the row clears none of them — those are never returned.
    fn grade(&self, row: &Row<'_>, thresholds: &Thresholds) -> Option<(Reason, f64)> {
        let trigram = jaccard(&self.trigrams, &char_trigrams(row.summary));
        let words = jaccard(&self.words, &word_tokens(row.summary));
        // Reported strength is the better of the two measures whichever rung
        // matched, so it is not comparable across rungs: a structural match can
        // out-score a textual one. Candidates sort by rung first for that reason.
        let score = trigram.max(words);
        if trigram >= thresholds.strong {
            return Some((Reason::Trigram, score));
        }
        if words >= thresholds.related {
            return Some((Reason::Words, score));
        }
        if shared_leading(&self.area, &components(row.area)) >= SHARED_STRUCTURE_MIN {
            return Some((Reason::Area, score));
        }
        if self
            .tags
            .iter()
            .filter(|tag| row.tags.contains(tag.as_str()))
            .count()
            >= SHARED_STRUCTURE_MIN
        {
            return Some((Reason::Tags, score));
        }
        None
    }
}

struct Candidate {
    id: String,
    summary: String,
    score: f64,
    reason: Reason,
    status: String,
    seen_count: i64,
    context: String,
}

struct Verdicts {
    verdict: &'static str,
    candidates: Vec<Candidate>,
}

impl Verdicts {
    fn cap(&mut self, limit: usize) {
        self.candidates.truncate(limit);
    }
}

/// One stored row, flattened across both arrays so the ladder walks a single
/// list. `array` is what separates `duplicate` from `previously-resolved`.
struct Row<'a> {
    array: &'static str,
    id: &'a str,
    dedup_id: &'a str,
    summary: &'a str,
    area: &'a str,
    status: &'a str,
    context: &'a str,
    tags: BTreeSet<&'a str>,
    seen_count: i64,
}

fn str_field<'a>(item: &'a TomlValue, key: &str) -> &'a str {
    item.get(key).and_then(TomlValue::as_str).unwrap_or_default()
}

fn rows(doc: &TomlValue) -> Vec<Row<'_>> {
    let mut out = Vec::new();
    for array in [ARRAY_BACKLOG, ARRAY_COMPACTED] {
        for item in items_array(doc, array) {
            out.push(Row {
                array,
                id: str_field(item, FIELD_ID),
                dedup_id: str_field(item, FIELD_DEDUP_ID),
                summary: str_field(item, FIELD_SUMMARY),
                area: str_field(item, FIELD_AREA),
                status: str_field(item, FIELD_STATUS),
                context: str_field(item, FIELD_CONTEXT),
                tags: item
                    .get(FIELD_TAGS)
                    .and_then(TomlValue::as_array)
                    .map(|tags| tags.iter().filter_map(TomlValue::as_str).collect())
                    .unwrap_or_default(),
                seen_count: item
                    .get(FIELD_SEEN_COUNT)
                    .and_then(TomlValue::as_integer)
                    .unwrap_or_default(),
            });
        }
    }
    out
}

fn components(area: &str) -> Vec<String> {
    area.split('/')
        .filter(|part| !part.is_empty())
        .map(str::to_owned)
        .collect()
}

fn shared_leading(a: &[String], b: &[String]) -> usize {
    a.iter().zip(b).take_while(|(x, y)| x == y).count()
}

/// Row indices keyed by fingerprint, so the three exact rungs cost one
/// lookup and only the fallback scan pays per-candidate trigram folding.
fn index_by_dedup<'a>(rows: &'a [Row<'a>]) -> BTreeMap<&'a str, Vec<usize>> {
    let mut index: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (position, row) in rows.iter().enumerate() {
        if !row.dedup_id.is_empty() {
            index.entry(row.dedup_id).or_default().push(position);
        }
    }
    index
}

/// Rows sharing an `id` with a row of a different fingerprint — the shape a
/// text merge of two worktrees leaves behind. Reported whatever the probe
/// asked, because it makes every later id lookup ambiguous.
fn colliding_ids(rows: &[Row<'_>]) -> BTreeSet<usize> {
    let mut by_id: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (position, row) in rows.iter().enumerate() {
        if !row.id.is_empty() {
            by_id.entry(row.id).or_default().push(position);
        }
    }
    by_id
        .values()
        .filter(|positions| {
            positions
                .iter()
                .map(|&position| rows[position].dedup_id)
                .collect::<BTreeSet<_>>()
                .len()
                > 1
        })
        .flatten()
        .copied()
        .collect()
}

fn evaluate(doc: &TomlValue, probe: &Probe, thresholds: &Thresholds) -> Verdicts {
    let rows = rows(doc);
    let by_dedup = index_by_dedup(&rows);
    let mut graded: BTreeMap<usize, (Reason, f64)> = BTreeMap::new();

    for &position in by_dedup
        .get(probe.dedup_id.as_str())
        .map(Vec::as_slice)
        .unwrap_or_default()
    {
        let reason = if rows[position].array == ARRAY_BACKLOG {
            Reason::DedupId
        } else {
            Reason::Compacted
        };
        graded.insert(position, (reason, 1.0));
    }
    let exact = !graded.is_empty();

    for position in colliding_ids(&rows) {
        graded.entry(position).or_insert((Reason::DuplicateId, 1.0));
    }
    // An exact fingerprint hit answers the question the caller asked; the
    // near matches behind it are noise, and skipping them is what keeps the
    // common case off the per-candidate trigram path.
    if !exact {
        for (position, row) in rows.iter().enumerate() {
            if graded.contains_key(&position) {
                continue;
            }
            if let Some(grade) = probe.grade(row, thresholds) {
                graded.insert(position, grade);
            }
        }
    }

    let mut candidates: Vec<Candidate> = graded
        .into_iter()
        .map(|(position, (reason, score))| {
            let row = &rows[position];
            Candidate {
                id: row.id.to_owned(),
                summary: row.summary.to_owned(),
                score,
                reason,
                status: row.status.to_owned(),
                seen_count: row.seen_count,
                context: row.context.to_owned(),
            }
        })
        .collect();
    // Rung outranks score: `--limit` must truncate the weaker reasons, never
    // the stronger ones. Score only orders candidates within one rung.
    candidates.sort_by(|a, b| {
        a.reason
            .cmp(&b.reason)
            .then_with(|| b.score.total_cmp(&a.score))
            .then_with(|| a.id.cmp(&b.id))
    });

    Verdicts {
        verdict: candidates
            .iter()
            .map(|candidate| candidate.reason)
            .min()
            .map_or(VERDICT_NOVEL, Reason::verdict),
        candidates,
    }
}

/// Non-marker files in the item's drop-box, 0 when it has none. A row with no
/// `id` owns no directory, so it is not a path to resolve.
fn evidence_count(id: &str) -> Result<usize> {
    if id.is_empty() {
        return Ok(0);
    }
    let dir = evidence::dir_for(id)?;
    Ok(evidence::list_dir(&dir)?.map_or(0, |files| files.len()))
}

/// Four decimals, because a raw Jaccard prints seventeen significant digits
/// and the caller compares it against a two-decimal threshold.
fn round4(score: f64) -> f64 {
    (score * 10_000.0).round() / 10_000.0
}

fn render(probe: &Probe, thresholds: &Thresholds, verdicts: &Verdicts) -> Result<JsonValue> {
    let mut candidates = Vec::with_capacity(verdicts.candidates.len());
    for candidate in &verdicts.candidates {
        candidates.push(json!({
            "id": candidate.id,
            "summary": candidate.summary,
            "score": round4(candidate.score),
            "reason": candidate.reason.as_str(),
            "status": candidate.status,
            "seen_count": candidate.seen_count,
            "context": candidate.context,
            "evidence_files": evidence_count(&candidate.id)?,
        }));
    }
    Ok(json!({
        "verdict": verdicts.verdict,
        "dedup_id": probe.dedup_id,
        "thresholds": {"strong": thresholds.strong, "related": thresholds.related},
        "candidates": candidates,
    }))
}

fn threshold(flag: &str, value: Option<f64>, default: f64) -> Result<f64> {
    match value {
        None => Ok(default),
        // NaN fails `contains`, so it is rejected here rather than silently
        // failing every later comparison.
        Some(given) if (0.0..=1.0).contains(&given) => Ok(given),
        Some(given) => Err(tagged_err(
            ErrorKind::Validation,
            None,
            format!("{flag} must be between 0.0 and 1.0; got {given}"),
        )),
    }
}

// One parameter per flag on the CLI variant, which is the dispatch contract.
#[allow(clippy::too_many_arguments)]
pub(crate) fn dispatch(
    summary: String,
    area: Option<String>,
    kind: Option<String>,
    tag: Vec<String>,
    limit: usize,
    similarity_strong: Option<f64>,
    similarity_related: Option<f64>,
    integrity: ReadIntegrityArgs,
) -> Result<()> {
    let thresholds = Thresholds {
        strong: threshold("--similarity-strong", similarity_strong, SIMILARITY_STRONG)?,
        related: threshold("--similarity-related", similarity_related, SIMILARITY_RELATED)?,
    };
    let probe = Probe::new(&summary, area.as_deref(), kind.as_deref(), &tag);

    let file = schema::backlog_path()?;
    let doc = if file.exists() {
        maybe_verify_integrity(&file, read_integrity_opts(&integrity))?;
        read_toml(&file)?
    } else if integrity.strict_read {
        return Err(tagged_err(
            ErrorKind::NotFound,
            Some(file.clone()),
            format!("file does not exist: {}", file.display()),
        ));
    } else {
        TomlValue::Table(toml::Table::new())
    };

    let mut verdicts = evaluate(&doc, &probe, &thresholds);
    verdicts.cap(limit);
    print_json(&render(&probe, &thresholds, &verdicts)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backlog::schema::{
        COMPACTED_FIELDS, FIELD_COMPACTED_ON, FIELD_KIND, FIELD_TERMINAL_DATE,
        FIELD_TERMINAL_REASON, KIND_BUG, KIND_FLAKY_TEST, STATUS_OPEN, STATUS_RESOLVED,
    };
    use std::fs;
    use std::path::{Path, PathBuf};

    const FLAKE_AREA: &str = "lumina/server/tests/pty_readiness_probe.rs";
    const FLAKE_SUMMARY: &str = "PTY readiness probe flakes on slow CI";
    const FLAKE_CONTEXT: &str = "Only reproduces when the readiness gate races the first write.";

    const COMPACTED_AREA: &str = "lumina/server/src/pty/spawn.rs";
    const COMPACTED_SUMMARY: &str = "spawning claude fails with CreateProcessW error 5";
    const COMPACTED_CONTEXT: &str = "An empty entry in HKLM PATH; resolve the binary absolutely.";

    fn defaults() -> Thresholds {
        Thresholds {
            strong: SIMILARITY_STRONG,
            related: SIMILARITY_RELATED,
        }
    }

    fn read_args() -> ReadIntegrityArgs {
        ReadIntegrityArgs {
            verify_integrity: false,
            strict_read: false,
        }
    }

    fn table(pairs: &[(&str, &str)]) -> TomlValue {
        let mut row = toml::map::Map::new();
        for (key, value) in pairs {
            row.insert((*key).to_owned(), TomlValue::String((*value).to_owned()));
        }
        TomlValue::Table(row)
    }

    fn live_row(id: &str, kind: &str, area: &str, summary: &str, context: &str) -> TomlValue {
        let dedup = dedup_id_from_parts(kind, area, summary);
        let mut row = table(&[
            (FIELD_ID, id),
            (FIELD_KIND, kind),
            (FIELD_AREA, area),
            (FIELD_SUMMARY, summary),
            (FIELD_STATUS, STATUS_OPEN),
            (FIELD_CONTEXT, context),
            (FIELD_DEDUP_ID, dedup.as_str()),
        ]);
        row.as_table_mut()
            .unwrap()
            .insert(FIELD_SEEN_COUNT.to_owned(), TomlValue::Integer(1));
        row
    }

    /// Built by walking `COMPACTED_FIELDS` so the fixture cannot drift from
    /// the shape `compact` writes: a new field fails the match arm loudly.
    fn compacted_row() -> TomlValue {
        let dedup = dedup_id_from_parts(KIND_BUG, COMPACTED_AREA, COMPACTED_SUMMARY);
        let mut row = toml::map::Map::new();
        for field in COMPACTED_FIELDS {
            let value = match *field {
                FIELD_ID => "B-c0ffee11",
                FIELD_DEDUP_ID => dedup.as_str(),
                FIELD_SUMMARY => COMPACTED_SUMMARY,
                FIELD_KIND => KIND_BUG,
                FIELD_AREA => COMPACTED_AREA,
                FIELD_STATUS => STATUS_RESOLVED,
                FIELD_TERMINAL_DATE => "2026-08-01",
                FIELD_TERMINAL_REASON => "fixed by resolving the binary absolutely",
                FIELD_CONTEXT => COMPACTED_CONTEXT,
                FIELD_COMPACTED_ON => "2026-08-20",
                other => panic!("no fixture value for compacted field `{other}`"),
            };
            row.insert((*field).to_owned(), TomlValue::String(value.to_owned()));
        }
        TomlValue::Table(row)
    }

    fn store(backlog: Vec<TomlValue>, compacted: Vec<TomlValue>) -> TomlValue {
        let mut doc = toml::map::Map::new();
        doc.insert("schema_version".to_owned(), TomlValue::Integer(1));
        doc.insert(ARRAY_BACKLOG.to_owned(), TomlValue::Array(backlog));
        doc.insert(ARRAY_COMPACTED.to_owned(), TomlValue::Array(compacted));
        TomlValue::Table(doc)
    }

    fn populated() -> TomlValue {
        store(
            vec![
                live_row(
                    "B-a1b2c3d4",
                    KIND_FLAKY_TEST,
                    FLAKE_AREA,
                    FLAKE_SUMMARY,
                    FLAKE_CONTEXT,
                ),
                live_row(
                    "B-7f0e2d91",
                    KIND_BUG,
                    "tomlctl/src/backlog/add.rs",
                    "sqlite migration checksum drifts after a renormalise",
                    "",
                ),
            ],
            vec![compacted_row()],
        )
    }

    fn probe(summary: &str, area: &str, kind: &str) -> Probe {
        Probe::new(summary, Some(area), Some(kind), &[])
    }

    fn verdict_of(doc: &TomlValue, probe: &Probe) -> (String, Vec<(String, String, f64)>) {
        let verdicts = evaluate(doc, probe, &defaults());
        (
            verdicts.verdict.to_owned(),
            verdicts
                .candidates
                .iter()
                .map(|c| (c.id.clone(), c.reason.as_str().to_owned(), c.score))
                .collect(),
        )
    }

    /// Resolve store and evidence paths under a throwaway root, dropping the
    /// override before any assertion runs.
    fn under_root<T>(f: impl FnOnce(&Path) -> T) -> (PathBuf, T) {
        let _guard = crate::test_support::env_lock();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        // SAFETY: set_var is unsafe in edition 2024; acceptable inside tests
        // where we hold the env lock.
        unsafe {
            std::env::set_var("TOMLCTL_ROOT", root.as_os_str());
        }
        let out = f(&root);
        unsafe {
            std::env::remove_var("TOMLCTL_ROOT");
        }
        (root, out)
    }

    fn kind_of(err: &anyhow::Error) -> &'static str {
        err.downcast_ref::<crate::errors::TaggedError>()
            .map_or("other", |tagged| tagged.kind.as_str())
    }

    #[test]
    fn the_probe_fingerprint_is_the_one_add_would_store() {
        let stored = dedup_id_from_parts(KIND_FLAKY_TEST, FLAKE_AREA, FLAKE_SUMMARY);
        assert_eq!(
            probe(FLAKE_SUMMARY, FLAKE_AREA, KIND_FLAKY_TEST).dedup_id,
            stored
        );
        // An omitted --kind must fingerprint as `other`, not as the empty
        // string, or every kindless check misses its own stored row.
        assert_eq!(
            Probe::new("a summary", None, None, &[]).dedup_id,
            dedup_id_from_parts(KIND_OTHER, "", "a summary")
        );
    }

    #[test]
    fn a_punctuation_and_case_rephrasing_is_a_duplicate() {
        let (verdict, candidates) = verdict_of(
            &populated(),
            &probe(
                "  PTY-READINESS-PROBE   flakes,  on  slow CI!! ",
                FLAKE_AREA,
                KIND_FLAKY_TEST,
            ),
        );
        assert_eq!(verdict, "duplicate");
        assert_eq!(
            candidates,
            vec![("B-a1b2c3d4".to_owned(), "dedup_id".to_owned(), 1.0)]
        );
    }

    #[test]
    fn a_near_paraphrase_is_a_likely_duplicate() {
        let doc = store(
            vec![live_row(
                "B-a1b2c3d4",
                KIND_BUG,
                "lumina/web/src/checkout/Total.vue",
                "checkout total overlaps the confirm button below 1400px",
                "",
            )],
            vec![],
        );
        let (verdict, candidates) = verdict_of(
            &doc,
            &probe(
                "checkout total overlaps the confirm button below 1440px",
                "lumina/web/src/checkout/Total.vue",
                KIND_BUG,
            ),
        );
        assert_eq!(verdict, "likely-duplicate");
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].1, "trigram");
        assert!(candidates[0].2 >= SIMILARITY_STRONG, "{candidates:?}");
        assert!(candidates[0].2 < 1.0, "{candidates:?}");
    }

    #[test]
    fn an_unrelated_summary_in_the_same_directory_is_related_by_area() {
        let (verdict, candidates) = verdict_of(
            &populated(),
            &probe(
                "dry run preview must leave the sidecar byte identical",
                "tomlctl/src/backlog/check.rs",
                KIND_BUG,
            ),
        );
        assert_eq!(verdict, "related");
        assert_eq!(
            candidates
                .iter()
                .map(|(id, reason, _)| (id.as_str(), reason.as_str()))
                .collect::<Vec<_>>(),
            vec![("B-7f0e2d91", "area")]
        );
    }

    #[test]
    fn one_shared_area_component_is_not_enough() {
        let (verdict, candidates) = verdict_of(
            &populated(),
            &probe(
                "dry run preview must leave the sidecar byte identical",
                "tomlctl/tests/backlog_cli.rs",
                KIND_BUG,
            ),
        );
        assert_eq!(verdict, VERDICT_NOVEL);
        assert!(candidates.is_empty(), "{candidates:?}");
    }

    #[test]
    fn two_shared_tags_are_related_where_one_is_not() {
        let mut tagged = live_row("B-11111111", KIND_BUG, "", "an unrelated capture", "");
        tagged.as_table_mut().unwrap().insert(
            FIELD_TAGS.to_owned(),
            TomlValue::Array(vec![
                TomlValue::String("ci".to_owned()),
                TomlValue::String("windows".to_owned()),
                TomlValue::String("pty".to_owned()),
            ]),
        );
        let doc = store(vec![tagged], vec![]);

        let both = Probe::new(
            "nothing whatever in common",
            None,
            Some(KIND_BUG),
            &["ci".to_owned(), "windows".to_owned()],
        );
        let (verdict, candidates) = verdict_of(&doc, &both);
        assert_eq!(verdict, "related");
        assert_eq!(candidates[0].1, "tags");

        let one = Probe::new(
            "nothing whatever in common",
            None,
            Some(KIND_BUG),
            &["ci".to_owned(), "sqlite".to_owned()],
        );
        assert_eq!(verdict_of(&doc, &one).0, VERDICT_NOVEL);
    }

    #[test]
    fn a_compacted_fingerprint_hit_is_previously_resolved() {
        let doc = populated();
        let probe = probe(COMPACTED_SUMMARY, COMPACTED_AREA, KIND_BUG);
        let verdicts = evaluate(&doc, &probe, &defaults());
        assert_eq!(verdicts.verdict, "previously-resolved");
        assert_eq!(verdicts.candidates.len(), 1);
        let hit = &verdicts.candidates[0];
        assert_eq!(hit.reason.as_str(), "compacted");
        assert_eq!(hit.id, "B-c0ffee11");
        assert_eq!(hit.status, STATUS_RESOLVED);
        // The workaround is the whole point of surfacing an aged-out row.
        assert_eq!(hit.context, COMPACTED_CONTEXT);
    }

    #[test]
    fn rows_sharing_an_id_with_different_fingerprints_are_reported() {
        // Same id, different fingerprint — what a text merge of two worktrees
        // leaves behind.
        let second = live_row(
            "B-a1b2c3d4",
            KIND_BUG,
            "tomlctl/src/io.rs",
            "guard_write_path refuses a symlinked leaf",
            "",
        );
        let doc = store(
            vec![
                live_row(
                    "B-a1b2c3d4",
                    KIND_FLAKY_TEST,
                    FLAKE_AREA,
                    FLAKE_SUMMARY,
                    FLAKE_CONTEXT,
                ),
                second,
            ],
            vec![],
        );
        let (verdict, candidates) =
            verdict_of(&doc, &probe("nothing whatever in common", "", KIND_BUG));
        assert_eq!(verdict, "duplicate-id");
        assert_eq!(candidates.len(), 2);
        for (id, reason, _) in &candidates {
            assert_eq!(id, "B-a1b2c3d4");
            assert_eq!(reason, "duplicate-id");
        }
    }

    #[test]
    fn one_id_with_one_fingerprint_is_not_a_collision() {
        let row = live_row(
            "B-a1b2c3d4",
            KIND_FLAKY_TEST,
            FLAKE_AREA,
            FLAKE_SUMMARY,
            FLAKE_CONTEXT,
        );
        let doc = store(vec![row.clone(), row], vec![]);
        assert_eq!(
            verdict_of(&doc, &probe("nothing whatever in common", "", KIND_BUG)).0,
            VERDICT_NOVEL
        );
    }

    #[test]
    fn an_empty_store_is_novel() {
        for doc in [store(vec![], vec![]), TomlValue::Table(toml::Table::new())] {
            let (verdict, candidates) =
                verdict_of(&doc, &probe(FLAKE_SUMMARY, FLAKE_AREA, KIND_FLAKY_TEST));
            assert_eq!(verdict, VERDICT_NOVEL);
            assert!(candidates.is_empty());
        }
    }

    #[test]
    fn a_missing_store_reads_as_novel_unless_strict() {
        let (_root, (lenient, strict)) = under_root(|root| {
            fs::create_dir_all(root.join(".claude")).unwrap();
            let lenient = dispatch(
                FLAKE_SUMMARY.to_owned(),
                Some(FLAKE_AREA.to_owned()),
                Some(KIND_FLAKY_TEST.to_owned()),
                vec![],
                5,
                None,
                None,
                read_args(),
            );
            let mut args = read_args();
            args.strict_read = true;
            let strict = dispatch(
                FLAKE_SUMMARY.to_owned(),
                None,
                None,
                vec![],
                5,
                None,
                None,
                args,
            );
            (lenient, strict)
        });
        assert!(lenient.is_ok(), "{:#}", lenient.unwrap_err());
        assert_eq!(kind_of(&strict.unwrap_err()), "not_found");
    }

    #[test]
    fn evidence_files_counts_the_directory_at_read_time() {
        let doc = populated();
        let probe = probe(FLAKE_SUMMARY, FLAKE_AREA, KIND_FLAKY_TEST);
        let verdicts = evaluate(&doc, &probe, &defaults());

        let (_root, (absent, populated_dir)) = under_root(|root| {
            let dir = root
                .join(".claude")
                .join(evidence::EVIDENCE_ROOT_NAME)
                .join("B-a1b2c3d4");
            let absent = render(&probe, &defaults(), &verdicts).unwrap();
            fs::create_dir_all(&dir).unwrap();
            fs::write(
                dir.join(evidence::MARKER_NAME),
                evidence::marker_text("B-a1b2c3d4", FLAKE_SUMMARY, Some(true)),
            )
            .unwrap();
            fs::write(dir.join("probe.log"), b"tail").unwrap();
            fs::write(dir.join("run-2026-09-01.png"), b"bytes").unwrap();
            let present = render(&probe, &defaults(), &verdicts).unwrap();
            (absent, present)
        });

        assert_eq!(absent["verdict"], "duplicate");
        assert_eq!(absent["candidates"][0]["evidence_files"], 0);
        assert_eq!(populated_dir["candidates"][0]["evidence_files"], 2);
    }

    #[test]
    fn the_envelope_carries_the_fingerprint_and_the_thresholds_in_force() {
        let probe = probe(FLAKE_SUMMARY, FLAKE_AREA, KIND_FLAKY_TEST);
        let thresholds = Thresholds {
            strong: 0.9,
            related: 0.1,
        };
        let verdicts = Verdicts {
            verdict: VERDICT_NOVEL,
            candidates: vec![],
        };
        let (_root, envelope) = under_root(|_| render(&probe, &thresholds, &verdicts).unwrap());
        assert_eq!(envelope["dedup_id"], probe.dedup_id);
        assert_eq!(envelope["thresholds"]["strong"], 0.9);
        assert_eq!(envelope["thresholds"]["related"], 0.1);
        assert_eq!(envelope["candidates"], json!([]));
    }

    #[test]
    fn lowering_the_strong_threshold_promotes_a_related_hit() {
        let doc = store(
            vec![live_row(
                "B-a1b2c3d4",
                KIND_BUG,
                "lumina/web/src/checkout/Total.vue",
                "checkout total overlaps the confirm button",
                "",
            )],
            vec![],
        );
        let probe = probe(
            "checkout total covers the confirm control",
            "lumina/web/src/checkout/Total.vue",
            KIND_BUG,
        );
        assert_eq!(evaluate(&doc, &probe, &defaults()).verdict, "related");
        let loosened = Thresholds {
            strong: 0.2,
            related: SIMILARITY_RELATED,
        };
        assert_eq!(
            evaluate(&doc, &probe, &loosened).verdict,
            "likely-duplicate"
        );
    }

    #[test]
    fn an_out_of_range_threshold_is_a_validation_error() {
        assert_eq!(
            threshold("--similarity-strong", None, SIMILARITY_STRONG).unwrap(),
            SIMILARITY_STRONG
        );
        for given in [-0.1, 1.1, f64::NAN] {
            let err = threshold("--similarity-strong", Some(given), SIMILARITY_STRONG).unwrap_err();
            assert_eq!(kind_of(&err), "validation", "{given}");
        }
        assert_eq!(
            threshold("--similarity-related", Some(0.0), SIMILARITY_RELATED).unwrap(),
            0.0
        );
    }

    #[test]
    fn the_cap_trims_candidates_without_changing_the_verdict() {
        let doc = store(
            (0..4)
                .map(|n| {
                    live_row(
                        &format!("B-0000000{n}"),
                        KIND_BUG,
                        &format!("tomlctl/src/backlog/leaf{n}.rs"),
                        "the sidecar rename fails with access denied",
                        "",
                    )
                })
                .collect(),
            vec![],
        );
        let probe = probe(
            "the sidecar rename fails with access denied",
            "tomlctl/src/backlog/check.rs",
            KIND_BUG,
        );
        let mut verdicts = evaluate(&doc, &probe, &defaults());
        assert_eq!(verdicts.candidates.len(), 4);
        verdicts.cap(2);
        assert_eq!(verdicts.verdict, "likely-duplicate");
        assert_eq!(verdicts.candidates.len(), 2);
    }

    #[test]
    fn candidates_are_ordered_by_descending_score() {
        let doc = store(
            vec![
                live_row(
                    "B-11111111",
                    KIND_BUG,
                    "tomlctl/src/backlog/add.rs",
                    "the sidecar rename fails with access denied on windows",
                    "",
                ),
                live_row(
                    "B-22222222",
                    KIND_BUG,
                    "tomlctl/src/backlog/query.rs",
                    "the sidecar rename fails intermittently",
                    "",
                ),
            ],
            vec![],
        );
        let probe = probe(
            "the sidecar rename fails with access denied on windows",
            "tomlctl/src/backlog/check.rs",
            KIND_BUG,
        );
        let verdicts = evaluate(&doc, &probe, &defaults());
        let scores: Vec<f64> = verdicts.candidates.iter().map(|c| c.score).collect();
        assert_eq!(verdicts.candidates.len(), 2);
        assert_eq!(verdicts.candidates[0].id, "B-11111111");
        assert!(scores[0] > scores[1], "{scores:?}");
    }

    #[test]
    fn a_stronger_rung_outranks_a_higher_score() {
        let doc = store(
            vec![
                live_row(
                    "B-11111111",
                    KIND_BUG,
                    "tomlctl/src/backlog/add.rs",
                    "renamings sidecars denials",
                    "",
                ),
                live_row(
                    "B-22222222",
                    KIND_BUG,
                    "lumina/server/src/pty/spawn.rs",
                    "renaming the sidecar denied while indexing scans locked archives",
                    "",
                ),
            ],
            vec![],
        );
        let probe = probe(
            "renaming the sidecar denied",
            "tomlctl/src/backlog/check.rs",
            KIND_BUG,
        );
        let (_, candidates) = verdict_of(&doc, &probe);
        assert_eq!(
            candidates
                .iter()
                .map(|(id, reason, _)| (id.as_str(), reason.as_str()))
                .collect::<Vec<_>>(),
            vec![("B-22222222", "words"), ("B-11111111", "area")]
        );
        assert!(candidates[0].2 < candidates[1].2, "{candidates:?}");
    }

    #[test]
    fn round4_keeps_a_threshold_comparison_readable() {
        assert_eq!(round4(2.0 / 3.0), 0.6667);
        assert_eq!(round4(1.0), 1.0);
    }
}
