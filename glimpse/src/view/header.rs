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
            app.theme.pending,
        ))
    })
}

/// One row, or two when there is an error, a warning or a flow event to show.
pub(crate) fn height(app: &App) -> u16 {
    if second_row(app).is_some() { 2 } else { 1 }
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

    let mut spans = vec![Span::styled(snap.slug.clone(), app.theme.badge)];
    if !snap.flow_status.is_empty() {
        spans.push(Span::styled(
            format!("  {}", snap.flow_status),
            app.theme.status(&snap.flow_status),
        ));
    }
    if !snap.policy.checkpoints.is_empty() {
        spans.push(Span::raw(format!("  {}", snap.policy.checkpoints)));
    }
    spans.push(Span::raw(format!("  {done}/{} done", snap.tasks.len())));
    if app.follow {
        spans.push(Span::styled("  ⟳ follow", app.theme.in_progress));
    } else {
        spans.push(Span::styled(
            format!("  ⏸ paused (+{})", app.pending_changes),
            app.theme.warning,
        ));
    }
    spans.push(Span::styled(
        format!("  {running} running"),
        app.theme.agent_chip,
    ));
    Line::from(spans)
}

pub(crate) fn render(frame: &mut Frame, area: Rect, app: &App) {
    let mut lines = vec![first_row(app)];
    if let Some(line) = second_row(app) {
        lines.push(line);
    }
    frame.render_widget(Paragraph::new(lines), area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::model::fixture;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn draw(app: &App) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(100, 2)).expect("terminal");
        terminal
            .draw(|frame| render(frame, frame.area(), app))
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
        assert_eq!(height(&app), 2);
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
        assert_eq!(height(&app), 1);
    }
}
