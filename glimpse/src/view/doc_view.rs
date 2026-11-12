//! A fetched document in the details panel.
//!
//! The panel shows it in place of the row's details while one is open: the border names the
//! target and any notice, an authored markdown file is rendered at the panel's width, and
//! everything else is plain text with a cited line marked.

use ratatui::text::{Line, Text};

use crate::app::App;
use crate::model::Task;
use crate::nav::{DocState, Target};
use crate::plan_state::Record;
use crate::source::Fetched;
use crate::view::details::wrap;
use crate::view::doc::{Doc, Link};
use crate::view::{markdown, plan_details};

/// The border title and the document for `target`, its text laid out for a panel `width`
/// cells wide inside its borders.
pub(crate) fn content(app: &App, target: &Target, width: u16) -> (String, Doc) {
    let theme = &app.theme;
    let (title, text) = match app.doc_state(target) {
        DocState::Loading => (
            name(target),
            Text::from(Line::styled("loading…", theme.pending)),
        ),
        DocState::Failed(reason) => (
            name(target),
            Text::from(Line::styled(reason.clone(), theme.failed)),
        ),
        DocState::Text(fetched) => {
            let mut title = fetched.title.clone();
            for notice in &fetched.notices {
                title.push_str(" · ");
                title.push_str(notice);
            }
            (title, body(app, fetched, width))
        }
    };
    let doc = Doc {
        text,
        links: links(app, target),
    };
    (format!(" {title} "), doc)
}

/// The scroll that puts `line` of `fetched` a third of the way down a panel `width` wide
/// and `page` rows high.
pub(crate) fn scroll_to(app: &App, fetched: &Fetched, line: u32, width: u16, page: u16) -> u16 {
    let before = usize::try_from(line.saturating_sub(1)).unwrap_or(usize::MAX);
    let lines: Vec<Line<'static>> = body(app, fetched, width)
        .lines
        .into_iter()
        .take(before)
        .collect();
    let rows = wrap(Text::from(lines), width).len();
    u16::try_from(rows)
        .unwrap_or(u16::MAX)
        .saturating_sub(page / 3)
}

/// What the title says before the document has arrived.
fn name(target: &Target) -> String {
    match target {
        Target::Authored(path) => path.clone(),
        Target::Source {
            path,
            line: Some(line),
            ..
        } => format!("{path}:{line}"),
        Target::Source { path, .. } => path.clone(),
        Target::Commit(sha) => format!("commit {sha}"),
        Target::Finding { store, id } => format!("{id} in {store}"),
        Target::Url(url) => url.clone(),
        other => format!("{other:?}"),
    }
}

fn body(app: &App, fetched: &Fetched, width: u16) -> Text<'static> {
    if fetched.markdown {
        return markdown::to_text_within(&fetched.text, &app.theme, usize::from(width));
    }
    let marked = fetched
        .line
        .and_then(|line| usize::try_from(line).ok())
        .filter(|line| *line > 0);
    let lines: Vec<Line<'static>> = fetched
        .text
        .lines()
        .enumerate()
        .map(|(at, line)| {
            if Some(at + 1) == marked {
                Line::styled(line.to_string(), app.theme.selection)
            } else {
                Line::raw(line.to_string())
            }
        })
        .collect();
    Text::from(lines)
}

/// An authored file links what its Plan row links; a commit, the tasks stamped with it; a
/// finding, the tasks that fix or settle it.
fn links(app: &App, target: &Target) -> Vec<Link> {
    match target {
        Target::Authored(path) => app
            .plan
            .rows
            .iter()
            .find(|row| matches!(&row.record, Record::Authored(p) if p == path))
            .map(|row| plan_details::content(app, row).links)
            .unwrap_or_default(),
        Target::Commit(sha) => task_links(app, |task| same_commit(&task.commit, sha)),
        Target::Finding { store, id } => task_links(app, |task| {
            task.finding.as_ref().is_some_and(|finding| {
                &finding.store == store
                    && (finding.fixes.contains(id) || finding.settles.contains(id))
            })
        }),
        _ => Vec::new(),
    }
}

fn task_links(app: &App, keep: impl Fn(&Task) -> bool) -> Vec<Link> {
    app.snapshot
        .tasks
        .iter()
        .filter(|task| keep(task))
        .map(|task| Link {
            label: format!("#{} {}", task.id, task.title),
            target: Target::Task(task.id),
        })
        .collect()
}

