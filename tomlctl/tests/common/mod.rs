//! Shared helpers for the integration tests under `tomlctl/tests/`, declared
//! once by `tests/it.rs`; each test file reaches it with `use crate::common;`.

#![allow(dead_code)]

use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

static ROOT_ENV: Mutex<()> = Mutex::new(());

/// Holds `TOMLCTL_ROOT` at a sandbox for an in-process facade write, and the
/// lock that serialises every such test in this binary; dropping it removes
/// the variable before releasing the lock.
pub struct RootEnv {
    _lock: MutexGuard<'static, ()>,
}

pub fn pin_root(root: &Path) -> RootEnv {
    let lock = ROOT_ENV.lock().unwrap_or_else(PoisonError::into_inner);
    // SAFETY: every test in this binary that touches TOMLCTL_ROOT does so
    // through this guard, so no other thread reads or writes it meanwhile.
    unsafe { std::env::set_var("TOMLCTL_ROOT", root) };
    RootEnv { _lock: lock }
}

impl Drop for RootEnv {
    fn drop(&mut self) {
        // SAFETY: the lock is still held here; fields drop after this body.
        unsafe { std::env::remove_var("TOMLCTL_ROOT") };
    }
}

/// Create a tempdir, seed `<tempdir>/.claude/ledger.toml` with `initial`,
/// and return both the tempdir (RAII cleanup) and the ledger path.
pub fn seed_ledger(initial: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let claude = dir.path().join(".claude");
    fs::create_dir_all(&claude).unwrap();
    let ledger = claude.join("ledger.toml");
    fs::write(&ledger, initial).unwrap();
    (dir, ledger)
}

/// Seed `<root>/.claude/<name>` with `toml` inside an existing sandbox, so a
/// verb that walks the root sees the ledger under the root it is confined to.
pub fn seed_ledger_in(root: &Path, name: &str, toml: &str) -> PathBuf {
    let claude = root.join(".claude");
    fs::create_dir_all(&claude).unwrap();
    let ledger = claude.join(name);
    fs::write(&ledger, toml).unwrap();
    ledger
}

/// Six-item fixture spanning status ∈ {open, fixed}, severity ∈ {minor,
/// major, critical}, file ∈ {src/a.rs, src/b.rs}, first_flagged in
/// 2026-03 and 2026-04, plus one row carrying `symbol = "old::fn"` for the
/// regex/presence tests. `first_flagged` is a TOML date literal so the
/// query engine's date comparison path is exercised end-to-end.
pub const QUERY_FIXTURE: &str = r#"schema_version = 1

[[items]]
id = "R1"
status = "open"
severity = "minor"
file = "src/a.rs"
category = "style"
first_flagged = 2026-03-10
summary = "trailing whitespace"

[[items]]
id = "R2"
status = "open"
severity = "major"
file = "src/a.rs"
category = "bug"
first_flagged = 2026-04-02
summary = "nil deref"
symbol = "old::fn"

[[items]]
id = "R3"
status = "fixed"
severity = "critical"
file = "src/b.rs"
category = "bug"
first_flagged = 2026-03-25
summary = "panic on empty input"

[[items]]
id = "R4"
status = "open"
severity = "major"
file = "src/b.rs"
category = "perf"
first_flagged = 2026-04-10
summary = "n^2 loop"

[[items]]
id = "R5"
status = "fixed"
severity = "minor"
file = "src/a.rs"
category = "style"
first_flagged = 2026-04-05
summary = "unused import"

[[items]]
id = "R6"
status = "open"
severity = "critical"
file = "src/b.rs"
category = "security"
first_flagged = 2026-04-15
summary = "unsafe block"
"#;

/// Seed the 6-row `QUERY_FIXTURE` and run `tomlctl items list <ledger> <args…>`.
/// Returns stdout as a String. Used by the query-shape suite.
pub fn run_list_query(args: &[&str]) -> String {
    let (dir, ledger) = seed_ledger(QUERY_FIXTURE);
    let mut cmd = Command::cargo_bin("tomlctl").unwrap();
    cmd.env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("items")
        .arg("list")
        .arg(&ledger);
    for a in args {
        cmd.arg(a);
    }
    let out = cmd.write_stdin("").assert().success();
    String::from_utf8_lossy(&out.get_output().stdout).to_string()
}

