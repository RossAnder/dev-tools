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

use crate::errors::{ErrorKind, tagged_err};

macro_rules! str_enum {
    ($(#[$meta:meta])* $name:ident { $($variant:ident => $text:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub(crate) enum $name {
            $($variant),+
        }

        impl $name {
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

pub(crate) const FINDING_STATUSES: &[&str] = &[
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
];
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
pub(crate) const CELL_STATUSES: &[&str] = &["assigned", "covered", "thin", "uncovered"];
pub(crate) const CELL_AGENTS: &[&str] = &["review-finder", "review-sweeper", "plan-executor"];
const FIX_SHAPES: &[&str] = &["edits", "needs-plan"];
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
const LENS: [Vis; 8] = [N, R, N, N, R, R, R, R];
const SEVERITY: [Vis; 8] = [N, R, N, R, R, R, N, R];
const FAILURE: [Vis; 8] = [N, R, N, R, CONTESTED, R, N, R];
const COUNTER: [Vis; 8] = [N, R, N, N, N, R, N, N];
const EVIDENCE: [Vis; 8] = [N, R, N, N, N, R, Vis::First(2), N];
const GRADE: [Vis; 8] = [N, N, N, N, R, N, N, N];
const FIX: [Vis; 8] = [N, N, N, R, N, N, N, R];
const FIX_SHAPE: [Vis; 8] = [N, N, N, R, R, N, N, R];
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
    // findings: identity and bookkeeping
    field(Finding, "id", Ty::Str, IDENT).computed(),
    field(Finding, "run", Ty::Str, INTERNAL).computed(),
    field(Finding, "cell", Ty::Str, LENS).computed(),
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
    field(Finding, "line", Ty::Int, IDENT)
        .range(1, i64::MAX)
        .need(Need::RequiredFor(DEFECT_OR_CLEANUP)),
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
    field(Finding, "fix_shape", Ty::Str, FIX_SHAPE)
        .kinds(KindSet::DEFECT)
        .values(FIX_SHAPES)
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
    field(Finding, "plan_section", Ty::Str, PLAN)
        .plan_only()
        .cap(200)
        .need(Need::RequiredFor(DEFECT_OR_CLEANUP)),
    field(Finding, "anchor_old", Ty::Str, PLAN)
        .plan_only()
        .kinds(DEFECT_OR_CLEANUP)
        .cap(400),
    field(Finding, "anchor_new", Ty::Str, PLAN)
        .plan_only()
        .kinds(DEFECT_OR_CLEANUP)
        .cap(800),
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
];

/// The catalogue entry for `name` on `record`; for a finding, the entry that
/// applies to `kind`, since `effort` differs between defect and cleanup.
pub(crate) fn spec(record: Record, name: &str, kind: Option<Kind>) -> Option<&'static FieldSpec> {
    FIELDS
        .iter()
        .find(|s| s.record == record && s.name == name && kind.is_none_or(|k| s.kinds.contains(k)))
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
            let reason = match kind {
                Some(kind) if spec(record, key, None).is_some() => {
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
                " for direction change or remove, and for add unless target_file names a new file",
            ),
            Need::WhenChallenge => (
                str_field(row, "angle") == Some("challenge"),
                " when angle is challenge",
            ),
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

    if refusals.is_empty() {
        Ok(())
    } else {
        Err(refusals)
    }
}

fn old_required(row: &Map<String, JsonValue>, ctx: &RowContext<'_>) -> bool {
    match str_field(row, "direction").and_then(Direction::parse) {
        Some(Direction::Change | Direction::Remove) => true,
        Some(Direction::Add) => match str_field(row, "target_file") {
            Some(target) => (ctx.target_exists)(target),
            None => true,
        },
        None => false,
    }
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
    match path_problem(path) {
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

/// Why `path` is not repo-relative and `/`-separated, or `None` when it is.
fn path_problem(path: &str) -> Option<&'static str> {
    let bytes = path.as_bytes();
    if path.is_empty() {
        Some("is empty")
    } else if path.contains('\\') {
        Some("uses `\\`; paths are `/`-separated")
    } else if path.starts_with('/')
        || (bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':')
    {
        Some("is absolute; paths are repo-relative")
    } else if path.split('/').any(|segment| segment == "..") {
        Some("contains `..`")
    } else {
        None
    }
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

fn compile(key: &str, patterns: &[String]) -> Result<GlobSet> {
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

/// Who verifies a finding. A critical defect at `deep` goes to a panel and
/// any other defect to the verifier. Cleanup goes to the verifier when it
/// edits executable non-test code or removes a test, at its edit target
/// (`target_file`, else `file`) or any of its `instances`, and to the
/// checker otherwise. A test path is never treated as executable.
#[allow(clippy::too_many_arguments)]
pub(crate) fn route(
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
        Kind::Cleanup => {
            let Some(direction) = direction else {
                return Route::Verifier;
            };
            let edited = std::iter::once(target_file.unwrap_or(file))
                .chain(instances.iter().map(String::as_str));
            for path in edited {
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
            "fix_shape": "edits",
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
    fn defect_requires_failure_scenario_and_fix_shape() {
        assert_eq!(
            validate_row(Record::Finding, &defect(), &ctx(Family::Code)),
            Ok(())
        );
        let mut bare = defect();
        bare.remove("failure_scenario");
        bare.remove("fix_shape");
        let refused = refused_fields(validate_row(Record::Finding, &bare, &ctx(Family::Code)));
        assert_eq!(refused, ["failure_scenario", "fix_shape"]);
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
        for withheld in ["grade", "fix", "fix_shape", "effort"] {
            assert!(!verifier.contains_key(withheld), "verifier sees {withheld}");
        }
        for shown in ["failure_scenario", "counter", "evidence", "severity"] {
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
        (Finding, "lens", None, "- R - - R R R R"),
        (Finding, "cell", None, "- R - - R R R R"),
        (Finding, "severity", None, "- R - R R R - R"),
        (Finding, "failure_scenario", None, "- R - R ct R - R"),
        (Finding, "counter", None, "- R - - - R - -"),
        (Finding, "evidence", None, "- R - - - R E2 -"),
        (Finding, "grade", None, "- - - - R - - -"),
        (Finding, "fix", None, "- - - R - - - R"),
        (Finding, "fix_shape", None, "- - - R R - - R"),
        (Finding, "effort", Some(Kind::Defect), "- - - R - - - R"),
        (Finding, "plan_section", None, "- R - R - R - R"),
        (Finding, "anchor_old", None, "- R - R - R - R"),
        (Finding, "anchor_new", None, "- R - R - R - R"),
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
    fn route_sends_critical_at_deep_to_panel() {
        let defect = |severity, depth| {
            route(
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
}
