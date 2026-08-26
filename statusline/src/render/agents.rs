//! Agent-panel rows for `subagentStatusLine`.
//!
//! The default row body is `name · description · elapsed · ↓ N tokens`, laid out
//! for a wide terminal and left to Ink's blunt end-of-line truncation below
//! that. Here the row is budgeted against the `columns` Claude Code hands us —
//! already the usable body width — and every field carries a priority, so a
//! narrow pane sheds the least useful segment instead of clipping the line:
//!
//! ```text
//! t10-css-face-split · implement-deep · @2   stalled · 126k · 2m13s
//! ```
//!
//! The row has two groups. Identity — title, badge, spec, inbox, activity —
//! stays flush left, where the eye starts reading. The tail — stall marker,
//! token count, context fill, runtime — is flushed right against the same
//! budget, so those fields land in one column down the panel instead of
//! drifting with the length of each row's title. See `in_tail`.
//!
//! Within each group the order is by *positional stability*, mirrored about the
//! gutter: fields that hold their value for a row's whole life sit against the
//! group's anchored edge, and fields that come and go — or change width every
//! tick — sit against the gutter, where their appearing and vanishing spends
//! slack instead of shoving a neighbour into a new column. So `stalled` opens
//! leftward *into* the gutter rather than wedging itself between the token count
//! and the clock, and the activity, the widest-swinging field on the row, is
//! last on the left rather than in the middle of the identity group. See
//! `segments`.
//!
//! `name` is the task title and is never sacrificed: it is the one field that
//! reliably identifies a row. Everything else is shed in priority order and,
//! only once nothing else can give, the name itself is clipped.
//!
//! The activity is deliberately picky about what counts. Claude Code fills the
//! payload's `label` with `progressSummary || description`, so it degrades
//! silently to the first line of the orchestrator's prompt whenever no progress
//! summary exists yet — headings, markdown and all. That prompt text is exactly
//! what this renderer exists to replace, so it is filtered out rather than
//! rendered.
//!
//! **Observed against Claude Code 2.1.234, 2026-08-18.** Two constants here are
//! calibrated against that build's behaviour rather than against anything it
//! promises: `FILLER`, which matches an upstream UI string byte-for-byte, and
//! `STALL_WINDOW`, which is sized against the 16-sample history upstream
//! retains. Both fail *open and quiet*. Reword `working` upstream and the filter
//! stops matching, so a column that says nothing comes back with no error
//! anywhere; drop the retained history below nine and `is_stalled` can never
//! fire again, and no row will mention that it has gone blind. Neither gets
//! runtime detection — the payload carries nothing to detect against, and
//! guessing would be worse than a stale note. `ansi::badge_bg` is the pattern to
//! copy where a fallback *is* possible: an unrecognised colour name renders the
//! badge plainly, visibly unpainted, rather than quietly right-looking. So this
//! line is the anchor instead: the build to diff against when a row starts
//! showing filler or stops ever saying `stalled`.

use crate::ansi::{BG, BLACK_FG, FG, badge_bg};
use crate::fmt::{SEP, ellipsize, format_duration, format_tokens, one_line, width};
use crate::subagent::{SubagentPayload, Task};
use crate::teamdata::Team;

/// Below this many characters an activity string is noise rather than
/// information, so it is dropped whole rather than clipped down to it.
const MIN_LABEL: usize = 8;
/// Fallback when the payload carries no width (hand-run, or a future schema).
const DEFAULT_COLUMNS: usize = 76;
/// Minimum flat readings before a row is allowed to say `stalled`. Upstream
/// keeps at most 16 samples, so this is a little over half the available
/// history — twitchy enough to catch a wedge inside a refresh or two, slack
/// enough that a slow-but-working agent is not accused of one.
///
/// A tuning knob for what a row *says*, in the same class as `MIN_LABEL`, so it
/// lives beside it rather than beside the wire field it reads. The wire fact it
/// is derived from — the 16-sample cap — stays documented on
/// `Task::token_samples`, where it belongs.
const STALL_WINDOW: usize = 9;

// Shed order, lowest first. The name is absent from this scale: it is never shed.
const P_ACTIVITY: u8 = 10;
const P_SPEC: u8 = 20;
/// Context-window fill, `rich` only. Slotted into the gap the ten-spacing exists
/// for: above the model/effort spec, because it is a live reading rather than
/// static configuration, but below the runtime, because it is a restatement of
/// the token count — and the token count sits above `P_PROTECTED` and is never
/// shed at all. Under pressure the derived number should go before the one
/// nothing else reports.
const P_CONTEXT: u8 = 25;
const P_RUNTIME: u8 = 30;
const P_BADGE: u8 = 40;
const P_STALLED: u8 = 50;
const P_MSGS: u8 = 60;
const P_TOKENS: u8 = 70;
const P_NAME: u8 = u8::MAX;
/// At or above this a segment is never shed — only the title is clipped to make
/// room for it. The token count and the inbox depth are the numbers worth
/// looking at, so they outlast a fully-spelled title.
///
/// **The alias is deliberate and it is a trap.** The floor is defined as "the
/// message count and everything above it", so it is written as `P_MSGS` rather
/// than as `60` — move `P_MSGS` and the floor follows it, which is the intent.
/// The hazard is that a single `u8` is carrying two independent things: a
/// segment's *rank* in the shed order, and its *class* — sheddable, or not. The
/// constants above are spaced by ten precisely so a new segment can be slotted
/// between two of them, but that only works below the floor. Insert one at 65
/// and you have not given it a high priority, you have made it **unsheddable**,
/// and nothing in the code will say so: it simply stops disappearing on a narrow
/// pane and starts clipping the title instead.
///
/// The type-level fix is a separate `Shrink { Drop, Clip, Keep }` field on
/// `Seg`, which would also fold `row`'s two special-cased clip passes into one
/// generic step. It is deliberately not done: the segment list is seven stable
/// entries mirroring the payload, and the restructure would rework the most
/// intricate and most heavily-pinned logic in the crate for a hazard that is
/// still latent. Revisit it when the list actually starts growing. Until then
/// the guard is a test, not a type:
/// `the_unsheddable_class_is_exactly_the_two_counts_and_the_title` pins which
/// segment *kinds* sit above this floor, so a reordering that promotes one
/// across it fails a test instead of quietly changing what narrow rows show.
const P_PROTECTED: u8 = P_MSGS;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    /// Name, agent type, messages, activity, stall, tokens, runtime.
    Tiers,
    /// As `tiers`, plus resolved model and effort when the width allows.
    Rich,
}