/// Same as [`run_list_query`] but the caller supplies the fixture bytes.
/// Used where the 6-row default would bias the result (e.g. single-row
/// pluck tests that assert scalar cardinality).
pub fn run_list_query_with(fixture: &str, args: &[&str]) -> String {
    let (dir, ledger) = seed_ledger(fixture);
    let mut cmd = Command::cargo_bin("tomlctl").unwrap();
    cmd.env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("items")
        .arg("list")
        .arg(&ledger);
    for a in args {
        cmd.arg(a);
    }
    let out = cmd.write_stdin("").assert().success();
    String::from_utf8_lossy(&out.get_output().stdout).to_string()
}

/// Extract sorted `id` values from a JSON-array list output. Used by the
/// `where`-predicate suite where the canonical check is "which rows
/// survived the predicate".
pub fn ids_from(stdout: &str) -> Vec<String> {
    let v: serde_json::Value = serde_json::from_str(stdout)
        .unwrap_or_else(|e| panic!("stdout must be JSON: {e}; stdout:\n{stdout}"));
    let arr = v.as_array().expect("list output is a JSON array");
    let mut ids: Vec<String> = arr
        .iter()
        .map(|el| {
            el.get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        })
        .collect();
    ids.sort();
    ids
}

/// Verify the `<ledger>.sha256` sidecar exists, ends with `  <basename>\n`,
/// carries a 64-hex-char digest, and that digest matches a fresh recompute
/// of the live ledger bytes.
pub fn assert_sidecar_matches(ledger: &Path) {
    let sidecar: PathBuf = {
        let mut s = ledger.as_os_str().to_os_string();
        s.push(".sha256");
        PathBuf::from(s)
    };
    assert!(
        sidecar.exists(),
        "sidecar must exist at {}",
        sidecar.display()
    );
    let raw = fs::read_to_string(&sidecar).unwrap();
    let basename = ledger.file_name().unwrap().to_string_lossy();
    assert!(
        raw.ends_with(&format!("  {basename}\n")),
        "sidecar must end with `  <basename>\\n`, got: {raw:?}"
    );
    let hex = raw.split_whitespace().next().unwrap();
    assert_eq!(hex.len(), 64, "digest must be 64 hex chars, got {hex:?}");
    assert!(hex.chars().all(|c| c.is_ascii_hexdigit()));
    // Recompute against the live bytes and compare.
    let bytes = fs::read(ledger).unwrap();
    use sha2::{Digest, Sha256};
    let actual = Sha256::digest(&bytes);
    let mut actual_hex = String::with_capacity(64);
    for b in actual.iter() {
        use std::fmt::Write;
        let _ = write!(actual_hex, "{:02x}", b);
    }
    assert_eq!(
        hex.to_ascii_lowercase(),
        actual_hex,
        "sidecar digest must match recomputed file hash"
    );
}

/// Parse a single-line JSON error envelope from `stderr`. Asserts the
/// envelope has a top-level `error` object carrying `kind`, `message`,
/// and `file` keys (the latter nullable). Returns the inner `error`
/// object so callers can assert on specific fields.
pub fn parse_json_error_envelope(stderr: &str) -> serde_json::Value {
    let line = stderr.trim();
    assert!(
        !line.is_empty(),
        "stderr is empty — expected a JSON error envelope"
    );
    let v: serde_json::Value =
        serde_json::from_str(line).expect("stderr must be a single JSON line");
    let err = v
        .get("error")
        .cloned()
        .expect("envelope must have top-level `error` key");
    assert!(
        err.get("kind").and_then(|k| k.as_str()).is_some(),
        "error.kind must be a string, got: {err}"
    );
    assert!(
        err.get("message").and_then(|m| m.as_str()).is_some(),
        "error.message must be a string, got: {err}"
    );
    assert!(
        err.get("file").is_some(),
        "error.file key must always be present (null when unknown), got: {err}"
    );
    err
}

/*
<!-- SHARED-BLOCK:shipped-gitignore START -->
*/
/// The repository's own rules, verbatim: a fixture that drifted from them
/// would have every ignore-dependent verdict assert against rules nobody
/// ships.
const GITIGNORE: &str = "/.claude/backlog-evidence/**\n\
                         !/.claude/backlog-evidence/*/\n\
                         !/.claude/backlog-evidence/*/.evidence\n";

