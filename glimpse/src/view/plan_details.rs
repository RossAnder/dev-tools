//! The details of a Plan row: the record it stands for, shown whole, with every
//! record it relates to as a link, in the order the text mentions them.
//!
//! An authored file's own text arrives through the document lane, not here; its row
//! shows what the listing holds, which is its kind, headline and sources.

use ratatui::style::Style;
use ratatui::text::{Line, Span, Text};

use crate::app::App;
use crate::authored::{AuthoredFile, AuthoredKind, AuthoredSource};
use crate::model::plan::{Contract, Decision, GateRound, Handover, Health, HealthSite, Probe, Run};
use crate::nav::Target;
use crate::plan_state::{PlanRow, Record};
use crate::rollup::{self, Tokens};
use crate::theme::Theme;
use crate::view::details::{INDENT, indented, section};
use crate::view::doc::{Doc, Link};
use crate::view::{self, markdown};

/// The details of `row`, a row of the Plan surface.
pub(crate) fn content(app: &App, row: &PlanRow) -> Doc {
    let mut b = Builder::new(app);
    let plan = app.snapshot.plan.as_ref();
    match &row.record {
        Record::Decision(id) => match plan.and_then(|p| p.decisions.iter().find(|d| &d.id == id)) {
            Some(decision) => b.decision(decision),
            None => b.missing(&row.id),
        },
        Record::Contract(id) => match plan.and_then(|p| p.contracts.iter().find(|c| &c.id == id)) {
            Some(contract) => b.contract(contract),
            None => b.missing(&row.id),
        },
        Record::Handover(id) => match plan.and_then(|p| p.handovers.iter().find(|h| &h.id == id)) {
            Some(handover) => b.handover(handover),
            None => b.missing(&row.id),
        },
        Record::Milestone(id) => b.milestone(id),
        Record::Run(pos) => match plan.and_then(|p| p.runs.get(*pos)) {
            Some(run) => b.run(run),
            None => b.missing(&row.id),
        },
        Record::Authored(path) => match app.authored.by_path(path) {
            Some(file) => b.authored(file),
            None => b.missing(path),
        },
        Record::Change(pos) => b.change(*pos, &row.id),
        Record::Departure { attempt, deviation } => b.departure(*attempt, *deviation, &row.id),
        Record::Cost(cost) => b.cost(row, cost),
        Record::Health(health) => b.health(health, &row.id),
    }
    b.finish()
}

/// The `notes/<id>.md` paths `evidence` names, each once, in the order it names them.
fn cited_notes(evidence: &str) -> Vec<String> {
    const LEAD: &str = "notes/";
    let mut out: Vec<String> = Vec::new();
    let mut rest = evidence;
    while let Some(at) = rest.find(LEAD) {
        let tail = &rest[at + LEAD.len()..];
        let id_len = tail
            .find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')))
            .unwrap_or(tail.len());
        let id = tail[..id_len].trim_end_matches('.');
        if let Some(stem) = id.strip_suffix(".md").filter(|stem| !stem.is_empty()) {
            let path = format!("{LEAD}{stem}.md");
            if !out.contains(&path) {
                out.push(path);
            }
        }
        rest = &tail[id_len..];
    }
    out
}

/// A source reference as a target: a path, with its line when it has one, or a URL.
fn source_target(source: &AuthoredSource) -> Option<Target> {
    if !source.path.is_empty() {
        Some(Target::Source {
            path: source.path.clone(),
            line: (source.line > 0).then_some(source.line),
            read_at: source.read_at.clone(),
            dirty: source.dirty,
        })
    } else if !source.url.is_empty() {
        Some(Target::Url(source.url.clone()))
    } else {
        None
    }
}

fn source_label(source: &AuthoredSource) -> String {
    match (source.path.is_empty(), source.line) {
        (true, _) => source.url.clone(),
        (false, 0) => source.path.clone(),
        (false, line) => format!("{}:{line}", source.path),
    }
}

fn path_target(path: &str) -> Target {
    Target::Source {
        path: path.to_string(),
        line: None,
        read_at: String::new(),
        dirty: false,
    }
}

fn kind_name(kind: AuthoredKind) -> &'static str {
    match kind {
        AuthoredKind::Brief => "brief",
        AuthoredKind::Design => "design",
        AuthoredKind::Note => "note",
        AuthoredKind::Challenge => "challenge",
        AuthoredKind::ColdRead => "cold read",
        AuthoredKind::Backlog => "backlog",
        AuthoredKind::Settle => "settle",
        AuthoredKind::Unknown => "other",
        AuthoredKind::RunNote => "run note",
    }
}

fn yes_no(flag: bool) -> &'static str {
    if flag { "yes" } else { "no" }
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

