// Crate root for the `tomlctl` library: module wiring plus `run()`, the parse,
// dispatch and error-report cycle the binary's `main` delegates to. CLI
// parsing, dispatch and output plumbing belong in `cli.rs`, per-subcommand
// behaviour in sibling modules. Modules stay private; anything a library
// consumer needs is exposed as a `pub` wrapper or constant here.

mod agents;
mod anchor;
mod backlog;
mod blocks;
mod capabilities;
mod cli;
mod clusters;
mod convert;
mod dedup;
mod errors;
mod flow;
mod inputs;
mod integrity;
mod io;
mod items;
mod items_sweep;
mod json;
mod ledgers;
mod orphans;
mod output;
mod owner;
mod query;
mod repo_files;
mod repo_root;
mod sweep;
mod tasks;
#[cfg(test)]
mod test_support;
mod time;
mod union_find;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Parser;

use crate::cli::{Cli, ErrorFormat, ReadIntegrityArgs, WriteIntegrityArgs};
use crate::errors::TaggedError;
use crate::ledgers::FACADE_WRITE;

pub use crate::backlog::triage::BacklogTriage;
pub use crate::items::STATUS_COMPANIONS;
pub use crate::ledgers::{LedgerKind, LedgerRef, RestoreRow};

/// The files of a flow directory that [`snapshot`] reads, store first. Its
/// `revision` hashes them in this order, so a poller fingerprinting exactly
/// these files sees every change the snapshot can.
pub const SNAPSHOT_INPUTS: [&str; 4] = [
    "tasks.toml",
    "execution-record.toml",
    "agents.toml",
    "context.toml",
];

/// Refuses a flow slug the CLI's `--slug` would refuse, so a consumer can
/// reject one up front rather than on every [`snapshot`] call.
pub fn validate_slug(slug: &str) -> anyhow::Result<()> {
    flow::validate_slug(slug)
}

/// The `tasks snapshot --slug <slug>` document for the flow under
/// `<root>/.claude/flows`, read without integrity checks. Refuses a slug the
/// CLI would refuse, so it can never name a path outside that directory.
/// Advisories are silenced for the process.
pub fn snapshot(root: &Path, slug: &str) -> anyhow::Result<serde_json::Value> {
    let built = snapshot_if_changed(root, slug, None)?;
    Ok(built.expect("no known revision always builds"))
}

/// [`snapshot`], or `None` when the flow's inputs still hash to
/// `known_revision`, in which case nothing past the hash is parsed or built.
pub fn snapshot_if_changed(
    root: &Path,
    slug: &str,
    known_revision: Option<&str>,
) -> anyhow::Result<Option<serde_json::Value>> {
    io::silence_advisories();
    flow::validate_slug(slug)?;
    let store_path = root
        .join(".claude")
        .join("flows")
        .join(slug)
        .join(SNAPSHOT_INPUTS[0]);
    let read_opts = ReadIntegrityArgs {
        verify_integrity: false,
        strict_read: false,
    };
    tasks::snapshot(slug, &store_path, &read_opts, known_revision)
}

/// The unfiltered `flow list` envelope for every flow under
/// `<root>/.claude/flows`, read without integrity checks.
pub fn flow_list(root: &Path) -> anyhow::Result<serde_json::Value> {
    flow_list_matching(root, |_| true)
}

/// [`flow_list`] restricted to the flows whose slug `keep` accepts; a rejected
/// flow's `context.toml` is never read, so it cannot land in `skipped` either.
pub fn flow_list_matching(
    root: &Path,
    keep: impl FnMut(&str) -> bool,
) -> anyhow::Result<serde_json::Value> {
    io::silence_advisories();
    flow::list_all(root, keep)
}

/// One ledger's `{"path", "kind", "revision", "items"}`, read without
/// integrity checks. `revision` is the hex sha256 of the file's bytes, and a
/// missing file reads as no items with a null revision.
pub fn ledger_read(root: &Path, ledger: &LedgerRef) -> anyhow::Result<serde_json::Value> {
    let read = ledger_read_if_changed(root, ledger, None)?;
    Ok(read.expect("no known revision always reads"))
}

/// [`ledger_read`], or `None` when the file still hashes to `known_revision`,
/// in which case nothing past the hash is parsed.
pub fn ledger_read_if_changed(
    root: &Path,
    ledger: &LedgerRef,
    known_revision: Option<&str>,
) -> anyhow::Result<Option<serde_json::Value>> {
    io::silence_advisories();
    ledgers::read(root, ledger, known_revision)
}

