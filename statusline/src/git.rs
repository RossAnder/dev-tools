//! Git facts, read as cheaply as possible — the statusline runs on every
//! refresh in every session.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

/// Resolve the branch by reading `.git/HEAD` directly — no `git` spawn. Walks
/// up parents (subdir launches), follows the `gitdir:` pointer file
/// (worktrees/submodules; relative pointers resolve against the pointer's
/// directory), and falls back to a short sha for detached HEAD.
pub fn branch(dir: &Path) -> Option<String> {
    let mut d = dir;
    let git_dir: PathBuf = loop {
        let g = d.join(".git");
        if g.is_file() {
            let line = std::fs::read_to_string(&g).ok()?;
            let rest = line.lines().next()?.strip_prefix("gitdir:")?.trim();
            let p = PathBuf::from(rest);
            break if p.is_absolute() { p } else { d.join(p) };
        }
        if g.is_dir() {
            break g;
        }
        d = d.parent()?;
    };
    let head = std::fs::read_to_string(git_dir.join("HEAD")).ok()?;
    parse_head(head.lines().next()?.trim())
}

pub fn parse_head(head: &str) -> Option<String> {
    if let Some(r) = head.strip_prefix("ref:") {
        return r.trim().strip_prefix("refs/heads/").map(str::to_string);
    }
    let is_sha = (7..=40).contains(&head.len())
        && head
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    is_sha.then(|| head[..7].to_string())
}

/// How long the spawn gets before the changes segment is dropped. Every other
/// failure in this crate degrades silently; a git that blocks — an `index.lock`
/// held by an interactive command, a stalled network filesystem — must do the
/// same rather than hang a line that re-renders on every refresh. Roughly 3x
/// the measured cost of the spawn in a repo this size: a hang guard, not a
/// latency target.
const DIFF_BUDGET: Duration = Duration::from_millis(1000);

/// Top-level `git` options, which have to precede the subcommand. The
/// hardening is not incidental: `git diff` honours `core.fsmonitor` from the
/// *target* repo's own config, so a tree that ships its own `.git` would
/// otherwise choose a program we run on every refresh. An empty `-c` value is
/// the documented spelling of boolean false, which disables the hook.
/// `--no-optional-locks` keeps the tick from refreshing (and locking) the index
/// under the user's own interactive git.
const DIFF_ARGS_PRE: [&str; 4] = ["--no-optional-locks", "-c", "core.fsmonitor=", "-C"];

/// The subcommand and its own options, which have to follow it — `--no-ext-diff`
/// and `--no-textconv` close the other two config-driven execution paths
/// (`diff.external` and `diff.<driver>.textconv`, both enabled by default for
/// `git diff`).
///
/// KNOWN RESIDUAL: the clean filter is still open, and no fixed argument closes
/// it. `git diff HEAD` compares worktree content against HEAD, so it converts
/// that content to its object form first — which runs it through
/// `filter.<driver>.clean` (or `.process`) whenever the target repo's own
/// `.gitattributes` assigns a filter to the path. The driver *name* comes from
/// that `.gitattributes`, so there is nothing to spell as a `-c` override
/// without first enumerating the foreign repo's config, and git has no blanket
/// "run no filters" flag for `diff` (`git cat-file --filters` is the opt-*in*,
/// not a counterpart; `diff-options` documents only `--no-ext-diff` and
/// `--no-textconv`, both already passed above). Closing it properly means a
/// config-parsing layer, which is out of proportion to this crate.
///
/// It matters only under one threat model: the statusline renders whatever
/// directory the session sits in, so pointing it at an untrusted tree — a
/// cloned repo whose `.gitattributes` and `.git/config` an attacker wrote —
/// turns a per-refresh tick into an attacker-chosen spawn. Same shape as the
/// `core.fsmonitor` path closed above; the difference is only that fsmonitor
/// has a fixed key and this does not.
///
/// `core.hooksPath` was assessed and deliberately left alone, so don't
/// re-derive it: `post-index-change` is the only hook `git diff` can reach, and
/// `--no-optional-locks` already suppresses the index write that would fire it.
const DIFF_ARGS_POST: [&str; 5] = ["diff", "HEAD", "--numstat", "--no-ext-diff", "--no-textconv"];

