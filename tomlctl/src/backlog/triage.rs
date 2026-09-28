//! `backlog triage` — status transitions and the companion fields each one
//! requires.
//!
//! Every transition rewrites the whole managed cluster through
//! `schema::clear_for_transition`, not just the pair it sets: `resolved` →
//! `dismissed` without clearing `resolved` / `resolution` would produce a row
//! `schema::validate` refuses. The promotion claim survives a move to
//! `resolved` or `dismissed` as history; `reopen` drops it.
//!
//! A bulk sweep is all-or-nothing: every id is resolved and every rewritten
//! row validated before the first is stored, so one unknown id leaves the
//! file and its sidecar untouched.

use anyhow::Result;
use serde_json::json;
use toml::Value as TomlValue;

use super::schema::{self, FIELD_LAST_UPDATED};
use super::target::{Resolver, Target};
use crate::cli::{TriageMode, WriteIntegrityArgs, write_integrity_opts};
use crate::convert::toml_to_json;
use crate::errors::{ErrorKind, tagged_arg_err, tagged_err};
use crate::io::{
    item_id, items_array, items_array_mut, mutate_doc, on_missing_for, relativise,
    repo_or_cwd_root, warn_if_created,
};
use crate::output::print_json_compact;

/// The chosen mode with its companion value already resolved, so the write
/// path can no longer be handed a `--promote` without a `--to`.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Transition {
    Promote(String),
    Dismiss(String),
    Resolve(String),
    Reopen(String),
}

impl Transition {
    fn name(&self) -> &'static str {
        match self {
            Self::Promote(_) => "promote",
            Self::Dismiss(_) => "dismiss",
            Self::Resolve(_) => "resolve",
            Self::Reopen(_) => "reopen",
        }
    }

    fn status(&self) -> &'static str {
        match self {
            Self::Promote(_) => schema::STATUS_PROMOTED,
            Self::Dismiss(_) => schema::STATUS_DISMISSED,
            Self::Resolve(_) => schema::STATUS_RESOLVED,
            Self::Reopen(_) => schema::STATUS_OPEN,
        }
    }

    /// The terminal date field to stamp (`None` for `reopen`, which lands on
    /// `open` and so may carry no terminal date), the companion field, and
    /// the companion's value.
    fn writes(&self) -> (Option<&'static str>, &'static str, &str) {
        match self {
            Self::Promote(v) => (
                Some(schema::FIELD_PROMOTED),
                schema::FIELD_PROMOTED_TO,
                v.as_str(),
            ),
            Self::Dismiss(v) => (
                Some(schema::FIELD_DISMISSED),
                schema::FIELD_DISMISS_REASON,
                v.as_str(),
            ),
            Self::Resolve(v) => (
                Some(schema::FIELD_RESOLVED),
                schema::FIELD_RESOLUTION,
                v.as_str(),
            ),
            Self::Reopen(v) => (None, schema::FIELD_REOPEN_RATIONALE, v.as_str()),
        }
    }

    fn from_cli(
        mode: TriageMode,
        to: Option<String>,
        reason: Option<String>,
        resolution: Option<String>,
        rationale: Option<String>,
    ) -> Result<Self> {
        let TriageMode {
            promote,
            dismiss,
            resolve,
            reopen,
        } = mode;
        match (promote, dismiss, resolve, reopen) {
            (true, false, false, false) => Ok(Self::Promote(companion(to, "--promote", "--to")?)),
            (false, true, false, false) => {
                Ok(Self::Dismiss(companion(reason, "--dismiss", "--reason")?))
            }
            (false, false, true, false) => Ok(Self::Resolve(companion(
                resolution,
                "--resolve",
                "--resolution",
            )?)),
            (false, false, false, true) => Ok(Self::Reopen(companion(
                rationale,
                "--reopen",
                "--rationale",
            )?)),
            _ => Err(tagged_err(
                ErrorKind::Validation,
                None,
                "`backlog triage` takes exactly one of --promote, --dismiss, --resolve, --reopen",
            )),
        }
    }
}