fn count(n: u64) -> String {
    if n < 1000 {
        n.to_string()
    } else if n < 1_000_000 {
        let tenths = (n + 50) / 100;
        format!("{}.{}k", tenths / 10, tenths % 10)
    } else {
        let tenths = (n + 50_000) / 100_000;
        format!("{}.{}M", tenths / 10, tenths % 10)
    }
}

/// The text and links of one record, built top to bottom.
struct Builder<'a> {
    app: &'a App,
    theme: &'a Theme,
    out: Vec<Line<'static>>,
    links: Vec<Link>,
}

impl<'a> Builder<'a> {
    fn new(app: &'a App) -> Self {
        Self {
            app,
            theme: &app.theme,
            out: Vec::new(),
            links: Vec::new(),
        }
    }

    fn finish(self) -> Doc {
        Doc {
            text: Text::from(self.out),
            links: self.links,
        }
    }

    fn title(&mut self, text: String) {
        self.out.push(Line::styled(text, self.theme.slug));
    }

    fn missing(&mut self, what: &str) {
        self.title(what.to_string());
        self.out
            .push(Line::styled("no longer in the store", self.theme.warning));
    }

    /// One `label value` line; nothing when `value` is empty.
    fn fact(&mut self, label: &str, value: &str, style: Style) {
        if value.is_empty() {
            return;
        }
        self.out.push(Line::from(vec![
            Span::styled(format!("{label} "), self.theme.secondary),
            Span::styled(value.to_string(), style),
        ]));
    }

    /// A section rendering `body` as markdown; nothing when it is blank.
    fn body(&mut self, label: &str, body: &str) {
        if body.trim().is_empty() {
            return;
        }
        section(&mut self.out, label, self.theme);
        self.out
            .extend(indented(markdown::to_text(body, self.theme)));
    }

    /// A section holding one command, drawn as code; nothing when it is empty.
    fn command(&mut self, label: &str, command: &str) {
        if command.is_empty() {
            return;
        }
        section(&mut self.out, label, self.theme);
        self.out.push(Line::from(vec![
            Span::raw(INDENT),
            Span::styled(command.to_string(), self.theme.inline_code),
        ]));
    }

    fn link(&mut self, label: String, target: Target) {
        if !self.links.iter().any(|link| link.target == target) {
            self.links.push(Link { label, target });
        }
    }

    /// A line naming task `id` by its mark and title, and its link when the store holds it.
    fn task(&mut self, id: u32) {
        let app = self.app;
        match app.index.task(&app.snapshot, id) {
            Some(task) => {
                let (glyph, style) = view::status_mark(app, task);
                self.out.push(Line::from(vec![
                    Span::raw(INDENT),
                    Span::styled(format!("{glyph} {}", task.id), style),
                    Span::raw(format!(" {}", task.title)),
                ]));
                self.link(
                    format!("#{} {}", task.id, task.title),
                    Target::Task(task.id),
                );
            }
            None => self.out.push(Line::styled(
                format!("{INDENT}? {id} (not in the store)"),
                self.theme.warning,
            )),
        }
    }

    fn tasks(&mut self, label: &str, ids: &[u32]) {
        if ids.is_empty() {
            return;
        }
        section(&mut self.out, label, self.theme);
        for id in ids {
            self.task(*id);
        }
    }

    /// A verdict's style: `good` reads as done, none recorded as a warning, any other as failed.
    fn verdict_style(&self, verdict: &str, good: &str) -> Style {
        if verdict == good {
            self.theme.done
        } else if verdict.is_empty() {
            self.theme.warning
        } else {
            self.theme.failed
        }
    }

    fn decision(&mut self, decision: &Decision) {
        let theme = self.theme;
        self.title(decision.id.clone());
        self.fact("provenance", &decision.provenance, theme.facet);
        self.fact("door", &decision.door, theme.facet);
        self.fact("surfaced", yes_no(decision.surfaced), theme.badge);
        let vetoed = if decision.vetoed {
            theme.warning
        } else {
            theme.badge
        };
        self.fact("vetoed", yes_no(decision.vetoed), vetoed);
        self.body("Decision", &decision.decision);
        self.body("Why", &decision.why);
        self.body("Evidence", &decision.evidence);
        for note in cited_notes(&decision.evidence) {
            let label = match self.app.authored.by_path(&note) {
                Some(file) if !file.headline.is_empty() => format!("{note}  {}", file.headline),
                _ => note.clone(),
            };
            self.link(label, Target::Authored(note));
        }
        self.body("If false", &decision.if_false);
        self.body("Revisit if", &decision.revisit_if);
        self.command("Check", &decision.check);
        if !decision.rejected.is_empty() {
            section(&mut self.out, "Rejected", theme);
            for rejected in &decision.rejected {
                let mut text = format!("{INDENT}• {}", rejected.option);
                if !rejected.because.is_empty() {
                    text.push_str(&format!(" — {}", rejected.because));
                }
                self.out.push(Line::raw(text));
            }
        }
        if !decision.binds.is_empty() {
            section(&mut self.out, "Binds", theme);
            for entry in &decision.binds {
                match entry.parse::<u32>() {
                    Ok(id) if entry.bytes().all(|b| b.is_ascii_digit()) => self.task(id),
                    _ => {
                        self.out
                            .push(Line::styled(format!("{INDENT}{entry}"), theme.secondary));
                        self.link(entry.clone(), path_target(entry));
                    }
                }
            }
        }
    }

