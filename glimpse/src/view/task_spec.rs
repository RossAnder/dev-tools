//! The sections a task with a spec gains in its details: what the spec holds, the
//! change trail's entries for the task, its recorded attempts, and a Links section
//! naming every related record the sections above have not already drawn.
//!
//! A task with no spec gets nothing from this module.

use ratatui::style::Style;
use ratatui::text::{Line, Span, Text};

use crate::app::App;
use crate::model::Task;
use crate::model::plan::{Probe, Spec};
use crate::nav::Target;
use crate::theme::Theme;
use crate::view::details::{INDENT, indented, section};
use crate::view::doc::Link;
use crate::view::markdown;

/// Appends the spec sections of `task` to `out` and their links to `links`, each
/// link once at its first mention. Nothing for a task without a spec.
pub(crate) fn sections(
    app: &App,
    task: &Task,
    out: &mut Vec<Line<'static>>,
    links: &mut Vec<Link>,
) {
    let Some(spec) = &task.spec else {
        return;
    };
    let theme = &app.theme;

    if !spec.outcome.trim().is_empty() {
        section(out, "Outcome", theme);
        out.extend(indented(markdown::to_text(&spec.outcome, theme)));
    }
    if !spec.constraints.is_empty() {
        section(out, "Constraints", theme);
        out.extend(indented(markdown::to_text(
            &bullets(&spec.constraints),
            theme,
        )));
    }
    means(out, spec, theme);
    if !spec.executor.is_empty() {
        section(out, "Executor", theme);
        out.push(Line::from(vec![
            Span::raw(INDENT),
            Span::styled(spec.executor.clone(), theme.badge),
        ]));
    }

    if !spec.design.is_empty() {
        section(out, "Design", theme);
        for entry in &spec.design {
            out.push(Line::styled(format!("{INDENT}{entry}"), theme.secondary));
            let file = entry
                .split_once('#')
                .map_or(entry.as_str(), |(file, _)| file);
            push(links, entry.clone(), Target::Authored(file.to_string()));
        }
    }
    if !spec.precedent.is_empty() {
        section(out, "Precedent", theme);
        for entry in &spec.precedent {
            out.push(Line::styled(format!("{INDENT}{entry}"), theme.secondary));
            let target = Target::Source {
                path: entry.clone(),
                line: None,
                read_at: String::new(),
                dirty: false,
            };
            push(links, entry.clone(), target);
        }
    }

    accept(out, spec, theme);
    premises(out, spec, theme);
    changes(out, app, task);
    attempts(out, app, task);
    link_section(out, links, app, task);
}

/// `items` as a markdown bullet list, so each renders through the markdown helper.
fn bullets(items: &[String]) -> String {
    items
        .iter()
        .map(|item| format!("- {item}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn push(links: &mut Vec<Link>, label: String, target: Target) {
    if !links.iter().any(|link| link.target == target) {
        links.push(Link { label, target });
    }
}

fn means(out: &mut Vec<Line<'static>>, spec: &Spec, theme: &Theme) {
    if spec.means.is_empty() {
        return;
    }
    section(out, "Means", theme);
    for means in &spec.means {
        let style = if means.binding == "required" {
            theme.warning
        } else {
            theme.secondary
        };
        out.push(Line::from(vec![
            Span::raw(INDENT),
            Span::styled(means.binding.clone(), style),
            Span::raw(format!(" {}", means.text)),
        ]));
        if !means.detail.trim().is_empty() {
            let detail =
                Text::from(indented(markdown::to_text(&means.detail, theme)).collect::<Vec<_>>());
            out.extend(indented(detail));
        }
    }
}

/// One accept entry or premise's verdict: the recorded probe's, `unprobed` for a
/// probeable entry with none recorded, `not probed` for one the emitter skips.
fn verdict(
    probeable: bool,
    probe: Option<&Probe>,
    good: &str,
    theme: &Theme,
) -> Vec<Span<'static>> {
    match (probeable, probe) {
        (false, _) => vec![Span::styled("not probed", theme.secondary)],
        (true, None) => vec![Span::styled("unprobed", theme.warning)],
        (true, Some(probe)) => {
            let style = if probe.verdict == good {
                theme.done
            } else {
                theme.failed
            };
            let mut spans = vec![Span::styled(probe.verdict.clone(), style)];
            if !probe.at.is_empty() {
                spans.push(Span::styled(format!(" at {}", probe.at), theme.secondary));
            }
            spans
        }
    }
}

fn label(text: &str, theme: &Theme) -> Span<'static> {
    Span::styled(format!("{text} "), theme.secondary)
}

