//! `backlog reconcile` — joins every `promoted` row to the tasks that close it
//! in its target flow's store, and buckets the row by how far that work has
//! got. It judges by task-row status; the flow's own `status` only decides
//! whether the flow is closed.
//!
//! `--adopt` links a row no task links yet to every task whose prose names
//! its id, and `--apply` resolves the rows whose closing tasks are all done.
//! With neither flag the verb writes nothing.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::Result;
use serde_json::{Value as JsonValue, json};
use toml::Value as TomlValue;

use super::schema::{self, FIELD_LAST_UPDATED};
use super::target::{Resolver, Target};
use super::triage::resolve_with_link;
use crate::cli::{ReadIntegrityArgs, WriteIntegrityArgs, write_integrity_opts};
use crate::io::{
    item_id, items_array, items_array_mut, mutate_doc_conditional, on_missing_for,
    repo_or_cwd_root, warn_if_created,
};
use crate::output::print_json_compact;
use crate::tasks::{BacklogLink, Status, Store, load_store, mutate_store, resolve_store_path};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Bucket {
    Ready,
    InProgress,
    Stalled,
    Unlinked,
    Orphaned,
    Dangling,
    External,
}

impl Bucket {
    const ALL: [Bucket; 7] = [
        Bucket::Ready,
        Bucket::InProgress,
        Bucket::Stalled,
        Bucket::Unlinked,
        Bucket::Orphaned,
        Bucket::Dangling,
        Bucket::External,
    ];

    fn name(self) -> &'static str {
        match self {
            Bucket::Ready => "ready",
            Bucket::InProgress => "in-progress",
            Bucket::Stalled => "stalled",
            Bucket::Unlinked => "unlinked",
            Bucket::Orphaned => "orphaned",
            Bucket::Dangling => "dangling",
            Bucket::External => "external",
        }
    }
}

/// One live `promoted` row and what its `promoted_to` resolves to.
struct Claim {
    id: String,
    summary: String,
    promoted_to: String,
    target: Target,
}

struct Entry {
    bucket: Bucket,
    id: String,
    summary: String,
    /// The `promoted_to` read, which `--apply` requires to be unchanged.
    promoted_to: String,
    target: String,
    flow: Option<String>,
    flow_status: Option<String>,
    closes: Vec<String>,
    refs: Vec<String>,
    commits: Vec<String>,
    reason: String,
}

impl Entry {
    fn to_json(&self) -> JsonValue {
        json!({
            "id": self.id,
            "summary": self.summary,
            "target": self.target,
            "flow_status": self.flow_status,
            "closes": self.closes,
            "refs": self.refs,
            "reason": self.reason,
        })
    }
}

struct Adoption {
    id: String,
    flow: String,
    task_refs: Vec<String>,
}

struct Survey {
    entries: Vec<Entry>,
    adopted: Vec<Adoption>,
}

#[derive(Default)]
struct Applied {
    applied: Vec<String>,
    skipped: Vec<(String, String)>,
}

pub(crate) fn dispatch(
    flow: Option<String>,
    apply: bool,
    adopt: bool,
    integrity: WriteIntegrityArgs,
) -> Result<()> {
    let today = crate::time::today_toml_date()?;
    let report = reconcile(flow.as_deref(), apply, adopt, &integrity, today)?;
    print_json_compact(&report)
}

fn reconcile(
    flow: Option<&str>,
    apply: bool,
    adopt: bool,
    integrity: &WriteIntegrityArgs,
    today: toml::value::Datetime,
) -> Result<JsonValue> {
    let survey = survey(flow, adopt, integrity)?;
    let outcome = match apply {
        true => apply_ready(&survey.entries, integrity, today)?,
        false => Applied::default(),
    };

    let mut buckets = serde_json::Map::new();
    for bucket in Bucket::ALL {
        let entries: Vec<JsonValue> = survey
            .entries
            .iter()
            .filter(|entry| entry.bucket == bucket)
            .map(Entry::to_json)
            .collect();
        buckets.insert(bucket.name().to_string(), JsonValue::Array(entries));
    }
    let render_needed: BTreeSet<&str> = survey
        .adopted
        .iter()
        .map(|adoption| adoption.flow.as_str())
        .collect();
    Ok(json!({
        "ok": true,
        "flow": flow,
        "buckets": buckets,
        "adopted": survey
            .adopted
            .iter()
            .map(|a| json!({"id": a.id, "flow": a.flow, "task_refs": a.task_refs}))
            .collect::<Vec<_>>(),
        "applied": outcome.applied,
        "skipped": outcome
            .skipped
            .iter()
            .map(|(id, reason)| json!({"id": id, "reason": reason}))
            .collect::<Vec<_>>(),
        "render_needed": render_needed,
    }))
}

