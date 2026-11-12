//! Navigation targets, locations, the back stack and the document open in the details panel.

use crate::app::{Action, App, Scroll};
use crate::ledger::StatusClass;
use crate::plan_state::Record;
use crate::scroll;
use crate::source::{DocRequest, Fetched};
use crate::surface::{CursorList, Surface, VisibleRow};
use crate::view::doc_view;

/// Anything glimpse can show; [`App::open`] is the only way to follow one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Target {
    Task(u32),
    Milestone(String),
    Decision(String),
    Contract(String),
    Handover(String),
    /// An index into the plan's run records.
    Run(usize),
    /// A listed path, such as `notes/surface-nav.md`.
    Authored(String),
    Source {
        path: String,
        line: Option<u32>,
        read_at: String,
        dirty: bool,
    },
    Url(String),
    Commit(String),
    Finding {
        store: String,
        id: String,
    },
    Backlog(String),
    Input(String),
}

impl Target {
    fn noun(&self) -> &'static str {
        match self {
            Self::Task(_) => "a task",
            Self::Milestone(_) => "a milestone",
            Self::Decision(_) => "a decision",
            Self::Contract(_) => "a contract",
            Self::Handover(_) => "a handover",
            Self::Run(_) => "a run record",
            Self::Authored(_) => "an authored file",
            Self::Source { .. } => "a source reference",
            Self::Url(_) => "a URL",
            Self::Commit(_) => "a commit",
            Self::Finding { .. } => "a finding",
            Self::Backlog(_) => "a backlog item",
            Self::Input(_) => "an input record",
        }
    }
}

/// Where the user was: a surface, that surface's cursor, and the document open over it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Location {
    pub(crate) surface: Surface,
    pub(crate) cursor: Cursor,
    pub(crate) doc: Option<Target>,
}

/// A source, commit or finding open in the details panel in place of the details of the
/// row it was opened over. Once the surface or its cursor moves, it is no longer shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OpenDoc {
    pub(crate) target: Target,
    over: Location,
    /// Rows the panel is scrolled down while it shows this document.
    pub(crate) scroll: u16,
}

/// The document last asked of the poller and what has come back for it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct DocLane {
    pub(crate) wanted: Option<DocRequest>,
    pub(crate) state: DocState,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) enum DocState {
    #[default]
    Loading,
    Text(Fetched),
    /// The one-line reason the read failed.
    Failed(String),
}

static LOADING: DocState = DocState::Loading;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Cursor {
    /// The selected task, on Tasks.
    Task(Option<u32>),
    /// The list cursor's id, on a list surface.
    Row(Option<String>),
}

impl App {
    /// Goes to `target`, first pushing the present location onto [`App::back`] when the
    /// move changes it. A target that resolves to nothing changes nothing but the footer.
    /// A URL is handed back as [`Action::Copy`]; a source, commit or finding opens as a
    /// document over the present row.
    pub(crate) fn open(&mut self, target: Target) -> Option<Action> {
        let here = self.location();
        let resolved = match &target {
            Target::Url(url) => {
                self.notify(format!("copied {url}"));
                return Some(Action::Copy(url.clone()));
            }
            Target::Source { .. } | Target::Commit(_) | Target::Finding { .. } => {
                self.show_doc(target.clone());
                true
            }
            row => {
                let shown = self.doc.take();
                let resolved = match row {
                    Target::Task(id) => self.open_task(*id),
                    Target::Backlog(id) => self.open_row(Surface::Backlog, id),
                    Target::Input(id) => self.open_row(Surface::Inbox, id),
                    _ => self.open_plan_row(row),
                };
                if !resolved {
                    self.doc = shown;
                } else if matches!(row, Target::Authored(_)) {
                    self.details_open = true;
                }
                resolved
            }
        };
        if resolved && self.location() != here {
            self.back.push(here);
        }
        None
    }