/// Staged + unstaged line counts via `git diff HEAD --numstat`. Returns None
/// when git fails, outruns `DIFF_BUDGET`, or there are no changes (the ps1
/// renders nothing for 0+0).
pub fn diff_stats(cwd: &str) -> Option<(u64, u64)> {
    let mut child = Command::new("git")
        .args(DIFF_ARGS_PRE)
        .arg(cwd)
        .args(DIFF_ARGS_POST)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;

    // Drain the pipe on a worker so the child stays owned here and can still be
    // killed once the budget is spent. Whichever side gives up first just drops
    // its end of the channel, so neither can wedge the other.
    let Some(mut stdout) = child.stdout.take() else {
        return abandon(&mut child);
    };
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = tx.send(stdout.read_to_end(&mut buf).ok().map(|_| buf));
    });
    let Ok(Some(bytes)) = rx.recv_timeout(DIFF_BUDGET) else {
        return abandon(&mut child);
    };

    // Stdout is at EOF, so this is the wait `output()` would have done anyway.
    if !child.wait().ok()?.success() {
        return None;
    }
    stats_from_numstat(&String::from_utf8_lossy(&bytes))
}

/// The pure half of `diff_stats`: sum the numstat, then drop `0 + 0` entirely.
/// Kept separate from the spawn so the rule the renderer depends on — no
/// `(+n -m)` group at all for an unchanged tree, matching the ps1 — is
/// reachable from a test. Module-private on purpose: nothing outside `git`
/// consumes it, and the crate's surface is already wider than it needs to be.
fn stats_from_numstat(text: &str) -> Option<(u64, u64)> {
    let (added, deleted) = sum_numstat(text);
    (added + deleted > 0).then_some((added, deleted))
}

/// Give up on a spawned git: kill it and reap it (a `Child` dropped without a
/// wait leaves a zombie), then degrade to the same None a failed spawn returns.
fn abandon(child: &mut Child) -> Option<(u64, u64)> {
    let _ = child.kill();
    let _ = child.wait();
    None
}

