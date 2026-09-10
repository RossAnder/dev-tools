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

/// One flag cell of a `| Flag | … |` table, carrying the subcommand path the
/// nearest enclosing backticked heading attributes it to.
struct DocumentedFlag {
    file: String,
    /// Heading path with any leading `tomlctl` dropped, e.g.
    /// `["tasks", "import-plan"]`.
    path: Vec<String>,
    /// Long name without its leading dashes, as `Arg::get_long` spells it.
    long: String,
}

/// A heading that is one backticked span and nothing else — the reference
/// docs' spelling of "this section documents this verb". A heading carrying
/// prose around the backticks (`## The `check` verdict ladder`) names no verb
/// and must not lend its context to a table below it.
fn subcommand_heading_path(line: &str) -> Option<Vec<String>> {
    let rest = line.trim_start_matches('#');
    let inner = rest.trim().strip_prefix('`')?.strip_suffix('`')?;
    if inner.contains('`') {
        return None;
    }
    let mut tokens: Vec<String> = inner.split_whitespace().map(str::to_string).collect();
    if tokens.first().is_some_and(|t| t == "tomlctl") {
        tokens.remove(0);
    }
    (!tokens.is_empty()).then_some(tokens)
}

/// The first cell of a table row. `\|` inside a cell is an escaped literal
/// (`` `S` \| `M` ``), not a column break.
fn first_table_cell(row: &str) -> &str {
    let body = row.strip_prefix('|').unwrap_or(row);
    let mut escaped = false;
    for (i, c) in body.char_indices() {
        match c {
            '\\' => escaped = !escaped,
            '|' if !escaped => return &body[..i],
            _ => escaped = false,
        }
    }
    body
}

/// Long names of the backticked spans in a cell. A cell naming two spellings
/// of one option (`` `--action` / `--action-file` ``) documents both.
fn cell_long_names(cell: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = cell;
    while let Some(open) = rest.find('`') {
        let after = &rest[open + 1..];
        let Some(close) = after.find('`') else { break };
        if let Some(name) = after[..close].strip_prefix("--")
            && !name.is_empty()
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        {
            out.push(name.to_string());
        }
        rest = &after[close + 1..];
    }
    out
}

/// Collect every flag-table cell in one document. Fenced blocks are skipped
/// so a `# comment` in a bash example cannot displace the heading a table
/// below the fence still belongs to.
fn documented_flags(text: &str, rel: &str) -> Vec<DocumentedFlag> {
    let mut out = Vec::new();
    let mut in_fence = false;
    let mut heading: Option<Vec<String>> = None;
    // The open flag table's subcommand path; `None` also covers a flag table
    // under a heading that names no verb, whose cells are unattributable.
    let mut table: Option<Vec<String>> = None;

    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            in_fence = !in_fence;
            table = None;
            continue;
        }
        if in_fence {
            continue;
        }
        if trimmed.starts_with("# ")
            || trimmed.starts_with("## ")
            || trimmed.starts_with("### ")
            || trimmed.starts_with("#### ")
        {
            heading = subcommand_heading_path(trimmed);
            table = None;
            continue;
        }
        if !trimmed.starts_with('|') {
            table = None;
            continue;
        }
        let cell = first_table_cell(trimmed).trim();
        if cell == "Flag" {
            table = heading.clone();
            continue;
        }
        let Some(path) = table.as_ref() else {
            continue;
        };
        // The `|---|---|` rule between header and body.
        if !cell.is_empty() && cell.chars().all(|c| c == '-' || c == ':') {
            continue;
        }
        for long in cell_long_names(cell) {
            out.push(DocumentedFlag {
                file: rel.to_string(),
                path: path.clone(),
                long,
            });
        }
    }

    out
}

/// Long names clap accepts for a subcommand path, or `None` when the path
/// names no subcommand. Args are unioned along the path so a `global` flag
/// declared at the root counts as accepted at every depth.
fn accepted_long_names(root: &clap::Command, path: &[String]) -> Option<Vec<String>> {
    let longs = |cmd: &clap::Command| -> Vec<String> {
        cmd.get_arguments()
            .filter_map(|a| a.get_long())
            .map(str::to_string)
            .collect()
    };
    let mut cmd = root;
    let mut out = longs(cmd);
    for token in path {
        cmd = cmd.find_subcommand(token.as_str())?;
        out.extend(longs(cmd));
    }
    Some(out)
}

/// Lint outcome over the flag tables of one file set.
struct FlagTableReport {
    /// Cells resolved and checked — zero over the live corpus would mean the
    /// table parser stopped matching, not that the docs are clean.
    checked: usize,
    /// (file, subcommand path, message).
    failures: Vec<(String, String, String)>,
}

fn flag_table_report(files: &[PathBuf], repo_root: &Path) -> FlagTableReport {
    use clap::CommandFactory as _;

    let mut root = Cli::command();
    root.build();

    let mut checked = 0usize;
    let mut failures = Vec::new();

    for file in files {
        let Ok(text) = fs::read_to_string(file) else {
            continue;
        };
        let rel = file
            .strip_prefix(repo_root)
            .unwrap_or(file)
            .to_string_lossy()
            .replace('\\', "/");

        for documented in documented_flags(&text, &rel) {
            checked += 1;
            let printed = documented.path.join(" ");
            match accepted_long_names(&root, &documented.path) {
                None => failures.push((
                    documented.file,
                    printed.clone(),
                    format!("no `tomlctl {printed}` subcommand exists"),
                )),
                Some(accepted) => {
                    if !accepted.contains(&documented.long) {
                        failures.push((
                            documented.file,
                            printed.clone(),
                            format!(
                                "`--{}` is not an argument of `tomlctl {printed}`",
                                documented.long
                            ),
                        ));
                    }
                }
            }
        }
    }

    FlagTableReport { checked, failures }
}

