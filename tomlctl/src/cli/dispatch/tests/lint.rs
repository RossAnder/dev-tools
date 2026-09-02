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

/// Stand-in substituted for a documented `<placeholder>` before the argv
/// reaches clap. `1` parses as an integer, a float, a string and a path, so a
/// placeholder occupying a typed flag's value slot cannot abort the parse
/// before the flags written after it.
const PLACEHOLDER_STAND_IN: &str = "1";

/// A shell token's role in a documented invocation.
enum TokenRole {
    /// Plumbing the shell consumes before exec: argv ends at this token.
    ShellOp,
    /// A `<name>` placeholder, rendered with the stand-in substituted.
    Placeholder(String),
    Argv,
}

/// Peel the optionality brackets and repetition ellipsis a usage synopsis
/// wraps a flag in (`[--branch <branch>]`, `[--scope <glob>]...`) so the flag
/// inside is still parsed — a misspelt `[--brnch]` must stay a lint failure.
/// A JSON array passed as a flag value loses its outer brackets here too,
/// which is inert: clap only checks that the value is present.
fn strip_synopsis_notation(token: &str) -> &str {
    let token = token.strip_prefix('[').unwrap_or(token);
    let token = token.strip_suffix("...").unwrap_or(token);
    token.strip_suffix(']').unwrap_or(token)
}

/// `<` opens either a redirection or a documentation placeholder, and only
/// the closing bracket separates them: `<slug>` and `<slug>/context.toml` are
/// argv, while a bare `<`, `<>` and `<file` are redirections.
fn classify_token(token: &str) -> TokenRole {
    if token.starts_with("<<")
        || token.starts_with("2>")
        || token.starts_with("1>")
        || token.starts_with('>')
        || token == "<"
    {
        return TokenRole::ShellOp;
    }
    let Some(rest) = token.strip_prefix('<') else {
        return TokenRole::Argv;
    };
    match rest.find('>') {
        Some(0) | None => TokenRole::ShellOp,
        Some(close) => {
            TokenRole::Placeholder(format!("{PLACEHOLDER_STAND_IN}{}", &rest[close + 1..]))
        }
    }
}

/// Lint outcome over one file set.
#[derive(Default)]
struct CommandLintReport {
    /// (file, logical line, first line of the clap error rendering).
    failures: Vec<(String, String, String)>,
    /// Lines skipped because the quote tokeniser choked — surfaced so an
    /// unparseable snippet doesn't silently vanish.
    unbalanced: Vec<(String, String)>,
}

/// Feed one logical shell line to the real parser when it invokes `tomlctl`.
fn lint_logical(rel: &str, logical: &str, report: &mut CommandLintReport) {
    use clap::Parser as _;
    use clap::error::ErrorKind;

    let trimmed = logical.trim_start();
    // A pipe into tomlctl (`… | tomlctl items add …`) — take the substring
    // from that `tomlctl` so the heredoc/cat prefix and its body don't
    // masquerade as argv.
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
            report
                .unbalanced
                .push((rel.to_string(), candidate.to_string()));
            return;
        }
    };
    // Everything from the first redirection or heredoc opener onward is
    // shell syntax consumed before exec, so it must not reach clap. A bare
    // `-` (the stdin sentinel for `--ndjson -` / `--ops -`) is a real argv
    // token and is preserved.
    let mut tokens: Vec<String> = Vec::new();
    for raw in &raw_tokens {
        let token = strip_synopsis_notation(raw);
        // A token that was pure notation (`...`) is not argv; an argument
        // written as an empty string is.
        if token.is_empty() && !raw.is_empty() {
            continue;
        }
        match classify_token(token) {
            TokenRole::ShellOp => break,
            TokenRole::Placeholder(stand_in) => tokens.push(stand_in),
            TokenRole::Argv => tokens.push(token.to_string()),
        }
    }
    if tokens.is_empty() {
        return;
    }
    // shell_words yields "tomlctl" as the first token, which is exactly the
    // program name clap's `try_parse_from` expects.
    if let Err(e) = Cli::try_parse_from(&tokens) {
        match e.kind() {
            ErrorKind::UnknownArgument | ErrorKind::InvalidSubcommand => {
                report.failures.push((
                    rel.to_string(),
                    candidate.to_string(),
                    e.to_string().lines().next().unwrap_or("").to_string(),
                ));
            }
            // Missing-required / value-validation / help / version are all
            // acceptable: placeholders mean required values are legitimately
            // absent or type-mismatched in docs.
            _ => {}
        }
    }
}

