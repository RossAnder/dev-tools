//! Field-name constants, the `kind` and `status` vocabularies, the per-status
//! required-field clusters, and the validator for `.claude/backlog.toml`.
//!
//! Owns `backlog_path()` and `read_store()`; every other leaf resolves and
//! reads the store through them rather than rebuilding the path or the
//! exists/verify/strict ladder.
//!
//! Two validators, because the invariants sit at two scopes. `validate`
//! takes one item and checks its status cluster; `validate_ids_unique` takes
//! the whole document, because ids are unique across the union of the
//! `backlog` and `compacted` arrays and no single item can answer that.
//! Neither runs implicitly — every write path calls them by hand.
//!
//! The store carries no evidence field of any kind. The evidence directory
//! is the record and `show` lists it at read time, so a stored count, flag
//! or path is wrong the moment a file is copied in.

use std::collections::BTreeSet;
use std::path::PathBuf;

use anyhow::Result;
use serde_json::Value as JsonValue;
use toml::Value as TomlValue;

use crate::cli::{ReadIntegrityArgs, read_integrity_opts};
use crate::convert::json_type_name;
use crate::errors::{ErrorKind, tagged_err};
use crate::integrity::maybe_verify_integrity;
use crate::io::{items_array, read_toml, repo_or_cwd_root};

/// Array of live captures. Never `items`: an array named `items` under
/// `.claude/` is the default target of `tomlctl items add|update|apply`,
/// whose dedup stamping would overwrite the content-derived `dedup_id`.
pub(crate) const ARRAY_BACKLOG: &str = "backlog";
/// Array of aged-out terminal captures, read by `check` so folding a row
/// away never loses the "we already solved this" answer.
pub(crate) const ARRAY_COMPACTED: &str = "compacted";

pub(crate) const FIELD_ID: &str = "id";
pub(crate) const FIELD_KIND: &str = "kind";
pub(crate) const FIELD_SUMMARY: &str = "summary";
pub(crate) const FIELD_AREA: &str = "area";
pub(crate) const FIELD_TAGS: &str = "tags";
pub(crate) const FIELD_STATUS: &str = "status";
pub(crate) const FIELD_CREATED: &str = "created";
pub(crate) const FIELD_LAST_SEEN: &str = "last_seen";
pub(crate) const FIELD_SEEN_COUNT: &str = "seen_count";
pub(crate) const FIELD_DEDUP_ID: &str = "dedup_id";
pub(crate) const FIELD_ORIGIN: &str = "origin";
pub(crate) const FIELD_FLOW: &str = "flow";
pub(crate) const FIELD_CONTEXT: &str = "context";
pub(crate) const FIELD_EVIDENCE: &str = "evidence";
pub(crate) const FIELD_RELATED: &str = "related";
pub(crate) const FIELD_DUPLICATE_OF: &str = "duplicate_of";
pub(crate) const FIELD_SUPERSEDES: &str = "supersedes";
pub(crate) const FIELD_PROMOTED: &str = "promoted";
pub(crate) const FIELD_PROMOTED_TO: &str = "promoted_to";
pub(crate) const FIELD_DISMISSED: &str = "dismissed";
pub(crate) const FIELD_DISMISS_REASON: &str = "dismiss_reason";
pub(crate) const FIELD_RESOLVED: &str = "resolved";
pub(crate) const FIELD_RESOLUTION: &str = "resolution";
pub(crate) const FIELD_REOPEN_RATIONALE: &str = "reopen_rationale";

/// Compacted-row fields with no live-row counterpart: the three terminal
/// date/companion pairs collapse into one pair, plus the fold date.
pub(crate) const FIELD_TERMINAL_DATE: &str = "terminal_date";
pub(crate) const FIELD_TERMINAL_REASON: &str = "terminal_reason";
pub(crate) const FIELD_COMPACTED_ON: &str = "compacted_on";

