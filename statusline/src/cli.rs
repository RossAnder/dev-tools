//! Argument parsing. Hand-rolled rather than pulled from a crate: the binary
//! runs on every status line refresh, so its startup cost is the whole point.

use crate::render::agents;

pub const HELP: &str = "\
statusline — native renderer for the Claude Code status line

USAGE
    statusline [--style <STYLE>]            main status line (statusLine)
    statusline subagent [--style <STYLE>]   agent-panel rows (subagentStatusLine)

    `subagents` and `agents` are accepted as aliases for `subagent`.

    Reads the matching JSON payload on stdin and writes the rendered output on
    stdout with no trailing newline.

STYLES (main)
    full    two colour-coded lines: cwd@branch (+/-) | model | effort | tokens,
            then 5h/7d rate-limit dots with reset times and API busy time
    min     one monotone line: repo, 5h, week, session context tokens
            (alias: minimal)

STYLES (subagent)
    tiers   name, activity, badge, @N inbox, tokens, stalled, runtime — shed
            lowest-first: activity, model/effort (rich), runtime, badge,
            stalled. Tokens and @N are protected; the name is never shed, only
            clipped, and only last
    rich    as tiers, plus resolved model and effort when the width allows

OPTIONS
    -s, --style <STYLE>   style to render (default: full / tiers)
        --columns <N>     override the width budget; otherwise COLUMNS for the
                          main line and the payload's `columns` for subagents
        --icon <TEXT>     `min` only: replace the leading glyph. Pass an empty
                          string to drop it. Avoid codepoints carrying the
                          Unicode Emoji property (U+2733, U+2734, U+2728,
                          U+2747) — terminals render those from a colour-emoji
                          font instead of your monospace face
        --list-styles     print the style names and exit
        --doctor          read the payload, then write what this binary
                          resolved — claude dir, subagents dir, parsed member
                          count, width and where it came from — to STDERR and
                          exit 0 without drawing a row. Stdout is the row
                          protocol, so nothing diagnostic may go there
    -h, --help            print this help and exit
    -V, --version         print the version and exit

DIAGNOSTICS
    STATUSLINE_DUMP=<path>  write the raw stdin payload to <path> before
                            rendering, so a bad line can be replayed offline.
                            A write failure is ignored, like every other
                            failure in this binary

SETUP
    ~/.claude/settings.json
      \"statusLine\":         {\"type\": \"command\", \"command\": \"<path>/statusline --style min\"}
      \"subagentStatusLine\": {\"type\": \"command\", \"command\": \"<path>/statusline subagent\"}
";

pub const STYLE_LIST: &str = "\
main:     full (default), min (alias: minimal)
subagent: tiers (default), rich
          mode word: subagent (aliases: subagents, agents)
";