/// Doc↔CLI flag-drift guard over the markdown flag TABLES, which carry the
/// bulk of the documented flag surface while the fenced examples
/// `command_lint` parses carry one or two invocations per verb. Every
/// `| Flag | … |` row's flag cell is resolved against the real clap argument
/// set of the subcommand its backticked heading names.
///
/// One direction only: a documented flag that does not exist. The reverse —
/// an existing flag no table lists — is not assertable here, because the
/// shared read/write bundles are deliberately documented once in prose
/// rather than repeated in each verb's table.
#[test]
fn flag_table_lint() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let repo_root = crate_dir.parent().expect("repo root").to_path_buf();
    let claude_dir = repo_root.join("claude");
    if !claude_dir.exists() {
        eprintln!("flag_table_lint: claude/ dir not found, skipping");
        return;
    }

    let files = command_lint_scan_set(&claude_dir);
    let report = flag_table_report(&files, &repo_root);

    assert!(
        report.checked > 0,
        "flag_table_lint parsed no flag-table rows at all — the table parser \
         has stopped matching the reference docs' shape"
    );

    if !report.failures.is_empty() {
        let mut msg = String::new();
        msg.push_str(&format!(
            "flag_table_lint: {} doc↔CLI flag drift(s) found.\n",
            report.failures.len()
        ));
        msg.push_str(
            "Each line below is a markdown flag-table row whose flag the real \
             clap parser does not define on the subcommand its heading \
             names:\n",
        );
        for (f, path, e) in &report.failures {
            msg.push_str(&format!(
                "  {f}\n    verb:  tomlctl {path}\n    error: {e}\n"
            ));
        }
        panic!("{msg}");
    }
}

/// A table row naming a flag the verb does not define is drift, and a fenced
/// `#` comment between the heading and the table does not break the
/// attribution. Asserted over a temp tree so the live corpus cannot make it
/// pass by accident.
#[test]
fn flag_table_lint_reports_a_row_naming_an_absent_flag() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let references = root.join("claude").join("skills").join("x");
    fs::create_dir_all(&references).unwrap();

    let doc = references.join("drifted.md");
    fs::write(
        &doc,
        "## `tasks add`\n\
         \n\
         ```bash\n\
         # not a heading\n\
         tomlctl tasks add --slug <slug>\n\
         ```\n\
         \n\
         | Flag | Value | Meaning | Default |\n\
         |---|---|---|---|\n\
         | `--title` | text | Row title. | — |\n\
         | `--effort` | `S` \\| `M` \\| `L` | Sizing. | — |\n\
         | `--action` / `--action-file` | text / path | Action body. | empty |\n\
         | `--bogus-flag` | text | Drift. | — |\n",
    )
    .unwrap();

    let report = flag_table_report(std::slice::from_ref(&doc), root);
    assert_eq!(
        report.checked, 5,
        "every flag cell below the fence must still be attributed to the \
         heading above it: {:?}",
        report.failures
    );
    assert_eq!(
        report.failures.len(),
        1,
        "exactly the absent flag must be reported: {:?}",
        report.failures
    );
    assert_eq!(report.failures[0].1, "tasks add");
    assert!(
        report.failures[0].2.contains("--bogus-flag"),
        "the failure must name the drifted flag: {:?}",
        report.failures[0]
    );
}

/// A flag table under a heading that names no verb is skipped rather than
/// misattributed; a heading that LOOKS like a verb and is not is drift.
#[test]
fn flag_table_lint_attributes_only_backticked_verb_headings() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let references = root.join("claude").join("skills").join("x");
    fs::create_dir_all(&references).unwrap();

    let shared = references.join("shared.md");
    fs::write(
        &shared,
        "## Store target\n\
         \n\
         | Flag | Value | Meaning |\n\
         |---|---|---|\n\
         | `--slug` / `--file` | text | Store to resolve. |\n",
    )
    .unwrap();
    let report = flag_table_report(std::slice::from_ref(&shared), root);
    assert_eq!(
        report.checked, 0,
        "a table under a prose heading is unattributable and must be skipped"
    );
    assert!(report.failures.is_empty(), "{:?}", report.failures);

    let renamed = references.join("renamed.md");
    fs::write(
        &renamed,
        "## `tasks bogus-verb`\n\
         \n\
         | Flag | Value | Meaning |\n\
         |---|---|---|\n\
         | `--title` | text | Row title. |\n",
    )
    .unwrap();
    let report = flag_table_report(std::slice::from_ref(&renamed), root);
    assert_eq!(
        report.failures.len(),
        1,
        "a heading naming no subcommand must be reported, not silently \
         disable the table below it: {:?}",
        report.failures
    );
    assert!(
        report.failures[0].2.contains("tasks bogus-verb"),
        "{:?}",
        report.failures[0]
    );
}
