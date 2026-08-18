//! Native Claude Code statusline renderer.
//!
//! Two payloads, selected by argv:
//! * default — the `statusLine` payload; styles `full` (the original two-line
//!   port of `~/.claude/statusline.ps1`) and `min`.
//! * `subagent` — the `subagentStatusLine` payload; emits NDJSON row overrides
//!   for the agent panel.
//!
//! Claude Code invokes these on every refresh, per session; the pwsh script
//! this replaced cost ~1s of cold-start CPU each time. Output is raw UTF-8 with
//! ANSI colour codes and no trailing newline, exactly like the ps1's
//! `Write-Host -NoNewline`.

mod ansi;
mod cli;
mod fmt;
mod git;
mod payload;
mod render;
mod subagent;
mod teamdata;
#[cfg(test)]
mod testing;

use std::collections::HashSet;
use std::env;
use std::io::{Read, Write, stdout};
use std::path::Path;
use std::process::ExitCode;

use cli::{Cli, MainStyle, Mode, Parsed};
use payload::Payload;
use subagent::SubagentPayload;

fn main() -> ExitCode {
    let cli = match cli::parse(env::args().skip(1)) {
        Parsed::Run(c) => c,
        Parsed::Print(s) => {
            let _ = write!(stdout().lock(), "{s}");
            return ExitCode::SUCCESS;
        }
        Parsed::Fail(s) => {
            eprint!("{s}");
            return ExitCode::from(2);
        }
    };

    let mut input = String::new();
    let _ = std::io::stdin().read_to_string(&mut input);

    // In situ this binary is silent by design — a bad payload renders `Claude`
    // or nothing at all, and every disk read degrades quietly — so a wrong line
    // is indistinguishable from a correct one about nothing. Two lines on a
    // cold branch turn any field report into a deterministic offline repro.
    // Written after stdin and before rendering, so a panic or a hang in a
    // renderer still leaves the payload on disk; a failure to write is ignored,
    // because a diagnostic that kills the status line is worse than none.
    if let Ok(path) = env::var("STATUSLINE_DUMP")
        && !path.is_empty()
    {
        let _ = std::fs::write(&path, &input);
    }

    if cli.doctor {
        // Stderr, never stdout: stdout is the row protocol, and anything extra
        // there corrupts the status line or the agent panel.
        eprint!("{}", doctor_report(&cli, &input));
        return ExitCode::SUCCESS;
    }

    let line = match cli.mode {
        Mode::Main(style) => main_line(&input, style, cli.columns, cli.icon.as_deref()),
        Mode::Subagent(style) => {
            let now = chrono::Local::now().timestamp_millis();
            subagent_line(&input, style, cli.columns, now, teamdata::load)
        }
    };
    // `print!` panics when the write fails, and the release profile aborts
    // rather than unwinds — so a panel closing mid-refresh would surface in
    // `claude --debug` as a crash instead of a missing line. Every other
    // failure in this crate degrades silently; this one does too.
    let _ = write!(stdout().lock(), "{line}");
    ExitCode::SUCCESS
}

fn main_line(
    input: &str,
    style: MainStyle,
    columns: Option<usize>,
    icon: Option<&str>,
) -> String {
    // The ps1 would die mid-render on empty or bad JSON; degrade to the label.
    let Some(data) = parse_json::<Payload>(input) else {
        return "Claude".into();
    };
    let cols = resolve_columns(columns, env::var("COLUMNS").ok().as_deref());
    match style {
        MainStyle::Full => {
            let (branch, diff) = git_facts(&data, cols);
            render::full::render(&data, cols, branch.as_deref(), diff)
        }
        MainStyle::Min => {
            render::min::render(&data, cols, icon.unwrap_or(render::min::DEFAULT_ICON))
        }
    }
}

/// The main line's width budget: the flag, else `COLUMNS`, else a wide default.
///
/// `COLUMNS` is set by Claude Code; querying the console handle is unreliable in
/// piped contexts (same rationale as the ps1), so this falls back to a wide
/// default rather than probing. A zero width says nothing a renderer can honour
/// — every style still emits a line — so it falls back too, the way
/// `render::agents` reads its own width. The flag short-circuits before the
/// environment is consulted, so `--columns 0` lands on the default rather than
/// deferring to `COLUMNS`.
///
/// The env READ stays at the call site: edition 2024 makes `std::env::set_var`
/// an `unsafe fn`, so a test that drove `COLUMNS` for real would need `unsafe`
/// plus a process-wide lock against the multi-threaded test harness. Passing the
/// value in costs nothing and makes every branch reachable — including
/// `COLUMNS=abc`, which no test could reach while the read was inline.
fn resolve_columns(flag: Option<usize>, env: Option<&str>) -> usize {
    flag.or_else(|| env.and_then(|c| c.parse().ok()))
        .filter(|c| *c > 0)
        .unwrap_or(120)
}

