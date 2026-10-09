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

use std::cell::OnceCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::Result;
use serde_json::{Map as JsonMap, Value as JsonValue, json};
use toml::Value as TomlValue;

use super::evidence;
use super::ids::dedup_id_from_parts;
use super::normalise::{
    SIMILARITY_RELATED, SIMILARITY_STRONG, char_trigrams, jaccard, word_tokens,
};
use super::schema::{
    self, ARRAY_BACKLOG, ARRAY_COMPACTED, FIELD_AREA, FIELD_CONTEXT, FIELD_DEDUP_ID, FIELD_ID,
    FIELD_PROMOTED_TO, FIELD_SEEN_COUNT, FIELD_STATUS, FIELD_SUMMARY, FIELD_TAGS, KIND_OTHER,
    STATUS_PROMOTED, coerce_kind,
};
use crate::cli::ReadIntegrityArgs;
use crate::errors::{ErrorKind, tagged_err};
use crate::io::{items_array, read_ndjson_source, read_text_arg};
use crate::output::{Rows, print_report};

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
    InFlight,
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
            Self::InFlight => "in-flight",
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
            Self::InFlight => "in-flight",
            Self::Compacted => "previously-resolved",
            Self::DuplicateId => "duplicate-id",
            Self::Trigram => "likely-duplicate",
            Self::Words | Self::Area | Self::Tags => "related",
        }
    }
}

const VERDICT_NOVEL: &str = "novel";

/// `--summary` value that means "the summary is on stdin".
const SUMMARY_STDIN: &str = "-";

/// The candidate cap when `--limit` is not given.
const DEFAULT_LIMIT: usize = 5;

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
    fn grade(
        &self,
        row: &Row<'_>,
        fold: &RowFold,
        thresholds: &Thresholds,
    ) -> Option<(Reason, f64)> {
        let trigram = jaccard(&self.trigrams, &fold.trigrams);
        let words = jaccard(&self.words, &fold.words);
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
        if shared_leading(&self.area, &fold.area) >= SHARED_STRUCTURE_MIN {
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
    promoted_to: Option<String>,
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
/// list. `array` is what separates `duplicate` from `previously-resolved`, and
/// a live row's `status` separates `duplicate` from `in-flight`.
struct Row<'a> {
    array: &'static str,
    id: &'a str,
    dedup_id: &'a str,
    summary: &'a str,
    area: &'a str,
    status: &'a str,
    context: &'a str,
    promoted_to: &'a str,
    tags: BTreeSet<&'a str>,
    seen_count: i64,
}

fn str_field<'a>(item: &'a TomlValue, key: &str) -> &'a str {
    item.get(key)
        .and_then(TomlValue::as_str)
        .unwrap_or_default()
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
                promoted_to: str_field(item, FIELD_PROMOTED_TO),
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
fn index_by_dedup<'a>(rows: &[Row<'a>]) -> BTreeMap<&'a str, Vec<usize>> {
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

/// A row's probe-independent comparison sets.
struct RowFold {
    trigrams: BTreeSet<String>,
    words: BTreeSet<String>,
    area: Vec<String>,
}

impl RowFold {
    fn new(row: &Row<'_>) -> Self {
        Self {
            trigrams: char_trigrams(row.summary),
            words: word_tokens(row.summary),
            area: components(row.area),
        }
    }
}

/// Everything about a store read that does not depend on the probe. Each
/// row's fold is built on first grade, so a probe that resolves on an exact
/// fingerprint hit folds nothing.
struct Store<'a> {
    rows: Vec<Row<'a>>,
    by_dedup: BTreeMap<&'a str, Vec<usize>>,
    colliding: BTreeSet<usize>,
    folds: Vec<OnceCell<RowFold>>,
}

impl<'a> Store<'a> {
    fn new(doc: &'a TomlValue) -> Self {
        let rows = rows(doc);
        let by_dedup = index_by_dedup(&rows);
        let colliding = colliding_ids(&rows);
        let folds = rows.iter().map(|_| OnceCell::new()).collect();
        Self {
            rows,
            by_dedup,
            colliding,
            folds,
        }
    }
}

fn evaluate(doc: &TomlValue, probe: &Probe, thresholds: &Thresholds) -> Verdicts {
    evaluate_in(&Store::new(doc), probe, thresholds)
}