fn check_line(check: &str, theme: &Theme) -> Line<'static> {
    Line::from(vec![
        Span::raw(format!("{INDENT}{INDENT}")),
        label("check", theme),
        Span::styled(check.to_string(), theme.inline_code),
    ])
}

fn accept(out: &mut Vec<Line<'static>>, spec: &Spec, theme: &Theme) {
    if spec.accept.is_empty() {
        return;
    }
    section(out, "Accept", theme);
    for entry in &spec.accept {
        out.push(Line::raw(format!("{INDENT}• {}", entry.behaviour)));
        if !entry.check.is_empty() {
            out.push(check_line(&entry.check, theme));
        }
        let mut facts = vec![Span::raw(format!("{INDENT}{INDENT}"))];
        if !entry.kind.is_empty() {
            facts.push(label("kind", theme));
            facts.push(Span::styled(entry.kind.clone(), theme.facet));
            facts.push(Span::raw("  "));
        }
        if !entry.owner.is_empty() {
            facts.push(label("owner", theme));
            let owner = if entry.covered_by.is_empty() {
                entry.owner.clone()
            } else {
                format!("{} by {}", entry.owner, entry.covered_by)
            };
            facts.push(Span::styled(owner, theme.badge));
            facts.push(Span::raw("  "));
        }
        facts.push(label("probe", theme));
        facts.extend(verdict(
            entry.probeable,
            entry.probe.as_ref(),
            "healthy",
            theme,
        ));
        out.push(Line::from(facts));
    }
}

fn premises(out: &mut Vec<Line<'static>>, spec: &Spec, theme: &Theme) {
    if spec.premises.is_empty() {
        return;
    }
    section(out, "Premises", theme);
    for premise in &spec.premises {
        out.push(Line::raw(format!("{INDENT}• {}", premise.claim)));
        if !premise.check.is_empty() {
            out.push(check_line(&premise.check, theme));
        }
        let mut facts = vec![Span::raw(format!("{INDENT}{INDENT}"))];
        if !premise.at.is_empty() {
            facts.push(label("when", theme));
            facts.push(Span::styled(premise.at.clone(), theme.facet));
            facts.push(Span::raw("  "));
        }
        facts.push(label("probe", theme));
        facts.extend(verdict(
            premise.probeable,
            premise.probe.as_ref(),
            "holds",
            theme,
        ));
        out.push(Line::from(facts));
    }
}

fn changes(out: &mut Vec<Line<'static>>, app: &App, task: &Task) {
    let theme = &app.theme;
    let trail = app.index.plan().changes_for(&app.snapshot, &task.task_ref);
    if trail.is_empty() {
        return;
    }
    section(out, "Changes", theme);
    for change in trail {
        let mut head = vec![Span::raw(INDENT)];
        if !change.at.is_empty() {
            head.push(Span::styled(format!("{} ", change.at), theme.secondary));
        }
        head.push(Span::styled(change.field.clone(), theme.badge));
        let old = if change.old.is_empty() {
            "(empty)"
        } else {
            change.old.as_str()
        };
        head.push(Span::raw(format!("  {old} → {}", change.new)));
        out.push(Line::from(head));
        let by = [change.actor.as_str(), change.reason.as_str()]
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(": ");
        if !by.is_empty() {
            out.push(Line::styled(
                format!("{INDENT}{INDENT}{by}"),
                theme.secondary,
            ));
        }
    }
}

