//! Pre-seed Claude Code workspace-trust for a spawned PTY session's cwd.
//!
//! Claude Code shows a one-time interactive **"Do you trust the files in this
//! folder?"** dialog the first time it is launched in an unfamiliar directory.
//! That dialog is TUI-only (never surfaced over JSONL), is gated BEFORE
//! permission evaluation, and — unlike the `bypassPermissions` warning we
//! suppress via `--settings skipDangerousModePermissionPrompt` (see
//! [`super::pty_transport`]) — has NO per-run flag, env var, or settings key to
//! skip it. `--permission-mode bypassPermissions` does NOT cover trust, and
//! `-p`/print mode (the only flag that skips it) abandons the interactive PTY
//! the supervisor is built around. lumina spawns each sprint into a FRESH git
//! worktree — a brand-new cwd — so every autonomous launch would otherwise
//! block on this dialog.
//!
//! The only mechanism that suppresses the dialog for an INTERACTIVE session is
//! the trust store itself: `~/.claude.json`'s per-project `hasTrustDialogAccepted`
//! flag — the exact field claude persists when a human clicks "Yes, proceed".
//! So lumina pre-seeds that entry for the cwd immediately before spawn. Doing it
//! on EVERY spawn (idempotent — a no-op once set) self-heals across a Claude
//! Code update that resets stored acceptance, mirroring the robustness the
//! `--settings` flag gives the bypass dialog.
//!
//! Cleanup is MANUAL and BOUNDED BY the operator-run `lumina prune-trust` sweep:
//! [`prune_orphaned_worktree_trusts`] sweeps entries UNDER the lumina worktree
//! root whose directory no longer exists on disk (and [`remove_dir_trust`] drops
//! a single entry), but NO worktree-teardown path invokes either automatically —
//! a removed worktree's trust entry lingers in `~/.claude.json` until the next
//! `lumina prune-trust`. Note the scope asymmetry: the seeder trusts ANY spawn
//! cwd, while the pruner only sweeps entries under `.lumina/worktrees/`, so a
//! non-worktree cwd's entry is not auto-reclaimable even by that sweep.
//!
//! ## Robustness contract
//!
//! - **Best-effort at the call site.** A failure is logged and the spawn
//!   proceeds (the dialog then appears, exactly as before this module) — trust
//!   pre-seeding never aborts a launch.
//! - **Never clobber.** A missing store is created; an unreadable or malformed
//!   (non-JSON / non-object) store is left UNTOUCHED and the error returned — we
//!   never overwrite a file we could not parse, so a corrupt `~/.claude.json` is
//!   reported, not destroyed.
//! - **Atomic write.** The store is rewritten via a sibling temp file + rename,
//!   so a crash/AV-lock mid-write leaves the original intact.
//! - **Idempotent.** [`ensure_dir_trusted`] writes only when the entry is not
//!   already trusted, so the one-time key-reordering cost of a full reserialize
//!   (`serde_json` is not built with `preserve_order`) is paid at most once per
//!   worktree, not on every spawn.
//! - **Serialised in-process.** A process-global lock is held across the whole
//!   read→modify→write of [`ensure_dir_trusted_at`], so two concurrent spawns
//!   into different fresh worktrees cannot both read the same snapshot and have
//!   the second rename drop the first's trust write (lost update). The atomic
//!   rename prevents torn files, not lost updates; this lock closes that
//!   in-process window. A residual cross-process race (an external `claude`
//!   rewriting the store between our read and rename) is accepted, not fixed.
//! - **Path-injected core.** The `*_at` fns take the store path explicitly so
//!   unit tests drive a tempfile with no process-global env mutation (race-free
//!   under both `cargo test` and nextest).

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde_json::{Map, Value};