fn survey(flow: Option<&str>, adopt: bool, integrity: &WriteIntegrityArgs) -> Result<Survey> {
    let read_args = ReadIntegrityArgs {
        verify_integrity: integrity.verify_integrity,
        strict_read: false,
    };
    let backlog = schema::read_store(&read_args)?;
    let resolver = Resolver::new(&repo_or_cwd_root()?)?;
    let claims = promoted_claims(&backlog, &resolver, flow);

    let mut stores: BTreeMap<String, Store> = BTreeMap::new();
    for claim in &claims {
        if let Target::Flow { slug, .. } = &claim.target
            && !stores.contains_key(slug)
        {
            stores.insert(slug.clone(), load_flow_store(slug, &read_args)?);
        }
    }
    let adopted = match adopt {
        true => adopt_links(&claims, &mut stores, integrity)?,
        false => Vec::new(),
    };
    let entries = claims
        .iter()
        .map(|claim| classify(claim, &stores))
        .collect();
    Ok(Survey { entries, adopted })
}

fn promoted_claims(backlog: &TomlValue, resolver: &Resolver, flow: Option<&str>) -> Vec<Claim> {
    items_array(backlog, schema::ARRAY_BACKLOG)
        .iter()
        .filter(|row| str_field(row, schema::FIELD_STATUS) == schema::STATUS_PROMOTED)
        .filter_map(|row| {
            let id = item_id(row).filter(|id| !id.is_empty())?;
            let promoted_to = str_field(row, schema::FIELD_PROMOTED_TO);
            Some(Claim {
                id: id.to_string(),
                summary: str_field(row, schema::FIELD_SUMMARY).to_string(),
                promoted_to: promoted_to.to_string(),
                target: resolver.resolve(promoted_to),
            })
        })
        .filter(|claim| {
            flow.is_none_or(
                |want| matches!(&claim.target, Target::Flow { slug, .. } if slug == want),
            )
        })
        .collect()
}

/// `load` errors on a missing file, and a flow with no store has no links.
fn load_flow_store(slug: &str, read_args: &ReadIntegrityArgs) -> Result<Store> {
    let path = resolve_store_path(Some(slug), None)?;
    if !path.exists() {
        return Ok(Store::default());
    }
    load_store(&path, read_args)
}

/// Link each claim no task links yet to the tasks naming its id, one locked
/// write per flow. The matches are recomputed against the store as re-read
/// under the lock, and the cached store is replaced with what was written.
fn adopt_links(
    claims: &[Claim],
    stores: &mut BTreeMap<String, Store>,
    integrity: &WriteIntegrityArgs,
) -> Result<Vec<Adoption>> {
    let mut wanted: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for claim in claims {
        let Target::Flow { slug, .. } = &claim.target else {
            continue;
        };
        let Some(store) = stores.get(slug) else {
            continue;
        };
        if !is_linked(store, &claim.id) && !mentions(store, &claim.id).is_empty() {
            wanted.entry(slug.as_str()).or_default().push(&claim.id);
        }
    }

    let mut adopted = Vec::new();
    for (slug, ids) in wanted {
        let path = resolve_store_path(Some(slug), None)?;
        let (made, store) = mutate_store(&path, integrity, |store| {
            let mut made: Vec<(String, Vec<String>)> = Vec::new();
            for id in &ids {
                if is_linked(store, id) {
                    continue;
                }
                let refs = mentions(store, id);
                for r#ref in &refs {
                    add_close(store, r#ref, id);
                }
                if !refs.is_empty() {
                    made.push(((*id).to_string(), refs));
                }
            }
            Ok((made, store.clone()))
        })?;
        stores.insert(slug.to_string(), store);
        adopted.extend(made.into_iter().map(|(id, task_refs)| Adoption {
            id,
            flow: slug.to_string(),
            task_refs,
        }));
    }
    Ok(adopted)
}

fn is_linked(store: &Store, id: &str) -> bool {
    store
        .backlog_links
        .iter()
        .any(|link| link.closes.iter().any(|c| c == id) || link.refs.iter().any(|r| r == id))
}