fn attempts(out: &mut Vec<Line<'static>>, app: &App, task: &Task) {
    let theme = &app.theme;
    let attempts = app.index.plan().attempts_for(&app.snapshot, task.id);
    if attempts.is_empty() {
        return;
    }
    section(out, "Attempts", theme);
    let detail = |text: String| Line::styled(format!("{INDENT}{INDENT}{text}"), theme.secondary);
    for attempt in attempts {
        let mut head = vec![Span::raw(INDENT)];
        if !attempt.run_id.is_empty() {
            head.push(Span::styled(
                format!("{} ", attempt.run_id),
                theme.secondary,
            ));
        }
        head.push(Span::styled(
            attempt.status.clone(),
            theme.status(&attempt.status),
        ));
        if !attempt.summary.is_empty() {
            head.push(Span::raw(format!("  {}", attempt.summary)));
        }
        out.push(Line::from(head));
        for deviation in &attempt.deviations {
            let mut text = format!("deviation: {}", deviation.what);
            if !deviation.why.is_empty() {
                text.push_str(&format!(" — {}", deviation.why));
            }
            if !deviation.means.is_empty() {
                text.push_str(&format!(" (from: {})", deviation.means));
            }
            out.push(Line::styled(
                format!("{INDENT}{INDENT}{text}"),
                theme.warning,
            ));
        }
        for check in &attempt.checks {
            let style = match check.result.as_str() {
                "pass" => theme.done,
                "fail" => theme.failed,
                _ => theme.secondary,
            };
            out.push(Line::from(vec![
                Span::raw(format!("{INDENT}{INDENT}")),
                label("check", theme),
                Span::styled(check.result.clone(), style),
                Span::raw(" "),
                Span::styled(check.command.clone(), theme.inline_code),
            ]));
        }
        if let Some(handover) = &attempt.handover {
            out.push(detail(format!("handover: {}", handover.action)));
        }
    }
}

/// The records the sections above do not draw: the milestone, binding decisions
/// and contracts, the run note, the commit, findings and backlog rows, and the
/// handovers recorded from this task's attempts.
fn link_section(out: &mut Vec<Line<'static>>, links: &mut Vec<Link>, app: &App, task: &Task) {
    let theme = &app.theme;
    let snapshot = &app.snapshot;
    let plan = app.index.plan();
    let mut rows: Vec<(String, String, Style, Target)> = Vec::new();
    let mut row = |kind: &str, id: &str, text: &str, style: Style, target: Target| {
        let text = if text.is_empty() {
            id.to_string()
        } else {
            format!("{id}  {text}")
        };
        rows.push((kind.to_string(), text, style, target));
    };

    if !task.checkpoint.is_empty() {
        let target = Target::Milestone(task.checkpoint.clone());
        row("milestone", &task.checkpoint, "", theme.checkpoint, target);
    }
    for decision in plan.decisions_for(snapshot, task.id) {
        let target = Target::Decision(decision.id.clone());
        row(
            "decision",
            &decision.id,
            &decision.decision,
            theme.badge,
            target,
        );
    }
    for contract in plan.contracts_for(snapshot, task.id) {
        let target = Target::Contract(contract.id.clone());
        row(
            "contract",
            &contract.id,
            &contract.claim,
            theme.badge,
            target,
        );
    }
    if let Some(note) = app.authored.run_note(task.id) {
        let target = Target::Authored(note.path.clone());
        row("run note", &note.path, "", theme.secondary, target);
    }
    if !task.commit.is_empty() {
        let target = Target::Commit(task.commit.clone());
        row("commit", &task.commit, "", theme.commit, target);
    }
    if let Some(finding) = &task.finding {
        for (verb, ids) in [("fixes", &finding.fixes), ("settles", &finding.settles)] {
            for id in ids {
                let target = Target::Finding {
                    store: finding.store.clone(),
                    id: id.clone(),
                };
                row(verb, id, &finding.store, theme.item_id, target);
            }
        }
    }
    for (verb, ids) in [("closes", &task.closes), ("refers to", &task.refs)] {
        for id in ids {
            row(verb, id, "", theme.item_id, Target::Backlog(id.clone()));
        }
    }
    for handover in plan.handovers_for(snapshot, task.id) {
        let state = if handover.done { "done" } else { "open" };
        let text = format!("{state}  {}", handover.action);
        let target = Target::Handover(handover.id.clone());
        row("handover", &handover.id, &text, theme.badge, target);
    }

    if rows.is_empty() {
        return;
    }
    section(out, "Links", theme);
    for (kind, text, style, target) in rows {
        out.push(Line::from(vec![
            Span::raw(INDENT),
            label(&kind, theme),
            Span::styled(text.clone(), style),
        ]));
        push(links, format!("{kind} {text}"), target);
    }
}