/// A blank companion is a missing one: `schema::validate` reads `""` as
/// absent, so accepting it would only defer the rejection to the validator.
fn companion(value: Option<String>, mode_flag: &str, flag: &str) -> Result<String> {
    match value {
        Some(v) if !v.trim().is_empty() => Ok(v),
        _ => Err(tagged_err(
            ErrorKind::Validation,
            None,
            format!("`backlog triage {mode_flag}` requires a non-empty `{flag}`"),
        )),
    }
}

fn not_found(id: &str, compacted: bool) -> anyhow::Error {
    let msg = if compacted {
        format!(
            "backlog id \"{id}\" has been compacted; triage only transitions live `[[{}]]` rows",
            schema::ARRAY_BACKLOG
        )
    } else {
        format!("no backlog item with id \"{id}\"")
    };
    tagged_arg_err(ErrorKind::NotFound, "ids", msg)
}

fn rewrite(row: &mut TomlValue, t: &Transition, today: toml::value::Datetime) -> Result<()> {
    let table = row.as_table_mut().ok_or_else(|| {
        tagged_err(
            ErrorKind::Validation,
            None,
            format!("`[[{}]]` entry is not a table", schema::ARRAY_BACKLOG),
        )
    })?;
    let (date_field, companion_field, value) = t.writes();
    schema::clear_for_transition(table, t.status());
    // `insert` keeps an existing key in place under `preserve_order`, so
    // re-running the same transition does not reshuffle the row.
    table.insert(
        schema::FIELD_STATUS.to_string(),
        TomlValue::String(t.status().to_string()),
    );
    if let Some(field) = date_field {
        table.insert(field.to_string(), TomlValue::Datetime(today));
    }
    table.insert(
        companion_field.to_string(),
        TomlValue::String(value.to_string()),
    );
    Ok(())
}

/// Resolve one row and record what delivered it. The row is validated here;
/// storing it is the caller's.
pub(crate) fn resolve_with_link(
    row: &mut TomlValue,
    resolution: &str,
    link: &schema::ResolutionLink,
    today: toml::value::Datetime,
) -> Result<()> {
    rewrite(row, &Transition::Resolve(resolution.to_string()), today)?;
    if let Some(table) = row.as_table_mut() {
        link.write_to(table);
    }
    schema::validate(&toml_to_json(row)).map_err(|e| e.into_tagged(None))
}

/// Transition every named id, or none of them: ids are resolved and their
/// rewritten rows validated up front, and only then is anything stored.
/// A duplicate id is `check`'s to report rather than triage's to refuse, so
/// an id matching two rows transitions both.
fn apply_transition(
    doc: &mut TomlValue,
    ids: &[String],
    t: &Transition,
    today: toml::value::Datetime,
) -> Result<()> {
    let mut targets: Vec<usize> = Vec::new();
    for id in ids {
        let mut found = false;
        for (index, item) in items_array(doc, schema::ARRAY_BACKLOG).iter().enumerate() {
            if item_id(item) != Some(id.as_str()) {
                continue;
            }
            found = true;
            if !targets.contains(&index) {
                targets.push(index);
            }
        }
        if !found {
            let compacted = items_array(doc, schema::ARRAY_COMPACTED)
                .iter()
                .any(|item| item_id(item) == Some(id.as_str()));
            return Err(not_found(id, compacted));
        }
    }

    let mut staged: Vec<(usize, TomlValue)> = Vec::with_capacity(targets.len());
    for &index in &targets {
        let mut row = items_array(doc, schema::ARRAY_BACKLOG)[index].clone();
        rewrite(&mut row, t, today)?;
        schema::validate(&toml_to_json(&row)).map_err(|e| e.into_tagged(None))?;
        staged.push((index, row));
    }

    let rows = items_array_mut(doc, schema::ARRAY_BACKLOG)?;
    for (index, row) in staged {
        rows[index] = row;
    }
    if let Some(table) = doc.as_table_mut() {
        table.insert(FIELD_LAST_UPDATED.to_string(), TomlValue::Datetime(today));
    }
    Ok(())
}