/// What every evidence rule starts with once the `!` of a negation is
/// stripped.
const EVIDENCE_RULE_PREFIX: &str = "/.claude/backlog-evidence/";

/// [`GITIGNORE`], checked against the evidence rules the repository's own
/// `.gitignore` carries — hand-kept copies, and nothing else would notice
/// them diverging. A checkout is not guaranteed (a vendored crate has no
/// repo root to read), so the check is skipped there rather than failed.
pub(crate) fn shipped_gitignore() -> &'static str {
    let repo_gitignore = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|repo| repo.join(".gitignore"))
        .and_then(|path| fs::read_to_string(path).ok());
    if let Some(text) = repo_gitignore {
        let shipped: String = text
            .lines()
            .filter(|line| {
                line.trim_start_matches('!')
                    .starts_with(EVIDENCE_RULE_PREFIX)
            })
            .map(|line| format!("{line}\n"))
            .collect();
        assert_eq!(
            shipped, GITIGNORE,
            "the sandbox fixture and the evidence rules the repository ships have diverged"
        );
    }
    GITIGNORE
}
/*
<!-- SHARED-BLOCK:shipped-gitignore END -->
*/

/// A throwaway repo root with `.claude/` and the evidence ignore rules in
/// place. The `TempDir` is returned so the caller keeps it alive for the whole
/// test — dropping it deletes the tree.
pub fn sandbox() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    // `TOMLCTL_ROOT` is canonicalised by the binary, so canonicalise here too
    // or every emitted path fails to relativise against it.
    let root = dir.path().canonicalize().unwrap();
    fs::create_dir_all(root.join(".claude")).unwrap();
    fs::write(root.join(".gitignore"), shipped_gitignore()).unwrap();
    // A git that never ran at all stays tolerated: the tests that need a repo
    // gate on `git_available`, the rest do not care.
    if let Some(template) = git_init_template() {
        copy_tree(&template.join(".git"), &root.join(".git"));
    }
    (dir, root)
}

/// A directory holding what `git init -q .` creates on this machine, built
/// once per git install and global config, or `None` when git cannot be
/// started. Copying it saves each test a git process.
fn git_init_template() -> Option<PathBuf> {
    static TEMPLATE: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
    TEMPLATE.get_or_init(build_git_init_template).clone()
}

fn build_git_init_template() -> Option<PathBuf> {
    let base = Path::new(env!("CARGO_TARGET_TMPDIR"));
    let template = base.join(format!("git-init-template-{}", git_install_key()));
    if template.join(".git").is_dir() {
        return Some(template);
    }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos());
    let building = base.join(format!(
        "git-init-template-building-{}-{nanos}",
        std::process::id()
    ));
    fs::create_dir_all(&building).unwrap();
    // An init that ran and failed would surface much later as an unrelated
    // tracked-count assertion, so it is caught here instead.
    let Ok(init) = git_command(&building).args(["init", "-q", "."]).output() else {
        let _ = fs::remove_dir_all(&building);
        return None;
    };
    if !init.status.success() || !building.join(".git").is_dir() {
        let _ = fs::remove_dir_all(&building);
    }
    assert!(
        init.status.success(),
        "`git init` failed in {}: {}",
        building.display(),
        String::from_utf8_lossy(&init.stderr)
    );
    assert!(
        building.join(".git").is_dir(),
        "`git init` reported success but {} has no .git directory",
        building.display()
    );
    // Another test process may publish the same template first, and a scanner
    // can hold the fresh directory open for a moment, refusing the rename.
    let mut last_error = None;
    for _ in 0..20 {
        match fs::rename(&building, &template) {
            Ok(()) => return Some(template),
            Err(_) if template.join(".git").is_dir() => {
                let _ = fs::remove_dir_all(&building);
                return Some(template);
            }
            Err(error) => last_error = Some(error),
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    panic!(
        "could not publish the git init template {} as {}: {:?}",
        building.display(),
        template.display(),
        last_error
    );
}

/// A digest of the git executable on PATH and the user's global config, read
/// without starting a process, so an upgrade or a config edit gets a fresh
/// template.
fn git_install_key() -> String {
    let exe_name = format!("git{}", std::env::consts::EXE_SUFFIX);
    let git_exe = std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .map(|dir| dir.join(&exe_name))
            .find(|candidate| candidate.is_file())
    });
    let home = std::env::var_os("HOME")
        .filter(|home| !home.is_empty())
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from);
    let modified_nanos = |meta: &fs::Metadata| {
        meta.modified()
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |since| since.as_nanos())
    };
    let mut key = String::new();
    if let Some(exe) = &git_exe {
        key.push_str(&exe.to_string_lossy());
        if let Ok(meta) = fs::metadata(exe) {
            key.push_str(&format!("|{}|{}", meta.len(), modified_nanos(&meta)));
        }
    }
    if let Some(meta) = home.and_then(|home| fs::metadata(home.join(".gitconfig")).ok()) {
        key.push_str(&format!("|gitconfig {}", modified_nanos(&meta)));
    }
    hex_digest(key.as_bytes())[..16].to_string()
}

