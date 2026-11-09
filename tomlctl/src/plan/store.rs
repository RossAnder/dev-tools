//! Store path resolution and locked load/mutate over a flow's `plan.toml`,
//! and the digest that decides whether a brief has changed.
//!
//! `mutate` converts through `schema::from_toml` / `schema::to_toml` inside
//! the `io::mutate_doc` closure, so the exclusive lock and the sidecar write
//! apply unchanged. Every mutation restamps `last_updated`; nothing else writes
//! it.
//!
//! A flow is v2 because `plan.toml` exists, so `mutate` never creates the file:
//! seeding it would silently turn a v1 flow into a v2 one. Only `plan open`
//! bootstraps it.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::Value as JsonValue;
use toml::Value as TomlValue;

use super::schema::{self, PlanStore, Run, RunState};
use crate::cli::{
    ReadIntegrityArgs, WriteIntegrityArgs, read_integrity_opts, write_integrity_opts,
};
use crate::errors::{ErrorKind, tagged_err};
use crate::integrity::sha256_hex_crlf_folded;
use crate::io::{OnMissing, mutate_doc, read_doc, read_toml, repo_or_cwd_root, strict_read_check};
use crate::tasks::schema::{Accept, Store};

const STORE_FILE: &str = "plan.toml";
pub(super) const BRIEF_FILE: &str = "brief.md";
pub(super) const REVIEW_STORE_FILE: &str = "review-v2.toml";
const PLAN_FAMILY: &str = "plan";
const REVIEW_STEP: &str = "review";

/// `<root>/.claude/flows/<slug>/plan.toml`. The flow-init slug rule admits
/// neither a separator nor a `..`, which is the only thing keeping `--slug`
/// from reaching outside `.claude/flows/`.
pub(crate) fn resolve(slug: &str) -> Result<PathBuf> {
    crate::flow::validate_slug(slug)?;
    Ok(repo_or_cwd_root()?
        .join(".claude")
        .join("flows")
        .join(slug)
        .join(STORE_FILE))
}

/// `<root>/docs/flows/<slug>`, holding the files agents author with Write and
/// Edit: `brief.md`, `design/` and `notes/`. Writes under `.claude/` are protected
/// paths that no allow rule can pre-approve, so these live outside it. The slug
/// rule keeps `--slug` from reaching outside `docs/flows/`.
pub(crate) fn authored_dir(slug: &str) -> Result<PathBuf> {
    crate::flow::validate_slug(slug)?;
    Ok(repo_or_cwd_root()?.join("docs").join("flows").join(slug))
}

/// The containment anchor for authored files: the root's real path joined with
/// `authored`'s spelling, so a symlink at any directory below the root cannot move
/// the anchor along with the files it contains. A path outside the root, which only
/// a unit test builds, anchors on its own real path.
pub(super) fn authored_anchor(authored: &Path) -> Result<PathBuf> {
    let root = repo_or_cwd_root()?;
    match authored.strip_prefix(&root) {
        Ok(relative) => Ok(root
            .canonicalize()
            .with_context(|| format!("resolving {}", root.display()))?
            .join(relative)),
        Err(_) => authored
            .canonicalize()
            .with_context(|| format!("resolving {}", authored.display())),
    }
}

/// The brief's bytes, `None` when it is absent or resolves outside `authored`, so a
/// symlink cannot inline another file into a `plan view` prompt.
pub(super) fn read_brief(authored: &Path) -> Result<Option<Vec<u8>>> {
    let path = authored.join(BRIEF_FILE);
    let resolved = match path.canonicalize() {
        Ok(resolved) => resolved,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err).with_context(|| format!("resolving {}", path.display())),
    };
    if !resolved.starts_with(authored_anchor(authored)?) {
        return Ok(None);
    }
    fs::read(&resolved)
        .map(Some)
        .with_context(|| format!("reading {}", path.display()))
}

/// The flow directory holding a store path `resolve` built.
pub(super) fn flow_dir(plan_path: &Path) -> &Path {
    plan_path
        .parent()
        .expect("a resolved store path sits in its flow directory")
}

