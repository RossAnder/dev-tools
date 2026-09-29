//! Find the repository root `git rev-parse --show-toplevel` would print, without
//! spawning git. Invariant: whenever [`discover`] answers `Root(p)`, git run
//! from the same start succeeds and its top level canonicalises to `p`; when
//! it answers `NoRepo`, git fails. Every case where the two could disagree is
//! `AskGit` and the caller runs git: a discovery variable in the environment,
//! a start inside a git dir, a `.git` git would skip or reject, config that
//! moves or removes the worktree, or a path not owned by the current user.
//! The walk never reimplements one of git's recovery rules; it only declines.
//! Semantics checked against git 2.55.0.windows.3.

use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use crate::owner::is_owned_by_current_user;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Discovery {
    /// The canonical top level of the repository enclosing the start.
    Root(PathBuf),
    NoRepo,
    AskGit,
}

/// Variables whose presence, even with an empty value, changes git's answer.
const DISCOVERY_ENV: [&str; 7] = [
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_CEILING_DIRECTORIES",
    "GIT_COMMON_DIR",
    "GIT_OBJECT_DIRECTORY",
    "GIT_DISCOVERY_ACROSS_FILESYSTEM",
    "GIT_TEST_ASSUME_DIFFERENT_OWNER",
];

/// Larger `HEAD`, gitfile or `commondir` contents are not ones git wrote.
const SMALL_FILE_MAX: u64 = 64 * 1024;

/// `env` answers whether a variable is present; callers pass the process
/// environment, tests a closure.
pub(crate) fn discover(start: &Path, env: impl Fn(&str) -> bool) -> Discovery {
    if env_forces_git(&env) {
        return Discovery::AskGit;
    }
    let Ok(start) = start.canonicalize() else {
        return Discovery::AskGit;
    };
    #[cfg(unix)]
    let Ok(start_device) = device(&start) else {
        return Discovery::AskGit;
    };
    for dir in start.ancestors() {
        #[cfg(unix)]
        if dir != start {
            match device(dir) {
                Ok(d) if d == start_device => {}
                Ok(_) => return Discovery::NoRepo,
                Err(_) => return Discovery::AskGit,
            }
        }
        match probe(dir) {
            Probe::Absent if looks_like_git_dir(dir) => return Discovery::AskGit,
            Probe::Absent => {}
            Probe::Worktree => return Discovery::Root(dir.to_path_buf()),
            Probe::Doubt => return Discovery::AskGit,
        }
    }
    Discovery::NoRepo
}

fn env_forces_git(env: &impl Fn(&str) -> bool) -> bool {
    DISCOVERY_ENV.iter().any(|name| env(name))
}

#[cfg(unix)]
fn device(path: &Path) -> io::Result<u64> {
    use std::os::unix::fs::MetadataExt;
    Ok(fs::metadata(path)?.dev())
}

enum Probe {
    /// No `.git` entry here; keep walking.
    Absent,
    /// A `.git` git would accept, making this directory the top level.
    Worktree,
    Doubt,
}

fn probe(dir: &Path) -> Probe {
    let dotgit = dir.join(".git");
    let meta = match fs::symlink_metadata(&dotgit) {
        Ok(meta) => meta,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Probe::Absent,
        Err(_) => return Probe::Doubt,
    };
    let (git_dir, gitfile) = if meta.is_dir() {
        (dotgit, None)
    } else if meta.is_file() {
        match read_gitfile(dir, &dotgit) {
            Some(git_dir) => (git_dir, Some(dotgit)),
            None => return Probe::Doubt,
        }
    } else {
        return Probe::Doubt;
    };
    let Some(common) = is_git_dir(&git_dir) else {
        return Probe::Doubt;
    };
    if config_forces_git(&git_dir, &common) {
        return Probe::Doubt;
    }
    let mut paths = vec![dir, git_dir.as_path(), common.as_path()];
    paths.extend(gitfile.as_deref());
    if owned(&paths) {
        Probe::Worktree
    } else {
        Probe::Doubt
    }
}