/// Every flow under `<root>/.claude/flows` holding a task store or a ledger,
/// and every flow-less review, optimise and plan-review ledger.
pub fn ledger_scopes(root: &Path) -> anyhow::Result<serde_json::Value> {
    io::silence_advisories();
    ledgers::scopes(root)
}

/// Moves each of `ids` whose status is still `expect_status` to `to`, writing
/// the transition's form `fields` and dropping the companions of the status
/// it leaves. Returns `{"applied": [ids], "skipped_stale": [{id, field,
/// expected, found}]}`. Refuses with `root mismatch` unless `root` is the
/// process root, and refuses any transition the write model does not offer.
pub fn ledger_transition(
    root: &Path,
    ledger: &LedgerRef,
    ids: &[String],
    to: &str,
    fields: serde_json::Map<String, serde_json::Value>,
    expect_status: &str,
) -> anyhow::Result<serde_json::Value> {
    io::silence_advisories();
    ledgers::transition(root, ledger, ids, to, fields, expect_status)
}

/// Sets `severity`, `effort` and `category` on each of `ids` whose fields
/// still match `expect`, on a review or optimise ledger; returns what
/// [`ledger_transition`] does, and refuses a mismatched root the same way.
pub fn ledger_classify(
    root: &Path,
    ledger: &LedgerRef,
    ids: &[String],
    fields: serde_json::Map<String, serde_json::Value>,
    expect: serde_json::Map<String, serde_json::Value>,
) -> anyhow::Result<serde_json::Value> {
    io::silence_advisories();
    ledgers::classify(root, ledger, ids, fields, expect)
}

/// Undoes one write on `id`: restores `set`, removes `unset`, provided the
/// row still holds `expect`. Returns what [`ledger_transition`] does, and
/// refuses a mismatched root the same way.
pub fn ledger_restore(
    root: &Path,
    ledger: &LedgerRef,
    id: &str,
    set: serde_json::Map<String, serde_json::Value>,
    unset: Vec<String>,
    expect: serde_json::Map<String, serde_json::Value>,
) -> anyhow::Result<serde_json::Value> {
    let row = RestoreRow {
        id: id.to_string(),
        set,
        unset,
        expect,
    };
    ledger_restore_many(root, ledger, vec![row])
}

/// [`ledger_restore`] for several rows in one locked write, each guarded on
/// its own `expect`. Any refused row refuses the whole call, writing nothing.
pub fn ledger_restore_many(
    root: &Path,
    ledger: &LedgerRef,
    rows: Vec<RestoreRow>,
) -> anyhow::Result<serde_json::Value> {
    io::silence_advisories();
    ledgers::restore(root, ledger, rows)
}

/// Applies `triage` to each of `ids` in `<root>/.claude/backlog.toml` whose
/// status is still `expect_status`, as `backlog triage` does. Returns what
/// [`ledger_transition`] does, refuses a mismatched root the same way, and
/// refuses a missing store rather than creating one.
pub fn backlog_triage(
    root: &Path,
    ids: &[String],
    triage: BacklogTriage,
    expect_status: &str,
) -> anyhow::Result<serde_json::Value> {
    io::silence_advisories();
    io::ensure_process_root(root)?;
    let write_opts = WriteIntegrityArgs {
        allow_outside: false,
        no_write_integrity: false,
        verify_integrity: false,
        strict_integrity: false,
        no_create: true,
    };
    let out =
        backlog::triage::triage_value(root, ids, &triage.into(), Some(expect_status), &write_opts)?;
    Ok(serde_json::json!({
        "applied": out["applied"],
        "skipped_stale": out["skipped_stale"],
    }))
}

/// The input store, `<root>/.claude/inputs.toml`.
pub fn inputs_path(root: &Path) -> PathBuf {
    inputs::path(root)
}

/// Every record of `<root>/.claude/inputs.toml` as `inputs list` prints it:
/// `{"path", "revision", "inputs"}`, read without integrity checks. A missing
/// store reads as no records with a null revision.
pub fn inputs_read(root: &Path) -> anyhow::Result<serde_json::Value> {
    let read = inputs_read_if_changed(root, None)?;
    Ok(read.expect("no known revision always reads"))
}

/// [`inputs_read`], or `None` when the store still hashes to `known_revision`,
/// in which case nothing past the hash is parsed.
pub fn inputs_read_if_changed(
    root: &Path,
    known_revision: Option<&str>,
) -> anyhow::Result<Option<serde_json::Value>> {
    io::silence_advisories();
    inputs::list_if_changed(root, &inputs::Filter::default(), known_revision)
}

