//! The scrollable details panel for the selected task.
//!
//! Scroll state belongs to the caller: [`render`] takes the offset to draw
//! at and returns the content height, which the caller clamps its offset
//! against on the next key press.

use std::time::{SystemTime, UNIX_EPOCH};

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Paragraph, Wrap};

use crate::app::App;
use crate::model::{Agent, AgentKind, AgentStatus, RecordEntry, TaskStatus};
use crate::theme::Theme;
use crate::view::activity::parse_utc;
use crate::view::markdown;

const INDENT: &str = "  ";

/// Draws the details of `app.selected` into `area`, scrolled down `scroll`
/// rows, and returns the wrapped content height in rows. An offset past the
/// end is drawn as the last full page.
pub(crate) fn render(frame: &mut Frame, area: Rect, app: &App, scroll: u16) -> u16 {
    let text = content(app, SystemTime::now());
    let title = match app
        .selected
        .and_then(|id| app.index.task(&app.snapshot, id))
    {
        Some(task) => format!(" #{} {} ", task.id, task.title),
        None => " Details ".to_string(),
    };
    let block = Block::bordered().title(Line::from(title));
    let inner = block.inner(area);
    let height = wrapped_height(&text, inner.width);
    let scroll = scroll.min(height.saturating_sub(inner.height));

    let mut block = block;
    if scroll > 0 || scroll + inner.height < height {
        block =
            block.title_bottom(Line::from(format!(" {}/{} ", scroll + 1, height)).right_aligned());
    }
    let paragraph = Paragraph::new(text)
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));
    frame.render_widget(paragraph, area);
    height
}

/// The panel's lines for the selected task, unwrapped. `now` dates the open
/// segment of a running agent.
pub(crate) fn content(app: &App, now: SystemTime) -> Text<'static> {
    let theme = &app.theme;
    let snapshot = &app.snapshot;
    let index = &app.index;
    let Some(task) = app.selected.and_then(|id| index.task(snapshot, id)) else {
        return Text::from(Line::styled("No task selected.", theme.pending));
    };

    let mut out: Vec<Line<'static>> = Vec::new();
    out.push(Line::styled(
        format!("#{} {}", task.id, task.title),
        theme.badge,
    ));

    let mut facts = vec![Span::styled(
        format!("{} {}", glyph(task.status), task.status.as_str()),
        theme.status(task.status.as_str()),
    )];
    let mut fact = |label: &str, value: String| {
        if !value.is_empty() {
            facts.push(Span::raw(format!("  {label} ")));
            facts.push(Span::styled(value, theme.badge));
        }
    };
    fact("effort", task.effort.clone());
    let checkpoint = match index.checkpoint(snapshot, task.id) {
        Some(group) => match &group.verification {
            Some(v) if !v.outcome.is_empty() => format!("{} ({})", group.id, v.outcome),
            _ => group.id.clone(),
        },
        None => task.checkpoint.clone(),
    };
    fact("checkpoint", checkpoint);
    fact("phase", task.phase.clone());
    out.push(Line::from(facts));

    if !task.files.is_empty() {
        section(&mut out, "Files", theme);
        out.extend(task.files.iter().map(|f| Line::raw(format!("{INDENT}{f}"))));
    }

    let mut coupled: Vec<u32> = task
        .coupling
        .iter()
        .chain(index.coupled(task.id))
        .copied()
        .collect();
    coupled.sort_unstable();
    coupled.dedup();
    for (label, ids) in [
        ("Needs", task.needs.as_slice()),
        ("Coupling", coupled.as_slice()),
        ("Dependents", index.dependents(task.id)),
    ] {
        if ids.is_empty() {
            continue;
        }
        section(&mut out, label, theme);
        for id in ids {
            out.push(match index.task(snapshot, *id) {
                Some(peer) => Line::from(vec![
                    Span::raw(INDENT),
                    Span::styled(
                        format!("{} {}", glyph(peer.status), peer.id),
                        theme.status(peer.status.as_str()),
                    ),
                    Span::raw(format!(" {}", peer.title)),
                ]),
                None => Line::styled(format!("{INDENT}? {id} (not in the store)"), theme.warning),
            });
        }
    }

    for (label, body) in [
        ("Action", &task.action),
        ("Detail", &task.detail),
        ("Acceptance", &task.acceptance),
        ("Dependency note", &task.deps_note),
    ] {
        if body.trim().is_empty() {
            continue;
        }
        section(&mut out, label, theme);
        out.extend(indented(markdown::to_text(body, theme)));
    }

    let record = index.record_for(snapshot, task.id);
    if !record.is_empty() {
        section(&mut out, "Record", theme);
        for entry in record {
            record_lines(&mut out, entry, theme);
        }
    }

    let agents = index.agents_for(snapshot, task.id);
    if !agents.is_empty() {
        section(&mut out, "Agents", theme);
        for agent in agents {
            agent_lines(&mut out, agent, theme, now);
        }
    }

    Text::from(out)
}

