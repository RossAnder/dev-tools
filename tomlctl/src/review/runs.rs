//! `review run start`, `run update` and `run finish` — a run's lifecycle.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::Result;
use serde_json::{Map, Value as JsonValue, json};

use super::catalogue::{self, Catalogue};
use super::cells::{self, Accounting};
use super::digest::Counts;
use super::fields::{self, Family, FindingStatus, Origin, Record, RowContext};
use super::invalid;
use super::store::{self, Minted, Run, Signal, Store, Target, Waiver};
use crate::cli::{ReviewRunFinishArgs, ReviewRunStartArgs, ReviewRunUpdateArgs};
use crate::errors::{ErrorKind, tagged_err};
use crate::fields::FieldArgs;
use crate::io::{guard_write_path, read_prose, relativise, relativise_under, repo_or_cwd_root};
use crate::output::print_json_compact;

/// The run fields `run start` takes through `--set` / `--set-json`.
const START_FIELDS: &[&str] = &["scope", "models", "substrate"];
/// The run fields `run update` patches.
const UPDATE_FIELDS: &[&str] = &["signals", "waivers", "audit", "agents", "baselines"];

const SIGNAL_KEYS: &[&str] = &["name", "source", "roots", "accepted", "evidence"];
const WAIVER_KEYS: &[&str] = &["concern", "file", "reason"];