/// Process-global lock serialising the read→modify→write of the shared trust
/// store across concurrent in-process [`ensure_dir_trusted_at`] calls (e.g. two
/// simultaneous spawns into different fresh worktrees). Held across the WHOLE
/// critical section, so two callers can never both read the same snapshot and
/// have the second rename clobber the first's `hasTrustDialogAccepted` write —
/// the atomic rename prevents torn files, NOT lost updates (R1).
///
/// RESIDUAL cross-process race (accepted, documented, NOT fixed here): an
/// external `claude` process can still rewrite the store between our read and
/// our rename — claude itself does not lock — so the cross-process lost-update
/// window is minimal but non-zero. This lock closes the in-process vector only.
/// The guarded data is `()`, so a poisoned lock carries no corrupt state.
static STORE_WRITE_LOCK: Mutex<()> = Mutex::new(());

/// Env override for the trust-store path. Tests point it at a tempdir; prod
/// resolves `~/.claude.json`. Precedent: `LUMINA_CLAUDE_BIN`,
/// `LUMINA_PTY_PROJECTS_ROOT`, `LUMINA_WORKTREE_ROOT`.
const CLAUDE_JSON_ENV: &str = "LUMINA_CLAUDE_JSON";

/// Resolve the `~/.claude.json` trust store. `LUMINA_CLAUDE_JSON` override →
/// else `%USERPROFILE%\.claude.json` (Windows) / `$HOME/.claude.json` (Unix).
///
/// Returns `None` when the home env var is unset — we NEVER fall back to the
/// CWD (writing a stray `.claude.json` into a repo/worktree would be wrong and
/// confusing). Home-based, matching lumina's existing
/// [`lumina_core::jsonl_tail::resolve_projects_root`] assumption (`~/.claude/projects`).
pub fn claude_json_path() -> Option<PathBuf> {
    if let Some(v) = std::env::var_os(CLAUDE_JSON_ENV) {
        return Some(PathBuf::from(v));
    }
    #[cfg(target_os = "windows")]
    let home_var = "USERPROFILE";
    #[cfg(not(target_os = "windows"))]
    let home_var = "HOME";
    std::env::var_os(home_var).map(|home| PathBuf::from(home).join(".claude.json"))
}