/// Anything git might treat as a git dir itself, which would make a start
/// here or below fail in git rather than find an outer repository.
fn looks_like_git_dir(dir: &Path) -> bool {
    if dir
        .file_name()
        .is_some_and(|name| name.eq_ignore_ascii_case(".git"))
    {
        return true;
    }
    !matches!(
        fs::symlink_metadata(dir.join("HEAD")),
        Err(e) if e.kind() == io::ErrorKind::NotFound
    )
}

/// The common dir of a git dir passing git's own validity test: a valid
/// `HEAD`, then `objects/` and `refs/` under `commondir`'s target, or under
/// the git dir itself when it has no `commondir`.
fn is_git_dir(git_dir: &Path) -> Option<PathBuf> {
    let head = git_dir.join("HEAD");
    if !fs::symlink_metadata(&head).ok()?.is_file() || !valid_head(&read_small(&head)?) {
        return None;
    }
    let commondir = git_dir.join("commondir");
    let common = match fs::symlink_metadata(&commondir) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => git_dir.to_path_buf(),
        Ok(meta) if meta.is_file() => {
            let text = read_small(&commondir)?;
            join_relative(git_dir, single_line(&text)?)?
                .canonicalize()
                .ok()?
        }
        _ => return None,
    };
    let is_dir = |name: &str| fs::metadata(common.join(name)).is_ok_and(|m| m.is_dir());
    (is_dir("objects") && is_dir("refs")).then_some(common)
}

/// A symbolic ref into `refs/`, or an object id; anything else git rejects.
fn valid_head(text: &str) -> bool {
    if let Some(target) = text.strip_prefix("ref:") {
        return target
            .trim_start_matches([' ', '\t', '\n', '\r'])
            .starts_with("refs/");
    }
    text.len() >= 40 && text.as_bytes()[..40].iter().all(u8::is_ascii_hexdigit)
}

/// The canonical git dir a `gitdir: <path>` file names, relative paths taken
/// from the directory holding the file.
fn read_gitfile(dir: &Path, gitfile: &Path) -> Option<PathBuf> {
    let text = read_small(gitfile)?;
    let target = single_line(text.strip_prefix("gitdir: ")?)?;
    join_relative(dir, target)?.canonicalize().ok()
}

fn read_small(path: &Path) -> Option<String> {
    if fs::metadata(path).ok()?.len() > SMALL_FILE_MAX {
        return None;
    }
    fs::read_to_string(path).ok()
}

/// `text` without its trailing line breaks, refused when that leaves it empty,
/// multi-line, or with whitespace at either end.
fn single_line(text: &str) -> Option<&str> {
    let line = text.trim_end_matches(['\n', '\r']);
    let clean = !line.is_empty() && !line.contains(['\n', '\r', '\0']) && line.trim() == line;
    clean.then_some(line)
}

/// `rel` resolved against the canonical `base`. Leading `..` steps pop `base`
/// lexically, which is exact because `base` holds no links; a `..` after a
/// named step could cross a link, so it is refused.
fn join_relative(base: &Path, rel: &str) -> Option<PathBuf> {
    let rel = Path::new(rel);
    if rel.is_absolute() {
        return Some(rel.to_path_buf());
    }
    let mut out = base.to_path_buf();
    let mut named = false;
    for component in rel.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir if !named => {
                if !out.pop() {
                    return None;
                }
            }
            Component::Normal(part) => {
                named = true;
                out.push(part);
            }
            _ => return None,
        }
    }
    Some(out)
}

/// Whether the repository config makes git's answer differ from the walk's:
/// read the way git reads it during discovery, without includes.
fn config_forces_git(git_dir: &Path, common: &Path) -> bool {
    let Ok(config) = fs::read_to_string(common.join("config")) else {
        return true;
    };
    if config_text_forces_git(&config) {
        return true;
    }
    match fs::read_to_string(git_dir.join("config.worktree")) {
        Ok(text) => config_text_forces_git(&text),
        Err(e) => e.kind() != io::ErrorKind::NotFound,
    }
}

/// A line-by-line scan; a line it cannot read plainly forces git.
fn config_text_forces_git(text: &str) -> bool {
    text.lines().any(config_line_forces_git)
}

