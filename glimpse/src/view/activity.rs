//! The activity panel for the selected task's newest running agent.
//!
//! The panel draws from a [`TailState`] the caller owns and refreshes; it
//! reads no files itself. [`agent`] names the row whose transcript the caller
//! should point that tail at.

use std::time::{SystemTime, UNIX_EPOCH};

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Padding, Paragraph};

use crate::app::App;
use crate::hook::parse_utc;
use crate::model::{Agent, AgentStatus};
use crate::transcript::{EntryKind, TailState};

/// Entries shown at most, newest last.
pub(crate) const SHOWN_ENTRIES: usize = 10;

/// The newest `running` agent assigned to the selected task.
pub(crate) fn agent(app: &App) -> Option<&Agent> {
    let id = app.selected?;
    app.index
        .agents_for(&app.snapshot, id)
        .into_iter()
        .find(|agent| agent.status == AgentStatus::Running)
}

/// The panel's frame: themed border and title, a cell of padding inside each side.
fn panel(app: &App, title: String) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_style(app.theme.border)
        .title(Line::styled(title, app.theme.border_title))
        .padding(Padding::horizontal(1))
}

pub(crate) fn render(frame: &mut Frame, area: Rect, app: &App, tail: &TailState, now: SystemTime) {
    let Some(agent) = agent(app) else {
        let message = match app.selected {
            Some(id) => format!("no running agent on task {id}"),
            None => "no task selected".to_string(),
        };
        let block = panel(app, " activity ".to_string());
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(message, app.theme.secondary))).block(block),
            area,
        );
        return;
    };

    let title = format!(" {} {} ", agent.agent_type, agent.id);
    let block = panel(app, title);

    let mut status = Vec::new();
    if let Some(elapsed) = parse_utc(&agent.started_at).and_then(|start| {
        now.duration_since(UNIX_EPOCH)
            .ok()?
            .as_secs()
            .checked_sub(start)
    }) {
        status.push(Span::styled(
            format!("running {}", format_elapsed(elapsed)),
            app.theme.in_progress,
        ));
    } else {
        status.push(Span::styled("running", app.theme.in_progress));
    }
    let tokens = tail.tokens.unwrap_or(agent.context_tokens);
    if tokens > 0 {
        status.push(Span::raw(format!("  {} tokens", format_tokens(tokens))));
    }
    if app.stale_agents.contains(&agent.id) {
        status.push(Span::styled("  stale", app.theme.warning));
    }
    let mut lines = vec![Line::from(status)];

    if tail.rejected {
        lines.push(Line::from(Span::styled(
            "transcript outside the Claude directory; not read",
            app.theme.warning,
        )));
    } else if tail.entries.is_empty() {
        lines.push(Line::from(Span::styled(
            "no activity yet",
            app.theme.secondary,
        )));
    }
    let inner_rows = usize::from(area.height.saturating_sub(3));
    let shown = SHOWN_ENTRIES.min(inner_rows);
    let skip = tail.entries.len().saturating_sub(shown);
    for entry in &tail.entries[skip..] {
        let mut spans = vec![Span::styled(
            format!("{} ", clock(&entry.ts)),
            app.theme.secondary,
        )];
        match &entry.kind {
            EntryKind::ToolUse { name, detail } => {
                spans.push(Span::styled(format!("▸ {name}"), app.theme.section));
                if !detail.is_empty() {
                    spans.push(Span::raw(format!("  {detail}")));
                }
            }
            EntryKind::Text => spans.push(Span::raw(format!("  {}", entry.text))),
        }
        lines.push(Line::from(spans));
    }

    frame.render_widget(Paragraph::new(lines).block(block), area);
}

/// `HH:MM:SS` out of an RFC 3339 timestamp, or blanks when it is not one.
fn clock(ts: &str) -> &str {
    ts.get(11..19).unwrap_or("        ")
}

fn format_elapsed(secs: u64) -> String {
    let (h, m, s) = (secs / 3_600, secs % 3_600 / 60, secs % 60);
    if h > 0 {
        format!("{h}h {m:02}m")
    } else if m > 0 {
        format!("{m}m {s:02}s")
    } else {
        format!("{s}s")
    }
}

