//! The field catalogue of the v2 review store: every field of its `runs`,
//! `cells`, `findings` and `verdicts` arrays with its type, the finding kinds
//! it applies to, when it is required, its family, enum values, length cap,
//! who may supply it and what each role sees of it. `review add` validates
//! through [`validate_row`], `review show --for <role>` projects through
//! [`project`], and `review add` computes a finding's verifier with [`route`],
//! so a field's contract lives only here.

use std::fmt;

use anyhow::Result;
use globset::{Glob, GlobSet, GlobSetBuilder};
use serde_json::{Map, Value as JsonValue};

use super::tier::Tier;
use crate::errors::{ErrorKind, tagged_err};

macro_rules! str_enum {
    ($(#[$meta:meta])* $name:ident { $($variant:ident => $text:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub(crate) enum $name {
            $($variant),+
        }

        #[allow(
            dead_code,
            reason = "the macro gives every word enum the same accessors; not every enum calls each one"
        )]
        impl $name {
            /// Every variant, in declaration order — the order `Ord` sorts by.
            pub(crate) const ALL: &'static [Self] = &[$(Self::$variant),+];

            pub(crate) const VALUES: &'static [&'static str] = &[$($text),+];

            pub(crate) fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $text),+
                }
            }

            pub(crate) fn parse(text: &str) -> Option<Self> {
                match text {
                    $($text => Some(Self::$variant),)+
                    _ => None,
                }
            }
        }
    };
}
pub(crate) use str_enum;

str_enum!(Kind {
    Defect => "defect",
    Cleanup => "cleanup",
    Advisory => "advisory",
});

str_enum!(Family {
    Code => "code",
    Perf => "perf",
    Plan => "plan",
});

str_enum!(Direction {
    Add => "add",
    Remove => "remove",
    Change => "change",
});

str_enum!(Severity {
    Critical => "critical",
    Major => "major",
    Minor => "minor",
});

str_enum!(Depth {
    Quick => "quick",
    Standard => "standard",
    Deep => "deep",
});

str_enum!(
    /// Who verifies a finding; `Unrouted` is advisory, which nobody verifies.
    Route {
        Verifier => "verifier",
        Panel => "panel",
        Checker => "checker",
        Unrouted => "none",
    }
);

str_enum!(
    /// The readers a projection serves. The declaration order is the column
    /// order of every [`FieldSpec::vis`] array.
    Role {
        Finder => "finder",
        Verifier => "verifier",
        Checker => "checker",
        Implementer => "implementer",
        Orchestrator => "orchestrator",
        Audit => "audit",
        Backlog => "backlog",
        Report => "report",
    }
);

str_enum!(Record {
    Run => "run",
    Cell => "cell",
    Finding => "finding",
    Verdict => "verdict",
});

str_enum!(
    /// A finding's lifecycle status. Its subsets are the predicates below,
    /// and each status owns the companion fields [`Self::companions`] names.
    FindingStatus {
        Unverified => "unverified",
        Superseded => "superseded",
        Open => "open",
        Refuted => "refuted",
        Merged => "merged",
        Parked => "parked",
        Dismissed => "dismissed",
        Deferred => "deferred",
        Backlogged => "backlogged",
        Converted => "converted",
        Resolved => "resolved",
        Exported => "exported",
        Scheduled => "scheduled",
    }
);

impl FindingStatus {
    /// The fields only this status carries; a status change clears every
    /// other status's companions.
    pub(crate) const fn companions(self) -> &'static [&'static str] {
        match self {
            Self::Merged => &["merged_into"],
            Self::Parked => &["park_reason"],
            Self::Dismissed => &["dismiss_reason"],
            Self::Deferred => &["defer_reason", "defer_trigger"],
            Self::Backlogged => &["backlog_ref"],
            Self::Converted => &["converted_to"],
            Self::Resolved => &["resolution", "resolved"],
            Self::Open => &["reopen_reason"],
            Self::Unverified
            | Self::Superseded
            | Self::Refuted
            | Self::Exported
            | Self::Scheduled => &[],
        }
    }

    /// No disposition moves these: withdrawn, merged into another row,
    /// replaced by a conversion, owned by a v1 ledger since export, or held
    /// by a live task in an apply flow.
    pub(crate) fn is_settled(self) -> bool {
        matches!(
            self,
            Self::Superseded | Self::Merged | Self::Converted | Self::Exported | Self::Scheduled
        )
    }

    /// `triage --reopen` returns one of these to `open`.
    pub(crate) fn is_reopenable(self) -> bool {
        matches!(
            self,
            Self::Dismissed
                | Self::Deferred
                | Self::Parked
                | Self::Backlogged
                | Self::Refuted
                | Self::Resolved
        )
    }

    /// A target in one of these suppresses a row naming it in `same_as`.
    pub(crate) fn is_suppressing(self) -> bool {
        matches!(
            self,
            Self::Dismissed | Self::Deferred | Self::Parked | Self::Backlogged | Self::Converted
        )
    }

    /// A sync may overwrite these: the finding as exported (or still `open`
    /// when an export's v2 write never landed), or as a sync left it.
    pub(crate) fn is_syncable(self) -> bool {
        matches!(
            self,
            Self::Open | Self::Exported | Self::Resolved | Self::Dismissed | Self::Deferred
        )
    }

    /// A verdict decides these; every other status is a later disposition a
    /// verdict is recorded beside but never overrides.
    pub(crate) fn decided_by_verdict(self) -> bool {
        matches!(self, Self::Unverified | Self::Open | Self::Refuted)
    }
}

const fn companion_count() -> usize {
    let mut n = 0;
    let mut i = 0;
    while i < FindingStatus::ALL.len() {
        n += FindingStatus::ALL[i].companions().len();
        i += 1;
    }
    n
}

const ALL_COMPANION_ARRAY: [&str; companion_count()] = {
    let mut out = [""; companion_count()];
    let mut n = 0;
    let mut i = 0;
    while i < FindingStatus::ALL.len() {
        let owned = FindingStatus::ALL[i].companions();
        let mut j = 0;
        while j < owned.len() {
            out[n] = owned[j];
            n += 1;
            j += 1;
        }
        i += 1;
    }
    out
};

/// Every status's companions, in status order.
pub(crate) const ALL_COMPANIONS: &[&str] = &ALL_COMPANION_ARRAY;

/// A finding's link to the v1 row it was exported as. Not companions: a sync
/// keeps them when it moves an exported finding to resolved, dismissed or
/// deferred.
pub(crate) const LINK_FIELDS: &[&str] = &["exported_as", "export_ledger"];

pub(crate) const FINDING_STATUSES: &[&str] = FindingStatus::VALUES;

str_enum!(
    /// What a reproduce check found of a finding after its fix landed.
    ReproduceVerdict {
        Gone => "gone",
        Reproduces => "reproduces",
        Unsettled => "unsettled",
    }
);

str_enum!(ReproduceBy {
    Verifier => "verifier",
    Orchestrator => "orchestrator",
});
pub(crate) const OUTCOMES: &[&str] = &[
    "repeat",
    "contested",
    "reopened",
    "regression",
    "suppressed",
];
pub(crate) const VERDICTS: &[&str] = &["confirmed", "plausible", "refuted", "unsettled"];
pub(crate) const VERDICT_BY: &[&str] = &[
    "verifier",
    "panel",
    "checker",
    "audit",
    "orchestrator",
    "sync",
];
str_enum!(CellStatus {
    Assigned => "assigned",
    Covered => "covered",
    Thin => "thin",
    Uncovered => "uncovered",
});

pub(crate) const CELL_STATUSES: &[&str] = CellStatus::VALUES;
pub(super) const FINDER: &str = "review-finder";
/// The finder run without CLAUDE.md, for a correctness-only cell.
pub(super) const FINDER_CF: &str = "review-finder-cf";
pub(super) const SWEEPER: &str = "review-sweeper";
pub(super) const EXECUTOR: &str = "plan-executor";
pub(crate) const CELL_AGENTS: &[&str] = &[FINDER, FINDER_CF, SWEEPER, EXECUTOR];

str_enum!(
    /// What a defect or cleanup cannot be fixed without. A finding with none
    /// is fixable as written.
    BlockedOn {
        Decision => "decision",
        Evidence => "evidence",
        Measurement => "measurement",
        Alternatives => "alternatives",
        Design => "design",
    }
);

/// What `finding` is blocked on, or `None` when it is fixable. A stored row
/// with `fix_shape = "needs-plan"` and no `blocked_on` is blocked on design.
pub(crate) fn blocked_on(finding: &super::store::Finding) -> Option<BlockedOn> {
    let text = |key: &str| finding.rest.get(key).and_then(toml::Value::as_str);
    match text("blocked_on").map(str::trim) {
        Some(value) if !value.is_empty() => BlockedOn::parse(value),
        _ if text("fix_shape") == Some("needs-plan") => Some(BlockedOn::Design),
        _ => None,
    }
}

/// Who answered a blocked finding, recorded as its `unblocked_by`.
pub(crate) const UNBLOCKED_BY: &[&str] = &["verifier", "user", "settle"];

/// The tiers `review settle --accept` records as a finding's `settle_tier`.
pub(crate) const SETTLE_TIERS: &[&str] = &["mechanical", "cited", "user"];

/// Drops `finding`'s block: `blocked_on`, each `blocked_*` detail not in
/// `keep`, and a legacy `fix_shape = "needs-plan"`, which [`blocked_on`]
/// would still read as a design block.
pub(crate) fn clear_block(finding: &mut super::store::Finding, keep: &[&str]) {
    let detail = FIELDS
        .iter()
        .filter(|s| s.record == Record::Finding && matches!(s.need, Need::WhenBlockedOn(_)))
        .map(|s| s.name);
    for key in std::iter::once("blocked_on").chain(detail) {
        if !keep.contains(&key) {
            finding.rest.remove(key);
        }
    }
    if finding.rest.get("fix_shape").and_then(toml::Value::as_str) == Some("needs-plan") {
        finding.rest.remove("fix_shape");
    }
}