    /// Pops [`App::back`] and restores that surface, its cursor and the document open over
    /// it; nothing on an empty stack.
    pub(crate) fn go_back(&mut self) {
        let Some(to) = self.back.pop() else {
            return;
        };
        self.doc = None;
        self.apply(Action::SwitchSurface(to.surface));
        match to.cursor {
            Cursor::Task(id) => {
                if id != self.selected {
                    self.select(id);
                    self.follow = false;
                }
            }
            Cursor::Row(Some(id)) => self.edit_list(|list| list.select_row(&id)),
            Cursor::Row(None) => {}
        }
        if let Some(target) = to.doc {
            self.show_doc(target);
        }
    }

    /// Opens `target` as a document over the present row, with the details panel. On Tasks
    /// it pauses follow, as a click does, so the next snapshot does not move off the row.
    fn show_doc(&mut self, target: Target) {
        if self.surface == Surface::Tasks {
            self.follow = false;
        }
        self.doc = Some(OpenDoc {
            target,
            over: self.spot(),
            scroll: 0,
        });
        self.details_open = true;
    }

    fn location(&self) -> Location {
        Location {
            doc: self.open_doc().map(|doc| doc.target.clone()),
            ..self.spot()
        }
    }

    /// The present surface and cursor, with no document.
    fn spot(&self) -> Location {
        let cursor = match self.surface {
            Surface::Tasks => Cursor::Task(self.selected),
            _ => Cursor::Row(self.list().and_then(CursorList::cursor).map(str::to_owned)),
        };
        Location {
            surface: self.surface,
            cursor,
            doc: None,
        }
    }

    /// The document opened over the present row, if the cursor has not moved off it.
    pub(crate) fn open_doc(&self) -> Option<&OpenDoc> {
        let spot = self.spot();
        self.doc.as_ref().filter(|doc| doc.over == spot)
    }

    /// The document the details panel shows in place of the record's details: the one
    /// opened over the present row, else the authored file under the Plan cursor while the
    /// panel is open.
    pub(crate) fn doc_target(&self) -> Option<Target> {
        if let Some(doc) = self.open_doc() {
            return Some(doc.target.clone());
        }
        if !self.details_open || self.surface != Surface::Plan {
            return None;
        }
        match &self.plan.cursor_row()?.record {
            Record::Authored(path) => Some(Target::Authored(path.clone())),
            _ => None,
        }
    }

    /// What the poller is asked to read for `target`; `None` for a target with a row.
    fn doc_request(&self, target: &Target) -> Option<DocRequest> {
        Some(match target {
            Target::Authored(path) => DocRequest::Authored {
                slug: self.snapshot.slug.clone(),
                path: path.clone(),
            },
            Target::Source {
                path,
                line,
                read_at,
                dirty,
            } => DocRequest::Source {
                path: path.clone(),
                line: *line,
                read_at: read_at.clone(),
                dirty: *dirty,
            },
            Target::Commit(sha) => DocRequest::Commit { sha: sha.clone() },
            Target::Finding { store, id } => DocRequest::Finding {
                store: store.clone(),
                id: id.clone(),
            },
            _ => return None,
        })
    }

    /// The request the details panel wants answered; `None` while it is closed or shows
    /// no document.
    pub(crate) fn wanted_doc(&self) -> Option<DocRequest> {
        if !self.details_open {
            return None;
        }
        self.doc_target()
            .and_then(|target| self.doc_request(&target))
    }

    /// `target`'s state: loading until the answer to its own request has arrived.
    pub(crate) fn doc_state(&self, target: &Target) -> &DocState {
        match &self.lane.wanted {
            Some(wanted) if self.doc_request(target).as_ref() == Some(wanted) => &self.lane.state,
            _ => &LOADING,
        }
    }