pub(crate) fn start(target: &Target, args: ReviewRunStartArgs) -> Result<()> {
    let verb = "review run start";
    let root = repo_or_cwd_root()?;
    let mut row = Map::new();
    row.insert("command".into(), json!(args.command));
    row.insert("family".into(), json!(args.family));
    row.insert("depth".into(), json!(args.depth));
    row.extend(flag_fields(&args.fields, START_FIELDS, verb)?);
    let family = Family::parse(&args.family);
    let ctx = RowContext {
        family: family.unwrap_or(Family::Code),
        target_exists: &|_| true,
    };
    if let Err(refusals) = fields::validate_row(Record::Run, &row, &ctx) {
        let reasons: Vec<String> = refusals.iter().map(ToString::to_string).collect();
        return Err(invalid(format!("{verb}: {}", reasons.join("; "))));
    }
    let family = family.expect("validate_row refuses an unknown family");
    let scope: Vec<String> = row
        .get("scope")
        .and_then(JsonValue::as_array)
        .map(|a| {
            a.iter()
                .filter_map(JsonValue::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    for entry in &scope {
        if let Some(problem) = super::fields::path_problem(entry, "scope paths") {
            return Err(invalid(format!("{verb}: scope entry {entry:?} {problem}")));
        }
    }

    let base = resolve_base(&root, &args.base)?;
    let catalogue_record = start_catalogue(&root, &base, args.builtin, args.project)?;
    let changed_lines = match family {
        Family::Plan => None,
        Family::Code | Family::Perf => Some(changed_lines(&root, &base, &scope)?),
    };

    row.insert(
        "started".into(),
        json!(
            crate::time::now()
                .strftime("%Y-%m-%dT%H:%M:%SZ")
                .to_string()
        ),
    );
    row.insert("base".into(), json!(base));
    if let Some(n) = changed_lines {
        row.insert("changed_lines".into(), json!(n));
    }
    row.insert("catalogue".into(), catalogue_record);

    let runs_parent = target
        .run_dir("U1")?
        .parent()
        .map(Path::to_path_buf)
        .expect("a run directory has a parent");
    let allow_outside = args.integrity.allow_outside;
    let rows = [JsonValue::Object(row)];
    let (id, dir) = store::mutate_minting(
        &target.path,
        &args.integrity,
        |doc| {
            retire_ids_of_existing_dirs(doc, &runs_parent)?;
            store::mint(doc, Minted::Runs, &rows)
        },
        |_, ids| {
            let id = ids.into_iter().next().expect("one row minted one id");
            let dir = target.run_dir(&id)?;
            // Inside the write: a directory that cannot be made leaves no run behind.
            guard_write_path(&dir, allow_outside)?;
            fs::create_dir_all(dir.join("payloads")).map_err(|e| {
                tagged_err(
                    ErrorKind::Other,
                    Some(dir.clone()),
                    format!("creating run directory {}: {e}", dir.display()),
                )
            })?;
            ignore_runs_root(&runs_parent)?;
            Ok((id, dir))
        },
    )?;
    print_json_compact(&json!({
        "ok": true,
        "run": id,
        "dir": relativise(&root, &dir),
        "base": base,
        "changed_lines": changed_lines,
    }))
}

/// Writes `.gitignore` holding `*` at the runs root above `runs_parent` when
/// absent, so run artifacts stay out of git in a repo that ignores nothing.
fn ignore_runs_root(runs_parent: &Path) -> Result<()> {
    let Some(runs_root) = runs_parent.parent() else {
        return Ok(());
    };
    let ignore = runs_root.join(".gitignore");
    if ignore.exists() {
        return Ok(());
    }
    fs::write(&ignore, "*\n").map_err(|e| {
        tagged_err(
            ErrorKind::Other,
            Some(ignore.clone()),
            format!("writing {}: {e}", ignore.display()),
        )
    })
}

/// Folds the field flags into an object, refusing any key outside `allowed`.
fn flag_fields(args: &FieldArgs, allowed: &[&str], verb: &str) -> Result<Map<String, JsonValue>> {
    let Some(JsonValue::Object(map)) = crate::fields::build(args, None)? else {
        return Ok(Map::new());
    };
    if let Some(key) = map.keys().find(|k| !allowed.contains(&k.as_str())) {
        if fields::spec(Record::Run, key, None).is_some_and(|s| s.origin == Origin::Computed) {
            return Err(invalid(format!(
                "{verb}: `{key}` is set by tomlctl and cannot be supplied"
            )));
        }
        return Err(invalid(format!(
            "{verb}: `{key}` cannot be set here; settable fields: {}",
            allowed.join(", ")
        )));
    }
    Ok(map)
}

fn git(root: &Path) -> Command {
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(root);
    cmd
}

fn git_output(root: &Path, cmd: &mut Command, what: &str) -> Result<Vec<u8>> {
    let out = cmd.output().map_err(|e| {
        tagged_err(
            ErrorKind::Other,
            Some(root.to_path_buf()),
            format!("{what}: {e}"),
        )
    })?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(tagged_err(
            ErrorKind::Other,
            Some(root.to_path_buf()),
            format!("{what} failed: {}", stderr.trim()),
        ));
    }
    Ok(out.stdout)
}

/// `--end-of-options` keeps a `base` starting with `-` from reaching git as
/// an option; `^{commit}` refuses a tree or blob.
fn resolve_base(root: &Path, base: &str) -> Result<String> {
    let out = git(root)
        .args(["rev-parse", "--verify", "--quiet", "--end-of-options"])
        .arg(format!("{base}^{{commit}}"))
        .output()
        .map_err(|e| {
            tagged_err(
                ErrorKind::Other,
                Some(root.to_path_buf()),
                format!("git rev-parse: {e}"),
            )
        })?;
    let sha = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if !out.status.success() || sha.is_empty() {
        return Err(invalid(format!(
            "review run start: --base {base:?} does not name a commit"
        )));
    }
    Ok(sha)
}

/// Loads and validates the merged catalogue, returning the run's
/// `catalogue` record. Any validation error halts the start, as does a
/// default project layer the reviewed range `base..HEAD` itself changed.
fn start_catalogue(
    root: &Path,
    base: &str,
    builtin: Option<PathBuf>,
    project: Option<PathBuf>,
) -> Result<JsonValue> {
    // A default root is left unrecorded, so the git-tracked store names no
    // machine-specific path; `run_catalogue` resolves it again.
    let explicit_builtin = builtin.map(|dir| std::path::absolute(&dir).unwrap_or(dir));
    let builtin = match &explicit_builtin {
        Some(dir) => dir.clone(),
        None => catalogue::default_builtin_root().ok_or_else(|| {
            invalid("review run start: no claude dir (CLAUDE_CONFIG_DIR, USERPROFILE or HOME); pass --builtin")
        })?,
    };
    let explicit_project = project.is_some();
    let project = catalogue::project_layer(root, project)?;
    let layer_changed = project_layer_changes(root, base)?;
    if !explicit_project && !layer_changed.is_empty() {
        return Err(invalid(format!(
            "review run start: the reviewed range {base}..HEAD changes the project lens layer, \
             which would configure its own review:\n{}\n\
             pass --project <dir> explicitly to accept that layer",
            layer_changed
                .iter()
                .map(|p| format!("  - {p}"))
                .collect::<Vec<_>>()
                .join("\n")
        )));
    }
    let catalogue = Catalogue::load(&builtin, project.as_deref())?;
    let errors = catalogue.validate();
    if !errors.is_empty() {
        let lines: Vec<String> = errors.iter().map(|e| format!("  - {e}")).collect();
        return Err(invalid(format!(
            "review run start: the lens catalogue has {} error(s); fix them before starting a run:\n{}",
            errors.len(),
            lines.join("\n")
        )));
    }
    let mut record = Map::new();
    if let Some(dir) = &explicit_builtin {
        record.insert(
            "builtin".into(),
            json!(dir.to_string_lossy().replace('\\', "/")),
        );
    }
    if let Some(dir) = &project {
        let shown =
            relativise_under(root, dir).unwrap_or_else(|| dir.to_string_lossy().replace('\\', "/"));
        record.insert("project".into(), json!(shown));
    }
    record.insert("tree_sha".into(), json!(catalogue.tree_sha()));
    if let Some(sha) = catalogue.config_sha() {
        record.insert("config_sha".into(), json!(sha));
    }
    if !layer_changed.is_empty() {
        record.insert("project_layer_changed".into(), json!(layer_changed));
    }
    Ok(JsonValue::Object(record))
}

/// Paths under `.claude/review-lenses` that `base..HEAD` adds, edits or
/// deletes. Renames are split so a file moved out of the layer still counts.
fn project_layer_changes(root: &Path, base: &str) -> Result<Vec<String>> {
    let listed = git_output(
        root,
        git(root)
            .args(["diff", "--name-only", "-z", "--no-ext-diff", "--no-renames"])
            .arg(format!("{base}..HEAD"))
            .args(["--", ".claude/review-lenses"]),
        "git diff --name-only",
    )?;
    Ok(String::from_utf8_lossy(&listed)
        .split('\0')
        .filter(|p| !p.is_empty())
        .map(str::to_string)
        .collect())
}

fn pathspecs(scope: &[String]) -> Vec<String> {
    scope.iter().map(|s| format!(":(glob){s}")).collect()
}

/// Lines added plus removed over `scope` in `base..HEAD`; when that is 0,
/// the scope's tracked line count.
fn changed_lines(root: &Path, base: &str, scope: &[String]) -> Result<i64> {
    let specs = pathspecs(scope);
    let numstat = git_output(
        root,
        git(root)
            .args(["diff", "--numstat", "--no-ext-diff", "--find-renames"])
            .arg(format!("{base}..HEAD"))
            .arg("--")
            .args(&specs),
        "git diff --numstat",
    )?;
    let mut total: i64 = 0;
    for line in String::from_utf8_lossy(&numstat).lines() {
        let mut cols = line.split('\t');
        for col in [cols.next(), cols.next()] {
            // A binary file reports `-` for both counts.
            if let Some(n) = col.and_then(|c| c.parse::<i64>().ok()) {
                total += n;
            }
        }
    }
    if total > 0 {
        return Ok(total);
    }
    let listed = git_output(
        root,
        git(root).args(["ls-files", "-z", "--"]).args(&specs),
        "git ls-files",
    )?;
    let mut lines: i64 = 0;
    for rel in String::from_utf8_lossy(&listed).split('\0') {
        if rel.is_empty() {
            continue;
        }
        let Ok(bytes) = fs::read(root.join(rel)) else {
            continue;
        };
        let newlines = bytes.iter().filter(|&&b| b == b'\n').count();
        let unterminated = bytes.last().is_some_and(|&b| b != b'\n');
        lines += i64::try_from(newlines + usize::from(unterminated)).unwrap_or(i64::MAX);
    }
    Ok(lines)
}

/// Raises the `U` high-water mark past every run directory already on disk,
/// so a run whose row was deleted by hand never hands its id, and with it
/// its leftover artifacts, to a new run.
fn retire_ids_of_existing_dirs(doc: &mut toml::Value, runs_parent: &Path) -> Result<()> {
    let prefix = Minted::Runs.prefix();
    let Ok(entries) = fs::read_dir(runs_parent) else {
        return Ok(());
    };
    let highest = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let digits = name.strip_prefix(prefix)?.to_string();
            let n: u64 = digits.parse().ok()?;
            (n.to_string() == digits).then_some(n)
        })
        .max();
    if let Some(n) = highest {
        crate::items::record_id_high_water(doc, Minted::Runs.array(), &[format!("{prefix}{n}")])?;
    }
    Ok(())
}