/// The git facts [`render::full`] draws, resolved here rather than inside the
/// renderer so every style stays a pure `payload -> String` and the crate's disk
/// reads and its one subprocess spawn stay in `main` — the same reason
/// [`subagent_line`] takes `load_team` as a parameter.
///
/// The guards are not an optimisation, they are the pre-existing behaviour:
/// `full` draws the changes group only inside the branch group and only at
/// [`render::full::CHANGES_COLS`] or wider, and `git::diff_stats` spawns a real
/// `git` on a line that re-renders on every refresh. Resolving it at every width
/// would put that spawn on the hot path the crate exists to keep cheap.
fn git_facts(data: &Payload, cols: usize) -> (Option<String>, Option<(u64, u64)>) {
    let Some(cwd) = data.dir() else {
        return (None, None);
    };
    let branch = git::branch(Path::new(cwd));
    let diff = (branch.is_some() && cols >= render::full::CHANGES_COLS)
        .then(|| git::diff_stats(cwd))
        .flatten();
    (branch, diff)
}

/// The `subagent` counterpart to [`main_line`]. `load_team` is threaded in
/// rather than called directly so this stays a pure `input -> String` and the
/// crate's only disk reads stay in `main`; the payload is parsed once here
/// because the transcript path that keys the lookup comes out of it.
fn subagent_line(
    input: &str,
    style: render::agents::Style,
    columns: Option<usize>,
    now_ms: i64,
    load_team: fn(Option<&str>, &HashSet<&str>) -> teamdata::Team,
) -> String {
    // No output means "keep the default rendering" for every row, which is the
    // right failure mode for an unreadable or empty payload.
    let Some(p) = parse_json::<SubagentPayload>(input) else {
        return String::new();
    };
    // Agent type, teammate colour and inbox depth are not in the payload; they
    // come off disk, keyed by the transcript path.
    //
    // The names are handed down with it because `render::agents` only ever asks
    // the map about rows it is drawing, and one of the two files per member —
    // the inbox — is only read to answer that. The meta files still all have to
    // be read (the name is inside the file), but the inbox reads for a team
    // larger than the panel are pure waste, so the loader is told which ones
    // matter. `load_team` stays a parameter rather than a direct call so this
    // function remains pure and testable; only its type widened.
    let drawn: HashSet<&str> = p.tasks.iter().filter_map(|t| t.name.as_deref()).collect();
    let team = load_team(p.transcript_path.as_deref(), &drawn);
    render::agents::render(&p, style, columns, now_ms, &team)
}

/// `--doctor`. Everything this binary resolved before drawing anything, as
/// prose on stderr — the answer to "the badge is missing and nothing said why".
///
/// It reports after stdin is read rather than exiting early like `--help`,
/// because the two facts worth having — whether the subagents directory was
/// derivable and how many members parsed out of it — both come from the
/// payload. With no payload piped in it still resolves the claude dir and the
/// width, and says the rest is unavailable rather than inventing it.
fn doctor_report(cli: &Cli, input: &str) -> String {
    let mut out = String::from("statusline --doctor\n");
    for (key, value) in doctor_fields(cli, input) {
        out.push_str(&format!("  {key:<16}{value}\n"));
    }
    out
}

