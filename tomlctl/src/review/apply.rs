//! `review apply` — writes the record edits that open plan findings on a v2
//! flow carry, each through its target store's own write path with `expect`
//! re-checked inside that store's lock.
//!
//! Every id is judged before anything is written: status, kind and tier, the
//! `--user-accepted` gate, `new` against the target store's validator, and a
//! mechanical row's proof. One refusal refuses the call. A value already equal
//! to `new` is decided before the proof is re-verified, since an edit already
//! made no longer satisfies the proof that justified it.
//!
//! Each finding is stamped `resolved` with its `applied` record right after its
//! own target write, so a failure leaves at most one edit unrecorded, and a
//! re-run finds that edit already made and records it `recovered`. A prose
//! edit is marked `applying` on its finding before its write, since a short
//! `new` can occur in a drifted section by chance: without the mark, prose
//! reads as already made only when `expect` is part of `new`.

use std::path::PathBuf;

use anyhow::{Context, Result};
use serde_json::{Value as JsonValue, json};
use toml::Value as TomlValue;

use super::fields::{Family, FindingStatus, Kind};
use super::invalid;
use super::store::{self, Finding, Store, Target};
use super::target::{self, RecordTarget};
use super::tier::{self, Tier};
use crate::cli::{ReadIntegrityArgs, ReviewApplyArgs, WriteIntegrityArgs};
use crate::convert::{decode_json_text, encode_json_text, toml_to_json};
use crate::output::print_json_compact;
use crate::plan::schema::RecordKind;
use crate::plan::{MilestoneEdit, RecordEdit};
use crate::tasks::{ReviewEdit, ReviewEditOutcome, Slot};

const VERB: &str = "review apply";
/// The actor the task and milestone audit rows name.
const ACTOR: &str = "review";
/// The finding key a prose write pass sets before writing and the stamp drops.
const MARK: &str = "applying";

/// One finding judged applicable, with its stored values decoded.
struct Planned {
    id: String,
    run: String,
    tier: Tier,
    target: RecordTarget,
    expect: Option<JsonValue>,
    new: JsonValue,
    proof: Option<serde_json::Map<String, JsonValue>>,
    /// The finding carried the write-ahead mark when loaded.
    marked: bool,
    /// What the target store answered before any write.
    judged: ReviewEditOutcome,
}

impl Planned {
    /// A mechanical row's proof re-checked against the stores as they stand;
    /// any other tier needs none.
    fn proven(&self, slug: &str) -> std::result::Result<(), String> {
        if self.tier != Tier::Mechanical {
            return Ok(());
        }
        let Some(proof) = self.proof.as_ref() else {
            return Err(format!(
                "{} is mechanical tier but carries no `proof`",
                self.id
            ));
        };
        tier::verify_proof(proof, &self.target, self.expect.as_ref(), &self.new, slug)
            .map_err(|why| format!("{}'s proof no longer holds: {why}", self.id))
    }

    /// The record the edit changes, narrowed for a list entry or prose to the
    /// `expect` it replaces, since an edit elsewhere in the same list or
    /// section does not move it. `None` for an append.
    fn record(&self, slug: &str) -> Option<String> {
        let target = &self.target;
        let record = format!(
            "{} {} {}",
            target.file(slug),
            target.symbol(),
            target.field_name()
        );
        match target {
            RecordTarget::SpecEntry {
                slot: Slot::Append, ..
            } => None,
            RecordTarget::SpecEntry { .. }
            | RecordTarget::Brief { .. }
            | RecordTarget::Design { .. } => Some(format!(
                "{record} {}",
                self.expect.as_ref().unwrap_or(&JsonValue::Null)
            )),
            _ => Some(record),
        }
    }
}

#[derive(Default)]
struct Report {
    applied: Vec<JsonValue>,
    already: Vec<String>,
    stale: Vec<JsonValue>,
}

impl Report {
    fn to_json(&self, dry_run: bool) -> JsonValue {
        let mut out = json!({
            "ok": true,
            "applied": self.applied,
            "already": self.already,
            "skipped_stale": self.stale,
        });
        if dry_run {
            out["dry_run"] = json!(true);
        }
        out
    }

    fn take(&mut self, planned: &Planned, outcome: ReviewEditOutcome) {
        match outcome {
            ReviewEditOutcome::Applied => self.applied.push(json!({
                "id": planned.id,
                "target": planned.target.to_string(),
                "tier": planned.tier.as_str(),
            })),
            ReviewEditOutcome::Already => self.already.push(planned.id.clone()),
            ReviewEditOutcome::Stale { found } => self.stale.push(json!({
                "id": planned.id,
                "target": planned.target.to_string(),
                "expected": planned.expect.clone().unwrap_or(JsonValue::Null),
                "found": found,
            })),
        }
    }
}

/// Whether an edit is only judged against its store or written to it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Pass {
    Validate,
    Write,
}

pub(crate) fn run(target: &Target, args: ReviewApplyArgs) -> Result<()> {
    print_json_compact(&execute(target, &args)?)
}

