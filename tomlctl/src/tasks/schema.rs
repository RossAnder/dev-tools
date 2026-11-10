//! Store, policy, checkpoint and task-row types, and their TOML conversions.
//!
//! ```toml
//! schema_version = 1
//! last_updated = 2026-09-07
//! plan_path = "docs/plans/<slug>.md"
//! last_import_refs = ["…"]
//! minted_refs = ["…"]
//! audit = true
//! id_high_water = 12
//! [policy] / [[checkpoints]] / side tables / [[items]]
//! ```
//!
//! `to_toml` inserts keys in that order and `preserve_order` holds it, so a
//! write is byte-stable with no sort step. Bodies are LF-normalised on both
//! read and write: a lone `\r` forces `toml`'s writer into the escaped `"""`
//! form. Unknown keys are dropped — the store is tool-owned and every write
//! re-emits it from this shape.

use std::fmt;

use anyhow::{Result, anyhow};
use toml::Value as TomlValue;
use toml::map::Map;
use toml::value::Datetime;

/// The newest version this binary reads, stamped on a store holding `specs`,
/// `milestones`, `changes`, `finding_links`, `audit` or `id_high_water`. A binary that only knows
/// [`BASE_SCHEMA_VERSION`] refuses that stamp rather than dropping those keys.
pub(crate) const SCHEMA_VERSION: i64 = 2;

/// The stamp on a store holding none of those keys, so it stays readable by a
/// binary that predates them.
pub(crate) const BASE_SCHEMA_VERSION: i64 = 1;

/// `elaboration` for a milestone recording none: detail upfront is the norm.
pub(crate) const ELABORATION_UPFRONT: &str = "upfront";
pub(crate) const ELABORATION_DEFERRED: &str = "deferred";
pub(crate) const ELABORATION_ELABORATED: &str = "elaborated";

pub(crate) const KIND_FORWARD: &str = "forward";
pub(crate) const KIND_FALSIFIER: &str = "falsifier";
pub(crate) const KIND_GUARD: &str = "guard";
pub(crate) const ACCEPT_KINDS: [&str; 3] = [KIND_FORWARD, KIND_FALSIFIER, KIND_GUARD];

pub(crate) const BINDING_REQUIRED: &str = "required";
pub(crate) const BINDING_SUGGESTED: &str = "suggested";
pub(crate) const MEANS_BINDINGS: [&str; 2] = [BINDING_REQUIRED, BINDING_SUGGESTED];

/// The one probe verdict that clears `plan/probe-unhealthy`.
pub(crate) const VERDICT_HEALTHY: &str = "healthy";

/// Opens the `[[changes]]` key of a milestone's rows, which no task's trail shares.
pub(crate) const MILESTONE_REF_PREFIX: &str = "milestone:";

/// Who a `[[changes]]` row names when the caller gives no `actor`.
pub(crate) const DEFAULT_ACTOR: &str = "orchestrator";

/// Depth a row's heading renders at when the store records none — the
/// shallowest depth that still sits inside a `## ` section.
pub(crate) const DEFAULT_HEADING_DEPTH: u32 = 3;

/// `policy.origin`: the plan's `## Execution Policy` section is where the
/// values came from.
pub(crate) const POLICY_ORIGIN_PLAN: &str = "plan";

/// `policy.origin`: the plan authored no policy section, so every value is a
/// house default. The import always materialises a `[policy]` table, so this
/// is the only thing distinguishing a pre-policy plan afterwards.
pub(crate) const POLICY_ORIGIN_DEFAULT: &str = "default";

/// The diagnostic a pre-`origin` import stamped into `note` to mark an absent
/// policy section. Read back as `origin` and dropped, so the sentence stops
/// reaching the plan the next render rewrites.
const LEGACY_ABSENT_NOTE: &str = "policy absent in source plan";

type Table = Map<String, TomlValue>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Store {
    /// The version as read. `to_toml` stamps from the store's content instead.
    pub(crate) schema_version: i64,
    /// Bare TOML date, absent until the first mutation stamps it.
    pub(crate) last_updated: Option<Datetime>,
    pub(crate) plan_path: String,
    /// Ref set of the last import; a row outside it has been renamed or
    /// deleted in the plan.
    pub(crate) last_import_refs: Vec<String>,
    /// Refs `tasks add` minted that no import has produced yet. Such a row is new
    /// to the plan rather than gone from it, so `plan/orphan-row` passes it over,
    /// and the import that first produces the ref ends the exemption.
    pub(crate) minted_refs: Vec<String>,
    pub(crate) policy: Policy,
    pub(crate) checkpoints: Vec<Checkpoint>,
    /// Hand-patched import-owned fields and the plan values they were taken
    /// against, keyed by `ref` like `last_import_refs`.
    pub(crate) import_overrides: Vec<ImportOverride>,
    /// The annotation a `Files` entry carried in the plan, keyed by `ref` and
    /// path. A row's `files` holds bare paths — every consumer of a file claim
    /// compares them — so an annotation the author wrote has nowhere else to
    /// live and the render would drop it.
    pub(crate) file_notes: Vec<FileNote>,
    /// Backlog items a row closes or references, keyed by `ref`. On a v1 flow a
    /// link whose ref no row carries is kept until the next import.
    pub(crate) backlog_links: Vec<BacklogLink>,
    /// Review findings a row fixes or settles, keyed by `ref`. Unlike a backlog
    /// link no import rebuilds one, so holding any makes the store v2.
    pub(crate) finding_links: Vec<FindingLink>,
    /// Set by approval: from then on a plan field changes only with a reason,
    /// and each change leaves a row in `changes`.
    pub(crate) audit: bool,
    /// The highest id a removal retired from a v2 store, `0` while none has.
    /// Plan records and premises name tasks by id, so a retired id must never
    /// be minted for another task.
    pub(crate) id_high_water: u32,
    pub(crate) specs: Vec<TaskSpec>,
    pub(crate) milestones: Vec<MilestoneSpec>,
    /// The audit trail, keyed by `ref` or `milestone:<id>`. Outlives the row it
    /// records, so a removal stays visible as a tombstone.
    pub(crate) changes: Vec<Change>,
    pub(crate) items: Vec<TaskRow>,
}

/// `## Execution Policy` as stored. The two vocabulary fields stay `String`
/// so `tasks check` — not the reader — decides what is out of vocabulary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Policy {
    pub(crate) checkpoints: String,
    pub(crate) max_parallel: u32,
    pub(crate) commit_granularity: String,
    /// `plan` or `default` — whether the three fields above were read from a
    /// plan or fallen back to. A reader deciding between policy and pre-policy
    /// semantics has nothing else to go on once the import has written the
    /// table.
    pub(crate) origin: String,
    /// Prose belonging to no bullet. Authored, never a tool diagnostic: the
    /// renderer writes it back under `## Execution Policy` and the next import
    /// reads it as the author's.
    pub(crate) note: String,
    /// The clause trailing a bullet's value, kept against the bullet that
    /// carried it so the render puts it back on that line.
    pub(crate) checkpoints_note: String,
    pub(crate) max_parallel_note: String,
    pub(crate) commit_granularity_note: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Checkpoint {
    pub(crate) id: String,
    pub(crate) rationale: String,
}

/// One row's hand-patched import-owned fields. Each `Some` holds the value the
/// plan stated when the patch was made, not the patch itself: `import-plan`
/// keeps the row's own value while the plan still states that base, and drops
/// the field the moment the plan states anything else. A row the plan and the
/// store agree on carries no entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ImportOverride {
    pub(crate) r#ref: String,
    pub(crate) files: Option<Vec<String>>,
    pub(crate) needs: Option<Vec<u32>>,
}

impl ImportOverride {
    pub(crate) fn new(r#ref: &str) -> Self {
        Self {
            r#ref: r#ref.to_string(),
            files: None,
            needs: None,
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.files.is_none() && self.needs.is_none()
    }
}

/// One `Files` entry's trailing annotation — `(new)` and the like — against
/// the row that claims the path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FileNote {
    pub(crate) r#ref: String,
    pub(crate) file: String,
    pub(crate) note: String,
}

impl FileNote {
    fn is_empty(&self) -> bool {
        self.r#ref.is_empty() || self.file.is_empty() || self.note.trim().is_empty()
    }
}

/// The change a `Files` note declares for its path; a note declaring neither
/// is an edit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FileKind {
    New,
    Delete,
}