    fn contract(&mut self, contract: &Contract) {
        let theme = self.theme;
        self.title(contract.id.clone());
        self.fact("kind", &contract.kind, theme.facet);
        self.body("Claim", &contract.claim);
        self.command("Check", &contract.check);
        if !contract.provides.is_empty() || !contract.consumes.is_empty() {
            self.out.push(Line::default());
            self.fact("provides", &contract.provides, theme.badge);
            self.fact("consumes", &contract.consumes, theme.badge);
        }
        self.tasks("Tasks", &contract.tasks);
    }

    fn handover(&mut self, handover: &Handover) {
        let theme = self.theme;
        self.title(handover.id.clone());
        self.fact("reason", &handover.reason, theme.facet);
        let (state, style) = if handover.done {
            ("done", theme.done)
        } else {
            ("open", theme.warning)
        };
        self.fact("state", state, style);
        self.body("Action", &handover.action);
        self.body("When", &handover.when);
        self.body("Done when", &handover.done_when);
        self.tasks("Source task", &[handover.source_task]);
    }

    fn milestone(&mut self, id: &str) {
        let app = self.app;
        let theme = self.theme;
        let snapshot = &app.snapshot;
        self.title(format!("milestone {id}"));
        let milestone = snapshot.milestones.iter().find(|m| m.id == id);
        if let Some(milestone) = milestone {
            self.fact("elaboration", &milestone.elaboration, theme.facet);
            self.fact("defer reason", &milestone.defer_reason, theme.warning);
        }
        let goal = snapshot
            .checkpoints
            .iter()
            .find(|c| c.id == id)
            .map(|c| c.rationale.as_str())
            .unwrap_or_default();
        self.body("Goal", goal);

        if let Some(milestone) = milestone {
            let mut gates: Vec<&str> = milestone.gates.iter().map(String::as_str).collect();
            for probe in &milestone.gate_probes {
                if !gates.contains(&probe.gate.as_str()) {
                    gates.push(&probe.gate);
                }
            }
            if !gates.is_empty() {
                section(&mut self.out, "Gates", theme);
            }
            for gate in gates {
                self.out.push(Line::from(vec![
                    Span::raw(format!("{INDENT}• ")),
                    Span::styled(gate.to_string(), theme.inline_code),
                ]));
                for probe in milestone.gate_probes.iter().filter(|p| p.gate == gate) {
                    let verdict = if probe.verdict.is_empty() {
                        "unprobed"
                    } else {
                        probe.verdict.as_str()
                    };
                    let mut spans = vec![
                        Span::styled(format!("{INDENT}{INDENT}probe "), theme.secondary),
                        Span::styled(probe.probe.clone(), theme.inline_code),
                        Span::raw("  "),
                        Span::styled(
                            verdict.to_string(),
                            self.verdict_style(&probe.verdict, "holds"),
                        ),
                    ];
                    if !probe.at.is_empty() {
                        spans.push(Span::styled(format!(" at {}", probe.at), theme.secondary));
                    }
                    self.out.push(Line::from(spans));
                }
            }
        }

        let members: Vec<u32> = snapshot
            .tasks
            .iter()
            .filter(|t| t.checkpoint == id)
            .map(|t| t.id)
            .collect();
        self.tasks("Tasks", &members);

        let rounds = app.index.plan().gate_rounds_for(snapshot, id);
        if !rounds.is_empty() {
            section(&mut self.out, "Gate rounds", theme);
            for round in rounds {
                self.gate_round(round);
            }
        }
    }