pub(crate) const KIND_BUG: &str = "bug";
pub(crate) const KIND_FLAKY_TEST: &str = "flaky-test";
pub(crate) const KIND_DEBT: &str = "debt";
pub(crate) const KIND_DIRECTION: &str = "direction";
pub(crate) const KIND_ANNOYANCE: &str = "annoyance";
pub(crate) const KIND_QUESTION: &str = "question";
/// Coercion target for an unrecognised `kind`.
pub(crate) const KIND_OTHER: &str = "other";

pub(crate) const KINDS: &[&str] = &[
    KIND_BUG,
    KIND_FLAKY_TEST,
    KIND_DEBT,
    KIND_DIRECTION,
    KIND_ANNOYANCE,
    KIND_QUESTION,
    KIND_OTHER,
];

pub(crate) const STATUS_OPEN: &str = "open";
pub(crate) const STATUS_PROMOTED: &str = "promoted";
pub(crate) const STATUS_DISMISSED: &str = "dismissed";
pub(crate) const STATUS_RESOLVED: &str = "resolved";

pub(crate) const STATUSES: &[&str] = &[
    STATUS_OPEN,
    STATUS_PROMOTED,
    STATUS_DISMISSED,
    STATUS_RESOLVED,
];

/// The date field each terminal status must carry, and which `open` must
/// carry none of. Each is spelled the same as its status.
pub(crate) const TERMINAL_DATE_FIELDS: &[&str] =
    &[FIELD_PROMOTED, FIELD_DISMISSED, FIELD_RESOLVED];

/// The typed relation fields, in the order `show` reports them. `related`
/// holds an array of ids; the other two hold a single id.
pub(crate) const RELATION_FIELDS: &[&str] =
    &[FIELD_RELATED, FIELD_DUPLICATE_OF, FIELD_SUPERSEDES];

/// Fields a status transition owns outright. A transition clears every one
/// it does not itself write, which is what lets a row move between two
/// terminal states.
pub(crate) const MANAGED_FIELDS: &[&str] = &[
    FIELD_PROMOTED,
    FIELD_PROMOTED_TO,
    FIELD_DISMISSED,
    FIELD_DISMISS_REASON,
    FIELD_RESOLVED,
    FIELD_RESOLUTION,
    FIELD_REOPEN_RATIONALE,
];

/// Row shape written by `compact` and read by `check`'s
/// `previously-resolved` verdict. `dedup_id` and `context` are load-bearing:
/// the verdict keys on the first and reports the second.
pub(crate) const COMPACTED_FIELDS: &[&str] = &[
    FIELD_ID,
    FIELD_DEDUP_ID,
    FIELD_SUMMARY,
    FIELD_KIND,
    FIELD_AREA,
    FIELD_STATUS,
    FIELD_TERMINAL_DATE,
    FIELD_TERMINAL_REASON,
    FIELD_CONTEXT,
    FIELD_COMPACTED_ON,
];

/// The date/companion pair each terminal status owns, and the single source
/// `required_fields` and `terminal_pair` both read. Typed at exactly two, so
/// a status that grows a third required field is a compile error here rather
/// than a silent `None` in the callers that want the pair.
const TERMINAL_CLUSTERS: &[(&str, [&str; 2])] = &[
    (STATUS_PROMOTED, [FIELD_PROMOTED, FIELD_PROMOTED_TO]),
    (STATUS_DISMISSED, [FIELD_DISMISSED, FIELD_DISMISS_REASON]),
    (STATUS_RESOLVED, [FIELD_RESOLVED, FIELD_RESOLUTION]),
];

fn cluster_of(status: &str) -> Option<&'static [&'static str; 2]> {
    TERMINAL_CLUSTERS
        .iter()
        .find_map(|(name, fields)| (*name == status).then_some(fields))
}

/// Fields an item must carry non-empty for its `status`. `open` requires
/// none; `reopen_rationale` is optional on it.
pub(crate) fn required_fields(status: &str) -> &'static [&'static str] {
    cluster_of(status).map_or(&[], |fields| fields.as_slice())
}