/// Copy `from` into `to` directory by directory, creating the empty ones too:
/// git does not recognise a `.git` that lacks `objects/` or `refs/`.
fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target).unwrap();
        }
    }
}

pub fn store_path(root: &Path) -> PathBuf {
    root.join(".claude").join("backlog.toml")
}

/// Flow slug every task-store fixture is staged under. It satisfies the
/// `--slug` regex, so `tasks … --slug TASKS_SLUG` resolves to the file
/// [`seed_tasks`] writes.
pub const TASKS_SLUG: &str = "fixture-tasks-flow";

/// The sibling `flow init` leaves beside a store. Its `plan_path` is the one
/// `--slug` resolves `import-plan`, `render` and `check --plan` through — a
/// `plan_path` inside the store is never consulted there — and without the
/// file `--slug` refuses those verbs with `kind=not_found` before any plan is
/// read. It ends on a table, so a caller can append an `[artifacts]` one.
pub fn tasks_context(slug: &str, plan_path: &str) -> String {
    format!(
        "slug = \"{slug}\"\n\
         plan_path = \"{plan_path}\"\n\
         status = \"in-progress\"\n\
         created = 2026-09-07\n\
         updated = 2026-09-07\n\
         scope = []\n\
         \n\
         [tasks]\n\
         total = 0\n\
         completed = 0\n\
         in_progress = 0\n"
    )
}

/// Stage `<root>/.claude/flows/<slug>/context.toml` recording `plan_path`, and
/// the store beside it when `store` is given. The store path comes back either
/// way, since an import creates the file there.
pub fn stage_tasks_flow(root: &Path, slug: &str, plan_path: &str, store: Option<&str>) -> PathBuf {
    let flow = root.join(".claude").join("flows").join(slug);
    fs::create_dir_all(&flow).unwrap();
    fs::write(flow.join("context.toml"), tasks_context(slug, plan_path)).unwrap();
    let path = flow.join("tasks.toml");
    if let Some(toml) = store {
        write_tasks_store(&path, toml);
    }
    path
}

/// Write a store and a digest over its bytes. The sidecar is written because a
/// fixture without one — or with a stale one — reads as an integrity failure
/// under `--verify-integrity`, which would look like a product bug.
pub fn write_tasks_store(store: &Path, toml: &str) {
    fs::write(store, toml).unwrap();
    refresh_sidecar(store);
}

/// [`stage_tasks_flow`] under [`TASKS_SLUG`], recording
/// `docs/plans/<TASKS_SLUG>.md`.
pub fn seed_tasks(root: &Path, toml: &str) -> PathBuf {
    stage_tasks_flow(
        root,
        TASKS_SLUG,
        &format!("docs/plans/{TASKS_SLUG}.md"),
        Some(toml),
    )
}

pub fn cli(root: &Path) -> Command {
    let mut cmd = Command::cargo_bin("tomlctl").unwrap();
    cmd.env("TOMLCTL_ROOT", root)
        .env("TOMLCTL_LOCK_TIMEOUT", "5");
    cmd
}

/// The built binary as a real process, for a test that asserts what separate
/// processes do to each other.
pub fn tomlctl_process() -> assert_cmd::Command {
    assert_cmd::Command::cargo_bin("tomlctl").unwrap()
}