    fn gate_round(&mut self, round: &GateRound) {
        let theme = self.theme;
        let kind = if round.round == 0 {
            "first gate".to_string()
        } else {
            format!("fix round {}", round.round)
        };
        let mut head = vec![
            Span::raw(INDENT),
            Span::styled(
                format!("round {} run {}", round.round, round.seq),
                theme.badge,
            ),
            Span::styled(format!(" ({kind})  "), theme.secondary),
            Span::styled(
                round.outcome.clone(),
                self.verdict_style(&round.outcome, "pass"),
            ),
        ];
        if !round.at.is_empty() {
            head.push(Span::styled(format!("  {}", round.at), theme.secondary));
        }
        self.out.push(Line::from(head));
        let deep = format!("{INDENT}{INDENT}{INDENT}");
        for command in &round.commands {
            let style = match command.outcome.as_str() {
                "pass" => theme.done,
                "flaky" => theme.warning,
                _ => theme.failed,
            };
            let mut spans = vec![
                Span::raw(format!("{INDENT}{INDENT}")),
                Span::styled(command.outcome.clone(), style),
                Span::raw(" "),
                Span::styled(command.command.clone(), theme.inline_code),
                Span::styled(
                    format!("  {}", duration(command.duration_s)),
                    theme.secondary,
                ),
            ];
            if let Some(exit) = command.exit {
                spans.push(Span::styled(format!("  exit {exit}"), theme.secondary));
            }
            self.out.push(Line::from(spans));
            if !command.summary.is_empty() {
                self.out
                    .push(Line::raw(format!("{deep}{}", command.summary)));
            }
            if !command.failed_ids.is_empty() {
                self.out.push(Line::styled(
                    format!("{deep}failed: {}", command.failed_ids.join(", ")),
                    theme.failed,
                ));
            }
            if !command.log.is_empty() {
                self.out.push(Line::styled(
                    format!("{deep}log {}", command.log),
                    theme.secondary,
                ));
                self.link(format!("log {}", command.log), path_target(&command.log));
            }
        }
    }

    fn run(&mut self, run: &Run) {
        let theme = self.theme;
        self.title(if run.step.is_empty() {
            "run".to_string()
        } else {
            run.step.clone()
        });
        self.fact("workflow", &run.workflow, theme.facet);
        self.fact("run id", &run.run_id, theme.secondary);
        self.fact("session", &run.session, theme.secondary);
        self.fact("started", &run.started, theme.secondary);
        let finished = if run.finished.is_empty() && run.skip_reason.is_empty() {
            "open"
        } else {
            run.finished.as_str()
        };
        self.fact("finished", finished, theme.secondary);
        self.fact(
            "outcome",
            &run.outcome,
            self.verdict_style(&run.outcome, "ok"),
        );
        self.body("Skip reason", &run.skip_reason);
        self.body("Reason", &run.reason);
        self.tasks("Dispatched", &run.tasks);
    }

    fn authored(&mut self, file: &AuthoredFile) {
        let theme = self.theme;
        self.title(file.path.clone());
        self.fact("kind", kind_name(file.kind), theme.facet);
        self.fact("state", &file.state, theme.badge);
        self.fact("sources", &file.sources.len().to_string(), theme.badge);
        self.body("Headline", &file.headline);
        if !file.sources.is_empty() {
            section(&mut self.out, "Sources", theme);
            for source in &file.sources {
                let label = source_label(source);
                let mut spans = vec![
                    Span::raw(INDENT),
                    Span::styled(label.clone(), theme.secondary),
                ];
                if source.dirty {
                    spans.push(Span::styled("  dirty", theme.warning));
                }
                if !source.claim.is_empty() {
                    spans.push(Span::raw(format!("  {}", source.claim)));
                }
                self.out.push(Line::from(spans));
                if let Some(target) = source_target(source) {
                    self.link(label, target);
                }
            }
        }
        if file.kind == AuthoredKind::RunNote {
            if file.task > 0 {
                self.tasks("Task", &[file.task]);
            }
            return;
        }
        let app = self.app;
        let citing: Vec<&Decision> = app
            .snapshot
            .plan
            .iter()
            .flat_map(|plan| &plan.decisions)
            .filter(|d| cited_notes(&d.evidence).contains(&file.path))
            .collect();
        if !citing.is_empty() {
            section(&mut self.out, "Cited by", theme);
            for decision in citing {
                let first = decision.decision.lines().next().unwrap_or_default();
                self.out.push(Line::from(vec![
                    Span::raw(INDENT),
                    Span::styled(decision.id.clone(), theme.badge),
                    Span::raw(format!(" {first}")),
                ]));
                self.link(
                    format!("{} {first}", decision.id),
                    Target::Decision(decision.id.clone()),
                );
            }
        }
    }

    fn change(&mut self, pos: usize, row_id: &str) {
        let theme = self.theme;
        let app = self.app;
        let Some(change) = app.snapshot.changes.get(pos) else {
            return self.missing(row_id);
        };
        self.title(format!("change to {}", or(&change.field, "a field")));
        self.fact("at", or(&change.at, "undated").as_str(), theme.secondary);
        self.fact("ref", &change.task_ref, theme.facet);
        self.fact("field", &change.field, theme.badge);
        self.fact("actor", &change.actor, theme.facet);
        self.body("Old", or(&change.old, "(empty)").as_str());
        self.body("New", or(&change.new, "(empty)").as_str());
        self.body("Reason", &change.reason);
        let task = app
            .snapshot
            .tasks
            .iter()
            .find(|t| !change.task_ref.is_empty() && t.task_ref == change.task_ref)
            .map(|t| t.id);
        if let Some(id) = task {
            self.tasks("Task", &[id]);
        }
    }