fn execute(target: &Target, args: &ReviewApplyArgs) -> Result<JsonValue> {
    let Some(slug) = args.store.slug.as_deref() else {
        return Err(invalid(format!(
            "{VERB}: findings edit a v2 flow's records, so name the flow with --slug; --scope names a flow-less store"
        )));
    };
    let mut ids: Vec<String> = Vec::new();
    for id in args.ids.iter().map(|i| i.trim()).filter(|i| !i.is_empty()) {
        if !ids.iter().any(|seen| seen == id) {
            ids.push(id.to_string());
        }
    }
    if ids.is_empty() {
        return Err(invalid(format!("{VERB}: no finding ids given")));
    }
    let Some(plan_path) = super::add::v2_plan(slug)? else {
        return Err(invalid(format!(
            "{VERB}: flow `{slug}` has no plan.toml, so its findings carry no record edits"
        )));
    };
    let read = ReadIntegrityArgs {
        verify_integrity: args.integrity.verify_integrity,
        strict_read: true,
    };
    let approved = !crate::plan::store::load(&plan_path, &read)?
        .approvals
        .is_empty();
    let review = store::load(&target.path, &read)?;

    let mut planned = Vec::new();
    let mut refused = Vec::new();
    for id in &ids {
        match judge(&review, id, slug, args, approved) {
            Ok(row) => planned.push(row),
            Err(reasons) => refused.push(json!({"id": id, "reasons": reasons})),
        }
    }
    if !refused.is_empty() {
        let lines: Vec<String> = refused.iter().map(JsonValue::to_string).collect();
        return Err(invalid(format!(
            "{VERB}: {} of {} ids refused; nothing was written:\n{}",
            refused.len(),
            ids.len(),
            lines.join("\n")
        )));
    }

    let mut report = Report::default();
    if args.dry_run {
        // Every row was judged against the untouched stores, so a row whose
        // record an earlier applied row edits is reported stale, as the real
        // run would find it, rather than applied.
        let mut edited: Vec<(String, &str)> = Vec::new();
        for row in &planned {
            let record = row.record(slug);
            let earlier = record
                .as_ref()
                .and_then(|record| edited.iter().find(|(seen, _)| seen == record))
                .map(|(_, id)| *id);
            match (earlier, &row.judged) {
                (Some(by), ReviewEditOutcome::Applied | ReviewEditOutcome::Already) => {
                    report.stale.push(json!({
                        "id": row.id,
                        "target": row.target.to_string(),
                        "expected": row.expect.clone().unwrap_or(JsonValue::Null),
                        "found": JsonValue::Null,
                        "edited_by": by,
                    }));
                }
                (_, judged) => {
                    if let (ReviewEditOutcome::Applied, Some(record)) = (judged, record) {
                        edited.push((record, &row.id));
                    }
                    report.take(row, judged.clone());
                }
            }
        }
        return Ok(report.to_json(true));
    }
    let by = if args.user_accepted { "user" } else { "tool" };
    let review_integrity = WriteIntegrityArgs {
        no_create: true,
        ..args.integrity.clone()
    };
    for row in &planned {
        let interrupted = |report: &Report| {
            let done: Vec<&str> = report
                .applied
                .iter()
                .filter_map(|a| a["id"].as_str())
                .chain(report.already.iter().map(String::as_str))
                .collect();
            format!(
                "{VERB}: applying {id} failed; stamped before it: [{}]. {id}'s edit may or \
                 may not have landed: re-run `{VERB}` with only the ids not listed as \
                 stamped, and an edit that landed reads as already applied",
                done.join(", "),
                id = row.id,
            )
        };
        let reason = format!("review {} {}", row.run, row.id);
        // An earlier row can have moved this one's value, so its outcome is
        // the write pass's, and a mechanical row the judge did not prove gets
        // its proof checked before that pass can write.
        if row.judged != ReviewEditOutcome::Applied && row.tier == Tier::Mechanical {
            let now = edit(slug, &args.integrity, row, &reason, Pass::Validate)
                .with_context(|| interrupted(&report))?;
            if now == ReviewEditOutcome::Applied {
                row.proven(slug)
                    .map_err(invalid)
                    .with_context(|| interrupted(&report))?;
            }
        }
        let prose = is_prose(&row.target);
        if prose && !row.marked {
            mark(target, &review_integrity, &row.id, true).with_context(|| interrupted(&report))?;
        }
        let outcome = edit(slug, &args.integrity, row, &reason, Pass::Write)
            .with_context(|| interrupted(&report))?;
        match outcome {
            ReviewEditOutcome::Applied => stamp(target, &review_integrity, row, by, false),
            ReviewEditOutcome::Already => stamp(target, &review_integrity, row, by, true),
            // Nothing landed, so the mark would vouch for a later coincidence.
            ReviewEditOutcome::Stale { .. } if prose => {
                mark(target, &review_integrity, &row.id, false)
            }
            ReviewEditOutcome::Stale { .. } => Ok(()),
        }
        .with_context(|| interrupted(&report))?;
        report.take(row, outcome);
    }
    Ok(report.to_json(false))
}