/// The variables an in-process run takes as its overlay. Setting any other
/// one, or removing one the test process has, sends the command to a child
/// process, since the in-process run reads the rest of its environment from
/// the test process.
const OVERLAY_KEYS: [&str; 4] = [
    "TOMLCTL_ROOT",
    "TOMLCTL_LOCK_TIMEOUT",
    "TOMLCTL_NO_DEDUP_ID",
    "CLAUDE_CONFIG_DIR",
];

/// A tomlctl command shaped like `assert_cmd::Command`. It runs inside the
/// test process through `tomlctl::run_with` unless it sets a working
/// directory, changes a variable outside [`OVERLAY_KEYS`] or runs `agents
/// record`, which moves its process into the payload's `cwd`; those start the
/// built binary, configured call for call as the in-process path is.
#[derive(Debug)]
pub struct Command {
    child: assert_cmd::Command,
    is_tomlctl: bool,
    args: Vec<OsString>,
    env: Vec<(OsString, Option<OsString>)>,
    sets_current_dir: bool,
    foreign_env: bool,
    stdin: Vec<u8>,
}

impl Command {
    pub fn cargo_bin<S: AsRef<str>>(name: S) -> Result<Self, assert_cmd::cargo::CargoError> {
        Ok(Self {
            child: assert_cmd::Command::cargo_bin(name.as_ref())?,
            is_tomlctl: name.as_ref() == "tomlctl",
            args: Vec::new(),
            env: Vec::new(),
            sets_current_dir: false,
            foreign_env: false,
            stdin: Vec::new(),
        })
    }

    pub fn arg<S: AsRef<OsStr>>(&mut self, arg: S) -> &mut Self {
        self.child.arg(arg.as_ref());
        self.args.push(arg.as_ref().to_owned());
        self
    }

    pub fn args<I, S>(&mut self, args: I) -> &mut Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        for arg in args {
            self.arg(arg);
        }
        self
    }

    pub fn env<K: AsRef<OsStr>, V: AsRef<OsStr>>(&mut self, key: K, val: V) -> &mut Self {
        self.child.env(key.as_ref(), val.as_ref());
        self.note_env(key.as_ref(), Some(val.as_ref().to_owned()));
        self
    }

    pub fn env_remove<K: AsRef<OsStr>>(&mut self, key: K) -> &mut Self {
        self.child.env_remove(key.as_ref());
        self.note_env(key.as_ref(), None);
        self
    }

    pub fn current_dir<P: AsRef<Path>>(&mut self, dir: P) -> &mut Self {
        self.child.current_dir(dir);
        self.sets_current_dir = true;
        self
    }

    pub fn write_stdin<S: Into<Vec<u8>>>(&mut self, buffer: S) -> &mut Self {
        let buffer = buffer.into();
        self.child.write_stdin(buffer.clone());
        self.stdin = buffer;
        self
    }

    /// Whether [`Command::output`] and [`Command::assert`] would run this
    /// command in the test process rather than start the binary.
    pub fn runs_in_process(&self) -> bool {
        let records_agent = self
            .args
            .windows(2)
            .any(|pair| pair[0] == "agents" && pair[1] == "record");
        self.is_tomlctl && !self.sets_current_dir && !self.foreign_env && !records_agent
    }

    pub fn output(&mut self) -> std::io::Result<std::process::Output> {
        if !self.runs_in_process() {
            return self.child.output();
        }
        let argv = std::iter::once(OsString::from(env!("CARGO_BIN_EXE_tomlctl")))
            .chain(self.args.iter().cloned());
        let outcome = tomlctl::run_with(argv, &self.env, &self.stdin);
        Ok(std::process::Output {
            status: exit_status(outcome.code),
            stdout: outcome.stdout,
            stderr: outcome.stderr,
        })
    }

    #[must_use]
    #[track_caller]
    pub fn assert(&mut self) -> assert_cmd::assert::Assert {
        if !self.runs_in_process() {
            return self.child.assert();
        }
        let output = self.output().expect("an in-process run returns its output");
        assert_cmd::assert::Assert::new(output)
            .append_context("command", format!("{:?} (in process)", self.child))
    }

    /// Removing a variable the test process lacks changes nothing a child
    /// would see, so only that case stays in-process.
    fn note_env(&mut self, key: &OsStr, value: Option<OsString>) {
        let overlay = OVERLAY_KEYS.iter().any(|known| key == *known);
        if !overlay && (value.is_some() || std::env::var_os(key).is_some()) {
            self.foreign_env = true;
        }
        self.env.retain(|(name, _)| name.as_os_str() != key);
        self.env.push((key.to_owned(), value));
    }
}