    fn departure(&mut self, attempt: usize, deviation: usize, row_id: &str) {
        let theme = self.theme;
        let app = self.app;
        let found = app
            .snapshot
            .plan
            .as_ref()
            .and_then(|plan| plan.attempts.get(attempt))
            .and_then(|a| a.deviations.get(deviation).map(|d| (a, d)));
        let Some((attempt, deviation)) = found else {
            return self.missing(row_id);
        };
        self.title(format!("departure in task {}", attempt.task));
        self.fact("run", &attempt.run_id, theme.secondary);
        self.fact("milestone", &attempt.milestone, theme.checkpoint);
        self.body("What", &deviation.what);
        self.body("Why", &deviation.why);
        self.body("Means departed from", &deviation.means);
        self.tasks("Task", &[attempt.task]);
    }

    fn cost(&mut self, row: &PlanRow, cost: &rollup::Row) {
        let theme = self.theme;
        let name = row.cells.first().cloned().unwrap_or_default();
        self.title(format!("cost: {name}"));
        self.fact("time", &duration(cost.seconds), theme.badge);
        if cost.gate_seconds > 0 {
            self.fact("gate time", &duration(cost.gate_seconds), theme.badge);
        }
        let usage = &cost.usage;
        let total = usage.input + usage.cache_write + usage.cache_read + usage.output;
        let tokens = match cost.tokens {
            Tokens::Recorded => count(total),
            Tokens::Partial => format!("{} partial", count(total)),
            Tokens::NotRecorded => "not recorded".to_string(),
        };
        let style = if cost.tokens == Tokens::Recorded {
            theme.badge
        } else {
            theme.warning
        };
        self.fact("tokens", &tokens, style);
        if cost.agents.is_empty() {
            return;
        }
        section(&mut self.out, "Agents", theme);
        for share in &cost.agents {
            let u = &share.usage;
            let tokens = if share.recorded {
                format!(
                    "{} tokens (in {}, cache write {}, cache read {}, out {})",
                    count(u.input + u.cache_write + u.cache_read + u.output),
                    count(u.input),
                    count(u.cache_write),
                    count(u.cache_read),
                    count(u.output)
                )
            } else {
                "tokens not recorded".to_string()
            };
            self.out.push(Line::from(vec![
                Span::raw(INDENT),
                view::chip(&share.agent_type, theme.agent_chip),
                Span::raw(format!(" {}  {}", share.agent_id, duration(share.seconds))),
            ]));
            self.out.push(Line::styled(
                format!("{INDENT}{INDENT}{tokens}"),
                theme.secondary,
            ));
        }
    }

    fn health(&mut self, health: &Health, row_id: &str) {
        let theme = self.theme;
        let app = self.app;
        let snapshot = &app.snapshot;
        let spec_of = |task: u32| {
            snapshot
                .tasks
                .iter()
                .find(|t| t.id == task)
                .and_then(|t| t.spec.as_ref())
        };
        match &health.site {
            HealthSite::Accept { task, index } => {
                let Some(accept) = spec_of(*task).and_then(|s| s.accept.get(*index)) else {
                    return self.missing(row_id);
                };
                self.title(format!("accept check {} of task {task}", index + 1));
                self.body("Behaviour", &accept.behaviour);
                self.command("Check", &accept.check);
                self.probe(accept.probe.as_ref(), accept.probeable, "healthy");
                self.tasks("Task", &[*task]);
            }
            HealthSite::Premise { task, index } => {
                let Some(premise) = spec_of(*task).and_then(|s| s.premises.get(*index)) else {
                    return self.missing(row_id);
                };
                self.title(format!("premise {} of task {task}", index + 1));
                self.body("Claim", &premise.claim);
                self.command("Check", &premise.check);
                self.fact("when", &premise.at, theme.facet);
                self.probe(premise.probe.as_ref(), premise.probeable, "holds");
                self.tasks("Task", &[*task]);
            }
            HealthSite::GateProbe { milestone, index } => {
                let probe = snapshot
                    .milestones
                    .iter()
                    .find(|m| &m.id == milestone)
                    .and_then(|m| m.gate_probes.get(*index));
                let Some(probe) = probe else {
                    return self.missing(row_id);
                };
                self.title(format!("gate probe of milestone {milestone}"));
                self.command("Gate", &probe.gate);
                self.command("Probe command", &probe.probe);
                section(&mut self.out, "Probe", theme);
                let verdict = or(&probe.verdict, "unprobed");
                self.fact(
                    "verdict",
                    &verdict,
                    self.verdict_style(&probe.verdict, "holds"),
                );
                self.fact("at", &probe.at, theme.commit);
                section(&mut self.out, "Milestone", theme);
                self.out.push(Line::styled(
                    format!("{INDENT}{milestone}"),
                    theme.checkpoint,
                ));
                self.link(
                    format!("milestone {milestone}"),
                    Target::Milestone(milestone.clone()),
                );
            }
        }
    }

