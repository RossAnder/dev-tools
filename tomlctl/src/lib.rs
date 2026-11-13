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
mod fields;
mod flow;
mod inputs;
mod integrity;
mod invocation;
mod io;
mod items;
mod items_sweep;
mod json;
mod ledgers;
mod orphans;
mod output;
mod owner;
mod payload;
mod plan;
mod query;
mod repo_files;
mod repo_root;
mod review;
mod store_guard;
mod sweep;
mod tasks;
#[cfg(test)]
mod test_support;
mod time;
mod union_find;

use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Parser;

use crate::cli::{Cli, ErrorFormat, ReadIntegrityArgs, WriteIntegrityArgs};
use crate::errors::TaggedError;
use crate::ledgers::FACADE_WRITE;

pub use crate::backlog::focus::{FocusChange as BacklogFocusChange, FocusOp as BacklogFocus};
pub use crate::backlog::schema::MANAGED_FIELDS as BACKLOG_MANAGED_FIELDS;
pub use crate::backlog::schema::TIERS as BACKLOG_TIERS;
pub use crate::backlog::triage::BacklogTriage;
pub use crate::items::STATUS_COMPANIONS;
pub use crate::ledgers::{LedgerKind, LedgerRef, RestoreRow};

/// The stack [`run`] needs. Debug builds of clap's derived command tree take
/// close to 1 MB, the whole of a Windows main thread, so callers run it on a
/// thread sized like a Linux main thread instead.
pub const CLI_STACK_BYTES: usize = 8 * 1024 * 1024;