/// Every reason `id` cannot be applied, or the row decoded with the outcome its
/// target store gives the edit as it stands.
fn judge(
    review: &Store,
    id: &str,
    slug: &str,
    args: &ReviewApplyArgs,
    approved: bool,
) -> std::result::Result<Planned, Vec<String>> {
    let Some(finding) = review.findings.iter().find(|f| f.id == id) else {
        return Err(vec![format!("{id} is not in this store")]);
    };
    let mut reasons = Vec::new();
    if finding.family != Family::Plan {
        reasons.push(format!(
            "{id} is a {} finding; only plan findings carry record edits",
            finding.family.as_str()
        ));
    }
    if !matches!(finding.kind, Kind::Defect | Kind::Cleanup) {
        reasons.push(format!(
            "{id} is {}; only defect and cleanup findings are applied",
            finding.kind.as_str()
        ));
    }
    let status = finding.status();
    if status != FindingStatus::Open {
        reasons.push(format!(
            "{id} is {}; only an open finding is applied",
            status.as_str()
        ));
    }
    let new = decoded(finding, "new");
    // An advisory-tier defect or cleanup that carries an edit is the user's to
    // accept; the kind check above still refuses every other kind.
    let acceptable_edit = new.is_some() && matches!(finding.kind, Kind::Defect | Kind::Cleanup);
    let tier = match finding.text("tier") {
        None => {
            reasons.push(format!(
                "{id} carries no `tier`; a flow-mode finding gets one when it settles open"
            ));
            None
        }
        Some(raw) => match Tier::parse(raw) {
            Some(Tier::Advisory) if acceptable_edit && args.user_accepted => Some(Tier::Advisory),
            Some(Tier::Advisory) if acceptable_edit => {
                reasons.push(format!(
                    "{id} is advisory tier; apply its edit with --user-accepted once the user \
                     accepts it"
                ));
                None
            }
            Some(Tier::Advisory) => {
                reasons.push(format!("{id} is advisory tier, which is never applied"));
                None
            }
            Some(Tier::Question) => {
                reasons.push(format!(
                    "{id} is question tier, which carries no edit to apply; the user answers \
                     it: revise the record through `plan` verbs, then close it with \
                     `review triage --resolve {id}`, or keep the record and resolve it the same way"
                ));
                None
            }
            Some(tier) => Some(tier),
            None => {
                reasons.push(format!("{id} carries an unknown tier `{raw}`"));
                None
            }
        },
    };
    if tier == Some(Tier::User) && !args.user_accepted {
        reasons.push(format!(
            "{id} is user tier; apply it with --user-accepted once the user accepts it"
        ));
    }
    if approved && !args.user_accepted {
        reasons.push(format!(
            "the plan is approved, so {id} applies only with --user-accepted"
        ));
    }
    let target = match finding.text("target").filter(|raw| !raw.is_empty()) {
        None => {
            reasons.push(format!("{id} carries no `target`"));
            None
        }
        Some(raw) => match target::parse(raw) {
            Ok(RecordTarget::Handover { .. }) => {
                reasons.push(format!("{id} targets a handover, which no verb writes yet"));
                None
            }
            Ok(parsed) => Some(parsed),
            Err(err) => {
                reasons.push(format!("{id}: {err:#}"));
                None
            }
        },
    };
    if new.is_none() {
        reasons.push(format!("{id} carries no `new`, so it proposes no edit"));
    }
    let (Some(tier), Some(target), Some(new), true) = (tier, target, new, reasons.is_empty())
    else {
        return Err(reasons);
    };
    let mut row = Planned {
        id: id.to_string(),
        run: finding.run.clone(),
        tier,
        target,
        expect: decoded(finding, "expect"),
        new,
        proof: finding
            .rest
            .get("proof")
            .and_then(|proof| match toml_to_json(proof) {
                JsonValue::Object(map) => Some(map),
                _ => None,
            }),
        marked: finding.rest.get(MARK).and_then(TomlValue::as_bool) == Some(true),
        judged: ReviewEditOutcome::Applied,
    };
    let reason = format!("review {} {}", row.run, row.id);
    row.judged = edit(slug, &args.integrity, &row, &reason, Pass::Validate)
        .map_err(|err| vec![format!("{id}: {err:#}")])?;
    if row.judged == ReviewEditOutcome::Applied {
        row.proven(slug).map_err(|why| vec![why])?;
    }
    Ok(row)
}

/// The stored `expect` or `new` decoded, `None` when the finding carries none.
fn decoded(finding: &Finding, key: &str) -> Option<JsonValue> {
    finding
        .rest
        .get(key)
        .map(|value| decode_json_text(Some(value)))
}

/// Runs `row`'s edit through its target store: judged only under
/// [`Pass::Validate`], written with `expect` re-checked under the lock under
/// [`Pass::Write`].
fn edit(
    slug: &str,
    integrity: &WriteIntegrityArgs,
    row: &Planned,
    reason: &str,
    pass: Pass,
) -> Result<ReviewEditOutcome> {
    target_edit(
        slug,
        integrity,
        &row.target,
        row.expect.as_ref(),
        &row.new,
        row.marked,
        reason,
        pass,
    )
}

fn is_prose(target: &RecordTarget) -> bool {
    matches!(
        target,
        RecordTarget::Brief { .. } | RecordTarget::Design { .. }
    )
}