/// The plan-family part of a flow's `review-v2.toml`, which `plan gate`, `plan approve`
/// and `plan stage` all read so that none of them can disagree with another.
#[derive(Debug, Default, Clone, PartialEq)]
pub(super) struct PlanReviewState {
    pub(super) finished_runs: Vec<String>,
    pub(super) unfinished_runs: Vec<String>,
    pub(super) applied: Vec<AppliedEdit>,
    pub(super) open: Vec<OpenReviewRow>,
}

/// A plan finding whose edit `review apply` wrote, with `old` and `new` decoded.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct AppliedEdit {
    pub(super) id: String,
    pub(super) target: String,
    pub(super) tier: String,
    pub(super) by: String,
    pub(super) summary: String,
    pub(super) old: JsonValue,
    pub(super) new: JsonValue,
}

/// An open plan finding. `recheck` marks one minted by a run that re-checks
/// another; empty strings stand for absent fields.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct OpenReviewRow {
    pub(super) id: String,
    pub(super) kind: String,
    pub(super) severity: String,
    pub(super) tier: String,
    pub(super) target: String,
    pub(super) summary: String,
    pub(super) recheck: bool,
}

impl PlanReviewState {
    /// Open findings whose tier asks the user, which `--review ran` refuses.
    pub(super) fn open_user_tier(&self) -> impl Iterator<Item = &OpenReviewRow> {
        self.open.iter().filter(|f| f.tier == TIER_USER)
    }

    /// Applied findings that wrote a `spec.accept` entry still stored without a
    /// probe. An entry since changed again or removed is no longer theirs to name.
    pub(super) fn unprobed_accept_edits(&self, tasks: &Store) -> Vec<&str> {
        self.applied
            .iter()
            .filter(|finding| {
                let Some(r#ref) = accept_target_ref(&finding.target) else {
                    return false;
                };
                let Some(spec) = tasks.spec_for(r#ref) else {
                    return false;
                };
                written_entries(&finding.new).any(|entry| {
                    spec.accept
                        .iter()
                        .any(|stored| stored.probe.is_none() && entry_matches(entry, stored))
                })
            })
            .map(|finding| finding.id.as_str())
            .collect()
    }

    /// Why review cannot count as run: no finished plan-family run, an unfinished
    /// one, an open user-tier finding, or an applied accept edit left unprobed.
    /// `None` when it is settled.
    pub(super) fn unsettled(&self, tasks: &Store) -> Option<String> {
        if self.finished_runs.is_empty() {
            return Some(format!(
                "no finished plan-family run in `{REVIEW_STORE_FILE}`"
            ));
        }
        let mut reasons = Vec::new();
        if !self.unfinished_runs.is_empty() {
            reasons.push(format!(
                "unfinished plan-family runs: {}",
                self.unfinished_runs.join(", ")
            ));
        }
        let user: Vec<&str> = self.open_user_tier().map(|f| f.id.as_str()).collect();
        if !user.is_empty() {
            reasons.push(format!(
                "open user-tier findings, to accept or veto: {}",
                user.join(", ")
            ));
        }
        let unprobed = self.unprobed_accept_edits(tasks);
        if !unprobed.is_empty() {
            reasons.push(format!(
                "applied findings left a `spec.accept` entry unprobed, so run `plan probes`: {}",
                unprobed.join(", ")
            ));
        }
        (!reasons.is_empty()).then(|| reasons.join("; "))
    }
}