#[cfg(windows)]
fn exit_status(code: i32) -> std::process::ExitStatus {
    use std::os::windows::process::ExitStatusExt;
    std::process::ExitStatus::from_raw(code as u32)
}

#[cfg(unix)]
fn exit_status(code: i32) -> std::process::ExitStatus {
    use std::os::unix::process::ExitStatusExt;
    std::process::ExitStatus::from_raw(code << 8)
}

pub fn stdout_json(assert: &assert_cmd::assert::Assert) -> serde_json::Value {
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).to_string();
    serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("stdout must be JSON: {e}; stdout:\n{stdout}"))
}

pub fn hex_digest(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Run `tomlctl backlog <args…>`, require success, and hand back stdout.
pub fn backlog_stdout(root: &Path, args: &[&str]) -> String {
    let out = cli(root)
        .arg("backlog")
        .args(args)
        .write_stdin("")
        .assert()
        .success();
    String::from_utf8_lossy(&out.get_output().stdout).to_string()
}

/// Run `tomlctl backlog <args…>`, require success, and parse stdout as JSON.
pub fn backlog(root: &Path, args: &[&str]) -> serde_json::Value {
    let stdout = backlog_stdout(root, args);
    serde_json::from_str(stdout.trim()).unwrap_or_else(|e| {
        panic!(
            "`backlog {}` stdout must be JSON: {e}; got: {stdout}",
            args.join(" ")
        )
    })
}

/// A `git` command rooted at `dir` that ignores any repository a running git
/// hook exported through the environment.
pub fn git_command(dir: &Path) -> std::process::Command {
    let mut cmd = std::process::Command::new("git");
    cmd.current_dir(dir)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_OBJECT_DIRECTORY");
    cmd
}

pub fn git_available() -> bool {
    git_command(Path::new("."))
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// The first bash that runs: the `SHELL_GATE_BASH` override, `bash` from PATH
/// (not on Windows, where a bare `bash` resolves to System32's WSL launcher),
/// the Git for Windows bash found from `git --exec-path`, then its default
/// install location.
pub fn git_bash() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = std::env::var_os("SHELL_GATE_BASH")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .into_iter()
        .collect();
    if !cfg!(windows) {
        candidates.push(PathBuf::from("bash"));
    }
    let exec_path = git_command(Path::new("."))
        .arg("--exec-path")
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string());
    // Git for Windows reports `<root>/mingw64/libexec/git-core`.
    if let Some(root) = exec_path
        .as_deref()
        .map(Path::new)
        .and_then(Path::parent)
        .and_then(Path::parent)
        .and_then(Path::parent)
    {
        candidates.push(root.join("bin").join("bash.exe"));
    }
    candidates.push(PathBuf::from(r"C:\Program Files\Git\bin\bash.exe"));
    candidates.into_iter().find(|exe| {
        std::process::Command::new(exe)
            .arg("--version")
            .output()
            .is_ok_and(|out| out.status.success())
    })
}

/// Rewrite the row's terminal date to one far enough back that any realistic
/// `--older-than` fires. The clock cannot be moved and a same-day terminal
/// date is zero days old, so the fixture is aged instead.
pub fn age_terminal_date(store: &Path, field: &str) {
    let text = fs::read_to_string(store).unwrap();
    let needle = format!("{field} = ");
    let mut hit = false;
    let aged: Vec<String> = text
        .lines()
        .map(|line| {
            if line.starts_with(&needle) {
                hit = true;
                format!("{needle}2020-01-01")
            } else {
                line.to_string()
            }
        })
        .collect();
    assert!(hit, "no `{field} = …` line to age in:\n{text}");
    fs::write(store, format!("{}\n", aged.join("\n"))).unwrap();
    refresh_sidecar(store);
}