/// Either sha may be abbreviated.
fn same_commit(stamped: &str, sha: &str) -> bool {
    !stamped.is_empty() && !sha.is_empty() && (stamped.starts_with(sha) || sha.starts_with(stamped))
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::layout::Rect;

    use super::*;
    use crate::app::Action;
    use crate::authored::{AuthoredFile, AuthoredKind, AuthoredSource};
    use crate::config::Config;
    use crate::model::fixture;
    use crate::model::plan::FindingLink;
    use crate::source::DocRequest;
    use crate::surface::Surface;
    use crate::view::details;

    const NOTE: &str = "notes/keys.md";

    fn app_with(mut snap: crate::model::Snapshot) -> App {
        snap.plan.as_mut().expect("plan").decisions[0].evidence = format!("See {NOTE}.");
        let mut app = App::new(snap, &Config::default());
        app.authored.files = vec![AuthoredFile {
            path: NOTE.to_string(),
            kind: AuthoredKind::Note,
            id: NOTE.to_string(),
            sources: vec![AuthoredSource {
                path: "src/keys.rs".to_string(),
                line: 30,
                read_at: "abc1234".to_string(),
                ..AuthoredSource::default()
            }],
            ..AuthoredFile::default()
        }];
        app.rebuild_plan(Instant::now());
        app
    }

    fn app() -> App {
        app_with(fixture())
    }

    fn source() -> Target {
        Target::Source {
            path: "src/keys.rs".to_string(),
            line: Some(30),
            read_at: "abc1234".to_string(),
            dirty: false,
        }
    }

    fn fetched(title: &str, text: &str, line: Option<u32>, markdown: bool) -> Fetched {
        Fetched {
            title: title.to_string(),
            text: text.to_string(),
            notices: Vec::new(),
            line,
            markdown,
        }
    }

    fn plain(text: &Text<'static>) -> Vec<String> {
        text.lines
            .iter()
            .map(|line| line.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect()
    }

    fn shown(app: &App) -> (String, Doc) {
        let target = app.doc_target().expect("a document is shown");
        content(app, &target, 40)
    }

    /// The app on the note's Plan row with its details open and its text answered.
    fn on_note() -> App {
        let mut app = app();
        app.open(Target::Authored(NOTE.to_string()));
        assert_eq!(app.surface, Surface::Plan);
        assert!(app.details_open, "opening a document opens the panel");
        let request = app.follow_doc().flatten().expect("the note is wanted");
        assert_eq!(
            request,
            DocRequest::Authored {
                slug: app.snapshot.slug.clone(),
                path: NOTE.to_string(),
            }
        );
        let text = "# Keys\n\nThe map.\n\n| key | does |\n|---|---|\n| j | down |\n";
        assert!(app.doc_answer(request, Ok(fetched("notes/keys.md", text, None, true))));
        app
    }

    #[test]
    fn doc_view_an_authored_rows_details_show_its_rendered_text() {
        let app = on_note();
        let (title, doc) = shown(&app);
        assert_eq!(title, " notes/keys.md ");
        let lines = plain(&doc.text);
        assert!(lines.iter().any(|l| l.contains("Keys")), "{lines:?}");
        assert!(
            !lines.iter().any(|l| l.starts_with('#')),
            "rendered: {lines:?}"
        );
        assert!(
            lines.iter().any(|l| l.contains(" │ ")),
            "a table: {lines:?}"
        );
    }

    #[test]
    fn doc_view_b_on_an_open_note_lists_its_sources_and_citing_decision() {
        let mut app = on_note();
        app.apply(Action::ToggleLinks);
        let targets: Vec<Target> = app
            .links
            .as_ref()
            .expect("the list is up")
            .links
            .iter()
            .map(|link| link.target.clone())
            .collect();
        assert_eq!(targets, [source(), Target::Decision("D1".to_string())]);
    }

    #[test]
    fn doc_view_a_source_opens_at_its_line_and_backspace_returns_to_the_note() {
        let mut app = on_note();
        app.regions.details = Some(Rect::new(0, 0, 44, 12));
        app.details_page = 10;
        app.apply(Action::ToggleLinks);
        app.apply(Action::Details);
        assert_eq!(app.doc_target(), Some(source()));

        let request = app.follow_doc().flatten().expect("the source is requested");
        assert_eq!(
            request,
            DocRequest::Source {
                path: "src/keys.rs".to_string(),
                line: Some(30),
                read_at: "abc1234".to_string(),
                dirty: false,
            }
        );
        let (title, doc) = shown(&app);
        assert_eq!(title, " src/keys.rs:30 ");
        assert_eq!(plain(&doc.text), ["loading…"]);

        let text: String = (1..=40).map(|n| format!("line {n}\n")).collect();
        let mut answer = fetched("src/keys.rs @ abc1234", &text, Some(30), false);
        answer.notices = vec!["changed since"];
        assert!(app.doc_answer(request, Ok(answer)));
        assert_eq!(
            app.open_doc().map(|doc| doc.scroll),
            Some(26),
            "line 30 sits a third of the way down a 10-row page"
        );
        let (title, doc) = shown(&app);
        assert_eq!(title, " src/keys.rs @ abc1234 · changed since ");
        assert_eq!(
            doc.text.lines[29].style, app.theme.selection,
            "the line is marked"
        );
        assert_eq!(doc.text.lines[28].style, ratatui::style::Style::default());

        let mut terminal = Terminal::new(TestBackend::new(44, 12)).expect("terminal");
        terminal
            .draw(|frame| {
                let scroll = app.details_scroll;
                details::render(frame, Rect::new(0, 0, 44, 12), &app, scroll);
            })
            .expect("draw");
        let buffer = terminal.backend().buffer();
        let rows: Vec<String> = (0..12)
            .map(|y| (0..44).map(|x| buffer[(x, y)].symbol()).collect())
            .collect();
        assert!(rows.iter().any(|r| r.contains("line 30")), "{rows:#?}");
        assert!(rows[0].contains("changed since"), "{rows:#?}");

        app.apply(Action::GoBack);
        assert_eq!(app.surface, Surface::Plan);
        assert_eq!(app.plan.cursor.as_deref(), Some("notes:notes/keys.md"));
        assert_eq!(app.doc_target(), Some(Target::Authored(NOTE.to_string())));
        assert!(app.open_doc().is_none());
    }

    #[test]
    fn doc_view_a_failure_shows_its_reason() {
        let mut app = app();
        app.open(Target::Commit("abc1234".to_string()));
        let request = app.follow_doc().flatten().expect("requested");
        assert!(app.doc_answer(request, Err("abc1234: unknown revision".to_string())));
        let (title, doc) = shown(&app);
        assert_eq!(title, " commit abc1234 ");
        assert_eq!(plain(&doc.text), ["abc1234: unknown revision"]);
    }

    #[test]
    fn doc_view_a_commit_links_its_tasks_and_a_finding_its_fixing_task() {
        let mut snap = fixture();
        snap.tasks[1].commit = "abc1234def".to_string();
        snap.tasks[4].commit = "abc1234".to_string();
        snap.tasks[5].finding = Some(FindingLink {
            store: "review-v2.toml".to_string(),
            fixes: vec!["F1".to_string()],
            settles: Vec::new(),
        });
        let mut app = app_with(snap);
        let ids = |app: &App| -> Vec<Target> {
            details::links(app)
                .into_iter()
                .map(|link| link.target)
                .collect()
        };

        app.open(Target::Commit("abc1234".to_string()));
        assert_eq!(ids(&app), [Target::Task(2), Target::Task(5)]);

        app.open(Target::Finding {
            store: "review-v2.toml".to_string(),
            id: "F1".to_string(),
        });
        assert_eq!(ids(&app), [Target::Task(6)]);
    }

    #[test]
    fn doc_view_an_answer_for_a_document_no_longer_wanted_is_not_shown() {
        let mut app = app();
        app.open(Target::Commit("aaaaaaa".to_string()));
        let first = app.follow_doc().flatten().expect("first");
        app.open(Target::Commit("bbbbbbb".to_string()));
        let second = app.follow_doc().flatten().expect("second");

        assert!(!app.doc_answer(first, Ok(fetched("late", "late text", None, false))));
        assert_eq!(plain(&shown(&app).1.text), ["loading…"]);

        assert!(app.doc_answer(second, Ok(fetched("current", "now", None, false))));
        assert_eq!(plain(&shown(&app).1.text), ["now"]);

        app.apply(Action::Select(2));
        assert_eq!(
            app.doc_target(),
            None,
            "moving off the row leaves the document"
        );
        assert_eq!(app.follow_doc(), Some(None), "and withdraws its request");
        assert!(app.doc.is_none());
    }
}
