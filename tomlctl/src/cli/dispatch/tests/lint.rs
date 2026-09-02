//! Carrier/skill markdown lint tests for the dispatch layer.

use crate::cli::dispatch::*;
use std::fs;
use std::path::{Path, PathBuf};

/// The harness markdown `command_lint` parses, rooted at a `claude/` dir so
/// the set is testable against a temp tree rather than only the live repo.
///
/// Every skill body plus one level of `references/*.md`: an unscanned skill
/// is an ungated skill, so the skills glob carries no name prefix.
/// `templates/`, `scripts/` and anything nested deeper stay out — template
/// argv is placeholder text that is not meant to parse. std `read_dir`; no
/// glob crate is in the dependency tree.
fn command_lint_scan_set(claude_dir: &Path) -> Vec<PathBuf> {
    let md = |p: &Path| p.extension().and_then(|e| e.to_str()) == Some("md");
    let mut files: Vec<PathBuf> = Vec::new();

    if let Ok(entries) = fs::read_dir(claude_dir.join("skills")) {
        for entry in entries.flatten() {
            let skill_dir = entry.path();
            if !skill_dir.is_dir() {
                continue;
            }
            let body = skill_dir.join("SKILL.md");
            if body.is_file() {
                files.push(body);
            }
            if let Ok(refs) = fs::read_dir(skill_dir.join("references")) {
                for r in refs.flatten() {
                    let p = r.path();
                    if p.is_file() && md(&p) {
                        files.push(p);
                    }
                }
            }
        }
    }

    for dir in ["commands", "agents"] {
        if let Ok(entries) = fs::read_dir(claude_dir.join(dir)) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_file() && md(&p) {
                    files.push(p);
                }
            }
        }
    }

    files.sort();
    files
}

/// T5: carrier↔CLI flag-drift guard. Every `tomlctl …` invocation written
/// in the project's command/skill markdown is fed to the REAL clap `Cli`
/// parser; an `UnknownArgument` / `InvalidSubcommand` error is a lint
/// failure. This catches the class of bug that shipped in the pilot — a
/// `--flow` vs `--flow-override` mismatch that no review lens caught because
/// lenses read prose, they don't execute the parser.
///
/// What is NOT a failure: missing-required-argument / value-validation
/// errors. Doc snippets use placeholders (`<ledger>`, `<slug>`) for required
/// positionals, so a parse that fails only because a required value is
/// absent or bogus is expected and ignored.
///
/// Opt-out: a ```bash fence whose info-string carries the token
/// `ignore-command-lint` skips the whole block (for deliberately partial /
/// illustrative snippets). Same repo-root resolution + graceful-skip pattern
/// as `blocks_verify_reproduces_shell_hashes`.
#[test]
fn command_lint() {
    use clap::Parser as _;
    use clap::error::ErrorKind;

    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let repo_root = crate_dir.parent().expect("repo root").to_path_buf();
    let claude_dir = repo_root.join("claude");
    if !claude_dir.exists() {
        eprintln!("command_lint: claude/ dir not found, skipping");
        return;
    }

    let files = command_lint_scan_set(&claude_dir);

    // Collected lint failures: (file, logical-line, clap error rendering).
    let mut failures: Vec<(String, String, String)> = Vec::new();
    // Lines we skipped because the quote tokeniser choked — surfaced in the
    // report so an unparseable snippet doesn't silently vanish.
    let mut unbalanced: Vec<(String, String)> = Vec::new();

    for file in &files {
        let text = match fs::read_to_string(file) {
            Ok(t) => t,
            Err(_) => continue,
        };
        let rel = file
            .strip_prefix(&repo_root)
            .unwrap_or(file)
            .to_string_lossy()
            .replace('\\', "/");

        // Walk the file line-by-line tracking fence state. A bash block is
        // opened by a trimmed line starting with "```bash"; its info-string
        // is the remainder after that prefix. The block closes at the next
        // line whose trimmed form starts with "```".
        let mut in_bash = false;
        let mut skip_block = false;
        // Buffer for stitching shell line-continuations (trailing `\`).
        let mut cont = String::new();

        let lint_logical = |rel: &str,
                            logical: &str,
                            failures: &mut Vec<(String, String, String)>,
                            unbalanced: &mut Vec<(String, String)>| {
            let trimmed = logical.trim_start();
            // A pipe into tomlctl (`… | tomlctl items add …`) — take the
            // substring from that `tomlctl` so the heredoc/cat prefix and
            // its body don't masquerade as argv.
            let candidate = if let Some(idx) = trimmed.find("| tomlctl ") {
                &trimmed[idx + 2..]
            } else {
                trimmed
            };
            if !candidate.starts_with("tomlctl") {
                return;
            }
            let raw_tokens = match shell_words::split(candidate) {
                Ok(t) => t,
                Err(_) => {
                    unbalanced.push((rel.to_string(), candidate.to_string()));
                    return;
                }
            };
            // Strip shell plumbing that is NOT part of tomlctl's argv:
            // redirections (`2>/dev/null`, `>/dev/null`, `2>&1`) and the
            // heredoc opener (`<<'EOF'`). Everything from the first such
            // operator onward is shell syntax the shell consumes before
            // exec, so it must not be fed to clap. A bare `-` (the stdin
            // sentinel for `--ndjson -` / `--ops -`) is a real argv token
            // and is preserved.
            let is_shell_op = |t: &str| -> bool {
                t.starts_with("<<")
                    || t.starts_with("2>")
                    || t.starts_with("1>")
                    || t.starts_with('>')
                    || (t.starts_with('<') && t != "<")
            };
            let tokens: Vec<String> = raw_tokens
                .into_iter()
                .take_while(|t| !is_shell_op(t))
                .collect();
            if tokens.is_empty() {
                return;
            }
            // shell_words yields "tomlctl" as the first token, which is
            // exactly the program name clap's `try_parse_from` expects.
            if let Err(e) = Cli::try_parse_from(&tokens) {
                match e.kind() {
                    ErrorKind::UnknownArgument | ErrorKind::InvalidSubcommand => {
                        failures.push((
                            rel.to_string(),
                            candidate.to_string(),
                            e.to_string().lines().next().unwrap_or("").to_string(),
                        ));
                    }
                    // Missing-required / value-validation / help / version
                    // are all acceptable: placeholders mean required values
                    // are legitimately absent in docs.
                    _ => {}
                }
            }
        };

        for line in text.lines() {
            let trimmed = line.trim_start();
            if !in_bash {
                if let Some(info) = trimmed.strip_prefix("```bash") {
                    in_bash = true;
                    skip_block = info.contains("ignore-command-lint");
                    cont.clear();
                }
                continue;
            }
            // Inside a bash block.
            if trimmed.starts_with("```") {
                in_bash = false;
                skip_block = false;
                cont.clear();
                continue;
            }
            if skip_block {
                continue;
            }
            // Stitch shell line-continuations: a line ending in `\` joins
            // with the next.
            let body = line;
            if let Some(stripped) = body.strip_suffix('\\') {
                cont.push_str(stripped);
                cont.push(' ');
                continue;
            }
            let logical = if cont.is_empty() {
                body.to_string()
            } else {
                let mut full = std::mem::take(&mut cont);
                full.push_str(body);
                full
            };
            lint_logical(&rel, &logical, &mut failures, &mut unbalanced);
        }
    }

    if !unbalanced.is_empty() {
        eprintln!(
            "command_lint: {} line(s) skipped (unbalanced quotes in snippet):",
            unbalanced.len()
        );
        for (f, l) in &unbalanced {
            eprintln!("  {f}: {l}");
        }
    }

    if !failures.is_empty() {
        let mut msg = String::new();
        msg.push_str(&format!(
            "command_lint: {} carrier↔CLI flag/subcommand drift(s) found.\n",
            failures.len()
        ));
        msg.push_str(
            "Each line below is a `tomlctl …` invocation in the project \
             markdown that the real clap parser rejected as an unknown \
             argument or subcommand:\n",
        );
        for (f, l, e) in &failures {
            msg.push_str(&format!("  {f}\n    line:  {l}\n    error: {e}\n"));
        }
        panic!("{msg}");
    }
}