/// A field a row may no longer supply, with what replaced it.
const RETIRED: &[(&str, &str)] = &[(
    "fix_shape",
    "is retired; a fixable finding omits `blocked_on`, and one that cannot be fixed as written names what it lacks in `blocked_on`",
)];

const EFFORTS: &[&str] = &["trivial", "small", "medium", "large"];
const GRADES: &[&str] = &["high", "medium", "low"];
const ANGLES: &[&str] = &["risk", "challenge", "probe", "alternative", "direction"];
const COMMANDS: &[&str] = &["review-v2", "optimise-v2", "review-plan-v2"];
const SUBSTRATES: &[&str] = &["workflow", "agent-waves"];

/// A set of finding kinds, as a bitmask over [`Kind`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct KindSet(u8);

impl KindSet {
    pub(crate) const DEFECT: Self = Self(1);
    pub(crate) const CLEANUP: Self = Self(2);
    pub(crate) const ADVISORY: Self = Self(4);
    pub(crate) const ALL: Self = Self(7);

    const fn or(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub(crate) fn contains(self, kind: Kind) -> bool {
        self.0 & (1 << kind as u8) != 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Ty {
    Str,
    Int,
    Bool,
    Date,
    StrList,
    Table,
    TableList,
    /// Any JSON value; the store keeps it as JSON text.
    Any,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Need {
    Optional,
    /// Required for every kind the field applies to.
    Required,
    RequiredFor(KindSet),
    /// Cleanup `old`: required for `change` and `remove`, and for `add`
    /// unless `target_file` names a file that does not exist yet.
    OldText,
    /// Advisory `challenges`: required when `angle = challenge`.
    WhenChallenge,
    /// A `blocked_*` detail field: required when `blocked_on` is one of these.
    WhenBlockedOn(&'static [BlockedOn]),
    /// Verdict `fix_sketch`: required when `unblocks = true`.
    WhenUnblocks,
    /// Cell `question`: required when the cell's concern is a `probe:`.
    WhenProbe,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Origin {
    /// The caller supplies it in the row.
    Input,
    /// tomlctl derives or stamps it; a row supplying it is refused.
    Computed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PathForm {
    No,
    Path,
    /// A path checked only when the citation is a bare `path`, `path:line`
    /// or `path#id`; a citation holding whitespace is command output.
    Citation,
}

/// What one role sees of a field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Vis {
    No,
    Read,
    /// Aggregated by the digest, never shown per row.
    Count,
    /// The first `n` elements of a list.
    First(usize),
    /// Only on a contested finding.
    IfContested,
    /// Only on a finding routed to the verifier or a panel.
    IfRouted,
    /// Only while the finding is still `open`, i.e. untriaged.
    UntilTriaged,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct FieldSpec {
    pub(crate) name: &'static str,
    pub(crate) record: Record,
    pub(crate) ty: Ty,
    pub(crate) kinds: KindSet,
    pub(crate) need: Need,
    /// `Some` when the field exists only in one family.
    pub(crate) family: Option<Family>,
    pub(crate) values: &'static [&'static str],
    /// In characters, per string or per list element.
    pub(crate) cap: Option<usize>,
    pub(crate) max_items: Option<usize>,
    pub(crate) min_items: Option<usize>,
    pub(crate) range: Option<(i64, i64)>,
    pub(crate) path: PathForm,
    pub(crate) origin: Origin,
    pub(crate) vis: [Vis; 8],
}

const fn field(record: Record, name: &'static str, ty: Ty, vis: [Vis; 8]) -> FieldSpec {
    FieldSpec {
        name,
        record,
        ty,
        kinds: KindSet::ALL,
        need: Need::Optional,
        family: None,
        values: &[],
        cap: None,
        max_items: None,
        min_items: None,
        range: None,
        path: PathForm::No,
        origin: Origin::Input,
        vis,
    }
}

impl FieldSpec {
    const fn kinds(mut self, kinds: KindSet) -> Self {
        self.kinds = kinds;
        self
    }
    const fn need(mut self, need: Need) -> Self {
        self.need = need;
        self
    }
    const fn required(self) -> Self {
        self.need(Need::Required)
    }
    const fn values(mut self, values: &'static [&'static str]) -> Self {
        self.values = values;
        self
    }
    const fn cap(mut self, cap: usize) -> Self {
        self.cap = Some(cap);
        self
    }
    const fn items(mut self, max: usize) -> Self {
        self.max_items = Some(max);
        self
    }
    const fn min_items(mut self, min: usize) -> Self {
        self.min_items = Some(min);
        self
    }
    const fn range(mut self, lo: i64, hi: i64) -> Self {
        self.range = Some((lo, hi));
        self
    }
    const fn path(mut self, form: PathForm) -> Self {
        self.path = form;
        self
    }
    const fn computed(mut self) -> Self {
        self.origin = Origin::Computed;
        self
    }
    const fn plan_only(mut self) -> Self {
        self.family = Some(Family::Plan);
        self
    }
}

const N: Vis = Vis::No;
const R: Vis = Vis::Read;
const COUNT: Vis = Vis::Count;
const CONTESTED: Vis = Vis::IfContested;
const ROUTED: Vis = Vis::IfRouted;
const UNTRIAGED: Vis = Vis::UntilTriaged;

// Columns: finder, verifier, checker, implementer, orchestrator, audit, backlog, report.
const IDENT: [Vis; 8] = [R, R, R, R, R, R, R, R];
const STATUS: [Vis; 8] = [R, N, N, N, R, N, N, R];
// The checker reads `lens` to tell whether a row's lens is a mechanical one.
const LENS: [Vis; 8] = [N, R, R, N, R, R, R, R];
const CELL_OF: [Vis; 8] = [N, R, N, N, R, R, R, R];
const SEVERITY: [Vis; 8] = [N, R, N, R, R, R, N, R];
const FAILURE: [Vis; 8] = [N, R, N, R, CONTESTED, R, N, R];
const COUNTER: [Vis; 8] = [N, R, N, N, N, R, N, N];
const EVIDENCE: [Vis; 8] = [N, R, N, N, N, R, Vis::First(2), N];
const GRADE: [Vis; 8] = [N, N, N, N, R, N, N, N];
const FIX: [Vis; 8] = [N, N, N, R, N, N, N, R];
const BLOCKED: [Vis; 8] = [N, R, N, R, R, R, N, R];
const PLAN: [Vis; 8] = [N, R, N, R, N, R, N, R];
const EDIT: [Vis; 8] = [N, ROUTED, R, R, COUNT, R, N, R];
const EDIT_DETAIL: [Vis; 8] = [N, ROUTED, R, R, COUNT, R, N, N];
const ADVISORY: [Vis; 8] = [N, N, N, N, UNTRIAGED, N, R, R];
const SAME_AS: [Vis; 8] = [N, R, R, N, N, R, N, N];
const DEPENDS: [Vis; 8] = [N, N, N, R, R, N, N, R];
const INTERNAL: [Vis; 8] = [N, N, N, N, R, N, N, R];
const VERDICT_LATEST: [Vis; 8] = [N, N, N, N, R, R, N, R];
const CITATION: [Vis; 8] = [N, N, N, R, R, R, N, R];
const AUDIT_ONLY: [Vis; 8] = [N, N, N, N, N, R, N, N];
const CELL_ASSIGNMENT: [Vis; 8] = [R, N, N, N, R, N, N, R];
const RECORD_STATE: [Vis; 8] = [N, N, N, N, R, N, N, R];
const TARGET: [Vis; 8] = [R, R, N, R, R, R, N, R];
const TIER: [Vis; 8] = [N, N, N, R, R, R, N, R];

use Record::{Cell, Finding, Run, Verdict};

const DEFECT_OR_CLEANUP: KindSet = KindSet::DEFECT.or(KindSet::CLEANUP);

pub(crate) const FIELDS: &[FieldSpec] = &[
    // runs
    field(Run, "id", Ty::Str, RECORD_STATE).computed(),
    field(Run, "command", Ty::Str, RECORD_STATE)
        .values(COMMANDS)
        .required(),
    field(Run, "family", Ty::Str, RECORD_STATE)
        .values(Family::VALUES)
        .required(),
    field(Run, "depth", Ty::Str, RECORD_STATE)
        .values(Depth::VALUES)
        .required(),
    field(Run, "started", Ty::Str, RECORD_STATE).computed(),
    field(Run, "finished", Ty::Str, RECORD_STATE).computed(),
    field(Run, "base", Ty::Str, RECORD_STATE),
    field(Run, "head", Ty::Str, RECORD_STATE),
    field(Run, "scope", Ty::StrList, RECORD_STATE),
    field(Run, "changed_lines", Ty::Int, RECORD_STATE).computed(),
    field(Run, "substrate", Ty::Str, RECORD_STATE).values(SUBSTRATES),
    field(Run, "models", Ty::Table, RECORD_STATE),
    field(Run, "catalogue", Ty::Table, RECORD_STATE).computed(),
    field(Run, "signals", Ty::TableList, RECORD_STATE),
    field(Run, "waivers", Ty::TableList, RECORD_STATE),
    field(Run, "baselines", Ty::Table, RECORD_STATE),
    field(Run, "waves", Ty::Int, RECORD_STATE).computed(),
    field(Run, "agents", Ty::Int, RECORD_STATE),
    field(Run, "counts", Ty::Table, RECORD_STATE).computed(),
    field(Run, "audit", Ty::Table, RECORD_STATE),
    field(Run, "rechecks", Ty::Str, RECORD_STATE),
    field(Run, "incomplete", Ty::Str, RECORD_STATE).computed(),
    // cells
    field(Cell, "id", Ty::Str, CELL_ASSIGNMENT).computed(),
    field(Cell, "run", Ty::Str, RECORD_STATE).computed(),
    field(Cell, "wave", Ty::Int, RECORD_STATE).computed(),
    field(Cell, "pass", Ty::Int, CELL_ASSIGNMENT).range(1, 2),
    field(Cell, "slice", Ty::StrList, CELL_ASSIGNMENT).required(),
    field(Cell, "concerns", Ty::StrList, CELL_ASSIGNMENT)
        .items(3)
        .required(),
    field(Cell, "question", Ty::Str, CELL_ASSIGNMENT).need(Need::WhenProbe),
    field(Cell, "agent", Ty::Str, RECORD_STATE)
        .values(CELL_AGENTS)
        .required(),
    field(Cell, "reason", Ty::Str, RECORD_STATE).required(),
    field(Cell, "bound", Ty::TableList, CELL_ASSIGNMENT).computed(),
    field(Cell, "facets", Ty::StrList, CELL_ASSIGNMENT),
    field(Cell, "status", Ty::Str, RECORD_STATE)
        .values(CELL_STATUSES)
        .computed(),
    field(Cell, "sealed", Ty::Bool, RECORD_STATE).computed(),
    field(Cell, "note", Ty::Str, RECORD_STATE)
        .cap(600)
        .computed(),
    field(Cell, "ruled_out", Ty::TableList, RECORD_STATE).computed(),
    // findings: identity and bookkeeping
    field(Finding, "id", Ty::Str, IDENT).computed(),
    field(Finding, "run", Ty::Str, INTERNAL).computed(),
    field(Finding, "cell", Ty::Str, CELL_OF).computed(),
    field(Finding, "seen_in", Ty::StrList, INTERNAL).computed(),
    field(Finding, "family", Ty::Str, INTERNAL)
        .values(Family::VALUES)
        .computed(),
    field(Finding, "kind", Ty::Str, IDENT)
        .values(Kind::VALUES)
        .required(),
    field(Finding, "lens", Ty::Str, LENS).required(),
    field(Finding, "route", Ty::Str, INTERNAL)
        .values(Route::VALUES)
        .computed(),
    field(Finding, "file", Ty::Str, IDENT)
        .path(PathForm::Path)
        .required(),
    // `review add` requires `line` on a defect or cleanup outside a v2 flow,
    // whose findings name a record in `target` instead.
    field(Finding, "line", Ty::Int, IDENT).range(1, i64::MAX),
    field(Finding, "symbol", Ty::Str, IDENT),
    field(Finding, "summary", Ty::Str, IDENT)
        .cap(160)
        .required(),
    // findings: defect
    field(Finding, "severity", Ty::Str, SEVERITY)
        .kinds(KindSet::DEFECT)
        .values(Severity::VALUES)
        .required(),
    field(Finding, "failure_scenario", Ty::Str, FAILURE)
        .kinds(KindSet::DEFECT)
        .cap(600)
        .required(),
    field(Finding, "fix", Ty::Str, FIX)
        .kinds(KindSet::DEFECT)
        .cap(800)
        .required(),
    field(Finding, "effort", Ty::Str, FIX)
        .kinds(KindSet::DEFECT)
        .values(EFFORTS)
        .required(),
    field(Finding, "counter", Ty::Str, COUNTER)
        .kinds(DEFECT_OR_CLEANUP)
        .cap(300)
        .need(Need::RequiredFor(KindSet::DEFECT)),
    field(Finding, "grade", Ty::Str, GRADE)
        .values(GRADES)
        .need(Need::RequiredFor(KindSet::DEFECT)),
    field(Finding, "evidence", Ty::StrList, EVIDENCE)
        .items(5)
        .cap(300)
        .need(Need::RequiredFor(KindSet::DEFECT)),
    // findings: cleanup
    field(Finding, "edit", Ty::Str, EDIT)
        .kinds(KindSet::CLEANUP)
        .cap(800)
        .required(),
    field(Finding, "direction", Ty::Str, EDIT_DETAIL)
        .kinds(KindSet::CLEANUP)
        .values(Direction::VALUES)
        .required(),
    field(Finding, "old", Ty::Str, EDIT_DETAIL)
        .kinds(KindSet::CLEANUP)
        .cap(400)
        .need(Need::OldText),
    field(Finding, "target_file", Ty::Str, EDIT_DETAIL)
        .kinds(KindSet::CLEANUP)
        .path(PathForm::Path),
    field(Finding, "effort", Ty::Str, EDIT_DETAIL)
        .kinds(KindSet::CLEANUP)
        .values(EFFORTS)
        .required(),
    field(Finding, "instances", Ty::StrList, EDIT_DETAIL)
        .kinds(KindSet::CLEANUP)
        .items(50)
        .path(PathForm::Path),
    // findings: blocked defect or cleanup
    field(Finding, "blocked_on", Ty::Str, BLOCKED)
        .kinds(DEFECT_OR_CLEANUP)
        .values(BlockedOn::VALUES),
    field(Finding, "blocked_missing", Ty::Str, BLOCKED)
        .kinds(DEFECT_OR_CLEANUP)
        .cap(400)
        .need(Need::WhenBlockedOn(BlockedOn::ALL)),
    // A decision option names its consequence in the same string.
    field(Finding, "blocked_options", Ty::StrList, BLOCKED)
        .kinds(DEFECT_OR_CLEANUP)
        .min_items(2)
        .items(6)
        .cap(300)
        .need(Need::WhenBlockedOn(&[
            BlockedOn::Decision,
            BlockedOn::Alternatives,
        ])),
    field(Finding, "blocked_metric", Ty::Str, BLOCKED)
        .kinds(DEFECT_OR_CLEANUP)
        .cap(200)
        .need(Need::WhenBlockedOn(&[BlockedOn::Measurement])),
    field(Finding, "blocked_command", Ty::Str, BLOCKED)
        .kinds(DEFECT_OR_CLEANUP)
        .cap(300)
        .need(Need::WhenBlockedOn(&[BlockedOn::Measurement])),
    field(Finding, "blocked_contract", Ty::Str, BLOCKED)
        .kinds(DEFECT_OR_CLEANUP)
        .cap(600)
        .need(Need::WhenBlockedOn(&[BlockedOn::Design])),
    // Set when a block is answered, and kept through every later status.
    field(Finding, "fix_sketch", Ty::Str, FIX)
        .kinds(DEFECT_OR_CLEANUP)
        .cap(800)
        .computed(),
    field(Finding, "unblocked_by", Ty::Str, STATUS)
        .kinds(DEFECT_OR_CLEANUP)
        .values(UNBLOCKED_BY)
        .computed(),
    field(Finding, "decision_answer", Ty::Str, BLOCKED)
        .kinds(DEFECT_OR_CLEANUP)
        .cap(800)
        .computed(),
    field(Finding, "settle_tier", Ty::Str, TIER)
        .kinds(DEFECT_OR_CLEANUP)
        .values(SETTLE_TIERS)
        .computed(),
    // A deferred settle proposal: its note's path, and when it was deferred.
    // `proposal_at` is a UTC datetime; `Date` is the catalogue's TOML
    // datetime type, which reads a full offset datetime back as one.
    field(Finding, "proposal", Ty::Str, INTERNAL)
        .kinds(DEFECT_OR_CLEANUP)
        .computed(),
    field(Finding, "proposal_at", Ty::Date, INTERNAL)
        .kinds(DEFECT_OR_CLEANUP)
        .computed(),
    // findings: advisory
    field(Finding, "angle", Ty::Str, ADVISORY)
        .kinds(KindSet::ADVISORY)
        .values(ANGLES)
        .required(),
    field(Finding, "rationale", Ty::Str, ADVISORY)
        .kinds(KindSet::ADVISORY)
        .cap(800)
        .required(),
    field(Finding, "alternative", Ty::Str, ADVISORY).kinds(KindSet::ADVISORY),
    field(Finding, "adoption_cost", Ty::Str, ADVISORY)
        .kinds(KindSet::ADVISORY)
        .required(),
    field(Finding, "challenges", Ty::Str, ADVISORY)
        .kinds(KindSet::ADVISORY)
        .need(Need::WhenChallenge),
    // findings: plan family
    // Required by `review add` on a markdown plan's defect or cleanup, and
    // refused on a v2 flow.
    field(Finding, "plan_section", Ty::Str, PLAN)
        .plan_only()
        .cap(200),
    field(Finding, "anchor_old", Ty::Str, PLAN)
        .plan_only()
        .kinds(DEFECT_OR_CLEANUP)
        .cap(400),
    field(Finding, "anchor_new", Ty::Str, PLAN)
        .plan_only()
        .kinds(DEFECT_OR_CLEANUP)
        .cap(800),
    field(Finding, "target", Ty::Str, TARGET)
        .plan_only()
        .cap(300),
    field(Finding, "expect", Ty::Any, PLAN)
        .plan_only()
        .kinds(DEFECT_OR_CLEANUP),
    field(Finding, "new", Ty::Any, PLAN)
        .plan_only()
        .kinds(DEFECT_OR_CLEANUP),
    field(Finding, "proof", Ty::Table, PLAN)
        .plan_only()
        .kinds(DEFECT_OR_CLEANUP),
    field(Finding, "tier", Ty::Str, TIER)
        .values(Tier::VALUES)
        .computed(),
    field(Finding, "applied", Ty::Table, TIER).computed(),
    // Set by `review apply` before a prose write and dropped by its stamp.
    field(Finding, "applying", Ty::Bool, TIER).computed(),
    // findings: identity links and dispositions
    field(Finding, "same_as", Ty::Str, SAME_AS),
    field(Finding, "depends_on", Ty::StrList, DEPENDS),
    field(Finding, "related", Ty::StrList, INTERNAL).computed(),
    field(Finding, "anchor", Ty::Str, INTERNAL).computed(),
    field(Finding, "outcome", Ty::Str, STATUS)
        .values(OUTCOMES)
        .computed(),
    field(Finding, "status", Ty::Str, STATUS)
        .values(FINDING_STATUSES)
        .computed(),
    field(Finding, "first_seen", Ty::Date, INTERNAL).computed(),
    field(Finding, "merged_into", Ty::Str, STATUS).computed(),
    field(Finding, "park_reason", Ty::Str, STATUS).computed(),
    field(Finding, "dismiss_reason", Ty::Str, STATUS).computed(),
    field(Finding, "defer_reason", Ty::Str, STATUS).computed(),
    field(Finding, "defer_trigger", Ty::Str, STATUS).computed(),
    field(Finding, "reopen_reason", Ty::Str, STATUS).computed(),
    field(Finding, "backlog_ref", Ty::Str, STATUS).computed(),
    field(Finding, "converted_to", Ty::Str, STATUS).computed(),
    field(Finding, "resolution", Ty::Str, STATUS).computed(),
    field(Finding, "resolved", Ty::Date, STATUS).computed(),
    field(Finding, "exported_as", Ty::Str, STATUS).computed(),
    field(Finding, "export_ledger", Ty::Str, STATUS).computed(),
    // The apply flow's slug and task ref of a scheduled finding. Not
    // companions: a move to resolved keeps them.
    field(Finding, "apply_flow", Ty::Str, STATUS).computed(),
    field(Finding, "apply_task", Ty::Str, STATUS).computed(),
    field(Finding, "retired_links", Ty::StrList, STATUS).computed(),
    // verdicts
    field(Verdict, "finding", Ty::Str, AUDIT_ONLY).required(),
    field(Verdict, "run", Ty::Str, AUDIT_ONLY).computed(),
    field(Verdict, "by", Ty::Str, VERDICT_LATEST)
        .values(VERDICT_BY)
        .required(),
    field(Verdict, "seat", Ty::Int, AUDIT_ONLY).range(1, 3),
    field(Verdict, "verdict", Ty::Str, VERDICT_LATEST)
        .values(VERDICTS)
        .required(),
    field(Verdict, "severity", Ty::Str, VERDICT_LATEST).values(Severity::VALUES),
    field(Verdict, "citation", Ty::Str, CITATION)
        .path(PathForm::Citation)
        .required(),
    field(Verdict, "rationale", Ty::Str, VERDICT_LATEST)
        .cap(600)
        .required(),
    // A verifier's answer to a blocked finding; `review verdict` clears the
    // block and copies the sketch onto the finding.
    field(Verdict, "unblocks", Ty::Bool, VERDICT_LATEST),
    field(Verdict, "fix_sketch", Ty::Str, VERDICT_LATEST)
        .cap(800)
        .need(Need::WhenUnblocks),
];

/// The catalogue entry for `name` on `record`; for a finding, the entry that
/// applies to `kind`, since `effort` differs between defect and cleanup.
pub(crate) fn spec(record: Record, name: &str, kind: Option<Kind>) -> Option<&'static FieldSpec> {
    FIELDS
        .iter()
        .find(|s| s.record == record && s.name == name && kind.is_none_or(|k| s.kinds.contains(k)))
}

/// The character cap the catalogue sets on `name` of `record`. Panics when
/// the catalogue gives that field no cap, which no caller's input can cause.
pub(crate) fn cap(record: Record, name: &str) -> usize {
    spec(record, name, None)
        .and_then(|s| s.cap)
        .unwrap_or_else(|| panic!("the field catalogue sets no cap on {record:?}.{name}"))
}

/// One reason a row was refused, naming the field it concerns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Refusal {
    pub(crate) field: String,
    pub(crate) reason: String,
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "`{}` {}", self.field, self.reason)
    }
}

/// What validation needs beyond the row: the run's family, and whether a
/// repo-relative path exists, which decides when a cleanup `add` needs `old`.
pub(crate) struct RowContext<'a> {
    pub(crate) family: Family,
    pub(crate) target_exists: &'a dyn Fn(&str) -> bool,
}