    /// A probe section: the verdict, both controls' exit codes and the commit it ran at.
    fn probe(&mut self, probe: Option<&Probe>, probeable: bool, good: &str) {
        let theme = self.theme;
        section(&mut self.out, "Probe", theme);
        let Some(probe) = probe else {
            let text = if probeable { "unprobed" } else { "not probed" };
            let style = if probeable {
                theme.warning
            } else {
                theme.secondary
            };
            self.out.push(Line::styled(text, style));
            return;
        };
        let verdict = or(&probe.verdict, "unprobed");
        self.fact(
            "verdict",
            &verdict,
            self.verdict_style(&probe.verdict, good),
        );
        let rc = |rc: Option<i64>| rc.map_or_else(|| "not run".to_string(), |rc| rc.to_string());
        self.fact("negative exit", &rc(probe.neg_rc), theme.secondary);
        if probe.pos_rc.is_some() {
            self.fact("positive exit", &rc(probe.pos_rc), theme.secondary);
        }
        self.fact("at", &probe.at, theme.commit);
    }
}

fn or(value: &str, fallback: &str) -> String {
    if value.is_empty() {
        fallback.to_string()
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;
    use crate::config::Config;
    use crate::model::fixture;
    use crate::surface::Surface;
    use crate::view::details;

    fn file(path: &str, kind: AuthoredKind, task: u32) -> AuthoredFile {
        AuthoredFile {
            path: path.to_string(),
            kind,
            id: path.to_string(),
            headline: format!("{path} headline"),
            task,
            ..AuthoredFile::default()
        }
    }

    fn app_with(snapshot: crate::model::Snapshot) -> App {
        let mut app = App::new(snapshot, &Config::default());
        let mut note = file("notes/keys.md", AuthoredKind::Note, 0);
        note.sources = vec![
            AuthoredSource {
                path: "src/keys.rs".to_string(),
                line: 12,
                claim: "the map is fixed".to_string(),
                ..AuthoredSource::default()
            },
            AuthoredSource {
                url: "https://example.com/keys".to_string(),
                ..AuthoredSource::default()
            },
        ];
        app.authored.files = vec![note, file("notes/run/task-3.md", AuthoredKind::RunNote, 3)];
        app.rebuild_plan(Instant::now());
        app
    }

    fn app() -> App {
        let mut snap = fixture();
        snap.plan.as_mut().expect("plan").decisions[0].evidence =
            "Read src/config.rs; notes/keys.md finding 2 and notes/keys.md again.".to_string();
        app_with(snap)
    }

    fn doc(app: &App, id: &str) -> Doc {
        let row = app.plan.row(id).unwrap_or_else(|| panic!("no row {id}"));
        content(app, row)
    }

    fn plain(doc: &Doc) -> Vec<String> {
        doc.text
            .lines
            .iter()
            .map(|line| line.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect()
    }

    fn targets(doc: &Doc) -> Vec<Target> {
        doc.links.iter().map(|link| link.target.clone()).collect()
    }

    fn after(lines: &[String], label: &str) -> Vec<String> {
        let at = lines
            .iter()
            .position(|l| l == label)
            .unwrap_or_else(|| panic!("{label}: {lines:?}"));
        lines[at + 1..]
            .iter()
            .take_while(|l| !l.is_empty())
            .cloned()
            .collect()
    }

    fn source(path: &str) -> Target {
        path_target(path)
    }

    #[test]
    fn plan_details_decision_shows_its_record_and_links_its_notes_then_binds() {
        let app = app();
        let doc = doc(&app, "decisions:D1");
        let lines = plain(&doc);
        assert_eq!(lines[0], "D1");
        for fact in [
            "provenance agent-assumed",
            "door two-way",
            "surfaced no",
            "vetoed no",
        ] {
            assert!(lines.contains(&fact.to_string()), "{fact}: {lines:?}");
        }
        assert_eq!(
            after(&lines, "Decision"),
            ["  Read the key names from the config."]
        );
        assert_eq!(
            after(&lines, "Why"),
            ["  Users rebind keys without a rebuild."]
        );
        assert_eq!(after(&lines, "If false"), ["  keys.rs owns the names."]);
        assert_eq!(
            after(&lines, "Rejected"),
            ["  • Hard-code the key names — a rebind would need a rebuild"]
        );
        let binds = after(&lines, "Binds");
        assert!(binds[0].ends_with("6 Bind the keys"), "{binds:?}");
        assert_eq!(binds[1], "  src/render.rs");
        assert_eq!(
            targets(&doc),
            [
                Target::Authored("notes/keys.md".to_string()),
                Target::Task(6),
                source("src/render.rs"),
            ]
        );
        assert_eq!(doc.links[0].label, "notes/keys.md  notes/keys.md headline");
    }

    #[test]
    fn plan_details_contract_and_handover_link_their_tasks() {
        let app = app();
        let doc = doc(&app, "contracts:C1");
        let lines = plain(&doc);
        assert_eq!(lines[0], "C1");
        assert!(lines.contains(&"kind interface".to_string()));
        assert_eq!(after(&lines, "Check"), ["  grep -q \"keys\" src/config.rs"]);
        assert!(lines.contains(&"provides config".to_string()));
        assert!(lines.contains(&"consumes key map".to_string()));
        assert_eq!(after(&lines, "Tasks").len(), 2);
        assert_eq!(targets(&doc), [Target::Task(3), Target::Task(6)]);

        let doc = self::doc(&app, "handovers:H2");
        let lines = plain(&doc);
        assert_eq!(lines[0], "H2");
        assert!(lines.contains(&"reason permissions".to_string()));
        assert!(lines.contains(&"state done".to_string()));
        assert_eq!(
            after(&lines, "Done when"),
            ["  The loader writes without an error."]
        );
        assert!(after(&lines, "Source task")[0].ends_with("2 Define the schema"));
        assert_eq!(targets(&doc), [Target::Task(2)]);
        assert!(plain(&self::doc(&app, "handovers:H1")).contains(&"state open".to_string()));
    }

    #[test]
    fn plan_details_milestone_lists_every_gate_run_with_its_failed_round() {
        let app = app();
        let doc = doc(&app, "milestones:A");
        let lines = plain(&doc);
        assert_eq!(lines[0], "milestone A");
        assert!(lines.contains(&"elaboration upfront".to_string()));
        assert_eq!(after(&lines, "Goal"), ["  the foundation every view reads"]);
        assert_eq!(
            after(&lines, "Gates"),
            [
                "  • cargo test",
                "    probe cargo --version  holds at a1b2c3d"
            ]
        );
        assert_eq!(after(&lines, "Tasks").len(), 3);
        assert_eq!(
            after(&lines, "Gate rounds"),
            [
                "  round 0 run 1 (first gate)  fail  2026-09-28T10:20:00Z",
                "    pass cargo clippy  31s  exit 0",
                "      clean",
                "    fail cargo test  42s  exit 101",
                "      40 passed; 1 failed",
                "      failed: config::tests::unknown_key",
                "      log .claude/flows/demo-flow/gates/A-r0-s1-c2.log",
                "  round 1 run 1 (fix round 1)  pass  2026-09-28T10:40:00Z",
                "    pass cargo test  40s  exit 0",
                "      41 passed",
            ]
        );
        assert_eq!(
            targets(&doc),
            [
                Target::Task(1),
                Target::Task(2),
                Target::Task(3),
                source(".claude/flows/demo-flow/gates/A-r0-s1-c2.log"),
            ]
        );

        let lines = plain(&self::doc(&app, "milestones:B"));
        assert!(
            lines.contains(&"    probe cargo clippy --version  unprobed".to_string()),
            "{lines:?}"
        );
        assert!(
            lines.contains(&"    fail cargo test  55s".to_string()),
            "{lines:?}"
        );
    }

    #[test]
    fn plan_details_run_lists_its_facts_and_dispatched_tasks() {
        let app = app();
        let doc = doc(&app, "timeline:2");
        let lines = plain(&doc);
        assert_eq!(lines[0], "implement:B");
        for fact in [
            "workflow implement-v2-milestone",
            "run id wf_3",
            "started 2026-09-28T11:00:00Z",
            "finished 2026-09-28T11:58:00Z",
            "outcome stopped",
        ] {
            assert!(lines.contains(&fact.to_string()), "{fact}: {lines:?}");
        }
        assert_eq!(after(&lines, "Dispatched").len(), 2);
        assert_eq!(targets(&doc), [Target::Task(4), Target::Task(8)]);
    }

    #[test]
    fn plan_details_note_links_its_sources_then_the_decisions_citing_it() {
        let app = app();
        let doc = doc(&app, "notes:notes/keys.md");
        let lines = plain(&doc);
        assert_eq!(lines[0], "notes/keys.md");
        assert!(lines.contains(&"kind note".to_string()));
        assert!(lines.contains(&"sources 2".to_string()));
        assert_eq!(
            after(&lines, "Sources"),
            [
                "  src/keys.rs:12  the map is fixed",
                "  https://example.com/keys"
            ]
        );
        assert_eq!(
            after(&lines, "Cited by"),
            ["  D1 Read the key names from the config."]
        );
        assert_eq!(
            targets(&doc),
            [
                Target::Source {
                    path: "src/keys.rs".to_string(),
                    line: Some(12),
                    read_at: String::new(),
                    dirty: false,
                },
                Target::Url("https://example.com/keys".to_string()),
                Target::Decision("D1".to_string()),
            ]
        );
    }

    #[test]
    fn plan_details_run_note_links_its_task_and_no_decision() {
        let app = app();
        let doc = doc(&app, "notes:notes/run/task-3.md");
        let lines = plain(&doc);
        assert!(lines.contains(&"kind run note".to_string()));
        assert!(lines.contains(&"sources 0".to_string()));
        assert_eq!(targets(&doc), [Target::Task(3)]);
    }

    #[test]
    fn plan_details_change_and_departure_link_the_task_they_name() {
        let app = app();
        let doc = doc(&app, "changes:0");
        let lines = plain(&doc);
        assert_eq!(lines[0], "change to detail");
        assert!(lines.contains(&"at 2026-09-28T09:15:00Z".to_string()));
        assert!(lines.contains(&"ref bind-the-keys".to_string()));
        assert!(lines.contains(&"actor plan-update".to_string()));
        assert_eq!(after(&lines, "Old"), ["  (empty)"]);
        assert_eq!(
            after(&lines, "New"),
            ["  Read the key names from the config."]
        );
        assert_eq!(
            after(&lines, "Reason"),
            ["  the config task settled the names"]
        );
        assert_eq!(targets(&doc), [Target::Task(6)]);

        let doc = self::doc(&app, "departures:0.0");
        let lines = plain(&doc);
        assert_eq!(lines[0], "departure in task 3");
        assert!(lines.contains(&"run wf_2".to_string()));
        assert_eq!(
            after(&lines, "What"),
            ["  Returned warnings beside the config"]
        );
        assert_eq!(
            after(&lines, "Why"),
            ["  An unknown key must reach the header"]
        );
        assert_eq!(
            after(&lines, "Means departed from"),
            ["  Return the config alone"]
        );
        assert_eq!(targets(&doc), [Target::Task(3)]);
    }

    #[test]
    fn plan_details_cost_row_lists_each_agent_counted() {
        let app = app();
        let id = app
            .plan
            .rows
            .iter()
            .find(|r| matches!(&r.record, Record::Cost(c) if !c.agents.is_empty()))
            .map(|r| r.id.clone())
            .expect("a cost row with agents");
        let row = app.plan.row(&id).expect("row");
        let Record::Cost(cost) = &row.record else {
            unreachable!()
        };
        let doc = content(&app, row);
        let lines = plain(&doc);
        assert!(lines[0].starts_with("cost: "), "{lines:?}");
        assert!(lines.iter().any(|l| l.starts_with("time ")));
        assert!(lines.iter().any(|l| l.starts_with("tokens ")));
        let agents = after(&lines, "Agents");
        assert_eq!(agents.len(), cost.agents.len() * 2);
        assert!(agents[0].contains(&cost.agents[0].agent_id), "{agents:?}");
        assert!(doc.links.is_empty());
    }

    #[test]
    fn plan_details_health_rows_show_the_probe_and_link_their_subject() {
        let app = app();
        let doc = doc(&app, "health:accept:6.1");
        let lines = plain(&doc);
        assert_eq!(lines[0], "accept check 2 of task 6");
        assert!(lines.contains(&"Behaviour".to_string()));
        let probe = after(&lines, "Probe");
        assert_eq!(probe[0], "verdict vacuous", "{probe:?}");
        assert!(
            probe.iter().any(|l| l.starts_with("negative exit ")),
            "{probe:?}"
        );
        assert_eq!(targets(&doc), [Target::Task(6)]);

        let doc = self::doc(&app, "health:premise:6.0");
        let lines = plain(&doc);
        assert_eq!(lines[0], "premise 1 of task 6");
        assert!(lines.contains(&"Claim".to_string()));
        assert_eq!(targets(&doc), [Target::Task(6)]);

        let doc = self::doc(&app, "health:gate:B.0");
        let lines = plain(&doc);
        assert_eq!(lines[0], "gate probe of milestone B");
        assert_eq!(after(&lines, "Gate"), ["  cargo clippy"]);
        assert!(lines.contains(&"verdict unprobed".to_string()), "{lines:?}");
        assert_eq!(targets(&doc), [Target::Milestone("B".to_string())]);
    }

    #[test]
    fn plan_details_show_on_the_plan_surface_and_read_as_empty_without_a_row() {
        let mut app = app();
        app.surface = Surface::Plan;
        app.plan.select_row("contracts:C1");
        assert_eq!(
            details::links(&app),
            [
                Link {
                    label: "#3 Load the config".to_string(),
                    target: Target::Task(3),
                },
                Link {
                    label: "#6 Bind the keys".to_string(),
                    target: Target::Task(6),
                }
            ]
        );
        app.plan.cursor = None;
        assert!(details::links(&app).is_empty());
    }

    #[test]
    fn plan_details_cited_notes_reads_each_note_once() {
        assert_eq!(
            cited_notes("see notes/a.md, notes/b-c.md. and notes/a.md; notes/run/x.md notes/.md"),
            ["notes/a.md", "notes/b-c.md"]
        );
        assert!(cited_notes("Read src/config.rs.").is_empty());
    }
}