/// The report as label/value pairs, so the resolution logic is separable from
/// the layout — and so a test can assert on a field without matching prose.
fn doctor_fields(cli: &Cli, input: &str) -> Vec<(&'static str, String)> {
    let unresolved = |p: Option<std::path::PathBuf>, absent: &str| {
        p.map_or_else(|| absent.to_string(), |p| p.display().to_string())
    };
    let mut f: Vec<(&'static str, String)> = vec![
        ("version", env!("CARGO_PKG_VERSION").to_string()),
        ("claude dir", unresolved(teamdata::claude_dir(), "<unresolved>")),
    ];

    match cli.mode {
        Mode::Main(_) => {
            let env_cols = env::var("COLUMNS").ok();
            f.push(("mode", "main".to_string()));
            f.push((
                "columns",
                resolve_columns(cli.columns, env_cols.as_deref()).to_string(),
            ));
            f.push((
                "columns source",
                match (cli.columns, env_cols.as_deref()) {
                    (Some(n), _) if n > 0 => "--columns".to_string(),
                    (_, Some(c)) if c.parse::<usize>().is_ok_and(|n| n > 0) => {
                        format!("COLUMNS={c}")
                    }
                    _ => "default".to_string(),
                },
            ));
            f.push((
                "payload",
                match parse_json::<Payload>(input) {
                    Some(_) => "parsed".to_string(),
                    None => "unparseable or absent (renders the bare label)".to_string(),
                },
            ));
        }
        Mode::Subagent(_) => {
            f.push(("mode", "subagent".to_string()));
            let Some(p) = parse_json::<SubagentPayload>(input) else {
                f.push(("payload", "unparseable or absent (emits no rows)".to_string()));
                for key in ["columns source", "transcript", "subagents dir", "members parsed"] {
                    f.push((key, "<needs a payload on stdin>".to_string()));
                }
                return f;
            };
            f.push(("payload", format!("{} task(s)", p.tasks.len())));
            f.push((
                "columns source",
                match (cli.columns, p.columns) {
                    (Some(n), _) if n > 0 => "--columns".to_string(),
                    (_, Some(n)) if n > 0 => format!("payload columns={n}"),
                    _ => "default".to_string(),
                },
            ));
            let transcript = p.transcript_path.as_deref();
            f.push((
                "transcript",
                transcript.unwrap_or("<absent from payload>").to_string(),
            ));
            f.push((
                "subagents dir",
                unresolved(
                    transcript.and_then(teamdata::subagents_dir),
                    "<underivable>",
                ),
            ));
            let drawn: HashSet<&str> =
                p.tasks.iter().filter_map(|t| t.name.as_deref()).collect();
            // Zero here with a plausible directory above is the interesting
            // case: the containment guard refused it, the directory is empty,
            // or nothing in it parsed.
            f.push((
                "members parsed",
                teamdata::load(transcript, &drawn).len().to_string(),
            ));
        }
    }
    f
}