/// Walk each file's ```bash fences and lint every `tomlctl …` line in them.
fn command_lint_report(files: &[PathBuf], repo_root: &Path) -> CommandLintReport {
    let mut report = CommandLintReport::default();

    for file in files {
        let Ok(text) = fs::read_to_string(file) else {
            continue;
        };
        let rel = file
            .strip_prefix(repo_root)
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
            lint_logical(&rel, &logical, &mut report);
        }
    }

    report
}

/// Carrier↔CLI flag-drift guard. Every `tomlctl …` invocation written
/// in the project's command/skill markdown is fed to the REAL clap `Cli`
/// parser; an `UnknownArgument` / `InvalidSubcommand` error is a lint
/// failure. This catches `--flow` vs `--flow-override`-shaped drift that no
/// review lens sees, because lenses read prose and don't execute the parser.
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
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let repo_root = crate_dir.parent().expect("repo root").to_path_buf();
    let claude_dir = repo_root.join("claude");
    if !claude_dir.exists() {
        eprintln!("command_lint: claude/ dir not found, skipping");
        return;
    }

    let files = command_lint_scan_set(&claude_dir);
    let report = command_lint_report(&files, &repo_root);

    if !report.unbalanced.is_empty() {
        eprintln!(
            "command_lint: {} line(s) skipped (unbalanced quotes in snippet):",
            report.unbalanced.len()
        );
        for (f, l) in &report.unbalanced {
            eprintln!("  {f}: {l}");
        }
    }

    if !report.failures.is_empty() {
        let mut msg = String::new();
        msg.push_str(&format!(
            "command_lint: {} carrier↔CLI flag/subcommand drift(s) found.\n",
            report.failures.len()
        ));
        msg.push_str(
            "Each line below is a `tomlctl …` invocation in the project \
             markdown that the real clap parser rejected as an unknown \
             argument or subcommand:\n",
        );
        for (f, l, e) in &report.failures {
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

/// `--no-create` appears on a WRITE subcommand (`set`) and is
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

/// A flag written after a `<placeholder>` still reaches the parser: the
/// placeholder is argv, not a redirection that ends the command line. Asserted
/// over a temp tree so the live corpus cannot make it pass by accident.
#[test]
fn command_lint_checks_flags_written_after_a_placeholder() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let commands = root.join("claude").join("commands");
    fs::create_dir_all(&commands).unwrap();

    let drifted = commands.join("drifted.md");
    fs::write(
        &drifted,
        "```bash\ntomlctl flow init --slug <slug> --bogus-flag\n```\n",
    )
    .unwrap();
    let report = command_lint_report(std::slice::from_ref(&drifted), root);
    assert_eq!(
        report.failures.len(),
        1,
        "a bogus flag written after a placeholder must be reported: {:?}",
        report.failures
    );
    // clap names the offending flag but not the invocation it came from, so
    // the report carries the source line alongside the rejection.
    assert_eq!(
        report.failures[0].1, "tomlctl flow init --slug <slug> --bogus-flag",
        "the failure must quote the offending line: {:?}",
        report.failures[0]
    );
    assert!(
        report.failures[0]
            .2
            .contains("unexpected argument '--bogus-flag' found"),
        "the failure must name the drifted flag, not just the rejection: {:?}",
        report.failures[0]
    );
}

/// The placeholder substitution must not swallow shell plumbing: a
/// redirection, a heredoc opener and a bare `<` still end the argv, and a
/// well-formed invocation carrying placeholders is not drift.
#[test]
fn command_lint_still_truncates_at_shell_plumbing() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let commands = root.join("claude").join("commands");
    fs::create_dir_all(&commands).unwrap();

    let clean = commands.join("clean.md");
    fs::write(
        &clean,
        "```bash\n\
         tomlctl flow init --slug <slug> --plan docs/plans/<slug>.md\n\
         tomlctl items add-many <ledger> --ndjson - <<'EOF'\n\
         tomlctl flow active > /dev/null --bogus-flag\n\
         tomlctl flow active 2>/dev/null --bogus-flag\n\
         tomlctl flow active < --bogus-flag\n\
         ```\n",
    )
    .unwrap();

    let report = command_lint_report(std::slice::from_ref(&clean), root);
    assert!(
        report.failures.is_empty(),
        "shell plumbing must still end the argv: {:?}",
        report.failures
    );
    assert!(
        report.unbalanced.is_empty(),
        "fixture must tokenise cleanly: {:?}",
        report.unbalanced
    );
}