/// Checks a caller-supplied row of `record` against the catalogue: unknown,
/// computed, wrong-kind and wrong-family fields, types, enum values, caps,
/// list lengths, path form, and each field's required-ness. Returns every
/// refusal, not just the first, so one resubmission can fix them all.
pub(crate) fn validate_row(
    record: Record,
    row: &Map<String, JsonValue>,
    ctx: &RowContext<'_>,
) -> std::result::Result<(), Vec<Refusal>> {
    let mut refusals = Vec::new();
    let mut refuse = |field: &str, reason: String| {
        refusals.push(Refusal {
            field: field.to_string(),
            reason,
        })
    };

    let kind = if record == Record::Finding {
        match row.get("kind") {
            Some(JsonValue::String(text)) => match Kind::parse(text) {
                Some(kind) => Some(kind),
                None => {
                    refuse(
                        "kind",
                        format!("is {text:?}; expected one of {}", Kind::VALUES.join(", ")),
                    );
                    return Err(refusals);
                }
            },
            Some(_) => {
                refuse("kind", "must be a string".to_string());
                return Err(refusals);
            }
            None => {
                refuse("kind", "is required".to_string());
                return Err(refusals);
            }
        }
    } else {
        None
    };

    for (key, value) in row {
        let Some(spec) = spec(record, key, kind) else {
            let retired = RETIRED
                .iter()
                .find(|(name, _)| record == Record::Finding && name == key);
            let reason = match (kind, retired) {
                (_, Some((_, why))) => why.to_string(),
                (Some(kind), None) if spec(record, key, None).is_some() => {
                    format!("does not apply to a {} finding", kind.as_str())
                }
                _ => format!("is not a field of a {}", record.as_str()),
            };
            refuse(key, reason);
            continue;
        };
        if spec.origin == Origin::Computed {
            refuse(key, "is set by tomlctl and cannot be supplied".to_string());
            continue;
        }
        if let Some(family) = spec.family
            && family != ctx.family
        {
            refuse(
                key,
                format!("applies only to the {} family", family.as_str()),
            );
            continue;
        }
        if let Err(reason) = check_value(spec, value) {
            refuse(key, reason);
        }
    }

    let blocked_on = str_field(row, "blocked_on").and_then(BlockedOn::parse);
    let blocked_when = blocked_on
        .map(|b| format!(" when blocked_on is {}", b.as_str()))
        .unwrap_or_default();
    for spec in FIELDS.iter().filter(|s| {
        s.record == record
            && s.origin == Origin::Input
            && kind.is_none_or(|k| s.kinds.contains(k))
            && s.family.is_none_or(|f| f == ctx.family)
    }) {
        let (required, when) = match spec.need {
            Need::Optional => (false, ""),
            Need::Required => (true, ""),
            Need::RequiredFor(kinds) => (kind.is_some_and(|k| kinds.contains(k)), ""),
            Need::OldText => (
                old_required(row, ctx),
                " for direction change or remove, and for add unless target_file names a new file, except in the plan family",
            ),
            Need::WhenChallenge => (
                str_field(row, "angle") == Some("challenge"),
                " when angle is challenge",
            ),
            Need::WhenBlockedOn(blocks) => (
                blocked_on.is_some_and(|b| blocks.contains(&b)),
                blocked_when.as_str(),
            ),
            Need::WhenUnblocks => (unblocks(row), " when unblocks is true"),
            Need::WhenProbe => (
                row.get("concerns")
                    .and_then(JsonValue::as_array)
                    .is_some_and(|c| {
                        c.iter()
                            .any(|t| t.as_str().is_some_and(|t| t.starts_with("probe:")))
                    }),
                " for a probe cell",
            ),
        };
        if required && is_absent(row.get(spec.name)) {
            refuse(spec.name, format!("is required{when}"));
        }
    }

    if record == Record::Verdict
        && str_field(row, "verdict") == Some("unsettled")
        && str_field(row, "by") != Some("checker")
    {
        refuse(
            "verdict",
            "may be unsettled only from the checker".to_string(),
        );
    }
    if record == Record::Verdict && !is_absent(row.get("fix_sketch")) && !unblocks(row) {
        refuse(
            "fix_sketch",
            "is given only with unblocks = true".to_string(),
        );
    }

    if refusals.is_empty() {
        Ok(())
    } else {
        Err(refusals)
    }
}