/// Compute the `projects`-map KEY claude uses for `cwd`: the absolute path with
/// the Windows verbatim prefix (`\\?\`) stripped, back-slashes folded to
/// forward slashes, and any trailing slash removed.
///
/// Matches the keys claude itself writes — verified against the on-disk
/// `~/.claude.json` (`C:/Users/rossa/dev/dev-tools`). claude derives its key
/// from `process.cwd()`, which is exactly the path lumina passes via
/// `cmd.cwd(config.cwd)`, so the forward-slashed form lines up.
pub fn project_key(cwd: &Path) -> String {
    let raw = cwd.to_string_lossy();
    let stripped = raw.strip_prefix(r"\\?\").unwrap_or(&raw);
    let fwd = stripped.replace('\\', "/");
    fwd.strip_suffix('/').unwrap_or(&fwd).to_string()
}

/// Ensure `cwd` is recorded trusted in the resolved trust store.
///
/// Best-effort wrapper over [`ensure_dir_trusted_at`] that resolves
/// [`claude_json_path`] first; `Err(NotFound)` when the home dir is unresolved.
pub fn ensure_dir_trusted(cwd: &Path) -> io::Result<bool> {
    let path = claude_json_path().ok_or_else(home_unresolved)?;
    ensure_dir_trusted_at(&path, cwd)
}

/// Path-injected core of [`ensure_dir_trusted`]. Returns `Ok(true)` when a write
/// was made, `Ok(false)` when the entry was already trusted (no write).
pub fn ensure_dir_trusted_at(store: &Path, cwd: &Path) -> io::Result<bool> {
    // Hold the process-global lock across the ENTIRE read→modify→write so two
    // concurrent spawns can't both read the same snapshot and have the second
    // rename clobber the first's trust write (R1). The guarded data is `()`, so
    // a poisoned lock carries no corrupt state — recover the guard and proceed.
    let _guard = STORE_WRITE_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    let key = project_key(cwd);
    let mut root = read_root(store)?;
    let projects = projects_obj_mut(&mut root)?;

    // Idempotent: already trusted ⇒ no write (so the one-time reserialise
    // reordering is paid at most once per worktree). NOTE this gate is
    // TRUST-FLAG ONLY: `hasCompletedProjectOnboarding` is set on the FIRST write
    // (below) but is NOT reconciled once trust is set — a store that is trusted
    // but onboarding-absent returns early here and is not backfilled (R6).
    let already = projects
        .get(&key)
        .and_then(|e| e.get("hasTrustDialogAccepted"))
        .and_then(Value::as_bool)
        == Some(true);
    if already {
        return Ok(false);
    }

    let entry = projects
        .entry(key.clone())
        .or_insert_with(|| Value::Object(Map::new()));
    match entry.as_object_mut() {
        Some(obj) => {
            obj.insert("hasTrustDialogAccepted".to_string(), Value::Bool(true));
            obj.insert("hasCompletedProjectOnboarding".to_string(), Value::Bool(true));
        }
        // A pre-existing NON-object entry is corrupt for our purposes — replace
        // it with a minimal trusted object rather than failing the seed. This is
        // the one path inside `ensure` that destroys existing data, so surface
        // it (the key only, NOT the old value — it may be unexpected data) so a
        // rare clobber is visible in the log (R7).
        None => {
            tracing::warn!(
                project_key = %key,
                "workspace-trust pre-seed: replacing a non-object projects entry with a trusted object"
            );
            *entry = serde_json::json!({
                "hasTrustDialogAccepted": true,
                "hasCompletedProjectOnboarding": true,
            });
        }
    }

    write_root_atomic(store, &root)?;
    Ok(true)
}

/// Remove the trust entry for a single `cwd`. Best-effort wrapper resolving
/// [`claude_json_path`]. Returns `Ok(true)` when an entry was removed.
pub fn remove_dir_trust(cwd: &Path) -> io::Result<bool> {
    let path = claude_json_path().ok_or_else(home_unresolved)?;
    remove_dir_trust_at(&path, cwd)
}

/// Path-injected core of [`remove_dir_trust`].
pub fn remove_dir_trust_at(store: &Path, cwd: &Path) -> io::Result<bool> {
    if !store.exists() {
        return Ok(false);
    }
    let key = project_key(cwd);
    let mut root = read_root(store)?;
    let Some(projects) = root
        .get_mut("projects")
        .and_then(Value::as_object_mut)
    else {
        return Ok(false);
    };
    if projects.remove(&key).is_some() {
        write_root_atomic(store, &root)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

/// Sweep trust entries for lumina worktree dirs that no longer exist on disk.
///
/// SCOPED to keys strictly under `worktrees_dir` (so a non-lumina trust, or a
/// LIVE worktree's trust, is never touched), and only those whose directory is
/// absent. Returns the count removed. Best-effort wrapper resolving
/// [`claude_json_path`].
pub fn prune_orphaned_worktree_trusts(worktrees_dir: &Path) -> io::Result<usize> {
    let path = claude_json_path().ok_or_else(home_unresolved)?;
    prune_orphaned_worktree_trusts_at(&path, worktrees_dir)
}

/// Path-injected core of [`prune_orphaned_worktree_trusts`].
pub fn prune_orphaned_worktree_trusts_at(
    store: &Path,
    worktrees_dir: &Path,
) -> io::Result<usize> {
    if !store.exists() {
        return Ok(0);
    }
    // A trailing slash makes `starts_with` a clean directory-boundary test:
    // `.../worktrees/` matches `.../worktrees/sprint-x` but not a sibling
    // `.../worktrees-backup`.
    let prefix = format!("{}/", project_key(worktrees_dir));

    let mut root = read_root(store)?;
    let Some(projects) = root
        .get_mut("projects")
        .and_then(Value::as_object_mut)
    else {
        return Ok(0);
    };

    let stale: Vec<String> = projects
        .keys()
        .filter(|k| k.starts_with(&prefix))
        // The key IS an absolute path; `std::path::Path` accepts forward slashes
        // on Windows, so this existence check is cross-OS.
        .filter(|k| !Path::new(k.as_str()).exists())
        .cloned()
        .collect();

    for k in &stale {
        projects.remove(k);
    }
    if !stale.is_empty() {
        write_root_atomic(store, &root)?;
    }
    Ok(stale.len())
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn home_unresolved() -> io::Error {
    io::Error::new(
        io::ErrorKind::NotFound,
        "cannot resolve ~/.claude.json: USERPROFILE/HOME unset and LUMINA_CLAUDE_JSON not set",
    )
}

/// Read + parse the store into a JSON object value. A missing or empty file
/// yields an empty object `{}`; a malformed or non-JSON file is an
/// `InvalidData` error (so the caller never clobbers an unparseable store).
fn read_root(store: &Path) -> io::Result<Value> {
    match std::fs::read_to_string(store) {
        Ok(s) if s.trim().is_empty() => Ok(Value::Object(Map::new())),
        Ok(s) => serde_json::from_str(&s)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("parse {}: {e}", store.display()))),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Value::Object(Map::new())),
        Err(e) => Err(e),
    }
}

/// Borrow the `projects` object, creating it if absent. Errors (without
/// writing) when the top-level value or an existing `projects` value is not a
/// JSON object.
fn projects_obj_mut(root: &mut Value) -> io::Result<&mut Map<String, Value>> {
    let obj = root.as_object_mut().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidData, "~/.claude.json top-level is not a JSON object")
    })?;
    let projects = obj
        .entry("projects")
        .or_insert_with(|| Value::Object(Map::new()));
    projects.as_object_mut().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidData, "~/.claude.json `projects` is not a JSON object")
    })
}