fn config_line_forces_git(line: &str) -> bool {
    let mut rest = line.trim();
    if rest.ends_with('\\') {
        return true;
    }
    if let Some(after) = rest.strip_prefix('[') {
        let Some((header, tail)) = split_section_header(after) else {
            return true;
        };
        let name = header
            .trim_start()
            .split([' ', '\t', '"', '.'])
            .next()
            .unwrap_or("");
        if name.eq_ignore_ascii_case("extensions") {
            return true;
        }
        rest = tail.trim_start();
    }
    if rest.is_empty() || rest.starts_with(['#', ';']) {
        return false;
    }
    let (key, value) = match rest.split_once('=') {
        Some((key, value)) => (key.trim(), Some(plain_value(value))),
        None => (rest, None),
    };
    if key.is_empty() || !key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
        return true;
    }
    match key.to_ascii_lowercase().as_str() {
        "worktree" => true,
        "bare" => !matches!(value, Some(Some(v)) if ["false", "no", "off", "0"]
            .iter()
            .any(|f| v.eq_ignore_ascii_case(f))),
        "repositoryformatversion" => !matches!(value, Some(Some("0" | "1"))),
        _ => false,
    }
}

/// Splits `[`-stripped header text at its closing `]`, honouring a quoted
/// subsection that may itself contain `]`.
fn split_section_header(after: &str) -> Option<(&str, &str)> {
    let mut quoted = false;
    let mut escaped = false;
    for (i, c) in after.char_indices() {
        match c {
            _ if escaped => escaped = false,
            '\\' if quoted => escaped = true,
            '"' => quoted = !quoted,
            ']' if !quoted => return Some((&after[..i], &after[i + 1..])),
            _ => {}
        }
    }
    None
}

/// An unquoted value with any trailing comment removed; `None` for a value
/// using quotes or escapes, which the scan does not interpret.
fn plain_value(value: &str) -> Option<&str> {
    if value.contains(['"', '\\']) {
        return None;
    }
    let end = value.find(['#', ';']).unwrap_or(value.len());
    Some(value[..end].trim())
}