pub(crate) fn update(target: &Target, args: ReviewRunUpdateArgs) -> Result<()> {
    let verb = "review run update";
    let patch = flag_fields(&args.fields, UPDATE_FIELDS, verb)?;
    if patch.is_empty() {
        return Err(invalid(format!(
            "{verb}: nothing to update; pass --set-json with one of {}",
            UPDATE_FIELDS.join(", ")
        )));
    }
    let signals = match patch.get("signals") {
        Some(value) => Some(parse_list(value, "signals", parse_signal)?),
        None => None,
    };
    let waivers = match patch.get("waivers") {
        Some(value) => Some(parse_list(value, "waivers", parse_waiver)?),
        None => None,
    };
    let agents = match patch.get("agents") {
        Some(value) => Some(
            value
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .ok_or_else(|| invalid("`agents` must be a non-negative integer"))?,
        ),
        None => None,
    };
    let mut tables = Vec::new();
    for key in ["audit", "baselines"] {
        let Some(value) = patch.get(key) else {
            continue;
        };
        let object = value
            .as_object()
            .ok_or_else(|| invalid(format!("`{key}` must be a table")))?;
        if key == "baselines"
            && let Some((check, _)) = object.iter().find(|(_, v)| !v.is_string())
        {
            return Err(invalid(format!(
                "`baselines.{check}` must be a log path or `none — <reason>`"
            )));
        }
        let toml =
            crate::convert::json_to_toml(value).map_err(|e| invalid(format!("`{key}`: {e}")))?;
        tables.push((key, toml));
    }

    store::require(&target.path)?;
    let run_id = args.run.clone();
    store::mutate(&target.path, &args.integrity, |store| {
        let run = store
            .runs
            .iter_mut()
            .find(|r| r.id == run_id)
            .ok_or_else(|| invalid(format!("{verb}: no run {run_id} in this store")))?;
        if let Some(incoming) = signals {
            upsert_signals(run, incoming)?;
        }
        if let Some(incoming) = waivers {
            upsert_waivers(run, incoming);
        }
        if let Some(n) = agents {
            run.agents = Some(n);
        }
        for (key, incoming) in tables {
            let toml::Value::Table(incoming) = incoming else {
                continue;
            };
            let slot = run
                .rest
                .entry(key.to_string())
                .or_insert_with(|| toml::Value::Table(toml::Table::new()));
            match slot.as_table_mut() {
                Some(existing) => existing.extend(incoming),
                None => *slot = toml::Value::Table(incoming),
            }
        }
        Ok(())
    })?;
    let updated: Vec<&String> = patch.keys().collect();
    print_json_compact(&json!({"ok": true, "run": args.run, "updated": updated}))
}