/// Rewrite `<file>.sha256` over the live bytes, in the same
/// `<hex>  <basename>\n` shape [`assert_sidecar_matches`] checks. A fixture
/// edited out of band otherwise leaves a stale digest behind, so any later
/// `--verify-integrity` read would fail as an apparent product bug.
fn refresh_sidecar(file: &Path) {
    use sha2::{Digest, Sha256};
    use std::fmt::Write;
    let bytes = fs::read(file).unwrap();
    let mut hex = String::with_capacity(64);
    for b in Sha256::digest(&bytes).iter() {
        let _ = write!(hex, "{:02x}", b);
    }
    let sidecar: PathBuf = {
        let mut s = file.as_os_str().to_os_string();
        s.push(".sha256");
        PathBuf::from(s)
    };
    let basename = file.file_name().unwrap().to_string_lossy();
    fs::write(&sidecar, format!("{hex}  {basename}\n")).unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run `record` with an `agents record` payload whose `cwd` is another
    /// existing directory, and require this process's working directory
    /// unchanged afterwards.
    fn record_agent_elsewhere(record: impl FnOnce(String)) {
        let elsewhere = tempfile::tempdir().unwrap();
        let payload = serde_json::json!({
            "hook_event_name": "Notification",
            "session_id": "s",
            "agent_id": "a",
            "cwd": elsewhere.path(),
        });
        let before = std::env::current_dir().unwrap();
        record(payload.to_string());
        assert_eq!(std::env::current_dir().unwrap(), before);
    }

    #[test]
    fn current_dir_runs_a_child() {
        let (_dir, root) = sandbox();
        let mut cmd = cli(&root);
        assert!(cmd.runs_in_process());
        cmd.current_dir(&root);
        assert!(!cmd.runs_in_process());
    }

    #[test]
    fn foreign_env_runs_a_child() {
        let (_dir, root) = sandbox();
        let mut set = cli(&root);
        set.env("GIT_DIR", root.join(".git"));
        assert!(!set.runs_in_process());
        let present = std::env::vars_os()
            .map(|(name, _)| name)
            .find(|name| !OVERLAY_KEYS.iter().any(|known| name == *known))
            .expect("the test process has a variable outside the overlay");
        let mut removed = cli(&root);
        removed.env_remove(&present);
        assert!(!removed.runs_in_process());
    }

    #[test]
    fn removing_an_absent_variable_runs_in_process() {
        let (_dir, root) = sandbox();
        let absent = "TOMLCTL_TEST_NEVER_SET_7F3A";
        assert!(std::env::var_os(absent).is_none());
        let mut removed = cli(&root);
        removed.env_remove(absent);
        assert!(removed.runs_in_process());
        let out = removed.arg("--version").assert().success();
        assert!(String::from_utf8_lossy(&out.get_output().stdout).contains("tomlctl"));
        let mut set = cli(&root);
        set.env(absent, "1");
        assert!(!set.runs_in_process());
    }

    #[test]
    fn tomlctl_process_runs_a_child() {
        let (_dir, root) = sandbox();
        record_agent_elsewhere(|payload| {
            tomlctl_process()
                .env("TOMLCTL_ROOT", &root)
                .args(["agents", "record", "--harness", "claude-code"])
                .write_stdin(payload)
                .output()
                .unwrap();
        });
    }

    #[test]
    fn agents_record_runs_a_child() {
        let (_dir, root) = sandbox();
        let mut cmd = cli(&root);
        cmd.args(["agents", "record", "--harness", "claude-code"]);
        assert!(!cmd.runs_in_process());
        record_agent_elsewhere(|payload| {
            cmd.write_stdin(payload).output().unwrap();
        });
    }

    #[test]
    fn overlay_keys_run_in_process() {
        let (_dir, root) = sandbox();
        let mut cmd = cli(&root);
        cmd.env("TOMLCTL_NO_DEDUP_ID", "1")
            .env("CLAUDE_CONFIG_DIR", root.join("config"))
            .env_remove("TOMLCTL_LOCK_TIMEOUT");
        assert!(cmd.runs_in_process());
        let out = cmd.arg("--version").assert().success();
        assert!(String::from_utf8_lossy(&out.get_output().stdout).contains("tomlctl"));
    }
}