fn owned(paths: &[&Path]) -> bool {
    paths
        .iter()
        .all(|path| matches!(is_owned_by_current_user(path), Ok(true)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{git, git_available, git_command, write};
    use std::sync::OnceLock;

    fn no_env(_: &str) -> bool {
        false
    }

    fn have_git() -> bool {
        static AVAILABLE: OnceLock<bool> = OnceLock::new();
        *AVAILABLE.get_or_init(git_available)
    }

    fn canon(path: &Path) -> PathBuf {
        path.canonicalize().unwrap()
    }

    fn init(dir: &Path) {
        fs::create_dir_all(dir).unwrap();
        git(dir, &["init", "-q"]);
    }

    fn commit(dir: &Path) {
        git(
            dir,
            &[
                "-c",
                "user.email=t@example.invalid",
                "-c",
                "user.name=t",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-q",
                "--no-verify",
                "--allow-empty",
                "-m",
                "init",
            ],
        );
    }

    fn git_toplevel(start: &Path) -> std::process::Output {
        let mut cmd = git_command(start);
        for name in DISCOVERY_ENV {
            cmd.env_remove(name);
        }
        cmd.args(["rev-parse", "--show-toplevel"]).output().unwrap()
    }

    /// `discover` from `start`, held to the module invariant against real git
    /// when git is installed: `Root` must be git's top level, `NoRepo` a
    /// start git fails from.
    fn check(start: &Path) -> Discovery {
        let found = discover(start, no_env);
        if !have_git() {
            return found;
        }
        let out = git_toplevel(start);
        match &found {
            Discovery::Root(root) => {
                assert!(
                    out.status.success(),
                    "walk found {} from {} but git failed: {}",
                    root.display(),
                    start.display(),
                    String::from_utf8_lossy(&out.stderr)
                );
                let top = String::from_utf8(out.stdout).unwrap();
                assert_eq!(
                    &canon(Path::new(top.trim_end())),
                    root,
                    "from {}",
                    start.display()
                );
            }
            Discovery::NoRepo => assert!(
                !out.status.success(),
                "walk found no repo from {} but git printed {}",
                start.display(),
                String::from_utf8_lossy(&out.stdout)
            ),
            Discovery::AskGit => {}
        }
        found
    }

    #[test]
    fn a_plain_repo_is_found_from_a_subdirectory() {
        if !have_git() {
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        init(&repo);
        fs::create_dir_all(repo.join("a/b")).unwrap();

        assert_eq!(check(&repo.join("a/b")), Discovery::Root(canon(&repo)));
        assert_eq!(check(&repo), Discovery::Root(canon(&repo)));
    }

    #[test]
    fn a_linked_worktree_is_its_own_root() {
        if !have_git() {
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let main = tmp.path().join("main");
        let wt = tmp.path().join("wt");
        init(&main);
        commit(&main);
        git(&main, &["worktree", "add", "-q", wt.to_str().unwrap()]);
        fs::create_dir_all(wt.join("sub")).unwrap();

        assert_eq!(check(&wt.join("sub")), Discovery::Root(canon(&wt)));
        assert_eq!(check(&main), Discovery::Root(canon(&main)));
    }

    #[test]
    fn a_submodule_worktree_asks_git() {
        if !have_git() {
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        let main = tmp.path().join("main");
        init(&src);
        commit(&src);
        init(&main);
        commit(&main);
        let url = src.to_str().unwrap().replace('\\', "/");
        git(
            &main,
            &[
                "-c",
                "protocol.file.allow=always",
                "submodule",
                "add",
                "-q",
                &url,
                "sm",
            ],
        );

        assert_eq!(check(&main.join("sm")), Discovery::AskGit);
        assert_eq!(check(&main), Discovery::Root(canon(&main)));
    }

    #[test]
    fn a_start_inside_a_git_dir_asks_git() {
        if !have_git() {
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        init(&repo);
        git(tmp.path(), &["init", "-q", "--bare", "bare.git"]);
        let bare = tmp.path().join("bare.git");

        for start in [
            repo.join(".git"),
            repo.join(".git/refs"),
            bare.clone(),
            bare.join("refs"),
        ] {
            assert_eq!(check(&start), Discovery::AskGit, "{}", start.display());
        }
    }

    #[test]
    fn an_empty_dot_git_dir_under_a_repo_asks_git() {
        if !have_git() {
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        init(&repo);
        fs::create_dir_all(repo.join("inner/.git")).unwrap();

        assert_eq!(check(&repo.join("inner")), Discovery::AskGit);
    }

    #[test]
    fn a_garbage_or_dangling_gitfile_asks_git() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir_all(tmp.path().join("not-a-git-dir")).unwrap();
        for (name, body) in [
            ("garbage", "garbage\n"),
            ("dangling", "gitdir: missing/place\n"),
            ("empty-target", "gitdir: ../not-a-git-dir\n"),
            ("no-space", "gitdir:../not-a-git-dir\n"),
        ] {
            write(tmp.path(), &format!("{name}/.git"), body.as_bytes());
            assert_eq!(check(&tmp.path().join(name)), Discovery::AskGit, "{name}");
        }
    }

    #[test]
    fn repository_config_that_moves_the_worktree_asks_git() {
        if !have_git() {
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let elsewhere = tmp.path().join("elsewhere");
        fs::create_dir_all(&elsewhere).unwrap();
        let settings: [(&str, &str, &str); 3] = [
            ("worktree", "core.worktree", elsewhere.to_str().unwrap()),
            ("bare", "core.bare", "true"),
            ("extensions", "extensions.worktreeConfig", "true"),
        ];
        for (name, key, value) in settings {
            let repo = tmp.path().join(name);
            init(&repo);
            assert_eq!(check(&repo), Discovery::Root(canon(&repo)), "{name} before");
            git(&repo, &["config", key, value]);
            assert_eq!(check(&repo), Discovery::AskGit, "{name}");
        }
    }

    #[test]
    fn each_discovery_variable_asks_git() {
        if !have_git() {
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        init(&repo);
        assert_eq!(discover(&repo, no_env), Discovery::Root(canon(&repo)));
        for name in [
            "GIT_DIR",
            "GIT_WORK_TREE",
            "GIT_CEILING_DIRECTORIES",
            "GIT_COMMON_DIR",
            "GIT_OBJECT_DIRECTORY",
            "GIT_DISCOVERY_ACROSS_FILESYSTEM",
            "GIT_TEST_ASSUME_DIFFERENT_OWNER",
        ] {
            assert_eq!(discover(&repo, |k| k == name), Discovery::AskGit, "{name}");
        }
    }

    #[test]
    fn a_directory_outside_any_repo_has_none() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("plain");
        fs::create_dir_all(&dir).unwrap();
        if have_git() && git_toplevel(&dir).status.success() {
            eprintln!("skipped: the temp dir sits inside a repository on this machine");
            return;
        }
        assert_eq!(check(&dir), Discovery::NoRepo);
    }

    #[test]
    fn a_directory_holding_head_asks_git() {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), "odd/HEAD", b"ref: refs/heads/main\n");
        assert_eq!(check(&tmp.path().join("odd")), Discovery::AskGit);
    }

    #[test]
    fn a_missing_start_asks_git() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(
            discover(&tmp.path().join("missing"), no_env),
            Discovery::AskGit
        );
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn a_start_through_a_link_finds_the_same_root() {
        if !have_git() {
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        init(&repo);
        let target = repo.join("sub");
        fs::create_dir_all(&target).unwrap();
        let link = tmp.path().join("link");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, &link).unwrap();
        #[cfg(windows)]
        {
            let status = std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(&link)
                .arg(&target)
                .stdout(std::process::Stdio::null())
                .status()
                .unwrap();
            assert!(status.success(), "mklink /J failed: {status}");
        }

        assert_eq!(check(&target), Discovery::Root(canon(&repo)));
        assert_eq!(check(&link), Discovery::Root(canon(&repo)));
    }

    #[test]
    fn head_contents_follow_gits_validity_rule() {
        assert!(valid_head("ref: refs/heads/main\n"));
        assert!(valid_head("ref:\trefs/heads/main"));
        assert!(valid_head(&"a".repeat(40)));
        assert!(!valid_head("ref: heads/main"));
        assert!(!valid_head(&"a".repeat(39)));
        assert!(!valid_head(&format!("{}g", "a".repeat(39))));
        assert!(!valid_head(""));
    }

    #[test]
    fn relative_targets_resolve_only_leading_parent_steps() {
        let base = std::env::temp_dir().canonicalize().unwrap();
        let parent = base.parent().unwrap();
        assert_eq!(
            join_relative(&base, "../x/y").unwrap(),
            parent.join("x").join("y")
        );
        assert_eq!(join_relative(&base, "./x").unwrap(), base.join("x"));
        assert_eq!(join_relative(&base, "x/../y"), None);
        let root = base.ancestors().last().unwrap();
        assert_eq!(join_relative(root, ".."), None);
    }

    #[test]
    fn single_line_refuses_anything_git_would_read_differently() {
        assert_eq!(single_line("x/y\r\n\n"), Some("x/y"));
        assert_eq!(single_line("x\ny\n"), None);
        assert_eq!(single_line("x \n"), None);
        assert_eq!(single_line("\n"), None);
    }

    #[test]
    fn the_config_scan_passes_a_default_config_and_nothing_doubtful() {
        let default = "[core]\n\trepositoryformatversion = 0\n\tfilemode = false\n\
                       \tbare = false\n\tlogallrefupdates = true\n\tsymlinks = false\n\
                       \tignorecase = true\n[remote \"origin\"]\n\turl = C:/x\n\
                       \tfetch = +refs/heads/*:refs/remotes/origin/*\n";
        assert!(!config_text_forces_git(default));
        assert!(!config_text_forces_git(
            "[core]\r\n\tbare = No ; comment\r\n"
        ));
        assert!(!config_text_forces_git(
            "[core]\n\trepositoryformatversion = 1\n"
        ));

        for doubtful in [
            "[core]\n\tworktree = ../x\n",
            "[core] WorkTree = ../x\n",
            "[core]\n\tbare = true\n",
            "[core]\n\tbare\n",
            "[core]\n\tbare =\n",
            "[core]\n\tbare = \"false\"\n",
            "[extensions]\n\tfoo = bar\n",
            "[Extensions \"x\"]\n",
            "[core]\n\trepositoryformatversion = 2\n",
            "[core]\n\trepositoryformatversion\n",
            "[core\n",
            "[x \"]\"] bare = true\n",
            "[core]\n\tbare = false \\\n",
        ] {
            assert!(config_text_forces_git(doubtful), "{doubtful:?}");
        }
    }
}