fn section(out: &mut Vec<Line<'static>>, label: &str, theme: &Theme) {
    out.push(Line::default());
    out.push(Line::styled(
        label.to_string(),
        theme.badge.add_modifier(Modifier::UNDERLINED),
    ));
}

fn indented(text: Text<'static>) -> impl Iterator<Item = Line<'static>> {
    text.lines.into_iter().map(|mut line| {
        if !line.spans.is_empty() {
            line.spans.insert(0, Span::raw(INDENT));
        }
        line
    })
}

fn record_lines(out: &mut Vec<Line<'static>>, entry: &RecordEntry, theme: &Theme) {
    let type_style = match entry.entry_type.as_str() {
        "deviation" | "deferral" => theme.warning,
        "verification" if entry.outcome == "pass" => theme.done,
        "verification" if !entry.outcome.is_empty() => theme.failed,
        _ => theme.badge,
    };
    out.push(Line::from(vec![
        Span::raw(INDENT),
        Span::styled(entry.entry_type.clone(), type_style),
        Span::raw(format!(" {}  {}", entry.date, entry.summary)),
    ]));
    let detail = Style::new().add_modifier(Modifier::DIM);
    for (label, value) in [
        ("intended", &entry.original_intent),
        ("because", &entry.rationale),
    ] {
        if !value.is_empty() {
            out.push(Line::styled(
                format!("{INDENT}{INDENT}{label}: {value}"),
                detail,
            ));
        }
    }
}

fn agent_lines(out: &mut Vec<Line<'static>>, agent: &Agent, theme: &Theme, now: SystemTime) {
    let kind = match agent.kind {
        AgentKind::Subagent => "subagent",
        AgentKind::Teammate => "teammate",
        AgentKind::Unknown => "unknown",
    };
    let (status, status_style) = match agent.status {
        AgentStatus::Running => ("running", theme.in_progress),
        AgentStatus::Idle => ("idle", theme.pending),
        AgentStatus::Stopped => ("stopped", theme.done),
        AgentStatus::Unknown => ("unknown", theme.warning),
    };
    let mut head = vec![
        Span::raw(INDENT),
        Span::styled(format!(" {} ", agent.agent_type), theme.agent_chip),
    ];
    if !agent.name.is_empty() {
        head.push(Span::raw(format!(" {}", agent.name)));
    }
    head.push(Span::raw(format!(" {kind} ")));
    head.push(Span::styled(status, status_style));
    if agent.context_tokens > 0 {
        head.push(Span::raw(format!(
            "  {} tokens",
            tokens(agent.context_tokens)
        )));
    }
    out.push(Line::from(head));

    let now_s = now.duration_since(UNIX_EPOCH).ok().map(|d| d.as_secs());
    for segment in &agent.segments {
        let ids = segment
            .task_ids
            .iter()
            .map(|id| format!("#{id}"))
            .collect::<Vec<_>>()
            .join(" ");
        let end = if segment.ended_at.is_empty() {
            now_s
        } else {
            parse_utc(&segment.ended_at)
        };
        let span = match (parse_utc(&segment.started_at), end) {
            (Some(start), Some(end)) if end >= start => duration(end - start),
            _ => "?".to_string(),
        };
        let range = if segment.ended_at.is_empty() {
            format!("{} → now", segment.started_at)
        } else {
            format!("{} → {}", segment.started_at, segment.ended_at)
        };
        out.push(Line::raw(format!("{INDENT}{INDENT}{ids}  {range}  {span}")));
    }
    if !agent.summary.is_empty() {
        out.push(Line::styled(
            format!("{INDENT}{INDENT}{}", agent.summary),
            Style::new().add_modifier(Modifier::DIM),
        ));
    }
}

fn glyph(status: TaskStatus) -> &'static str {
    match status {
        TaskStatus::Pending => "○",
        TaskStatus::InProgress => "●",
        TaskStatus::Done => "✓",
        TaskStatus::Failed => "✗",
        TaskStatus::Deferred => "–",
        TaskStatus::Unknown => "?",
    }
}