fn format_tokens(tokens: u64) -> String {
    if tokens >= 1_000 {
        format!("{:.1}k", tokens as f64 / 1_000.0)
    } else {
        tokens.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::model::fixture;
    use crate::transcript::Entry;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use std::time::Duration;

    fn draw(app: &App, tail: &TailState, now: SystemTime, height: u16) -> Vec<String> {
        let width = 80;
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
        terminal
            .draw(|frame| render(frame, frame.area(), app, tail, now))
            .expect("draw");
        let buffer = terminal.backend().buffer().clone();
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol().to_string())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect()
    }

    fn at(ts: &str) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(parse_utc(ts).expect("timestamp"))
    }

    fn tail_with(count: usize) -> TailState {
        let mut tail = TailState::within("", None);
        for n in 0..count {
            tail.entries.push(Entry {
                ts: format!("2026-09-28T11:{:02}:00Z", 5 + n),
                kind: if n % 2 == 0 {
                    EntryKind::ToolUse {
                        name: "Bash".to_string(),
                        detail: format!("step {n}"),
                    }
                } else {
                    EntryKind::Text
                },
                text: if n % 2 == 0 {
                    String::new()
                } else {
                    format!("note {n}")
                },
            });
        }
        tail
    }

    #[test]
    fn the_panel_shows_elapsed_tokens_and_the_last_ten_entries() {
        let mut app = App::new(fixture(), &Config::default());
        app.selected = Some(4);
        app.stale_agents.clear();
        let mut tail = tail_with(12);
        tail.tokens = Some(45_210);
        let now = at("2026-09-28T11:16:56Z");

        let rows = draw(&app, &tail, now, 16);
        let screen = rows.join("\n");
        assert!(rows[0].contains("implement-deep A2"), "{screen}");
        assert!(rows[1].contains("running 12m 34s"), "{screen}");
        assert!(rows[1].contains("45.2k tokens"), "{screen}");
        assert!(
            !screen.contains("11:05:00") && !screen.contains("11:06:00"),
            "{screen}"
        );
        assert!(
            rows[2].contains("11:07:00")
                && rows[2].contains("▸ Bash")
                && rows[2].contains("step 2"),
            "{screen}"
        );
        assert!(
            rows[11].contains("11:16:00") && rows[11].contains("note 11"),
            "{screen}"
        );
    }

    #[test]
    fn the_panel_falls_back_to_the_rows_token_count_and_flags_staleness() {
        let mut snap = fixture();
        snap.agents[1].context_tokens = 900;
        let mut app = App::new(snap, &Config::default());
        app.selected = Some(4);
        app.stale_agents.insert("A2".to_string());
        let rows = draw(&app, &tail_with(0), at("2026-09-28T13:10:22Z"), 8);
        assert!(rows[1].contains("running 2h 06m"), "{rows:?}");
        assert!(rows[1].contains("900 tokens"), "{rows:?}");
        assert!(rows[1].contains("stale"), "{rows:?}");
        assert!(rows[2].contains("no activity yet"), "{rows:?}");
    }

    #[test]
    fn only_a_running_agent_is_shown() {
        let mut app = App::new(fixture(), &Config::default());
        // Task 3's only agent is an idle teammate.
        app.selected = Some(3);
        assert!(agent(&app).is_none());
        let rows = draw(&app, &tail_with(3), at("2026-09-28T12:00:00Z"), 6);
        assert!(rows[1].contains("no running agent on task 3"), "{rows:?}");

        app.selected = Some(4);
        assert_eq!(agent(&app).map(|a| a.id.as_str()), Some("A2"));
    }

    #[test]
    fn a_rejected_transcript_says_so() {
        let mut app = App::new(fixture(), &Config::default());
        app.selected = Some(4);
        let mut tail = tail_with(0);
        tail.rejected = true;
        let rows = draw(&app, &tail, at("2026-09-28T12:00:00Z"), 6);
        assert!(rows[2].contains("outside the Claude directory"), "{rows:?}");
    }

    #[test]
    fn elapsed_and_tokens_format_compactly() {
        assert_eq!(format_elapsed(45), "45s");
        assert_eq!(format_elapsed(754), "12m 34s");
        assert_eq!(format_elapsed(7_560), "2h 06m");
        assert_eq!(format_tokens(999), "999");
        assert_eq!(format_tokens(61_872), "61.9k");
    }
}