fn old_required(row: &Map<String, JsonValue>, ctx: &RowContext<'_>) -> bool {
    if ctx.family == Family::Plan {
        return false;
    }
    match str_field(row, "direction").and_then(Direction::parse) {
        Some(Direction::Change | Direction::Remove) => true,
        Some(Direction::Add) => match str_field(row, "target_file") {
            Some(target) => (ctx.target_exists)(target),
            None => true,
        },
        None => false,
    }
}

/// Whether a verdict row answers its finding's block.
pub(crate) fn unblocks(row: &Map<String, JsonValue>) -> bool {
    row.get("unblocks").and_then(JsonValue::as_bool) == Some(true)
}

fn str_field<'a>(row: &'a Map<String, JsonValue>, name: &str) -> Option<&'a str> {
    row.get(name).and_then(JsonValue::as_str)
}

fn is_absent(value: Option<&JsonValue>) -> bool {
    match value {
        None | Some(JsonValue::Null) => true,
        Some(JsonValue::String(s)) => s.trim().is_empty(),
        Some(JsonValue::Array(a)) => a.is_empty(),
        Some(_) => false,
    }
}

fn check_value(spec: &FieldSpec, value: &JsonValue) -> std::result::Result<(), String> {
    match spec.ty {
        Ty::Str | Ty::Date => {
            let text = value.as_str().ok_or("must be a string")?;
            check_text(spec, text)
        }
        Ty::Int => {
            let n = value.as_i64().ok_or("must be an integer")?;
            match spec.range {
                Some((lo, hi)) if n < lo || n > hi => Err(if hi == i64::MAX {
                    format!("is {n}; it must be at least {lo}")
                } else {
                    format!("is {n}; it must be {lo} to {hi}")
                }),
                _ => Ok(()),
            }
        }
        Ty::Any => Ok(()),
        Ty::Bool => value
            .as_bool()
            .map(|_| ())
            .ok_or_else(|| "must be a boolean".to_string()),
        Ty::Table => value
            .as_object()
            .map(|_| ())
            .ok_or_else(|| "must be a table".to_string()),
        Ty::TableList => match value.as_array() {
            Some(rows) if rows.iter().all(JsonValue::is_object) => Ok(()),
            _ => Err("must be an array of tables".to_string()),
        },
        Ty::StrList => {
            let list = value.as_array().ok_or("must be an array of strings")?;
            if let Some(max) = spec.max_items
                && list.len() > max
            {
                return Err(format!("has {} entries; the cap is {max}", list.len()));
            }
            if let Some(min) = spec.min_items
                && list.len() < min
            {
                return Err(format!(
                    "has {} entries; at least {min} are required",
                    list.len()
                ));
            }
            for (i, element) in list.iter().enumerate() {
                let text = element.as_str().ok_or("must be an array of strings")?;
                check_text(spec, text).map_err(|reason| format!("entry {i} {reason}"))?;
            }
            Ok(())
        }
    }
}