fn evaluate_in(store: &Store<'_>, probe: &Probe, thresholds: &Thresholds) -> Verdicts {
    let rows = &store.rows;
    let by_dedup = &store.by_dedup;
    let mut graded: BTreeMap<usize, (Reason, f64)> = BTreeMap::new();

    for &position in by_dedup
        .get(probe.dedup_id.as_str())
        .map(Vec::as_slice)
        .unwrap_or_default()
    {
        let row = &rows[position];
        let reason = if row.array != ARRAY_BACKLOG {
            Reason::Compacted
        } else if row.status == STATUS_PROMOTED {
            Reason::InFlight
        } else {
            Reason::DedupId
        };
        graded.insert(position, (reason, 1.0));
    }
    let exact = !graded.is_empty();

    for &position in &store.colliding {
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
            let fold = store.folds[position].get_or_init(|| RowFold::new(row));
            if let Some(grade) = probe.grade(row, fold, thresholds) {
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
                promoted_to: (!row.promoted_to.is_empty()).then(|| row.promoted_to.to_owned()),
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
        let mut entry = json!({
            "id": candidate.id,
            "summary": candidate.summary,
            "score": round4(candidate.score),
            "reason": candidate.reason.as_str(),
            "status": candidate.status,
            "seen_count": candidate.seen_count,
            "context": candidate.context,
            "evidence_files": evidence_count(&candidate.id)?,
        });
        if let Some(target) = &candidate.promoted_to {
            entry[FIELD_PROMOTED_TO] = json!(target);
        }
        candidates.push(entry);
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

/// The summary a caller passed, or the whole of stdin under the `-` sentinel.
///
/// A summary is text somebody else wrote — the caller is relaying a discovery,
/// not composing a command — and a flag value is the one place a shell gets to
/// re-tokenise it. The sentinel is what lets the text stay data end to end.
///
/// One trailing newline is dropped, so a heredoc and a staging file fingerprint
/// identically.
fn resolve_summary(raw: String) -> Result<String> {
    if raw != SUMMARY_STDIN {
        return Ok(raw);
    }
    // Routed through the shared funnel so the single-consumption guard and the
    // stdin byte cap hold on this path too.
    let buf = read_text_arg(SUMMARY_STDIN)?;
    let text = buf.strip_suffix('\n').unwrap_or(&buf);
    let text = text.strip_suffix('\r').unwrap_or(text);
    Ok(text.to_string())
}

/// Refuse a `--summary` of `@<path>` when `<path>`, resolved against `base`,
/// is an existing file. The value would otherwise be probed as the literal
/// string and answer a confident `novel` for a summary nobody wrote, while
/// an `@handle` mention that names no file is still ordinary text.
fn refuse_at_file(raw: &str, base: &Path) -> Result<()> {
    let Some(name) = raw.strip_prefix('@') else {
        return Ok(());
    };
    if name.is_empty() || !base.join(name).is_file() {
        return Ok(());
    }
    Err(tagged_err(
        ErrorKind::Validation,
        None,
        format!(
            "--summary `{raw}` names a file, but backlog check probes --summary as literal \
             text; stage the text and pass `--summary - < {name}`"
        ),
    ))
}

// One parameter per flag on the CLI variant, which is the dispatch contract.
#[allow(clippy::too_many_arguments)]
pub(crate) fn dispatch(
    summary: Option<String>,
    ndjson: Option<String>,
    area: Option<String>,
    kind: Option<String>,
    tag: Vec<String>,
    similarity_strong: Option<f64>,
    similarity_related: Option<f64>,
    integrity: ReadIntegrityArgs,
) -> Result<()> {
    let thresholds = Thresholds {
        strong: threshold("--similarity-strong", similarity_strong, SIMILARITY_STRONG)?,
        related: threshold(
            "--similarity-related",
            similarity_related,
            SIMILARITY_RELATED,
        )?,
    };
    if let Some(src) = ndjson {
        return dispatch_batch(&src, &thresholds, &integrity);
    }
    // The parser requires one of the two, so this is unreachable from the CLI.
    let summary = summary.ok_or_else(|| {
        tagged_err(
            ErrorKind::Validation,
            None,
            "backlog check needs --summary or --ndjson",
        )
    })?;
    refuse_at_file(&summary, Path::new("."))?;
    let summary = resolve_summary(summary)?;
    let probe = Probe::new(&summary, area.as_deref(), kind.as_deref(), &tag);

    let doc = schema::read_store(&integrity)?;

    let verdicts = evaluate(&doc, &probe, &thresholds);
    print_report(
        build_report(&probe, &thresholds, verdicts, crate::output::opts().limit)?,
        Rows::Field("candidates"),
    )
}

/// An explicit `--limit` is left to the output layer, which cuts the rows and
/// reports the cut; without one the default cap applies here and the header
/// carries the same `limited` shape the output layer would have written.
fn build_report(
    probe: &Probe,
    thresholds: &Thresholds,
    mut verdicts: Verdicts,
    limit: Option<usize>,
) -> Result<JsonValue> {
    let mut cut = None;
    if limit.is_none() {
        let total = verdicts.candidates.len();
        verdicts.cap(DEFAULT_LIMIT);
        if total > verdicts.candidates.len() {
            cut = Some((verdicts.candidates.len(), total));
        }
    }
    let mut report = render(probe, thresholds, &verdicts)?;
    if let Some((shown, total)) = cut {
        report["limited"] = json!({ "shown": shown, "total": total });
    }
    Ok(report)
}

/// The keys a batch line may carry. Anything else is refused: a typo'd
/// `are` would otherwise fingerprint the probe without its area and miss
/// the stored row silently.
const BATCH_KEYS: &[&str] = &["summary", "kind", "area", "tags"];

/// One parsed batch line and the 1-based source line it came from.
struct BatchProbe {
    line: usize,
    summary: String,
    probe: Probe,
}

fn batch_refusal(line: usize, message: impl std::fmt::Display) -> anyhow::Error {
    tagged_err(
        ErrorKind::Validation,
        None,
        format!("line {line}: {message}"),
    )
}

fn batch_str(
    payload: &JsonMap<String, JsonValue>,
    key: &str,
    line: usize,
) -> Result<Option<String>> {
    match payload.get(key) {
        None | Some(JsonValue::Null) => Ok(None),
        Some(JsonValue::String(text)) => Ok(Some(text.clone())),
        Some(other) => Err(batch_refusal(
            line,
            format!("`{key}` must be a string; got {other}"),
        )),
    }
}

/// Every line is parsed before the store is read, so a malformed line fails
/// the batch without a partial answer.
fn parse_probes(text: &str) -> Result<Vec<BatchProbe>> {
    let mut probes = Vec::new();
    for (line, payload) in crate::items::parse_ndjson_objects(text)? {
        if let Some(unknown) = payload.keys().find(|k| !BATCH_KEYS.contains(&k.as_str())) {
            return Err(batch_refusal(
                line,
                format!(
                    "unknown key `{unknown}`; a probe carries only {}",
                    BATCH_KEYS.join(", ")
                ),
            ));
        }
        let summary = batch_str(&payload, "summary", line)?
            .ok_or_else(|| batch_refusal(line, "`summary` is required"))?;
        let area = batch_str(&payload, "area", line)?;
        let kind = batch_str(&payload, "kind", line)?;
        let tags = match payload.get("tags") {
            None | Some(JsonValue::Null) => Vec::new(),
            Some(JsonValue::Array(items)) => items
                .iter()
                .map(|item| {
                    item.as_str().map(str::to_owned).ok_or_else(|| {
                        batch_refusal(line, format!("`tags` must hold strings; got {item}"))
                    })
                })
                .collect::<Result<_>>()?,
            Some(other) => {
                return Err(batch_refusal(
                    line,
                    format!("`tags` must be an array of strings; got {other}"),
                ));
            }
        };
        let probe = Probe::new(&summary, area.as_deref(), kind.as_deref(), &tags);
        probes.push(BatchProbe {
            line,
            summary,
            probe,
        });
    }
    if probes.is_empty() {
        return Err(tagged_err(
            ErrorKind::Validation,
            None,
            "the NDJSON source carries no probes",
        ));
    }
    Ok(probes)
}

/// One result row per probe, every row graded against the same store read.
/// The per-row candidate cap is the default one whatever `--limit` says,
/// because the output layer applies `--limit` to the result rows instead.
fn batch_report(
    doc: &TomlValue,
    probes: &[BatchProbe],
    thresholds: &Thresholds,
) -> Result<JsonValue> {
    let mut results = Vec::with_capacity(probes.len());
    let store = Store::new(doc);
    for batch in probes {
        let verdicts = evaluate_in(&store, &batch.probe, thresholds);
        let report = build_report(&batch.probe, thresholds, verdicts, None)?;
        let mut row = JsonMap::new();
        row.insert("line".to_owned(), json!(batch.line));
        row.insert("summary".to_owned(), json!(batch.summary));
        for key in ["verdict", "dedup_id", "candidates", "limited"] {
            if let Some(value) = report.get(key) {
                row.insert(key.to_owned(), value.clone());
            }
        }
        results.push(JsonValue::Object(row));
    }
    Ok(json!({
        "thresholds": {"strong": thresholds.strong, "related": thresholds.related},
        "results": results,
    }))
}

fn dispatch_batch(src: &str, thresholds: &Thresholds, integrity: &ReadIntegrityArgs) -> Result<()> {
    let probes = parse_probes(&read_ndjson_source(src)?)?;
    let doc = schema::read_store(integrity)?;
    print_report(
        batch_report(&doc, &probes, thresholds)?,
        Rows::Field("results"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backlog::schema::{
        COMPACTED_FIELDS, FIELD_COMPACTED_ON, FIELD_KIND, FIELD_PROMOTED, FIELD_TERMINAL_DATE,
        FIELD_TERMINAL_REASON, KIND_BUG, KIND_FLAKY_TEST, STATUS_OPEN, STATUS_RESOLVED,
    };
    use crate::test_support::with_root;
    use std::fs;

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
    fn a_fingerprint_hit_on_a_promoted_row_is_in_flight() {
        let mut row = live_row(
            "B-a1b2c3d4",
            KIND_FLAKY_TEST,
            FLAKE_AREA,
            FLAKE_SUMMARY,
            FLAKE_CONTEXT,
        );
        let fields = row.as_table_mut().unwrap();
        for (key, value) in [
            (FIELD_STATUS, STATUS_PROMOTED),
            (FIELD_PROMOTED, "2026-09-20"),
            (FIELD_PROMOTED_TO, "docs/plans/pty-readiness.md"),
        ] {
            fields.insert(key.to_owned(), TomlValue::String(value.to_owned()));
        }
        let doc = store(vec![row], vec![]);
        let probe = probe(FLAKE_SUMMARY, FLAKE_AREA, KIND_FLAKY_TEST);
        let verdicts = evaluate(&doc, &probe, &defaults());
        assert_eq!(verdicts.verdict, "in-flight");
        assert_eq!(verdicts.candidates[0].reason.as_str(), "in-flight");

        let envelope = with_root(|_| render(&probe, &defaults(), &verdicts).unwrap());
        assert_eq!(
            envelope["candidates"][0][FIELD_PROMOTED_TO],
            "docs/plans/pty-readiness.md"
        );
    }

    #[test]
    fn an_open_row_carries_no_promotion_target() {
        let doc = populated();
        let probe = probe(FLAKE_SUMMARY, FLAKE_AREA, KIND_FLAKY_TEST);
        let verdicts = evaluate(&doc, &probe, &defaults());
        let envelope = with_root(|_| render(&probe, &defaults(), &verdicts).unwrap());
        assert!(envelope["candidates"][0].get(FIELD_PROMOTED_TO).is_none());
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
        let (lenient, strict) = with_root(|_| {
            let lenient = dispatch(
                Some(FLAKE_SUMMARY.to_owned()),
                None,
                Some(FLAKE_AREA.to_owned()),
                Some(KIND_FLAKY_TEST.to_owned()),
                vec![],
                None,
                None,
                read_args(),
            );
            let mut args = read_args();
            args.strict_read = true;
            let strict = dispatch(
                Some(FLAKE_SUMMARY.to_owned()),
                None,
                None,
                None,
                vec![],
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

        let (absent, populated_dir) = with_root(|root| {
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
        let envelope = with_root(|_| render(&probe, &thresholds, &verdicts).unwrap());
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
    fn an_at_summary_naming_an_existing_file_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("notes.txt"), "the real summary\n").unwrap();

        let err = refuse_at_file("@notes.txt", dir.path()).unwrap_err();
        assert_eq!(kind_of(&err), "validation");
        assert!(err.to_string().contains("--summary - < notes.txt"), "{err}");
    }

    #[test]
    fn an_at_mention_naming_no_file_stays_literal() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("handle")).unwrap();

        for raw in ["@handle mention", "@handle", "@", "plain text"] {
            assert!(refuse_at_file(raw, dir.path()).is_ok(), "{raw}");
        }
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
    fn the_default_cap_reports_a_limited_header_only_when_it_cuts() {
        let rows = |count: usize| {
            store(
                (0..count)
                    .map(|n| {
                        live_row(
                            &format!("B-{n:08x}"),
                            KIND_BUG,
                            &format!("tomlctl/src/backlog/leaf{n}.rs"),
                            "the sidecar rename fails with access denied",
                            "",
                        )
                    })
                    .collect(),
                vec![],
            )
        };
        let probe = probe(
            "the sidecar rename fails with access denied",
            "tomlctl/src/backlog/check.rs",
            KIND_BUG,
        );
        let over = DEFAULT_LIMIT + 2;
        let (cut, whole, explicit) = with_root(|_| {
            let build = |count: usize, limit: Option<usize>| {
                let verdicts = evaluate(&rows(count), &probe, &defaults());
                build_report(&probe, &defaults(), verdicts, limit).unwrap()
            };
            (
                build(over, None),
                build(DEFAULT_LIMIT - 1, None),
                build(over, Some(2)),
            )
        });
        assert_eq!(cut["candidates"].as_array().unwrap().len(), DEFAULT_LIMIT);
        assert_eq!(
            cut["limited"],
            json!({ "shown": DEFAULT_LIMIT, "total": over })
        );
        assert!(whole.get("limited").is_none());
        assert_eq!(explicit["candidates"].as_array().unwrap().len(), over);
        assert!(explicit.get("limited").is_none());
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
    fn a_batch_grades_each_probe_on_its_own_line() {
        let text = format!(
            "{}\n\n{}\n",
            json!({"summary": FLAKE_SUMMARY, "kind": KIND_FLAKY_TEST, "area": FLAKE_AREA}),
            json!({"summary": "nothing whatever in common", "tags": ["ci"]}),
        );
        let probes = parse_probes(&text).unwrap();
        let report = with_root(|_| batch_report(&populated(), &probes, &defaults()).unwrap());
        let results = report["results"].as_array().unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0]["line"], 1);
        assert_eq!(results[0]["verdict"], "duplicate");
        assert_eq!(results[0]["candidates"][0]["id"], "B-a1b2c3d4");
        assert_eq!(results[1]["line"], 3);
        assert_eq!(results[1]["summary"], "nothing whatever in common");
        assert_eq!(results[1]["verdict"], VERDICT_NOVEL);
        assert_eq!(report["thresholds"]["strong"], SIMILARITY_STRONG);
    }

    #[test]
    fn a_batch_row_caps_its_candidates_at_the_default() {
        let doc = store(
            (0..DEFAULT_LIMIT + 2)
                .map(|n| {
                    live_row(
                        &format!("B-{n:08x}"),
                        KIND_BUG,
                        &format!("tomlctl/src/backlog/leaf{n}.rs"),
                        "the sidecar rename fails with access denied",
                        "",
                    )
                })
                .collect(),
            vec![],
        );
        let probes = parse_probes(
            &json!({
                "summary": "the sidecar rename fails with access denied",
                "area": "tomlctl/src/backlog/check.rs",
                "kind": KIND_BUG,
            })
            .to_string(),
        )
        .unwrap();
        let report = with_root(|_| batch_report(&doc, &probes, &defaults()).unwrap());
        let row = &report["results"][0];
        assert_eq!(row["candidates"].as_array().unwrap().len(), DEFAULT_LIMIT);
        assert_eq!(
            row["limited"],
            json!({ "shown": DEFAULT_LIMIT, "total": DEFAULT_LIMIT + 2 })
        );
    }

    #[test]
    fn a_bad_batch_line_is_refused_by_its_line_number() {
        let cases = [
            ("{\"summary\":\"ok\"}\n{\"summary\":", "line 2: "),
            ("[1]", "line 1: "),
            ("\n{\"kind\":\"bug\"}", "line 2: `summary` is required"),
            (
                "{\"summary\":\"x\",\"are\":\"y\"}",
                "line 1: unknown key `are`",
            ),
            (
                "{\"summary\":\"x\",\"tags\":\"ci\"}",
                "line 1: `tags` must be an array",
            ),
            (
                "{\"summary\":\"x\",\"tags\":[1]}",
                "line 1: `tags` must hold strings",
            ),
            ("{\"summary\":3}", "line 1: `summary` must be a string"),
            ("\n  \n", "carries no probes"),
        ];
        for (text, expected) in cases {
            let err = parse_probes(text)
                .err()
                .unwrap_or_else(|| panic!("{text:?}"));
            assert_eq!(kind_of(&err), "validation", "{text:?}");
            let message = format!("{err:#}");
            assert!(message.contains(expected), "{text:?}: {message}");
        }
    }

    #[test]
    fn round4_keeps_a_threshold_comparison_readable() {
        assert_eq!(round4(2.0 / 3.0), 0.6667);
        assert_eq!(round4(1.0), 1.0);
    }
}
