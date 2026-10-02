//! The header strip: flow identity, progress, follow state, running agents and the surface tabs.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::App;
use crate::ledger::StatusClass;
use crate::model::{AgentStatus, RecordEntry, TaskStatus};
use crate::surface::Surface;

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

/// The identity row and the surface tabs, plus a row when there is an error, a
/// warning or a flow event to show. A `compact` header folds the tabs into its one
/// row and drops the flow event, which is history rather than state.
pub(crate) fn height(app: &App, compact: bool) -> u16 {
    u16::try_from(rows(app, compact).len()).unwrap_or(u16::MAX)
}

fn rows(app: &App, compact: bool) -> Vec<Line<'static>> {
    let mut lines = if compact {
        vec![compact_row(app)]
    } else {
        vec![first_row(app), tabs_row(app)]
    };
    let second = if compact && app.source_error.is_none() && app.warning.is_none() {
        None
    } else {
        second_row(app)
    };
    lines.extend(second);
    lines
}

/// A tab's count text: open rows, `—` while the surface has no ledger file read,
/// and Inbox's unanswered questions marked `?`. Tasks has none.
fn tab_count(app: &App, surface: Surface) -> Option<String> {
    let count = match surface {
        Surface::Tasks => return None,
        Surface::Inbox => app.inbox_unanswered().map(|n| {
            if n > 0 {
                format!("{n}?")
            } else {
                n.to_string()
            }
        }),
        _ => app
            .items
            .get(&surface)
            .filter(|state| matches!(state.revision, Some(Some(_))))
            .map(|state| {
                let open = state
                    .rows
                    .iter()
                    .filter(|row| row.class == StatusClass::Live);
                open.count().to_string()
            }),
    };
    Some(count.unwrap_or_else(|| "—".to_string()))
}

fn tab_style(app: &App, surface: Surface) -> Style {
    if app.surface == surface {
        app.theme.surface_tab_active
    } else {
        app.theme.surface_tab
    }
}

/// The `+N` arrivals since the surface was last on screen, when there are any.
fn arrival_badge(app: &App, surface: Surface) -> Option<Span<'static>> {
    let arrived = match surface {
        Surface::Inbox => app.inbox.new_since_view,
        _ => app
            .items
            .get(&surface)
            .map_or(0, |state| state.new_since_view),
    };
    (arrived > 0).then(|| Span::styled(format!("+{arrived}"), app.theme.arrival_badge))
}

/// `Tasks  Review 12 +3  Optimise 4  Plan-review —  Backlog 6  Inbox 2?`
fn tabs_row(app: &App) -> Line<'static> {
    let mut spans = Vec::new();
    for surface in Surface::ALL {
        if !spans.is_empty() {
            spans.push(GAP);
        }
        let text = match tab_count(app, surface) {
            Some(count) => format!("{} {count}", surface.label()),
            None => surface.label().to_string(),
        };
        spans.push(Span::styled(text, tab_style(app, surface)));
        if let Some(badge) = arrival_badge(app, surface) {
            spans.push(Span::raw(" "));
            spans.push(badge);
        }
    }
    Line::from(spans)
}

/// `R12+3 O4 P— B6 I2?`: the item surfaces' tabs by initial, for the compact row.
fn compact_tabs(app: &App) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    for surface in Surface::ALL {
        let Some(count) = tab_count(app, surface) else {
            continue;
        };
        if !spans.is_empty() {
            spans.push(Span::raw(" "));
        }
        let initial = surface.label().chars().next().unwrap_or(' ');
        spans.push(Span::styled(
            format!("{initial}{count}"),
            tab_style(app, surface),
        ));
        spans.extend(arrival_badge(app, surface));
    }
    spans
}