/// Refs of the rows whose title, action, detail or acceptance name `id`.
fn mentions(store: &Store, id: &str) -> Vec<String> {
    store
        .items
        .iter()
        .filter(|row| {
            [&row.title, &row.action, &row.detail, &row.acceptance]
                .into_iter()
                .any(|text| names_id(text, id))
        })
        .map(|row| row.r#ref.clone())
        .collect()
}

/// `id` as a whole token. A hex digit after it is a longer id that `id`
/// is a prefix of — ids are widened by extending the hex tail.
fn names_id(text: &str, id: &str) -> bool {
    if id.is_empty() {
        return false;
    }
    text.match_indices(id).any(|(at, _)| {
        let before = text[..at].chars().next_back();
        let after = text[at + id.len()..].chars().next();
        !before.is_some_and(|c| c.is_ascii_alphanumeric())
            && !after.is_some_and(|c| c.is_ascii_hexdigit())
    })
}

fn add_close(store: &mut Store, r#ref: &str, id: &str) {
    match store
        .backlog_links
        .iter_mut()
        .find(|link| link.r#ref == r#ref)
    {
        Some(link) => {
            if !link.closes.iter().any(|c| c == id) {
                link.closes.push(id.to_string());
            }
        }
        None => store.backlog_links.push(BacklogLink {
            r#ref: r#ref.to_string(),
            closes: vec![id.to_string()],
            refs: Vec::new(),
        }),
    }
}

fn classify(claim: &Claim, stores: &BTreeMap<String, Store>) -> Entry {
    let mut entry = Entry {
        bucket: Bucket::Dangling,
        id: claim.id.clone(),
        summary: claim.summary.clone(),
        promoted_to: claim.promoted_to.clone(),
        target: claim.target.stored_value(),
        flow: None,
        flow_status: None,
        closes: Vec::new(),
        refs: Vec::new(),
        commits: Vec::new(),
        reason: String::new(),
    };
    let (slug, status) = match &claim.target {
        Target::Flow { slug, status, .. } => (slug, status),
        Target::External(_) => {
            entry.bucket = Bucket::External;
            entry.reason = "promoted to an external reference".to_string();
            return entry;
        }
        Target::Plan { path } => {
            entry.reason = format!("plan `{path}` is bound to no flow");
            return entry;
        }
        Target::Unknown(raw) => {
            entry.reason = format!("`{raw}` names no flow and no plan");
            return entry;
        }
    };
    let fallback = Store::default();
    let store = stores.get(slug).unwrap_or(&fallback);

    let mut notes = Vec::new();
    let closes = joined(store, &claim.id, |link| &link.closes, &mut notes);
    let refs = joined(store, &claim.id, |link| &link.refs, &mut notes);
    let status_of = |name: &str| {
        store
            .find_ref(name)
            .map_or(Status::Pending, |row| row.status)
    };
    let not_done: Vec<String> = closes
        .iter()
        .map(|name| (name, status_of(name)))
        .filter(|(_, task_status)| *task_status != Status::Done)
        .map(|(name, task_status)| format!("{name} ({task_status})"))
        .collect();
    let stopped = closes
        .iter()
        .any(|name| matches!(status_of(name), Status::Deferred | Status::Failed));
    let progress = match closes.is_empty() {
        true => "no task closes this item".to_string(),
        false => format!("closing tasks not done: {}", not_done.join(", ")),
    };

    let (bucket, lead) = if !closes.is_empty() && not_done.is_empty() {
        (
            Bucket::Ready,
            format!("every closing task is done: {}", closes.join(", ")),
        )
    } else if claim.target.is_closed() {
        (
            Bucket::Orphaned,
            format!("flow `{slug}` is at `{status}`; {progress}"),
        )
    } else if closes.is_empty() {
        (Bucket::Unlinked, progress)
    } else if stopped {
        (Bucket::Stalled, progress)
    } else {
        (Bucket::InProgress, progress)
    };
    if closes.is_empty() && !refs.is_empty() {
        notes.insert(0, format!("referenced only by: {}", refs.join(", ")));
    }

    entry.commits = closes
        .iter()
        .chain(&refs)
        .filter_map(|r#ref| store.find_ref(r#ref))
        .map(|row| row.commit.as_str())
        .filter(|commit| !commit.is_empty())
        .fold(Vec::new(), |mut seen: Vec<String>, commit| {
            if !seen.iter().any(|s| s == commit) {
                seen.push(commit.to_string());
            }
            seen
        });
    entry.bucket = bucket;
    entry.reason = std::iter::once(lead)
        .chain(notes)
        .collect::<Vec<_>>()
        .join("; ");
    entry.flow = Some(slug.clone());
    entry.flow_status = Some(status.clone());
    entry.closes = closes;
    entry.refs = refs;
    entry
}

/// The refs of the links naming `id` in `side`, left out and noted when no
/// row carries them.
fn joined(
    store: &Store,
    id: &str,
    side: impl Fn(&BacklogLink) -> &Vec<String>,
    notes: &mut Vec<String>,
) -> Vec<String> {
    let mut refs: Vec<String> = Vec::new();
    for link in &store.backlog_links {
        if !side(link).iter().any(|linked| linked == id) || refs.contains(&link.r#ref) {
            continue;
        }
        match store.find_ref(&link.r#ref) {
            Some(_) => refs.push(link.r#ref.clone()),
            None => notes.push(format!("link names no task: {}", link.r#ref)),
        }
    }
    refs
}

/// Resolve every `ready` entry in one locked write. Each row is re-found and
/// re-checked under the lock, and one that fails is skipped without holding
/// the others back; the file is left untouched when none resolves.
fn apply_ready(
    entries: &[Entry],
    integrity: &WriteIntegrityArgs,
    today: toml::value::Datetime,
) -> Result<Applied> {
    let ready: Vec<&Entry> = entries
        .iter()
        .filter(|entry| entry.bucket == Bucket::Ready)
        .collect();
    let mut outcome = Applied::default();
    if ready.is_empty() {
        return Ok(outcome);
    }
    let path = schema::backlog_path()?;
    let on_missing = on_missing_for(&path, integrity.no_create)?;
    let created = mutate_doc_conditional(
        &path,
        integrity.allow_outside,
        write_integrity_opts(integrity),
        on_missing,
        |doc| {
            for entry in &ready {
                match resolve_entry(doc, entry, today) {
                    Ok(()) => outcome.applied.push(entry.id.clone()),
                    Err(reason) => outcome.skipped.push((entry.id.clone(), reason)),
                }
            }
            if outcome.applied.is_empty() {
                return Ok(false);
            }
            if let Some(table) = doc.as_table_mut() {
                table.insert(FIELD_LAST_UPDATED.to_string(), TomlValue::Datetime(today));
            }
            Ok(true)
        },
    )?;
    warn_if_created(&path, created);
    Ok(outcome)
}

fn resolve_entry(
    doc: &mut TomlValue,
    entry: &Entry,
    today: toml::value::Datetime,
) -> std::result::Result<(), String> {
    let slug = entry.flow.as_deref().unwrap_or_default();
    let rows = items_array_mut(doc, schema::ARRAY_BACKLOG).map_err(|e| format!("{e:#}"))?;
    let Some(row) = rows
        .iter_mut()
        .find(|row| item_id(row) == Some(entry.id.as_str()))
    else {
        return Err("no longer in the backlog".to_string());
    };
    let status = str_field(row, schema::FIELD_STATUS);
    if status != schema::STATUS_PROMOTED {
        return Err(format!("no longer `promoted` (now `{status}`)"));
    }
    let target = str_field(row, schema::FIELD_PROMOTED_TO);
    if target != entry.promoted_to {
        return Err(format!(
            "`promoted_to` changed from `{}` to `{target}`",
            entry.promoted_to
        ));
    }
    let resolution = format!(
        "resolved by flow `{slug}` (tasks {})",
        entry.closes.join(", ")
    );
    let link = schema::ResolutionLink {
        flow: slug.to_string(),
        tasks: entry.closes.iter().chain(&entry.refs).cloned().collect(),
        commits: entry.commits.clone(),
    };
    let mut staged = row.clone();
    resolve_with_link(&mut staged, &resolution, &link, today).map_err(|e| format!("{e:#}"))?;
    *row = staged;
    Ok(())
}

fn str_field<'a>(row: &'a TomlValue, field: &str) -> &'a str {
    row.get(field)
        .and_then(TomlValue::as_str)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{with_root, write};
    use std::path::Path;

    fn write_args() -> WriteIntegrityArgs {
        WriteIntegrityArgs {
            allow_outside: false,
            no_write_integrity: false,
            verify_integrity: false,
            strict_integrity: false,
            no_create: false,
        }
    }

    fn today() -> toml::value::Datetime {
        "2026-09-28".parse().unwrap()
    }

    fn run(flow: Option<&str>, apply: bool, adopt: bool) -> JsonValue {
        reconcile(flow, apply, adopt, &write_args(), today()).expect("reconcile runs")
    }

    fn seed_flow(root: &Path, slug: &str, status: &str) {
        write(
            root,
            &format!(".claude/flows/{slug}/context.toml"),
            format!("status = \"{status}\"\nplan_path = \"docs/plans/{slug}.md\"\n").as_bytes(),
        );
    }

    struct Task<'a> {
        r#ref: &'a str,
        status: &'a str,
        commit: &'a str,
        detail: &'a str,
    }

    fn task<'a>(r#ref: &'a str, status: &'a str) -> Task<'a> {
        Task {
            r#ref,
            status,
            commit: "",
            detail: "",
        }
    }

    type Link<'a> = (&'a str, &'a [&'a str], &'a [&'a str]);

    fn tasks_path(root: &Path, slug: &str) -> std::path::PathBuf {
        root.join(".claude")
            .join("flows")
            .join(slug)
            .join("tasks.toml")
    }

    fn seed_tasks(root: &Path, slug: &str, tasks: &[Task], links: &[Link]) {
        let mut text = format!("schema_version = 1\nplan_path = \"docs/plans/{slug}.md\"\n");
        for (name, closes, refs) in links {
            text.push_str(&format!(
                "\n[[backlog_links]]\nref = {name:?}\ncloses = {closes:?}\nrefs = {refs:?}\n"
            ));
        }
        for (index, t) in tasks.iter().enumerate() {
            text.push_str(&format!(
                "\n[[items]]\nid = {}\nref = {:?}\ntitle = \"Task {}\"\neffort = \"S\"\n\
                 status = {:?}\ncommit = {:?}\ndetail = {:?}\n",
                index + 1,
                t.r#ref,
                t.r#ref,
                t.status,
                t.commit,
                t.detail
            ));
        }
        write(
            root,
            &format!(".claude/flows/{slug}/tasks.toml"),
            text.as_bytes(),
        );
    }

    /// Promoted rows `(id, promoted_to)`, plus one open row no bucket takes.
    fn seed_backlog(root: &Path, rows: &[(&str, &str)]) -> std::path::PathBuf {
        let mut text = String::from(
            "schema_version = 1\nlast_updated = 2026-08-01\n\n[[backlog]]\n\
             id = \"B-0pe00000\"\nkind = \"bug\"\nsummary = \"still open\"\narea = \"\"\n\
             status = \"open\"\ncreated = 2026-08-01\nlast_seen = 2026-08-01\nseen_count = 1\n",
        );
        for (id, target) in rows {
            text.push_str(&format!(
                "\n[[backlog]]\nid = {id:?}\nkind = \"bug\"\nsummary = \"item {id}\"\n\
                 area = \"\"\nstatus = \"promoted\"\ncreated = 2026-08-01\n\
                 last_seen = 2026-08-01\nseen_count = 1\npromoted = 2026-08-02\n\
                 promoted_to = {target:?}\n"
            ));
        }
        let path = root.join(".claude").join("backlog.toml");
        std::fs::write(&path, text).unwrap();
        path
    }

    fn bucket_ids(report: &JsonValue, bucket: &str) -> Vec<String> {
        report["buckets"][bucket]
            .as_array()
            .unwrap_or_else(|| panic!("bucket `{bucket}` missing: {report}"))
            .iter()
            .map(|entry| entry["id"].as_str().unwrap().to_string())
            .collect()
    }

    fn entry<'a>(report: &'a JsonValue, bucket: &str, id: &str) -> &'a JsonValue {
        report["buckets"][bucket]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["id"] == id)
            .unwrap_or_else(|| panic!("`{id}` is not in `{bucket}`: {report}"))
    }

    fn backlog_row(path: &Path, id: &str) -> toml::Table {
        let doc: TomlValue = toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        items_array(&doc, schema::ARRAY_BACKLOG)
            .iter()
            .find(|row| item_id(row) == Some(id))
            .and_then(TomlValue::as_table)
            .cloned()
            .unwrap()
    }

    fn strings(value: Option<&TomlValue>) -> Vec<String> {
        value
            .and_then(TomlValue::as_array)
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect()
    }

    #[test]
    fn every_closing_task_done_is_ready_and_the_run_writes_nothing() {
        let (report, before, after, sidecar) = with_root(|root| {
            seed_flow(root, "alpha", "in-progress");
            seed_tasks(
                root,
                "alpha",
                &[
                    task("one", "done"),
                    task("two", "done"),
                    task("three", "pending"),
                ],
                &[
                    ("one", &["B-aaaa0001"], &[]),
                    ("two", &["B-aaaa0001"], &[]),
                    ("three", &[], &["B-aaaa0001"]),
                ],
            );
            let path = seed_backlog(root, &[("B-aaaa0001", "alpha")]);
            let before = std::fs::read(&path).unwrap();
            let report = run(None, false, false);
            let sidecar = path.with_file_name("backlog.toml.sha256").exists();
            (report, before, std::fs::read(&path).unwrap(), sidecar)
        });
        assert_eq!(bucket_ids(&report, "ready"), ["B-aaaa0001"], "{report}");
        let ready = entry(&report, "ready", "B-aaaa0001");
        assert_eq!(ready["closes"], json!(["one", "two"]));
        assert_eq!(ready["refs"], json!(["three"]));
        assert_eq!(ready["target"], "alpha");
        assert_eq!(ready["flow_status"], "in-progress");
        assert_eq!(report["applied"], json!([]));
        assert_eq!(before, after, "a run without flags is read-only");
        assert!(!sidecar);
        for bucket in Bucket::ALL {
            assert!(report["buckets"][bucket.name()].is_array(), "{report}");
        }
    }

    #[test]
    fn a_pending_closing_task_is_in_progress() {
        let report = with_root(|root| {
            seed_flow(root, "alpha", "in-progress");
            seed_tasks(
                root,
                "alpha",
                &[task("one", "done"), task("two", "in-progress")],
                &[("one", &["B-aaaa0001"], &[]), ("two", &["B-aaaa0001"], &[])],
            );
            seed_backlog(root, &[("B-aaaa0001", "alpha")]);
            run(None, false, false)
        });
        assert_eq!(bucket_ids(&report, "in-progress"), ["B-aaaa0001"]);
        let reason = entry(&report, "in-progress", "B-aaaa0001")["reason"]
            .as_str()
            .unwrap();
        assert!(reason.contains("two (in-progress)"), "{reason}");
        assert!(!reason.contains("one ("), "{reason}");
    }

    #[test]
    fn a_deferred_or_failed_closing_task_is_stalled() {
        let report = with_root(|root| {
            seed_flow(root, "alpha", "in-progress");
            seed_tasks(
                root,
                "alpha",
                &[
                    task("one", "deferred"),
                    task("two", "pending"),
                    task("three", "failed"),
                ],
                &[
                    ("one", &["B-aaaa0001"], &[]),
                    ("two", &["B-aaaa0001"], &[]),
                    ("three", &["B-aaaa0002"], &[]),
                ],
            );
            seed_backlog(root, &[("B-aaaa0001", "alpha"), ("B-aaaa0002", "alpha")]);
            run(None, false, false)
        });
        assert_eq!(
            bucket_ids(&report, "stalled"),
            ["B-aaaa0001", "B-aaaa0002"],
            "{report}"
        );
    }

    #[test]
    fn a_refs_only_link_is_unlinked_and_noted() {
        let report = with_root(|root| {
            seed_flow(root, "alpha", "in-progress");
            seed_tasks(
                root,
                "alpha",
                &[task("one", "done")],
                &[("one", &[], &["B-aaaa0001"])],
            );
            seed_backlog(root, &[("B-aaaa0001", "alpha")]);
            run(None, false, false)
        });
        assert_eq!(bucket_ids(&report, "unlinked"), ["B-aaaa0001"], "{report}");
        let unlinked = entry(&report, "unlinked", "B-aaaa0001");
        assert_eq!(unlinked["refs"], json!(["one"]));
        assert!(
            unlinked["reason"]
                .as_str()
                .unwrap()
                .contains("referenced only by: one"),
            "{unlinked}"
        );
    }

    #[test]
    fn a_closes_link_naming_no_task_is_left_out_of_the_join() {
        let report = with_root(|root| {
            seed_flow(root, "alpha", "in-progress");
            seed_tasks(
                root,
                "alpha",
                &[task("one", "done")],
                &[
                    ("renamed-away", &["B-aaaa0001", "B-aaaa0002"], &[]),
                    ("one", &["B-aaaa0002"], &[]),
                ],
            );
            seed_backlog(root, &[("B-aaaa0001", "alpha"), ("B-aaaa0002", "alpha")]);
            run(None, false, false)
        });
        let unlinked = entry(&report, "unlinked", "B-aaaa0001");
        assert_eq!(unlinked["closes"], json!([]));
        assert!(
            unlinked["reason"]
                .as_str()
                .unwrap()
                .contains("link names no task: renamed-away"),
            "{unlinked}"
        );
        let ready = entry(&report, "ready", "B-aaaa0002");
        assert_eq!(ready["closes"], json!(["one"]));
        assert!(
            ready["reason"]
                .as_str()
                .unwrap()
                .contains("link names no task: renamed-away"),
            "{ready}"
        );
    }

    #[test]
    fn a_closed_flow_orphans_unfinished_rows_but_not_ready_ones() {
        let report = with_root(|root| {
            seed_flow(root, "parked", "review");
            seed_tasks(
                root,
                "parked",
                &[task("one", "pending"), task("two", "done")],
                &[("one", &["B-aaaa0001"], &[]), ("two", &["B-aaaa0003"], &[])],
            );
            seed_backlog(
                root,
                &[
                    ("B-aaaa0001", "parked"),
                    ("B-aaaa0002", "parked"),
                    ("B-aaaa0003", "parked"),
                ],
            );
            run(None, false, false)
        });
        assert_eq!(
            bucket_ids(&report, "orphaned"),
            ["B-aaaa0001", "B-aaaa0002"],
            "{report}"
        );
        assert_eq!(bucket_ids(&report, "ready"), ["B-aaaa0003"]);
        let orphan = entry(&report, "orphaned", "B-aaaa0002");
        assert_eq!(orphan["flow_status"], "review");
        assert!(
            orphan["reason"]
                .as_str()
                .unwrap()
                .contains("flow `parked` is at `review`"),
            "{orphan}"
        );
    }

    #[test]
    fn unknown_and_unbound_plan_targets_dangle_and_external_ones_are_external() {
        let report = with_root(|root| {
            write(root, "docs/plans/loose.md", b"# Plan\n");
            seed_backlog(
                root,
                &[
                    ("B-aaaa0001", "task-store-polish"),
                    ("B-aaaa0002", "docs/plans/loose.md"),
                    ("B-aaaa0003", "external:GH-42"),
                ],
            );
            run(None, false, false)
        });
        assert_eq!(
            bucket_ids(&report, "dangling"),
            ["B-aaaa0001", "B-aaaa0002"],
            "{report}"
        );
        assert_eq!(bucket_ids(&report, "external"), ["B-aaaa0003"]);
        let external = entry(&report, "external", "B-aaaa0003");
        assert_eq!(external["target"], "external:GH-42");
        assert_eq!(external["flow_status"], JsonValue::Null);
        assert!(
            entry(&report, "dangling", "B-aaaa0002")["reason"]
                .as_str()
                .unwrap()
                .contains("bound to no flow")
        );
    }

    #[test]
    fn a_flow_with_no_tasks_store_is_unlinked_and_adopt_creates_none() {
        let (report, created) = with_root(|root| {
            seed_flow(root, "alpha", "in-progress");
            seed_backlog(root, &[("B-aaaa0001", "alpha")]);
            let report = run(None, false, true);
            (report, tasks_path(root, "alpha").exists())
        });
        assert_eq!(bucket_ids(&report, "unlinked"), ["B-aaaa0001"], "{report}");
        assert_eq!(report["adopted"], json!([]));
        assert!(!created, "--adopt must not create a store");
    }

    #[test]
    fn apply_resolves_ready_rows_with_the_link_and_keeps_the_claim() {
        let (report, backlog, pending) = with_root(|root| {
            seed_flow(root, "alpha", "in-progress");
            seed_tasks(
                root,
                "alpha",
                &[
                    Task {
                        commit: "abc1234",
                        ..task("one", "done")
                    },
                    Task {
                        commit: "abc1234",
                        ..task("two", "done")
                    },
                    Task {
                        commit: "def5678",
                        ..task("three", "pending")
                    },
                    task("four", "pending"),
                ],
                &[
                    ("one", &["B-aaaa0001"], &[]),
                    ("two", &["B-aaaa0001"], &[]),
                    ("three", &[], &["B-aaaa0001"]),
                    ("four", &["B-aaaa0002"], &[]),
                ],
            );
            let path = seed_backlog(root, &[("B-aaaa0001", "alpha"), ("B-aaaa0002", "alpha")]);
            let report = run(None, true, false);
            (
                report,
                backlog_row(&path, "B-aaaa0001"),
                backlog_row(&path, "B-aaaa0002"),
            )
        });
        assert_eq!(report["applied"], json!(["B-aaaa0001"]), "{report}");
        assert_eq!(report["skipped"], json!([]));
        assert_eq!(
            backlog
                .get(schema::FIELD_STATUS)
                .and_then(TomlValue::as_str),
            Some(schema::STATUS_RESOLVED)
        );
        assert_eq!(
            backlog
                .get(schema::FIELD_RESOLUTION)
                .and_then(TomlValue::as_str),
            Some("resolved by flow `alpha` (tasks one, two)")
        );
        assert_eq!(
            backlog
                .get(schema::FIELD_RESOLVED_FLOW)
                .and_then(TomlValue::as_str),
            Some("alpha")
        );
        assert_eq!(
            strings(backlog.get(schema::FIELD_RESOLVED_TASKS)),
            ["one", "two", "three"]
        );
        assert_eq!(
            strings(backlog.get(schema::FIELD_RESOLVED_COMMITS)),
            ["abc1234", "def5678"]
        );
        assert_eq!(
            backlog
                .get(schema::FIELD_PROMOTED_TO)
                .and_then(TomlValue::as_str),
            Some("alpha")
        );
        assert_eq!(
            pending
                .get(schema::FIELD_STATUS)
                .and_then(TomlValue::as_str),
            Some(schema::STATUS_PROMOTED),
            "an in-progress row is left alone"
        );
    }

    #[test]
    fn apply_stamps_last_updated_and_records_empty_commits() {
        let (last_updated, commits) = with_root(|root| {
            seed_flow(root, "alpha", "in-progress");
            seed_tasks(
                root,
                "alpha",
                &[task("one", "done")],
                &[("one", &["B-aaaa0001"], &[])],
            );
            let path = seed_backlog(root, &[("B-aaaa0001", "alpha")]);
            run(None, true, false);
            let doc: TomlValue = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            (
                doc.get(FIELD_LAST_UPDATED).map(ToString::to_string),
                backlog_row(&path, "B-aaaa0001")
                    .get(schema::FIELD_RESOLVED_COMMITS)
                    .cloned(),
            )
        });
        assert_eq!(last_updated.as_deref(), Some("2026-09-28"));
        assert_eq!(commits, Some(TomlValue::Array(Vec::new())));
    }

    #[test]
    fn apply_skips_a_row_whose_claim_moved_and_leaves_the_file_untouched() {
        let (outcome, before, after) = with_root(|root| {
            seed_flow(root, "alpha", "in-progress");
            seed_flow(root, "beta", "in-progress");
            seed_tasks(
                root,
                "alpha",
                &[task("one", "done")],
                &[("one", &["B-aaaa0001"], &[])],
            );
            let path = seed_backlog(root, &[("B-aaaa0001", "alpha")]);
            let survey = survey(None, false, &write_args()).unwrap();
            seed_backlog(root, &[("B-aaaa0001", "beta")]);
            let before = std::fs::read(&path).unwrap();
            let outcome = apply_ready(&survey.entries, &write_args(), today()).unwrap();
            (outcome, before, std::fs::read(&path).unwrap())
        });
        assert!(outcome.applied.is_empty());
        assert_eq!(outcome.skipped.len(), 1);
        assert!(
            outcome.skipped[0]
                .1
                .contains("changed from `alpha` to `beta`"),
            "{:?}",
            outcome.skipped
        );
        assert_eq!(before, after, "no row resolved, so nothing is written");
    }

    #[test]
    fn a_row_failing_validation_is_skipped_and_the_others_still_resolve() {
        let (report, good) = with_root(|root| {
            seed_flow(root, "alpha", "in-progress");
            seed_tasks(
                root,
                "alpha",
                &[task("one", "done")],
                &[("one", &["B-aaaa0001", "B-aaaa0002"], &[])],
            );
            let path = seed_backlog(root, &[("B-aaaa0001", "alpha"), ("B-aaaa0002", "alpha")]);
            let text = std::fs::read_to_string(&path)
                .unwrap()
                .replace("summary = \"item B-aaaa0001\"", "summary = \"\"");
            std::fs::write(&path, text).unwrap();
            let report = run(None, true, false);
            (report, backlog_row(&path, "B-aaaa0002"))
        });
        assert_eq!(report["applied"], json!(["B-aaaa0002"]), "{report}");
        assert_eq!(report["skipped"][0]["id"], "B-aaaa0001", "{report}");
        assert_eq!(
            good.get(schema::FIELD_STATUS).and_then(TomlValue::as_str),
            Some(schema::STATUS_RESOLVED)
        );
    }

    #[test]
    fn adopt_links_a_prose_referenced_id_and_names_the_flow_to_render() {
        let (report, links) = with_root(|root| {
            seed_flow(root, "alpha", "in-progress");
            seed_tasks(
                root,
                "alpha",
                &[
                    Task {
                        detail: "Fixes backlog item B-aaaa0001.",
                        ..task("one", "pending")
                    },
                    task("two", "pending"),
                ],
                &[],
            );
            seed_backlog(root, &[("B-aaaa0001", "alpha")]);
            let report = run(None, false, true);
            let store = load_store(
                &tasks_path(root, "alpha"),
                &ReadIntegrityArgs {
                    verify_integrity: true,
                    strict_read: false,
                },
            )
            .unwrap();
            (report, store.backlog_links)
        });
        assert_eq!(
            report["adopted"],
            json!([{"id": "B-aaaa0001", "flow": "alpha", "task_refs": ["one"]}]),
            "{report}"
        );
        assert_eq!(report["render_needed"], json!(["alpha"]));
        assert_eq!(
            links,
            vec![BacklogLink {
                r#ref: "one".into(),
                closes: vec!["B-aaaa0001".into()],
                refs: Vec::new(),
            }]
        );
        assert_eq!(
            bucket_ids(&report, "in-progress"),
            ["B-aaaa0001"],
            "bucketing reads the store as adopt left it"
        );
    }

    #[test]
    fn adopt_then_apply_resolves_a_row_adopted_in_the_same_run() {
        let (report, row) = with_root(|root| {
            seed_flow(root, "alpha", "in-progress");
            seed_tasks(
                root,
                "alpha",
                &[Task {
                    detail: "Closes `B-aaaa0001`",
                    commit: "abc1234",
                    ..task("one", "done")
                }],
                &[],
            );
            let path = seed_backlog(root, &[("B-aaaa0001", "alpha")]);
            let report = run(None, true, true);
            (report, backlog_row(&path, "B-aaaa0001"))
        });
        assert_eq!(report["applied"], json!(["B-aaaa0001"]), "{report}");
        assert_eq!(
            row.get(schema::FIELD_STATUS).and_then(TomlValue::as_str),
            Some(schema::STATUS_RESOLVED)
        );
        assert_eq!(strings(row.get(schema::FIELD_RESOLVED_TASKS)), ["one"]);
    }

    #[test]
    fn adopt_does_not_link_an_id_from_text_naming_a_widened_one() {
        let (report, before, after) = with_root(|root| {
            seed_flow(root, "alpha", "in-progress");
            seed_tasks(
                root,
                "alpha",
                &[Task {
                    detail: "See B-1a2b3c4d5e for the full story.",
                    ..task("one", "pending")
                }],
                &[],
            );
            seed_backlog(root, &[("B-1a2b3c4d", "alpha")]);
            let before = std::fs::read(tasks_path(root, "alpha")).unwrap();
            let report = run(None, false, true);
            (
                report,
                before,
                std::fs::read(tasks_path(root, "alpha")).unwrap(),
            )
        });
        assert_eq!(report["adopted"], json!([]), "{report}");
        assert_eq!(bucket_ids(&report, "unlinked"), ["B-1a2b3c4d"]);
        assert_eq!(before, after, "a run adopting nothing writes no store");
    }

    #[test]
    fn names_id_matches_whole_tokens_only() {
        for (text, want) in [
            ("B-1a2b3c4d", true),
            ("(B-1a2b3c4d)", true),
            ("`B-1a2b3c4d`, then", true),
            ("B-1a2b3c4d.", true),
            ("B-1a2b3c4d5e", false),
            ("B-1a2b3c4dF", false),
            ("XB-1a2b3c4d", false),
            ("B-1a2b3c4d5e and B-1a2b3c4d", true),
            ("", false),
        ] {
            assert_eq!(names_id(text, "B-1a2b3c4d"), want, "{text:?}");
        }
    }

    #[test]
    fn the_flow_filter_keeps_only_rows_whose_target_resolves_to_it() {
        let report = with_root(|root| {
            seed_flow(root, "alpha", "in-progress");
            seed_flow(root, "beta", "in-progress");
            write(root, "docs/plans/alpha.md", b"# Plan\n");
            seed_backlog(
                root,
                &[
                    ("B-aaaa0001", "alpha"),
                    ("B-aaaa0002", "docs/plans/alpha.md"),
                    ("B-aaaa0003", "beta"),
                    ("B-aaaa0004", "task-store-polish"),
                ],
            );
            run(Some("alpha"), false, false)
        });
        assert_eq!(report["flow"], "alpha");
        assert_eq!(
            bucket_ids(&report, "unlinked"),
            ["B-aaaa0001", "B-aaaa0002"],
            "{report}"
        );
        assert_eq!(entry(&report, "unlinked", "B-aaaa0002")["target"], "alpha");
        assert_eq!(bucket_ids(&report, "dangling"), Vec::<String>::new());
    }
}