/// The (date, companion) pair a terminal status owns; `None` for `open` and
/// for any unrecognised status.
pub(crate) fn terminal_pair(status: &str) -> Option<(&'static str, &'static str)> {
    cluster_of(status).map(|[date, companion]| (*date, *companion))
}

/// Resolve a stored `kind` against the vocabulary, coercing an unrecognised
/// one to `other` with a stderr warning. Fail-soft, matching the ledger
/// schema's rule for unknown enum values: `kind` only drives `--count-by`
/// grouping, so a wrong bucket costs less than a rejected capture.
pub(crate) fn coerce_kind(raw: &str) -> &'static str {
    if let Some(known) = KINDS.iter().copied().find(|k| *k == raw) {
        return known;
    }
    eprintln!("tomlctl: unknown backlog kind `{raw}` — reading it as `{KIND_OTHER}`");
    KIND_OTHER
}

/// Resolve `<repo-or-cwd-root>/.claude/backlog.toml`, honouring
/// `TOMLCTL_ROOT` through `io::repo_or_cwd_root` so a test sandbox and the
/// real repo resolve by the same precedence. Inside `guard_write_path`'s
/// `.claude/` containment, so the store needs no `--allow-outside`.
pub(crate) fn backlog_path() -> Result<PathBuf> {
    Ok(repo_or_cwd_root()?.join(".claude").join("backlog.toml"))
}

/// The single read seam over the store. Every read verb goes through it, so
/// `--verify-integrity` and `--strict-read` cannot be honoured on one verb
/// and dropped on the next.
///
/// A missing store reads as an empty table — the first capture in a repo
/// runs `check` before anything exists — and `--strict-read` is what turns
/// that into `kind=not_found`. The gate fires before the sidecar check, so a
/// missing file is `not_found` rather than `integrity` even with both flags.
pub(crate) fn read_store(integrity: &ReadIntegrityArgs) -> Result<TomlValue> {
    let file = backlog_path()?;
    if !file.exists() {
        if integrity.strict_read {
            return Err(tagged_err(
                ErrorKind::NotFound,
                Some(file.clone()),
                format!("file does not exist: {}", file.display()),
            ));
        }
        return Ok(TomlValue::Table(toml::map::Map::new()));
    }
    maybe_verify_integrity(&file, read_integrity_opts(integrity))?;
    read_toml(&file)
}

/// Rejection reasons from the two validators. Every variant is a caller
/// mistake rather than an environment failure, hence the single
/// `ErrorKind::Validation` mapping in `kind`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BacklogError {
    NotAnObject {
        got: &'static str,
    },
    MissingField {
        field: &'static str,
    },
    UnknownStatus {
        status: String,
    },
    MissingStatusField {
        status: String,
        field: &'static str,
    },
    /// A terminal date or companion belonging to some status other than the
    /// row's own — the reverse half of the terminal-status invariant, and
    /// what holds a row to exactly one cluster.
    ForeignTerminalField {
        status: String,
        field: &'static str,
    },
    DuplicateId {
        id: String,
    },
}

impl BacklogError {
    pub(crate) fn kind(&self) -> ErrorKind {
        match self {
            Self::NotAnObject { .. }
            | Self::MissingField { .. }
            | Self::UnknownStatus { .. }
            | Self::MissingStatusField { .. }
            | Self::ForeignTerminalField { .. }
            | Self::DuplicateId { .. } => ErrorKind::Validation,
        }
    }

    /// Lift into the `anyhow` chain with the tag `--error-format json`
    /// reads, so a rejected write surfaces as
    /// `{"error":{"kind":"validation",…}}`.
    pub(crate) fn into_tagged(self, file: Option<PathBuf>) -> anyhow::Error {
        tagged_err(self.kind(), file, self.to_string())
    }
}