/// `slug  3/8  ⟳  1▶  R12+3 …`: the first row with the flow status, the checkpoint
/// policy and the words dropped, and the tabs folded in, to fit a narrow pane.
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
    let mut spans = vec![
        Span::styled(snap.slug.clone(), app.theme.slug),
        GAP,
        Span::raw(format!("{done}/{}", snap.tasks.len())),
        GAP,
        follow,
        GAP,
        running_chip(app, format!("{running}▶"), running),
        GAP,
    ];
    spans.extend(compact_tabs(app));
    Line::from(spans)
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
    use crate::ledger::ItemRow;
    use crate::model::fixture;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn draw(app: &App) -> Vec<String> {
        draw_as(app, false)
    }

    fn draw_as(app: &App, compact: bool) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(100, 3)).expect("terminal");
        terminal
            .draw(|frame| render(frame, frame.area(), app, compact))
            .expect("draw");
        let buffer = terminal.backend().buffer().clone();
        (0..3)
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
            rows[2].contains("status-transition") && rows[2].contains("planned -> in-progress"),
            "the third row carries the latest flow event: {:?}",
            rows[2]
        );
        assert_eq!(height(&app, false), 3);
    }

    fn rows_of(statuses: &[&str]) -> Vec<ItemRow> {
        statuses
            .iter()
            .enumerate()
            .map(|(n, status)| ItemRow {
                id: format!("R{n}"),
                status: (*status).to_string(),
                class: StatusClass::of(status),
                ..ItemRow::default()
            })
            .collect()
    }

    fn load(app: &mut App, surface: Surface, rows: Vec<ItemRow>, revision: Option<Option<&str>>) {
        let state = app.items.get_mut(&surface).expect("an item surface");
        state.rows = rows;
        state.revision = revision.map(|r| r.map(str::to_string));
    }

    #[test]
    fn surface_tabs_show_counts_and_badges() {
        let mut app = App::new(fixture(), &Config::default());
        load(
            &mut app,
            Surface::Review,
            rows_of(&["open", "open", "fixed", "deferred"]),
            Some(Some("r1")),
        );
        app.items
            .get_mut(&Surface::Review)
            .expect("review")
            .new_since_view = 3;
        load(
            &mut app,
            Surface::Optimise,
            rows_of(&["open"]),
            Some(Some("o1")),
        );
        load(&mut app, Surface::Backlog, Vec::new(), Some(None));
        app.surface = Surface::Optimise;

        let rows = draw(&app);
        assert_eq!(
            rows[1],
            "Tasks  Review 2 +3  Optimise 1  Plan-review —  Backlog —  Inbox —"
        );
        assert!(
            draw_as(&app, true)[0].ends_with("  R2+3 O1 P— B— I—"),
            "{:?}",
            draw_as(&app, true)[0]
        );

        let mut terminal = Terminal::new(TestBackend::new(100, 3)).expect("terminal");
        terminal
            .draw(|frame| render(frame, frame.area(), &app, false))
            .expect("draw");
        let buffer = terminal.backend().buffer();
        let column = |needle: &str| {
            let at = rows[1].find(needle).expect("on the tabs row");
            u16::try_from(rows[1][..at].chars().count()).expect("column")
        };
        assert_eq!(
            buffer[(column("Optimise"), 1)].fg,
            app.theme.surface_tab_active.fg.expect("a colour"),
            "the active tab"
        );
        assert_eq!(
            buffer[(column("Review"), 1)].fg,
            app.theme.surface_tab.fg.expect("a colour")
        );
        assert_eq!(
            buffer[(column("+3"), 1)].fg,
            app.theme.arrival_badge.fg.expect("a colour")
        );
    }

    #[test]
    fn apply_inputs_updates_the_inbox_badge() {
        use crate::ledger::{InputRow, Inputs};
        let record = |id: &str, kind: &str, status: &str| InputRow {
            id: id.to_string(),
            kind: kind.to_string(),
            status: status.to_string(),
            ..InputRow::default()
        };
        let inputs = |revision: &str, rows: Vec<InputRow>| Inputs {
            path: ".claude/inputs.toml".to_string(),
            revision: Some(revision.to_string()),
            rows,
        };
        let mut app = App::new(fixture(), &Config::default());
        let t0 = std::time::Instant::now();
        app.apply_inputs(inputs("v1", vec![record("I1", "note", "new")]), t0);
        assert!(draw(&app)[1].ends_with("Inbox 0"), "{:?}", draw(&app)[1]);

        let rows = vec![
            record("I1", "note", "new"),
            record("I2", "question", "new"),
            record("I3", "question", "handled"),
        ];
        app.apply_inputs(inputs("v2", rows), t0);
        assert!(
            draw(&app)[1].ends_with("Inbox 1? +2"),
            "{:?}",
            draw(&app)[1]
        );
        assert!(draw_as(&app, true)[0].ends_with(" I1?+2"));
    }

    #[test]
    fn the_compact_header_is_one_short_row_unless_something_is_wrong() {
        let mut app = App::new(fixture(), &Config::default());
        let rows = draw_as(&app, true);
        assert_eq!(rows[0], "demo-flow  3/8  ⟳   1▶   R— O— P— B— I—");
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
        assert!(draw(&app)[2].contains("unknown config key"));

        let error = "parse .claude/flows/demo-flow/tasks.toml: expected `=` at line 4";
        app.source_error = Some(error.to_string());
        let rows = draw(&app);
        assert!(rows[2].contains(error), "{:?}", rows[2]);
        assert!(!rows[2].contains("status-transition"));
    }

    #[test]
    fn a_flow_with_nothing_to_report_takes_the_identity_and_tabs_rows() {
        let mut snap = fixture();
        snap.record.clear();
        let app = App::new(snap, &Config::default());
        assert_eq!(height(&app, false), 2);
        assert_eq!(height(&app, true), 1);
    }
}