#[cfg(test)]
mod tests {
    use std::time::SystemTime;

    use super::*;
    use crate::authored::AuthoredList;
    use crate::config::Config;
    use crate::model::fixture;
    use crate::view::details::content;
    use crate::view::doc::Doc;

    fn app_on(id: u32) -> App {
        let mut app = App::new(fixture(), &Config::default());
        app.selected = Some(id);
        app
    }

    fn plain(doc: &Doc) -> Vec<String> {
        doc.text
            .lines
            .iter()
            .map(|line| line.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect()
    }

    fn after(lines: &[String], label: &str) -> Vec<String> {
        let at = lines.iter().position(|l| l == label).expect(label);
        lines[at + 1..]
            .iter()
            .take_while(|l| !l.is_empty())
            .cloned()
            .collect()
    }

    #[test]
    fn a_v2_task_shows_every_spec_section_in_order() {
        let lines = plain(&content(&app_on(6), SystemTime::now()));
        let order = [
            "Action",
            "Outcome",
            "Constraints",
            "Means",
            "Executor",
            "Design",
            "Accept",
            "Premises",
            "Changes",
            "Links",
        ];
        let at: Vec<usize> = order
            .iter()
            .map(|label| lines.iter().position(|l| l == label).expect(label))
            .collect();
        assert!(at.windows(2).all(|w| w[0] < w[1]), "{lines:?}");

        assert_eq!(
            after(&lines, "Outcome"),
            ["  Every key event maps to one action."]
        );
        assert_eq!(
            after(&lines, "Constraints"),
            ["  • Release events are dropped."]
        );
        assert_eq!(
            after(&lines, "Means"),
            [
                "  suggested Map through one match in keys.rs",
                "    The config names the keys."
            ]
        );
        assert_eq!(after(&lines, "Executor"), ["  deep"]);
        assert_eq!(after(&lines, "Design"), ["  design/keys.md#bindings"]);
        let accept = after(&lines, "Accept");
        assert_eq!(accept[0], "  • the key map exists");
        assert_eq!(accept[1], "    check grep -q \"fn map\" src/keys.rs");
        assert_eq!(
            accept[2],
            "    kind forward  owner worker  probe healthy at c3d4e5f"
        );
        assert!(
            accept[5].ends_with("probe vacuous at c3d4e5f"),
            "{accept:?}"
        );
        let changes = after(&lines, "Changes");
        assert_eq!(
            changes[0],
            "  2026-09-28T09:15:00Z detail  (empty) → Read the key names from the config."
        );
        assert_eq!(
            changes[1],
            "    plan-update: the config task settled the names"
        );
        let links = after(&lines, "Links");
        assert_eq!(
            links,
            [
                "  milestone B",
                "  decision D1  Read the key names from the config.",
                "  contract C1  The config exposes the key names the key map reads.",
                "  refers to B-1a2b3c4d",
            ]
        );
    }

    #[test]
    fn precedent_attempts_and_closed_rows_are_drawn() {
        let lines = plain(&content(&app_on(5), SystemTime::now()));
        assert_eq!(after(&lines, "Precedent"), ["  src/render.rs"]);
        assert!(
            after(&lines, "Links")
                .contains(&"  fixes F3  .claude/flows/demo-flow/review-v2.toml".to_string()),
            "{lines:?}"
        );

        let lines = plain(&content(&app_on(8), SystemTime::now()));
        assert_eq!(
            after(&lines, "Attempts"),
            ["  wf_3 parked  the required hand-rolled parser conflicts with the clap constraint"]
        );
        assert_eq!(
            after(&lines, "Means"),
            [
                "  required Parse the arguments by hand",
                "    Start-up cost matters."
            ]
        );
        assert!(after(&lines, "Links").contains(&"  closes B-5e6f7081".to_string()));
        assert_eq!(after(&lines, "Files"), ["  src/main.rs  — wiring only"]);
    }

    #[test]
    fn an_attempts_deviations_checks_and_handover_are_drawn() {
        let mut snap = fixture();
        let spec = snap.tasks[5].spec.clone();
        snap.tasks[2].spec = spec;
        let mut app = App::new(snap, &Config::default());
        app.selected = Some(3);
        let lines = plain(&content(&app, SystemTime::now()));
        assert_eq!(
            after(&lines, "Attempts"),
            [
                "  wf_2 done  Config parser with defaults",
                "    deviation: Returned warnings beside the config — An unknown key must reach the header (from: Return the config alone)",
                "    check pass cargo clippy",
                "    check not-run cargo test config::",
                "    handover: Restart the running app.",
            ]
        );
        assert!(
            after(&lines, "Links")
                .contains(&"  handover H1  open  Restart the running app.".to_string())
        );
    }

    #[test]
    fn an_unprobed_and_a_not_probeable_check_read_differently() {
        let lines = plain(&content(&app_on(6), SystemTime::now()));
        let premises = after(&lines, "Premises");
        assert_eq!(
            premises,
            [
                "  • the config parser exists",
                "    check test -f src/config.rs",
                "    when base  probe unprobed",
                "  • the renderer draws rows",
                "    check grep -q \"fn render\" src/render.rs",
                "    when after:4  probe not probed",
            ]
        );
        let lines = plain(&content(&app_on(5), SystemTime::now()));
        assert_eq!(
            after(&lines, "Accept")[2],
            "    kind forward  owner covered by cargo test  probe not probed"
        );
    }

    #[test]
    fn the_links_follow_the_section_order() {
        let mut snap = fixture();
        snap.tasks[5].commit = "abc1234".to_string();
        snap.tasks[5].spec.as_mut().expect("spec").precedent = vec!["src/config.rs".to_string()];
        let mut app = App::new(snap, &Config::default());
        app.selected = Some(6);
        let list: AuthoredList = serde_json::from_str(
            r#"{"files": [{"path": "notes/run/task-6.md", "kind": "run-note", "task": 6}]}"#,
        )
        .expect("listing parses");
        app.apply_authored(list, 1);

        let doc = content(&app, SystemTime::now());
        let spec_links: Vec<Target> = doc
            .links
            .iter()
            .map(|link| link.target.clone())
            .filter(|target| !matches!(target, Target::Task(_)))
            .collect();
        assert_eq!(
            spec_links,
            [
                Target::Authored("design/keys.md".to_string()),
                Target::Source {
                    path: "src/config.rs".to_string(),
                    line: None,
                    read_at: String::new(),
                    dirty: false,
                },
                Target::Milestone("B".to_string()),
                Target::Decision("D1".to_string()),
                Target::Contract("C1".to_string()),
                Target::Authored("notes/run/task-6.md".to_string()),
                Target::Commit("abc1234".to_string()),
                Target::Backlog("B-1a2b3c4d".to_string()),
            ]
        );
        let first_spec = doc
            .links
            .iter()
            .position(|link| !matches!(link.target, Target::Task(_)))
            .expect("spec links");
        assert!(
            doc.links[first_spec..]
                .iter()
                .all(|link| !matches!(link.target, Target::Task(_))),
            "the task links of the earlier sections come first"
        );
        assert_eq!(doc.links[first_spec].label, "design/keys.md#bindings");
    }

    #[test]
    fn a_task_without_a_spec_gets_nothing() {
        let app = app_on(7);
        let task = app.index.task(&app.snapshot, 7).expect("task 7");
        let mut out = Vec::new();
        let mut links = Vec::new();
        sections(&app, task, &mut out, &mut links);
        assert!(out.is_empty() && links.is_empty());

        let mut snap = fixture();
        snap.tasks[7].spec = None;
        let mut app = App::new(snap, &Config::default());
        app.selected = Some(8);
        let doc = content(&app, SystemTime::now());
        let lines = plain(&doc);
        assert_eq!(
            after(&lines, "Files"),
            ["  src/main.rs"],
            "file notes are drawn only for a task with a spec"
        );
        for label in ["Outcome", "Means", "Accept", "Attempts", "Links"] {
            assert!(!lines.iter().any(|l| l == label), "{label}: {lines:?}");
        }
        assert!(
            doc.links
                .iter()
                .all(|link| matches!(link.target, Target::Task(_)))
        );
    }
}