    /// Drops a document the cursor has moved off, and returns the request to send when the
    /// wanted document changed, `Some(None)` withdrawing the last one. The old answer is
    /// dropped with it, so it is never drawn for the new request.
    pub(crate) fn follow_doc(&mut self) -> Option<Option<DocRequest>> {
        if self.doc.is_some() && self.open_doc().is_none() {
            self.doc = None;
        }
        let wanted = self.wanted_doc();
        if wanted == self.lane.wanted {
            return None;
        }
        self.lane = DocLane {
            wanted: wanted.clone(),
            state: DocState::Loading,
        };
        Some(wanted)
    }

    /// Takes the poller's answer to `request`, dropping one for anything but the request
    /// last sent; returns whether it was taken. A source opened at a line scrolls to it.
    pub(crate) fn doc_answer(
        &mut self,
        request: DocRequest,
        answer: Result<Fetched, String>,
    ) -> bool {
        if self.lane.wanted.as_ref() != Some(&request) {
            return false;
        }
        let jump = match (&answer, self.open_doc()) {
            (Ok(fetched), Some(doc))
                if self.doc_request(&doc.target).as_ref() == Some(&request) =>
            {
                fetched.line.map(|line| {
                    let width = self
                        .regions
                        .details
                        .map_or(u16::MAX, |rect| rect.width.saturating_sub(4));
                    doc_view::scroll_to(self, fetched, line, width, self.details_page)
                })
            }
            _ => None,
        };
        self.lane.state = match answer {
            Ok(fetched) => DocState::Text(fetched),
            Err(reason) => DocState::Failed(reason),
        };
        if let Some(scroll) = jump
            && let Some(doc) = self.doc.as_mut()
        {
            doc.scroll = scroll;
        }
        true
    }

    /// Scrolls the open document when the panel shows one; returns false when it does not.
    pub(crate) fn scroll_doc(&mut self, by: Scroll) -> bool {
        let (max, page) = (self.details_max_scroll, self.details_page);
        if self.open_doc().is_none() {
            return false;
        }
        if let Some(doc) = self.doc.as_mut() {
            doc.scroll = scroll::offset(doc.scroll, max, page, by);
        }
        true
    }

    /// Selects task `id` on Tasks, pausing follow as a click does.
    fn open_task(&mut self, id: u32) -> bool {
        if self.index.task(&self.snapshot, id).is_none() {
            self.notify(format!("no task {id} in this flow"));
            return false;
        }
        self.apply(Action::SwitchSurface(Surface::Tasks));
        self.select(Some(id));
        self.follow = false;
        true
    }

    /// Puts the Plan surface's cursor on the first row that leads to `target`.
    fn open_plan_row(&mut self, target: &Target) -> bool {
        let Some(id) = self
            .plan
            .rows
            .iter()
            .find(|row| row.target.as_ref() == Some(target))
            .map(|row| row.id.clone())
        else {
            self.notify(format!("{} is not in this flow's plan", target.noun()));
            return false;
        };
        self.apply(Action::SwitchSurface(Surface::Plan));
        self.edit_list(|list| list.select_row(&id));
        true
    }

    /// Puts `surface`'s cursor on row `id`, first revealing a row the closed toggle or
    /// the filter hides.
    fn open_row(&mut self, surface: Surface, id: &str) -> bool {
        let exists = match surface {
            Surface::Inbox => self.inbox.row(id).is_some(),
            _ => self
                .items
                .get(&surface)
                .is_some_and(|state| state.row(id).is_some()),
        };
        if !exists {
            let what = match surface {
                Surface::Inbox => "input record",
                _ => "backlog item",
            };
            self.notify(format!("no {what} {id}"));
            return false;
        }
        self.apply(Action::SwitchSurface(surface));
        if surface == Surface::Inbox {
            self.reveal_input(id);
        } else {
            self.reveal_item(id);
        }
        self.edit_list(|list| list.select_row(id));
        true
    }