/// The row ref of a `task:<ref>.spec.accept` target, with or without an `[n]` or
/// `[+]` selector.
fn accept_target_ref(target: &str) -> Option<&str> {
    let rest = target.strip_prefix("task:")?;
    let (r#ref, tail) = rest.split_once(".spec.accept")?;
    (tail.is_empty() || tail.starts_with('[')).then_some(r#ref)
}

/// The accept entries a `new` value wrote: one entry, every entry of a whole-list
/// replacement, or none for a removal.
fn written_entries(new: &JsonValue) -> impl Iterator<Item = &JsonValue> {
    let entries = match new {
        JsonValue::Array(entries) => entries.as_slice(),
        JsonValue::Object(_) => std::slice::from_ref(new),
        _ => &[],
    };
    entries.iter().filter(|entry| entry.is_object())
}

/// Whether a written entry describes `stored`: every field it carries agrees, and
/// it carries at least one.
fn entry_matches(entry: &JsonValue, stored: &Accept) -> bool {
    let strings = [
        ("behaviour", &stored.behaviour),
        ("check", &stored.check),
        ("kind", &stored.kind),
        ("positive", &stored.positive),
    ];
    let mut compared = false;
    for (key, value) in strings {
        if let Some(given) = entry.get(key) {
            compared = true;
            if given.as_str() != Some(value.as_str()) {
                return false;
            }
        }
    }
    if let Some(given) = entry.get("writes_build") {
        compared = true;
        if given.as_bool() != Some(stored.writes_build) {
            return false;
        }
    }
    compared
}

impl OpenReviewRow {
    /// A critical or major defect, or anything a re-check run raised.
    pub(super) fn is_gate_risk(&self) -> bool {
        self.recheck
            || (self.kind == KIND_DEFECT && matches!(self.severity.as_str(), "critical" | "major"))
    }
}

pub(super) const TIER_USER: &str = "user";
const KIND_DEFECT: &str = "defect";
const STATUS_OPEN: &str = "open";

/// Reads `review-v2.toml` as a plain document, since the review group owns its shape;
/// a flow without one has an empty state. Only plan-family runs and findings count.
pub(super) fn plan_review_state(flow_dir: &Path) -> Result<PlanReviewState> {
    let path = flow_dir.join(REVIEW_STORE_FILE);
    if !path.exists() {
        return Ok(PlanReviewState::default());
    }
    let doc = read_toml(&path)?;
    let rows = |key: &str| {
        doc.get(key)
            .and_then(TomlValue::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default()
    };
    let mut state = PlanReviewState::default();
    let mut rechecking = Vec::new();
    for run in rows("runs") {
        if text(run, "family") != PLAN_FAMILY {
            continue;
        }
        let id = text(run, "id").to_string();
        if !text(run, "rechecks").is_empty() {
            rechecking.push(id.clone());
        }
        if text(run, "finished").is_empty() {
            state.unfinished_runs.push(id);
        } else {
            state.finished_runs.push(id);
        }
    }
    for finding in rows("findings") {
        if text(finding, "family") != PLAN_FAMILY {
            continue;
        }
        if let Some(applied) = finding.get("applied").filter(|v| v.is_table()) {
            state.applied.push(AppliedEdit {
                id: text(finding, "id").to_string(),
                target: text(applied, "target").to_string(),
                tier: Some(text(applied, "tier"))
                    .filter(|tier| !tier.is_empty())
                    .unwrap_or_else(|| text(finding, "tier"))
                    .to_string(),
                by: text(applied, "by").to_string(),
                summary: text(finding, "summary").to_string(),
                old: json_text(applied.get("old")),
                new: json_text(applied.get("new")),
            });
        }
        if text(finding, "status") == STATUS_OPEN {
            state.open.push(OpenReviewRow {
                id: text(finding, "id").to_string(),
                kind: text(finding, "kind").to_string(),
                severity: text(finding, "severity").to_string(),
                tier: text(finding, "tier").to_string(),
                target: text(finding, "target").to_string(),
                summary: text(finding, "summary").to_string(),
                recheck: rechecking.iter().any(|run| run == text(finding, "run")),
            });
        }
    }
    Ok(state)
}

fn text<'a>(row: &'a TomlValue, key: &str) -> &'a str {
    row.get(key).and_then(TomlValue::as_str).unwrap_or("")
}

/// The review store keeps record values as JSON text, since TOML holds no null; text
/// that does not parse is kept as a string, and an absent value reads as null.
fn json_text(value: Option<&TomlValue>) -> JsonValue {
    match value.and_then(TomlValue::as_str) {
        Some(raw) => {
            serde_json::from_str(raw).unwrap_or_else(|_| JsonValue::String(raw.to_string()))
        }
        None => JsonValue::Null,
    }
}

/// The review step's runs recorded as skipped, in store order.
pub(super) fn skipped_reviews(plan: &PlanStore) -> impl Iterator<Item = &Run> {
    plan.runs
        .iter()
        .filter(|run| run.step == REVIEW_STEP && run.state() == RunState::Skipped)
}

/// Why `plan approve` would refuse the plan's surfaced decisions, `None` when every
/// one is answered.
pub(super) fn unanswered_decisions(plan: &PlanStore) -> Option<String> {
    let unanswered: Vec<&str> = plan
        .decisions
        .iter()
        .filter(|d| d.is_unanswered())
        .map(|d| d.id.as_str())
        .collect();
    (!unanswered.is_empty()).then(|| {
        format!(
            "surfaced decisions still agent-assumed, so unanswered: {}",
            unanswered.join(", ")
        )
    })
}

/// The plan's open runs in store order, so the oldest row comes first.
pub(super) fn open_runs(plan: &PlanStore) -> Vec<&Run> {
    plan.runs.iter().filter(|run| run.is_open()).collect()
}

/// An open run may be alive or dead, and only the caller can tell, so the choice is
/// the caller's.
pub(super) fn relaunch_or_abandon(step: &str) -> String {
    format!("relaunch step {step} or plan run abandon --step {step} --reason <why>")
}

/// Read the store at `path`. Under `--verify-integrity` the sidecar check
/// runs first, under a shared lock, so a write mid-swap cannot be read as a
/// mismatch.
pub(crate) fn load(path: &Path, integrity: &ReadIntegrityArgs) -> Result<PlanStore> {
    require_open(path)?;
    strict_read_check(path, integrity.strict_read)?;
    read_doc(path, read_integrity_opts(integrity), |doc| {
        parse_store(path, doc)
    })
}

/// `schema::from_toml` with its failure tagged `kind=parse` and carrying the path,
/// so a store that parses but fails validation reports like one that does not parse.
fn parse_store(path: &Path, doc: &TomlValue) -> Result<PlanStore> {
    schema::from_toml(doc).map_err(|err| {
        tagged_err(
            ErrorKind::Parse,
            Some(path.to_path_buf()),
            format!("parsing {}: {err:#}", path.display()),
        )
    })
}

/// Refuses a flow with no `plan.toml`, for readers and writers alike. Nothing here
/// can tell a mistyped slug from a v1 flow or a flow not yet opened, so the message
/// covers all three. Runs ahead of the write pipeline, so a mistyped slug leaves no
/// directory behind.
pub(crate) fn require_open(path: &Path) -> Result<()> {
    if path.is_file() {
        return Ok(());
    }
    let slug = path
        .parent()
        .and_then(Path::file_name)
        .map(|name| name.to_string_lossy())
        .unwrap_or_default();
    Err(tagged_err(
        ErrorKind::NotFound,
        Some(path.to_path_buf()),
        format!(
            "flow `{slug}` has no plan.toml; run `tomlctl plan open --slug {slug}` first, the \
             only verb that makes a flow v2. If the slug is mistyped, correct it; a v1 flow \
             stays v1, because `plan open` refuses one"
        ),
    ))
}

/// Run `f` over the parsed store inside the write pipeline's exclusive lock
/// and write the result back with `last_updated` restamped. `f` returning
/// `Err` persists nothing, and a missing store is refused (`require_open`).
pub(crate) fn mutate<T>(
    path: &Path,
    integrity: &WriteIntegrityArgs,
    f: impl FnOnce(&mut PlanStore) -> Result<T>,
) -> Result<T> {
    require_open(path)?;
    let today = crate::time::today_toml_date()?;
    let mut outcome = None;
    mutate_doc(
        path,
        integrity.allow_outside,
        write_integrity_opts(integrity),
        OnMissing::Error,
        |doc| {
            let mut store = parse_store(path, doc)?;
            outcome = Some(f(&mut store)?);
            store.last_updated = Some(today);
            *doc = schema::to_toml(&store);
            Ok(())
        },
    )?;
    Ok(outcome.expect("mutate_doc reports success only after running the closure"))
}

/// SHA-256 of a brief with CRLF folded to LF, so that a checkout's line endings
/// never read as a change to intent. Lone CRs are content and stay.
pub(crate) fn brief_digest(bytes: &[u8]) -> String {
    sha256_hex_crlf_folded(bytes)
}

#[cfg(test)]
mod tests {
    use super::super::schema::tests::fixture;
    use super::*;
    use crate::errors::TaggedError;
    use crate::integrity::{hex_lower, sidecar_path, verify_integrity};
    use crate::test_support::with_root;
    use sha2::{Digest, Sha256};

    const SLUG: &str = "whimsical-hugging-puppy";

    fn read_args(verify: bool) -> ReadIntegrityArgs {
        ReadIntegrityArgs {
            verify_integrity: verify,
            strict_read: false,
        }
    }

    fn write_args() -> WriteIntegrityArgs {
        WriteIntegrityArgs {
            allow_outside: false,
            no_write_integrity: false,
            verify_integrity: false,
            strict_integrity: false,
            no_create: false,
        }
    }

    fn store_path(root: &Path) -> PathBuf {
        root.join(".claude")
            .join("flows")
            .join(SLUG)
            .join(STORE_FILE)
    }

    fn seed_store(path: &Path) {
        std::fs::create_dir_all(path.parent().expect("a flow dir")).unwrap();
        std::fs::write(path, "schema_version = 1\n").unwrap();
    }

    #[test]
    fn load_reads_back_every_record_mutate_wrote() {
        with_root(|root| {
            let path = store_path(root);
            seed_store(&path);
            let written = fixture();
            mutate(&path, &write_args(), |store| {
                *store = written.clone();
                Ok(())
            })
            .expect("the existing store is written");

            assert!(
                sidecar_path(&path).exists(),
                "the same write must leave a .sha256 sidecar"
            );
            verify_integrity(&path).expect("the sidecar matches the written bytes");

            let read = load(&path, &read_args(true)).expect("the store loads");
            let stamped = read.last_updated;
            assert_ne!(
                stamped, written.last_updated,
                "every mutation restamps last_updated"
            );
            assert_eq!(
                read,
                schema::PlanStore {
                    last_updated: stamped,
                    ..written
                }
            );
        });
    }

    #[test]
    fn a_failing_closure_leaves_the_store_untouched() {
        with_root(|root| {
            let path = store_path(root);
            seed_store(&path);
            let before = std::fs::read(&path).unwrap();
            let err = mutate(&path, &write_args(), |_store| {
                Err::<(), _>(anyhow::anyhow!("refused"))
            })
            .expect_err("the closure's error propagates");
            assert!(err.to_string().contains("refused"), "{err}");
            assert_eq!(
                std::fs::read(&path).unwrap(),
                before,
                "a refused mutation must persist nothing"
            );
        });
    }

    #[test]
    fn a_missing_store_is_refused_naming_plan_open_and_creates_nothing() {
        with_root(|root| {
            let path = store_path(root);
            let err = mutate(&path, &write_args(), |_store| Ok(()))
                .expect_err("only `plan open` creates the store");
            let message = format!("{err:#}");
            assert!(
                message.contains(&format!("tomlctl plan open --slug {SLUG}")),
                "{message}"
            );
            assert!(
                !path.parent().expect("a flow dir").exists(),
                "the refusal must not create the flow directory"
            );
        });
    }

    #[test]
    fn a_newer_store_is_left_untouched_by_a_mutation() {
        with_root(|root| {
            let path = store_path(root);
            std::fs::create_dir_all(path.parent().expect("a flow dir")).unwrap();
            let body = format!(
                "schema_version = {}\nfuture = true\n",
                schema::SCHEMA_VERSION + 1
            );
            std::fs::write(&path, &body).unwrap();

            let err = mutate(&path, &write_args(), |_store| Ok(()))
                .expect_err("a newer store is refused");
            assert!(format!("{err:#}").contains("upgrade tomlctl"), "{err:#}");
            assert_eq!(std::fs::read_to_string(&path).unwrap(), body);
        });
    }

    fn assert_tagged_parse(err: &anyhow::Error, path: &Path) {
        let tagged = err
            .downcast_ref::<TaggedError>()
            .unwrap_or_else(|| panic!("a tagged error, got: {err:#}"));
        assert!(matches!(tagged.kind, ErrorKind::Parse), "{tagged:?}");
        assert_eq!(tagged.file.as_deref(), Some(path));
    }

    /// Syntactically valid TOML whose root fails the schema: a newer version.
    #[test]
    fn a_store_failing_schema_validation_reports_kind_parse_with_its_path() {
        with_root(|root| {
            let path = store_path(root);
            std::fs::create_dir_all(path.parent().expect("a flow dir")).unwrap();
            let body = format!("schema_version = {}\n", schema::SCHEMA_VERSION + 1);
            std::fs::write(&path, &body).unwrap();

            let err = load(&path, &read_args(false)).expect_err("load validates");
            assert_tagged_parse(&err, &path);
            assert!(format!("{err:#}").contains("upgrade tomlctl"), "{err:#}");

            let err = mutate(&path, &write_args(), |_store| Ok(())).expect_err("mutate validates");
            assert_tagged_parse(&err, &path);
            assert_eq!(std::fs::read_to_string(&path).unwrap(), body);
        });
    }

    #[test]
    fn a_closure_error_keeps_its_own_kind() {
        with_root(|root| {
            let path = store_path(root);
            seed_store(&path);
            let err = mutate(&path, &write_args(), |_store| {
                Err::<(), _>(anyhow::anyhow!("refused"))
            })
            .expect_err("the closure's error propagates");
            assert!(err.downcast_ref::<TaggedError>().is_none(), "{err:#}");
        });
    }

    #[test]
    fn resolve_joins_the_flow_dir_and_refuses_traversal() {
        with_root(|root| {
            assert_eq!(resolve(SLUG).expect("slug resolves"), store_path(root));
            for bad in ["..", "../escape", "a/b", "a\\b", "", "Upper", "-lead"] {
                assert!(resolve(bad).is_err(), "slug `{bad}` must be refused");
            }
        });
    }

    #[test]
    fn authored_dir_sits_under_docs_and_refuses_traversal() {
        with_root(|root| {
            assert_eq!(
                authored_dir(SLUG).expect("slug resolves"),
                root.join("docs").join("flows").join(SLUG)
            );
            for bad in ["..", "../escape", "a/b", "a\\b", "", "Upper", "-lead"] {
                assert!(authored_dir(bad).is_err(), "slug `{bad}` must be refused");
            }
        });
    }

    const REVIEW_STORE: &str = r#"
[[runs]]
id = "U1"
family = "plan"
finished = "2026-11-09T10:00:00Z"

[[runs]]
id = "U2"
family = "plan"
rechecks = "U1"

[[runs]]
id = "U3"
family = "code"

[[findings]]
id = "F1"
run = "U1"
family = "plan"
kind = "defect"
severity = "major"
status = "resolved"
summary = "Accept check tests nothing."
applied = { at = "2026-11-09T10:05:00Z", tier = "mechanical", target = "task:t1.spec.accept[0]", old = '{"behaviour":"b","check":"old"}', new = '{"behaviour":"b","check":"new"}', by = "tool" }

[[findings]]
id = "F2"
run = "U1"
family = "plan"
kind = "defect"
severity = "minor"
status = "open"
tier = "user"
target = "decision:D1.why"
summary = "Why is thin."

[[findings]]
id = "F3"
run = "U2"
family = "plan"
kind = "cleanup"
status = "open"
tier = "cited"
target = "task:t1.files"
summary = "Re-check raised this."

[[findings]]
id = "F4"
run = "U3"
family = "code"
kind = "defect"
severity = "critical"
status = "open"
summary = "Not a plan finding."
"#;

    fn review_state(body: &str) -> PlanReviewState {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(REVIEW_STORE_FILE), body).unwrap();
        plan_review_state(dir.path()).expect("the review store reads")
    }

    fn accept(check: &str, probed: bool) -> Accept {
        Accept {
            behaviour: "b".to_string(),
            check: check.to_string(),
            kind: String::new(),
            positive: String::new(),
            writes_build: true,
            probe: probed.then(schema_probe),
        }
    }

    fn schema_probe() -> crate::tasks::schema::Probe {
        crate::tasks::schema::Probe {
            at: "2026-11-09T10:10:00Z".to_string(),
            neg_rc: Some(1),
            pos_rc: None,
            verdict: "healthy".to_string(),
        }
    }

    fn tasks_with_accept(entry: Accept) -> Store {
        let mut tasks = Store::default();
        tasks.specs.push(crate::tasks::schema::TaskSpec {
            r#ref: "t1".to_string(),
            outcome: String::new(),
            constraints: Vec::new(),
            accept: vec![entry],
            premises: Vec::new(),
            means: Vec::new(),
            design: Vec::new(),
            precedent: Vec::new(),
            executor: String::new(),
        });
        tasks
    }

    #[test]
    fn review_state_reads_only_plan_family_runs_and_findings() {
        let state = review_state(REVIEW_STORE);
        assert_eq!(state.finished_runs, ["U1"]);
        assert_eq!(state.unfinished_runs, ["U2"]);
        assert_eq!(state.applied.len(), 1);
        let applied = &state.applied[0];
        assert_eq!(applied.target, "task:t1.spec.accept[0]");
        assert_eq!(applied.tier, "mechanical");
        assert_eq!(
            applied.new,
            serde_json::json!({"behaviour": "b", "check": "new"})
        );
        let open: Vec<(&str, bool)> = state
            .open
            .iter()
            .map(|f| (f.id.as_str(), f.recheck))
            .collect();
        assert_eq!(open, [("F2", false), ("F3", true)]);
        assert!(
            !state.open[0].is_gate_risk(),
            "a minor defect is no gate risk"
        );
        assert!(state.open[1].is_gate_risk(), "a re-check finding always is");
    }

    #[test]
    fn a_missing_review_store_is_an_empty_state() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            plan_review_state(dir.path()).unwrap(),
            PlanReviewState::default()
        );
    }

    #[test]
    fn an_applied_accept_edit_without_a_probe_is_named() {
        let state = review_state(REVIEW_STORE);
        assert_eq!(
            state.unprobed_accept_edits(&tasks_with_accept(accept("new", false))),
            ["F1"]
        );
        assert!(
            state
                .unprobed_accept_edits(&tasks_with_accept(accept("new", true)))
                .is_empty(),
            "a re-probed entry is settled"
        );
        assert!(
            state
                .unprobed_accept_edits(&tasks_with_accept(accept("other", false)))
                .is_empty(),
            "an entry changed since is no longer the finding's"
        );
    }

    #[test]
    fn unsettled_names_every_reason_and_its_ids() {
        let why = review_state(REVIEW_STORE)
            .unsettled(&tasks_with_accept(accept("new", false)))
            .expect("the review is not settled");
        for needle in ["U2", "F2", "F1"] {
            assert!(why.contains(needle), "{why}");
        }
        assert!(
            review_state("")
                .unsettled(&Store::default())
                .is_some_and(|why| why.contains("no finished plan-family run"))
        );
    }

    #[test]
    fn line_endings_alone_do_not_change_a_brief_digest() {
        let lf = b"# Intent\n\nStop the retry storm.\n";
        let crlf = b"# Intent\r\n\r\nStop the retry storm.\r\n";
        assert_eq!(brief_digest(lf), brief_digest(crlf));
        assert_eq!(brief_digest(lf), hex_lower(&Sha256::digest(lf)));
    }

    #[test]
    fn a_changed_word_changes_a_brief_digest() {
        assert_ne!(
            brief_digest(b"# Intent\r\nStop the retry storm.\r\n"),
            brief_digest(b"# Intent\r\nStart the retry storm.\r\n")
        );
        assert_ne!(brief_digest(b"a\rb"), brief_digest(b"a\nb"));
    }
}