fn check_text(spec: &FieldSpec, text: &str) -> std::result::Result<(), String> {
    if !spec.values.is_empty() && !spec.values.contains(&text) {
        return Err(format!(
            "is {text:?}; expected one of {}",
            spec.values.join(", ")
        ));
    }
    if let Some(cap) = spec.cap {
        let len = text.chars().count();
        if len > cap {
            return Err(format!("is {len} characters; the cap is {cap}"));
        }
    }
    let path = match spec.path {
        PathForm::No => return Ok(()),
        PathForm::Path => text,
        PathForm::Citation if text.chars().any(char::is_whitespace) => return Ok(()),
        PathForm::Citation => citation_path(text),
    };
    match path_problem(path, "paths") {
        Some(problem) => Err(format!("{text:?} {problem}")),
        None => Ok(()),
    }
}

/// The path part of a `path`, `path:line`, `path:a-b` or `path#id` citation.
fn citation_path(text: &str) -> &str {
    if let Some((path, suffix)) = text.rsplit_once(':')
        && !suffix.is_empty()
        && suffix.bytes().all(|b| b.is_ascii_digit() || b == b'-')
    {
        return path;
    }
    text.split_once('#').map_or(text, |(path, _)| path)
}

/// Why `path` is not repo-relative and `/`-separated, or `None` when it is;
/// `noun` names the paths in the message ("scope paths"). A `:` as the
/// second byte reads as a drive prefix whatever its first byte.
pub(super) fn path_problem(path: &str, noun: &str) -> Option<String> {
    if path.trim().is_empty() {
        Some("is empty".into())
    } else if path.contains('\\') {
        Some(format!("uses `\\`; {noun} are `/`-separated"))
    } else if path.starts_with('/') || path.as_bytes().get(1) == Some(&b':') {
        Some(format!("is absolute; {noun} are repo-relative"))
    } else if path.split('/').any(|segment| segment == "..") {
        Some("contains `..`".into())
    } else {
        None
    }
}

/// Keys a cell's `ruled_out` entry may carry.
const RULED_OUT_KEYS: &[&str] = &["file", "symbol", "line", "reason"];
/// The longest `reason` a `ruled_out` entry may carry, in characters.
const RULED_OUT_REASON_CAP: usize = 200;

/// Why `entry` cannot be stored as one of a cell's `ruled_out` sites; empty
/// when it can. `Ty::TableList` admits any table, so the entry's shape is
/// checked here.
pub(crate) fn ruled_out_refusals(entry: &Map<String, JsonValue>) -> Vec<String> {
    let mut reasons: Vec<String> = entry
        .keys()
        .filter(|key| !RULED_OUT_KEYS.contains(&key.as_str()))
        .map(|key| {
            format!(
                "`{key}` is not a rule-out key; expected {}",
                RULED_OUT_KEYS.join(", ")
            )
        })
        .collect();
    match entry.get("file") {
        Some(JsonValue::String(file)) => {
            if let Some(problem) = path_problem(file, "rule-out files") {
                reasons.push(format!("`file` {file:?} {problem}"));
            }
        }
        None | Some(JsonValue::Null) => reasons.push("`file` is required".to_string()),
        Some(_) => reasons.push("`file` must be a string".to_string()),
    }
    match entry.get("symbol") {
        None | Some(JsonValue::Null | JsonValue::String(_)) => {}
        Some(_) => reasons.push("`symbol` must be a string".to_string()),
    }
    match entry.get("line") {
        None | Some(JsonValue::Null) => {}
        Some(value)
            if value
                .as_i64()
                .is_some_and(|n| n >= 1 && n <= i64::from(u32::MAX)) => {}
        Some(_) => reasons.push("`line` must be a positive integer".to_string()),
    }
    let symbol = matches!(entry.get("symbol"), Some(JsonValue::String(s)) if !s.is_empty());
    if !symbol && entry.get("line").is_none_or(JsonValue::is_null) {
        reasons.push(
            "`symbol` or `line` is required; a rule-out with neither matches no finding"
                .to_string(),
        );
    }
    match entry.get("reason") {
        Some(JsonValue::String(reason)) if !reason.trim().is_empty() => {
            let len = reason.chars().count();
            if len > RULED_OUT_REASON_CAP {
                reasons.push(format!(
                    "`reason` is {len} characters; the cap is {RULED_OUT_REASON_CAP}"
                ));
            }
        }
        Some(JsonValue::String(_)) | None | Some(JsonValue::Null) => {
            reasons.push("`reason` is required".to_string());
        }
        Some(_) => reasons.push("`reason` must be a string".to_string()),
    }
    reasons
}

/// What [`project`] needs beyond the row: the finding's verdicts, oldest
/// first, and the finding its `same_as` names.
#[derive(Default)]
pub(crate) struct ProjectCtx<'a> {
    pub(crate) verdicts: &'a [Map<String, JsonValue>],
    pub(crate) same_as_target: Option<&'a Map<String, JsonValue>>,
}

/// The fields of `row` that `role` sees, in row order. A field the catalogue
/// does not know is withheld. A finding also carries, per role, its
/// `same_as` target's id, summary and status (`same_as_target`), its latest
/// verdict (`latest_verdict`), or for the audit every verdict (`verdicts`).
pub(crate) fn project(
    role: Role,
    record: Record,
    row: &Map<String, JsonValue>,
    ctx: &ProjectCtx<'_>,
) -> Map<String, JsonValue> {
    let kind = if record == Record::Finding {
        str_field(row, "kind").and_then(Kind::parse)
    } else {
        None
    };
    let contested =
        str_field(row, "outcome") == Some("contested") || verdicts_disagree(ctx.verdicts);
    let routed = matches!(str_field(row, "route"), Some("verifier" | "panel"));
    let untriaged = str_field(row, "status") == Some("open");

    let mut out = Map::new();
    for (key, value) in row {
        let Some(spec) = spec(record, key, kind) else {
            continue;
        };
        let shown = match spec.vis[role as usize] {
            Vis::No | Vis::Count => false,
            Vis::Read => true,
            Vis::IfContested => contested,
            Vis::IfRouted => routed,
            Vis::UntilTriaged => untriaged,
            Vis::First(n) => {
                let head = match value.as_array() {
                    Some(list) => JsonValue::Array(list.iter().take(n).cloned().collect()),
                    None => value.clone(),
                };
                out.insert(key.clone(), head);
                continue;
            }
        };
        if shown {
            out.insert(key.clone(), value.clone());
        }
    }

    if record != Record::Finding {
        return out;
    }
    if out
        .get("same_as")
        .and_then(JsonValue::as_str)
        .is_some_and(|s| !s.is_empty())
        && let Some(target) = ctx.same_as_target
    {
        let mut view = Map::new();
        for name in ["id", "summary", "status"] {
            if let Some(value) = target.get(name) {
                view.insert(name.to_string(), value.clone());
            }
        }
        out.insert("same_as_target".to_string(), JsonValue::Object(view));
    }
    let none = ProjectCtx::default();
    if role == Role::Audit {
        let all = ctx
            .verdicts
            .iter()
            .map(|v| JsonValue::Object(project(role, Record::Verdict, v, &none)))
            .collect();
        out.insert("verdicts".to_string(), JsonValue::Array(all));
    } else if let Some(latest) = ctx.verdicts.last() {
        let view = project(role, Record::Verdict, latest, &none);
        if !view.is_empty() {
            out.insert("latest_verdict".to_string(), JsonValue::Object(view));
        }
    }
    out
}

fn verdicts_disagree(verdicts: &[Map<String, JsonValue>]) -> bool {
    let said = |wanted: &[&str]| {
        verdicts
            .iter()
            .any(|v| str_field(v, "verdict").is_some_and(|text| wanted.contains(&text)))
    };
    said(&["refuted"]) && said(&["confirmed", "plausible"])
}

/// The vocabulary's `[routing]` globs, compiled.
pub(crate) struct RoutingGlobs {
    executable: GlobSet,
    tests: GlobSet,
}

impl RoutingGlobs {
    pub(crate) fn new(executable: &[String], tests: &[String]) -> Result<Self> {
        Ok(Self {
            executable: compile("executable", executable)?,
            tests: compile("tests", tests)?,
        })
    }
}