impl std::fmt::Display for BacklogError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAnObject { got } => {
                write!(f, "backlog item must be a JSON object; got {got}")
            }
            Self::MissingField { field } => {
                write!(f, "backlog item is missing required field `{field}`")
            }
            Self::UnknownStatus { status } => write!(
                f,
                "backlog item has unknown status \"{status}\"; expected one of {}",
                STATUSES.join(", ")
            ),
            Self::MissingStatusField { status, field } => write!(
                f,
                "backlog item with status=\"{status}\" is missing required field `{field}`"
            ),
            Self::ForeignTerminalField { status, field } => write!(
                f,
                "backlog item with status=\"{status}\" must not carry the terminal field `{field}`"
            ),
            Self::DuplicateId { id } => {
                write!(f, "backlog id \"{id}\" appears more than once")
            }
        }
    }
}

impl std::error::Error for BacklogError {}

/// Same predicate as `items::is_empty_json`, which is module-private there:
/// `null`, `""` and `[]` count as absent, so a placeholder field an agent
/// never filled in reads as a gap rather than a value.
fn is_empty_json(v: &JsonValue) -> bool {
    match v {
        JsonValue::Null => true,
        JsonValue::String(s) => s.is_empty(),
        JsonValue::Array(a) => a.is_empty(),
        _ => false,
    }
}

fn missing(map: &serde_json::Map<String, JsonValue>, field: &str) -> bool {
    !map.get(field).is_some_and(|v| !is_empty_json(v))
}

/// Validate one backlog item. `id`, `summary` and `status` are required of
/// every row — `dedup_id`, the id and the evidence directory all derive from
/// the first two. The status invariant runs both ways: a row carries the
/// date and companion its own status names, non-empty, and no field from any
/// other status's cluster — so `open` carries no terminal cluster at all,
/// though it may hold `reopen_rationale`.
///
/// An unknown `status` is rejected rather than coerced, because `triage`,
/// `check` and `compact` all select on the four known values and would skip
/// a typo'd row forever. `kind` is not checked here: an unknown one coerces
/// where it is read — see `coerce_kind`.
///
/// Ids are unique across both arrays, which no single item can check — call
/// `validate_ids_unique` on the document too.
///
/// Live `backlog` rows only: a `compacted` row folds its terminal date and
/// companion into `terminal_date` / `terminal_reason` (`COMPACTED_FIELDS`),
/// so it does not satisfy the cluster its `status` names.
pub(crate) fn validate(value: &JsonValue) -> std::result::Result<(), BacklogError> {
    let JsonValue::Object(map) = value else {
        return Err(BacklogError::NotAnObject {
            got: json_type_name(value),
        });
    };
    for field in [FIELD_ID, FIELD_SUMMARY, FIELD_STATUS] {
        if missing(map, field) {
            return Err(BacklogError::MissingField { field });
        }
    }
    let status = map
        .get(FIELD_STATUS)
        .and_then(|v| v.as_str())
        .ok_or(BacklogError::MissingField {
            field: FIELD_STATUS,
        })?;
    if !STATUSES.contains(&status) {
        return Err(BacklogError::UnknownStatus {
            status: status.to_string(),
        });
    }
    for field in required_fields(status) {
        if missing(map, field) {
            return Err(BacklogError::MissingStatusField {
                status: status.to_string(),
                field,
            });
        }
    }
    for (owner, [date_field, companion]) in TERMINAL_CLUSTERS {
        if *owner == status {
            continue;
        }
        for field in [*date_field, *companion] {
            if !missing(map, field) {
                return Err(BacklogError::ForeignTerminalField {
                    status: status.to_string(),
                    field,
                });
            }
        }
    }
    Ok(())
}