pub fn sum_numstat(text: &str) -> (u64, u64) {
    let (mut added, mut deleted) = (0u64, 0u64);
    for line in text.lines() {
        let mut cols = line.split_whitespace();
        // Binary files report "-" per column; skip whatever doesn't parse.
        if let Some(a) = cols.next().and_then(|c| c.parse::<u64>().ok()) {
            added += a;
        }
        if let Some(d) = cols.next().and_then(|c| c.parse::<u64>().ok()) {
            deleted += d;
        }
    }
    (added, deleted)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn head_parsing() {
        assert_eq!(parse_head("ref: refs/heads/main"), Some("main".into()));
        assert_eq!(
            parse_head("ref: refs/heads/feature/x-1"),
            Some("feature/x-1".into())
        );
        assert_eq!(
            parse_head("5a715f7deadbeef5a715f7deadbeef5a715f7dea"),
            Some("5a715f7".into())
        );
        // Uppercase hex and non-hex are rejected, matching the ps1 regex.
        assert_eq!(parse_head("5A715F7DEADBEEF"), None);
        assert_eq!(parse_head("not-a-head"), None);
        assert_eq!(parse_head("ref: refs/tags/v1"), None);
    }

    // A misplaced flag is a git usage error, which this crate would swallow as
    // a silently missing changes segment — so pin the split.
    #[test]
    fn diff_args_keep_top_level_options_ahead_of_the_subcommand() {
        assert!(DIFF_ARGS_PRE.iter().all(|a| *a != "diff"));
        assert!(DIFF_ARGS_PRE.contains(&"--no-optional-locks"));
        assert!(DIFF_ARGS_PRE.contains(&"core.fsmonitor="));
        assert_eq!(DIFF_ARGS_PRE.last(), Some(&"-C")); // the cwd goes here
        assert_eq!(DIFF_ARGS_POST.first(), Some(&"diff"));
        assert!(DIFF_ARGS_POST.contains(&"--no-ext-diff"));
        assert!(DIFF_ARGS_POST.contains(&"--no-textconv"));
    }

    #[test]
    fn numstat_summing() {
        let text = "10\t2\tsrc/main.rs\n-\t-\tassets/logo.png\n3\t0\tREADME.md\n";
        assert_eq!(sum_numstat(text), (13, 2));
        assert_eq!(sum_numstat(""), (0, 0));
    }

    // The renderer omits the whole (+n -m) group rather than printing "+0 -0",
    // and that decision lives here rather than in the renderer — so pin it here.
    #[test]
    fn a_tree_with_no_counted_changes_yields_no_stats_at_all() {
        assert_eq!(stats_from_numstat(""), None);
        assert_eq!(stats_from_numstat("0\t0\tsrc/main.rs\n"), None);
        // Binary-only churn counts as nothing: numstat reports "-" per column,
        // so there is no number to render even though the tree is dirty.
        assert_eq!(stats_from_numstat("-\t-\tassets/logo.png\n"), None);
        assert_eq!(
            stats_from_numstat("10\t2\tsrc/main.rs\n-\t-\tassets/logo.png\n3\t0\tREADME.md\n"),
            Some((13, 2))
        );
    }

    /// A fixture root under the system temp dir — deliberately NOT under the
    /// working tree, because `branch` walks up parents: a fixture rooted in this
    /// repo would find dev-tools' own `.git` and make the walks-off-the-top case
    /// pass for entirely the wrong reason. The pid separates concurrent `cargo
    /// test` processes and `tag` separates the tests within one process, which
    /// share it across parallel threads; a fixed name would race.
    fn fixture(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("statusline-git-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("fixture root");
        root
    }

    fn write(path: &Path, body: &str) {
        std::fs::create_dir_all(path.parent().expect("fixture parent")).expect("fixture dirs");
        std::fs::write(path, body).expect("fixture file");
    }

    #[test]
    fn branch_walks_up_to_an_ancestors_git_directory_when_launched_in_a_subdirectory() {
        let root = fixture("walk-up");
        write(&root.join(".git/HEAD"), "ref: refs/heads/main\n");
        std::fs::create_dir_all(root.join("src/render/deep")).expect("fixture dirs");

        assert_eq!(branch(&root.join("src/render/deep")), Some("main".into()));

        let _ = std::fs::remove_dir_all(&root);
    }

    // The pointer is resolved against the directory holding it, not against the
    // directory the walk started in — the two differ whenever a worktree is
    // entered from a subdirectory, which is the normal case.
    #[test]
    fn branch_resolves_a_relative_gitdir_pointer_against_the_pointer_files_own_directory() {
        let root = fixture("relative-pointer");
        write(&root.join("wt/.git"), "gitdir: ../store/worktrees/wt\n");
        write(
            &root.join("store/worktrees/wt/HEAD"),
            "ref: refs/heads/feature/x-1\n",
        );
        std::fs::create_dir_all(root.join("wt/sub")).expect("fixture dirs");

        assert_eq!(branch(&root.join("wt/sub")), Some("feature/x-1".into()));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn branch_follows_an_absolute_gitdir_pointer_verbatim() {
        let root = fixture("absolute-pointer");
        let store = root.join("store");
        write(&store.join("HEAD"), "ref: refs/heads/trunk\n");
        write(
            &root.join("proj/.git"),
            &format!("gitdir: {}\n", store.display()),
        );

        assert_eq!(branch(&root.join("proj")), Some("trunk".into()));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn branch_gives_up_when_the_walk_runs_off_the_top_without_finding_a_git_dir() {
        let root = fixture("no-git");
        let leaf = root.join("a/b/c");
        std::fs::create_dir_all(&leaf).expect("fixture dirs");

        assert_eq!(branch(&leaf), None);

        let _ = std::fs::remove_dir_all(&root);
    }
}