/// One rendered field. `plain` drives the width budget; `painted` is what is
/// emitted, and differs only for the agent-type badge.
///
/// `painted` is `None` when the two forms are identical, which is every segment
/// but a coloured badge. Storing it as an owned `String` meant a second heap
/// allocation of every field of every row on every refresh tick — six-odd
/// clones per row for a value nothing ever read separately — and this crate's
/// entire reason to exist is the per-tick cost. `Cow<'_, str>` is the other
/// obvious shape and is not available here: the borrowed arm would have to
/// point at `plain` in the same struct, which is self-referential and
/// unexpressible in safe Rust, so it would degenerate to `Cow::Owned` and the
/// clone would come straight back. `Option` says the same thing with no
/// lifetime and no allocation, resolved once at the join site in `fit`.
struct Seg {
    plain: String,
    painted: Option<String>,
    prio: u8,
}

impl Seg {
    fn text(s: impl Into<String>, prio: u8) -> Self {
        Seg { plain: s.into(), painted: None, prio }
    }

    /// What actually goes to stdout: the painted form when there is one, the
    /// plain text otherwise.
    fn emit(&self) -> &str {
        self.painted.as_deref().unwrap_or(&self.plain)
    }

    /// The agent-type chip: the teammate's own colour as the background, with
    /// the foreground dropped to black so it reads inverted against it. ANSI
    /// indices, so the chip draws from the terminal's scheme rather than a
    /// palette baked in here. Closed with SGR 49/39 rather than a reset, which
    /// would cancel Claude Code's dim wrapper for the rest of the row. The text
    /// is agent-supplied, so it goes through `one_line` first: an escape smuggled
    /// in there would repaint the row and everything after it, and `width` would
    /// have budgeted against bytes the terminal never draws.
    fn badge(text: &str, color: Option<&str>) -> Self {
        let plain = format!(" {} ", one_line(text));
        // The unmapped-colour arm leaves `painted` unset rather than cloning the
        // plain text into it: an unrecognised name renders the badge bare, which
        // is the visible degradation this file wants, and it costs nothing.
        let painted = color
            .and_then(badge_bg)
            .map(|bg| format!("{bg}{BLACK_FG}{plain}{BG}{FG}"));
        Seg { plain, painted, prio: P_BADGE }
    }
}