/// Enforce id-uniqueness across the union of the `backlog` and `compacted`
/// arrays. Content-derived ids do not make a text merge converge — two
/// worktrees minting one discovery agree on `id` and differ on `created`,
/// `origin` and `context` — so this is what makes such a collision visible
/// rather than silent. A row with no `id` is `validate`'s to reject.
pub(crate) fn validate_ids_unique(doc: &TomlValue) -> std::result::Result<(), BacklogError> {
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    for array in [ARRAY_BACKLOG, ARRAY_COMPACTED] {
        for item in items_array(doc, array) {
            let id = item
                .get(FIELD_ID)
                .and_then(TomlValue::as_str)
                .unwrap_or_default();
            if id.is_empty() {
                continue;
            }
            if !seen.insert(id) {
                return Err(BacklogError::DuplicateId { id: id.to_string() });
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn open_item() -> JsonValue {
        json!({
            "id": "B-a1b2c3d4",
            "summary": "pty readiness probe flakes on slow CI",
            "kind": KIND_FLAKY_TEST,
            "status": STATUS_OPEN,
        })
    }

    fn with(status: &str, extra: &[(&str, &str)]) -> JsonValue {
        let mut v = open_item();
        let map = v.as_object_mut().unwrap();
        map.insert(FIELD_STATUS.into(), json!(status));
        for (k, val) in extra {
            map.insert((*k).into(), json!(val));
        }
        v
    }

    #[test]
    fn open_item_needs_no_companion() {
        assert_eq!(validate(&open_item()), Ok(()));
    }

    #[test]
    fn promoted_requires_date_and_target() {
        assert_eq!(
            validate(&with(
                STATUS_PROMOTED,
                &[
                    (FIELD_PROMOTED, "2026-09-01"),
                    (FIELD_PROMOTED_TO, "docs/plans/x.md")
                ]
            )),
            Ok(())
        );
        assert_eq!(
            validate(&with(STATUS_PROMOTED, &[(FIELD_PROMOTED, "2026-09-01")])),
            Err(BacklogError::MissingStatusField {
                status: STATUS_PROMOTED.into(),
                field: FIELD_PROMOTED_TO,
            })
        );
        assert_eq!(
            validate(&with(
                STATUS_PROMOTED,
                &[(FIELD_PROMOTED_TO, "docs/plans/x.md")]
            )),
            Err(BacklogError::MissingStatusField {
                status: STATUS_PROMOTED.into(),
                field: FIELD_PROMOTED,
            })
        );
    }

    #[test]
    fn dismissed_requires_date_and_reason() {
        assert_eq!(
            validate(&with(
                STATUS_DISMISSED,
                &[
                    (FIELD_DISMISSED, "2026-09-01"),
                    (FIELD_DISMISS_REASON, "duplicate of B-7f0e2d91")
                ]
            )),
            Ok(())
        );
        assert_eq!(
            validate(&with(STATUS_DISMISSED, &[(FIELD_DISMISSED, "2026-09-01")])),
            Err(BacklogError::MissingStatusField {
                status: STATUS_DISMISSED.into(),
                field: FIELD_DISMISS_REASON,
            })
        );
    }

    #[test]
    fn resolved_requires_resolution() {
        assert_eq!(
            validate(&with(
                STATUS_RESOLVED,
                &[
                    (FIELD_RESOLVED, "2026-09-01"),
                    (FIELD_RESOLUTION, "fixed in abc123")
                ]
            )),
            Ok(())
        );
        assert_eq!(
            validate(&with(STATUS_RESOLVED, &[(FIELD_RESOLVED, "2026-09-01")])),
            Err(BacklogError::MissingStatusField {
                status: STATUS_RESOLVED.into(),
                field: FIELD_RESOLUTION,
            })
        );
    }

    #[test]
    fn empty_companion_counts_as_missing() {
        let mut v = with(STATUS_RESOLVED, &[(FIELD_RESOLVED, "2026-09-01")]);
        v.as_object_mut()
            .unwrap()
            .insert(FIELD_RESOLUTION.into(), json!(""));
        assert_eq!(
            validate(&v),
            Err(BacklogError::MissingStatusField {
                status: STATUS_RESOLVED.into(),
                field: FIELD_RESOLUTION,
            })
        );
    }

    #[test]
    fn open_rejects_every_terminal_date() {
        for field in TERMINAL_DATE_FIELDS {
            let mut v = open_item();
            v.as_object_mut()
                .unwrap()
                .insert((*field).into(), json!("2026-09-01"));
            assert_eq!(
                validate(&v),
                Err(BacklogError::ForeignTerminalField {
                    status: STATUS_OPEN.into(),
                    field
                }),
                "status=open must reject a `{field}` date"
            );
        }
    }

    #[test]
    fn a_terminal_status_rejects_another_status_cluster() {
        let mut v = with(
            STATUS_DISMISSED,
            &[
                (FIELD_DISMISSED, "2026-09-01"),
                (FIELD_DISMISS_REASON, "superseded by B-7f0e2d91"),
                (FIELD_RESOLVED, "2026-08-01"),
            ],
        );
        assert_eq!(
            validate(&v),
            Err(BacklogError::ForeignTerminalField {
                status: STATUS_DISMISSED.into(),
                field: FIELD_RESOLVED,
            })
        );
        let map = v.as_object_mut().unwrap();
        map.remove(FIELD_RESOLVED);
        map.insert(FIELD_RESOLUTION.into(), json!("fixed in abc123"));
        assert_eq!(
            validate(&v),
            Err(BacklogError::ForeignTerminalField {
                status: STATUS_DISMISSED.into(),
                field: FIELD_RESOLUTION,
            }),
            "the companion is as foreign as the date"
        );
    }

    #[test]
    fn open_accepts_reopen_rationale() {
        let mut v = open_item();
        v.as_object_mut().unwrap().insert(
            FIELD_REOPEN_RATIONALE.into(),
            json!("resurfaced on the 2026-09 CI run"),
        );
        assert_eq!(validate(&v), Ok(()));
    }

    #[test]
    fn identity_fields_are_required() {
        for field in [FIELD_ID, FIELD_SUMMARY, FIELD_STATUS] {
            let mut v = open_item();
            v.as_object_mut().unwrap().remove(field);
            assert_eq!(validate(&v), Err(BacklogError::MissingField { field }));
        }
    }

    #[test]
    fn unknown_status_is_rejected() {
        assert_eq!(
            validate(&with("resolvd", &[])),
            Err(BacklogError::UnknownStatus {
                status: "resolvd".into()
            })
        );
    }

    #[test]
    fn non_object_is_rejected() {
        assert_eq!(
            validate(&json!(["not", "an", "object"])),
            Err(BacklogError::NotAnObject { got: "array" })
        );
    }

    #[test]
    fn unknown_kind_coerces_to_other() {
        let mut v = open_item();
        v.as_object_mut()
            .unwrap()
            .insert(FIELD_KIND.into(), json!("regression"));
        assert_eq!(validate(&v), Ok(()));
        assert_eq!(coerce_kind("regression"), KIND_OTHER);
        assert_eq!(coerce_kind(KIND_FLAKY_TEST), KIND_FLAKY_TEST);
    }

    #[test]
    fn every_error_maps_to_validation() {
        let err = BacklogError::DuplicateId {
            id: "B-a1b2c3d4".into(),
        };
        assert_eq!(err.kind().as_str(), "validation");
        let tagged = err.into_tagged(None);
        assert!(format!("{tagged:#}").contains("B-a1b2c3d4"));
    }

    fn doc(s: &str) -> TomlValue {
        toml::from_str(s).unwrap()
    }

    const CROSS_ARRAY_COLLISION: &str = r#"
[[backlog]]
id = "B-a1b2c3d4"
summary = "live row"
status = "open"

[[compacted]]
id = "B-a1b2c3d4"
summary = "aged-out row"
status = "resolved"
"#;

    #[test]
    fn ids_are_unique_across_both_arrays() {
        assert_eq!(
            validate_ids_unique(&doc(CROSS_ARRAY_COLLISION)),
            Err(BacklogError::DuplicateId {
                id: "B-a1b2c3d4".into()
            })
        );
    }

    #[test]
    fn ids_are_unique_within_the_backlog_array() {
        let d = doc(
            r#"
[[backlog]]
id = "B-a1b2c3d4"
summary = "one"
status = "open"

[[backlog]]
id = "B-a1b2c3d4"
summary = "two"
status = "open"
"#,
        );
        assert_eq!(
            validate_ids_unique(&d),
            Err(BacklogError::DuplicateId {
                id: "B-a1b2c3d4".into()
            })
        );
    }

    #[test]
    fn distinct_ids_and_an_empty_store_pass() {
        let d = doc(
            r#"
[[backlog]]
id = "B-a1b2c3d4"
summary = "one"
status = "open"

[[compacted]]
id = "B-7f0e2d91"
summary = "two"
status = "resolved"
"#,
        );
        assert_eq!(validate_ids_unique(&d), Ok(()));
        assert_eq!(validate_ids_unique(&doc("schema_version = 1\n")), Ok(()));
    }

    #[test]
    fn compacted_fields_are_pinned() {
        assert_eq!(
            COMPACTED_FIELDS,
            &[
                "id",
                "dedup_id",
                "summary",
                "kind",
                "area",
                "status",
                "terminal_date",
                "terminal_reason",
                "context",
                "compacted_on",
            ]
        );
        // The directory is the record: no stored count, flag or path.
        assert!(
            !COMPACTED_FIELDS
                .iter()
                .any(|f| f.starts_with(FIELD_EVIDENCE))
        );
    }

    #[test]
    fn vocabularies_are_pinned() {
        assert_eq!(
            KINDS,
            &[
                "bug",
                "flaky-test",
                "debt",
                "direction",
                "annoyance",
                "question",
                "other"
            ]
        );
        assert_eq!(
            STATUSES,
            &["open", "promoted", "dismissed", "resolved"]
        );
        assert_eq!(TERMINAL_DATE_FIELDS, &["promoted", "dismissed", "resolved"]);
        assert_eq!(
            RELATION_FIELDS,
            &["related", "duplicate_of", "supersedes"]
        );
    }

    #[test]
    fn every_terminal_status_owns_a_pair_and_open_owns_none() {
        assert_eq!(terminal_pair(STATUS_OPEN), None);
        assert!(required_fields(STATUS_OPEN).is_empty());
        for status in STATUSES.iter().filter(|s| **s != STATUS_OPEN) {
            let Some((date, companion)) = terminal_pair(status) else {
                panic!("status=\"{status}\" must own a terminal pair");
            };
            assert_eq!(date, *status, "the date field is spelled as its status");
            assert!(TERMINAL_DATE_FIELDS.contains(&date));
            assert_eq!(
                required_fields(status),
                &[date, companion],
                "the pair and the required cluster must not disagree"
            );
        }
    }

    #[test]
    fn backlog_path_resolves_under_dot_claude() {
        let (root, got) = crate::test_support::with_root(|root| {
            (root.to_path_buf(), backlog_path().unwrap())
        });
        assert_eq!(got, root.join(".claude").join("backlog.toml"));
        assert!(got.ends_with(std::path::Path::new(".claude/backlog.toml")));
    }

    fn read_args(strict_read: bool) -> ReadIntegrityArgs {
        ReadIntegrityArgs {
            verify_integrity: false,
            strict_read,
        }
    }

    #[test]
    fn a_missing_store_reads_empty_and_gates_on_strict_read() {
        let (lenient, strict) = crate::test_support::with_root(|_| {
            (read_store(&read_args(false)), read_store(&read_args(true)))
        });
        assert!(items_array(&lenient.unwrap(), ARRAY_BACKLOG).is_empty());
        let err = strict.unwrap_err();
        assert_eq!(
            err.downcast_ref::<crate::errors::TaggedError>()
                .map_or("other", |tagged| tagged.kind.as_str()),
            "not_found"
        );
    }
}