/// The `promoted_to` value for a `--to` argument. A value already spelled
/// `external:<text>` passes without `external`, and is not prefixed twice
/// with it.
fn promotion_target(
    resolver: &Resolver,
    raw: &str,
    external: bool,
    allow_closed: bool,
) -> Result<String> {
    let target = match resolver.resolve(raw) {
        t @ Target::External(_) => t,
        _ if external => Target::External(raw.to_string()),
        Target::Unknown(v) => {
            return Err(tagged_arg_err(
                ErrorKind::NotFound,
                "to",
                format!(
                    "no flow or plan `{v}` — bootstrap a draft seed flow \
                     (see /backlog) or pass --external"
                ),
            ));
        }
        t => t,
    };
    if let Target::Flow { slug, status, .. } = &target
        && target.is_closed()
        && !allow_closed
    {
        return Err(tagged_arg_err(
            ErrorKind::Validation,
            "to",
            format!("flow `{slug}` is at `{status}`; pass --allow-closed to promote into it"),
        ));
    }
    Ok(target.stored_value())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn dispatch(
    ids: Vec<String>,
    mode: TriageMode,
    to: Option<String>,
    reason: Option<String>,
    resolution: Option<String>,
    rationale: Option<String>,
    external: bool,
    allow_closed: bool,
    integrity: WriteIntegrityArgs,
) -> Result<()> {
    let transition = Transition::from_cli(mode, to, reason, resolution, rationale)?;
    let root = repo_or_cwd_root()?;
    let transition = match transition {
        Transition::Promote(raw) => {
            // Ahead of the lookup, so a `--to` naming the skipped flow fails
            // after the line saying why.
            let resolver = Resolver::new(&root)?;
            resolver.advise_skipped();
            Transition::Promote(promotion_target(&resolver, &raw, external, allow_closed)?)
        }
        other => other,
    };
    let path = schema::backlog_path()?;
    let today = crate::time::today_toml_date()?;
    let opts = write_integrity_opts(&integrity);
    let on_missing = on_missing_for(&path, integrity.no_create)?;
    let created = mutate_doc(&path, integrity.allow_outside, opts, on_missing, |doc| {
        apply_transition(doc, &ids, &transition, today)
    })?;
    warn_if_created(&path, created);
    let mut out = json!({
        "ok": true,
        "transition": transition.name(),
        "ids": ids,
        "path": relativise(&root, &path),
    });
    if let Transition::Promote(to) = &transition {
        out["to"] = json!(to);
    }
    print_json_compact(&out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::with_root;
    use std::path::{Path, PathBuf};

    const STORE: &str = r#"schema_version = 1
last_updated = 2026-08-01

[[backlog]]
id = "B-aaaaaaaa"
kind = "bug"
summary = "first"
area = "tomlctl/src/io.rs"
status = "open"
created = 2026-08-01
last_seen = 2026-08-01
seen_count = 1
dedup_id = "aaaaaaaaaaaaaaaa"

[[backlog]]
id = "B-bbbbbbbb"
kind = "debt"
summary = "second"
area = ""
status = "open"
created = 2026-08-01
last_seen = 2026-08-01
seen_count = 1
dedup_id = "bbbbbbbbbbbbbbbb"

[[backlog]]
id = "B-cccccccc"
kind = "question"
summary = "third"
area = ""
status = "open"
created = 2026-08-01
last_seen = 2026-08-01
seen_count = 1
dedup_id = "cccccccccccccccc"

[[compacted]]
id = "B-dddddddd"
dedup_id = "dddddddddddddddd"
summary = "aged out"
kind = "bug"
area = ""
status = "resolved"
terminal_date = 2026-01-01
terminal_reason = "fixed"
context = ""
compacted_on = 2026-06-01
"#;

    fn store() -> TomlValue {
        toml::from_str(STORE).unwrap()
    }

    fn today() -> toml::value::Datetime {
        "2026-09-02".parse().unwrap()
    }

    fn row<'a>(doc: &'a TomlValue, id: &str) -> &'a toml::Table {
        items_array(doc, schema::ARRAY_BACKLOG)
            .iter()
            .find(|item| item_id(item) == Some(id))
            .and_then(TomlValue::as_table)
            .unwrap()
    }

    fn field(doc: &TomlValue, id: &str, name: &str) -> Option<String> {
        row(doc, id).get(name).map(ToString::to_string)
    }

    fn kind_of(err: &anyhow::Error) -> &'static str {
        err.downcast_ref::<crate::errors::TaggedError>()
            .map_or("other", |tagged| tagged.kind.as_str())
    }

    fn arg_of(err: &anyhow::Error) -> Option<&'static str> {
        err.downcast_ref::<crate::errors::TaggedError>()
            .and_then(|tagged| tagged.arg)
    }

    fn assert_valid(doc: &TomlValue, id: &str) {
        let item = TomlValue::Table(row(doc, id).clone());
        assert_eq!(
            schema::validate(&toml_to_json(&item)),
            Ok(()),
            "{id} must satisfy its status cluster"
        );
    }

    fn ids(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn each_transition_writes_its_status_date_and_companion() {
        let cases = [
            (
                Transition::Promote("docs/plans/x.md".into()),
                schema::STATUS_PROMOTED,
                Some(schema::FIELD_PROMOTED),
                schema::FIELD_PROMOTED_TO,
            ),
            (
                Transition::Dismiss("not worth it".into()),
                schema::STATUS_DISMISSED,
                Some(schema::FIELD_DISMISSED),
                schema::FIELD_DISMISS_REASON,
            ),
            (
                Transition::Resolve("fixed in 960677b".into()),
                schema::STATUS_RESOLVED,
                Some(schema::FIELD_RESOLVED),
                schema::FIELD_RESOLUTION,
            ),
            (
                Transition::Reopen("resurfaced on CI".into()),
                schema::STATUS_OPEN,
                None,
                schema::FIELD_REOPEN_RATIONALE,
            ),
        ];
        for (transition, status, date_field, companion_field) in cases {
            let mut doc = store();
            apply_transition(&mut doc, &ids(&["B-aaaaaaaa"]), &transition, today()).unwrap();
            assert_eq!(
                field(&doc, "B-aaaaaaaa", schema::FIELD_STATUS).as_deref(),
                Some(format!("\"{status}\"").as_str())
            );
            match date_field {
                Some(f) => assert_eq!(
                    field(&doc, "B-aaaaaaaa", f).as_deref(),
                    Some("2026-09-02"),
                    "{status} must stamp `{f}`"
                ),
                None => {
                    for f in schema::TERMINAL_DATE_FIELDS {
                        assert_eq!(field(&doc, "B-aaaaaaaa", f), None);
                    }
                }
            }
            assert!(
                field(&doc, "B-aaaaaaaa", companion_field).is_some(),
                "{status} must write `{companion_field}`"
            );
            assert_valid(&doc, "B-aaaaaaaa");
            assert_eq!(
                doc.get(FIELD_LAST_UPDATED)
                    .map(ToString::to_string)
                    .as_deref(),
                Some("2026-09-02")
            );
        }
    }

    #[test]
    fn reopen_clears_the_terminal_date_and_companion() {
        let mut doc = store();
        apply_transition(
            &mut doc,
            &ids(&["B-aaaaaaaa"]),
            &Transition::Promote("docs/plans/x.md".into()),
            today(),
        )
        .unwrap();
        apply_transition(
            &mut doc,
            &ids(&["B-aaaaaaaa"]),
            &Transition::Reopen("plan was shelved".into()),
            today(),
        )
        .unwrap();
        assert_eq!(field(&doc, "B-aaaaaaaa", schema::FIELD_PROMOTED), None);
        assert_eq!(field(&doc, "B-aaaaaaaa", schema::FIELD_PROMOTED_TO), None);
        assert_eq!(
            field(&doc, "B-aaaaaaaa", schema::FIELD_REOPEN_RATIONALE).as_deref(),
            Some("\"plan was shelved\"")
        );
        assert_valid(&doc, "B-aaaaaaaa");
    }

    fn promote_then(t: &Transition) -> TomlValue {
        let mut doc = store();
        apply_transition(
            &mut doc,
            &ids(&["B-aaaaaaaa"]),
            &Transition::Promote("some-flow".into()),
            today(),
        )
        .unwrap();
        apply_transition(&mut doc, &ids(&["B-aaaaaaaa"]), t, today()).unwrap();
        doc
    }

    #[test]
    fn resolve_and_dismiss_keep_the_promotion_claim() {
        for t in [
            Transition::Resolve("shipped".into()),
            Transition::Dismiss("dropped".into()),
        ] {
            let doc = promote_then(&t);
            assert_eq!(
                field(&doc, "B-aaaaaaaa", schema::FIELD_PROMOTED).as_deref(),
                Some("2026-09-02"),
                "{}",
                t.name()
            );
            assert_eq!(
                field(&doc, "B-aaaaaaaa", schema::FIELD_PROMOTED_TO).as_deref(),
                Some("\"some-flow\""),
                "{}",
                t.name()
            );
            assert_valid(&doc, "B-aaaaaaaa");
        }
    }

    fn link() -> schema::ResolutionLink {
        schema::ResolutionLink {
            flow: "some-flow".into(),
            tasks: vec!["add-the-thing".into()],
            commits: vec!["abc1234".into()],
        }
    }

    #[test]
    fn resolve_with_link_writes_the_link_and_keeps_the_claim() {
        let doc = promote_then(&Transition::Promote("some-flow".into()));
        let mut item = TomlValue::Table(row(&doc, "B-aaaaaaaa").clone());
        resolve_with_link(&mut item, "shipped", &link(), today()).unwrap();
        let table = item.as_table().unwrap();
        assert_eq!(
            table.get(schema::FIELD_STATUS).and_then(TomlValue::as_str),
            Some(schema::STATUS_RESOLVED)
        );
        assert_eq!(
            table
                .get(schema::FIELD_RESOLVED_FLOW)
                .and_then(TomlValue::as_str),
            Some("some-flow")
        );
        assert!(table.contains_key(schema::FIELD_RESOLVED_TASKS));
        assert!(table.contains_key(schema::FIELD_RESOLVED_COMMITS));
        assert!(table.contains_key(schema::FIELD_PROMOTED_TO));
    }

    #[test]
    fn a_linked_resolved_row_re_dismissed_drops_the_link() {
        let mut doc = store();
        let rows = items_array_mut(&mut doc, schema::ARRAY_BACKLOG).unwrap();
        resolve_with_link(&mut rows[0], "shipped", &link(), today()).unwrap();
        apply_transition(
            &mut doc,
            &ids(&["B-aaaaaaaa"]),
            &Transition::Dismiss("was never ours".into()),
            today(),
        )
        .unwrap();
        for f in schema::RESOLUTION_LINK_FIELDS {
            assert_eq!(field(&doc, "B-aaaaaaaa", f), None, "`{f}` must be dropped");
        }
        assert_valid(&doc, "B-aaaaaaaa");
    }

    #[test]
    fn dismiss_after_resolve_clears_the_resolved_pair() {
        let mut doc = store();
        apply_transition(
            &mut doc,
            &ids(&["B-bbbbbbbb"]),
            &Transition::Resolve("fixed upstream".into()),
            today(),
        )
        .unwrap();
        apply_transition(
            &mut doc,
            &ids(&["B-bbbbbbbb"]),
            &Transition::Dismiss("was never ours".into()),
            today(),
        )
        .unwrap();
        assert_eq!(field(&doc, "B-bbbbbbbb", schema::FIELD_RESOLVED), None);
        assert_eq!(field(&doc, "B-bbbbbbbb", schema::FIELD_RESOLUTION), None);
        assert_eq!(
            field(&doc, "B-bbbbbbbb", schema::FIELD_DISMISS_REASON).as_deref(),
            Some("\"was never ours\"")
        );
        assert_valid(&doc, "B-bbbbbbbb");
    }

    #[test]
    fn a_terminal_transition_drops_a_stale_reopen_rationale() {
        let mut doc = store();
        apply_transition(
            &mut doc,
            &ids(&["B-cccccccc"]),
            &Transition::Reopen("came back".into()),
            today(),
        )
        .unwrap();
        apply_transition(
            &mut doc,
            &ids(&["B-cccccccc"]),
            &Transition::Resolve("fixed for real".into()),
            today(),
        )
        .unwrap();
        assert_eq!(
            field(&doc, "B-cccccccc", schema::FIELD_REOPEN_RATIONALE),
            None
        );
        assert_valid(&doc, "B-cccccccc");
    }

    #[test]
    fn an_unknown_id_in_a_bulk_call_mutates_nothing() {
        let mut doc = store();
        let before = doc.clone();
        let err = apply_transition(
            &mut doc,
            &ids(&["B-aaaaaaaa", "B-nosuchid", "B-bbbbbbbb"]),
            &Transition::Dismiss("sweep".into()),
            today(),
        )
        .unwrap_err();
        assert_eq!(kind_of(&err), "not_found", "{err:#}");
        assert_eq!(arg_of(&err), Some("ids"), "{err:#}");
        assert_eq!(doc, before);
    }

    #[test]
    fn a_row_failing_validation_mid_batch_leaves_the_sweep_unwritten() {
        let hand_edited = format!("{STORE}\n[[backlog]]\nid = \"B-eeeeeeee\"\nstatus = \"open\"\n");
        let mut doc: TomlValue = toml::from_str(&hand_edited).unwrap();
        let before = doc.clone();
        let err = apply_transition(
            &mut doc,
            &ids(&["B-aaaaaaaa", "B-eeeeeeee"]),
            &Transition::Dismiss("sweep".into()),
            today(),
        )
        .unwrap_err();
        assert_eq!(kind_of(&err), "validation", "{err:#}");
        assert_eq!(doc, before, "the earlier row must not be stored");
    }

    #[test]
    fn a_compacted_only_id_is_not_found() {
        let mut doc = store();
        let before = doc.clone();
        let err = apply_transition(
            &mut doc,
            &ids(&["B-dddddddd"]),
            &Transition::Dismiss("sweep".into()),
            today(),
        )
        .unwrap_err();
        assert_eq!(kind_of(&err), "not_found");
        assert_eq!(arg_of(&err), Some("ids"), "{err:#}");
        assert!(format!("{err:#}").contains("compacted"), "{err:#}");
        assert_eq!(doc, before);
    }

    #[test]
    fn a_bulk_dismiss_applies_to_every_id() {
        let mut doc = store();
        apply_transition(
            &mut doc,
            &ids(&["B-aaaaaaaa", "B-bbbbbbbb", "B-cccccccc"]),
            &Transition::Dismiss("stale capture sweep".into()),
            today(),
        )
        .unwrap();
        for id in ["B-aaaaaaaa", "B-bbbbbbbb", "B-cccccccc"] {
            assert_eq!(
                field(&doc, id, schema::FIELD_STATUS).as_deref(),
                Some("\"dismissed\""),
                "{id}"
            );
            assert_eq!(
                field(&doc, id, schema::FIELD_DISMISSED).as_deref(),
                Some("2026-09-02")
            );
            assert_valid(&doc, id);
        }
    }

    #[test]
    fn a_repeated_id_is_applied_once_and_is_not_a_miss() {
        let mut doc = store();
        apply_transition(
            &mut doc,
            &ids(&["B-aaaaaaaa", "B-aaaaaaaa"]),
            &Transition::Dismiss("sweep".into()),
            today(),
        )
        .unwrap();
        assert_eq!(
            field(&doc, "B-aaaaaaaa", schema::FIELD_STATUS).as_deref(),
            Some("\"dismissed\"")
        );
    }

    #[test]
    fn a_mode_without_its_companion_is_rejected() {
        let cases = [
            (
                TriageMode {
                    promote: true,
                    dismiss: false,
                    resolve: false,
                    reopen: false,
                },
                "--to",
            ),
            (
                TriageMode {
                    promote: false,
                    dismiss: true,
                    resolve: false,
                    reopen: false,
                },
                "--reason",
            ),
            (
                TriageMode {
                    promote: false,
                    dismiss: false,
                    resolve: true,
                    reopen: false,
                },
                "--resolution",
            ),
            (
                TriageMode {
                    promote: false,
                    dismiss: false,
                    resolve: false,
                    reopen: true,
                },
                "--rationale",
            ),
        ];
        for (mode, flag) in cases {
            let err = Transition::from_cli(mode, None, None, None, None).unwrap_err();
            assert_eq!(kind_of(&err), "validation");
            assert!(format!("{err:#}").contains(flag), "{err:#}");
        }
    }

    #[test]
    fn a_blank_companion_counts_as_missing() {
        let err = Transition::from_cli(
            TriageMode {
                promote: false,
                dismiss: true,
                resolve: false,
                reopen: false,
            },
            None,
            Some("   ".into()),
            None,
            None,
        )
        .unwrap_err();
        assert_eq!(kind_of(&err), "validation");
    }

    #[test]
    fn two_mode_flags_conflict_at_the_parser() {
        use clap::Parser;
        let parsed = crate::cli::Cli::try_parse_from([
            "tomlctl",
            "backlog",
            "triage",
            "B-x",
            "--dismiss",
            "--resolve",
            "--reason",
            "r",
            "--resolution",
            "r",
        ]);
        // `Cli` is not `Debug`, so `unwrap_err` is unavailable here.
        let Err(err) = parsed else {
            panic!("two mode flags must not parse");
        };
        assert_eq!(err.kind(), clap::error::ErrorKind::ArgumentConflict);
    }

    /// Seed the store under a throwaway sandbox root and hand back its path.
    fn seed(root: &Path) -> PathBuf {
        let file = root.join(".claude").join("backlog.toml");
        std::fs::write(&file, STORE).unwrap();
        file
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

    fn mode(flag: &str) -> TriageMode {
        TriageMode {
            promote: flag == "promote",
            dismiss: flag == "dismiss",
            resolve: flag == "resolve",
            reopen: flag == "reopen",
        }
    }

    #[test]
    fn a_missing_companion_leaves_the_file_byte_identical() {
        let (err, bytes, sidecar) = with_root(|root| {
            let file = seed(root);
            let err = dispatch(
                ids(&["B-aaaaaaaa"]),
                mode("promote"),
                None,
                None,
                None,
                None,
                false,
                false,
                write_args(),
            )
            .unwrap_err();
            (
                err,
                std::fs::read(&file).unwrap(),
                file.with_extension("toml.sha256").exists(),
            )
        });
        assert_eq!(kind_of(&err), "validation", "{err:#}");
        assert_eq!(bytes, STORE.as_bytes());
        assert!(!sidecar);
    }

    #[test]
    fn an_unknown_id_leaves_the_file_byte_identical() {
        let (err, bytes) = with_root(|root| {
            let file = seed(root);
            let err = dispatch(
                ids(&["B-aaaaaaaa", "B-nosuchid"]),
                mode("dismiss"),
                None,
                Some("sweep".into()),
                None,
                None,
                false,
                false,
                write_args(),
            )
            .unwrap_err();
            (err, std::fs::read(&file).unwrap())
        });
        assert_eq!(kind_of(&err), "not_found", "{err:#}");
        assert_eq!(bytes, STORE.as_bytes());
    }

    #[test]
    fn dispatch_persists_the_transition_through_the_store_path() {
        let (text, sidecar) = with_root(|root| {
            let file = seed(root);
            dispatch(
                ids(&["B-bbbbbbbb"]),
                mode("resolve"),
                None,
                None,
                Some("fixed in 960677b".into()),
                None,
                false,
                false,
                write_args(),
            )
            .unwrap();
            (
                std::fs::read_to_string(&file).unwrap(),
                file.with_file_name("backlog.toml.sha256").exists(),
            )
        });
        let doc: TomlValue = toml::from_str(&text).unwrap();
        assert_eq!(
            field(&doc, "B-bbbbbbbb", schema::FIELD_STATUS).as_deref(),
            Some("\"resolved\"")
        );
        assert_eq!(
            field(&doc, "B-bbbbbbbb", schema::FIELD_RESOLUTION).as_deref(),
            Some("\"fixed in 960677b\"")
        );
        assert_valid(&doc, "B-bbbbbbbb");
        assert!(sidecar);
    }

    fn seed_flow(root: &Path, slug: &str, status: &str) {
        crate::test_support::write(
            root,
            &format!(".claude/flows/{slug}/context.toml"),
            format!("status = \"{status}\"\nplan_path = \"docs/plans/{slug}.md\"\n").as_bytes(),
        );
    }

    /// Promote `B-aaaaaaaa` through `dispatch` and read back its `promoted_to`.
    fn promote(root: &Path, to: &str, external: bool, allow_closed: bool) -> Result<String> {
        dispatch(
            ids(&["B-aaaaaaaa"]),
            mode("promote"),
            Some(to.into()),
            None,
            None,
            None,
            external,
            allow_closed,
            write_args(),
        )?;
        let text = std::fs::read_to_string(root.join(".claude").join("backlog.toml")).unwrap();
        let doc: TomlValue = toml::from_str(&text).unwrap();
        Ok(row(&doc, "B-aaaaaaaa")
            .get(schema::FIELD_PROMOTED_TO)
            .and_then(TomlValue::as_str)
            .unwrap()
            .to_string())
    }

    #[test]
    fn an_unknown_promotion_target_is_not_found_and_writes_nothing() {
        let (err, bytes) = with_root(|root| {
            let file = seed(root);
            let err = promote(root, "task-store-polish", false, false).unwrap_err();
            (err, std::fs::read(&file).unwrap())
        });
        assert_eq!(kind_of(&err), "not_found", "{err:#}");
        assert_eq!(arg_of(&err), Some("to"), "{err:#}");
        assert!(format!("{err:#}").contains("--external"), "{err:#}");
        assert_eq!(bytes, STORE.as_bytes());
    }

    #[test]
    fn external_stores_the_prefixed_value_exactly_once() {
        with_root(|root| {
            seed(root);
            assert_eq!(
                promote(root, "task-store-polish", true, false).unwrap(),
                "external:task-store-polish"
            );
            assert_eq!(
                promote(root, "external:GH-12", true, false).unwrap(),
                "external:GH-12"
            );
        });
    }

    #[test]
    fn a_closed_flow_is_refused_unless_allow_closed() {
        with_root(|root| {
            seed(root);
            seed_flow(root, "parked", "review");
            let err = promote(root, "parked", false, false).unwrap_err();
            assert_eq!(kind_of(&err), "validation", "{err:#}");
            assert_eq!(arg_of(&err), Some("to"), "{err:#}");
            assert!(format!("{err:#}").contains("`review`"), "{err:#}");
            assert_eq!(promote(root, "parked", false, true).unwrap(), "parked");
        });
    }

    #[test]
    fn a_bound_plan_path_is_stored_as_its_flow_slug() {
        with_root(|root| {
            seed(root);
            seed_flow(root, "alpha", "in-progress");
            assert_eq!(
                promote(root, "docs/plans/alpha.md", false, false).unwrap(),
                "alpha"
            );
        });
    }
}