/// Serialise `root` (pretty, trailing newline) to a sibling temp file and
/// atomically rename it over `store`. The rename is the atomic swap, so a
/// reader never observes a partially-written store and a crash leaves the
/// original intact.
fn write_root_atomic(store: &Path, root: &Value) -> io::Result<()> {
    let mut body = serde_json::to_string_pretty(root)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("serialise: {e}")))?;
    body.push('\n');

    let dir = store.parent().unwrap_or_else(|| Path::new("."));
    // Unique sibling temp name (same dir ⇒ same filesystem ⇒ atomic rename).
    let tmp = dir.join(format!(".claude.json.lumina-{}.tmp", uuid::Uuid::now_v7()));

    // Write the temp file, then rename. The store holds OAuth tokens + MCP
    // config, so on Unix the temp is created with mode 0600 BEFORE any secret
    // bytes are written (never world-readable, even briefly) — plain
    // `std::fs::write` would create it at the umask default (~0644), leaving the
    // renamed `~/.claude.json` readable by other local users on a shared host
    // (R4). On a write failure, best-effort remove the temp so we don't litter.
    // `std::fs::rename` replaces an existing target on both Unix (rename(2)) and
    // Windows (MoveFileExW/REPLACE_EXISTING).
    if let Err(e) = write_temp_private(&tmp, body.as_bytes()) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    if let Err(e) = std::fs::rename(&tmp, store) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
}

/// Create `tmp` (which must not exist) and write `body` to it.
///
/// On Unix the file is created with mode 0600 so the credential-bearing store is
/// never world-readable, even for the brief window the temp is resident before
/// the rename (R4). On non-Unix the platform default applies (Windows inherits
/// the parent dir's ACL — `~/.claude.json` under USERPROFILE is already
/// user-scoped, so the Unix umask leak does not arise there).
#[cfg(unix)]
fn write_temp_private(tmp: &Path, body: &[u8]) -> io::Result<()> {
    use std::io::Write as _;
    use std::os::unix::fs::OpenOptionsExt as _;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(tmp)?;
    f.write_all(body)
}