#[derive(Debug, PartialEq, Eq)]
pub enum MainStyle {
    Full,
    Min,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Mode {
    Main(MainStyle),
    Subagent(agents::Style),
}

#[derive(Debug, PartialEq, Eq)]
pub struct Cli {
    pub mode: Mode,
    pub columns: Option<usize>,
    /// `min` only. `None` keeps the built-in glyph; `Some("")` drops it.
    pub icon: Option<String>,
    /// `--doctor`: report what was resolved on stderr instead of drawing.
    ///
    /// Not a [`Parsed::Print`] like `--help` is, because the report includes
    /// the parsed member count, which needs the payload — so it has to survive
    /// parsing and be acted on after stdin is read. It also has to reach
    /// stderr, and `Parsed::Print` is the stdout arm.
    pub doctor: bool,
}

/// What `main` should do with the parse result.
pub enum Parsed {
    Run(Cli),
    /// Print to stdout and exit 0.
    Print(String),
    /// Print to stderr and exit 2.
    Fail(String),
}

pub fn parse<I: IntoIterator<Item = String>>(args: I) -> Parsed {
    let mut args = args.into_iter().peekable();
    let mut subagent = false;
    let mut style: Option<String> = None;
    let mut columns: Option<usize> = None;
    let mut icon: Option<String> = None;
    let mut doctor = false;

    // Mode is a leading bare word, so an unflagged invocation stays the main
    // status line exactly as before.
    if let Some(first) = args.peek()
        && matches!(first.as_str(), "subagent" | "subagents" | "agents")
    {
        subagent = true;
        args.next();
    }

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => return Parsed::Print(HELP.to_string()),
            "-V" | "--version" => {
                return Parsed::Print(format!("statusline {}\n", env!("CARGO_PKG_VERSION")));
            }
            "--list-styles" => return Parsed::Print(STYLE_LIST.to_string()),
            "--doctor" => doctor = true,
            "--icon" => match args.next() {
                Some(v) => icon = Some(v),
                None => return Parsed::Fail("--icon needs a value\n".into()),
            },
            "-s" | "--style" => match args.next() {
                Some(v) => style = Some(v),
                None => return Parsed::Fail("--style needs a value\n".into()),
            },
            "--columns" => match args.next().map(|v| v.parse::<usize>()) {
                Some(Ok(n)) => columns = Some(n),
                Some(Err(_)) => return Parsed::Fail("--columns needs a number\n".into()),
                None => return Parsed::Fail("--columns needs a value\n".into()),
            },
            other => {
                if let Some(v) = other.strip_prefix("--icon=") {
                    icon = Some(v.to_string());
                } else if let Some(v) = other.strip_prefix("--style=") {
                    style = Some(v.to_string());
                } else if let Some(v) = other.strip_prefix("--columns=") {
                    match v.parse::<usize>() {
                        Ok(n) => columns = Some(n),
                        Err(_) => return Parsed::Fail("--columns needs a number\n".into()),
                    }
                } else {
                    return Parsed::Fail(format!("unknown argument: {other}\n\n{HELP}"));
                }
            }
        }
    }

    let mode = if subagent {
        match style.as_deref() {
            None | Some("tiers") => Mode::Subagent(agents::Style::Tiers),
            Some("rich") => Mode::Subagent(agents::Style::Rich),
            Some(s) => {
                return Parsed::Fail(format!("unknown subagent style: {s}\n\n{STYLE_LIST}"));
            }
        }
    } else {
        match style.as_deref() {
            None | Some("full") => Mode::Main(MainStyle::Full),
            Some("min") | Some("minimal") => Mode::Main(MainStyle::Min),
            Some(s) => return Parsed::Fail(format!("unknown style: {s}\n\n{STYLE_LIST}")),
        }
    };

    // `icon` is threaded only into `min`, so anywhere else it would render as
    // if it were absent. Reject it the way a cross-mode `--style` is rejected
    // rather than accepting it and doing nothing.
    if icon.is_some() && mode != Mode::Main(MainStyle::Min) {
        return Parsed::Fail("--icon applies to --style min only\n".into());
    }

    Parsed::Run(Cli { mode, columns, icon, doctor })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(args: &[&str]) -> Cli {
        match parse(args.iter().map(|s| s.to_string())) {
            Parsed::Run(c) => c,
            Parsed::Print(s) => panic!("expected a run, printed: {s}"),
            Parsed::Fail(s) => panic!("expected a run, failed: {s}"),
        }
    }

    fn fails(args: &[&str]) -> String {
        match parse(args.iter().map(|s| s.to_string())) {
            Parsed::Fail(s) => s,
            _ => panic!("expected a failure for {args:?}"),
        }
    }

    #[test]
    fn no_args_is_the_pre_existing_behaviour() {
        assert_eq!(
            run(&[]),
            Cli {
                mode: Mode::Main(MainStyle::Full),
                columns: None,
                icon: None,
                doctor: false,
            }
        );
    }

    #[test]
    fn doctor_is_a_flag_that_survives_parsing_rather_than_printing() {
        // `--help` returns before stdin is read; `--doctor` cannot, because its
        // report counts members off the payload.
        assert!(run(&["--doctor"]).doctor);
        assert!(run(&["subagent", "--doctor"]).doctor);
        assert!(!run(&["subagent"]).doctor);
        // It composes with the rest rather than swallowing them.
        let cli = run(&["--doctor", "--style", "min", "--columns", "40"]);
        assert_eq!(cli.mode, Mode::Main(MainStyle::Min));
        assert_eq!(cli.columns, Some(40));
    }

    /// The diagnostics are only reachable if they are written down.
    #[test]
    fn the_diagnostic_affordances_are_documented_in_the_help() {
        assert!(documents(HELP, "--doctor"), "HELP never mentions `--doctor`");
        assert!(
            documents(HELP, "STATUSLINE_DUMP"),
            "HELP never mentions `STATUSLINE_DUMP`"
        );
    }

    #[test]
    fn styles_select_per_mode() {
        assert_eq!(run(&["--style", "min"]).mode, Mode::Main(MainStyle::Min));
        assert_eq!(run(&["--style=minimal"]).mode, Mode::Main(MainStyle::Min));
        assert_eq!(run(&["-s", "full"]).mode, Mode::Main(MainStyle::Full));
        assert_eq!(run(&["subagent"]).mode, Mode::Subagent(agents::Style::Tiers));
        assert_eq!(
            run(&["subagent", "--style", "rich"]).mode,
            Mode::Subagent(agents::Style::Rich)
        );
    }

    #[test]
    fn columns_override_parses_both_spellings() {
        assert_eq!(run(&["--columns", "42"]).columns, Some(42));
        assert_eq!(run(&["--columns=42"]).columns, Some(42));
    }

    #[test]
    fn a_style_valid_in_the_other_mode_is_rejected_not_silently_defaulted() {
        assert!(fails(&["--style", "tiers"]).contains("unknown style"));
        assert!(fails(&["subagent", "--style", "min"]).contains("unknown subagent style"));
    }

    #[test]
    fn icon_override_parses_both_spellings_and_accepts_empty() {
        let icon = |a: &[&str]| run(a).icon;
        assert_eq!(icon(&["-s", "min", "--icon", "\u{2736}"]).as_deref(), Some("\u{2736}"));
        assert_eq!(icon(&["-s", "min", "--icon=\u{2736}"]).as_deref(), Some("\u{2736}"));
        // Empty is meaningful — "no glyph" — and must not collapse to the default.
        assert_eq!(icon(&["-s", "min", "--icon", ""]).as_deref(), Some(""));
        assert_eq!(icon(&[]), None);
    }

    #[test]
    fn an_icon_outside_min_is_rejected_not_silently_ignored() {
        assert!(fails(&["--icon", "\u{2736}"]).contains("--icon applies to --style min"));
        assert!(fails(&["subagent", "--icon=\u{2736}"]).contains("--icon applies to --style min"));
    }

    #[test]
    fn bad_input_is_reported() {
        assert!(fails(&["--style"]).contains("needs a value"));
        assert!(fails(&["--icon"]).contains("needs a value"));
        assert!(fails(&["--columns", "wide"]).contains("needs a number"));
        assert!(fails(&["--nope"]).contains("unknown argument"));
    }

    /// Every name `parse` accepts as a style or as the mode word, paired with
    /// the invocation that selects it and the mode it must resolve to. This is
    /// the single list the two gate tests below are driven from: adding a style
    /// means adding one row here, and a row whose name is missing from `HELP`
    /// or `STYLE_LIST` fails `every_accepted_name_is_documented`.
    fn accepted_names() -> Vec<(&'static str, Vec<&'static str>, Mode)> {
        vec![
            ("full", vec!["--style", "full"], Mode::Main(MainStyle::Full)),
            ("min", vec!["--style", "min"], Mode::Main(MainStyle::Min)),
            ("minimal", vec!["--style", "minimal"], Mode::Main(MainStyle::Min)),
            (
                "tiers",
                vec!["subagent", "--style", "tiers"],
                Mode::Subagent(agents::Style::Tiers),
            ),
            (
                "rich",
                vec!["subagent", "--style", "rich"],
                Mode::Subagent(agents::Style::Rich),
            ),
            ("subagent", vec!["subagent"], Mode::Subagent(agents::Style::Tiers)),
            ("subagents", vec!["subagents"], Mode::Subagent(agents::Style::Tiers)),
            ("agents", vec!["agents"], Mode::Subagent(agents::Style::Tiers)),
        ]
    }

    /// Whole-word containment: plain `contains` would let `min` ride in on
    /// `minimal`, so a documented alias could vanish without failing the gate.
    fn documents(doc: &str, name: &str) -> bool {
        let boundary = |c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-');
        doc.match_indices(name).any(|(i, _)| {
            let before = doc[..i].chars().next_back().is_none_or(boundary);
            let after = doc[i + name.len()..].chars().next().is_none_or(boundary);
            before && after
        })
    }

    #[test]
    fn every_accepted_name_resolves_to_its_documented_mode() {
        for (name, argv, expected) in accepted_names() {
            assert_eq!(run(&argv).mode, expected, "{name} resolved to the wrong mode");
        }
    }

    #[test]
    fn every_accepted_name_is_documented_in_both_the_help_and_the_style_list() {
        for (name, _, _) in accepted_names() {
            assert!(documents(STYLE_LIST, name), "STYLE_LIST never mentions `{name}`");
            assert!(documents(HELP, name), "HELP never mentions `{name}`");
        }
    }

    #[test]
    fn help_and_version_print_rather_than_run() {
        assert!(matches!(
            parse(["--help".to_string()]),
            Parsed::Print(s) if s.contains("USAGE")
        ));
        assert!(matches!(
            parse(["-V".to_string()]),
            Parsed::Print(s) if s.starts_with("statusline ")
        ));
    }
}