/// One NDJSON line per task, ready for stdout.
pub fn render(
    payload: &SubagentPayload,
    style: Style,
    cols_override: Option<usize>,
    now_ms: i64,
    team: &Team,
) -> String {
    let cols = cols_override
        .or(payload.columns)
        .filter(|c| *c > 0)
        .unwrap_or(DEFAULT_COLUMNS);
    payload
        .tasks
        .iter()
        .map(|t| {
            let content = row(t, style, cols, now_ms, team);
            // serde_json does the escaping; a raw label can contain quotes.
            format!(
                "{{\"id\":{},\"content\":{}}}",
                serde_json::Value::String(t.id.clone()),
                serde_json::Value::String(content)
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn name_of(t: &Task) -> Option<String> {
    t.name.as_deref().map(one_line).filter(|s| !s.is_empty())
}

/// Filler Claude Code emits when a teammate has no activity to report.
/// Rendering it costs a column and says nothing the runtime does not.
const FILLER: [&str; 1] = ["working"];

/// The live "what it is doing" string — but only when it genuinely is one.
///
/// `label` equal to `description` means Claude Code's `progressSummary ||
/// description` fell through to the prompt, so there is no real activity to
/// show. Treating that as a label is what let a markdown heading take the whole
/// row.
fn activity_of(t: &Task) -> Option<String> {
    let label = t.label.as_deref().map(one_line).filter(|s| !s.is_empty())?;
    if t.description.as_deref().map(one_line).as_deref() == Some(label.as_str()) {
        return None;
    }
    if FILLER.contains(&label.as_str()) {
        return None;
    }
    Some(label)
}

/// Last resort when a row has no name and no activity: the description, with
/// markdown lead-in stripped so a `## Heading` prompt does not render its
/// scaffolding.
fn description_of(t: &Task) -> Option<String> {
    let d = one_line(t.description.as_deref()?);
    let d = d.trim_start_matches(['#', '>', '-', '*', '+', ' ']).to_string();
    (!d.is_empty()).then_some(d)
}

/// How full the task's context window is, as a whole-percent reading.
///
/// This is the one documented purpose of the payload's `contextWindowSize`, and
/// the ratio the main status line already renders for the session — so a row
/// showing it says the same thing about a teammate that the bottom line says
/// about you. It is `rich`-only on purpose: `tiers` was specified as the raw
/// token count *without* a window ratio and must stay byte-identical.
///
/// `None` unless both numbers are actually present and positive; a missing or
/// zero window would otherwise render either a divide-by-zero or a confident
/// `0%`. Clamped at 100 because the count and the window are sampled
/// independently upstream and a momentarily-stale window would read as `104%`.
fn context_pct(t: &Task) -> Option<String> {
    let used = t.token_count.filter(|n| *n > 0)?;
    let size = t.context_window_size.filter(|n| *n > 0)?;
    Some(format!("{}%", (used * 100 / size).min(100)))
}

/// No token growth across the newest `STALL_WINDOW` samples.
///
/// The samples carry no timestamps, so this is "N consecutive readings with
/// no growth", not a duration. It catches a wedged agent, and also an agent
/// legitimately blocked in a long tool call — both are worth seeing, but it
/// is not by itself proof of a fault.
fn is_stalled(t: &Task) -> bool {
    if t.status.as_deref() != Some("running") {
        return false;
    }
    let s = &t.token_samples;
    if s.len() < STALL_WINDOW {
        return false;
    }
    // Trailing window, not the whole retained history: with 16 samples kept
    // upstream, testing all of them would delay the flag well past the 9
    // readings STALL_WINDOW advertises.
    let w = &s[s.len() - STALL_WINDOW..];
    w[0] > 0 && w.iter().all(|v| *v == w[0])
}

/// Elapsed time, or the state that replaces it. A queued or paused row has no
/// meaningful runtime yet, and saying so beats showing `0s`; a finished one has
/// no end time anywhere on the wire, so an elapsed reading would keep growing
/// after the fact and read as a task still running.
fn runtime(t: &Task, now_ms: i64) -> Option<String> {
    match t.status.as_deref() {
        Some("pending") => return Some("queued".into()),
        Some("paused") => return Some("paused".into()),
        Some("completed") => return Some("done".into()),
        Some("failed") => return Some("failed".into()),
        Some("killed") => return Some("killed".into()),
        _ => {}
    }
    let started = t.started_at_ms()?;
    Some(format_duration(now_ms.saturating_sub(started)))
}

/// Build the row's fields in *layout* order. Push order is layout order; `prio`
/// is shed order, and the two are independent — nothing here changes which field
/// a narrow pane gives up.
///
/// The order within each group is by positional stability, mirrored about the
/// gutter `fit` opens between them. The identity group is anchored at the left
/// edge, so its settled fields — the title, the badge, the resolved model —
/// come first and its coming-and-going ones last. The status group is anchored
/// at the right edge, so it is the mirror image: the stall marker leads, and the
/// fields that are there for every tick of a row's life trail it.
///
/// The point is what a row does *between* two refreshes. A field that appears
/// mid-run costs its group the width of itself plus a separator; put it against
/// the anchored edge and every field on its gutter side moves a column, on a
/// panel the eye is reading down. Against the gutter it eats the slack that is
/// already blank, and nothing else on the row moves at all. Hence `stalled`
/// ahead of the token count rather than between the count and the clock, and the
/// activity — free-form text that can change width on every single tick — last
/// on the left rather than in the middle of the identity group.
fn segments(t: &Task, style: Style, now_ms: i64, team: &Team) -> Vec<Seg> {
    let member = t.name.as_deref().and_then(|n| team.get(n));
    // Nine: the seven `tiers` segments plus `rich`'s model/effort spec and its
    // context reading, so the widest style never reallocates mid-row.
    let mut segs = Vec::with_capacity(9);

    // --- Identity, flush left, settled fields first.
    let mut activity = activity_of(t);
    // The title, and the only segment with no shed priority. `name` when there is
    // one; a bash task or a workflow carries none, and then the activity — or,
    // failing that, the description, or the bare task kind — is the only thing
    // identifying the row, so it is promoted into the title slot rather than left
    // in the sheddable, gutter-hugging activity one.
    let title = name_of(t)
        .or_else(|| activity.take())
        .or_else(|| description_of(t))
        .unwrap_or_else(|| one_line(t.r#type.as_deref().unwrap_or("agent")));
    segs.push(Seg::text(title, P_NAME));

    if let Some(kind) = member.and_then(|m| m.agent_type.as_deref()) {
        segs.push(Seg::badge(kind, member.and_then(|m| m.color.as_deref())));
    }
    if style == Style::Rich {
        // Both labels are payload-supplied strings, so they take the same
        // control-byte filter every other free-text field goes through.
        let spec: Vec<String> = [t.model_label(), t.effort_label()]
            .into_iter()
            .flatten()
            .map(|s| one_line(&s))
            .filter(|s| !s.is_empty())
            .collect();
        if !spec.is_empty() {
            segs.push(Seg::text(spec.join(" "), P_SPEC));
        }
    }
    // An inbox fills and drains mid-run, so it follows everything settled.
    if let Some(n) = member.map(|m| m.inbox).filter(|n| *n > 0) {
        segs.push(Seg::text(format!("@{n}"), P_MSGS));
    }
    // Last on the left, hard against the gutter: the activity both comes and goes
    // and re-widths itself when it does neither, and this is the one position
    // where that costs no other field its column.
    if let Some(a) = activity {
        segs.push(Seg::text(a, P_ACTIVITY));
    }

    // --- Status, flush right, settled fields last. The stall marker is the only
    // field here that arrives mid-run, so it leads and opens leftward into the
    // gutter; the count, the reading and the clock keep the columns they had.
    if is_stalled(t) {
        segs.push(Seg::text("stalled", P_STALLED));
    }
    if let Some(n) = t.token_count.filter(|n| *n > 0) {
        segs.push(Seg::text(format_tokens(n), P_TOKENS));
    }
    // Next to the count it is derived from, so the two still read together:
    // `124k · 62%`.
    if style == Style::Rich
        && let Some(pct) = context_pct(t)
    {
        segs.push(Seg::text(pct, P_CONTEXT));
    }
    if let Some(s) = runtime(t, now_ms) {
        segs.push(Seg::text(s, P_RUNTIME));
    }
    segs
}

/// Width of a segment run joined with `SEP`. Takes an iterator rather than a
/// slice so the same accounting serves the whole row and each of `fit`'s two
/// groups, which are borrowed rather than owned.
fn line_width<'a>(segs: impl IntoIterator<Item = &'a Seg>) -> usize {
    let mut total = 0;
    let mut n = 0usize;
    for s in segs {
        total += width(&s.plain);
        n += 1;
    }
    total + width(SEP) * n.saturating_sub(1)
}

fn join<'a>(segs: impl IntoIterator<Item = &'a Seg>) -> String {
    segs.into_iter().map(Seg::emit).collect::<Vec<_>>().join(SEP)
}

/// Segments that ride in the right-hand column rather than beside the title.
///
/// The counts and the clock are read *down* a panel more than along a row, and
/// left-flush they start at a different column on every line, because the title
/// and activity in front of them differ in length. Flushed right they become a
/// column: same fields, same place, every row — while the activity, which is
/// read along the row, stays where the eye starts.
///
/// Membership is by priority, not by position, so push order cannot silently
/// change it. The four are nonetheless contiguous at the end of `segments`, so
/// the split takes the tail's own reading — `stalled · 124k · 62% · 4m12s` —
/// across whole; only the join in front of it turns from a separator into a
/// gutter. `@N` stays on the left: an inbox depth identifies a teammate's state
/// rather than measuring its progress, and it is pushed before the count.
fn in_tail(prio: u8) -> bool {
    matches!(prio, P_TOKENS | P_CONTEXT | P_STALLED | P_RUNTIME)
}

/// Join for stdout, clipped to the budget, with the tail flushed right. The
/// shed and title passes cannot always get inside `cols` on their own — a lone
/// segment has nothing left to shed, and a residue entirely at or above
/// `P_PROTECTED` is by definition unsheddable — so the last word on width is
/// here, unconditionally. The clip runs on the plain text: a half-written escape
/// is worse than a lost badge colour, and a row this narrow has already shed the
/// badge anyway. An over-budget row is clipped flush left: there is no slack to
/// align with, and padding it would only push the tail off the end.
fn fit(segs: &[Seg], cols: usize) -> String {
    if line_width(segs) > cols {
        let plain = segs.iter().map(|s| s.plain.as_str()).collect::<Vec<_>>().join(SEP);
        return ellipsize(&plain, cols);
    }
    let (head, tail): (Vec<&Seg>, Vec<&Seg>) = segs.iter().partition(|s| !in_tail(s.prio));
    // Nothing to align against. A row shed down to its bare numbers reads better
    // flush left than indented away from every row above it, and a row with no
    // tail at all has no column to flush.
    if head.is_empty() || tail.is_empty() {
        return join(segs);
    }
    // The two group widths together are `line_width` less the single separator
    // the gutter replaces, so the gap is at least `SEP` wide for any row that
    // fits — which the branch above has already established. Saturating anyway:
    // the arithmetic is proved, but an underflow here would wrap in release and
    // ask for a string of near-`usize::MAX` spaces, and a collapsed gutter is a
    // cheaper way to be wrong than an allocation that takes the session with it.
    let content = line_width(head.iter().copied()) + line_width(tail.iter().copied());
    let gap = " ".repeat(cols.saturating_sub(content));
    format!("{}{gap}{}", join(head.iter().copied()), join(tail))
}

/// Everything `row` does except the final join: build the segments, clip the
/// activity, shed by priority, then clip or drop the title.
///
/// Split out from `row` so the shed order can be asserted on directly. Testing
/// it through the rendered string cannot distinguish a segment that was shed
/// from one `fit`'s backstop ellipsis happened to cut off, and the activity
/// changes its text when it is clipped rather than vanishing — so a
/// string-matching test would have to guess at both. Returning the surviving
/// segments lets `the_shed_order_holds_at_every_width` compare priorities, and
/// scopes it to the passes it is actually about: `fit`'s unconditional clip
/// happens after this returns and so is excluded by construction rather than by
/// a width filter.
fn shed(t: &Task, style: Style, cols: usize, now_ms: i64, team: &Team) -> Vec<Seg> {
    let mut segs = segments(t, style, now_ms, team);

    // The activity is the one segment worth clipping rather than dropping: it is
    // free-form, so a prefix of it still carries meaning.
    if line_width(&segs) > cols
        && let Some(i) = segs.iter().position(|s| s.prio == P_ACTIVITY)
    {
        let others = line_width(&segs) - width(&segs[i].plain);
        let room = cols.saturating_sub(others);
        if room >= MIN_LABEL {
            let clipped = ellipsize(&segs[i].plain, room);
            segs[i] = Seg::text(clipped, P_ACTIVITY);
        }
    }

    // Shed lowest-priority first, down to the protected floor.
    while line_width(&segs) > cols && segs.len() > 1 {
        let Some(i) = segs
            .iter()
            .enumerate()
            .filter(|(_, s)| s.prio < P_PROTECTED)
            .min_by_key(|(_, s)| s.prio)
            .map(|(i, _)| i)
        else {
            break;
        };
        segs.remove(i);
    }

    // Only now, with nothing sheddable left, does the title give ground — and it
    // is clipped rather than dropped, unless there is not even room for a
    // marker, in which case the numbers alone say more than a bare ellipsis.
    if line_width(&segs) > cols && segs.first().map(|s| s.prio) == Some(P_NAME) {
        let others = line_width(&segs) - width(&segs[0].plain);
        let clipped = ellipsize(&segs[0].plain, cols.saturating_sub(others));
        if !clipped.is_empty() {
            segs[0] = Seg::text(clipped, P_NAME);
        } else if segs.len() > 1 {
            // Dropping the title is only an option while something else is left
            // to identify the row; a lone one is clipped by `fit` instead.
            segs.remove(0);
        }
    }

    segs
}

fn row(t: &Task, style: Style, cols: usize, now_ms: i64, team: &Team) -> String {
    // No colour of our own beyond the badge. Claude Code already dims unselected
    // rows and brightens the viewed one; painting the rest would fight that.
    fit(&shed(t, style, cols, now_ms, team), cols)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::teamdata::Member;
    use crate::testing::plain;

    const NOW: i64 = 1_000_000_000;

    fn task(json: &str) -> Task {
        serde_json::from_str(json).expect("task parses")
    }

    fn no_team() -> Team {
        Team::new()
    }

    fn with_member(name: &str, agent_type: &str, color: &str, inbox: usize) -> Team {
        let mut t = Team::new();
        t.insert(
            name.into(),
            Member {
                agent_type: Some(agent_type.into()),
                color: Some(color.into()),
                inbox,
            },
        );
        t
    }

    fn falcon() -> Task {
        task(&format!(
            r#"{{"id":"t1","name":"Falcon","type":"in_process_teammate","status":"running",
                 "label":"research-deep","tokenCount":31000,"startTime":{},
                 "model":"claude-opus-5","effort":"high"}}"#,
            NOW - 252_000
        ))
    }

    /// A row carrying both context numbers. The only fixture that produces a
    /// `P_CONTEXT` segment, and it does so under `rich` alone — `tiers` reads the
    /// same task and must render no ratio at all.
    fn context_row() -> Task {
        task(&format!(
            r#"{{"id":"c","name":"Osprey","type":"in_process_teammate","status":"running",
                 "tokenCount":124000,"contextWindowSize":200000,"model":"claude-opus-5",
                 "startTime":{}}}"#,
            NOW - 95_000
        ))
    }

    /// A wedged row: nine flat samples, so it renders the stall marker. Shared by
    /// the stall test and the width fixtures, and the `_stalled` half of the
    /// stability pair below — `flat` is the same task with one growing sample, so
    /// the two differ in the marker and in nothing else.
    fn stalled_row() -> Task {
        kite(&"9000,".repeat(9))
    }

    fn kite(samples: &str) -> Task {
        task(&format!(
            r#"{{"id":"s","name":"Kite","status":"running","tokenCount":9000,
                 "startTime":{},"tokenSamples":[{}]}}"#,
            NOW - 60_000,
            samples.trim_end_matches(',')
        ))
    }

    fn at(t: &Task, cols: usize) -> String {
        plain(&row(t, Style::Tiers, cols, NOW, &no_team()))
    }

    fn at_team(t: &Task, cols: usize, team: &Team) -> String {
        plain(&row(t, Style::Tiers, cols, NOW, team))
    }

    /// The expected text of a two-group row: `head`, the gutter, and the tail
    /// flushed right against `cols`. Spelling the gutter out in every literal
    /// would bury the segment content these tests are actually about, and a row
    /// that was never going to fit underflows loudly here rather than quietly
    /// asserting the wrong string.
    fn aligned(head: &str, tail: &str, cols: usize) -> String {
        format!("{head}{}{tail}", " ".repeat(cols - width(head) - width(tail)))
    }

    #[test]
    fn wide_row_keeps_name_activity_tokens_and_runtime() {
        assert_eq!(
            at(&falcon(), 80),
            aligned("Falcon \u{b7} research-deep", "31k \u{b7} 4m12s", 80)
        );
    }

    #[test]
    fn tiers_clip_the_activity_then_shed_by_priority() {
        let t = falcon();
        let tail = "31k \u{b7} 4m12s";
        assert_eq!(at(&t, 36), aligned("Falcon \u{b7} research-deep", tail, 36));
        // One char short: the title holds, the activity gives ground.
        assert_eq!(at(&t, 35), aligned("Falcon \u{b7} research-de\u{2026}", tail, 35));
        assert_eq!(at(&t, 31), aligned("Falcon \u{b7} researc\u{2026}", tail, 31));
        // Below a readable activity it is dropped whole, not clipped to noise.
        assert_eq!(at(&t, 30), aligned("Falcon", tail, 30));
        // Runtime is the next lowest priority.
        assert_eq!(at(&t, 19), aligned("Falcon", "31k", 19));
        // Then the title itself is clipped, never the numbers. At 12 the gutter
        // is down to its `SEP`-wide floor, which is where a fitting row bottoms
        // out — one column narrower and the title starts giving ground.
        assert_eq!(at(&t, 12), aligned("Falcon", "31k", 12));
        assert_eq!(at(&t, 9), aligned("Fa\u{2026}", "31k", 9));
        // With no room even for a marker, the number alone beats a bare "…" —
        // and with nothing left to align against it goes flush left.
        assert_eq!(at(&t, 5), "31k");
    }

    /// Which segment kinds are *unsheddable*, pinned as a class and separately
    /// from the rank the same `u8` encodes.
    ///
    /// The ten-spacing between the priority constants exists so a new segment
    /// can be slotted between two of them. Below `P_PROTECTED` that changes only
    /// where the segment sits in the shed order; at or above it, the segment
    /// silently stops being sheddable at all and starts clipping the title
    /// instead. Nothing in the type system distinguishes the two outcomes, so
    /// this table does: a reordering that moves a kind across the floor changes
    /// a line here rather than changing what narrow rows show.
    #[test]
    fn the_unsheddable_class_is_exactly_the_two_counts_and_the_title() {
        // (kind, its priority, may the shed loop ever drop it?)
        let table = [
            ("activity", P_ACTIVITY, true),
            ("model/effort spec", P_SPEC, true),
            ("context fill", P_CONTEXT, true),
            ("runtime", P_RUNTIME, true),
            ("agent-type badge", P_BADGE, true),
            ("stall marker", P_STALLED, true),
            ("inbox depth", P_MSGS, false),
            ("token count", P_TOKENS, false),
            ("title", P_NAME, false),
        ];
        for (kind, prio, sheddable) in table {
            assert_eq!(
                prio < P_PROTECTED,
                sheddable,
                "{kind} ({prio}) changed shed class; P_PROTECTED is {P_PROTECTED}"
            );
        }

        // And behaviourally, on a row carrying one of every kind: squeezed until
        // nothing sheddable is left, exactly the two counts survive. The title
        // goes with the rest — unsheddable is not un-droppable, and once it is
        // too narrow even to clip, the numbers say more than a bare marker.
        let team = with_member("Falcon", "implement-deep", "purple", 3);
        assert_eq!(at_team(&falcon(), 8, &team), aligned("@3", "31k", 8));
    }

    #[test]
    fn the_agent_type_badge_carries_the_teammates_colour_inverted() {
        let t = falcon();
        let team = with_member("Falcon", "implement-deep", "green", 0);
        assert_eq!(
            at_team(&t, 90, &team),
            aligned(
                "Falcon \u{b7}  implement-deep  \u{b7} research-deep",
                "31k \u{b7} 4m12s",
                90
            )
        );
        let painted = row(&t, Style::Tiers, 90, NOW, &team);
        // Green background, black foreground, closed with SGR 49/39 so the
        // host's dim wrapper survives the rest of the row.
        assert!(painted.contains("\u{1b}[42m\u{1b}[30m implement-deep \u{1b}[49m\u{1b}[39m"));
        assert!(!painted.contains("\u{1b}[0m"), "must never emit SGR 0: {painted:?}");
    }

    #[test]
    fn an_unknown_teammate_colour_still_renders_the_type_plainly() {
        let mut team = Team::new();
        team.insert(
            "Falcon".into(),
            Member {
                agent_type: Some("verification".into()),
                color: Some("chartreuse".into()),
                inbox: 0,
            },
        );
        let painted = row(&falcon(), Style::Tiers, 90, NOW, &team);
        assert!(plain(&painted).contains("verification"));
        assert!(!painted.contains('\u{1b}'), "no colour for an unmapped name: {painted:?}");
    }

    #[test]
    fn a_pending_inbox_shows_and_outranks_the_badge() {
        let t = falcon();
        let team = with_member("Falcon", "implement-deep", "blue", 2);
        assert!(at_team(&t, 90, &team).contains("@2"));
        // Squeezed: activity, runtime, then the badge go; the count stays — and
        // the inbox depth stays with the identity on the left.
        assert_eq!(at_team(&t, 24, &team), aligned("Falcon \u{b7} @2", "31k", 24));
        // An empty inbox renders nothing at all.
        let quiet = with_member("Falcon", "implement-deep", "blue", 0);
        assert!(!at_team(&t, 90, &quiet).contains('@'));
    }

    #[test]
    fn a_stalled_task_says_so_and_outranks_the_runtime() {
        let t = stalled_row();
        // The marker opens leftward into the gutter: the count and the clock hold
        // the columns they had before it appeared — see
        // `an_appearing_field_never_shifts_a_settled_one`.
        assert_eq!(at(&t, 80), aligned("Kite", "stalled \u{b7} 9k \u{b7} 1m0s", 80));
        assert_eq!(at(&t, 20), aligned("Kite", "stalled \u{b7} 9k", 20));
    }

    /// Moved here with `STALL_WINDOW` and `is_stalled`: the threshold is a
    /// decision about what a row says, not about what the wire means, so it is
    /// tested beside the other row-content rules rather than beside the payload.
    #[test]
    fn stall_needs_a_full_flat_window_on_a_running_task() {
        let flat = "5000,".repeat(9);
        let t = task(&format!(
            r#"{{"status":"running","tokenSamples":[{}]}}"#,
            flat.trim_end_matches(',')
        ));
        assert!(is_stalled(&t));

        // Growth anywhere in the window clears it.
        let t = task(r#"{"status":"running",
            "tokenSamples":[5000,5000,5000,5000,5000,5000,5000,5000,5100]}"#);
        assert!(!is_stalled(&t));

        // Too little history to judge.
        let t = task(r#"{"status":"running","tokenSamples":[5000,5000,5000]}"#);
        assert!(!is_stalled(&t));

        // A task that has produced nothing at all is not "stalled", it is new.
        let t = task(r#"{"status":"running","tokenSamples":[0,0,0,0,0,0,0,0,0]}"#);
        assert!(!is_stalled(&t));

        // Only running tasks can stall; a completed one is just finished.
        let t = task(r#"{"status":"completed",
            "tokenSamples":[9,9,9,9,9,9,9,9,9]}"#);
        assert!(!is_stalled(&t));
    }

    /// The row that prompted the identity rework: a real teammate whose `label`
    /// had fallen through to the first line of the orchestrator's prompt.
    #[test]
    fn a_prompt_derived_label_never_displaces_the_task_title() {
        let prompt = "## Shared context \u{2014} theme-package font architecture \
                      You are working on the CSS face split";
        let t = task(&format!(
            r#"{{"id":"t10","name":"t10-css-face-split","type":"in_process_teammate",
                 "status":"running","description":{0},"label":{0},
                 "tokenCount":126000,"startTime":{1}}}"#,
            serde_json::Value::String(prompt.into()),
            NOW - 133_000
        ));
        assert_eq!(at(&t, 76), aligned("t10-css-face-split", "126k \u{b7} 2m13s", 76));
        assert_eq!(at(&t, 25), aligned("t10-css-face-split", "126k", 25));
        assert_eq!(at(&t, 24), aligned("t10-css-face-spl\u{2026}", "126k", 24));
    }

    #[test]
    fn filler_activity_is_not_worth_a_column() {
        let t = task(&format!(
            r#"{{"id":"t","name":"Kestrel","status":"running","label":"working",
                 "tokenCount":4000,"startTime":{}}}"#,
            NOW - 9_000
        ));
        assert_eq!(at(&t, 76), aligned("Kestrel", "4k \u{b7} 9s", 76));
    }

    #[test]
    fn a_markdown_description_is_cleaned_when_it_is_all_there_is() {
        // Three hashes deliberately: the JSON contains the sequence quote-hash-
        // hash, which terminates both `r#"` and `r##"`.
        let t = task(
            r###"{"id":"t","type":"local_agent","status":"running",
                  "description":"## Shared context\n\nsplit the faces","tokenCount":2000}"###,
        );
        assert_eq!(at(&t, 76), aligned("Shared context split the faces", "2k", 76));
    }

    #[test]
    fn a_task_without_a_start_time_shows_no_runtime() {
        let t =
            task(r#"{"id":"t","name":"Wren","status":"running","startTime":0,"tokenCount":5000}"#);
        assert_eq!(at(&t, 76), aligned("Wren", "5k", 76));
    }

    /// The shapes the shed guards actually branch on: how many segments a row
    /// starts with, and whether what survives the shed is entirely at or above
    /// `P_PROTECTED`. A table of only multi-segment rows with something
    /// sheddable in them — which is all `falcon` is — cannot see either
    /// overflow class.
    fn width_fixtures() -> Vec<(&'static str, Task, Team)> {
        vec![
            ("falcon", falcon(), no_team()),
            ("falcon + team", falcon(), with_member("Falcon", "implement-deep", "purple", 3)),
            // One segment and nothing protected beside it: the row is all title,
            // so only a clip can bring it inside the budget.
            (
                "name only",
                task(r#"{"id":"n","name":"t10-css-face-split","tokenCount":0}"#),
                no_team(),
            ),
            // A bash task carries no name, so its label is the only identifier.
            (
                "activity only",
                task(
                    r#"{"id":"b","type":"local_bash","status":"running",
                        "label":"bun run type-check","tokenCount":0}"#,
                ),
                no_team(),
            ),
            // Inbox and tokens are both protected, so once the title is gone the
            // residue can neither shed nor clip itself any further.
            (
                "protected only",
                task(r#"{"id":"p","name":"Falcon","status":"running","tokenCount":126000}"#),
                with_member("Falcon", "implement-deep", "purple", 12),
            ),
            // Carries `P_CONTEXT`, the one priority slotted between two existing
            // constants rather than appended past them — so the width and shed
            // properties below actually exercise the ten-spacing they document.
            ("rich context", context_row(), no_team()),
            // The only fixture carrying `P_STALLED`, and so the only one whose
            // tail is more than the count-and-clock pair. Without it the width
            // and flush-right properties never see a three-field tail at all.
            ("stalled", stalled_row(), with_member("Kite", "verification", "yellow", 1)),
        ]
    }

    /// Segment kinds, as the priorities that identify them. Priorities are
    /// unique within a row — only one segment is ever built at each constant —
    /// so this identifies the surviving *set* exactly, and unlike the rendered
    /// text it is unmoved by the activity clip rewriting a segment in place.
    fn prios(segs: &[Seg]) -> Vec<u8> {
        segs.iter().map(|s| s.prio).collect()
    }

    /// The invariant the eight hand-computed widths in
    /// `tiers_clip_the_activity_then_shed_by_priority` are a sample of.
    ///
    /// Those literals each probe a real transition and are worth keeping, but
    /// they check the priority table at eight of ninety-odd widths using
    /// arithmetic a human has to redo on every change. This asserts the rule
    /// they stand for at *every* width, so a reordering of `P_MSGS` /
    /// `P_TOKENS` / `P_CONTEXT` / `P_PROTECTED` fails here rather than slipping
    /// through the gaps between the samples.
    ///
    /// Two complications, both handled by asserting on priorities rather than on
    /// rendered text. The activity is *clipped* before anything is shed, so it
    /// changes content instead of vanishing — identifying segments by kind stops
    /// that reading as a shed. And `fit`'s final unconditional ellipsize can
    /// collapse a line at widths no shed order could satisfy; this runs against
    /// `shed`, which returns before `fit` is called, so those widths are outside
    /// the assertion by construction rather than by a filter that would weaken
    /// it everywhere else.
    #[test]
    fn the_shed_order_holds_at_every_width() {
        for (label, t, team) in width_fixtures() {
            for style in [Style::Tiers, Style::Rich] {
                let full = prios(&segments(&t, style, NOW, &team));
                let mut sheddable: Vec<u8> =
                    full.iter().copied().filter(|p| *p < P_PROTECTED).collect();
                sheddable.sort_unstable();

                // Descending, so `wider` always holds the result one column up.
                let mut wider: Option<Vec<u8>> = None;
                for cols in (1..=95).rev() {
                    let here = prios(&shed(&t, style, cols, NOW, &team));
                    let at = format!("{label} ({style:?}) at {cols} cols");

                    // 1. The survivors are a SUFFIX of the priority order:
                    //    nothing is shed while something strictly lower-priority
                    //    is still standing. Scoped to the sheddable class, which
                    //    is the only class the shed loop touches.
                    let mut kept: Vec<u8> =
                        here.iter().copied().filter(|p| *p < P_PROTECTED).collect();
                    kept.sort_unstable();
                    assert!(
                        kept.len() <= sheddable.len(),
                        "{at}: shed produced segments that were never built"
                    );
                    assert_eq!(
                        kept,
                        sheddable[sheddable.len() - kept.len()..],
                        "{at}: shed out of order, kept {kept:?} of {sheddable:?}"
                    );

                    // 2. Nothing at or above the floor is shed — except the
                    //    title, which the dedicated title pass may drop, and only
                    //    while something else is left to identify the row.
                    for p in full.iter().filter(|p| **p >= P_PROTECTED && **p != P_NAME) {
                        assert!(here.contains(p), "{at}: protected priority {p} was shed");
                    }
                    if full.contains(&P_NAME) && !here.contains(&P_NAME) {
                        assert!(!here.is_empty(), "{at}: title dropped with nothing left");
                    }

                    // 3. Survival is MONOTONE as the budget falls: a segment shed
                    //    at some width must not reappear at any narrower one.
                    if let Some(prev) = &wider {
                        for p in &here {
                            assert!(
                                prev.contains(p),
                                "{at}: priority {p} reappeared below {} cols",
                                cols + 1
                            );
                        }
                    }
                    wider = Some(here);
                }
            }
        }
    }

    #[test]
    fn every_tier_fits_its_budget() {
        for (label, t, team) in width_fixtures() {
            for style in [Style::Tiers, Style::Rich] {
                for cols in 1..=95 {
                    let w = plain(&row(&t, style, cols, NOW, &team)).chars().count();
                    assert!(w <= cols, "{label} ({style:?}) at {cols} cols overflowed to {w}");
                }
            }
        }
    }

    /// The tail is a *column*, not a suffix: on every row that fits its budget
    /// and carries both groups, the last character of the tail sits in the last
    /// usable column, so the counts and the clock line up down the panel however
    /// long the titles in front of them are.
    ///
    /// Asserted as a width identity rather than by matching the rendered text,
    /// which would only restate `aligned`'s arithmetic. The two excluded shapes
    /// are excluded deliberately: an over-budget row has no slack to spend on a
    /// gutter, and a row with an empty group has nothing to align against.
    #[test]
    fn the_tail_is_flush_right_on_every_row_that_fits() {
        for (label, t, team) in width_fixtures() {
            for style in [Style::Tiers, Style::Rich] {
                for cols in 1..=95 {
                    let segs = shed(&t, style, cols, NOW, &team);
                    let both = segs.iter().any(|s| in_tail(s.prio))
                        && segs.iter().any(|s| !in_tail(s.prio));
                    if !both || line_width(&segs) > cols {
                        continue;
                    }
                    let r = plain(&row(&t, style, cols, NOW, &team));
                    assert_eq!(
                        width(&r),
                        cols,
                        "{label} ({style:?}) at {cols} cols: tail not flush right: {r:?}"
                    );
                    // The gutter is padding, never a trailing edge.
                    assert!(!r.ends_with(' '), "{label} ({style:?}) at {cols} cols: {r:?}");
                }
            }
        }
    }

    /// The column a rendered field starts in, or `None` when the row does not
    /// carry it. Columns rather than byte offsets: `SEP` is multi-byte, so a byte
    /// index would move for reasons that have nothing to do with layout.
    fn column_of(rendered: &str, field: &str) -> Option<usize> {
        rendered.find(field).map(|i| width(&rendered[..i]))
    }

    /// The rule both group orders exist for: a field that arrives, departs, or
    /// re-widths itself mid-run spends the gutter, never a neighbour's column.
    ///
    /// This is the property `in_tail` and `segments`'s push order *jointly*
    /// produce, and neither can be checked for it alone — a correct order still
    /// shifts every column if `fit` flushes the wrong group, and a right-flushed
    /// tail still shifts them if the marker is pushed into the middle of it. So
    /// it is asserted where the two meet, on rendered columns, which is also what
    /// the eye actually tracks down a refreshing panel.
    ///
    /// Each case renders one task twice, differing only in the transient field,
    /// and pins every settled field to the same column in both.
    #[test]
    fn an_appearing_field_never_shifts_a_settled_one() {
        // Same nine samples as `stalled_row`, with the last one growing — so the
        // pair differs in the stall marker and in nothing else at all.
        let growing = kite(&format!("{}9100,", "9000,".repeat(8)));
        let verbose = task(&format!(
            r#"{{"id":"t1","name":"Falcon","status":"running","tokenCount":31000,
                 "label":"reading src/render/agents.rs","startTime":{},
                 "model":"claude-opus-5","effort":"high"}}"#,
            NOW - 252_000
        ));
        let quiet = with_member("Falcon", "implement-deep", "purple", 0);
        let busy = with_member("Falcon", "implement-deep", "purple", 2);

        // (what changed, before, after, the fields that must not have moved)
        let cases: [(&str, String, String, &[&str]); 3] = [
            (
                "the stall marker arrives",
                at(&growing, 80),
                at(&stalled_row(), 80),
                &["Kite", "9k", "1m0s"],
            ),
            (
                "the inbox fills",
                at_team(&falcon(), 80, &quiet),
                at_team(&falcon(), 80, &busy),
                // The badge with its padding, which is how `plain` renders it.
                &["Falcon", " implement-deep ", "31k", "4m12s"],
            ),
            (
                "the activity re-widths",
                plain(&row(&falcon(), Style::Rich, 80, NOW, &no_team())),
                plain(&row(&verbose, Style::Rich, 80, NOW, &no_team())),
                &["Falcon", "opus-5 high", "31k", "4m12s"],
            ),
        ];

        for (case, before, after, settled) in cases {
            assert_ne!(before, after, "{case}: the pair must actually differ");
            for field in settled {
                let (b, a) = (column_of(&before, field), column_of(&after, field));
                assert!(b.is_some(), "{case}: {field:?} is missing from {before:?}");
                assert_eq!(
                    b, a,
                    "{case}: {field:?} changed column\n  before: {before:?}\n  after:  {after:?}"
                );
            }
        }
    }

    /// An omitted id keeps a row's default rendering; empty content *hides* the
    /// row. A vanished row is worse than an overflowing one, so no real task may
    /// ever render to nothing.
    #[test]
    fn a_row_is_never_empty_at_any_width() {
        for (label, t, team) in width_fixtures() {
            for style in [Style::Tiers, Style::Rich] {
                for cols in 1..=95 {
                    let r = row(&t, style, cols, NOW, &team);
                    assert!(!r.is_empty(), "{label} ({style:?}) vanished at {cols} cols");
                }
            }
        }
    }

    #[test]
    fn no_tasks_means_no_output_at_all() {
        let payload: SubagentPayload =
            serde_json::from_str(r#"{"columns":80,"tasks":[]}"#).expect("payload parses");
        assert_eq!(render(&payload, Style::Tiers, None, NOW, &no_team()), "");
    }

    #[test]
    fn numbers_survive_even_when_the_label_cannot() {
        let t = falcon();
        for cols in 12..=80 {
            assert!(at(&t, cols).contains("31k"), "token count lost at {cols} cols");
        }
    }

    #[test]
    fn queued_and_paused_rows_say_so_instead_of_zero() {
        let t = task(r#"{"id":"t","label":"waiting","status":"pending","tokenCount":0}"#);
        assert_eq!(at(&t, 60), aligned("waiting", "queued", 60));
        let t = task(r#"{"id":"t","label":"held","status":"paused","tokenCount":900}"#);
        assert_eq!(at(&t, 60), aligned("held", "900 \u{b7} paused", 60));
    }

    #[test]
    fn a_finished_row_stops_counting() {
        let t = task(&format!(
            r#"{{"id":"c","name":"Wren","status":"completed","tokenCount":7000,"startTime":{}}}"#,
            NOW - 600_000
        ));
        assert_eq!(at(&t, 60), aligned("Wren", "7k \u{b7} done", 60));
    }

    #[test]
    fn a_multiline_progress_summary_stays_on_one_row() {
        let t = task(&format!(
            r#"{{"id":"t","name":"Wren","label":"reading src/main.rs\nthen  editing",
                 "status":"running","tokenCount":8000,"startTime":{}}}"#,
            NOW - 63_000
        ));
        let r = at(&t, 80);
        assert!(!r.contains('\n'), "row must be single-line: {r:?}");
        assert_eq!(
            r,
            aligned("Wren \u{b7} reading src/main.rs then editing", "8k \u{b7} 1m3s", 80)
        );
    }

    #[test]
    fn rich_style_adds_model_and_effort_when_it_fits() {
        let t = falcon();
        assert_eq!(
            plain(&row(&t, Style::Rich, 80, NOW, &no_team())),
            aligned("Falcon \u{b7} opus-5 high \u{b7} research-deep", "31k \u{b7} 4m12s", 80)
        );
        // Model/effort outranks only the activity, so it is shed second.
        assert_eq!(
            plain(&row(&t, Style::Rich, 30, NOW, &no_team())),
            aligned("Falcon", "31k \u{b7} 4m12s", 30)
        );
    }

    /// `contextWindowSize` earns its place on the wire type: `rich` renders the
    /// ratio it exists for — the same reading the main status line gives for the
    /// session, now per teammate.
    #[test]
    fn rich_style_shows_how_full_the_context_window_is() {
        let t = context_row();
        assert_eq!(
            plain(&row(&t, Style::Rich, 80, NOW, &no_team())),
            aligned("Osprey \u{b7} opus-5", "124k \u{b7} 62% \u{b7} 1m35s", 80)
        );
        // `tiers` was specified as the raw token count WITHOUT a window ratio:
        // no percentage, no model either.
        assert_eq!(at(&t, 80), aligned("Osprey", "124k \u{b7} 1m35s", 80));
        // Shed order among rich's two extras: the static spec goes first, then
        // the reading — which is a restatement of a count that is never shed at
        // all, so it gives ground before the runtime nothing else reports.
        assert_eq!(
            plain(&row(&t, Style::Rich, 30, NOW, &no_team())),
            aligned("Osprey", "124k \u{b7} 62% \u{b7} 1m35s", 30)
        );
        assert_eq!(
            plain(&row(&t, Style::Rich, 24, NOW, &no_team())),
            aligned("Osprey", "124k \u{b7} 1m35s", 24)
        );
    }

    /// A window we cannot divide by is no reading at all — better a missing
    /// segment than a confident `0%`.
    #[test]
    fn a_missing_or_zero_context_window_renders_no_percentage() {
        for json in [
            r#"{"id":"c","name":"Osprey","status":"running","tokenCount":124000}"#,
            r#"{"id":"c","name":"Osprey","status":"running","tokenCount":124000,
                "contextWindowSize":0}"#,
            r#"{"id":"c","name":"Osprey","status":"running","tokenCount":0,
                "contextWindowSize":200000}"#,
        ] {
            let out = plain(&row(&task(json), Style::Rich, 80, NOW, &no_team()));
            assert!(!out.contains('%'), "unusable window still rendered a ratio: {out:?}");
        }
        // Counts sampled independently upstream can momentarily exceed the
        // window; the reading clamps rather than reporting 104%.
        let over = task(
            r#"{"id":"c","name":"Osprey","status":"running","tokenCount":208000,
                "contextWindowSize":200000}"#,
        );
        assert!(plain(&row(&over, Style::Rich, 80, NOW, &no_team())).contains("100%"));
    }

    #[test]
    fn a_nameless_labelless_task_is_still_identifiable() {
        let t = task(r#"{"id":"t","type":"local_bash","status":"running","tokenCount":0}"#);
        assert_eq!(at(&t, 40), "local_bash");
    }

    #[test]
    fn output_is_one_ndjson_line_per_task_with_escaped_content() {
        let payload: SubagentPayload = serde_json::from_str(
            r#"{"columns":80,"tasks":[
                 {"id":"a","label":"say \"hi\"","status":"running","tokenCount":1000,"startTime":1},
                 {"id":"b","label":"other","status":"running","tokenCount":2000,"startTime":1}]}"#,
        )
        .unwrap();
        let out = render(&payload, Style::Tiers, None, 0, &no_team());
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 2);
        for line in &lines {
            let v: serde_json::Value = serde_json::from_str(line).expect("valid NDJSON");
            assert!(v.get("id").is_some() && v.get("content").is_some());
        }
        assert!(lines[0].contains(r#"say \"hi\""#), "quotes must be escaped: {}", lines[0]);
    }


    #[test]
    fn rows_carry_no_colour_beyond_the_badge() {
        let out = row(&falcon(), Style::Tiers, 80, NOW, &no_team());
        assert!(!out.contains('\u{1b}'), "an un-badged row must be bare text: {out:?}");
    }
}
