//! Command-line parsing for glimpse's subcommands. Hand-rolled: the parser is small, and
//! `hook` runs once per Claude Code or Codex subagent event, so startup cost matters.

use std::path::PathBuf;

use crate::config::{OrientationPref, ViewKind};
use crate::surface::{Group, Sort, Surface};

pub(crate) const HELP: &str = "\
glimpse — live terminal view of a flow's task graph, checkpoints and running agents

USAGE
    glimpse [OPTIONS]                       open the live view
    glimpse hook [--harness H]              handle one subagent hook payload on stdin
    glimpse ensure-pane [--slug S] [--focus]
                                            open or reuse the glimpse pane in this herdr tab
    glimpse setup [--dry-run]               install the Claude Code and Codex hooks and the
                                            herdr keybinding

VIEW OPTIONS
        --slug <S>              open flow S; without it glimpse opens the freshest
                                flow and follows whichever flow changes next
        --view <V>              layers, ego or diagram (default: the last run's, else
                                the config's)
        --orientation <O>       auto, vertical or horizontal (default: the last run's
                                flip, else the config's)
        --surface <S>           tasks, review, optimise, plan-review, backlog or inbox:
                                the surface to open on (default: the last run's)
        --once                  render one frame as plain text to stdout and exit
        --size <WxH>            --once only: frame size in cells (default: 120x40)
        --snapshot <FILE>       --once only: render a `tomlctl tasks snapshot` JSON
                                file instead of reading the flow
        --select <ID>           --once only: select task ID instead of the frontier
        --ledger <FILE>         --once only: show this review, optimise, plan-review
                                or backlog ledger on its surface; without --snapshot
                                or --slug the task graph is left empty
        --group <F>             --once only: group the item surface's rows by F, one
                                of the fields `g` cycles through on that surface
        --sort <F>              --once only: sort the item surface's rows by F, one
                                of the orders `S` cycles through on that surface
        --closed                --once only: list closed rows, as `c` does

HOOK OPTIONS
        --harness <H>           claude-code or codex: which harness sent the payload
                                (default: claude-code)

ENSURE-PANE OPTIONS
        --slug <S>              launch the pane on flow S instead of the freshest
        --focus                 move focus to the pane

SETUP OPTIONS
        --dry-run               report what would change and write nothing

OPTIONS
    -h, --help                  print this help and exit
    -V, --version               print the version and exit

EXIT STATUS
    0 on success, 1 on a runtime failure, 2 on a usage error. `hook` always exits 0
    and reports failures, usage errors included, only to <claude dir>/glimpse/hook.log.
";

/// The `--once` frame size when `--size` is absent.
pub(crate) const DEFAULT_SIZE: (u16, u16) = (120, 40);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Once {
    pub(crate) width: u16,
    pub(crate) height: u16,
    pub(crate) snapshot: Option<PathBuf>,
    pub(crate) select: Option<u32>,
    pub(crate) ledger: Option<PathBuf>,
    pub(crate) arrange: Arrange,
}

/// How the `--once` frame's item surface lays out its rows, in place of the `g`, `S`
/// and `c` keys. The parser checks each value names some surface's option; whether the
/// surface on show offers it is checked once that surface is known.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Arrange {
    pub(crate) group: Option<Group>,
    pub(crate) sort: Option<Sort>,
    pub(crate) closed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ViewArgs {
    pub(crate) slug: Option<String>,
    pub(crate) view: Option<ViewKind>,
    pub(crate) orientation: Option<OrientationPref>,
    pub(crate) surface: Option<Surface>,
    pub(crate) once: Option<Once>,
}

/// The harness a hook payload came from; it selects the in-process recorder's payload
/// adapter.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Harness {
    #[default]
    ClaudeCode,
    Codex,
}

impl Harness {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Harness::ClaudeCode => "claude-code",
            Harness::Codex => "codex",
        }
    }

    fn parse(s: &str) -> Option<Harness> {
        match s {
            "claude-code" => Some(Harness::ClaudeCode),
            "codex" => Some(Harness::Codex),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Command {
    View(ViewArgs),
    Hook { harness: Harness },
    EnsurePane { slug: Option<String>, focus: bool },
    Setup { dry_run: bool },
}

/// What `main` should do with the parse result.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Parsed {
    Run(Command),
    /// Print to stdout and exit 0.
    Print(String),
    /// Print to stderr and exit 2.
    Fail(String),
}

pub(crate) fn parse<I: IntoIterator<Item = String>>(args: I) -> Parsed {
    match parse_inner(args) {
        Ok(parsed) => parsed,
        Err(msg) => Parsed::Fail(format!(
            "glimpse: {msg}\n\nRun `glimpse --help` for usage.\n"
        )),
    }
}

fn parse_inner<I: IntoIterator<Item = String>>(args: I) -> Result<Parsed, String> {
    let mut args = args.into_iter().peekable();
    let sub = match args.peek().map(String::as_str) {
        Some(word @ ("hook" | "ensure-pane" | "setup")) => {
            let word = word.to_string();
            args.next();
            Some(word)
        }
        _ => None,
    };

    let mut slug = None;
    let mut view = None;
    let mut orientation = None;
    let mut once = false;
    let mut size = None;
    let mut snapshot = None;
    let mut select = None;
    let mut surface = None;
    let mut ledger = None;
    let mut arrange = Arrange::default();
    let mut focus = false;
    let mut dry_run = false;
    let mut harness = Harness::default();

    while let Some(arg) = args.next() {
        let (name, inline) = match arg.split_once('=') {
            Some((n, v)) if n.starts_with("--") => (n.to_string(), Some(v.to_string())),
            _ => (arg.clone(), None),
        };
        let mut value = |flag: &str| match inline.clone().or_else(|| args.next()) {
            Some(v) => Ok(v),
            None => Err(format!("{flag} needs a value")),
        };
        let allowed = match (sub.as_deref(), name.as_str()) {
            (_, "-h" | "--help") => return Ok(Parsed::Print(HELP.to_string())),
            (_, "-V" | "--version") => {
                return Ok(Parsed::Print(format!(
                    "glimpse {}\n",
                    env!("CARGO_PKG_VERSION")
                )));
            }
            (None | Some("ensure-pane"), "--slug") => {
                let v = value("--slug")?;
                if v.is_empty() {
                    return Err("--slug must not be empty".to_string());
                }
                tomlctl::validate_slug(&v).map_err(|e| format!("--slug: {e:#}"))?;
                slug = Some(v);
                true
            }
            (None, "--view") => {
                let v = value("--view")?;
                view = Some(ViewKind::parse(&v).ok_or_else(|| {
                    format!("unknown view `{v}`: expected layers, ego or diagram")
                })?);
                true
            }
            (None, "--orientation") => {
                let v = value("--orientation")?;
                orientation = Some(OrientationPref::parse(&v).ok_or_else(|| {
                    format!("unknown orientation `{v}`: expected auto, vertical or horizontal")
                })?);
                true
            }
            (None, "--once") => {
                once = true;
                true
            }
            (None, "--size") => {
                size = Some(parse_size(&value("--size")?)?);
                true
            }
            (None, "--snapshot") => {
                snapshot = Some(PathBuf::from(value("--snapshot")?));
                true
            }
            (None, "--select") => {
                let v = value("--select")?;
                select = Some(
                    v.parse()
                        .map_err(|_| format!("--select must be a task id, got `{v}`"))?,
                );
                true
            }
            (None, "--surface") => {
                let v = value("--surface")?;
                surface = Some(Surface::from_key(&v).ok_or_else(|| {
                    format!(
                        "unknown surface `{v}`: expected tasks, review, optimise, plan-review, \
                         backlog or inbox"
                    )
                })?);
                true
            }
            (None, "--ledger") => {
                ledger = Some(PathBuf::from(value("--ledger")?));
                true
            }
            (None, "--group") => {
                let v = value("--group")?;
                arrange.group = Some(pick("group", &v, Surface::groups, Group::key)?);
                true
            }
            (None, "--sort") => {
                let v = value("--sort")?;
                arrange.sort = Some(pick("sort", &v, Surface::sorts, Sort::key)?);
                true
            }
            (None, "--closed") => {
                arrange.closed = true;
                true
            }
            (Some("ensure-pane"), "--focus") => {
                focus = true;
                true
            }
            (Some("setup"), "--dry-run") => {
                dry_run = true;
                true
            }
            (Some("hook"), "--harness") => {
                let v = value("--harness")?;
                harness = Harness::parse(&v).ok_or_else(|| {
                    format!("unknown harness `{v}`: expected claude-code or codex")
                })?;
                true
            }
            _ => false,
        };
        if !allowed {
            return Err(match sub.as_deref() {
                Some(word) => format!("unexpected argument `{arg}` for `{word}`"),
                None => format!("unknown argument `{arg}`"),
            });
        }
        if inline.is_some()
            && matches!(
                name.as_str(),
                "--once" | "--focus" | "--dry-run" | "--closed"
            )
        {
            return Err(format!("{name} takes no value"));
        }
    }

    let command = match sub.as_deref() {
        Some("hook") => Command::Hook { harness },
        Some("ensure-pane") => Command::EnsurePane { slug, focus },
        Some(_) => Command::Setup { dry_run },
        None => {
            if !once
                && (size.is_some()
                    || snapshot.is_some()
                    || select.is_some()
                    || ledger.is_some()
                    || arrange != Arrange::default())
            {
                return Err(
                    "--size, --snapshot, --select, --ledger, --group, --sort and \
                            --closed need --once"
                        .to_string(),
                );
            }
            let once = once.then(|| {
                let (width, height) = size.unwrap_or(DEFAULT_SIZE);
                Once {
                    width,
                    height,
                    snapshot,
                    select,
                    ledger,
                    arrange,
                }
            });
            Command::View(ViewArgs {
                slug,
                view,
                orientation,
                surface,
                once,
            })
        }
    };
    Ok(Parsed::Run(command))
}

/// Finds `value` among the options any surface's `options` lists, by `key`; the error
/// names every option in first-offered order.
fn pick<T: Copy + PartialEq>(
    what: &str,
    value: &str,
    options: fn(Surface) -> &'static [T],
    key: fn(T) -> &'static str,
) -> Result<T, String> {
    let mut known: Vec<T> = Vec::new();
    for option in Surface::ALL.into_iter().flat_map(options) {
        if !known.contains(option) {
            known.push(*option);
        }
    }
    known
        .iter()
        .copied()
        .find(|o| key(*o) == value)
        .ok_or_else(|| {
            let names: Vec<&str> = known.iter().map(|o| key(*o)).collect();
            format!(
                "unknown {what} `{value}`: expected one of {}",
                names.join(", ")
            )
        })
}

/// The `--once` frame's arrangement checked against what `surface` offers, so a
/// field the surface cannot group or sort by is refused rather than ignored.
pub(crate) fn check_arrange(surface: Surface, arrange: &Arrange) -> Result<(), String> {
    let refuse = |flag: &str, value: &str, offered: Vec<&str>| {
        Err(format!(
            "{flag} {value}: the {} surface offers {}",
            surface.key(),
            offered.join(", ")
        ))
    };
    if let Some(group) = arrange.group
        && !surface.groups().contains(&group)
    {
        let offered = surface.groups().iter().map(|g| g.key()).collect();
        return refuse("--group", group.key(), offered);
    }
    if let Some(sort) = arrange.sort
        && !surface.sorts().contains(&sort)
    {
        let offered = surface.sorts().iter().map(|s| s.key()).collect();
        return refuse("--sort", sort.key(), offered);
    }
    if arrange.closed && surface == Surface::Tasks {
        return Err("--closed: the tasks surface has no closed rows to list".to_string());
    }
    Ok(())
}

fn parse_size(s: &str) -> Result<(u16, u16), String> {
    let bad = || format!("--size must be WxH with both at least 1, got `{s}`");
    let (w, h) = s.split_once(['x', 'X']).ok_or_else(bad)?;
    let w: u16 = w.parse().map_err(|_| bad())?;
    let h: u16 = h.parse().map_err(|_| bad())?;
    if w == 0 || h == 0 {
        return Err(bad());
    }
    Ok((w, h))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Orientation;

    fn run(args: &[&str]) -> Command {
        match parse(args.iter().map(|s| s.to_string())) {
            Parsed::Run(c) => c,
            other => panic!("expected a run for {args:?}, got {other:?}"),
        }
    }

    fn fails(args: &[&str]) -> String {
        match parse(args.iter().map(|s| s.to_string())) {
            Parsed::Fail(s) => s,
            other => panic!("expected a failure for {args:?}, got {other:?}"),
        }
    }

    fn view(args: &[&str]) -> ViewArgs {
        match run(args) {
            Command::View(v) => v,
            other => panic!("expected the view for {args:?}, got {other:?}"),
        }
    }

    #[test]
    fn no_arguments_opens_the_freshest_flow_live() {
        assert_eq!(
            view(&[]),
            ViewArgs {
                slug: None,
                view: None,
                orientation: None,
                surface: None,
                once: None,
            }
        );
    }

    #[test]
    fn surface_and_ledger_parse() {
        assert_eq!(
            view(&["--surface", "plan-review"]).surface,
            Some(Surface::PlanReview)
        );
        assert_eq!(view(&["--surface=inbox"]).surface, Some(Surface::Inbox));
        let v = view(&["--once", "--ledger", "r.toml", "--surface", "review"]);
        assert_eq!(v.surface, Some(Surface::Review));
        assert_eq!(
            v.once.expect("--once parsed").ledger,
            Some(PathBuf::from("r.toml"))
        );
        assert!(fails(&["--surface", "Review"]).contains("unknown surface"));
        assert!(fails(&["--surface", "ledger"]).contains("unknown surface"));
        assert!(fails(&["--ledger", "r.toml"]).contains("--once"));
        assert!(fails(&["ensure-pane", "--surface", "review"]).contains("for `ensure-pane`"));
    }

    #[test]
    fn view_options_parse_in_both_spellings() {
        let v = view(&[
            "--slug",
            "demo",
            "--view=diagram",
            "--orientation",
            "horizontal",
        ]);
        assert_eq!(v.slug.as_deref(), Some("demo"));
        assert_eq!(v.view, Some(ViewKind::Diagram));
        assert_eq!(
            v.orientation,
            Some(OrientationPref::Fixed(Orientation::Horizontal))
        );
        assert_eq!(v.once, None);
    }

    #[test]
    fn once_takes_a_size_and_a_snapshot() {
        let v = view(&["--once", "--size", "100x30", "--snapshot", "s.json"]);
        assert_eq!(
            v.once,
            Some(Once {
                width: 100,
                height: 30,
                snapshot: Some(PathBuf::from("s.json")),
                select: None,
                ledger: None,
                arrange: Arrange::default(),
            })
        );
        let v = view(&["--once", "--select", "7"]);
        assert_eq!(v.once.expect("--once parsed").select, Some(7));
        let v = view(&["--once"]);
        let once = v.once.expect("--once parsed");
        assert_eq!((once.width, once.height), DEFAULT_SIZE);
    }

    #[test]
    fn size_and_snapshot_without_once_are_usage_errors() {
        assert!(fails(&["--size", "10x10"]).contains("--once"));
        assert!(fails(&["--snapshot", "s.json"]).contains("--once"));
        assert!(fails(&["--select", "7"]).contains("--once"));
        assert!(fails(&["--once", "--select", "seven"]).contains("task id"));
    }

    #[test]
    fn once_takes_a_group_a_sort_and_closed() {
        let once = view(&["--once", "--group", "area", "--sort=newest", "--closed"])
            .once
            .expect("--once parsed");
        assert_eq!(
            once.arrange,
            Arrange {
                group: Some(Group::Area),
                sort: Some(Sort::Newest),
                closed: true,
            }
        );
        let once = view(&["--once", "--group=severity"])
            .once
            .expect("--once parsed");
        assert_eq!(once.arrange.group, Some(Group::Severity));
        assert_eq!(once.arrange.sort, None);
        assert!(!once.arrange.closed);
    }

    #[test]
    fn group_sort_and_closed_are_checked_and_need_once() {
        let bad_group = fails(&["--once", "--group", "Severity"]);
        assert!(
            bad_group.contains("unknown group `Severity`"),
            "{bad_group}"
        );
        assert!(
            bad_group.contains("none, severity, category, effort, file, status, kind, area"),
            "{bad_group}"
        );
        let bad_sort = fails(&["--once", "--sort", "oldest"]);
        assert!(
            bad_sort.contains("expected one of id, severity, effort, newest"),
            "{bad_sort}"
        );
        assert!(fails(&["--once", "--group"]).contains("needs a value"));
        assert!(fails(&["--once", "--closed=yes"]).contains("takes no value"));
        for args in [
            &["--group", "status"][..],
            &["--sort", "id"][..],
            &["--closed"][..],
        ] {
            assert!(fails(args).contains("need --once"), "{args:?}");
        }
        assert!(fails(&["ensure-pane", "--closed"]).contains("for `ensure-pane`"));
    }

    #[test]
    fn the_arrangement_must_be_offered_by_the_surface() {
        let arrange = |group, sort, closed| Arrange {
            group,
            sort,
            closed,
        };
        assert!(
            check_arrange(
                Surface::Review,
                &arrange(Some(Group::File), Some(Sort::Effort), true)
            )
            .is_ok()
        );
        assert!(
            check_arrange(
                Surface::Tasks,
                &arrange(Some(Group::None), Some(Sort::Id), false)
            )
            .is_ok()
        );
        assert!(check_arrange(Surface::Inbox, &arrange(None, None, true)).is_ok());
        let err = check_arrange(
            Surface::Backlog,
            &arrange(Some(Group::Severity), None, false),
        )
        .expect_err("backlog rows carry no severity");
        assert_eq!(
            err,
            "--group severity: the backlog surface offers none, kind, area, status"
        );
        let err = check_arrange(
            Surface::PlanReview,
            &arrange(None, Some(Sort::Effort), false),
        )
        .expect_err("plan-review rows carry no effort");
        assert!(
            err.starts_with("--sort effort: the plan-review surface"),
            "{err}"
        );
        assert!(check_arrange(Surface::Tasks, &arrange(None, None, true)).is_err());
    }

    #[test]
    fn a_malformed_size_is_rejected() {
        for bad in ["100", "0x30", "100x0", "ax30", "100x", "70000x10"] {
            assert!(
                fails(&["--once", "--size", bad]).contains("--size"),
                "{bad}"
            );
        }
    }

    #[test]
    fn unknown_values_and_arguments_are_rejected() {
        assert!(fails(&["--view", "tree"]).contains("unknown view"));
        assert!(fails(&["--orientation", "diagonal"]).contains("unknown orientation"));
        assert!(fails(&["--bogus"]).contains("unknown argument"));
        assert!(fails(&["--slug"]).contains("needs a value"));
        assert!(fails(&["--slug", "../x"]).contains("invalid slug: ../x"));
        assert!(fails(&["ensure-pane", "--slug", "Foo"]).contains("invalid slug: Foo"));
        assert!(fails(&["--once=yes"]).contains("takes no value"));
    }

    #[test]
    fn subcommands_accept_only_their_own_flags() {
        assert_eq!(
            run(&["hook"]),
            Command::Hook {
                harness: Harness::ClaudeCode,
            }
        );
        assert_eq!(
            run(&["ensure-pane"]),
            Command::EnsurePane {
                slug: None,
                focus: false,
            }
        );
        assert_eq!(
            run(&["ensure-pane", "--slug", "demo", "--focus"]),
            Command::EnsurePane {
                slug: Some("demo".to_string()),
                focus: true,
            }
        );
        assert_eq!(run(&["setup"]), Command::Setup { dry_run: false });
        assert_eq!(
            run(&["setup", "--dry-run"]),
            Command::Setup { dry_run: true }
        );
        assert!(fails(&["hook", "--slug", "demo"]).contains("for `hook`"));
        assert!(fails(&["setup", "--focus"]).contains("for `setup`"));
        assert!(fails(&["ensure-pane", "--once"]).contains("for `ensure-pane`"));
        assert!(fails(&["--focus"]).contains("unknown argument"));
    }

    #[test]
    fn hook_takes_a_harness_in_both_spellings() {
        for args in [
            &["hook", "--harness", "codex"][..],
            &["hook", "--harness=codex"][..],
        ] {
            assert_eq!(
                run(args),
                Command::Hook {
                    harness: Harness::Codex,
                },
                "{args:?}"
            );
        }
        assert_eq!(
            run(&["hook", "--harness", "claude-code"]),
            Command::Hook {
                harness: Harness::ClaudeCode,
            }
        );
        assert_eq!(Harness::Codex.as_str(), "codex");
        assert_eq!(Harness::ClaudeCode.as_str(), "claude-code");
    }

    #[test]
    fn a_bad_harness_is_rejected() {
        assert!(fails(&["hook", "--harness", "manual"]).contains("unknown harness"));
        assert!(fails(&["hook", "--harness", "Codex"]).contains("unknown harness"));
        assert!(fails(&["hook", "--harness"]).contains("needs a value"));
        assert!(fails(&["--harness", "codex"]).contains("unknown argument"));
        assert!(fails(&["setup", "--harness", "codex"]).contains("for `setup`"));
    }

    #[test]
    fn help_and_version_print() {
        assert_eq!(
            parse(["--help".to_string()]),
            Parsed::Print(HELP.to_string())
        );
        assert!(matches!(
            parse(["setup".to_string(), "-h".to_string()]),
            Parsed::Print(_)
        ));
        match parse(["-V".to_string()]) {
            Parsed::Print(s) => assert!(s.starts_with("glimpse ")),
            other => panic!("expected the version, got {other:?}"),
        }
    }
}