impl FileKind {
    /// Reads only a note opening with a parenthetical whose first word, spaces
    /// aside, is the whole of it or is followed by `,`, `;`, `:` or `—`, so
    /// `(new thread)`, an addition to an existing file, is no new file. The
    /// word is `new`, `create` or `created` for a new file and `delete`,
    /// `deleted`, `remove` or `removed` for a deletion, in any case.
    pub(crate) fn of_note(note: &str) -> Option<Self> {
        let inner = note.trim_start().strip_prefix('(')?.trim_start();
        let word_len = inner
            .find(|ch: char| !ch.is_alphabetic())
            .unwrap_or(inner.len());
        let (word, rest) = inner.split_at(word_len);
        if !rest.trim_start().starts_with([')', ',', ';', ':', '—']) {
            return None;
        }
        match word.to_lowercase().as_str() {
            "new" | "create" | "created" => Some(Self::New),
            "delete" | "deleted" | "remove" | "removed" => Some(Self::Delete),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BacklogLink {
    pub(crate) r#ref: String,
    pub(crate) closes: Vec<String>,
    pub(crate) refs: Vec<String>,
}

impl BacklogLink {
    fn is_empty(&self) -> bool {
        self.r#ref.is_empty() || (self.closes.is_empty() && self.refs.is_empty())
    }
}

/// `store` is the review store's repo-relative path. `fixes` are findings the
/// row's work resolves; `settles` are findings it investigates and proposes a
/// fix for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FindingLink {
    pub(crate) r#ref: String,
    pub(crate) store: String,
    pub(crate) fixes: Vec<String>,
    pub(crate) settles: Vec<String>,
}

impl FindingLink {
    pub(crate) fn is_empty(&self) -> bool {
        self.r#ref.is_empty() || (self.fixes.is_empty() && self.settles.is_empty())
    }
}

/// A row's spec, keyed by `ref`. Vocabulary fields here and below stay `String`
/// so the writers, not the reader, decide what is out of vocabulary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TaskSpec {
    pub(crate) r#ref: String,
    pub(crate) outcome: String,
    pub(crate) constraints: Vec<String>,
    pub(crate) accept: Vec<Accept>,
    pub(crate) premises: Vec<Premise>,
    pub(crate) means: Vec<Means>,
    pub(crate) design: Vec<String>,
    pub(crate) precedent: Vec<String>,
    pub(crate) executor: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Accept {
    pub(crate) behaviour: String,
    /// Must exit non-zero when `behaviour` does not hold.
    pub(crate) check: String,
    pub(crate) kind: String,
    /// The positive control's command; `""` when there is none.
    pub(crate) positive: String,
    /// An absent key reads as `true`, so only an explicit `false` is probed and
    /// an unlabelled command never runs.
    pub(crate) writes_build: bool,
    pub(crate) probe: Option<Probe>,
}

/// One probe run against an `Accept`. An rc is `None` for a control that did
/// not run, which a missing positive command makes the normal case for `pos_rc`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Probe {
    pub(crate) at: String,
    pub(crate) neg_rc: Option<i64>,
    pub(crate) pos_rc: Option<i64>,
    pub(crate) verdict: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Premise {
    pub(crate) claim: String,
    pub(crate) check: String,
    /// `base` or `after:<id>`.
    pub(crate) at: String,
    /// `neg_rc` holds the check's exit code; `pos_rc` is always `None`.
    pub(crate) probe: Option<Probe>,
}

/// The id in a canonical `after:<id>`. `after:+3` and `after:007` read as
/// nothing: a reader joining on the stored string would miss the row.
pub(crate) fn premise_target(at: &str) -> Option<u32> {
    let id = at.strip_prefix("after:")?;
    id.parse::<u32>().ok().filter(|n| n.to_string() == id)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Means {
    pub(crate) text: String,
    pub(crate) binding: String,
    pub(crate) detail: String,
}

/// A milestone's execution facts, keyed by a `[[checkpoints]]` id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MilestoneSpec {
    pub(crate) id: String,
    pub(crate) gates: Vec<String>,
    pub(crate) elaboration: String,
    pub(crate) defer_reason: String,
}

impl MilestoneSpec {
    pub(crate) fn is_deferred(&self) -> bool {
        self.elaboration == ELABORATION_DEFERRED
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Change {
    pub(crate) at: Option<Datetime>,
    pub(crate) r#ref: String,
    pub(crate) field: String,
    pub(crate) old: String,
    pub(crate) new: String,
    pub(crate) actor: String,
    pub(crate) reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TaskRow {
    pub(crate) id: u32,
    pub(crate) r#ref: String,
    pub(crate) title: String,
    pub(crate) effort: Effort,
    pub(crate) status: Status,
    /// `""` when the task belongs to no checkpoint group.
    pub(crate) checkpoint: String,
    /// Plan-owned heading structure: the phase label standing over the row in
    /// the document, that label's `#` run, and the row's own. A phase groups by
    /// authored position; `checkpoint` groups by dependency closure, and the
    /// two spans need not coincide.
    pub(crate) phase: String,
    pub(crate) phase_depth: u32,
    pub(crate) heading_depth: u32,
    pub(crate) files: Vec<String>,
    pub(crate) needs: Vec<u32>,
    pub(crate) coupling: Vec<u32>,
    pub(crate) deps_note: String,
    pub(crate) action: String,
    pub(crate) detail: String,
    pub(crate) acceptance: String,
    pub(crate) agent: String,
    pub(crate) commit: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Status {
    #[default]
    Pending,
    InProgress,
    Done,
    Failed,
    Deferred,
    Parked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Effort {
    S,
    M,
    L,
}

impl Default for Store {
    fn default() -> Self {
        Self {
            schema_version: BASE_SCHEMA_VERSION,
            last_updated: None,
            plan_path: String::new(),
            last_import_refs: Vec::new(),
            minted_refs: Vec::new(),
            policy: Policy::default(),
            checkpoints: Vec::new(),
            import_overrides: Vec::new(),
            file_notes: Vec::new(),
            backlog_links: Vec::new(),
            finding_links: Vec::new(),
            audit: false,
            id_high_water: 0,
            specs: Vec::new(),
            milestones: Vec::new(),
            changes: Vec::new(),
            items: Vec::new(),
        }
    }
}

/// Fallback for an absent or partial `[policy]` table. Every field is a
/// value from the documented vocabulary, so a store written before its first
/// import still serialises to a document `tasks check` can accept.
impl Default for Policy {
    fn default() -> Self {
        Self {
            checkpoints: "milestones".to_string(),
            max_parallel: 6,
            commit_granularity: "per-task".to_string(),
            origin: POLICY_ORIGIN_PLAN.to_string(),
            note: String::new(),
            checkpoints_note: String::new(),
            max_parallel_note: String::new(),
            commit_granularity_note: String::new(),
        }
    }
}

impl Store {
    pub(crate) fn next_id(&self) -> u32 {
        self.items
            .iter()
            .map(|row| row.id)
            .fold(self.id_high_water, u32::max)
            .saturating_add(1)
    }

    /// Whether the store holds any key that stamps it [`SCHEMA_VERSION`].
    pub(crate) fn holds_v2_keys(&self) -> bool {
        self.audit
            || self.id_high_water > 0
            || !self.specs.is_empty()
            || !self.milestones.is_empty()
            || !self.changes.is_empty()
            || self.finding_links.iter().any(|link| !link.is_empty())
    }

    pub(crate) fn find(&self, id: u32) -> Option<&TaskRow> {
        self.items.iter().find(|row| row.id == id)
    }

    pub(crate) fn find_ref(&self, r#ref: &str) -> Option<&TaskRow> {
        self.items.iter().find(|row| row.r#ref == r#ref)
    }

    pub(crate) fn find_override(&self, r#ref: &str) -> Option<&ImportOverride> {
        self.import_overrides
            .iter()
            .find(|entry| entry.r#ref == r#ref)
    }

    pub(crate) fn file_note(&self, r#ref: &str, file: &str) -> Option<&str> {
        self.file_notes
            .iter()
            .find(|entry| entry.r#ref == r#ref && entry.file == file)
            .map(|entry| entry.note.as_str())
    }

    pub(crate) fn links_for(&self, r#ref: &str) -> Option<&BacklogLink> {
        self.backlog_links.iter().find(|entry| entry.r#ref == r#ref)
    }

    pub(crate) fn spec_for(&self, r#ref: &str) -> Option<&TaskSpec> {
        self.specs.iter().find(|entry| entry.r#ref == r#ref)
    }

    /// The refs a task's audit trail is keyed on, live or tombstoned. A ref names
    /// one task's trail for the life of the store, so none of these can be claimed
    /// by a row that does not already hold it.
    pub(crate) fn trail_refs(&self) -> impl Iterator<Item = &str> {
        self.changes
            .iter()
            .map(|change| change.r#ref.as_str())
            .filter(|r#ref| !r#ref.starts_with(MILESTONE_REF_PREFIX))
    }

    /// Drops the file notes, backlog links, finding links and spec keyed on
    /// `ref`, so a row claiming a freed `ref` does not read a removed row's
    /// entries as its own. `changes` stays: a tombstone must outlive the row it
    /// records.
    pub(crate) fn drop_ref_entries(&mut self, r#ref: &str) {
        self.file_notes.retain(|entry| entry.r#ref != r#ref);
        self.backlog_links.retain(|entry| entry.r#ref != r#ref);
        self.finding_links.retain(|entry| entry.r#ref != r#ref);
        self.specs.retain(|entry| entry.r#ref != r#ref);
    }

    /// Run on every write, so a row an import has produced is judged like any
    /// other from then on, whichever verb removed or re-keyed its row meanwhile.
    pub(crate) fn prune_minted_refs(&mut self) {
        let Self {
            minted_refs,
            last_import_refs,
            items,
            ..
        } = self;
        minted_refs.retain(|minted| {
            !last_import_refs.contains(minted) && items.iter().any(|row| row.r#ref == *minted)
        });
    }
}

impl Status {
    pub(crate) const VOCABULARY: &'static [&'static str] = &[
        "pending",
        "in-progress",
        "done",
        "failed",
        "deferred",
        "parked",
    ];

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::InProgress => "in-progress",
            Self::Done => "done",
            Self::Failed => "failed",
            Self::Deferred => "deferred",
            Self::Parked => "parked",
        }
    }

    /// Case-sensitive: `Done` is not `done`.
    pub(crate) fn parse(raw: &str) -> Option<Self> {
        match raw {
            "pending" => Some(Self::Pending),
            "in-progress" => Some(Self::InProgress),
            "done" => Some(Self::Done),
            "failed" => Some(Self::Failed),
            "deferred" => Some(Self::Deferred),
            "parked" => Some(Self::Parked),
            _ => None,
        }
    }

    fn parse_in_row(raw: &str, id: u32) -> Result<Self> {
        Self::parse(raw).ok_or_else(|| {
            anyhow!(
                "task {id}: unknown status `{raw}` — expected one of {}",
                Self::VOCABULARY.join(", ")
            )
        })
    }
}

impl Effort {
    pub(crate) const VOCABULARY: &'static [&'static str] = &["S", "M", "L"];

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::S => "S",
            Self::M => "M",
            Self::L => "L",
        }
    }

    /// Case-sensitive: `s` is not `S`.
    pub(crate) fn parse(raw: &str) -> Option<Self> {
        match raw {
            "S" => Some(Self::S),
            "M" => Some(Self::M),
            "L" => Some(Self::L),
            _ => None,
        }
    }

    fn parse_in_row(raw: &str, id: u32) -> Result<Self> {
        Self::parse(raw).ok_or_else(|| {
            anyhow!(
                "task {id}: unknown effort `{raw}` — expected one of {}",
                Self::VOCABULARY.join(", ")
            )
        })
    }
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl fmt::Display for Effort {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The form a row's `files` path and a path naming it are compared in, with
/// backslashes folded and any leading `./` gone. Stored paths stay as written.
pub(crate) fn normal_path(path: &str) -> String {
    path.replace('\\', "/").trim_start_matches("./").to_string()
}

pub(crate) fn from_toml(doc: &TomlValue) -> Result<Store> {
    let root = doc
        .as_table()
        .ok_or_else(|| anyhow!("tasks store root is not a table"))?;

    let schema_version = root
        .get("schema_version")
        .and_then(TomlValue::as_integer)
        .unwrap_or(BASE_SCHEMA_VERSION);
    // A newer store holds keys this shape drops, and the next write would
    // re-emit it still stamped with its own version — the loss is invisible
    // to the binary that wrote it.
    if schema_version > SCHEMA_VERSION {
        return Err(anyhow!(
            "tasks store schema_version {schema_version} is newer than the supported \
             {SCHEMA_VERSION} — upgrade tomlctl to read this store"
        ));
    }

    let checkpoints = table_array(root, "checkpoints")?
        .iter()
        .enumerate()
        .map(|(index, value)| checkpoint_from_toml(value, index))
        .collect::<Result<Vec<_>>>()?;
    let items = table_array(root, "items")?
        .iter()
        .enumerate()
        .map(|(index, value)| row_from_toml(value, index))
        .collect::<Result<Vec<_>>>()?;
    let mut import_overrides = table_array(root, "import_overrides")?
        .iter()
        .enumerate()
        .map(|(index, value)| override_from_toml(value, index))
        .collect::<Result<Vec<_>>>()?;
    import_overrides.retain(|entry| !entry.is_empty());
    let mut file_notes = table_array(root, "file_notes")?
        .iter()
        .enumerate()
        .map(|(index, value)| file_note_from_toml(value, index))
        .collect::<Result<Vec<_>>>()?;
    file_notes.retain(|entry| !entry.is_empty());
    let mut backlog_links = table_array(root, "backlog_links")?
        .iter()
        .enumerate()
        .map(|(index, value)| backlog_link_from_toml(value, index))
        .collect::<Result<Vec<_>>>()?;
    backlog_links.retain(|entry| !entry.is_empty());
    let mut finding_links = table_array(root, "finding_links")?
        .iter()
        .enumerate()
        .map(|(index, value)| finding_link_from_toml(value, index))
        .collect::<Result<Vec<_>>>()?;
    finding_links.retain(|entry| !entry.is_empty());
    let mut specs = table_array(root, "specs")?
        .iter()
        .enumerate()
        .map(|(index, value)| spec_from_toml(value, index))
        .collect::<Result<Vec<_>>>()?;
    specs.retain(|entry| !entry.r#ref.is_empty());
    let mut milestones = table_array(root, "milestones")?
        .iter()
        .enumerate()
        .map(|(index, value)| milestone_from_toml(value, index))
        .collect::<Result<Vec<_>>>()?;
    milestones.retain(|entry| !entry.id.is_empty());
    let mut changes = table_array(root, "changes")?
        .iter()
        .enumerate()
        .map(|(index, value)| change_from_toml(value, index))
        .collect::<Result<Vec<_>>>()?;
    changes.retain(|entry| !entry.r#ref.is_empty());
    // Strict where a malformed `writes_build` is not: reading this flag as
    // false would lift the audit requirement off an approved plan.
    let audit = match root.get("audit") {
        Some(raw) => raw
            .as_bool()
            .ok_or_else(|| anyhow!("`audit` is not a boolean"))?,
        None => false,
    };
    // Strict on the same terms: reading a damaged mark as `0` would mint a
    // retired id and erase the mark at the next write.
    let id_high_water = match root.get("id_high_water") {
        Some(raw) => raw
            .as_integer()
            .and_then(|n| u32::try_from(n).ok())
            .ok_or_else(|| anyhow!("`id_high_water` is not a non-negative integer"))?,
        None => 0,
    };

    Ok(Store {
        schema_version,
        last_updated: root.get("last_updated").and_then(as_date),
        plan_path: str_or(root, "plan_path", ""),
        last_import_refs: str_array(root, "last_import_refs"),
        minted_refs: str_array(root, "minted_refs"),
        policy: match root.get("policy") {
            Some(value) => policy_from_toml(value)?,
            None => Policy::default(),
        },
        checkpoints,
        import_overrides,
        file_notes,
        backlog_links,
        finding_links,
        audit,
        id_high_water,
        specs,
        milestones,
        changes,
        items,
    })
}

pub(crate) fn to_toml(store: &Store) -> TomlValue {
    let Store {
        schema_version: _,
        last_updated,
        plan_path,
        last_import_refs,
        minted_refs,
        policy,
        checkpoints,
        import_overrides,
        file_notes,
        backlog_links,
        finding_links,
        audit,
        id_high_water,
        specs,
        milestones,
        changes,
        items,
    } = store;

    let finding_links: Vec<TomlValue> = finding_links
        .iter()
        .filter(|entry| !entry.is_empty())
        .map(finding_link_to_toml)
        .collect();
    let specs: Vec<TomlValue> = specs
        .iter()
        .filter(|entry| !entry.r#ref.is_empty())
        .map(spec_to_toml)
        .collect();
    let milestones: Vec<TomlValue> = milestones
        .iter()
        .filter(|entry| !entry.id.is_empty())
        .map(milestone_to_toml)
        .collect();
    let changes: Vec<TomlValue> = changes
        .iter()
        .filter(|entry| !entry.r#ref.is_empty())
        .map(change_to_toml)
        .collect();
    let holds_v2_keys = *audit
        || *id_high_water > 0
        || !specs.is_empty()
        || !milestones.is_empty()
        || !changes.is_empty()
        || !finding_links.is_empty();
    let stamp = match holds_v2_keys {
        true => SCHEMA_VERSION,
        false => BASE_SCHEMA_VERSION,
    };

    let mut root = Table::new();
    root.insert("schema_version".to_string(), TomlValue::Integer(stamp));
    if let Some(date) = last_updated {
        root.insert("last_updated".to_string(), TomlValue::Datetime(*date));
    }
    root.insert(
        "plan_path".to_string(),
        TomlValue::String(plan_path.clone()),
    );
    root.insert(
        "last_import_refs".to_string(),
        str_arr_value(last_import_refs),
    );
    // Omitted while empty, and no version stamp: a binary that predates the key
    // only loses the exemption, and an imported store writes back byte-identical.
    if !minted_refs.is_empty() {
        root.insert("minted_refs".to_string(), str_arr_value(minted_refs));
    }
    if *audit {
        root.insert("audit".to_string(), TomlValue::Boolean(true));
    }
    if *id_high_water > 0 {
        root.insert(
            "id_high_water".to_string(),
            TomlValue::Integer(i64::from(*id_high_water)),
        );
    }
    root.insert("policy".to_string(), policy_to_toml(policy));
    root.insert(
        "checkpoints".to_string(),
        TomlValue::Array(checkpoints.iter().map(checkpoint_to_toml).collect()),
    );
    // Omitted rather than emitted empty: a store that has never been
    // hand-patched writes back byte-identical to one written before the key
    // existed.
    let overrides: Vec<TomlValue> = import_overrides
        .iter()
        .filter(|entry| !entry.is_empty())
        .map(override_to_toml)
        .collect();
    if !overrides.is_empty() {
        root.insert("import_overrides".to_string(), TomlValue::Array(overrides));
    }
    // Omitted on the same terms: a plan whose `Files` lines carry no
    // annotation writes back exactly as it did before the key existed.
    let notes: Vec<TomlValue> = file_notes
        .iter()
        .filter(|entry| !entry.is_empty())
        .map(file_note_to_toml)
        .collect();
    if !notes.is_empty() {
        root.insert("file_notes".to_string(), TomlValue::Array(notes));
    }
    let links: Vec<TomlValue> = backlog_links
        .iter()
        .filter(|entry| !entry.is_empty())
        .map(backlog_link_to_toml)
        .collect();
    if !links.is_empty() {
        root.insert("backlog_links".to_string(), TomlValue::Array(links));
    }
    // Omitted while empty, so a v1 store writes back byte-identical.
    for (key, entries) in [
        ("finding_links", finding_links),
        ("specs", specs),
        ("milestones", milestones),
        ("changes", changes),
    ] {
        if !entries.is_empty() {
            root.insert(key.to_string(), TomlValue::Array(entries));
        }
    }
    root.insert(
        "items".to_string(),
        TomlValue::Array(items.iter().map(row_to_toml).collect()),
    );
    TomlValue::Table(root)
}

fn policy_from_toml(value: &TomlValue) -> Result<Policy> {
    let table = value
        .as_table()
        .ok_or_else(|| anyhow!("`policy` is not a table"))?;
    let fallback = Policy::default();
    let max_parallel = match table.get("max_parallel") {
        Some(raw) => raw
            .as_integer()
            .and_then(|n| u32::try_from(n).ok())
            .ok_or_else(|| anyhow!("`policy.max_parallel` is not a non-negative integer"))?,
        None => fallback.max_parallel,
    };
    // An absent `origin` is a store written before the key existed, and such a
    // store marked the same condition by the diagnostic it stamped into
    // `note`: that sentence is the older spelling of `default`, and anything
    // else — an authored note included — is a policy the plan stated.
    let note = lf(&str_or(table, "note", ""));
    let legacy_absent = note.trim() == LEGACY_ABSENT_NOTE;
    Ok(Policy {
        checkpoints: str_or(table, "checkpoints", &fallback.checkpoints),
        max_parallel,
        commit_granularity: str_or(table, "commit_granularity", &fallback.commit_granularity),
        origin: match legacy_absent {
            true => POLICY_ORIGIN_DEFAULT.to_string(),
            false => str_or(table, "origin", &fallback.origin),
        },
        note: match legacy_absent {
            true => String::new(),
            false => note,
        },
        checkpoints_note: lf(&str_or(table, "checkpoints_note", "")),
        max_parallel_note: lf(&str_or(table, "max_parallel_note", "")),
        commit_granularity_note: lf(&str_or(table, "commit_granularity_note", "")),
    })
}

fn policy_to_toml(policy: &Policy) -> TomlValue {
    let Policy {
        checkpoints,
        max_parallel,
        commit_granularity,
        origin,
        note,
        checkpoints_note,
        max_parallel_note,
        commit_granularity_note,
    } = policy;
    let mut table = Table::new();
    table.insert(
        "checkpoints".to_string(),
        TomlValue::String(checkpoints.clone()),
    );
    table.insert(
        "max_parallel".to_string(),
        TomlValue::Integer(i64::from(*max_parallel)),
    );
    table.insert(
        "commit_granularity".to_string(),
        TomlValue::String(commit_granularity.clone()),
    );
    table.insert("origin".to_string(), TomlValue::String(origin.clone()));
    table.insert("note".to_string(), TomlValue::String(lf(note)));
    // Each omitted while empty, so a policy whose bullets carry no clause
    // writes back exactly as it did before the keys existed.
    for (key, value) in [
        ("checkpoints_note", checkpoints_note),
        ("max_parallel_note", max_parallel_note),
        ("commit_granularity_note", commit_granularity_note),
    ] {
        if !value.trim().is_empty() {
            table.insert(key.to_string(), TomlValue::String(lf(value)));
        }
    }
    TomlValue::Table(table)
}

fn checkpoint_from_toml(value: &TomlValue, index: usize) -> Result<Checkpoint> {
    let table = value
        .as_table()
        .ok_or_else(|| anyhow!("checkpoints[{index}] is not a table"))?;
    let id = str_or(table, "id", "");
    if id.is_empty() {
        return Err(anyhow!("checkpoints[{index}] has no `id`"));
    }
    Ok(Checkpoint {
        id,
        rationale: lf(&str_or(table, "rationale", "")),
    })
}

fn checkpoint_to_toml(checkpoint: &Checkpoint) -> TomlValue {
    let Checkpoint { id, rationale } = checkpoint;
    let mut table = Table::new();
    table.insert("id".to_string(), TomlValue::String(id.clone()));
    table.insert("rationale".to_string(), TomlValue::String(lf(rationale)));
    TomlValue::Table(table)
}

fn override_from_toml(value: &TomlValue, index: usize) -> Result<ImportOverride> {
    let table = value
        .as_table()
        .ok_or_else(|| anyhow!("import_overrides[{index}] is not a table"))?;
    let r#ref = str_or(table, "ref", "");
    if r#ref.is_empty() {
        return Err(anyhow!("import_overrides[{index}] has no `ref`"));
    }
    let context = format!("import_overrides[{index}]");
    Ok(ImportOverride {
        files: table
            .contains_key("files")
            .then(|| str_array(table, "files")),
        needs: match table.contains_key("needs") {
            true => Some(id_array(table, "needs", &context)?),
            false => None,
        },
        r#ref,
    })
}

fn override_to_toml(entry: &ImportOverride) -> TomlValue {
    let ImportOverride {
        r#ref,
        files,
        needs,
    } = entry;
    let mut table = Table::new();
    table.insert("ref".to_string(), TomlValue::String(r#ref.clone()));
    if let Some(files) = files {
        table.insert("files".to_string(), str_arr_value(files));
    }
    if let Some(needs) = needs {
        table.insert("needs".to_string(), id_arr_value(needs));
    }
    TomlValue::Table(table)
}

fn file_note_from_toml(value: &TomlValue, index: usize) -> Result<FileNote> {
    let table = value
        .as_table()
        .ok_or_else(|| anyhow!("file_notes[{index}] is not a table"))?;
    Ok(FileNote {
        r#ref: str_or(table, "ref", ""),
        file: str_or(table, "file", ""),
        note: lf(&str_or(table, "note", "")),
    })
}

fn file_note_to_toml(entry: &FileNote) -> TomlValue {
    let FileNote { r#ref, file, note } = entry;
    let mut table = Table::new();
    table.insert("ref".to_string(), TomlValue::String(r#ref.clone()));
    table.insert("file".to_string(), TomlValue::String(file.clone()));
    table.insert("note".to_string(), TomlValue::String(lf(note)));
    TomlValue::Table(table)
}

fn backlog_link_from_toml(value: &TomlValue, index: usize) -> Result<BacklogLink> {
    let table = value
        .as_table()
        .ok_or_else(|| anyhow!("backlog_links[{index}] is not a table"))?;
    Ok(BacklogLink {
        r#ref: str_or(table, "ref", ""),
        closes: str_array(table, "closes"),
        refs: str_array(table, "refs"),
    })
}

fn backlog_link_to_toml(entry: &BacklogLink) -> TomlValue {
    let BacklogLink {
        r#ref,
        closes,
        refs,
    } = entry;
    let mut table = Table::new();
    table.insert("ref".to_string(), TomlValue::String(r#ref.clone()));
    table.insert("closes".to_string(), str_arr_value(closes));
    table.insert("refs".to_string(), str_arr_value(refs));
    TomlValue::Table(table)
}

fn finding_link_from_toml(value: &TomlValue, index: usize) -> Result<FindingLink> {
    let table = value
        .as_table()
        .ok_or_else(|| anyhow!("finding_links[{index}] is not a table"))?;
    Ok(FindingLink {
        r#ref: str_or(table, "ref", ""),
        store: str_or(table, "store", ""),
        fixes: str_array(table, "fixes"),
        settles: str_array(table, "settles"),
    })
}

fn finding_link_to_toml(entry: &FindingLink) -> TomlValue {
    let FindingLink {
        r#ref,
        store,
        fixes,
        settles,
    } = entry;
    let mut table = Table::new();
    table.insert("ref".to_string(), TomlValue::String(r#ref.clone()));
    table.insert("store".to_string(), TomlValue::String(store.clone()));
    table.insert("fixes".to_string(), str_arr_value(fixes));
    table.insert("settles".to_string(), str_arr_value(settles));
    TomlValue::Table(table)
}

fn spec_from_toml(value: &TomlValue, index: usize) -> Result<TaskSpec> {
    let context = format!("specs[{index}]");
    let table = value
        .as_table()
        .ok_or_else(|| anyhow!("{context} is not a table"))?;
    Ok(TaskSpec {
        r#ref: str_or(table, "ref", ""),
        outcome: lf(&str_or(table, "outcome", "")),
        constraints: lf_array(table, "constraints"),
        accept: sub_tables(table, "accept", &context)?
            .into_iter()
            .map(|entry| accept_from_toml(entry, &context))
            .collect::<Result<Vec<_>>>()?,
        premises: sub_tables(table, "premises", &context)?
            .into_iter()
            .map(|entry| premise_from_toml(entry, &context))
            .collect::<Result<Vec<_>>>()?,
        means: sub_tables(table, "means", &context)?
            .into_iter()
            .map(means_from_toml)
            .collect(),
        design: str_array(table, "design"),
        precedent: str_array(table, "precedent"),
        executor: str_or(table, "executor", ""),
    })
}

fn spec_to_toml(entry: &TaskSpec) -> TomlValue {
    let TaskSpec {
        r#ref,
        outcome,
        constraints,
        accept,
        premises,
        means,
        design,
        precedent,
        executor,
    } = entry;
    let mut table = Table::new();
    table.insert("ref".to_string(), TomlValue::String(r#ref.clone()));
    table.insert("outcome".to_string(), TomlValue::String(lf(outcome)));
    table.insert("constraints".to_string(), lf_arr_value(constraints));
    table.insert(
        "accept".to_string(),
        TomlValue::Array(accept.iter().map(accept_to_toml).collect()),
    );
    table.insert(
        "premises".to_string(),
        TomlValue::Array(premises.iter().map(premise_to_toml).collect()),
    );
    table.insert(
        "means".to_string(),
        TomlValue::Array(means.iter().map(means_to_toml).collect()),
    );
    table.insert("design".to_string(), str_arr_value(design));
    table.insert("precedent".to_string(), str_arr_value(precedent));
    table.insert("executor".to_string(), TomlValue::String(executor.clone()));
    TomlValue::Table(table)
}

fn accept_from_toml(table: &Table, context: &str) -> Result<Accept> {
    let probe = match table.get("probe") {
        Some(raw) => Some(probe_from_toml(raw.as_table().ok_or_else(|| {
            anyhow!("{context}: an `accept` entry's `probe` is not a table")
        })?)),
        None => None,
    };
    Ok(Accept {
        behaviour: lf(&str_or(table, "behaviour", "")),
        check: lf(&str_or(table, "check", "")),
        kind: str_or(table, "kind", ""),
        positive: lf(&str_or(table, "positive", "")),
        writes_build: table
            .get("writes_build")
            .and_then(TomlValue::as_bool)
            .unwrap_or(true),
        probe,
    })
}

fn accept_to_toml(entry: &Accept) -> TomlValue {
    let Accept {
        behaviour,
        check,
        kind,
        positive,
        writes_build,
        probe,
    } = entry;
    let mut table = Table::new();
    table.insert("behaviour".to_string(), TomlValue::String(lf(behaviour)));
    table.insert("check".to_string(), TomlValue::String(lf(check)));
    table.insert("kind".to_string(), TomlValue::String(kind.clone()));
    table.insert("positive".to_string(), TomlValue::String(lf(positive)));
    table.insert(
        "writes_build".to_string(),
        TomlValue::Boolean(*writes_build),
    );
    if let Some(probe) = probe {
        table.insert("probe".to_string(), probe_to_toml(probe));
    }
    TomlValue::Table(table)
}

fn probe_from_toml(table: &Table) -> Probe {
    Probe {
        at: str_or(table, "at", ""),
        neg_rc: table.get("neg_rc").and_then(TomlValue::as_integer),
        pos_rc: table.get("pos_rc").and_then(TomlValue::as_integer),
        verdict: str_or(table, "verdict", ""),
    }
}

fn probe_to_toml(entry: &Probe) -> TomlValue {
    let Probe {
        at,
        neg_rc,
        pos_rc,
        verdict,
    } = entry;
    let mut table = Table::new();
    table.insert("at".to_string(), TomlValue::String(at.clone()));
    if let Some(rc) = neg_rc {
        table.insert("neg_rc".to_string(), TomlValue::Integer(*rc));
    }
    if let Some(rc) = pos_rc {
        table.insert("pos_rc".to_string(), TomlValue::Integer(*rc));
    }
    table.insert("verdict".to_string(), TomlValue::String(verdict.clone()));
    TomlValue::Table(table)
}

fn premise_from_toml(table: &Table, context: &str) -> Result<Premise> {
    let probe = match table.get("probe") {
        Some(raw) => Some(probe_from_toml(raw.as_table().ok_or_else(|| {
            anyhow!("{context}: a `premises` entry's `probe` is not a table")
        })?)),
        None => None,
    };
    Ok(Premise {
        claim: lf(&str_or(table, "claim", "")),
        check: lf(&str_or(table, "check", "")),
        at: str_or(table, "at", ""),
        probe,
    })
}

fn premise_to_toml(entry: &Premise) -> TomlValue {
    let Premise {
        claim,
        check,
        at,
        probe,
    } = entry;
    let mut table = Table::new();
    table.insert("claim".to_string(), TomlValue::String(lf(claim)));
    table.insert("check".to_string(), TomlValue::String(lf(check)));
    table.insert("at".to_string(), TomlValue::String(at.clone()));
    if let Some(probe) = probe {
        table.insert("probe".to_string(), probe_to_toml(probe));
    }
    TomlValue::Table(table)
}

fn means_from_toml(table: &Table) -> Means {
    Means {
        text: lf(&str_or(table, "text", "")),
        binding: str_or(table, "binding", ""),
        detail: lf(&str_or(table, "detail", "")),
    }
}

fn means_to_toml(entry: &Means) -> TomlValue {
    let Means {
        text,
        binding,
        detail,
    } = entry;
    let mut table = Table::new();
    table.insert("text".to_string(), TomlValue::String(lf(text)));
    table.insert("binding".to_string(), TomlValue::String(binding.clone()));
    table.insert("detail".to_string(), TomlValue::String(lf(detail)));
    TomlValue::Table(table)
}

fn milestone_from_toml(value: &TomlValue, index: usize) -> Result<MilestoneSpec> {
    let table = value
        .as_table()
        .ok_or_else(|| anyhow!("milestones[{index}] is not a table"))?;
    Ok(MilestoneSpec {
        id: str_or(table, "id", ""),
        gates: lf_array(table, "gates"),
        elaboration: str_or(table, "elaboration", ELABORATION_UPFRONT),
        defer_reason: lf(&str_or(table, "defer_reason", "")),
    })
}

fn milestone_to_toml(entry: &MilestoneSpec) -> TomlValue {
    let MilestoneSpec {
        id,
        gates,
        elaboration,
        defer_reason,
    } = entry;
    let mut table = Table::new();
    table.insert("id".to_string(), TomlValue::String(id.clone()));
    table.insert("gates".to_string(), lf_arr_value(gates));
    table.insert(
        "elaboration".to_string(),
        TomlValue::String(elaboration.clone()),
    );
    table.insert(
        "defer_reason".to_string(),
        TomlValue::String(lf(defer_reason)),
    );
    TomlValue::Table(table)
}

fn change_from_toml(value: &TomlValue, index: usize) -> Result<Change> {
    let table = value
        .as_table()
        .ok_or_else(|| anyhow!("changes[{index}] is not a table"))?;
    Ok(Change {
        at: table.get("at").and_then(as_date),
        r#ref: str_or(table, "ref", ""),
        field: str_or(table, "field", ""),
        old: lf(&str_or(table, "old", "")),
        new: lf(&str_or(table, "new", "")),
        actor: str_or(table, "actor", ""),
        reason: lf(&str_or(table, "reason", "")),
    })
}

fn change_to_toml(entry: &Change) -> TomlValue {
    let Change {
        at,
        r#ref,
        field,
        old,
        new,
        actor,
        reason,
    } = entry;
    let mut table = Table::new();
    if let Some(date) = at {
        table.insert("at".to_string(), TomlValue::Datetime(*date));
    }
    table.insert("ref".to_string(), TomlValue::String(r#ref.clone()));
    table.insert("field".to_string(), TomlValue::String(field.clone()));
    table.insert("old".to_string(), TomlValue::String(lf(old)));
    table.insert("new".to_string(), TomlValue::String(lf(new)));
    table.insert("actor".to_string(), TomlValue::String(actor.clone()));
    table.insert("reason".to_string(), TomlValue::String(lf(reason)));
    TomlValue::Table(table)
}

fn row_from_toml(value: &TomlValue, index: usize) -> Result<TaskRow> {
    let table = value
        .as_table()
        .ok_or_else(|| anyhow!("items[{index}] is not a table"))?;
    let id = table
        .get("id")
        .and_then(TomlValue::as_integer)
        .and_then(|n| u32::try_from(n).ok())
        .ok_or_else(|| anyhow!("items[{index}] has no non-negative integer `id`"))?;

    let r#ref = str_or(table, "ref", "");
    if r#ref.is_empty() {
        return Err(anyhow!("task {id}: `ref` is missing or empty"));
    }
    let title = str_or(table, "title", "");
    if title.is_empty() {
        return Err(anyhow!("task {id}: `title` is missing or empty"));
    }
    let effort = match table.get("effort") {
        Some(raw) => Effort::parse_in_row(
            raw.as_str()
                .ok_or_else(|| anyhow!("task {id}: `effort` is not a string"))?,
            id,
        )?,
        None => return Err(anyhow!("task {id}: `effort` is missing")),
    };
    let status = match table.get("status") {
        Some(raw) => Status::parse_in_row(
            raw.as_str()
                .ok_or_else(|| anyhow!("task {id}: `status` is not a string"))?,
            id,
        )?,
        None => Status::default(),
    };

    let row_context = format!("task {id}");
    Ok(TaskRow {
        id,
        r#ref,
        title,
        effort,
        status,
        checkpoint: str_or(table, "checkpoint", ""),
        phase: str_or(table, "phase", ""),
        phase_depth: u32_or(table, "phase_depth", 0),
        heading_depth: u32_or(table, "heading_depth", DEFAULT_HEADING_DEPTH),
        files: str_array(table, "files"),
        needs: id_array(table, "needs", &row_context)?,
        coupling: id_array(table, "coupling", &row_context)?,
        deps_note: lf(&str_or(table, "deps_note", "")),
        action: lf(&str_or(table, "action", "")),
        detail: lf(&str_or(table, "detail", "")),
        acceptance: lf(&str_or(table, "acceptance", "")),
        agent: str_or(table, "agent", ""),
        commit: str_or(table, "commit", ""),
    })
}

fn row_to_toml(row: &TaskRow) -> TomlValue {
    let TaskRow {
        id,
        r#ref,
        title,
        effort,
        status,
        checkpoint,
        phase,
        phase_depth,
        heading_depth,
        files,
        needs,
        coupling,
        deps_note,
        action,
        detail,
        acceptance,
        agent,
        commit,
    } = row;

    let mut table = Table::new();
    table.insert("id".to_string(), TomlValue::Integer(i64::from(*id)));
    table.insert("ref".to_string(), TomlValue::String(r#ref.clone()));
    table.insert("title".to_string(), TomlValue::String(title.clone()));
    table.insert(
        "effort".to_string(),
        TomlValue::String(effort.as_str().to_string()),
    );
    table.insert(
        "status".to_string(),
        TomlValue::String(status.as_str().to_string()),
    );
    table.insert(
        "checkpoint".to_string(),
        TomlValue::String(checkpoint.clone()),
    );
    table.insert("phase".to_string(), TomlValue::String(phase.clone()));
    table.insert(
        "phase_depth".to_string(),
        TomlValue::Integer(i64::from(*phase_depth)),
    );
    table.insert(
        "heading_depth".to_string(),
        TomlValue::Integer(i64::from(*heading_depth)),
    );
    table.insert("files".to_string(), str_arr_value(files));
    table.insert("needs".to_string(), id_arr_value(needs));
    table.insert("coupling".to_string(), id_arr_value(coupling));
    table.insert("deps_note".to_string(), TomlValue::String(lf(deps_note)));
    table.insert("action".to_string(), TomlValue::String(lf(action)));
    table.insert("detail".to_string(), TomlValue::String(lf(detail)));
    table.insert("acceptance".to_string(), TomlValue::String(lf(acceptance)));
    table.insert("agent".to_string(), TomlValue::String(agent.clone()));
    table.insert("commit".to_string(), TomlValue::String(commit.clone()));
    TomlValue::Table(table)
}

/// Collapse CRLF and lone CR to LF.
pub(crate) fn lf(body: &str) -> String {
    if body.contains('\r') {
        body.replace("\r\n", "\n").replace('\r', "\n")
    } else {
        body.to_string()
    }
}

/// A bare TOML date or datetime, also accepting the quoted form a hand edit
/// leaves behind. Anything else reads as absent, for the caller to default or
/// refuse.
pub(crate) fn as_date(value: &TomlValue) -> Option<Datetime> {
    match value {
        TomlValue::Datetime(date) => Some(*date),
        TomlValue::String(raw) => raw.parse().ok(),
        _ => None,
    }
}

fn str_or(table: &Table, key: &str, fallback: &str) -> String {
    table
        .get(key)
        .and_then(TomlValue::as_str)
        .unwrap_or(fallback)
        .to_string()
}

/// A non-integer here costs a heading level, never a task, so it reads as the
/// fallback rather than refusing the whole store.
fn u32_or(table: &Table, key: &str, fallback: u32) -> u32 {
    table
        .get(key)
        .and_then(TomlValue::as_integer)
        .and_then(|n| u32::try_from(n).ok())
        .unwrap_or(fallback)
}

pub(crate) fn str_array(table: &Table, key: &str) -> Vec<String> {
    table
        .get(key)
        .and_then(TomlValue::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(TomlValue::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn lf_array(table: &Table, key: &str) -> Vec<String> {
    str_array(table, key).iter().map(|s| lf(s)).collect()
}

/// The tables under `key`, or none when absent. Refuses rather than skips a
/// malformed entry, as `table_array` does, naming the spec that holds it.
fn sub_tables<'a>(table: &'a Table, key: &str, context: &str) -> Result<Vec<&'a Table>> {
    let Some(raw) = table.get(key) else {
        return Ok(Vec::new());
    };
    raw.as_array()
        .ok_or_else(|| anyhow!("{context}: `{key}` is not an array of tables"))?
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            entry
                .as_table()
                .ok_or_else(|| anyhow!("{context}: `{key}[{index}]` is not a table"))
        })
        .collect()
}

pub(crate) fn id_array(table: &Table, key: &str, context: &str) -> Result<Vec<u32>> {
    let Some(raw) = table.get(key) else {
        return Ok(Vec::new());
    };
    let arr = raw
        .as_array()
        .ok_or_else(|| anyhow!("{context}: `{key}` is not an array"))?;
    arr.iter()
        .map(|entry| {
            entry
                .as_integer()
                .and_then(|n| u32::try_from(n).ok())
                .ok_or_else(|| anyhow!("{context}: `{key}` holds a non-id entry `{entry}`"))
        })
        .collect()
}

/// The array under `key`, or an empty slice when absent. A present
/// non-array is an error rather than an empty read — silently dropping a
/// hand-edited `items` table would look like an empty store.
fn table_array<'a>(table: &'a Table, key: &str) -> Result<&'a [TomlValue]> {
    match table.get(key) {
        None => Ok(&[]),
        Some(raw) => raw
            .as_array()
            .map(Vec::as_slice)
            .ok_or_else(|| anyhow!("`{key}` is not an array of tables")),
    }
}

pub(crate) fn str_arr_value(values: &[String]) -> TomlValue {
    TomlValue::Array(
        values
            .iter()
            .map(|s| TomlValue::String(s.clone()))
            .collect(),
    )
}

fn lf_arr_value(values: &[String]) -> TomlValue {
    TomlValue::Array(values.iter().map(|s| TomlValue::String(lf(s))).collect())
}

fn id_arr_value(values: &[u32]) -> TomlValue {
    TomlValue::Array(
        values
            .iter()
            .map(|id| TomlValue::Integer(i64::from(*id)))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Store {
        Store {
            schema_version: BASE_SCHEMA_VERSION,
            last_updated: Some("2026-09-07".parse().expect("bare date parses")),
            plan_path: "docs/plans/whimsical-hugging-puppy.md".to_string(),
            last_import_refs: vec![
                "seed-the-store".to_string(),
                "wire-the-renderer".to_string(),
            ],
            minted_refs: Vec::new(),
            policy: Policy {
                checkpoints: "milestones".to_string(),
                max_parallel: 6,
                commit_granularity: "per-task".to_string(),
                origin: POLICY_ORIGIN_PLAN.to_string(),
                note: "The trains are cut by hand.".to_string(),
                checkpoints_note: String::new(),
                max_parallel_note: String::new(),
                commit_granularity_note: "— tasks 6 and 7 land in one commit".to_string(),
            },
            checkpoints: vec![Checkpoint {
                id: "A".to_string(),
                rationale: "shared kit and the document panel — buildable alone".to_string(),
            }],
            import_overrides: vec![ImportOverride {
                r#ref: "wire-the-renderer".to_string(),
                files: Some(vec!["tomlctl/src/tasks/render.rs".to_string()]),
                needs: None,
            }],
            file_notes: vec![FileNote {
                r#ref: "wire-the-renderer".to_string(),
                file: "tomlctl/src/tasks/markdown.rs".to_string(),
                note: "(new)".to_string(),
            }],
            backlog_links: vec![BacklogLink {
                r#ref: "wire-the-renderer".to_string(),
                closes: vec!["B3".to_string()],
                refs: vec!["B1".to_string(), "B7".to_string()],
            }],
            finding_links: Vec::new(),
            audit: false,
            id_high_water: 0,
            specs: Vec::new(),
            milestones: Vec::new(),
            changes: Vec::new(),
            items: vec![
                TaskRow {
                    id: 1,
                    r#ref: "seed-the-store".to_string(),
                    title: "Seed the store".to_string(),
                    effort: Effort::S,
                    status: Status::Done,
                    checkpoint: "A".to_string(),
                    phase: "Milestone A — the store".to_string(),
                    phase_depth: 3,
                    heading_depth: 4,
                    files: vec!["tomlctl/src/tasks/store.rs".to_string()],
                    needs: Vec::new(),
                    coupling: Vec::new(),
                    deps_note: String::new(),
                    action: "Seed it.".to_string(),
                    detail: String::new(),
                    acceptance: "The seed exists.".to_string(),
                    agent: "implement-lite".to_string(),
                    commit: "0d1bf49".to_string(),
                },
                TaskRow {
                    id: 12,
                    r#ref: "wire-the-renderer".to_string(),
                    title: "Wire the renderer".to_string(),
                    effort: Effort::L,
                    status: Status::InProgress,
                    checkpoint: String::new(),
                    phase: String::new(),
                    phase_depth: 0,
                    heading_depth: DEFAULT_HEADING_DEPTH,
                    files: vec![
                        "tomlctl/src/tasks/render.rs".to_string(),
                        "tomlctl/src/tasks/markdown.rs".to_string(),
                    ],
                    needs: vec![1],
                    coupling: vec![1],
                    deps_note: "1 is what this task needs".to_string(),
                    action: "First line.\n\nSecond line.".to_string(),
                    detail: "Rewrite the three sections in place.".to_string(),
                    acceptance: "Round-trip holds.".to_string(),
                    agent: String::new(),
                    commit: String::new(),
                },
            ],
        }
    }

    fn row_mut(doc: &mut TomlValue, index: usize) -> &mut Table {
        doc.as_table_mut()
            .expect("root table")
            .get_mut("items")
            .expect("items array")
            .as_array_mut()
            .expect("items is an array")[index]
            .as_table_mut()
            .expect("row table")
    }

    #[test]
    fn store_round_trips_through_toml() {
        let store = fixture();
        assert_eq!(from_toml(&to_toml(&store)).expect("parses"), store);
    }

    #[test]
    fn crlf_bodies_normalise_to_lf() {
        let mut store = fixture();
        store.items[1].action = "First line.\r\nSecond line.".to_string();
        let mut doc = to_toml(&store);
        assert_eq!(
            row_mut(&mut doc, 1)
                .get("action")
                .and_then(TomlValue::as_str),
            Some("First line.\nSecond line.")
        );

        let mut raw = to_toml(&fixture());
        row_mut(&mut raw, 1).insert(
            "action".to_string(),
            TomlValue::String("First line.\r\nSecond line.".to_string()),
        );
        assert_eq!(
            from_toml(&raw).expect("parses").items[1].action,
            "First line.\nSecond line."
        );
    }

    #[test]
    fn unknown_status_names_the_row_id() {
        let mut raw = to_toml(&fixture());
        row_mut(&mut raw, 1).insert("status".to_string(), TomlValue::String("ready".to_string()));
        let message = from_toml(&raw)
            .expect_err("unknown status rejected")
            .to_string();
        assert!(message.contains("12"), "{message}");
        assert!(message.contains("ready"), "{message}");
    }

    /// A store written before the heading keys existed must still render where
    /// it always did, so their absence reads as "no phase, three hashes".
    #[test]
    fn a_row_without_the_heading_keys_takes_the_rendering_defaults() {
        let mut raw = to_toml(&fixture());
        for key in ["phase", "phase_depth", "heading_depth"] {
            row_mut(&mut raw, 0).remove(key);
        }
        let row = &from_toml(&raw).expect("parses").items[0];
        assert_eq!(row.phase, "");
        assert_eq!(row.phase_depth, 0);
        assert_eq!(row.heading_depth, DEFAULT_HEADING_DEPTH);
    }

    /// A store written before `origin` existed carries the absence in the one
    /// place it had: the note the renderer would otherwise stamp back into the
    /// plan as authored prose. Reading it into `origin` vacates the note, so
    /// the next write drops the sentence.
    #[test]
    fn a_policy_without_origin_reads_plan_unless_it_carries_the_older_marker() {
        fn policy(doc: &mut TomlValue) -> &mut Table {
            doc.as_table_mut()
                .expect("root table")
                .get_mut("policy")
                .expect("policy table")
                .as_table_mut()
                .expect("policy is a table")
        }

        let mut raw = to_toml(&fixture());
        policy(&mut raw).remove("origin");
        let read = from_toml(&raw).expect("parses").policy;
        assert_eq!(read.origin, POLICY_ORIGIN_PLAN);
        assert_eq!(read.note, "The trains are cut by hand.");

        policy(&mut raw).insert(
            "note".to_string(),
            TomlValue::String(LEGACY_ABSENT_NOTE.to_string()),
        );
        let read = from_toml(&raw).expect("parses").policy;
        assert_eq!(read.origin, POLICY_ORIGIN_DEFAULT);
        assert_eq!(read.note, "");
    }

    /// The key is absent from every store written before it existed, and a
    /// store nothing has hand-patched must write back without it rather than
    /// growing an empty array.
    #[test]
    fn an_override_free_store_neither_reads_nor_writes_the_key() {
        let mut store = fixture();
        store.import_overrides = Vec::new();
        let doc = to_toml(&store);
        assert!(
            !doc.as_table()
                .expect("root table")
                .contains_key("import_overrides")
        );
        assert_eq!(from_toml(&doc).expect("parses").import_overrides, vec![]);
    }

    /// An entry naming no overridden field pins nothing, so it is dropped on
    /// both sides rather than kept as a row nothing can clear.
    #[test]
    fn an_override_entry_with_no_field_is_dropped() {
        let mut raw = to_toml(&fixture());
        raw.as_table_mut()
            .expect("root table")
            .insert("import_overrides".to_string(), {
                let mut table = Table::new();
                table.insert("ref".to_string(), TomlValue::String("seed".to_string()));
                TomlValue::Array(vec![TomlValue::Table(table)])
            });
        assert_eq!(from_toml(&raw).expect("parses").import_overrides, vec![]);
    }

    /// An empty base is not an absent one: a plan that stated no files at all
    /// is what a `files = []` override was taken against.
    #[test]
    fn an_empty_override_base_survives_the_round_trip() {
        let mut store = fixture();
        store.import_overrides = vec![ImportOverride {
            r#ref: "seed-the-store".to_string(),
            files: Some(Vec::new()),
            needs: Some(vec![1]),
        }];
        let read = from_toml(&to_toml(&store)).expect("parses");
        assert_eq!(read.import_overrides, store.import_overrides);
        assert_eq!(
            read.find_override("seed-the-store")
                .and_then(|entry| entry.files.as_ref()),
            Some(&Vec::new())
        );
    }

    /// Both keys are absent from every store written before they existed, and
    /// a store carrying neither must write back without them rather than
    /// growing an empty array and three empty strings.
    #[test]
    fn an_unannotated_store_neither_reads_nor_writes_the_note_keys() {
        let mut store = fixture();
        store.file_notes = Vec::new();
        store.policy.commit_granularity_note = String::new();
        let doc = to_toml(&store);
        let root = doc.as_table().expect("root table");
        assert!(!root.contains_key("file_notes"));
        for key in [
            "checkpoints_note",
            "max_parallel_note",
            "commit_granularity_note",
        ] {
            assert!(
                !root
                    .get("policy")
                    .and_then(TomlValue::as_table)
                    .expect("policy table")
                    .contains_key(key),
                "`{key}` was written empty"
            );
        }

        let read = from_toml(&doc).expect("parses");
        assert_eq!(read.file_notes, vec![]);
        assert_eq!(read.policy, store.policy);
    }

    /// An entry naming no path or carrying no note annotates nothing, so it is
    /// dropped on both sides rather than kept as a row nothing can clear.
    #[test]
    fn a_file_note_with_no_path_or_no_note_is_dropped() {
        let mut store = fixture();
        store.file_notes.push(FileNote {
            r#ref: "seed-the-store".to_string(),
            file: "tomlctl/src/tasks/store.rs".to_string(),
            note: "  ".to_string(),
        });
        store.file_notes.push(FileNote {
            r#ref: "seed-the-store".to_string(),
            file: String::new(),
            note: "(new)".to_string(),
        });
        let read = from_toml(&to_toml(&store)).expect("parses");
        assert_eq!(read.file_notes, fixture().file_notes);
        assert_eq!(
            read.file_note("wire-the-renderer", "tomlctl/src/tasks/markdown.rs"),
            Some("(new)")
        );
        assert_eq!(read.file_note("wire-the-renderer", "nothing.rs"), None);
    }

    #[test]
    fn backlog_links_round_trip_and_answer_by_ref() {
        let read = from_toml(&to_toml(&fixture())).expect("parses");
        assert_eq!(read.backlog_links, fixture().backlog_links);
        assert_eq!(
            read.links_for("wire-the-renderer")
                .map(|link| link.closes.clone()),
            Some(vec!["B3".to_string()])
        );
        assert_eq!(read.links_for("seed-the-store"), None);
    }

    /// A store linking nothing writes back without the key, and an entry
    /// naming no item is dropped on both sides.
    #[test]
    fn an_unlinked_store_neither_reads_nor_writes_backlog_links() {
        let mut store = fixture();
        store.backlog_links = vec![BacklogLink {
            r#ref: "seed-the-store".to_string(),
            closes: Vec::new(),
            refs: Vec::new(),
        }];
        let doc = to_toml(&store);
        assert!(
            !doc.as_table()
                .expect("root table")
                .contains_key("backlog_links")
        );
        assert_eq!(from_toml(&doc).expect("parses").backlog_links, vec![]);

        let mut raw = to_toml(&fixture());
        raw.as_table_mut()
            .expect("root table")
            .insert("backlog_links".to_string(), {
                let mut table = Table::new();
                table.insert("ref".to_string(), TomlValue::String("seed".to_string()));
                TomlValue::Array(vec![TomlValue::Table(table)])
            });
        assert_eq!(from_toml(&raw).expect("parses").backlog_links, vec![]);
    }

    /// Every side table populated and `audit` set, with every optional shape
    /// present at least once: a probe with and without a positive control, a
    /// `writes_build = false` row, a dated change.
    fn v2_fixture() -> Store {
        Store {
            schema_version: SCHEMA_VERSION,
            audit: true,
            id_high_water: 40,
            specs: vec![TaskSpec {
                r#ref: "wire-the-renderer".to_string(),
                outcome: "the render round-trips\nacross two lines".to_string(),
                constraints: vec!["must not change the public signature".to_string()],
                accept: vec![
                    Accept {
                        behaviour: "a retry after 429 waits the header's delay".to_string(),
                        check: "cargo nextest run -E 'test(retry_after)'".to_string(),
                        kind: "forward".to_string(),
                        positive: "printf ok".to_string(),
                        writes_build: false,
                        probe: Some(Probe {
                            at: "0d1bf49".to_string(),
                            neg_rc: Some(1),
                            pos_rc: Some(0),
                            verdict: "healthy".to_string(),
                        }),
                    },
                    Accept {
                        behaviour: "the guard still holds".to_string(),
                        check: "true".to_string(),
                        kind: "guard".to_string(),
                        positive: String::new(),
                        writes_build: false,
                        probe: Some(Probe {
                            at: "0d1bf49".to_string(),
                            neg_rc: Some(0),
                            pos_rc: None,
                            verdict: "guard-alone".to_string(),
                        }),
                    },
                    Accept {
                        behaviour: "the suite passes".to_string(),
                        check: "cargo test".to_string(),
                        kind: "guard".to_string(),
                        positive: String::new(),
                        writes_build: true,
                        probe: None,
                    },
                ],
                premises: vec![Premise {
                    claim: "fn parse exists in src/a.rs".to_string(),
                    check: "grep -q 'fn parse' src/a.rs".to_string(),
                    at: "after:1".to_string(),
                    probe: Some(Probe {
                        at: "0d1bf49".to_string(),
                        neg_rc: Some(0),
                        pos_rc: None,
                        verdict: "holds".to_string(),
                    }),
                }],
                means: vec![Means {
                    text: "use the backoff helper".to_string(),
                    binding: "required".to_string(),
                    detail: "why this is required".to_string(),
                }],
                design: vec!["design/store.md#locking".to_string()],
                precedent: vec!["src/b.rs:40".to_string()],
                executor: "lite".to_string(),
            }],
            milestones: vec![MilestoneSpec {
                id: "A".to_string(),
                gates: vec!["cargo nextest run --manifest-path x/Cargo.toml".to_string()],
                elaboration: "deferred".to_string(),
                defer_reason: "depends on what A produces".to_string(),
            }],
            changes: vec![Change {
                at: Some("2026-11-05".parse().expect("bare date parses")),
                r#ref: "wire-the-renderer".to_string(),
                field: "spec.outcome".to_string(),
                old: "before".to_string(),
                new: "after".to_string(),
                actor: "orchestrator".to_string(),
                reason: "the probe failed".to_string(),
            }],
            finding_links: vec![FindingLink {
                r#ref: "wire-the-renderer".to_string(),
                store: ".claude/review-v2/auth.toml".to_string(),
                fixes: vec!["F3".to_string(), "F7".to_string()],
                settles: Vec::new(),
            }],
            ..fixture()
        }
    }

    fn root_mut(doc: &mut TomlValue) -> &mut Table {
        doc.as_table_mut().expect("root table")
    }

    /// The bytes a store with none of the v2 keys has always had on disk. Held
    /// as a literal so an emitted empty table or a raised stamp cannot pass by
    /// moving both sides of the comparison.
    #[test]
    fn a_v1_store_round_trips_byte_identical_at_version_1() {
        const V1: &str = r#"schema_version = 1
last_updated = 2026-09-07
plan_path = "docs/plans/x.md"
last_import_refs = ["seed-the-store"]

[policy]
checkpoints = "milestones"
max_parallel = 6
commit_granularity = "per-task"
origin = "plan"
note = ""

[[checkpoints]]
id = "A"
rationale = "the store"

[[items]]
id = 1
ref = "seed-the-store"
title = "Seed the store"
effort = "S"
status = "pending"
checkpoint = "A"
phase = ""
phase_depth = 0
heading_depth = 3
files = ["tomlctl/src/tasks/store.rs"]
needs = []
coupling = []
deps_note = ""
action = "Seed it."
detail = ""
acceptance = ""
agent = ""
commit = ""
"#;
        let doc: TomlValue = toml::from_str(V1).expect("the literal parses");
        let store = from_toml(&doc).expect("parses");
        assert_eq!(store.schema_version, BASE_SCHEMA_VERSION);
        assert_eq!(
            toml::to_string(&to_toml(&store)).expect("serialises"),
            V1,
            "a store holding no v2 key changed on the way through"
        );
    }

    #[test]
    fn a_store_with_every_v2_key_round_trips_exactly_at_version_2() {
        let store = v2_fixture();
        let text = toml::to_string(&to_toml(&store)).expect("serialises");
        assert!(text.starts_with("schema_version = 2\n"), "{text}");
        assert!(text.contains("\naudit = true\n"), "{text}");

        let reparsed: TomlValue = toml::from_str(&text).expect("the output parses");
        let read = from_toml(&reparsed).expect("parses");
        assert_eq!(read, store);
        assert_eq!(toml::to_string(&to_toml(&read)).expect("serialises"), text);
    }

    /// Any one of the six is enough: the older binary refuses version 2 but
    /// drops each of these keys silently at version 1.
    #[test]
    fn each_v2_key_alone_raises_the_stamp() {
        let full = v2_fixture();
        let only: [(&str, Store); 6] = [
            (
                "finding_links",
                Store {
                    finding_links: full.finding_links.clone(),
                    ..fixture()
                },
            ),
            (
                "specs",
                Store {
                    specs: full.specs.clone(),
                    ..fixture()
                },
            ),
            (
                "milestones",
                Store {
                    milestones: full.milestones.clone(),
                    ..fixture()
                },
            ),
            (
                "changes",
                Store {
                    changes: full.changes.clone(),
                    ..fixture()
                },
            ),
            (
                "audit",
                Store {
                    audit: true,
                    ..fixture()
                },
            ),
            (
                "id_high_water",
                Store {
                    id_high_water: 40,
                    ..fixture()
                },
            ),
        ];
        for (key, store) in only {
            let doc = to_toml(&store);
            let root = doc.as_table().expect("root table");
            assert_eq!(
                root.get("schema_version"),
                Some(&TomlValue::Integer(SCHEMA_VERSION)),
                "`{key}` alone left the stamp at the base version"
            );
            assert!(root.contains_key(key), "`{key}` was not written");
        }
    }

    /// The stamp follows the content, so a store emptied of its side tables
    /// goes back to version 1 rather than keeping the version it was read at.
    #[test]
    fn a_store_emptied_of_v2_keys_stamps_the_base_version() {
        let store = Store {
            schema_version: SCHEMA_VERSION,
            ..fixture()
        };
        let doc = to_toml(&store);
        let root = doc.as_table().expect("root table");
        assert_eq!(
            root.get("schema_version"),
            Some(&TomlValue::Integer(BASE_SCHEMA_VERSION))
        );
        for key in [
            "audit",
            "id_high_water",
            "specs",
            "milestones",
            "changes",
            "finding_links",
        ] {
            assert!(!root.contains_key(key), "`{key}` was written while empty");
        }
    }

    #[test]
    fn the_default_store_stays_at_the_base_version() {
        assert_eq!(Store::default().schema_version, BASE_SCHEMA_VERSION);
        assert_eq!(
            to_toml(&Store::default())
                .as_table()
                .expect("root table")
                .get("schema_version"),
            Some(&TomlValue::Integer(BASE_SCHEMA_VERSION))
        );
    }

    /// An entry with no key can never be looked up, so it is dropped on read
    /// and, on write, neither emitted nor counted toward the stamp.
    #[test]
    fn an_entry_with_no_key_is_dropped_on_both_sides() {
        let mut raw = to_toml(&fixture());
        for key in ["specs", "milestones", "changes", "finding_links"] {
            let mut entry = Table::new();
            entry.insert("outcome".to_string(), TomlValue::String("x".to_string()));
            root_mut(&mut raw).insert(
                key.to_string(),
                TomlValue::Array(vec![TomlValue::Table(entry)]),
            );
        }
        let read = from_toml(&raw).expect("parses");
        assert_eq!(read.specs, vec![]);
        assert_eq!(read.milestones, vec![]);
        assert_eq!(read.changes, vec![]);
        assert_eq!(read.finding_links, vec![]);

        let mut keyless = v2_fixture();
        keyless.audit = false;
        keyless.id_high_water = 0;
        keyless.specs[0].r#ref = String::new();
        keyless.milestones[0].id = String::new();
        keyless.changes[0].r#ref = String::new();
        keyless.finding_links[0].r#ref = String::new();
        let doc = to_toml(&keyless);
        let root = doc.as_table().expect("root table");
        assert_eq!(
            root.get("schema_version"),
            Some(&TomlValue::Integer(BASE_SCHEMA_VERSION))
        );
        for key in ["specs", "milestones", "changes", "finding_links"] {
            assert!(!root.contains_key(key), "`{key}` kept a keyless entry");
        }
    }

    #[test]
    fn an_absent_writes_build_reads_true_and_an_explicit_false_survives() {
        let accept = |extra: &str| -> Accept {
            let text = format!(
                "[[specs]]\nref = \"r\"\n[[specs.accept]]\nbehaviour = \"b\"\n\
                 check = \"c\"\n{extra}"
            );
            let doc: TomlValue = toml::from_str(&text).expect("the fixture parses");
            from_toml(&doc)
                .expect("parses")
                .specs
                .remove(0)
                .accept
                .remove(0)
        };
        assert!(accept("").writes_build);
        assert!(accept("writes_build = true\n").writes_build);
        assert!(!accept("writes_build = false\n").writes_build);
        assert_eq!(accept("").probe, None);
    }

    /// A spec holding a bare string where a table belongs is a damaged store,
    /// not an empty spec, and reading it as empty would let the next write
    /// erase it.
    #[test]
    fn a_malformed_spec_entry_names_where_it_is() {
        let refused = |text: &str| -> String {
            let doc: TomlValue = toml::from_str(text).expect("the fixture parses");
            from_toml(&doc)
                .expect_err("malformed spec rejected")
                .to_string()
        };
        let message = refused("[[specs]]\nref = \"r\"\naccept = \"nope\"\n");
        assert!(message.contains("specs[0]"), "{message}");
        assert!(message.contains("accept"), "{message}");
        let message = refused("[[specs]]\nref = \"r\"\nmeans = [\"nope\"]\n");
        assert!(message.contains("means[0]"), "{message}");
        let message = refused("[[specs]]\nref = \"r\"\n[[specs.accept]]\nprobe = \"nope\"\n");
        assert!(message.contains("probe"), "{message}");
        let message = refused("[[specs]]\nref = \"r\"\n[[specs.premises]]\nprobe = \"nope\"\n");
        assert!(message.contains("`premises` entry's `probe`"), "{message}");
    }

    /// Absent reads as off, but a value that is not a boolean must not: that
    /// would silently lift auditing from an approved plan.
    #[test]
    fn a_non_boolean_audit_is_refused() {
        let mut raw = to_toml(&fixture());
        root_mut(&mut raw).insert("audit".to_string(), TomlValue::String("true".to_string()));
        let message = from_toml(&raw).expect_err("rejected").to_string();
        assert!(message.contains("audit"), "{message}");
    }

    /// Reading a damaged mark as `0` would hand a retired id to the next task.
    #[test]
    fn a_malformed_id_high_water_is_refused() {
        for bad in [TomlValue::Integer(-1), TomlValue::String("7".to_string())] {
            let mut raw = to_toml(&fixture());
            root_mut(&mut raw).insert("id_high_water".to_string(), bad);
            let message = from_toml(&raw).expect_err("rejected").to_string();
            assert!(message.contains("id_high_water"), "{message}");
        }
    }

    #[test]
    fn a_mark_above_every_row_sets_the_next_id_and_one_below_does_not() {
        let mark = |id_high_water| Store {
            id_high_water,
            ..fixture()
        };
        assert_eq!(mark(40).next_id(), 41);
        assert_eq!(mark(3).next_id(), 13);
        let emptied = Store {
            id_high_water: 40,
            ..Store::default()
        };
        assert_eq!(emptied.next_id(), 41);
    }

    #[test]
    fn a_milestone_recording_no_elaboration_reads_upfront() {
        let doc: TomlValue = toml::from_str("[[milestones]]\nid = \"A\"\n").expect("parses");
        let read = from_toml(&doc).expect("parses");
        assert_eq!(read.milestones[0].elaboration, ELABORATION_UPFRONT);
        assert_eq!(read.milestones[0].gates, Vec::<String>::new());
    }

    #[test]
    fn side_table_bodies_normalise_to_lf_on_write_and_read() {
        let mut store = v2_fixture();
        store.specs[0].outcome = "one\r\ntwo".to_string();
        store.specs[0].constraints = vec!["three\r\nfour".to_string()];
        store.specs[0].accept[0].behaviour = "five\rsix".to_string();
        store.changes[0].old = "seven\r\neight".to_string();
        let doc = to_toml(&store);
        let read = from_toml(&doc).expect("parses");
        assert_eq!(read.specs[0].outcome, "one\ntwo");
        assert_eq!(read.specs[0].constraints, vec!["three\nfour".to_string()]);
        assert_eq!(read.specs[0].accept[0].behaviour, "five\nsix");
        assert_eq!(read.changes[0].old, "seven\neight");

        let raw: TomlValue =
            toml::from_str("[[specs]]\nref = \"r\"\noutcome = \"one\\r\\ntwo\"\n").expect("parses");
        assert_eq!(
            from_toml(&raw).expect("parses").specs[0].outcome,
            "one\ntwo"
        );
    }

    /// A row claiming a freed `ref` must not inherit the old spec, but the
    /// tombstone for the removed row has to stay readable.
    #[test]
    fn dropping_a_ref_takes_its_spec_and_leaves_its_changes() {
        let mut store = v2_fixture();
        store.specs.push(TaskSpec {
            r#ref: "seed-the-store".to_string(),
            ..store.specs[0].clone()
        });
        store.drop_ref_entries("wire-the-renderer");
        assert_eq!(
            store
                .specs
                .iter()
                .map(|entry| entry.r#ref.as_str())
                .collect::<Vec<_>>(),
            vec!["seed-the-store"]
        );
        assert_eq!(store.changes, v2_fixture().changes);
        assert_eq!(store.file_notes, vec![]);
        assert_eq!(store.backlog_links, vec![]);
    }

    #[test]
    fn newer_schema_version_is_refused() {
        let mut raw = to_toml(&fixture());
        raw.as_table_mut().expect("root table").insert(
            "schema_version".to_string(),
            TomlValue::Integer(SCHEMA_VERSION + 1),
        );
        let message = from_toml(&raw)
            .expect_err("newer schema rejected")
            .to_string();
        assert!(
            message.contains(&(SCHEMA_VERSION + 1).to_string()),
            "{message}"
        );
        assert!(message.contains(&SCHEMA_VERSION.to_string()), "{message}");
    }

    #[test]
    fn lookups_and_next_id_span_the_rows() {
        let store = fixture();
        assert_eq!(store.next_id(), 13);
        assert_eq!(
            store.find(12).map(|row| row.r#ref.as_str()),
            Some("wire-the-renderer")
        );
        assert_eq!(store.find(99), None);
        assert_eq!(store.find_ref("seed-the-store").map(|row| row.id), Some(1));
        assert_eq!(Store::default().next_id(), 1);
    }

    #[test]
    fn a_milestones_audit_rows_hold_no_tasks_ref() {
        let change = |r#ref: &str| Change {
            at: None,
            r#ref: r#ref.to_string(),
            field: "*".to_string(),
            old: String::new(),
            new: String::new(),
            actor: String::new(),
            reason: String::new(),
        };
        let store = Store {
            changes: vec![change("milestone:A"), change("gone")],
            ..Store::default()
        };
        assert_eq!(store.trail_refs().collect::<Vec<_>>(), ["gone"]);
    }
}
