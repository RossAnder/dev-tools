//! The scrollable details panel for the selected task or the cursor's ledger item.
//!
//! Scroll state belongs to the caller: [`render`] takes the offset to draw
//! at and returns the content height, which the caller clamps its offset
//! against on the next key press. Lines are wrapped here rather than by the
//! paragraph, so a wrapped list item or indented line continues under its text
//! instead of at the panel's edge, and the height is exact.

use std::time::{SystemTime, UNIX_EPOCH};

use ratatui::Frame;
use ratatui::layout::{Margin, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{
    Block, Padding, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
};

use serde_json::Value;

use crate::app::App;
use crate::hook::parse_utc;
use crate::ledger::{Anchor, InputRow, ItemRow, StatusClass};
use crate::model::{Agent, AgentKind, AgentStatus, RecordEntry};
use crate::surface::{ItemsState, Surface};
use crate::theme::Theme;
use crate::view::{self, markdown};

const INDENT: &str = "  ";

/// Draws the details of `app.selected`, or of the cursor row on an item
/// surface, into `area` scrolled down `scroll` rows, and returns the wrapped
/// content height in rows. The border names only the id; the first content
/// row carries the whole title.
pub(crate) fn render(frame: &mut Frame, area: Rect, app: &App, scroll: u16) -> u16 {
    let now = SystemTime::now();
    let (id, text) = match app.surface {
        Surface::Tasks => (
            app.selected
                .and_then(|id| app.index.task(&app.snapshot, id))
                .map(|task| format!("#{}", task.id)),
            content(app, now),
        ),
        _ => match app.current_items().and_then(ItemsState::cursor_row) {
            Some(row) => {
                let agents = app.index.agents_for_item(&app.snapshot, &row.id);
                let inputs = app
                    .surface
                    .ledger_kind()
                    .map(|kind| app.inbox.pending_for(kind, &row.id))
                    .unwrap_or_default();
                (
                    Some(row.id.clone()),
                    item_content(row, &agents, &inputs, &app.theme, now),
                )
            }
            None => (
                None,
                Text::from(Line::styled("No item selected.", app.theme.pending)),
            ),
        },
    };
    let title = id.map_or_else(|| " details ".to_string(), |id| format!(" {id} "));
    render_text(frame, area, &app.theme, title, text, scroll)
}

/// Draws `text` wrapped inside a bordered panel titled `title`. An offset past
/// the end is drawn as the last full page; overflowing content gets a scrollbar
/// on the right border and a `row/total` count on the bottom one.
fn render_text(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    title: String,
    text: Text<'static>,
    scroll: u16,
) -> u16 {
    let block = Block::bordered()
        .border_style(theme.border)
        .title(Line::styled(title, theme.border_title))
        .padding(Padding::horizontal(1));
    let inner = block.inner(area);
    let lines = wrap(text, inner.width);
    let height = u16::try_from(lines.len()).unwrap_or(u16::MAX);
    let scroll = scroll.min(height.saturating_sub(inner.height));

    let overflows = scroll > 0 || scroll + inner.height < height;
    let mut block = block;
    if overflows {
        block = block.title_bottom(
            Line::styled(format!(" {}/{} ", scroll + 1, height), theme.border_title)
                .right_aligned(),
        );
    }
    let paragraph = Paragraph::new(Text::from(lines))
        .block(block)
        .scroll((scroll, 0));
    frame.render_widget(paragraph, area);
    if overflows {
        let max_scroll = height.saturating_sub(inner.height);
        let mut state = ScrollbarState::new(usize::from(max_scroll) + 1)
            .viewport_content_length(usize::from(inner.height))
            .position(usize::from(scroll));
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight),
            area.inner(Margin::new(0, 1)),
            &mut state,
        );
    }
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
        theme.slug,
    ));

    let mut facts = vec![Span::styled(
        format!("{} {}", task.status.glyph(), task.status.as_str()),
        theme.status(task.status.as_str()),
    )];
    let mut fact = |label: &str, value: String, style: Style| {
        if !value.is_empty() {
            facts.push(Span::styled(format!("  {label} "), theme.secondary));
            facts.push(Span::styled(value, style));
        }
    };
    let effort = super::notice_style(app, task).unwrap_or(theme.badge);
    fact("effort", task.effort.clone(), effort);
    let checkpoint = match index.checkpoint(snapshot, task.id) {
        Some(group) => match &group.verification {
            Some(v) if !v.outcome.is_empty() => format!("{} ({})", group.id, v.outcome),
            _ => group.id.clone(),
        },
        None => task.checkpoint.clone(),
    };
    fact("checkpoint", checkpoint, theme.checkpoint);
    fact("phase", task.phase.clone(), theme.badge);
    out.push(Line::from(facts));

    if !task.files.is_empty() {
        section(&mut out, "Files", theme);
        out.extend(
            task.files
                .iter()
                .map(|f| Line::styled(format!("{INDENT}{f}"), theme.secondary)),
        );
    }

    let mut coupled: Vec<u32> = task
        .coupling
        .iter()
        .chain(index.coupled(task.id))
        .copied()
        .collect();
    coupled.sort_unstable();
    coupled.dedup();
    for (label, ids, needs) in [
        ("Needs", task.needs.as_slice(), true),
        ("Coupling", coupled.as_slice(), false),
        ("Dependents", index.dependents(task.id), false),
        ("Shares files with", index.overlaps(task.id), false),
    ] {
        if ids.is_empty() {
            continue;
        }
        section(&mut out, label, theme);
        for id in ids {
            out.push(match index.task(snapshot, *id) {
                Some(peer) => {
                    let mut spans = vec![
                        Span::raw(INDENT),
                        Span::styled(
                            format!("{} {}", peer.status.glyph(), peer.id),
                            theme.status(peer.status.as_str()),
                        ),
                        Span::raw(format!(" {}", peer.title)),
                    ];
                    if needs && app.implied.contains(&(peer.id, task.id)) {
                        spans.push(Span::styled("  implied", theme.secondary));
                    }
                    Line::from(spans)
                }
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

/// The panel's lines for one ledger row, unwrapped. `agents` are those working
/// the row; `inputs` are the pending input records naming it; `now` dates their
/// open segments.
pub(crate) fn item_content(
    row: &ItemRow,
    agents: &[&Agent],
    inputs: &[&InputRow],
    theme: &Theme,
    now: SystemTime,
) -> Text<'static> {
    let mut out: Vec<Line<'static>> = Vec::new();
    out.push(Line::styled(
        format!("{} {}", row.id, row.summary),
        theme.slug,
    ));

    let status = if row.status.is_empty() {
        "no status"
    } else {
        row.status.as_str()
    };
    out.push(Line::styled(
        format!("{} {status}", view::items::glyph(row.class)),
        class_style(row.class, theme),
    ));
    for (field, value) in &row.companions {
        out.push(Line::from(vec![
            Span::styled(
                format!("{INDENT}{}: ", field.replace('_', " ")),
                theme.secondary,
            ),
            Span::raw(value.clone()),
        ]));
    }

    let mut facts: Vec<Span<'static>> = Vec::new();
    let mut fact = |label: &str, value: &str, style: Style| {
        if !value.is_empty() {
            if !facts.is_empty() {
                facts.push(Span::raw("  "));
            }
            facts.push(Span::styled(format!("{label} "), theme.secondary));
            facts.push(Span::styled(value.to_string(), style));
        }
    };
    fact("kind", &row.kind, theme.facet);
    fact("severity", &row.severity, theme.severity(&row.severity));
    fact("category", &row.category, theme.facet);
    fact("effort", &row.effort, theme.effort);
    fact("tags", &row.tags.join(", "), theme.facet);
    if !facts.is_empty() {
        out.push(Line::from(facts));
    }

    let anchor = match &row.anchor {
        Anchor::Code { .. } => Some("at"),
        Anchor::Section(_) => Some("section"),
        Anchor::Area(_) => Some("area"),
        Anchor::None => None,
    };
    if let Some(label) = anchor {
        out.push(Line::from(vec![
            Span::styled(format!("{label} "), theme.secondary),
            Span::styled(row.anchor.to_string(), theme.facet),
        ]));
    }

    if !row.description.trim().is_empty() {
        section(&mut out, "Description", theme);
        out.extend(indented(markdown::to_text(&row.description, theme)));
    }
    for (label, list) in [("Evidence", &row.evidence), ("Instances", &row.instances)] {
        if list.is_empty() {
            continue;
        }
        section(&mut out, label, theme);
        out.extend(
            list.iter()
                .map(|entry| Line::styled(format!("{INDENT}{entry}"), theme.secondary)),
        );
    }

    let flagged = if row.raw.get("first_flagged").is_some() {
        "flagged"
    } else {
        "created"
    };
    let rounds = row
        .raw
        .get("rounds")
        .and_then(Value::as_u64)
        .map(|n| n.to_string())
        .unwrap_or_default();
    let dates: Vec<String> = [
        (flagged, row.created.clone()),
        ("closed", row.closed.clone()),
        ("rounds", rounds),
    ]
    .into_iter()
    .filter(|(_, value)| !value.is_empty())
    .map(|(label, value)| format!("{label} {value}"))
    .collect();
    if !dates.is_empty() {
        out.push(Line::default());
        out.push(Line::styled(dates.join("  "), theme.secondary));
    }

    if !inputs.is_empty() {
        section(&mut out, "Inputs", theme);
        for input in inputs {
            let style = if input.status == "new" {
                theme.input_new
            } else {
                theme.input_acknowledged
            };
            out.push(Line::from(vec![
                Span::raw(INDENT),
                Span::styled(format!("{} {}", input.kind, input.status), style),
                Span::raw(format!("  {}", input.text)),
            ]));
        }
    }

    if !agents.is_empty() {
        section(&mut out, "Agents", theme);
        for agent in agents {
            agent_lines(&mut out, agent, theme, now);
        }
    }

    Text::from(out)
}

fn class_style(class: StatusClass, theme: &Theme) -> Style {
    theme.item_status(view::items::class_name(class))
}

fn section(out: &mut Vec<Line<'static>>, label: &str, theme: &Theme) {
    out.push(Line::default());
    out.push(Line::styled(label.to_string(), theme.section));
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
    let detail = theme.secondary;
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
        super::chip(&agent.agent_type, theme.agent_chip),
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
            .chain(segment.item_ids.iter().cloned())
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
            theme.secondary,
        ));
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

/// `text` word-wrapped to `width` cells, one output line per drawn row. A wrapped
/// line continues at its [`hanging_indent`]; a word wider than a row is split.
pub(crate) fn wrap(text: Text<'static>, width: u16) -> Vec<Line<'static>> {
    let width = usize::from(width.max(1));
    text.lines
        .into_iter()
        .flat_map(|line| wrap_line(line, width))
        .collect()
}

fn wrap_line(line: Line<'static>, width: usize) -> Vec<Line<'static>> {
    if line.width() <= width {
        return vec![line];
    }
    let plain: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    let indent = hanging_indent(&plain).min(width / 2);
    let mut rows = Vec::new();
    let mut row: Vec<Span<'static>> = Vec::new();
    let mut used = 0;
    let mut has_word = false;
    let mut pending: Vec<Span<'static>> = Vec::new();
    for (piece, style) in pieces(&line) {
        if piece.starts_with(' ') {
            pending.push(Span::styled(piece, style));
            continue;
        }
        let w = Span::raw(piece.as_str()).width();
        let gap: usize = pending.iter().map(Span::width).sum();
        if has_word && used + gap + w > width {
            rows.push(Line::from(std::mem::take(&mut row)).style(line.style));
            row.push(Span::raw(" ".repeat(indent)));
            used = indent;
            pending.clear();
        }
        used += pending.iter().map(Span::width).sum::<usize>();
        row.append(&mut pending);
        // Only a word that starts a row can overflow it; it is split across rows.
        let mut rest = piece.as_str();
        while !rest.is_empty() {
            let room = width.saturating_sub(used).max(1);
            let (head, tail) = split_at_width(rest, room);
            used += Span::raw(head).width();
            row.push(Span::styled(head.to_string(), style));
            rest = tail;
            if !rest.is_empty() {
                rows.push(Line::from(std::mem::take(&mut row)).style(line.style));
                row.push(Span::raw(" ".repeat(indent)));
                used = indent;
            }
        }
        has_word = true;
    }
    rows.push(Line::from(row).style(line.style));
    rows
}

/// The line's text as alternating runs of spaces and non-spaces, each with its style.
fn pieces(line: &Line<'static>) -> Vec<(String, Style)> {
    let mut out: Vec<(String, Style)> = Vec::new();
    for span in &line.spans {
        let mut run = String::new();
        let mut spaces = None;
        for c in span.content.chars() {
            let is_space = c == ' ';
            if spaces.is_some_and(|s| s != is_space) {
                out.push((std::mem::take(&mut run), span.style));
            }
            spaces = Some(is_space);
            run.push(c);
        }
        if !run.is_empty() {
            out.push((run, span.style));
        }
    }
    out
}

/// The longest prefix of `s` at most `room` cells wide, at least one character.
fn split_at_width(s: &str, room: usize) -> (&str, &str) {
    let mut used = 0;
    for (at, c) in s.char_indices() {
        let w = Span::raw(c.encode_utf8(&mut [0; 4]).to_string()).width();
        if used + w > room && at > 0 {
            return s.split_at(at);
        }
        used += w;
    }
    (s, "")
}

/// Cells a continuation row is indented by: the line's leading spaces plus any list
/// marker (`• `, `- `, `* `, `1. `) after them, so a wrapped item lines up under its text.
fn hanging_indent(text: &str) -> usize {
    let rest = text.trim_start_matches(' ');
    let spaces = text.len() - rest.len();
    let marker = if ["• ", "- ", "* "].iter().any(|m| rest.starts_with(m)) {
        2
    } else {
        let digits = rest.chars().take_while(char::is_ascii_digit).count();
        if digits > 0 && rest[digits..].starts_with(". ") {
            digits + 2
        } else {
            0
        }
    };
    spaces + marker
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
        assert!(title.contains(" #3 ") && !title.contains("Load the config"));
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

    fn rows(text: Vec<Line<'static>>, width: u16) -> Vec<String> {
        wrap(Text::from(text), width)
            .iter()
            .map(|line| line.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect()
    }

    #[test]
    fn wrap_counts_rows_and_splits_long_words() {
        let text = || vec![Line::raw("aaaa bbbb cccc"), Line::raw(""), Line::raw("x")];
        assert_eq!(rows(text(), 20).len(), 3);
        assert_eq!(rows(text(), 9), ["aaaa bbbb", "cccc", "", "x"]);
        assert_eq!(rows(text(), 4).len(), 5);
        assert_eq!(rows(vec![Line::raw("abcdefgh")], 3), ["abc", "def", "gh"]);
    }

    #[test]
    fn wrap_continues_under_the_text_of_an_indented_or_listed_line() {
        assert_eq!(
            rows(vec![Line::raw("  • alpha beta gamma")], 12),
            ["  • alpha", "    beta", "    gamma"]
        );
        assert_eq!(
            rows(vec![Line::raw("  src/a/very/long/path.rs")], 12),
            ["  src/a/very", "  /long/path", "  .rs"]
        );
        let styled = Line::from(vec![
            Span::raw("  "),
            Span::styled(
                "red words here",
                Style::new().fg(ratatui::style::Color::Red),
            ),
        ]);
        let wrapped = wrap(Text::from(vec![styled]), 12);
        assert_eq!(wrapped.len(), 2);
        assert!(
            wrapped[1]
                .spans
                .iter()
                .any(|s| s.content == "here" && s.style.fg == Some(ratatui::style::Color::Red)),
            "a word keeps its style across the break: {wrapped:?}"
        );
    }

    fn review_app_on(id: &str) -> App {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let path = root
            .join("tests")
            .join("fixtures")
            .join("review-ledger.toml");
        let source = tomlctl::LedgerRef::File(path);
        let value = tomlctl::ledger_read(root, &source).expect("fixture reads");
        let ledger = crate::ledger::Ledger::from_value(value, source).expect("fixture loads");
        let mut app = app_on(1);
        app.surface = Surface::Review;
        let state = app.items.get_mut(&Surface::Review).expect("review state");
        state.set_rows(ledger.rows, Some("fixture"));
        state.cursor = Some(id.to_string());
        app
    }

    fn item_lines(app: &App, now: SystemTime) -> Vec<String> {
        let row = app
            .current_items()
            .and_then(ItemsState::cursor_row)
            .expect("a cursor row");
        let agents = app.index.agents_for_item(&app.snapshot, &row.id);
        plain(&item_content(row, &agents, &[], &app.theme, now))
    }

    #[test]
    fn item_details_show_the_wontfix_rationale() {
        let lines = item_lines(&review_app_on("R4"), SystemTime::now());
        assert_eq!(
            lines,
            [
                "R4 map could match on a tuple instead of nesting",
                "✗ wontfix",
                "  wontfix rationale: the nesting mirrors the key table",
                "severity suggestion  category idiom  effort trivial",
                "at src/keys.rs",
                "",
                "flagged 2026-09-21  rounds 1",
            ]
        );

        let lines = item_lines(&review_app_on("R3"), SystemTime::now());
        let after = |label: &str| {
            let at = lines.iter().position(|l| l == label).expect(label);
            lines[at + 1..]
                .iter()
                .take_while(|l| !l.is_empty())
                .cloned()
                .collect::<Vec<_>>()
        };
        assert_eq!(lines[2], "  resolution: fixed in abc1234");
        assert_eq!(lines[4], "at src/io.rs:7:read_all");
        assert_eq!(
            after("Evidence"),
            ["  src/io.rs:7 — the Err arm returns Ok(Vec::new())"]
        );
        assert_eq!(
            after("Instances"),
            ["  src/io.rs:read_all", "  src/net.rs:fetch"]
        );
        assert_eq!(
            lines.last().map(String::as_str),
            Some("flagged 2026-09-20  closed 2026-09-25  rounds 2")
        );
    }

    #[test]
    fn item_details_render_the_description_and_running_agents() {
        let mut app = review_app_on("R2");
        app.snapshot.agents[0].segments = vec![crate::model::Segment {
            item_ids: vec!["R2".to_string()],
            started_at: "2026-09-28T11:04:22Z".to_string(),
            ..crate::model::Segment::default()
        }];
        app.index = app.snapshot.index();
        let lines = item_lines(&app, at("2026-09-28T12:04:22Z"));
        let at = lines
            .iter()
            .position(|l| l == "Description")
            .expect("Description");
        assert_eq!(
            lines[at + 1],
            "  The hit path clones a Vec of rows; a borrow would do."
        );
        assert!(lines.contains(&"    R2  2026-09-28T11:04:22Z → now  1h00m".to_string()));
    }

    #[test]
    fn render_titles_the_item_surface_by_the_cursor_row() {
        let draw = |app: &App| {
            let mut terminal = Terminal::new(TestBackend::new(60, 12)).expect("terminal");
            terminal
                .draw(|frame| {
                    render(frame, frame.area(), app, 0);
                })
                .expect("draw");
            let buffer = terminal.backend().buffer().clone();
            (0..buffer.area.height)
                .map(|y| {
                    (0..buffer.area.width)
                        .map(|x| buffer[(x, y)].symbol())
                        .collect::<String>()
                })
                .collect::<Vec<_>>()
        };
        let mut app = review_app_on("R4");
        let rows = draw(&app);
        assert!(rows[0].contains(" R4 "), "{}", rows[0]);
        assert!(rows[1].contains("R4 map could match"), "{}", rows[1]);

        app.items.get_mut(&Surface::Review).expect("state").cursor = None;
        let rows = draw(&app);
        assert!(rows[0].contains(" details "), "{}", rows[0]);
        assert!(rows[1].contains("No item selected."), "{}", rows[1]);
    }

    #[test]
    fn implied_needs_are_marked_and_overlaps_listed() {
        let mut app = app_on(5);
        app.implied.insert((1, 5));
        let lines = plain(&content(&app, at("2026-09-28T12:00:00Z")));
        assert!(lines.contains(&"  ✓ 1 Scaffold the store  implied".to_string()));
        assert!(lines.contains(&"  ✓ 2 Define the schema".to_string()));
        let at = lines
            .iter()
            .position(|l| l == "Shares files with")
            .expect("an overlap section");
        assert!(
            lines[at + 1].ends_with("4 Render the rows"),
            "{}",
            lines[at + 1]
        );
    }
}