fn tokens(count: u64) -> String {
    if count < 1000 {
        count.to_string()
    } else {
        // One decimal, rounded, without going through floats.
        let tenths = (count + 50) / 100;
        format!("{}.{}k", tenths / 10, tenths % 10)
    }
}

fn duration(seconds: u64) -> String {
    let (d, h, m, s) = (
        seconds / 86_400,
        seconds / 3600 % 24,
        seconds / 60 % 60,
        seconds % 60,
    );
    if d > 0 {
        format!("{d}d{h:02}h")
    } else if h > 0 {
        format!("{h}h{m:02}m")
    } else if m > 0 {
        format!("{m}m{s:02}s")
    } else {
        format!("{s}s")
    }
}

/// Rows `text` occupies when word-wrapped to `width` columns. Greedy
/// wrapping on spaces approximates `Paragraph::wrap`, whose own count is
/// behind an unstable ratatui feature.
fn wrapped_height(text: &Text<'_>, width: u16) -> u16 {
    let width = usize::from(width.max(1));
    let rows: usize = text
        .lines
        .iter()
        .map(|line| {
            let content: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
            let mut rows = 1;
            let mut used = 0;
            for (i, word) in content.split(' ').enumerate() {
                let w = Span::raw(word).width();
                let sep = usize::from(i > 0);
                if used + sep + w <= width {
                    used += sep + w;
                    continue;
                }
                if used > 0 {
                    rows += 1;
                }
                used = w;
                while used > width {
                    rows += 1;
                    used -= width;
                }
            }
            rows
        })
        .sum();
    u16::try_from(rows).unwrap_or(u16::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::model::fixture;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn app_on(id: u32) -> App {
        let mut app = App::new(fixture(), &Config::default());
        app.selected = Some(id);
        app
    }

    fn plain(text: &Text<'_>) -> Vec<String> {
        text.lines
            .iter()
            .map(|line| line.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect()
    }

    fn at(stamp: &str) -> SystemTime {
        let secs = parse_utc(stamp).expect("valid stamp");
        UNIX_EPOCH + std::time::Duration::from_secs(secs)
    }

    #[test]
    fn a_task_shows_its_facts_links_and_body() {
        let lines = plain(&content(&app_on(5), at("2026-09-28T12:00:00Z")));
        assert_eq!(lines[0], "#5 Style the rows");
        assert_eq!(
            lines[1],
            "○ pending  effort S  checkpoint B  phase Phase B — views"
        );
        let after = |label: &str| {
            let at = lines.iter().position(|l| l == label).expect(label);
            lines[at + 1..]
                .iter()
                .take_while(|l| !l.is_empty())
                .cloned()
                .collect::<Vec<_>>()
        };
        assert_eq!(after("Files"), ["  src/render.rs", "  src/theme.rs"]);
        assert_eq!(
            after("Needs"),
            ["  ✓ 1 Scaffold the store", "  ✓ 2 Define the schema"]
        );
        assert_eq!(after("Coupling"), ["  ○ 8 Wire the entry point"]);
        assert_eq!(after("Dependents"), ["  ○ 7 Assemble the app"]);
        assert_eq!(after("Action"), ["  Apply the theme to each status."]);
        assert!(
            !lines
                .iter()
                .any(|l| l == "Detail" || l == "Record" || l == "Agents")
        );
    }

    #[test]
    fn record_and_agents_carry_deviation_and_segment_detail() {
        let lines = plain(&content(&app_on(3), at("2026-09-28T12:00:00Z")));
        assert!(lines[1].contains("checkpoint A (pass)"), "{}", lines[1]);
        let coupling = lines
            .iter()
            .position(|l| l == "Coupling")
            .expect("Coupling");
        assert_eq!(lines[coupling + 1], "  ○ 6 Bind the keys");
        assert!(lines.contains(&"    intended: parse(&str) -> Config".to_string()));
        assert!(
            lines
                .iter()
                .any(|l| l.starts_with("    because: An unknown key"))
        );
        assert!(
            lines
                .iter()
                .any(|l| l.starts_with("  deviation 2026-09-28  parse returns"))
        );
        assert!(
            lines.contains(&"   implement-lite  worker-1 teammate idle  61.9k tokens".to_string())
        );
        assert!(
            lines.contains(
                &"    #2  2026-09-27T09:12:04Z → 2026-09-27T09:30:51Z  18m47s".to_string()
            )
        );
        assert!(
            lines.contains(
                &"    #3  2026-09-28T10:02:19Z → 2026-09-28T10:41:37Z  39m18s".to_string()
            )
        );
        assert!(
            lines
                .iter()
                .any(|l| l.contains("Applied the schema and config tasks"))
        );
    }

    #[test]
    fn an_open_segment_runs_to_now() {
        let lines = plain(&content(&app_on(4), at("2026-09-28T12:04:22Z")));
        assert!(lines.contains(&"    #4  2026-09-28T11:04:22Z → now  1h00m".to_string()));
    }

    #[test]
    fn markdown_bodies_are_rendered_and_indented() {
        let lines = plain(&content(&app_on(1), at("2026-09-28T12:00:00Z")));
        let at = lines.iter().position(|l| l == "Detail").expect("Detail");
        assert_eq!(
            lines[at + 1..at + 3],
            [
                "  • Read with read_doc.",
                "  • An absent file is an empty store."
            ]
        );
    }

    #[test]
    fn no_selection_shows_a_placeholder() {
        let mut app = app_on(1);
        app.selected = None;
        assert_eq!(
            plain(&content(&app, SystemTime::now())),
            ["No task selected."]
        );
        app.selected = Some(99);
        assert_eq!(
            plain(&content(&app, SystemTime::now())),
            ["No task selected."]
        );
    }

    #[test]
    fn render_scrolls_and_clamps_past_the_end() {
        let app = app_on(3);
        let draw = |scroll: u16| {
            let mut terminal = Terminal::new(TestBackend::new(60, 8)).expect("terminal");
            let mut height = 0;
            terminal
                .draw(|frame| height = render(frame, frame.area(), &app, scroll))
                .expect("draw");
            let buffer = terminal.backend().buffer().clone();
            let row = |y: u16| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            };
            (height, row(0), row(1), row(7))
        };

        let (height, title, first, bottom) = draw(0);
        assert!(height > 6, "content overflows the 6 inner rows");
        assert!(title.contains("#3 Load the config"));
        assert!(first.contains("#3 Load the config"));
        assert!(bottom.contains(&format!(" 1/{height} ")));

        let (_, _, first, _) = draw(1);
        assert!(
            first.contains("pending") || first.contains("✓ done"),
            "{first}"
        );

        let (_, _, clamped, bottom) = draw(u16::MAX);
        let (_, _, last_page, _) = draw(height - 6);
        assert_eq!(clamped, last_page);
        assert!(bottom.contains(&format!(" {}/{height} ", height - 5)));
    }

    #[test]
    fn helpers_format_times_and_counts() {
        assert_eq!(duration(42), "42s");
        assert_eq!(duration(3 * 3600 + 5 * 60), "3h05m");
        assert_eq!(duration(2 * 86_400 + 3600), "2d01h");
        assert_eq!(tokens(999), "999");
        assert_eq!(tokens(61_872), "61.9k");
    }

    #[test]
    fn wrapped_height_counts_wrapped_rows() {
        let text = Text::from(vec![
            Line::raw("aaaa bbbb cccc"),
            Line::raw(""),
            Line::raw("x"),
        ]);
        assert_eq!(wrapped_height(&text, 20), 3);
        assert_eq!(wrapped_height(&text, 9), 4);
        assert_eq!(wrapped_height(&text, 4), 5);
    }
}