/// The scan set must reach a skill whose name carries no `flow-contract-`
/// prefix, and must reach one level into `references/` without descending
/// into `templates/`. Asserted over a temp tree so the live repo's contents
/// cannot make it pass by accident.
#[test]
fn command_lint_scan_set_includes_skill_and_reference_files() {
    let dir = tempfile::tempdir().unwrap();
    let claude_dir = dir.path();
    let skill = claude_dir.join("skills").join("x");
    fs::create_dir_all(skill.join("references")).unwrap();
    fs::create_dir_all(skill.join("templates")).unwrap();
    fs::create_dir_all(claude_dir.join("commands")).unwrap();
    fs::create_dir_all(claude_dir.join("agents")).unwrap();

    let body = skill.join("SKILL.md");
    let reference = skill.join("references").join("y.md");
    let template = skill.join("templates").join("z.md");
    let command = claude_dir.join("commands").join("c.md");
    let agent = claude_dir.join("agents").join("a.md");
    let not_markdown = skill.join("references").join("y.txt");
    for p in [
        &body,
        &reference,
        &template,
        &command,
        &agent,
        &not_markdown,
    ] {
        fs::write(p, "# fixture\n").unwrap();
    }

    let set = command_lint_scan_set(claude_dir);
    for p in [&body, &reference, &command, &agent] {
        assert!(
            set.contains(p),
            "scan set must include {}: {set:?}",
            p.display()
        );
    }
    for p in [&template, &not_markdown] {
        assert!(
            !set.contains(p),
            "scan set must exclude {}: {set:?}",
            p.display()
        );
    }
    let mut sorted = set.clone();
    sorted.sort();
    assert_eq!(set, sorted, "scan set must be returned sorted");
}

/// T1 (d): `--no-create` appears on a WRITE subcommand (`set`) and is
/// ABSENT from a READ-only subcommand (`get`). Driven through the real clap
/// parser — a write arm accepts `--no-create`; a read arm rejects it with
/// `UnknownArgument`. `Cli` is not `Debug`, so we inspect the `Result`
/// without `expect`/`unwrap` (which would require `Debug` on the Ok arm).
#[test]
fn no_create_flag_on_write_subcommands_only() {
    use clap::Parser as _;
    use clap::error::ErrorKind as ClapErrorKind;

    // WRITE path (`set`) must accept `--no-create`.
    let ok = Cli::try_parse_from(["tomlctl", "set", "/tmp/x.toml", "key", "val", "--no-create"]);
    assert!(
        ok.is_ok(),
        "`--no-create` must be accepted on the write subcommand `set`, got: {:?}",
        ok.err().map(|e| e.kind())
    );

    // READ path (`get`) must reject `--no-create` as an unknown argument.
    // Map to the clap error kind first so we never need `Debug` on `Cli`.
    let read_err_kind =
        Cli::try_parse_from(["tomlctl", "get", "/tmp/x.toml", "key", "--no-create"])
            .map_err(|e| e.kind())
            .err();
    assert_eq!(
        read_err_kind,
        Some(ClapErrorKind::UnknownArgument),
        "`--no-create` must NOT exist on the read subcommand `get` \
         (expected UnknownArgument), got: {read_err_kind:?}"
    );
}