fn parse_list<T>(
    value: &JsonValue,
    key: &str,
    parse: fn(&Map<String, JsonValue>, &str) -> Result<T>,
) -> Result<Vec<T>> {
    let list = value
        .as_array()
        .ok_or_else(|| invalid(format!("`{key}` must be an array of tables")))?;
    list.iter()
        .enumerate()
        .map(|(i, entry)| {
            let at = format!("{key}[{i}]");
            let object = entry
                .as_object()
                .ok_or_else(|| invalid(format!("`{at}` must be a table")))?;
            parse(object, &at)
        })
        .collect()
}

fn known_keys(object: &Map<String, JsonValue>, at: &str, keys: &[&str]) -> Result<()> {
    match object.keys().find(|k| !keys.contains(&k.as_str())) {
        Some(key) => Err(invalid(format!(
            "`{at}.{key}` is not a field; expected {}",
            keys.join(", ")
        ))),
        None => Ok(()),
    }
}

fn string_of(object: &Map<String, JsonValue>, at: &str, key: &str) -> Result<String> {
    match object.get(key) {
        None => Ok(String::new()),
        Some(JsonValue::String(s)) => Ok(s.clone()),
        Some(_) => Err(invalid(format!("`{at}.{key}` must be a string"))),
    }
}

fn strings_of(object: &Map<String, JsonValue>, at: &str, key: &str) -> Result<Vec<String>> {
    match object.get(key) {
        None => Ok(Vec::new()),
        Some(JsonValue::Array(items)) => items
            .iter()
            .map(|v| {
                v.as_str()
                    .map(str::to_string)
                    .ok_or_else(|| invalid(format!("`{at}.{key}` must be an array of strings")))
            })
            .collect(),
        Some(_) => Err(invalid(format!("`{at}.{key}` must be an array of strings"))),
    }
}