fn parse_json<T: serde::de::DeserializeOwned>(input: &str) -> Option<T> {
    if input.trim().is_empty() {
        return None;
    }
    serde_json::from_str(input).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use testing::plain;

    #[test]
    fn empty_and_malformed_input_degrade_to_the_bare_label() {
        for input in ["", "   ", "not json", "{oops"] {
            assert_eq!(main_line(input, MainStyle::Full, Some(120), None), "Claude");
            assert_eq!(main_line(input, MainStyle::Min, Some(120), None), "Claude");
        }
    }

    /// The disk lookup stubbed out: `subagent_line` is pure given a loader.
    fn no_team(_: Option<&str>, _: &HashSet<&str>) -> teamdata::Team {
        teamdata::Team::new()
    }

    #[test]
    fn empty_and_malformed_subagent_input_keeps_the_default_rows() {
        for input in ["", "   ", "not json", "{oops"] {
            let out =
                subagent_line(input, render::agents::Style::Tiers, Some(80), 0, no_team);
            assert_eq!(out, "", "no output means keep the defaults: {input:?}");
        }
    }

    #[test]
    fn a_parseable_subagent_payload_emits_one_row_per_task() {
        let input = r#"{"columns": 60, "tasks": [
            {"id": "a", "name": "t10-css-face-split", "status": "running"},
            {"id": "b", "name": "t11-token-budget", "status": "running"}
        ]}"#;
        let out = subagent_line(input, render::agents::Style::Tiers, None, 0, no_team);
        let rows: Vec<&str> = out.lines().collect();
        assert_eq!(rows.len(), 2, "one NDJSON row per task: {out:?}");
        assert!(rows[0].starts_with(r#"{"id":"a","content":"#), "{out:?}");
        assert!(rows[0].contains("t10-css-face-split"), "{out:?}");
    }

    #[test]
    fn the_two_main_styles_render_the_same_payload_differently() {
        let input = r#"{
            "workspace": {"repo": {"name": "dev-tools"}},
            "model": {"display_name": "Claude Opus 5"},
            "effort": {"level": "xhigh"},
            "context_window": {"total_input_tokens": 31000, "context_window_size": 200000,
                               "used_percentage": 15},
            "rate_limits": {"five_hour": {"used_percentage": 26},
                            "seven_day": {"used_percentage": 7}}
        }"#;
        let min = plain(&main_line(input, MainStyle::Min, Some(120), None));
        assert_eq!(min, "\u{273b} dev-tools \u{b7} 5h 26% \u{b7} week 7% \u{b7} chat 31k");

        let full = plain(&main_line(input, MainStyle::Full, Some(120), None));
        assert!(full.contains("Opus 5") && full.contains("xhigh") && full.contains("31k/200k"));
        assert!(full.contains('\n'), "full is two lines: {full:?}");
    }

    #[test]
    fn the_columns_flag_beats_the_environment() {
        let input = r#"{"workspace":{"repo":{"name":"dev-tools"}},
                        "context_window":{"total_input_tokens":31000},
                        "rate_limits":{"five_hour":{"used_percentage":26},
                                       "seven_day":{"used_percentage":7}}}"#;
        assert_eq!(
            plain(&main_line(input, MainStyle::Min, Some(31), None)),
            "\u{273b} dev-tools \u{b7} 5h 26% \u{b7} chat 31k"
        );
    }

    #[test]
    fn the_width_resolves_flag_then_environment_then_a_wide_default() {
        // The flag wins outright, including over a usable COLUMNS.
        assert_eq!(resolve_columns(Some(31), Some("100")), 31);
        assert_eq!(resolve_columns(None, Some("100")), 100);
        assert_eq!(resolve_columns(None, None), 120);
        // Zero says nothing a renderer can honour, from either source.
        assert_eq!(resolve_columns(Some(0), Some("100")), 120);
        assert_eq!(resolve_columns(None, Some("0")), 120);
        // A COLUMNS that is not a number at all — unreachable while the read
        // was inline, because edition 2024 makes `set_var` unsafe.
        assert_eq!(resolve_columns(None, Some("abc")), 120);
        assert_eq!(resolve_columns(None, Some("")), 120);
    }

    /// `--doctor` exists because a missing badge is otherwise indistinguishable
    /// from "no teammates". These pin the fields it must name, not their values.
    #[test]
    fn the_doctor_report_names_what_it_resolved() {
        let doctor = |argv: &[&str], input: &str| {
            let cli = match cli::parse(argv.iter().map(|s| s.to_string())) {
                Parsed::Run(c) => c,
                _ => panic!("expected a run for {argv:?}"),
            };
            doctor_fields(&cli, input)
                .into_iter()
                .collect::<std::collections::HashMap<_, _>>()
        };

        let main = doctor(&["--doctor", "--columns", "31"], "");
        assert_eq!(main["mode"], "main");
        assert_eq!(main["columns"], "31");
        assert_eq!(main["columns source"], "--columns");
        assert!(main.contains_key("claude dir"));

        let payload = r#"{"columns": 60, "transcript_path": "/p/proj/sess.jsonl",
            "tasks": [{"id": "a", "name": "t10", "status": "running"}]}"#;
        let sub = doctor(&["subagent", "--doctor"], payload);
        assert_eq!(sub["mode"], "subagent");
        assert_eq!(sub["payload"], "1 task(s)");
        assert_eq!(sub["columns source"], "payload columns=60");
        assert_eq!(sub["transcript"], "/p/proj/sess.jsonl");
        assert!(
            sub["subagents dir"].contains("sess") && sub["subagents dir"].ends_with("subagents"),
            "{:?}",
            sub["subagents dir"]
        );
        // The path is not inside any real Claude tree, so the containment
        // guard refuses it and the count is the honest zero.
        assert_eq!(sub["members parsed"], "0");

        // No payload: it still reports what is resolvable and says so.
        let bare = doctor(&["subagent", "--doctor"], "");
        assert!(bare["payload"].contains("unparseable or absent"));
        assert!(bare["members parsed"].contains("needs a payload"));
    }

    #[test]
    fn a_zero_width_falls_back_rather_than_shedding_everything() {
        let input = r#"{"workspace":{"repo":{"name":"dev-tools"}},
                        "context_window":{"total_input_tokens":31000},
                        "rate_limits":{"five_hour":{"used_percentage":26},
                                       "seven_day":{"used_percentage":7}}}"#;
        assert_eq!(
            plain(&main_line(input, MainStyle::Min, Some(0), None)),
            plain(&main_line(input, MainStyle::Min, Some(120), None))
        );
    }
}