/// Appends `record` as a `new` input, as `inputs add --json` does, and returns
/// `{"id"}`. Refuses with `root mismatch` unless `root` is the process root.
pub fn inputs_add(root: &Path, record: &serde_json::Value) -> anyhow::Result<serde_json::Value> {
    io::silence_advisories();
    inputs::add(root, record, FACADE_WRITE)
}

/// Answers the `new` question `question` with `picked` options and/or `text`,
/// closing it, as `inputs answer` does; returns `{"id", "question"}` and
/// refuses a mismatched root the same way as [`inputs_add`].
pub fn inputs_answer(
    root: &Path,
    question: &str,
    picked: &[String],
    text: Option<&str>,
) -> anyhow::Result<serde_json::Value> {
    io::silence_advisories();
    inputs::answer(root, question, picked, text, FACADE_WRITE)
}

/// Withdraws `ids`, all of which must be `new`, as `inputs withdraw` does;
/// returns `{"applied", "reopened"}` and refuses a mismatched root the same
/// way as [`inputs_add`].
pub fn inputs_withdraw(root: &Path, ids: &[String]) -> anyhow::Result<serde_json::Value> {
    io::silence_advisories();
    inputs::withdraw(root, ids, FACADE_WRITE)
}

/// Record one hook payload as `agents record <harness>` does. Changes the
/// process's working directory to the payload's `cwd` and fixes the repo root
/// for the rest of the process, so call it once per process.
pub fn record_agent(
    harness: &str,
    payload: &serde_json::Value,
) -> anyhow::Result<serde_json::Value> {
    io::silence_advisories();
    let write_opts = WriteIntegrityArgs {
        allow_outside: false,
        no_write_integrity: false,
        verify_integrity: false,
        strict_integrity: false,
        no_create: false,
    };
    agents::dispatch::record_value(harness, payload, &write_opts)
}

/// Parses the process arguments, runs the selected verb and reports any error
/// on stderr. Returns `ExitCode::FAILURE` on error; clap's own parse-error and
/// `--help` / `--version` exits happen inside `Cli::parse` as before.
pub fn run() -> ExitCode {
    // Parse `Cli` exactly once, here: peeking `--error-format` with a second
    // `try_parse()` swallows clap's errors on the peek path and double-renders
    // `--help` / `--version`. `error_format` is plucked from the parsed struct
    // so `emit_error` still has it after `run()` bails.
    let cli = Cli::parse();
    let error_format = cli.error_format;
    match cli::run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            emit_error(&err, error_format);
            ExitCode::FAILURE
        }
    }
}

fn emit_error(err: &anyhow::Error, fmt: ErrorFormat) {
    match fmt {
        ErrorFormat::Text => {
            // `{:#}` prints the full anyhow cause chain inline; combined
            // with `with_context(…"parsing {}", path)` in `read_toml`, toml's
            // Display impl then emits line:col + caret diagnostics for syntax
            // errors.
            eprintln!("tomlctl: {:#}", err);
        }
        ErrorFormat::Json => {
            // Must be anyhow's inherent `downcast_ref`, not
            // `err.chain().find_map(|e| e.downcast_ref::<TaggedError>())`:
            // `chain()` yields `&dyn Error`, whose downcast sees
            // `ContextError<C, E>` rather than the tag inside it.
            let tagged: Option<&TaggedError> = err.downcast_ref::<TaggedError>();
            let kind = tagged.map(|t| t.kind.as_str()).unwrap_or("other");
            let file = tagged
                .and_then(|t| t.file.as_ref())
                .map(|p| p.to_string_lossy().into_owned());
            let arg = tagged.and_then(|t| t.arg);
            // `{:#}` to match text mode's full-chain rendering, so JSON
            // consumers get the same prose in the `message` field.
            let message = format!("{:#}", err);
            let envelope = serde_json::json!({
                "error": {
                    "kind": kind,
                    "message": message,
                    // Always include the key — consumers can rely on a
                    // stable JSON shape (null when the tag carries no path).
                    "file": file,
                    // The clap id of the argument the error is about, when a
                    // verb tags one; null otherwise.
                    "arg": arg,
                }
            });
            // Ignore write errors on the stderr path — if stderr itself is
            // broken there's nothing reasonable to do, and the process is
            // about to exit 1 regardless.
            let mut stderr = std::io::stderr().lock();
            let _ = serde_json::to_writer(&mut stderr, &envelope);
            let _ = writeln!(stderr);
        }
    }
}