    fn reveal_item(&mut self, id: &str) {
        let Some(state) = self.current_items() else {
            return;
        };
        if shows(&state.visible(), id) {
            return;
        }
        let closed = state
            .row(id)
            .is_some_and(|row| matches!(row.class, StatusClass::Done | StatusClass::Declined));
        let mut cleared = false;
        self.edit_items(|state| {
            if closed {
                state.show_closed = true;
            }
            if !shows(&state.visible(), id) {
                state.filter.clear();
                cleared = true;
            }
        });
        let how = match (closed, cleared) {
            (true, true) => "closed items shown and filter cleared",
            (true, false) => "closed items shown",
            _ => "filter cleared",
        };
        self.notify(format!("{how} to show {id}"));
    }

    fn reveal_input(&mut self, id: &str) {
        if shows(&self.inbox.visible(), id) {
            return;
        }
        self.edit_inbox(|inbox| inbox.show_closed = true);
        self.notify(format!("closed items shown to show {id}"));
    }
}

fn shows(visible: &[VisibleRow], id: &str) -> bool {
    visible
        .iter()
        .any(|row| matches!(row, VisibleRow::Item(shown) if shown == id))
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use tomlctl::LedgerRef;

    use super::*;
    use crate::authored::{AuthoredFile, AuthoredKind, AuthoredList};
    use crate::config::Config;
    use crate::keys;
    use crate::ledger::{InputRow, Inputs, ItemRow, Kind, Ledger};
    use crate::model::fixture;

    fn app() -> App {
        App::new(fixture(), &Config::default())
    }

    fn ledger(kind: Kind, source: LedgerRef, rows: &[(&str, &str)]) -> Ledger {
        let rows = rows
            .iter()
            .map(|&(id, status)| ItemRow {
                id: id.to_string(),
                status: status.to_string(),
                class: StatusClass::of(status),
                raw: serde_json::json!({"id": id, "status": status}),
                ..ItemRow::default()
            })
            .collect();
        Ledger {
            kind,
            source,
            revision: Some("v1".to_string()),
            rows,
        }
    }

    fn review_source() -> LedgerRef {
        LedgerRef::File("review.toml".into())
    }

    /// An app on the Review surface with its cursor on `R2`, and a backlog read.
    fn review_app() -> App {
        let mut app = app();
        let now = Instant::now();
        app.apply_ledger(
            ledger(
                Kind::Review,
                review_source(),
                &[("R1", "open"), ("R2", "open")],
            ),
            now,
        );
        app.apply_ledger(
            ledger(
                Kind::Backlog,
                LedgerRef::Backlog,
                &[("B-1", "open"), ("B-2", "resolved")],
            ),
            now,
        );
        app.apply(Action::SwitchSurface(Surface::Review));
        app.apply(Action::SelectItem("R2".to_string()));
        app
    }

    fn review_cursor(app: &App) -> Option<&str> {
        app.items[&Surface::Review].cursor.as_deref()
    }

    #[test]
    fn opening_a_task_from_a_ledger_surface_and_coming_back() {
        let mut app = review_app();
        assert_eq!(review_cursor(&app), Some("R2"));
        app.open(Target::Task(2));
        assert_eq!(app.surface, Surface::Tasks);
        assert_eq!(app.selected, Some(2));
        assert!(!app.follow, "an opened task stays selected");
        assert_eq!(app.back.len(), 1);

        app.apply(Action::GoBack);
        assert_eq!(app.surface, Surface::Review);
        assert_eq!(review_cursor(&app), Some("R2"));
        assert!(app.back.is_empty());

        app.apply(Action::GoBack);
        assert_eq!(app.surface, Surface::Review, "an empty stack does nothing");
    }

    #[test]
    fn back_restores_the_task_selected_before_the_open() {
        let mut app = app();
        app.open(Target::Task(2));
        app.open(Target::Backlog("missing".to_string()));
        app.open(Target::Task(7));
        assert_eq!(app.back.len(), 2);
        app.apply(Action::GoBack);
        assert_eq!(app.selected, Some(2));
        app.apply(Action::GoBack);
        assert_eq!(app.selected, Some(4), "the frontier the app started on");
    }

    #[test]
    fn opening_a_hidden_backlog_row_reveals_it() {
        let mut app = review_app();
        let backlog = &app.items[&Surface::Backlog];
        assert!(
            !shows(&backlog.visible(), "B-2"),
            "resolved rows start hidden"
        );

        app.open(Target::Backlog("B-2".to_string()));
        assert_eq!(app.surface, Surface::Backlog);
        let backlog = &app.items[&Surface::Backlog];
        assert!(backlog.show_closed);
        assert_eq!(backlog.cursor.as_deref(), Some("B-2"));
        assert!(app.live_notice(Instant::now()).is_some());
    }

    #[test]
    fn opening_a_filtered_out_row_clears_the_filter() {
        let mut app = review_app();
        app.items.get_mut(&Surface::Backlog).unwrap().filter = "zzz".to_string();
        app.open(Target::Backlog("B-1".to_string()));
        let backlog = &app.items[&Surface::Backlog];
        assert!(backlog.filter.is_empty());
        assert!(!backlog.show_closed, "an open row needs no closed toggle");
        assert_eq!(backlog.cursor.as_deref(), Some("B-1"));
    }

    #[test]
    fn opening_an_input_record_reveals_a_closed_one() {
        let mut app = app();
        let record = |id: &str, status: &str| InputRow {
            id: id.to_string(),
            kind: "note".to_string(),
            author: "user".to_string(),
            status: status.to_string(),
            ..InputRow::default()
        };
        app.apply_inputs(
            Inputs {
                path: ".claude/inputs.toml".to_string(),
                revision: Some("v1".to_string()),
                rows: vec![record("I1", "new"), record("I2", "handled")],
            },
            Instant::now(),
        );
        app.open(Target::Input("I2".to_string()));
        assert_eq!(app.surface, Surface::Inbox);
        assert!(app.inbox.show_closed);
        assert_eq!(app.inbox.cursor.as_deref(), Some("I2"));
        app.apply(Action::GoBack);
        assert_eq!(app.surface, Surface::Tasks);
    }

    #[test]
    fn an_unknown_id_changes_nothing_but_the_footer() {
        let mut app = review_app();
        for target in [
            Target::Task(99),
            Target::Backlog("B-9".to_string()),
            Target::Input("I9".to_string()),
            Target::Decision("D9".to_string()),
            Target::Run(99),
            Target::Authored("notes/a.md".to_string()),
        ] {
            app.notice = None;
            app.open(target.clone());
            assert_eq!(app.surface, Surface::Review, "{target:?}");
            assert_eq!(review_cursor(&app), Some("R2"), "{target:?}");
            assert!(app.back.is_empty(), "{target:?}");
            assert!(app.live_notice(Instant::now()).is_some(), "{target:?}");
        }
    }

    #[test]
    fn targets_without_a_row_open_as_documents_over_the_row_and_stack() {
        let mut app = review_app();
        for (n, target) in [
            Target::Source {
                path: "src/a.rs".to_string(),
                line: Some(3),
                read_at: "abc".to_string(),
                dirty: false,
            },
            Target::Commit("abc".to_string()),
            Target::Finding {
                store: "review".to_string(),
                id: "F1".to_string(),
            },
        ]
        .into_iter()
        .enumerate()
        {
            app.open(target.clone());
            assert_eq!(app.surface, Surface::Review, "{target:?}");
            assert_eq!(review_cursor(&app), Some("R2"), "{target:?}");
            assert!(app.details_open, "{target:?}");
            assert_eq!(app.doc_target(), Some(target.clone()));
            assert_eq!(app.back.len(), n + 1, "{target:?}");
        }
        app.apply(Action::GoBack);
        assert!(matches!(app.doc_target(), Some(Target::Commit(_))));
        app.apply(Action::GoBack);
        app.apply(Action::GoBack);
        assert_eq!(app.doc_target(), None, "the row's own details again");
    }

    #[test]
    fn a_url_is_copied_and_moves_nothing() {
        let mut app = review_app();
        let url = "https://example.com".to_string();
        assert_eq!(
            app.open(Target::Url(url.clone())),
            Some(Action::Copy(url.clone()))
        );
        assert_eq!(app.surface, Surface::Review);
        assert!(app.back.is_empty());
        assert_eq!(app.doc_target(), None);
        let notice = app.live_notice(Instant::now()).unwrap_or_default();
        assert_eq!(notice, format!("copied {url}"));
    }

    #[test]
    fn opening_a_decision_lands_on_its_plan_surface_row_and_back_returns() {
        let mut app = review_app();
        app.open(Target::Decision("D1".to_string()));
        assert_eq!(app.surface, Surface::Plan);
        assert_eq!(app.plan.cursor.as_deref(), Some("decisions:D1"));
        assert_eq!(app.back.len(), 1);

        app.apply(Action::GoBack);
        assert_eq!(app.surface, Surface::Review);
        assert_eq!(review_cursor(&app), Some("R2"));
        assert!(app.back.is_empty());
    }

    #[test]
    fn every_plan_target_lands_on_its_row() {
        let mut app = app();
        let file = |path: &str, kind| AuthoredFile {
            path: path.to_string(),
            kind,
            id: path.to_string(),
            ..AuthoredFile::default()
        };
        app.apply_authored(
            AuthoredList {
                files: vec![
                    file("brief.md", AuthoredKind::Brief),
                    file("notes/keys.md", AuthoredKind::Note),
                ],
            },
            1,
        );
        for (target, row) in [
            (Target::Milestone("B".to_string()), "milestones:B"),
            (Target::Contract("C1".to_string()), "contracts:C1"),
            (Target::Handover("H2".to_string()), "handovers:H2"),
            (Target::Run(2), "timeline:2"),
            (Target::Authored("brief.md".to_string()), "brief:brief.md"),
            (
                Target::Authored("notes/keys.md".to_string()),
                "notes:notes/keys.md",
            ),
        ] {
            app.open(target.clone());
            assert_eq!(app.surface, Surface::Plan, "{target:?}");
            assert_eq!(app.plan.cursor.as_deref(), Some(row), "{target:?}");
        }
        assert_eq!(app.back.len(), 6, "each move within the surface stacks");
    }

    #[test]
    fn a_flow_switch_empties_the_stack() {
        let mut app = review_app();
        app.open(Target::Task(2));
        assert_eq!(app.back.len(), 1);
        let mut other = fixture();
        other.slug = "other".to_string();
        app.apply_snapshot(other, Instant::now());
        assert!(app.back.is_empty());

        app.open(Target::Task(2));
        app.flows.push(crate::flows::FlowEntry {
            slug: "third".to_string(),
            status: "in-progress".to_string(),
            updated: String::new(),
            plan_path: String::new(),
            tasks_mtime: std::time::SystemTime::UNIX_EPOCH,
        });
        app.apply(Action::SwitchFlow("third".to_string()));
        assert!(app.back.is_empty());
    }

    #[test]
    fn backspace_goes_back_and_esc_keeps_its_chain() {
        let press = |code| KeyEvent::new(code, KeyModifiers::NONE);
        assert_eq!(keys::map(press(KeyCode::Backspace)), Some(Action::GoBack));
        assert_eq!(keys::map(press(KeyCode::Esc)), Some(Action::Back));

        let mut app = review_app();
        app.open(Target::Task(2));
        app.details_open = true;
        app.apply(Action::Back);
        assert!(!app.details_open, "Esc closes the panel first");
        assert_eq!(app.surface, Surface::Tasks, "Esc never pops the stack");
        assert_eq!(app.back.len(), 1);
        assert_eq!(app.apply(Action::Back), Some(Action::Quit));
        assert_eq!(app.back.len(), 1);
    }
}