/// Compile one `[routing]` glob list; `key` names the list in the error.
pub(super) fn compile(key: &str, patterns: &[String]) -> Result<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        let glob = Glob::new(pattern).map_err(|e| {
            tagged_err(
                ErrorKind::Validation,
                None,
                format!("invalid [routing] {key} glob {pattern:?}: {e}"),
            )
        })?;
        builder.add(glob);
    }
    builder.build().map_err(|e| {
        tagged_err(
            ErrorKind::Validation,
            None,
            format!("invalid [routing] {key} globs: {e}"),
        )
    })
}

/// The paths a cleanup edits: its edit target (`target_file`, else `file`;
/// an empty `target_file` counts as absent) and each of its `instances`.
pub(super) fn edited_paths<'a>(
    file: &'a str,
    target_file: Option<&'a str>,
    instances: impl IntoIterator<Item = &'a str>,
) -> impl Iterator<Item = &'a str> {
    std::iter::once(target_file.filter(|t| !t.is_empty()).unwrap_or(file)).chain(instances)
}

/// Who verifies a finding. A critical defect at `deep` goes to a panel and
/// any other defect to the verifier. A plan cleanup goes to the verifier,
/// since the checker's projection hides the record edit it carries. Any
/// other cleanup goes to the verifier when it edits executable non-test code
/// or removes a test, at its edit target (`target_file`, else `file`) or any
/// of its `instances`, and to the checker otherwise. A test path is never
/// treated as executable.
#[allow(clippy::too_many_arguments)]
pub(crate) fn route(
    family: Family,
    kind: Kind,
    direction: Option<Direction>,
    severity: Option<Severity>,
    depth: Depth,
    file: &str,
    target_file: Option<&str>,
    instances: &[String],
    globs: &RoutingGlobs,
) -> Route {
    match kind {
        Kind::Advisory => Route::Unrouted,
        Kind::Defect if severity == Some(Severity::Critical) && depth == Depth::Deep => {
            Route::Panel
        }
        Kind::Defect => Route::Verifier,
        Kind::Cleanup if family == Family::Plan => Route::Verifier,
        Kind::Cleanup => {
            let Some(direction) = direction else {
                return Route::Verifier;
            };
            for path in edited_paths(file, target_file, instances.iter().map(String::as_str)) {
                let needs_verifier = if globs.tests.is_match(path) {
                    direction == Direction::Remove
                } else {
                    globs.executable.is_match(path)
                };
                if needs_verifier {
                    return Route::Verifier;
                }
            }
            Route::Checker
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn row(value: JsonValue) -> Map<String, JsonValue> {
        value.as_object().expect("test rows are objects").clone()
    }

    fn ctx(family: Family) -> RowContext<'static> {
        RowContext {
            family,
            target_exists: &|_| true,
        }
    }

    fn defect() -> Map<String, JsonValue> {
        row(json!({
            "kind": "defect",
            "lens": "correctness",
            "file": "src/a.rs",
            "line": 12,
            "symbol": "a::run",
            "summary": "run drops the last row",
            "severity": "major",
            "failure_scenario": "a two-row input returns one row",
            "fix": "iterate to len, not len - 1",
            "effort": "small",
            "counter": "a caller that pads the input",
            "grade": "high",
            "evidence": ["src/a.rs:12 loops to len - 1"],
        }))
    }

    fn cleanup(direction: &str) -> Map<String, JsonValue> {
        row(json!({
            "kind": "cleanup",
            "lens": "doc-drift",
            "file": "docs/a.md",
            "line": 4,
            "summary": "the doc names a removed flag",
            "edit": "drop the --fast line",
            "direction": direction,
            "effort": "trivial",
        }))
    }

    fn refused_fields(result: std::result::Result<(), Vec<Refusal>>) -> Vec<String> {
        result
            .err()
            .unwrap_or_default()
            .into_iter()
            .map(|r| r.field)
            .collect()
    }

    #[test]
    fn defect_requires_failure_scenario() {
        assert_eq!(
            validate_row(Record::Finding, &defect(), &ctx(Family::Code)),
            Ok(())
        );
        let mut bare = defect();
        bare.remove("failure_scenario");
        let refused = refused_fields(validate_row(Record::Finding, &bare, &ctx(Family::Code)));
        assert_eq!(refused, ["failure_scenario"]);
    }

    #[test]
    fn each_block_requires_its_detail() {
        let cases: [(&str, &[&str]); 5] = [
            ("decision", &["blocked_missing", "blocked_options"]),
            ("evidence", &["blocked_missing"]),
            (
                "measurement",
                &["blocked_missing", "blocked_metric", "blocked_command"],
            ),
            ("alternatives", &["blocked_missing", "blocked_options"]),
            ("design", &["blocked_missing", "blocked_contract"]),
        ];
        for (block, needs) in cases {
            for (base, family) in [(defect(), Family::Code), (cleanup("remove"), Family::Plan)] {
                let mut blocked = base;
                blocked.insert("blocked_on".into(), json!(block));
                let refused = refused_fields(validate_row(Record::Finding, &blocked, &ctx(family)));
                assert_eq!(refused, needs, "{block}");
                for need in needs {
                    let value = if *need == "blocked_options" {
                        json!([
                            "keep it, and callers stay as they are",
                            "drop it, and two callers change"
                        ])
                    } else {
                        json!("stated")
                    };
                    blocked.insert((*need).into(), value);
                }
                assert_eq!(
                    validate_row(Record::Finding, &blocked, &ctx(family)),
                    Ok(()),
                    "{block}"
                );
            }
        }
    }

    #[test]
    fn a_single_option_and_a_fix_shape_are_refused() {
        let mut decision = defect();
        decision.insert("blocked_on".into(), json!("decision"));
        decision.insert("blocked_missing".into(), json!("which owner"));
        decision.insert("blocked_options".into(), json!(["the parser owns it"]));
        let refusals = validate_row(Record::Finding, &decision, &ctx(Family::Code)).unwrap_err();
        assert_eq!(
            refusals.iter().map(ToString::to_string).collect::<Vec<_>>(),
            ["`blocked_options` has 1 entries; at least 2 are required"]
        );

        let mut shaped = defect();
        shaped.insert("fix_shape".into(), json!("edits"));
        let refusals = validate_row(Record::Finding, &shaped, &ctx(Family::Code)).unwrap_err();
        assert_eq!(refusals.len(), 1, "{refusals:?}");
        assert_eq!(refusals[0].field, "fix_shape");
        assert!(refusals[0].reason.contains("blocked_on"), "{refusals:?}");
    }

    #[test]
    fn a_legacy_needs_plan_row_reads_as_blocked_on_design() {
        use super::super::store::from_toml;
        let store = |extra: &str| {
            let text = format!(
                "schema_version = 1\n\n[[findings]]\nid = \"F1\"\nrun = \"U1\"\ncell = \"C1\"\nfamily = \"code\"\nkind = \"defect\"\nlens = \"correctness\"\nfile = \"src/a.rs\"\nsummary = \"s\"\nstatus = \"open\"\n{extra}"
            );
            let doc: toml::Value = toml::from_str(&text).expect("the fixture parses");
            from_toml(&doc).expect("the fixture reads")
        };
        for (extra, expected) in [
            ("", None),
            ("fix_shape = \"edits\"\n", None),
            ("fix_shape = \"needs-plan\"\n", Some(BlockedOn::Design)),
            ("blocked_on = \"evidence\"\n", Some(BlockedOn::Evidence)),
            (
                "fix_shape = \"needs-plan\"\nblocked_on = \"decision\"\n",
                Some(BlockedOn::Decision),
            ),
        ] {
            let store = store(extra);
            assert_eq!(blocked_on(&store.findings[0]), expected, "{extra:?}");
        }
    }

    #[test]
    fn cleanup_add_with_new_target_file_needs_no_old() {
        let mut add = cleanup("add");
        add.insert("target_file".into(), json!("docs/new.md"));
        let absent = RowContext {
            family: Family::Code,
            target_exists: &|_| false,
        };
        assert_eq!(validate_row(Record::Finding, &add, &absent), Ok(()));
        let refused = refused_fields(validate_row(Record::Finding, &add, &ctx(Family::Code)));
        assert_eq!(refused, ["old"]);
        add.remove("target_file");
        let refused = refused_fields(validate_row(Record::Finding, &add, &absent));
        assert_eq!(refused, ["old"]);
    }

    #[test]
    fn cleanup_change_requires_old() {
        for direction in ["change", "remove"] {
            let mut change = cleanup(direction);
            let refused =
                refused_fields(validate_row(Record::Finding, &change, &ctx(Family::Code)));
            assert_eq!(refused, ["old"], "{direction}");
            change.insert("old".into(), json!("--fast  skip checks"));
            assert_eq!(
                validate_row(Record::Finding, &change, &ctx(Family::Code)),
                Ok(())
            );
        }
    }

    #[test]
    fn advisory_challenge_requires_challenges() {
        let mut advisory = row(json!({
            "kind": "advisory",
            "lens": "contrarian",
            "file": "src/a.rs",
            "summary": "the lock could be per-cell",
            "angle": "challenge",
            "rationale": "contention grows with agents",
            "adoption_cost": "a store format change",
        }));
        let refused = refused_fields(validate_row(Record::Finding, &advisory, &ctx(Family::Code)));
        assert_eq!(refused, ["challenges"]);
        advisory.insert("challenges".into(), json!("ADR-0002"));
        assert_eq!(
            validate_row(Record::Finding, &advisory, &ctx(Family::Code)),
            Ok(())
        );
        advisory.remove("challenges");
        advisory.insert("angle".into(), json!("risk"));
        assert_eq!(
            validate_row(Record::Finding, &advisory, &ctx(Family::Code)),
            Ok(())
        );
    }

    #[test]
    fn over_cap_field_is_refused_with_its_cap() {
        let mut long = defect();
        long.insert("summary".into(), json!("x".repeat(160)));
        assert_eq!(
            validate_row(Record::Finding, &long, &ctx(Family::Code)),
            Ok(())
        );
        long.insert("summary".into(), json!("x".repeat(161)));
        long.insert("evidence".into(), json!(vec!["e"; 6]));
        let refusals = validate_row(Record::Finding, &long, &ctx(Family::Code)).unwrap_err();
        let messages: Vec<String> = refusals.iter().map(ToString::to_string).collect();
        assert_eq!(
            messages,
            [
                "`summary` is 161 characters; the cap is 160",
                "`evidence` has 6 entries; the cap is 5",
            ]
        );
    }

    #[test]
    fn verifier_projection_withholds_grade_and_fix() {
        let finding = defect();
        let none = ProjectCtx::default();
        let verifier = project(Role::Verifier, Record::Finding, &finding, &none);
        for withheld in ["grade", "fix", "effort"] {
            assert!(!verifier.contains_key(withheld), "verifier sees {withheld}");
        }
        let mut finding = finding;
        finding.insert("blocked_on".into(), json!("design"));
        finding.insert("blocked_missing".into(), json!("who owns the cache"));
        finding.insert("blocked_contract".into(), json!("the cache API"));
        let verifier = project(Role::Verifier, Record::Finding, &finding, &none);
        for shown in [
            "failure_scenario",
            "counter",
            "evidence",
            "severity",
            "blocked_on",
            "blocked_missing",
            "blocked_contract",
        ] {
            assert!(verifier.contains_key(shown), "verifier misses {shown}");
        }
        let implementer = project(Role::Implementer, Record::Finding, &finding, &none);
        assert!(implementer.contains_key("fix") && !implementer.contains_key("grade"));
    }

    /// The plan's field-readership matrix, extended with the bookkeeping
    /// fields it leaves out. `R` reads, `-` withheld, `C` count only, `E2`
    /// the first two entries, `ct` contested only, `rt` when routed to the
    /// verifier, `ut` until triaged.
    const MATRIX: &[(Record, &str, Option<Kind>, &str)] = &[
        (Finding, "id", None, "R R R R R R R R"),
        (Finding, "kind", None, "R R R R R R R R"),
        (Finding, "file", None, "R R R R R R R R"),
        (Finding, "line", None, "R R R R R R R R"),
        (Finding, "symbol", None, "R R R R R R R R"),
        (Finding, "summary", None, "R R R R R R R R"),
        (Finding, "status", None, "R - - - R - - R"),
        (Finding, "outcome", None, "R - - - R - - R"),
        (Finding, "merged_into", None, "R - - - R - - R"),
        (Finding, "park_reason", None, "R - - - R - - R"),
        (Finding, "dismiss_reason", None, "R - - - R - - R"),
        (Finding, "defer_reason", None, "R - - - R - - R"),
        (Finding, "defer_trigger", None, "R - - - R - - R"),
        (Finding, "reopen_reason", None, "R - - - R - - R"),
        (Finding, "backlog_ref", None, "R - - - R - - R"),
        (Finding, "converted_to", None, "R - - - R - - R"),
        (Finding, "resolution", None, "R - - - R - - R"),
        (Finding, "resolved", None, "R - - - R - - R"),
        (Finding, "exported_as", None, "R - - - R - - R"),
        (Finding, "export_ledger", None, "R - - - R - - R"),
        (Finding, "apply_flow", None, "R - - - R - - R"),
        (Finding, "apply_task", None, "R - - - R - - R"),
        (Finding, "retired_links", None, "R - - - R - - R"),
        (Finding, "lens", None, "- R R - R R R R"),
        (Finding, "cell", None, "- R - - R R R R"),
        (Finding, "severity", None, "- R - R R R - R"),
        (Finding, "failure_scenario", None, "- R - R ct R - R"),
        (Finding, "counter", None, "- R - - - R - -"),
        (Finding, "evidence", None, "- R - - - R E2 -"),
        (Finding, "grade", None, "- - - - R - - -"),
        (Finding, "fix", None, "- - - R - - - R"),
        (Finding, "blocked_on", None, "- R - R R R - R"),
        (Finding, "blocked_missing", None, "- R - R R R - R"),
        (Finding, "blocked_options", None, "- R - R R R - R"),
        (Finding, "blocked_metric", None, "- R - R R R - R"),
        (Finding, "blocked_contract", None, "- R - R R R - R"),
        (Finding, "blocked_command", None, "- R - R R R - R"),
        (Finding, "fix_sketch", None, "- - - R - - - R"),
        (Finding, "unblocked_by", None, "R - - - R - - R"),
        (Finding, "decision_answer", None, "- R - R R R - R"),
        (Finding, "settle_tier", None, "- - - R R R - R"),
        (Finding, "proposal", None, "- - - - R - - R"),
        (Finding, "proposal_at", None, "- - - - R - - R"),
        (Finding, "effort", Some(Kind::Defect), "- - - R - - - R"),
        (Finding, "plan_section", None, "- R - R - R - R"),
        (Finding, "anchor_old", None, "- R - R - R - R"),
        (Finding, "anchor_new", None, "- R - R - R - R"),
        (Finding, "target", None, "R R - R R R - R"),
        (Finding, "expect", None, "- R - R - R - R"),
        (Finding, "new", None, "- R - R - R - R"),
        (Finding, "proof", None, "- R - R - R - R"),
        (Finding, "tier", None, "- - - R R R - R"),
        (Finding, "applied", None, "- - - R R R - R"),
        (Finding, "applying", None, "- - - R R R - R"),
        (Run, "rechecks", None, "- - - - R - - R"),
        (Finding, "edit", None, "- rt R R C R - R"),
        (Finding, "direction", None, "- rt R R C R - -"),
        (Finding, "old", None, "- rt R R C R - -"),
        (Finding, "target_file", None, "- rt R R C R - -"),
        (Finding, "effort", Some(Kind::Cleanup), "- rt R R C R - -"),
        (Finding, "instances", None, "- rt R R C R - -"),
        (Finding, "angle", None, "- - - - ut - R R"),
        (Finding, "rationale", None, "- - - - ut - R R"),
        (Finding, "alternative", None, "- - - - ut - R R"),
        (Finding, "adoption_cost", None, "- - - - ut - R R"),
        (Finding, "challenges", None, "- - - - ut - R R"),
        (Finding, "same_as", None, "- R R - - R - -"),
        (Finding, "depends_on", None, "- - - R R - - R"),
        (Finding, "run", None, "- - - - R - - R"),
        (Finding, "seen_in", None, "- - - - R - - R"),
        (Finding, "family", None, "- - - - R - - R"),
        (Finding, "route", None, "- - - - R - - R"),
        (Finding, "related", None, "- - - - R - - R"),
        (Finding, "anchor", None, "- - - - R - - R"),
        (Finding, "first_seen", None, "- - - - R - - R"),
        (Verdict, "finding", None, "- - - - - R - -"),
        (Verdict, "run", None, "- - - - - R - -"),
        (Verdict, "seat", None, "- - - - - R - -"),
        (Verdict, "by", None, "- - - - R R - R"),
        (Verdict, "verdict", None, "- - - - R R - R"),
        (Verdict, "severity", None, "- - - - R R - R"),
        (Verdict, "rationale", None, "- - - - R R - R"),
        (Verdict, "citation", None, "- - - R R R - R"),
        (Verdict, "unblocks", None, "- - - - R R - R"),
        (Verdict, "fix_sketch", None, "- - - - R R - R"),
    ];

    fn parse_vis(token: &str) -> Vis {
        match token {
            "R" => Vis::Read,
            "-" => Vis::No,
            "C" => Vis::Count,
            "E2" => Vis::First(2),
            "ct" => Vis::IfContested,
            "rt" => Vis::IfRouted,
            "ut" => Vis::UntilTriaged,
            other => panic!("unknown matrix token {other:?}"),
        }
    }

    #[test]
    fn projection_table_matches_matrix() {
        for &(record, name, kind, cells) in MATRIX {
            let spec =
                spec(record, name, kind).unwrap_or_else(|| panic!("{name} is not catalogued"));
            let expected: Vec<Vis> = cells.split(' ').map(parse_vis).collect();
            assert_eq!(
                spec.vis.as_slice(),
                expected.as_slice(),
                "{} {name} {kind:?}",
                record.as_str()
            );
        }
        let listed = |s: &FieldSpec| {
            MATRIX.iter().any(|&(record, name, kind, _)| {
                record == s.record && name == s.name && kind.is_none_or(|k| s.kinds.contains(k))
            })
        };
        for s in FIELDS
            .iter()
            .filter(|s| matches!(s.record, Finding | Verdict))
        {
            assert!(
                listed(s),
                "{} {} is missing from the matrix",
                s.record.as_str(),
                s.name
            );
        }
    }

    #[test]
    fn finding_status_predicates_match_the_old_constants() {
        let set = |pick: fn(FindingStatus) -> bool| -> Vec<&'static str> {
            let mut out: Vec<&str> = FindingStatus::ALL
                .iter()
                .copied()
                .filter(|s| pick(*s))
                .map(FindingStatus::as_str)
                .collect();
            out.sort_unstable();
            out
        };
        let sorted = |mut list: Vec<&'static str>| {
            list.sort_unstable();
            list
        };
        assert_eq!(
            set(FindingStatus::is_settled),
            sorted(vec![
                "superseded",
                "merged",
                "converted",
                "exported",
                "scheduled"
            ])
        );
        assert_eq!(
            set(FindingStatus::is_reopenable),
            sorted(vec![
                "dismissed",
                "deferred",
                "parked",
                "backlogged",
                "refuted",
                "resolved"
            ])
        );
        assert_eq!(
            set(FindingStatus::is_suppressing),
            sorted(vec![
                "dismissed",
                "deferred",
                "parked",
                "backlogged",
                "converted"
            ])
        );
        assert_eq!(
            set(FindingStatus::is_syncable),
            sorted(vec![
                "open",
                "exported",
                "resolved",
                "dismissed",
                "deferred"
            ])
        );
        assert_eq!(
            set(FindingStatus::decided_by_verdict),
            sorted(vec!["unverified", "open", "refuted"])
        );
        assert_eq!(
            FINDING_STATUSES,
            [
                "unverified",
                "superseded",
                "open",
                "refuted",
                "merged",
                "parked",
                "dismissed",
                "deferred",
                "backlogged",
                "converted",
                "resolved",
                "exported",
                "scheduled",
            ]
        );
        assert_eq!(
            sorted(ALL_COMPANIONS.to_vec()),
            sorted(vec![
                "merged_into",
                "park_reason",
                "dismiss_reason",
                "defer_reason",
                "defer_trigger",
                "backlog_ref",
                "converted_to",
                "resolution",
                "resolved",
                "reopen_reason",
            ])
        );
        assert!(ALL_COMPANIONS.iter().all(|c| !LINK_FIELDS.contains(c)));
        assert!(
            ALL_COMPANIONS
                .iter()
                .all(|c| !["apply_flow", "apply_task"].contains(c))
        );
    }

    fn globs() -> RoutingGlobs {
        RoutingGlobs::new(
            &["**/*.rs".to_string(), "**/*.ts".to_string()],
            &["tests/**".to_string(), "**/*.test.ts".to_string()],
        )
        .expect("test globs compile")
    }

    fn route_cleanup(
        direction: Direction,
        file: &str,
        target: Option<&str>,
        instances: &[&str],
    ) -> Route {
        let instances: Vec<String> = instances.iter().map(|s| s.to_string()).collect();
        route(
            Family::Code,
            Kind::Cleanup,
            Some(direction),
            None,
            Depth::Standard,
            file,
            target,
            &instances,
            &globs(),
        )
    }

    #[test]
    fn route_sends_executable_edits_and_test_removals_to_verifier() {
        assert_eq!(
            route_cleanup(Direction::Change, "src/a.rs", None, &[]),
            Route::Verifier
        );
        assert_eq!(
            route_cleanup(Direction::Remove, "tests/a.rs", None, &[]),
            Route::Verifier
        );
        assert_eq!(
            route_cleanup(Direction::Add, "docs/a.md", Some("src/b.rs"), &[]),
            Route::Verifier
        );
        assert_eq!(
            route_cleanup(Direction::Change, "docs/a.md", None, &["web/c.ts"]),
            Route::Verifier
        );
        assert_eq!(
            route_cleanup(Direction::Add, "tests/a.rs", None, &[]),
            Route::Checker
        );
        assert_eq!(
            route_cleanup(Direction::Change, "docs/a.md", None, &["web/c.test.ts"]),
            Route::Checker
        );
        assert_eq!(
            route_cleanup(Direction::Remove, "docs/a.md", None, &[]),
            Route::Checker
        );
        let advisory = route(
            Family::Code,
            Kind::Advisory,
            None,
            None,
            Depth::Deep,
            "src/a.rs",
            None,
            &[],
            &globs(),
        );
        assert_eq!(advisory, Route::Unrouted);
    }

    #[test]
    fn route_sends_every_plan_cleanup_to_verifier() {
        for direction in [Direction::Add, Direction::Change, Direction::Remove] {
            let plan = route(
                Family::Plan,
                Kind::Cleanup,
                Some(direction),
                None,
                Depth::Standard,
                "docs/plans/a.md",
                Some(".claude/flows/s/tasks.toml"),
                &[],
                &globs(),
            );
            assert_eq!(plan, Route::Verifier, "{direction:?}");
        }
    }

    #[test]
    fn route_sends_critical_at_deep_to_panel() {
        let defect = |severity, depth| {
            route(
                Family::Code,
                Kind::Defect,
                None,
                Some(severity),
                depth,
                "src/a.rs",
                None,
                &[],
                &globs(),
            )
        };
        assert_eq!(defect(Severity::Critical, Depth::Deep), Route::Panel);
        assert_eq!(defect(Severity::Critical, Depth::Standard), Route::Verifier);
        assert_eq!(defect(Severity::Major, Depth::Deep), Route::Verifier);
        assert_eq!(defect(Severity::Minor, Depth::Quick), Route::Verifier);
    }

    #[test]
    fn absolute_or_backslash_path_is_refused() {
        for bad in [
            "/abs/a.rs",
            "C:/a.rs",
            "src\\a.rs",
            "../a.rs",
            "src/../a.rs",
        ] {
            let mut finding = defect();
            finding.insert("file".into(), json!(bad));
            let refused =
                refused_fields(validate_row(Record::Finding, &finding, &ctx(Family::Code)));
            assert_eq!(refused, ["file"], "{bad}");
        }
        let mut change = cleanup("change");
        change.insert("old".into(), json!("--fast"));
        change.insert("instances".into(), json!(["docs/b.md", "docs\\c.md"]));
        let refused = refused_fields(validate_row(Record::Finding, &change, &ctx(Family::Code)));
        assert_eq!(refused, ["instances"]);

        let verdict = |citation: &str| {
            row(json!({
                "finding": "F1",
                "by": "verifier",
                "verdict": "confirmed",
                "citation": citation,
                "rationale": "reproduced",
            }))
        };
        for good in [
            "src/a.rs:88",
            "src/a.rs:80-90",
            ".claude/reviews/x.toml#R3",
            "cargo test a → 1 failed",
        ] {
            assert_eq!(
                validate_row(Record::Verdict, &verdict(good), &ctx(Family::Code)),
                Ok(()),
                "{good}"
            );
        }
        for bad in ["C:\\src\\a.rs:88", "/src/a.rs:88", "../a.rs#R1"] {
            let refused = refused_fields(validate_row(
                Record::Verdict,
                &verdict(bad),
                &ctx(Family::Code),
            ));
            assert_eq!(refused, ["citation"], "{bad}");
        }
    }

    #[test]
    fn run_head_round_trips() {
        use super::super::store::{from_toml, to_toml};
        let store_text = |head: &str, ruled_out: &str| {
            format!(
                "schema_version = 1\n\n[[runs]]\nid = \"U1\"\nfamily = \"code\"\ndepth = \"standard\"\nbase = \"e23b918\"\n{head}scope = [\"src/a.rs\"]\n\n[[cells]]\nid = \"C1\"\nrun = \"U1\"\nslice = [\"src/a.rs\"]\nstatus = \"covered\"\nnote = \"swept\"\n{ruled_out}"
            )
        };
        let with_both = store_text(
            "head = \"4f0c2aa\"\n",
            "ruled_out = [{ file = \"src/a.rs\", symbol = \"a::run\", line = 12, reason = \"the pad is deliberate\" }]\n",
        );
        for (fixture, head) in [(with_both, Some("4f0c2aa")), (store_text("", ""), None)] {
            let doc: toml::Value = toml::from_str(&fixture).expect("the fixture parses");
            let store = from_toml(&doc).expect("the fixture reads");
            assert_eq!(store.runs[0].head.as_deref(), Some(head).flatten());
            assert!(
                store.runs[0].rest.get("head").is_none(),
                "`head` is typed, not carried in `rest`"
            );
            assert_eq!(store.cells[0].ruled_out.len(), usize::from(head.is_some()));
            assert_eq!(to_toml(&store), doc, "the store writes back as read");
            assert_eq!(
                toml::to_string(&to_toml(&store)).expect("serialises"),
                toml::to_string(&doc).expect("serialises"),
                "the written text matches the read text"
            );
        }
    }

    #[test]
    fn ruled_out_entry_refuses_unknown_key() {
        let entry = |value: JsonValue| row(value);
        assert_eq!(
            ruled_out_refusals(&entry(json!({
                "file": "src/a.rs",
                "symbol": "a::run",
                "line": 12,
                "reason": "the pad is deliberate",
            }))),
            Vec::<String>::new()
        );
        let refused = ruled_out_refusals(&entry(json!({
            "file": "src/a.rs",
            "line": 12,
            "reason": "the pad is deliberate",
            "lens": "correctness",
        })));
        assert_eq!(refused.len(), 1, "{refused:?}");
        assert!(
            refused[0].starts_with("`lens` is not a rule-out key"),
            "{refused:?}"
        );

        let refused = ruled_out_refusals(&entry(json!({
            "file": "C:\\src\\a.rs",
            "line": 0,
            "reason": "x".repeat(201),
        })));
        assert_eq!(refused.len(), 3, "{refused:?}");
        assert!(
            ruled_out_refusals(&entry(json!({"reason": "r"})))[0].contains("`file` is required")
        );
        assert!(
            ruled_out_refusals(&entry(json!({"file": "src/a.rs", "line": 1})))[0]
                .contains("`reason` is required")
        );
        assert!(
            ruled_out_refusals(&entry(
                json!({"file": "src/a.rs", "symbol": "a::run", "reason": "x".repeat(200)})
            ))
            .is_empty()
        );
        for neither in [
            json!({"file": "src/a.rs", "reason": "r"}),
            json!({"file": "src/a.rs", "symbol": "", "line": null, "reason": "r"}),
        ] {
            let refused = ruled_out_refusals(&entry(neither));
            assert_eq!(refused.len(), 1, "{refused:?}");
            assert!(refused[0].starts_with("`symbol` or `line` is required"));
        }
    }
}