/// An incoming signal entry: `accepted` stays `None` when absent so an
/// upsert can tell "not given" from `false`.
fn parse_signal(object: &Map<String, JsonValue>, at: &str) -> Result<Signal> {
    known_keys(object, at, SIGNAL_KEYS)?;
    let name = string_of(object, at, "name")?;
    if name.trim().is_empty() {
        return Err(invalid(format!("`{at}.name` is required")));
    }
    let source = match string_of(object, at, "source")?.as_str() {
        "" | "judged" => "judged".to_string(),
        other => {
            return Err(invalid(format!(
                "`{at}` ({name}): source {other:?} cannot be written by run update; \
                 only `judged` signals are added here — detector signals come from `review survey`"
            )));
        }
    };
    let accepted = match object.get("accepted") {
        None => None,
        Some(JsonValue::Bool(b)) => Some(*b),
        Some(_) => return Err(invalid(format!("`{at}.accepted` must be a boolean"))),
    };
    Ok(Signal {
        name,
        source,
        roots: strings_of(object, at, "roots")?,
        accepted,
        evidence: strings_of(object, at, "evidence")?,
    })
}

fn parse_waiver(object: &Map<String, JsonValue>, at: &str) -> Result<Waiver> {
    known_keys(object, at, WAIVER_KEYS)?;
    let waiver = Waiver {
        concern: string_of(object, at, "concern")?,
        file: string_of(object, at, "file")?,
        reason: string_of(object, at, "reason")?,
    };
    if waiver.concern.is_empty() == waiver.file.is_empty() {
        return Err(invalid(format!(
            "`{at}` must name exactly one of `concern` and `file`"
        )));
    }
    if waiver.reason.trim().is_empty() {
        return Err(invalid(format!("`{at}.reason` is required")));
    }
    Ok(waiver)
}

/// A new name is appended as a judged candidate. An existing judged entry
/// may only have `accepted` set; every other field given must equal what is
/// stored, so a retried write is a no-op. A detector entry is never touched.
fn upsert_signals(run: &mut Run, incoming: Vec<Signal>) -> Result<()> {
    for signal in incoming {
        let Some(existing) = run.signals.iter_mut().find(|s| s.name == signal.name) else {
            run.signals.push(signal);
            continue;
        };
        if existing.source != "judged" {
            return Err(invalid(format!(
                "signal `{}` was recorded by `{}`; run update changes only judged signals",
                signal.name, existing.source
            )));
        }
        let differs = |given: &[String], stored: &[String]| !given.is_empty() && given != stored;
        if differs(&signal.roots, &existing.roots) || differs(&signal.evidence, &existing.evidence)
        {
            return Err(invalid(format!(
                "signal `{}` already exists; run update may only set its `accepted`",
                signal.name
            )));
        }
        if signal.accepted.is_some() {
            existing.accepted = signal.accepted;
        }
    }
    Ok(())
}

/// Keyed by the waived concern or file; a repeated key replaces its reason.
fn upsert_waivers(run: &mut Run, incoming: Vec<Waiver>) {
    for waiver in incoming {
        match run
            .waivers
            .iter_mut()
            .find(|w| w.concern == waiver.concern && w.file == waiver.file)
        {
            Some(existing) => existing.reason = waiver.reason,
            None => run.waivers.push(waiver),
        }
    }
}