/// The files of a flow directory that [`snapshot`] reads, store first. Its
/// `revision` hashes them in this order, so a poller fingerprinting exactly
/// these files sees every change the snapshot can.
pub const SNAPSHOT_INPUTS: [&str; 5] = [
    "tasks.toml",
    "execution-record.toml",
    "agents.toml",
    "context.toml",
    "plan.toml",
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

/// `{"files": [...]}`: each brief, design file, note, run note and settle note
/// under `<root>/docs/flows/<slug>`, with its kind, id, headline, state, task
/// and sources, sorted by path. A file resolving outside that directory is
/// left out, and a missing directory lists no files.
pub fn authored_list(root: &Path, slug: &str) -> anyhow::Result<serde_json::Value> {
    io::silence_advisories();
    plan::authored::list(root, slug)
}

/// `{"path", "text"}` for one path [`authored_list`] would carry, the text
/// LF-normalised; any other path, or one resolving outside the flow's
/// directory, is refused.
pub fn authored_read(root: &Path, slug: &str, path: &str) -> anyhow::Result<serde_json::Value> {
    io::silence_advisories();
    plan::authored::read(root, slug, path)
}

/// Finding `id` of the v2 review store at the repo-relative `store`, such as
/// `.claude/flows/<slug>/review-v2.toml`, as `review show --for report` prints
/// it, read without integrity checks. A store path that is absolute, holds
/// `..` or resolves outside `<root>/.claude` is refused.
pub fn review_finding(root: &Path, store: &str, id: &str) -> anyhow::Result<serde_json::Value> {
    io::silence_advisories();
    review::facade::finding(root, store, id)
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
    let write_opts = backlog_facade_write();
    let out =
        backlog::triage::triage_value(root, ids, &triage.into(), Some(expect_status), &write_opts)?;
    Ok(serde_json::json!({
        "applied": out["applied"],
        "skipped_stale": out["skipped_stale"],
    }))
}

/// The write options of a facade write to the backlog: sidecar kept current,
/// never verified, and a missing store refused rather than created.
fn backlog_facade_write() -> WriteIntegrityArgs {
    WriteIntegrityArgs {
        allow_outside: false,
        no_write_integrity: false,
        verify_integrity: false,
        strict_integrity: false,
        no_create: true,
    }
}

/// Sets `tier` (removes it, for `None`) on each of `ids` in
/// `<root>/.claude/backlog.toml` whose fields still match `expect`, with
/// `{"tier": null}` expecting an untiered row. Returns what
/// [`ledger_transition`] does and refuses a mismatched root the same way.
pub fn backlog_set_tier(
    root: &Path,
    ids: &[String],
    tier: Option<&str>,
    expect: serde_json::Map<String, serde_json::Value>,
) -> anyhow::Result<serde_json::Value> {
    io::silence_advisories();
    io::ensure_process_root(root)?;
    ledgers::set_backlog_tier(root, ids, tier, expect)
}

/// Applies `op` to the backlog's focus queue in one locked write, as
/// `backlog focus` does, provided the stored live queue is exactly `expect`
/// (`(id, focus)` pairs; unqueued live rows may be omitted). Otherwise writes
/// nothing and lists each difference in `skipped_stale`. Returns `{applied,
/// skipped_stale, changed: [{id, before, after}]}`, `applied` naming every
/// changed row, shifted neighbours included.
pub fn backlog_focus(
    root: &Path,
    op: BacklogFocus,
    expect: Vec<(String, Option<i64>)>,
) -> anyhow::Result<serde_json::Value> {
    io::silence_advisories();
    io::ensure_process_root(root)?;
    let out = backlog::focus::apply(root, &op, Some(&expect), &backlog_facade_write(), false)?;
    Ok(serde_json::json!({
        "applied": out["applied"],
        "skipped_stale": out["skipped_stale"],
        "changed": out["changed"],
    }))
}

/// The `focus` changes [`backlog_focus`] would make to `queue`, the live rows
/// as `(id, focus)` pairs, without touching any store.
pub fn backlog_focus_plan(
    queue: &[(String, Option<i64>)],
    op: &BacklogFocus,
) -> anyhow::Result<Vec<BacklogFocusChange>> {
    backlog::focus::plan(queue, op)
}

/// Refuses `root` unless it is tomlctl's process root, with the `root
/// mismatch` error every facade write refuses with. Without `TOMLCTL_ROOT`,
/// the first resolution in a process caches the root found from the current
/// directory, so this call pins it if nothing has yet.
pub fn ensure_process_root(root: &Path) -> anyhow::Result<()> {
    io::ensure_process_root(root)
}

/// The top level `git rev-parse --show-toplevel` would print for `start`, found
/// without spawning git: `Some(Some(root))` inside a repo, `Some(None)` outside
/// any, and `None` when only git can answer. A drive-letter root loses the
/// verbatim `\\?\` prefix canonicalisation gives it on Windows.
pub fn discover_repo_root(start: &Path) -> Option<Option<std::path::PathBuf>> {
    match repo_root::discover(start, |k| std::env::var_os(k).is_some()) {
        repo_root::Discovery::Root(root) => Some(Some(strip_verbatim_drive(root))),
        repo_root::Discovery::NoRepo => Some(None),
        repo_root::Discovery::AskGit => None,
    }
}

fn strip_verbatim_drive(path: std::path::PathBuf) -> std::path::PathBuf {
    let text = path.to_string_lossy();
    match text.strip_prefix(r"\\?\") {
        Some(rest) if rest.as_bytes().get(1) == Some(&b':') => std::path::PathBuf::from(rest),
        _ => path,
    }
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
/// on stderr. Returns `ExitCode::FAILURE` on error, except that a reader
/// closing stdout early (`tomlctl … | head`) exits quietly with success and a
/// verb's `ExitWith` exits quietly with its own code; clap's
/// own parse-error and `--help` / `--version` exits happen inside
/// `Cli::parse_from`.
pub fn run() -> ExitCode {
    // Parse `Cli` exactly once, here: peeking `--error-format` with a second
    // `try_parse()` swallows clap's errors on the peek path and double-renders
    // `--help` / `--version`. `error_format` is plucked from the parsed struct
    // so `emit_error` still has it after `run()` bails.
    let args: Vec<OsString> = std::env::args_os().collect();
    let cli = Cli::parse_from(&args);
    let error_format = cli.error_format;
    match cli::run(cli, &args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) if output::is_stdout_closed(&err) => ExitCode::SUCCESS,
        Err(err) => match invocation::exit_code(&err) {
            Some(code) => ExitCode::from(code as u8),
            None => {
                emit_error(&err, error_format);
                ExitCode::FAILURE
            }
        },
    }
}

/// What one in-process run gives back: the code the binary would exit with
/// and everything the run wrote to stdout and stderr.
#[doc(hidden)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub code: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

/// Run one command inside this process as the binary would run it. `args`
/// starts with the program name, which clap puts in the usage line; an `env`
/// pair with `None` reads as unset for this run whatever the process holds.
/// Child processes and the working directory are the process's own.
#[doc(hidden)]
pub fn run_with<I, S>(args: I, env: &[(OsString, Option<OsString>)], stdin: &[u8]) -> Outcome
where
    I: IntoIterator<Item = S>,
    S: Into<OsString> + Clone,
{
    let args: Vec<OsString> = args.into_iter().map(Into::into).collect();
    let mut context = invocation::Invocation::new().stdin(stdin);
    for (name, value) in env {
        context = match value {
            Some(value) => context.env(name.clone(), value.clone()),
            None => context.unset(name.clone()),
        };
    }
    // The command tree needs the binary's stack, and a fresh thread keeps the
    // context away from whatever the caller's thread has installed.
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .name("tomlctl-run".to_string())
            .stack_size(CLI_STACK_BYTES)
            .spawn_scoped(scope, || run_in_context(context, &args))
            .expect("spawn a thread for an in-process run")
            .join()
            .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
    })
}