/// What `review apply` would judge an edit of `target` from `expect` to
/// `new` against the stores as they stand, writing nothing, for a finding
/// carrying no write-ahead mark.
pub(super) fn validate_target_edit(
    slug: &str,
    target: &RecordTarget,
    expect: Option<&JsonValue>,
    new: &JsonValue,
) -> Result<ReviewEditOutcome> {
    let integrity = WriteIntegrityArgs {
        allow_outside: false,
        no_write_integrity: false,
        verify_integrity: false,
        strict_integrity: false,
        no_create: true,
    };
    target_edit(
        slug,
        &integrity,
        target,
        expect,
        new,
        false,
        "",
        Pass::Validate,
    )
}

/// `marked` is whether the finding carries the write-ahead mark; only a
/// prose edit reads it.
#[allow(clippy::too_many_arguments)]
fn target_edit(
    slug: &str,
    integrity: &WriteIntegrityArgs,
    target: &RecordTarget,
    expect: Option<&JsonValue>,
    new: &JsonValue,
    marked: bool,
    reason: &str,
    pass: Pass,
) -> Result<ReviewEditOutcome> {
    let write = pass == Pass::Write;
    match target {
        RecordTarget::TaskField { r#ref, .. }
        | RecordTarget::SpecField { r#ref, .. }
        | RecordTarget::SpecEntry { r#ref, .. } => {
            let entry = match target {
                RecordTarget::SpecEntry { slot, .. } => Some(*slot),
                _ => None,
            };
            let field = target.field_name();
            let edit = ReviewEdit {
                task_ref: r#ref,
                field: &field,
                entry,
                expect,
                new,
                actor: ACTOR,
                reason,
            };
            if write {
                crate::tasks::apply_review_edit(slug, integrity, &edit)
            } else {
                crate::tasks::validate_review_edit(slug, integrity, &edit)
            }
        }
        RecordTarget::Decision { id, key } | RecordTarget::Contract { id, key } => {
            let kind = match target {
                RecordTarget::Decision { .. } => RecordKind::Decision,
                _ => RecordKind::Contract,
            };
            let edit = RecordEdit {
                kind,
                id,
                key,
                expect,
                new,
            };
            if write {
                crate::plan::apply_record_edit(slug, integrity, &edit)
            } else {
                crate::plan::validate_record_edit(slug, integrity, &edit)
            }
        }
        RecordTarget::Milestone { id, field } => {
            let edit = MilestoneEdit {
                id,
                field,
                expect,
                new,
                actor: ACTOR,
                reason,
            };
            if write {
                crate::plan::apply_milestone_edit(slug, integrity, &edit)
            } else {
                crate::plan::validate_milestone_edit(slug, integrity, &edit)
            }
        }
        RecordTarget::Brief { .. } | RecordTarget::Design { .. } => {
            prose(slug, target, expect, new, marked, write)
        }
        RecordTarget::Handover { .. } => Err(invalid(format!(
            "target `{target}`: no verb writes a handover yet"
        ))),
    }
}

/// A brief or design edit: `expect` replaced exactly once inside the section,
/// under the prose file's lock. The file is read as bytes, refused unless
/// UTF-8, and written back with its own line endings.
fn prose(
    slug: &str,
    target: &RecordTarget,
    expect: Option<&JsonValue>,
    new: &JsonValue,
    marked: bool,
    write: bool,
) -> Result<ReviewEditOutcome> {
    let refuse = |why: &str| invalid(format!("target `{target}`: {why}"));
    let expect = expect
        .and_then(JsonValue::as_str)
        .filter(|text| !text.is_empty())
        .ok_or_else(|| refuse("a prose edit needs `expect`, text occurring once in the section"))?;
    let new = new
        .as_str()
        .ok_or_else(|| refuse("a prose edit's `new` is text"))?;
    let path = prose_path(slug, target)?;
    let step = || -> Result<ReviewEditOutcome> {
        let bytes = std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
        let original = String::from_utf8(bytes)
            .map_err(|_| refuse("the file is not UTF-8, so it is not rewritten"))?;
        let (base, section) = match target {
            RecordTarget::Brief { section } => {
                let (at, body) = target::brief_section(&original.replace("\r\n", "\n"), section)?;
                (at, body)
            }
            RecordTarget::Design { anchor, .. } => {
                let (at, body) =
                    crate::plan::check::design_section_in(&original.replace("\r\n", "\n"), anchor)
                        .ok_or_else(|| refuse("names no design section"))?;
                (at, body)
            }
            _ => unreachable!("prose is called only for a brief or design target"),
        };
        let section = section.as_str();
        match place(&original, section, base, expect, new, marked).map_err(|why| refuse(&why))? {
            Placed::Edited(text) => {
                if write {
                    // Resolved again under the lock, so a symlink swapped in
                    // while the lock was awaited is refused, not written through.
                    if prose_path(slug, target)? != path {
                        return Err(refuse("the file's resolved path changed during the edit"));
                    }
                    crate::io::atomic_write(&path, text.as_bytes())?;
                }
                Ok(ReviewEditOutcome::Applied)
            }
            Placed::Already => Ok(ReviewEditOutcome::Already),
            Placed::Stale => Ok(ReviewEditOutcome::Stale {
                found: JsonValue::String(section.to_string()),
            }),
        }
    };
    if write {
        crate::io::with_exclusive_lock(&path, step)
    } else {
        step()
    }
}

/// The prose file `target` names, resolved through symlinks and held inside
/// the flow's authored directory.
fn prose_path(slug: &str, target: &RecordTarget) -> Result<PathBuf> {
    let authored = crate::plan::store::authored_dir(slug)?;
    let refuse = |why: &str| invalid(format!("target `{target}`: {why}"));
    match target {
        RecordTarget::Brief { section } => target::brief_path(&authored, section),
        RecordTarget::Design { file, .. } => crate::plan::check::design_path(&authored, file)
            .ok_or_else(|| refuse("names no design file inside the flow's design/ directory")),
        _ => unreachable!("prose_path is called only for a brief or design target"),
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Placed {
    /// The whole file with the edit made.
    Edited(String),
    Already,
    Stale,
}

/// Places the replacement of `expect` by `new` inside `section`, which is the
/// LF form of a span of `original` starting at byte `base` of that LF form.
/// `new` occurring once with every `expect` inside it reads as already made,
/// and so, when `marked`, does an empty `new` with no `expect` left. Unless
/// `marked` or `expect` is part of `new`, a `new` found with `expect` gone may
/// be chance, so it is not read as made. Otherwise `expect` must occur once:
/// none is stale, several is an error.
fn place(
    original: &str,
    section: &str,
    base: usize,
    expect: &str,
    new: &str,
    marked: bool,
) -> Result<Placed, String> {
    let expect = expect.replace("\r\n", "\n");
    let new = new.replace("\r\n", "\n");
    let hits = |hay: &str, needle: &str| -> Vec<usize> {
        hay.match_indices(needle).map(|(at, _)| at).collect()
    };
    let olds = hits(section, &expect);
    let already = if !marked && !new.contains(&expect) {
        false
    } else if new.is_empty() {
        olds.is_empty()
    } else {
        match hits(section, &new).as_slice() {
            [at] => olds
                .iter()
                .all(|old| *old >= *at && old + expect.len() <= at + new.len()),
            _ => false,
        }
    };
    if already {
        return Ok(Placed::Already);
    }
    let at = match olds.as_slice() {
        [] => return Ok(Placed::Stale),
        [at] => *at,
        many => {
            return Err(format!(
                "`expect` occurs {} times in the section, so the edit is ambiguous",
                many.len()
            ));
        }
    };
    let normal = original.replace("\r\n", "\n");
    if !normal
        .get(base..)
        .is_some_and(|rest| rest.starts_with(section))
    {
        return Err("the section is not in the file as read".to_string());
    }
    let start = original_offset(original, base + at);
    let end = original_offset(original, base + at + expect.len());
    let ending = if original.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    Ok(Placed::Edited(format!(
        "{}{}{}",
        &original[..start],
        new.replace('\n', ending),
        &original[end..]
    )))
}

/// The byte offset in `original` of byte `normal` of its `\r\n`-to-`\n` form.
fn original_offset(original: &str, normal: usize) -> usize {
    let bytes = original.as_bytes();
    let (mut at, mut seen) = (0, 0);
    while seen < normal {
        at += if bytes[at] == b'\r' && bytes.get(at + 1) == Some(&b'\n') {
            2
        } else {
            1
        };
        seen += 1;
    }
    at
}

/// Resolves the finding with its `applied` record under the review store's
/// lock, dropping the write-ahead mark. `old` is `expect`; `recovered` marks
/// an edit found already made.
fn stamp(
    target: &Target,
    integrity: &WriteIntegrityArgs,
    row: &Planned,
    by: &str,
    recovered: bool,
) -> Result<()> {
    let today = crate::time::today_toml_date()?;
    let old = row.expect.clone().unwrap_or(JsonValue::Null);
    let mut applied = toml::map::Map::new();
    let mut put = |key: &str, value: TomlValue| {
        applied.insert(key.to_string(), value);
    };
    put("at", TomlValue::String(crate::time::now_rfc3339()));
    put("tier", TomlValue::String(row.tier.as_str().to_string()));
    put("target", TomlValue::String(row.target.to_string()));
    put("old", TomlValue::String(encode_json_text(&old)));
    put("new", TomlValue::String(encode_json_text(&row.new)));
    put("by", TomlValue::String(by.to_string()));
    put("recovered", TomlValue::Boolean(recovered));
    let resolution = format!("applied to {}", row.target);
    store::mutate(&target.path, integrity, |store| {
        let finding = store
            .findings
            .iter_mut()
            .find(|f| f.id == row.id)
            .ok_or_else(|| invalid(format!("{VERB}: {} left the store mid-call", row.id)))?;
        finding.set_status(
            FindingStatus::Resolved,
            &[
                ("resolution", TomlValue::String(resolution)),
                ("resolved", TomlValue::Datetime(today)),
            ],
        );
        finding
            .rest
            .insert("applied".to_string(), TomlValue::Table(applied));
        finding.rest.remove(MARK);
        Ok(())
    })
}

/// Sets or drops the write-ahead mark on finding `id` under the review
/// store's lock.
fn mark(target: &Target, integrity: &WriteIntegrityArgs, id: &str, on: bool) -> Result<()> {
    store::mutate(&target.path, integrity, |store| {
        let finding = store
            .findings
            .iter_mut()
            .find(|f| f.id == id)
            .ok_or_else(|| invalid(format!("{VERB}: {id} left the store mid-call")))?;
        if on {
            finding
                .rest
                .insert(MARK.to_string(), TomlValue::Boolean(true));
        } else {
            finding.rest.remove(MARK);
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::ReviewStore;
    use crate::plan::store::BRIEF_FILE;
    use crate::test_support::with_root;
    use std::path::Path;

    const SLUG: &str = "applying";

    const TASKS: &str = r#"
schema_version = 1

[[specs]]
ref = "parse-it"
outcome = "targets parse"
executor = "deep"

[[items]]
id = 1
ref = "parse-it"
title = "Parse it"
effort = "S"
status = "pending"
files = ["src/a.rs", "src/gone.rs"]
action = "parse"
"#;

    const PLAN: &str = r#"
schema_version = 1

[[decision]]
id = "D1"
decision = "one carrier"
why = "fewer moving parts"
provenance = "user-stated"
door = "two-way"
"#;

    const BRIEF: &str = "# Brief\r\n\r\n## Intent\r\n\r\nShip it.\r\n\r\n## Background\r\n\r\nThe old parser\r\nis slow.\r\n";

    /// F1 mechanical path fix, F2 user-tier decision, F3 cited brief edit,
    /// F4 cited edit whose `expect` has drifted.
    const REVIEW: &str = r#"
[[findings]]
id = "F1"
run = "U1"
family = "plan"
kind = "defect"
status = "open"
tier = "mechanical"
target = "task:parse-it.files"
expect = '["src/a.rs","src/gone.rs"]'
new = '["src/a.rs"]'
proof = { kind = "path-missing", path = "src/gone.rs" }

[[findings]]
id = "F2"
run = "U1"
family = "plan"
kind = "defect"
status = "open"
tier = "user"
target = "decision:D1.why"
expect = '"fewer moving parts"'
new = '"one place to change"'

[[findings]]
id = "F3"
run = "U1"
family = "plan"
kind = "cleanup"
status = "open"
tier = "cited"
target = "brief#Background"
expect = '"The old parser\nis slow."'
new = '"The old parser\nis slow on large plans."'

[[findings]]
id = "F4"
run = "U1"
family = "plan"
kind = "defect"
status = "open"
tier = "cited"
target = "task:parse-it.action"
expect = '"tokenise"'
new = '"parse and tokenise"'
"#;

    fn seed(root: &Path) {
        let flow = root.join(".claude").join("flows").join(SLUG);
        std::fs::create_dir_all(&flow).unwrap();
        std::fs::write(flow.join("tasks.toml"), TASKS).unwrap();
        std::fs::write(flow.join("plan.toml"), PLAN).unwrap();
        std::fs::write(flow.join("review-v2.toml"), REVIEW).unwrap();
        let authored = root.join("docs").join("flows").join(SLUG);
        std::fs::create_dir_all(&authored).unwrap();
        std::fs::write(authored.join(BRIEF_FILE), BRIEF).unwrap();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src").join("a.rs"), "fn a() {}\n").unwrap();
    }

    fn args(ids: &[&str], user_accepted: bool, dry_run: bool) -> ReviewApplyArgs {
        ReviewApplyArgs {
            ids: ids.iter().map(|id| (*id).to_string()).collect(),
            store: ReviewStore {
                slug: Some(SLUG.to_string()),
                scope: None,
            },
            user_accepted,
            dry_run,
            integrity: WriteIntegrityArgs {
                allow_outside: false,
                no_write_integrity: false,
                verify_integrity: false,
                strict_integrity: false,
                no_create: false,
            },
        }
    }

    fn apply(ids: &[&str], user_accepted: bool, dry_run: bool) -> Result<JsonValue> {
        let target = store::resolve(Some(SLUG), None)?;
        execute(&target, &args(ids, user_accepted, dry_run))
    }

    fn finding(id: &str) -> Finding {
        let target = store::resolve(Some(SLUG), None).unwrap();
        let read = ReadIntegrityArgs {
            verify_integrity: false,
            strict_read: true,
        };
        store::load(&target.path, &read)
            .unwrap()
            .findings
            .into_iter()
            .find(|f| f.id == id)
            .unwrap()
    }

    fn task_files(root: &Path) -> String {
        std::fs::read_to_string(root.join(".claude/flows").join(SLUG).join("tasks.toml")).unwrap()
    }

    #[test]
    fn one_refusal_refuses_the_call_and_writes_nothing() {
        with_root(|root| {
            seed(root);
            let before = (task_files(root), finding("F1"));
            let err = format!("{:#}", apply(&["F1", "F2"], false, false).unwrap_err());
            assert!(err.contains("1 of 2 ids refused"), "{err}");
            assert!(err.contains("F2 is user tier"), "{err}");
            assert_eq!((task_files(root), finding("F1")), before);
        });
    }

    #[test]
    fn a_mechanical_fix_is_written_and_its_finding_stamped() {
        with_root(|root| {
            seed(root);
            let out = apply(&["F1"], false, false).unwrap();
            assert_eq!(
                out["applied"],
                json!([{"id": "F1", "target": "task:parse-it.files", "tier": "mechanical"}])
            );
            let tasks = crate::tasks::load_store(
                &crate::tasks::resolve_store_path(Some(SLUG), None).unwrap(),
                &ReadIntegrityArgs {
                    verify_integrity: false,
                    strict_read: true,
                },
            )
            .unwrap();
            assert_eq!(tasks.find_ref("parse-it").unwrap().files, ["src/a.rs"]);
            let f1 = finding("F1");
            assert_eq!(f1.status(), FindingStatus::Resolved);
            assert_eq!(
                f1.text("resolution"),
                Some("applied to task:parse-it.files")
            );
            let applied = f1.rest["applied"].as_table().unwrap();
            assert_eq!(applied["by"].as_str(), Some("tool"));
            assert_eq!(applied["recovered"].as_bool(), Some(false));
            assert_eq!(
                applied["old"].as_str(),
                Some(r#"["src/a.rs","src/gone.rs"]"#)
            );
            let err = format!("{:#}", apply(&["F1"], false, false).unwrap_err());
            assert!(err.contains("F1 is resolved"), "{err}");
        });
    }

    #[test]
    fn an_edit_already_made_is_recovered_without_its_proof() {
        with_root(|root| {
            seed(root);
            let tasks = root.join(".claude/flows").join(SLUG).join("tasks.toml");
            std::fs::write(
                &tasks,
                TASKS.replace(r#"["src/a.rs", "src/gone.rs"]"#, r#"["src/a.rs"]"#),
            )
            .unwrap();
            let out = apply(&["F1"], false, false).unwrap();
            assert_eq!(out["already"], json!(["F1"]));
            assert_eq!(out["applied"], json!([]));
            let applied = finding("F1").rest["applied"].as_table().unwrap().clone();
            assert_eq!(applied["recovered"].as_bool(), Some(true));
            assert_eq!(
                applied["old"].as_str(),
                Some(r#"["src/a.rs","src/gone.rs"]"#)
            );
        });
    }

    #[test]
    fn a_drifted_expect_is_skipped_and_the_rest_proceed() {
        with_root(|root| {
            seed(root);
            let out = apply(&["F4", "F3"], false, false).unwrap();
            assert_eq!(out["skipped_stale"][0]["id"], "F4");
            assert_eq!(out["skipped_stale"][0]["found"], "parse");
            assert_eq!(out["applied"][0]["id"], "F3");
            assert!(!finding("F3").rest.contains_key(MARK));
            assert_eq!(finding("F4").status(), FindingStatus::Open);
            let brief =
                std::fs::read_to_string(root.join("docs/flows").join(SLUG).join(BRIEF_FILE))
                    .unwrap();
            assert_eq!(
                brief,
                BRIEF.replace("is slow.", "is slow on large plans."),
                "the edit lands with the file's CRLF endings kept"
            );
        });
    }

    #[test]
    fn a_dry_run_writes_nothing_and_user_acceptance_is_recorded() {
        with_root(|root| {
            seed(root);
            let before =
                std::fs::read_to_string(root.join(".claude/flows").join(SLUG).join("plan.toml"))
                    .unwrap();
            let out = apply(&["F2"], true, true).unwrap();
            assert_eq!(out["dry_run"], true);
            assert_eq!(out["applied"][0]["tier"], "user");
            assert_eq!(finding("F2").status(), FindingStatus::Open);
            let after =
                std::fs::read_to_string(root.join(".claude/flows").join(SLUG).join("plan.toml"))
                    .unwrap();
            assert_eq!(before, after);

            apply(&["F2"], true, false).unwrap();
            let f2 = finding("F2");
            assert_eq!(f2.rest["applied"]["by"].as_str(), Some("user"));
        });
    }

    #[test]
    fn placement_keeps_line_endings_and_reads_repeats() {
        let original = "## A\r\n\r\nold line\r\nnext\r\n";
        let section = "\nold line\nnext\n";
        let base = "## A\n".len();
        assert_eq!(
            place(
                original,
                section,
                base,
                "old line\nnext",
                "new\nlines",
                false
            )
            .unwrap(),
            Placed::Edited("## A\r\n\r\nnew\r\nlines\r\n".to_string())
        );
        assert_eq!(
            place(original, section, base, "old", "old line", false).unwrap(),
            Placed::Already
        );
        assert_eq!(
            place(original, section, base, "gone", "fresh", false).unwrap(),
            Placed::Stale
        );
        let err = place(original, section, 0, "old line", "new", false).unwrap_err();
        assert!(err.contains("not in the file as read"), "{err}");
        let err = place("a a", "a a", 0, "a", "b", false).unwrap_err();
        assert!(err.contains("2 times"), "{err}");
    }

    #[test]
    fn a_new_found_by_chance_is_already_made_only_under_the_mark() {
        let original = "## A\n\nold line\nnext\n";
        let section = "\nold line\nnext\n";
        let base = "## A\n".len();
        // `x` occurs once, inside `next`, and `expect` is gone.
        assert_eq!(
            place(original, section, base, "gone", "x", false).unwrap(),
            Placed::Stale
        );
        assert_eq!(
            place(original, section, base, "gone", "x", true).unwrap(),
            Placed::Already
        );
        assert_eq!(
            place(original, section, base, "gone", "", false).unwrap(),
            Placed::Stale
        );
        assert_eq!(
            place(original, section, base, "gone", "", true).unwrap(),
            Placed::Already
        );
    }

    /// F3 rewritten so its `expect` is not part of its `new`, over a brief
    /// whose section holds `new` by chance and `expect` no longer.
    fn seed_coincidence(root: &Path, marked: bool) {
        seed(root);
        let path = root.join("docs/flows").join(SLUG).join(BRIEF_FILE);
        std::fs::write(&path, BRIEF.replace("is slow.", "drags on.")).unwrap();
        let mut review = REVIEW
            .replace(r#"'"The old parser\nis slow."'"#, r#"'"is slow."'"#)
            .replace(
                r#"'"The old parser\nis slow on large plans."'"#,
                r#"'"drags"'"#,
            );
        if marked {
            review = review.replace(
                "new = '\"drags\"'\n",
                "new = '\"drags\"'\napplying = true\n",
            );
        }
        let target = store::resolve(Some(SLUG), None).unwrap();
        std::fs::write(&target.path, review).unwrap();
    }

    #[test]
    fn a_prose_new_present_by_chance_without_the_mark_is_stale() {
        with_root(|root| {
            seed_coincidence(root, false);
            let out = apply(&["F3"], false, false).unwrap();
            assert_eq!(out["skipped_stale"][0]["id"], "F3", "{out}");
            assert_eq!(out["already"], json!([]));
            let f3 = finding("F3");
            assert_eq!(f3.status(), FindingStatus::Open);
            assert!(!f3.rest.contains_key(MARK), "a stale write drops its mark");
            assert!(!f3.rest.contains_key("applied"));
            let brief =
                std::fs::read_to_string(root.join("docs/flows").join(SLUG).join(BRIEF_FILE))
                    .unwrap();
            assert_eq!(brief, BRIEF.replace("is slow.", "drags on."));
        });
    }

    #[test]
    fn a_marked_prose_edit_already_written_is_recovered() {
        with_root(|root| {
            seed_coincidence(root, true);
            let out = apply(&["F3"], false, false).unwrap();
            assert_eq!(out["already"], json!(["F3"]), "{out}");
            let f3 = finding("F3");
            assert_eq!(f3.status(), FindingStatus::Resolved);
            assert_eq!(
                f3.rest["applied"]["recovered"].as_bool(),
                Some(true),
                "{f3:?}"
            );
            assert!(!f3.rest.contains_key(MARK), "the stamp drops the mark");
        });
    }

    #[test]
    fn a_prose_edit_whose_new_holds_expect_is_recovered_without_the_mark() {
        with_root(|root| {
            seed(root);
            let path = root.join("docs/flows").join(SLUG).join(BRIEF_FILE);
            std::fs::write(&path, BRIEF.replace("is slow.", "is slow on large plans.")).unwrap();
            let target = store::resolve(Some(SLUG), None).unwrap();
            let review = REVIEW
                .replace(r#"'"The old parser\nis slow."'"#, r#"'"is slow"'"#)
                .replace(
                    r#"'"The old parser\nis slow on large plans."'"#,
                    r#"'"is slow on large plans"'"#,
                );
            std::fs::write(&target.path, review).unwrap();
            let out = apply(&["F3"], false, false).unwrap();
            assert_eq!(out["already"], json!(["F3"]), "{out}");
            let f3 = finding("F3");
            assert_eq!(f3.rest["applied"]["recovered"].as_bool(), Some(true));
            assert!(!f3.rest.contains_key(MARK));
        });
    }

    #[test]
    fn a_section_whose_body_repeats_elsewhere_is_placed_at_its_offset() {
        with_root(|root| {
            seed(root);
            let brief = "# Brief\r\n\r\n## Intent\r\n\r\nNone.\r\n\r\n## Unknowns\r\n\r\nNone.\r\n";
            let path = root.join("docs/flows").join(SLUG).join(BRIEF_FILE);
            std::fs::write(&path, brief).unwrap();
            let target = store::resolve(Some(SLUG), None).unwrap();
            let review = REVIEW
                .replace("brief#Background", "brief#Unknowns")
                .replace(r#"'"The old parser\nis slow."'"#, r#"'"None."'"#)
                .replace(
                    r#"'"The old parser\nis slow on large plans."'"#,
                    r#"'"Whether the cache holds."'"#,
                );
            std::fs::write(&target.path, review).unwrap();
            let out = apply(&["F3"], false, false).unwrap();
            assert_eq!(out["applied"][0]["id"], "F3", "{out}");
            assert_eq!(
                std::fs::read_to_string(&path).unwrap(),
                brief.replace(
                    "## Unknowns\r\n\r\nNone.",
                    "## Unknowns\r\n\r\nWhether the cache holds."
                )
            );
        });
    }

    #[test]
    fn a_mechanical_row_without_a_proof_is_refused() {
        with_root(|root| {
            seed(root);
            let target = store::resolve(Some(SLUG), None).unwrap();
            let review = REVIEW.replace(
                "proof = { kind = \"path-missing\", path = \"src/gone.rs\" }\n",
                "",
            );
            std::fs::write(&target.path, review).unwrap();
            let before = task_files(root);
            let err = format!("{:#}", apply(&["F1"], false, false).unwrap_err());
            assert!(
                err.contains("F1 is mechanical tier but carries no `proof`"),
                "{err}"
            );
            assert_eq!(task_files(root), before);
        });
    }
}
