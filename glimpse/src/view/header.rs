//! The header strip: flow identity, progress, follow state and running agents.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::App;
use crate::model::{AgentStatus, RecordEntry, TaskStatus};

/// The latest flow-level record entry: a status transition or a reconcile.
fn latest_flow_event(app: &App) -> Option<&RecordEntry> {
    app.snapshot
        .record
        .iter()
        .rev()
        .find(|entry| matches!(entry.entry_type.as_str(), "status-transition" | "reconcile"))
}

/// The second row's text: the source error, else the config warning, else
/// the latest flow-level event.
fn second_row(app: &App) -> Option<Line<'static>> {
    if let Some(error) = &app.source_error {
        return Some(Line::from(Span::styled(
            format!("! {error}"),
            app.theme.warning,
        )));
    }
    if let Some(warning) = &app.warning {
        return Some(Line::from(Span::styled(
            format!("! {warning}"),
            app.theme.warning,
        )));
    }
    latest_flow_event(app).map(|entry| {
        Line::from(Span::styled(
            format!("{} {}: {}", entry.date, entry.entry_type, entry.summary),
            app.theme.secondary,
        ))
    })
}

/// One row, or two when there is an error, a warning or a flow event to show. A
/// `compact` header drops the flow event, which is history rather than state.
pub(crate) fn height(app: &App, compact: bool) -> u16 {
    if rows(app, compact).len() > 1 { 2 } else { 1 }
}

fn rows(app: &App, compact: bool) -> Vec<Line<'static>> {
    let mut lines = vec![if compact {
        compact_row(app)
    } else {
        first_row(app)
    }];
    let second = if compact && app.source_error.is_none() && app.warning.is_none() {
        None
    } else {
        second_row(app)
    };
    lines.extend(second);
    lines
}

/// `slug  3/8  ⟳  1▶`: the first row with the flow status, the checkpoint policy and the
/// words dropped, to fit a narrow pane.
fn compact_row(app: &App) -> Line<'static> {
    let snap = &app.snapshot;
    let done = snap
        .tasks
        .iter()
        .filter(|task| task.status == TaskStatus::Done)
        .count();
    let running = snap
        .agents
        .iter()
        .filter(|agent| agent.status == AgentStatus::Running)
        .count();
    let follow = if app.follow {
        Span::styled("⟳", app.theme.in_progress)
    } else {
        Span::styled(format!("⏸+{}", app.pending_changes), app.theme.warning)
    };
    Line::from(vec![
        Span::styled(snap.slug.clone(), app.theme.slug),
        GAP,
        Span::raw(format!("{done}/{}", snap.tasks.len())),
        GAP,
        follow,
        GAP,
        running_chip(app, format!("{running}▶"), running),
    ])
}

fn first_row(app: &App) -> Line<'static> {
    let snap = &app.snapshot;
    let done = snap
        .tasks
        .iter()
        .filter(|task| task.status == TaskStatus::Done)
        .count();
    let running = snap
        .agents
        .iter()
        .filter(|agent| agent.status == AgentStatus::Running)
        .count();

    let mut spans = vec![Span::styled(snap.slug.clone(), app.theme.slug)];
    if !snap.flow_status.is_empty() {
        spans.push(GAP);
        spans.push(Span::styled(
            snap.flow_status.clone(),
            app.theme.status(&snap.flow_status),
        ));
    }
    if !snap.policy.checkpoints.is_empty() {
        spans.push(GAP);
        spans.push(Span::styled(
            snap.policy.checkpoints.clone(),
            app.theme.checkpoint,
        ));
    }
    spans.push(GAP);
    spans.push(Span::raw(format!("{done}/{} done", snap.tasks.len())));
    spans.push(GAP);
    if app.follow {
        spans.push(Span::styled("⟳ follow", app.theme.in_progress));
    } else {
        spans.push(Span::styled(
            format!("⏸ paused (+{})", app.pending_changes),
            app.theme.warning,
        ));
    }
    spans.push(GAP);
    spans.push(running_chip(app, format!("{running} running"), running));
    Line::from(spans)
}

/// Separators stay unstyled so no chip's background runs into them.
const GAP: Span<'static> = Span {
    style: ratatui::style::Style::new(),
    content: std::borrow::Cow::Borrowed("  "),
};