#[cfg(not(unix))]
fn write_temp_private(tmp: &Path, body: &[u8]) -> io::Result<()> {
    std::fs::write(tmp, body)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A unique temp store path per test (no env mutation — the `*_at` core
    /// takes the path explicitly, so tests are race-free under `cargo test`).
    fn tmp_store() -> PathBuf {
        std::env::temp_dir().join(format!("lumina-trust-test-{}.json", uuid::Uuid::now_v7()))
    }

    fn read(store: &Path) -> Value {
        serde_json::from_str(&std::fs::read_to_string(store).expect("read store")).expect("parse")
    }

    #[test]
    fn project_key_folds_separators_and_strips_verbatim_prefix() {
        assert_eq!(
            project_key(Path::new(r"\\?\C:\Users\rossa\dev\dev-tools\.lumina\worktrees\sprint-x")),
            "C:/Users/rossa/dev/dev-tools/.lumina/worktrees/sprint-x"
        );
        assert_eq!(project_key(Path::new("/home/rossa/wt/")), "/home/rossa/wt");
        assert_eq!(project_key(Path::new("/home/rossa/wt")), "/home/rossa/wt");
    }

    #[test]
    fn project_key_for_fresh_worktree_matches_claude_key_form() {
        // The claude `projects.<key>` derivation contract for the case this
        // feature targets — a FRESH git worktree cwd. The key must be the
        // absolute path with the verbatim prefix stripped, back-slashes folded
        // to forward slashes, and any trailing slash removed, so it is
        // byte-identical to the key claude derives from `process.cwd()` and a
        // drive-case / 8.3 / junction divergence becomes a visible regression
        // here rather than a silent seed-under-the-wrong-key (R5).
        assert_eq!(
            project_key(Path::new(
                r"\\?\C:\Users\rossa\dev\dev-tools\.lumina\worktrees\sprint-019ee063\"
            )),
            "C:/Users/rossa/dev/dev-tools/.lumina/worktrees/sprint-019ee063",
            "verbatim prefix stripped, separators folded, trailing slash removed"
        );
        assert_eq!(
            project_key(Path::new("/home/rossa/dev/.lumina/worktrees/sprint-a/")),
            "/home/rossa/dev/.lumina/worktrees/sprint-a",
            "unix worktree form: trailing slash removed, already forward-slashed"
        );
    }

    #[test]
    fn ensure_creates_store_and_marks_trusted() {
        let store = tmp_store();
        let cwd = Path::new("/repo/.lumina/worktrees/sprint-a");

        let wrote = ensure_dir_trusted_at(&store, cwd).expect("ensure");
        assert!(wrote, "first ensure writes");

        let root = read(&store);
        let entry = &root["projects"]["/repo/.lumina/worktrees/sprint-a"];
        assert_eq!(entry["hasTrustDialogAccepted"], Value::Bool(true));
        assert_eq!(entry["hasCompletedProjectOnboarding"], Value::Bool(true));

        let _ = std::fs::remove_file(&store);
    }

    #[test]
    fn ensure_is_idempotent_after_first_write() {
        let store = tmp_store();
        let cwd = Path::new("/repo/.lumina/worktrees/sprint-b");

        assert!(ensure_dir_trusted_at(&store, cwd).expect("first"));
        assert!(
            !ensure_dir_trusted_at(&store, cwd).expect("second"),
            "second ensure is a no-op (already trusted)"
        );

        let _ = std::fs::remove_file(&store);
    }

    #[test]
    fn ensure_does_not_backfill_onboarding_on_an_already_trusted_entry() {
        // Trusted but WITHOUT the onboarding flag — the divergent state the
        // idempotency gate keys on trust alone (R6). The early-return must NOT
        // backfill `hasCompletedProjectOnboarding` for this entry.
        let store = tmp_store();
        std::fs::write(
            &store,
            serde_json::to_string(&serde_json::json!({
                "projects": { "/repo/wt": { "hasTrustDialogAccepted": true } }
            }))
            .unwrap(),
        )
        .unwrap();

        assert!(
            !ensure_dir_trusted_at(&store, Path::new("/repo/wt")).expect("ensure"),
            "an already-trusted entry ⇒ no write"
        );

        let root = read(&store);
        assert!(
            root["projects"]["/repo/wt"]
                .get("hasCompletedProjectOnboarding")
                .is_none(),
            "onboarding is NOT reconciled once trust is set (the gate is trust-flag-only)"
        );

        let _ = std::fs::remove_file(&store);
    }

    #[test]
    fn ensure_replaces_a_non_object_entry_and_keeps_siblings() {
        // A pre-existing NON-object entry at the cwd key is replaced wholesale
        // with a trusted object (R7); a sibling project entry is untouched.
        let store = tmp_store();
        std::fs::write(
            &store,
            serde_json::to_string(&serde_json::json!({
                "projects": {
                    "/repo/wt": "not-an-object",
                    "C:/other/project": { "hasTrustDialogAccepted": true, "lastCost": 2.0 }
                }
            }))
            .unwrap(),
        )
        .unwrap();

        assert!(
            ensure_dir_trusted_at(&store, Path::new("/repo/wt")).expect("ensure"),
            "replacing a non-object entry is a write"
        );

        let root = read(&store);
        assert_eq!(
            root["projects"]["/repo/wt"]["hasTrustDialogAccepted"],
            Value::Bool(true),
            "the non-object entry is replaced with a trusted object"
        );
        // The sibling entry survives the clobber untouched.
        assert_eq!(
            root["projects"]["C:/other/project"]["lastCost"],
            serde_json::json!(2.0),
            "a sibling project entry is not disturbed by the non-object replace"
        );

        let _ = std::fs::remove_file(&store);
    }

    #[test]
    fn ensure_preserves_other_projects_and_top_level_keys() {
        let store = tmp_store();
        std::fs::write(
            &store,
            serde_json::to_string_pretty(&serde_json::json!({
                "userID": "abc123",
                "projects": {
                    "C:/other/project": { "hasTrustDialogAccepted": true, "lastCost": 1.5 }
                }
            }))
            .unwrap(),
        )
        .unwrap();

        ensure_dir_trusted_at(&store, Path::new("C:/repo/.lumina/worktrees/sprint-c")).expect("ensure");

        let root = read(&store);
        // Untouched siblings survive.
        assert_eq!(root["userID"], Value::String("abc123".into()));
        assert_eq!(root["projects"]["C:/other/project"]["lastCost"], serde_json::json!(1.5));
        // New entry present.
        assert_eq!(
            root["projects"]["C:/repo/.lumina/worktrees/sprint-c"]["hasTrustDialogAccepted"],
            Value::Bool(true)
        );

        let _ = std::fs::remove_file(&store);
    }

    #[test]
    fn ensure_flips_an_existing_false_entry_to_true() {
        let store = tmp_store();
        std::fs::write(
            &store,
            serde_json::to_string(&serde_json::json!({
                "projects": { "/repo/wt": { "hasTrustDialogAccepted": false, "keep": 1 } }
            }))
            .unwrap(),
        )
        .unwrap();

        assert!(ensure_dir_trusted_at(&store, Path::new("/repo/wt")).expect("ensure"));

        let root = read(&store);
        assert_eq!(root["projects"]["/repo/wt"]["hasTrustDialogAccepted"], Value::Bool(true));
        // Pre-existing sibling field on the same entry is preserved.
        assert_eq!(root["projects"]["/repo/wt"]["keep"], serde_json::json!(1));

        let _ = std::fs::remove_file(&store);
    }

    #[test]
    fn malformed_store_is_not_clobbered() {
        let store = tmp_store();
        std::fs::write(&store, "{ this is not json").unwrap();

        let err = ensure_dir_trusted_at(&store, Path::new("/repo/wt")).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        // The original bytes are untouched.
        assert_eq!(std::fs::read_to_string(&store).unwrap(), "{ this is not json");

        let _ = std::fs::remove_file(&store);
    }

    #[test]
    fn remove_drops_only_the_named_entry() {
        let store = tmp_store();
        ensure_dir_trusted_at(&store, Path::new("/repo/wt/a")).unwrap();
        ensure_dir_trusted_at(&store, Path::new("/repo/wt/b")).unwrap();

        assert!(remove_dir_trust_at(&store, Path::new("/repo/wt/a")).expect("remove"));
        let root = read(&store);
        assert!(root["projects"].get("/repo/wt/a").is_none(), "a removed");
        assert!(root["projects"].get("/repo/wt/b").is_some(), "b kept");

        // Removing an absent key is a clean no-op.
        assert!(!remove_dir_trust_at(&store, Path::new("/repo/wt/a")).expect("remove-again"));

        let _ = std::fs::remove_file(&store);
    }

    #[test]
    fn prune_removes_only_absent_dirs_under_the_worktrees_root() {
        // A real, existing dir (kept) vs a synthetic absent one (pruned), both
        // under the worktrees root; plus an out-of-scope entry (never touched).
        let live_dir = std::env::temp_dir().join(format!("lumina-live-wt-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&live_dir).unwrap();
        let worktrees_root = live_dir.parent().unwrap().to_path_buf();
        let absent_dir = worktrees_root.join("lumina-absent-wt-does-not-exist");
        let outside = Path::new("/some/other/trusted/project");
        // A `worktrees`-PREFIXED SIBLING directory (same parent as the root, name
        // = `<root>-backup`) that is ALSO absent on disk. Its key shares the
        // root's text prefix but NOT the directory boundary, so the trailing-`/`
        // in the prune prefix must keep it OUT of scope. Dropping that slash
        // would prune this entry — this entry is the regression guard (R8).
        let sibling_backup_key = format!("{}-backup/orphan-absent", project_key(&worktrees_root));

        let store = tmp_store();
        ensure_dir_trusted_at(&store, &live_dir).unwrap();
        ensure_dir_trusted_at(&store, &absent_dir).unwrap();
        ensure_dir_trusted_at(&store, outside).unwrap();
        ensure_dir_trusted_at(&store, Path::new(&sibling_backup_key)).unwrap();

        let removed = prune_orphaned_worktree_trusts_at(&store, &worktrees_root).expect("prune");
        assert_eq!(removed, 1, "only the absent worktree dir is pruned");

        let root = read(&store);
        assert!(root["projects"].get(&project_key(&live_dir)).is_some(), "live kept");
        assert!(root["projects"].get(&project_key(&absent_dir)).is_none(), "absent pruned");
        assert!(root["projects"].get("/some/other/trusted/project").is_some(), "out-of-scope kept");
        assert!(
            root["projects"].get(&sibling_backup_key).is_some(),
            "an absent `<root>-backup` sibling is NOT pruned — the trailing-slash boundary holds"
        );

        let _ = std::fs::remove_dir_all(&live_dir);
        let _ = std::fs::remove_file(&store);
    }

    #[test]
    fn ops_on_absent_store_are_clean_noops() {
        let store = tmp_store(); // never created
        assert!(!remove_dir_trust_at(&store, Path::new("/x")).expect("remove"));
        assert_eq!(prune_orphaned_worktree_trusts_at(&store, Path::new("/x")).expect("prune"), 0);
        assert!(!store.exists(), "no store was created by read-only ops");
    }
}