pub(crate) fn finish(target: &Target, args: ReviewRunFinishArgs) -> Result<()> {
    let verb = "review run finish";
    let incomplete = read_prose(
        verb,
        "incomplete",
        args.incomplete.as_deref(),
        args.incomplete_file.as_deref(),
    )?;
    store::require(&target.path)?;
    let root = repo_or_cwd_root()?;
    let run_id = args.run.clone();
    let (finished, counts, gaps) = store::mutate(&target.path, &args.integrity, |store| {
        if !store.runs.iter().any(|r| r.id == run_id) {
            return Err(invalid(format!("{verb}: no run {run_id} in this store")));
        }
        // Read under the write lock, so no finding lands between the gate and the stamp.
        let accounting = cells::coverage_accounting(&root, store, &run_id)?;
        let gaps = Gaps::of(store, &run_id, &accounting);
        if !gaps.is_empty() && incomplete.is_none() {
            return Err(invalid(format!(
                "{verb}: {run_id} is not complete — {}; close them, or pass --incomplete <reason> to finish anyway",
                gaps.describe()
            )));
        }
        let counts = Counts::of(store, &run_id).stored();
        let finished = crate::time::now()
            .strftime("%Y-%m-%dT%H:%M:%SZ")
            .to_string();
        let run = store
            .runs
            .iter_mut()
            .find(|r| r.id == run_id)
            .expect("checked above");
        run.finished = finished.clone();
        let table = counts
            .iter()
            .map(|(k, n)| ((*k).to_string(), toml::Value::Integer(*n)))
            .collect();
        run.rest
            .insert("counts".to_string(), toml::Value::Table(table));
        match &incomplete {
            Some(reason) => {
                run.rest.insert(
                    "incomplete".to_string(),
                    toml::Value::String(reason.clone()),
                );
            }
            // A later clean finish clears the reason an earlier bypass left.
            None => {
                run.rest.remove("incomplete");
            }
        }
        Ok((finished, counts, gaps))
    })?;
    let counts: Map<String, JsonValue> = counts
        .into_iter()
        .map(|(k, n)| (k.to_string(), json!(n)))
        .collect();
    let mut out = json!({
        "ok": true,
        "run": args.run,
        "finished": finished,
        "counts": counts,
    });
    if let Some(reason) = incomplete {
        out["incomplete"] = json!(reason);
        out["gaps"] = gaps.to_json();
    }
    print_json_compact(&out)
}

/// What keeps a run from finishing cleanly.
struct Gaps {
    /// Findings still `unverified` with no `same_as`.
    unverified: Vec<String>,
    /// Findings still `unverified` whose `same_as` awaits `review resolve`.
    unresolved_same_as: Vec<String>,
    unselected: Vec<String>,
    unsliced: Vec<String>,
    /// Cells whose agent never sealed them.
    unsealed: Vec<String>,
}

impl Gaps {
    fn of(store: &Store, run: &str, accounting: &Accounting) -> Self {
        let pending = store
            .findings
            .iter()
            .filter(|f| f.run == run && f.status() == FindingStatus::Unverified);
        let (unresolved_same_as, unverified): (Vec<_>, Vec<_>) =
            pending.partition(|f| !f.same_as.is_empty());
        let ids = |rows: Vec<&store::Finding>| rows.into_iter().map(|f| f.id.clone()).collect();
        Self {
            unverified: ids(unverified),
            unresolved_same_as: ids(unresolved_same_as),
            unselected: accounting.unselected.clone(),
            unsliced: accounting.unsliced.clone(),
            unsealed: store
                .cells
                .iter()
                .filter(|c| c.run == run && !c.sealed)
                .map(|c| c.id.clone())
                .collect(),
        }
    }

    fn is_empty(&self) -> bool {
        self.unverified.is_empty()
            && self.unresolved_same_as.is_empty()
            && self.unselected.is_empty()
            && self.unsliced.is_empty()
            && self.unsealed.is_empty()
    }

    fn describe(&self) -> String {
        [
            ("unverified findings", &self.unverified),
            ("unresolved same_as", &self.unresolved_same_as),
            ("unselected concerns without a waiver", &self.unselected),
            ("unsliced files without a waiver", &self.unsliced),
            ("unsealed cells", &self.unsealed),
        ]
        .iter()
        .filter(|(_, ids)| !ids.is_empty())
        .map(|(what, ids)| format!("{what}: {}", ids.join(", ")))
        .collect::<Vec<_>>()
        .join("; ")
    }

    fn to_json(&self) -> JsonValue {
        json!({
            "unverified": self.unverified,
            "unresolved_same_as": self.unresolved_same_as,
            "unselected": self.unselected,
            "unsliced": self.unsliced,
            "unsealed": self.unsealed,
        })
    }
}