/// Install `context`, run the command under it and collect its output. A
/// panic is caught here so the output written before it is kept.
fn run_in_context(context: invocation::Invocation, args: &[OsString]) -> Outcome {
    let guard = context.install();
    let code = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run_args(args))) {
        Ok(code) => code,
        Err(panic) => {
            let message = panic
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| panic.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "a non-string panic payload".to_string());
            invocation::stderr_line(format_args!("tomlctl: panicked: {message}"));
            101
        }
    };
    let captured = guard.finish();
    Outcome {
        code,
        stdout: captured.stdout,
        stderr: captured.stderr,
    }
}

/// `run`'s parse, dispatch and error report, with clap's own print-and-exit
/// replaced by writing to the installed context and returning the code.
fn run_args(args: &[OsString]) -> i32 {
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(err) => {
            let rendered = err.render().to_string();
            if err.use_stderr() {
                // Writing to the context's buffer cannot fail.
                let _ = invocation::stderr().write_all(rendered.as_bytes());
            } else {
                invocation::with_stdout_buffer(|buf| buf.extend_from_slice(rendered.as_bytes()));
            }
            return err.exit_code();
        }
    };
    let error_format = cli.error_format;
    match cli::run(cli, args) {
        Ok(()) => 0,
        Err(err) if output::is_stdout_closed(&err) => 0,
        Err(err) => invocation::exit_code(&err).unwrap_or_else(|| {
            emit_error(&err, error_format);
            1
        }),
    }
}

fn emit_error(err: &anyhow::Error, fmt: ErrorFormat) {
    match fmt {
        ErrorFormat::Text => {
            // `{:#}` prints the full anyhow cause chain inline; combined
            // with `with_context(…"parsing {}", path)` in `read_toml`, toml's
            // Display impl then emits line:col + caret diagnostics for syntax
            // errors.
            invocation::stderr_line(format_args!("tomlctl: {:#}", err));
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
            let mut stderr = invocation::stderr();
            let _ = serde_json::to_writer(&mut stderr, &envelope);
            let _ = writeln!(stderr);
        }
    }
}