/// The running-agents count, as a chip while any agent runs.
fn running_chip(app: &App, text: String, running: usize) -> Span<'static> {
    if running > 0 {
        super::chip(&text, app.theme.agent_chip)
    } else {
        Span::styled(text, app.theme.secondary)
    }
}

pub(crate) fn render(frame: &mut Frame, area: Rect, app: &App, compact: bool) {
    frame.render_widget(Paragraph::new(rows(app, compact)), area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::model::fixture;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn draw(app: &App) -> Vec<String> {
        draw_as(app, false)
    }

    fn draw_as(app: &App, compact: bool) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(100, 2)).expect("terminal");
        terminal
            .draw(|frame| render(frame, frame.area(), app, compact))
            .expect("draw");
        let buffer = terminal.backend().buffer().clone();
        (0..2)
            .map(|y| {
                (0..100)
                    .map(|x| buffer[(x, y)].symbol().to_string())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect()
    }

    #[test]
    fn the_first_row_shows_identity_progress_follow_and_agents() {
        let app = App::new(fixture(), &Config::default());
        let rows = draw(&app);
        for want in [
            "demo-flow",
            "in-progress",
            "milestones",
            "3/8 done",
            "⟳ follow",
            "1 running",
        ] {
            assert!(
                rows[0].contains(want),
                "{want:?} missing from {:?}",
                rows[0]
            );
        }
        assert!(
            rows[1].contains("status-transition") && rows[1].contains("planned -> in-progress"),
            "the second row carries the latest flow event: {:?}",
            rows[1]
        );
        assert_eq!(height(&app, false), 2);
    }

    #[test]
    fn the_compact_header_is_one_short_row_unless_something_is_wrong() {
        let mut app = App::new(fixture(), &Config::default());
        let rows = draw_as(&app, true);
        assert_eq!(rows[0], "demo-flow  3/8  ⟳   1▶");
        assert_eq!(rows[1], "", "the flow event is dropped");
        assert_eq!(height(&app, true), 1);

        app.follow = false;
        app.pending_changes = 2;
        app.warning = Some("unknown config key".to_string());
        let rows = draw_as(&app, true);
        assert!(rows[0].contains("⏸+2"), "{:?}", rows[0]);
        assert!(rows[1].contains("unknown config key"));
        assert_eq!(height(&app, true), 2);
    }

    #[test]
    fn the_running_chip_is_padded_evenly_and_its_gap_is_unstyled() {
        let app = App::new(fixture(), &Config::default());
        let mut terminal = Terminal::new(TestBackend::new(100, 2)).expect("terminal");
        terminal
            .draw(|frame| render(frame, frame.area(), &app, false))
            .expect("draw");
        let buffer = terminal.backend().buffer();
        let row: Vec<&str> = (0..100).map(|x| buffer[(x, 0)].symbol()).collect();
        let start = (0..row.len())
            .find(|&x| row[x..].iter().take(9).copied().collect::<String>() == "1 running")
            .expect("the chip");
        let first = u16::try_from(start).expect("column");
        let last = first + 8;
        let bg = app.theme.agent_chip.bg.expect("a background");
        assert_eq!(buffer[(first - 1, 0)].bg, bg, "one padding cell before");
        assert_eq!(buffer[(last + 1, 0)].bg, bg, "one padding cell after");
        assert_ne!(buffer[(first - 2, 0)].bg, bg, "the gap is not painted");
    }

    #[test]
    fn a_paused_follow_counts_pending_changes() {
        let mut app = App::new(fixture(), &Config::default());
        app.follow = false;
        app.pending_changes = 3;
        let rows = draw(&app);
        assert!(rows[0].contains("⏸ paused (+3)"), "{:?}", rows[0]);
        assert!(!rows[0].contains("⟳ follow"));
    }

    #[test]
    fn an_error_or_warning_replaces_the_event_row() {
        let mut app = App::new(fixture(), &Config::default());
        app.warning = Some("unknown config key".to_string());
        assert!(draw(&app)[1].contains("unknown config key"));

        app.source_error = Some("tomlctl failed".to_string());
        let rows = draw(&app);
        assert!(rows[1].contains("tomlctl failed"), "{:?}", rows[1]);
        assert!(!rows[1].contains("status-transition"));
    }

    #[test]
    fn a_flow_with_nothing_to_report_takes_one_row() {
        let mut snap = fixture();
        snap.record.clear();
        let app = App::new(snap, &Config::default());
        assert_eq!(height(&app, false), 1);
    }
}
