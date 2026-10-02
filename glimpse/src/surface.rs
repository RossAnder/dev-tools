//! The surfaces switched with the digit keys, the per-surface item state (cursor, marks and
//! arrivals), and the Inbox's state over the input store.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::ops::{Deref, DerefMut};
use std::time::Instant;

use crate::app::FLASH;
use crate::ledger::{Anchor, EFFORTS, InputRow, ItemRow, Kind, SEVERITIES, Seen, StatusClass};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub(crate) enum Surface {
    #[default]
    Tasks,
    Review,
    Optimise,
    PlanReview,
    Backlog,
    Inbox,
}

impl Surface {
    /// In digit order: `ALL[n]` is the surface on key `n + 1`.
    pub(crate) const ALL: [Surface; 6] = [
        Self::Tasks,
        Self::Review,
        Self::Optimise,
        Self::PlanReview,
        Self::Backlog,
        Self::Inbox,
    ];

    pub(crate) fn from_digit(digit: char) -> Option<Surface> {
        let n = digit.to_digit(10)? as usize;
        n.checked_sub(1).and_then(|i| Self::ALL.get(i).copied())
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn digit(self) -> char {
        let index = Self::ALL.iter().position(|s| *s == self).unwrap_or(0);
        char::from(b'1' + index as u8)
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Tasks => "Tasks",
            Self::Review => "Review",
            Self::Optimise => "Optimise",
            Self::PlanReview => "Plan-review",
            Self::Backlog => "Backlog",
            Self::Inbox => "Inbox",
        }
    }

    /// The stable name `--surface` takes and `state.toml` saves; never the display label,
    /// so relabelling a tab cannot orphan a saved choice.
    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::Tasks => "tasks",
            Self::Review => "review",
            Self::Optimise => "optimise",
            Self::PlanReview => "plan-review",
            Self::Backlog => "backlog",
            Self::Inbox => "inbox",
        }
    }

    pub(crate) fn from_key(key: &str) -> Option<Surface> {
        Self::ALL.into_iter().find(|surface| surface.key() == key)
    }

    /// The ledger an item surface lists; `None` for Tasks and Inbox.
    pub(crate) fn ledger_kind(self) -> Option<Kind> {
        Some(match self {
            Self::Review => Kind::Review,
            Self::Optimise => Kind::Optimise,
            Self::PlanReview => Kind::PlanReview,
            Self::Backlog => Kind::Backlog,
            Self::Tasks | Self::Inbox => return None,
        })
    }

    /// The item surface that lists `kind`'s ledger.
    pub(crate) fn of_kind(kind: Kind) -> Option<Surface> {
        Self::ALL
            .into_iter()
            .find(|surface| surface.ledger_kind() == Some(kind))
    }

    /// The group-by options `g` cycles through, starting at `Group::None`.
    /// Each lists only the fields that surface's rows carry.
    pub(crate) fn groups(self) -> &'static [Group] {
        match self {
            Self::Review | Self::Optimise => &[
                Group::None,
                Group::Severity,
                Group::Category,
                Group::Effort,
                Group::File,
                Group::Status,
            ],
            Self::PlanReview => &[Group::None, Group::Severity, Group::Category, Group::Status],
            Self::Backlog => &[Group::None, Group::Kind, Group::Area, Group::Status],
            Self::Tasks | Self::Inbox => &[Group::None],
        }
    }

    /// The sort orders `S` cycles through, starting at `Sort::Id`.
    pub(crate) fn sorts(self) -> &'static [Sort] {
        match self {
            Self::Review | Self::Optimise => {
                &[Sort::Id, Sort::Severity, Sort::Effort, Sort::Newest]
            }
            Self::PlanReview => &[Sort::Id, Sort::Severity, Sort::Newest],
            Self::Backlog => &[Sort::Id, Sort::Newest],
            Self::Tasks | Self::Inbox => &[Sort::Id],
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub(crate) enum Group {
    #[default]
    None,
    Severity,
    Category,
    Effort,
    File,
    Status,
    Kind,
    Area,
}

impl Group {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Severity => "severity",
            Self::Category => "category",
            Self::Effort => "effort",
            Self::File => "file",
            Self::Status => "status",
            Self::Kind => "kind",
            Self::Area => "area",
        }
    }

    /// The stable name `state.toml` saves, independent of the label.
    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Severity => "severity",
            Self::Category => "category",
            Self::Effort => "effort",
            Self::File => "file",
            Self::Status => "status",
            Self::Kind => "kind",
            Self::Area => "area",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub(crate) enum Sort {
    #[default]
    Id,
    Severity,
    Effort,
    Newest,
}

impl Sort {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Id => "id",
            Self::Severity => "severity",
            Self::Effort => "effort",
            Self::Newest => "newest",
        }
    }

    /// The stable name `state.toml` saves, independent of the label.
    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::Id => "id",
            Self::Severity => "severity",
            Self::Effort => "effort",
            Self::Newest => "newest",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum VisibleRow {
    Header { label: String, count: usize },
    Item(String),
}

/// The rows of a refreshed list and what the user has seen of them: the part
/// [`ItemsState`] and [`InboxState`] share. Keyed by row id, so a refresh keeps
/// the cursor and saving set; the owner supplies the visible order.
#[derive(Debug, Clone, Default)]
pub(crate) struct Tracking<R> {
    pub(crate) rows: Vec<R>,
    pub(crate) revision: Seen,
    pub(crate) cursor: Option<String>,
    /// Row id to the instant it appeared or changed status; live for [`FLASH`].
    pub(crate) flashes: HashMap<String, Instant>,
    /// Rows that appeared or changed status while the surface was not on screen.
    pub(crate) new_since_view: usize,
    /// Ids with a write in flight, cleared when a read shows the row changed.
    pub(crate) saving: BTreeSet<String>,
}

impl<R: Tracked> Tracking<R> {
    fn is_current(&self, revision: &Option<String>) -> bool {
        self.revision.is(revision.as_deref())
    }

    /// Swaps in a read the caller has checked is not [`Tracking::is_current`]. The
    /// first read flashes nothing; later reads flash each new id and each status
    /// change, never moving the cursor to them. `true` when the cursor's row vanished,
    /// so the caller repositions it in its own visible order.
    fn refresh(
        &mut self,
        rows: Vec<R>,
        revision: Option<String>,
        viewing: bool,
        now: Instant,
    ) -> bool {
        let first = !self.revision.is_read();
        let (arrived, changed) = arrivals(&self.rows, &rows);

        self.rows = rows;
        self.revision = Seen::read(revision.as_deref());
        let present: BTreeSet<&str> = self.rows.iter().map(|r| r.id()).collect();
        self.saving
            .retain(|id| present.contains(id.as_str()) && !changed.contains(id));
        self.flashes.retain(|id, _| present.contains(id.as_str()));
        let cursor_present = self
            .cursor
            .as_deref()
            .is_some_and(|id| present.contains(id));

        if !first {
            if !viewing {
                self.new_since_view += arrived.len();
            }
            for id in arrived {
                self.flashes.insert(id, now);
            }
        }
        self.expire_flashes(now);
        !cursor_present
    }

    /// Called when the surface comes on screen.
    pub(crate) fn viewed(&mut self) {
        self.new_since_view = 0;
    }

    pub(crate) fn is_flashing(&self, id: &str, now: Instant) -> bool {
        self.flashes
            .get(id)
            .is_some_and(|at| now.saturating_duration_since(*at) < FLASH)
    }

    pub(crate) fn expire_flashes(&mut self, now: Instant) {
        self.flashes
            .retain(|_, at| now.saturating_duration_since(*at) < FLASH);
    }

    pub(crate) fn row(&self, id: &str) -> Option<&R> {
        self.rows.iter().find(|row| row.id() == id)
    }

    pub(crate) fn cursor_row(&self) -> Option<&R> {
        self.cursor.as_deref().and_then(|id| self.row(id))
    }

    /// Moves the cursor `delta` ids through `items`, clamped to its ends. A cursor
    /// not in `items` lands on the first.
    fn step_cursor(&mut self, items: &[String], delta: isize) {
        if items.is_empty() {
            return;
        }
        let next = match self.position_in(items) {
            Some(index) => index.saturating_add_signed(delta).min(items.len() - 1),
            None => 0,
        };
        self.cursor = items.get(next).cloned();
    }

    fn position_in(&self, items: &[String]) -> Option<usize> {
        let cursor = self.cursor.as_deref()?;
        items.iter().position(|id| id == cursor)
    }
}

/// One item surface's rows and the user's view of them. Every field is keyed
/// by row id, so a ledger refresh keeps the cursor, marks and saving set.
#[derive(Debug, Clone, Default)]
pub(crate) struct ItemsState {
    pub(crate) surface: Surface,
    /// Reached through `Deref`, so `state.rows` and `state.cursor` read as its own fields.
    pub(crate) tracking: Tracking<ItemRow>,
    pub(crate) marks: BTreeSet<String>,
    pub(crate) filter: String,
    pub(crate) group: Group,
    pub(crate) sort: Sort,
    pub(crate) show_closed: bool,
}

impl Deref for ItemsState {
    type Target = Tracking<ItemRow>;
    fn deref(&self) -> &Tracking<ItemRow> {
        &self.tracking
    }
}

impl DerefMut for ItemsState {
    fn deref_mut(&mut self) -> &mut Tracking<ItemRow> {
        &mut self.tracking
    }
}

impl ItemsState {
    pub(crate) fn new(surface: Surface) -> ItemsState {
        ItemsState {
            surface,
            ..ItemsState::default()
        }
    }

    /// Swaps in a fresh read of the ledger, flashing as [`Tracking`] does. Marks on
    /// vanished rows drop, and a cursor whose row vanished lands on the row that
    /// took its visible position.
    pub(crate) fn apply_ledger(
        &mut self,
        rows: Vec<ItemRow>,
        revision: Option<String>,
        viewing: bool,
        now: Instant,
    ) {
        if self.tracking.is_current(&revision) {
            return;
        }
        let old_position = self.cursor_position();
        let cursor_lost = self.tracking.refresh(rows, revision, viewing, now);
        let present: BTreeSet<&str> = self.tracking.rows.iter().map(|r| r.id.as_str()).collect();
        self.marks.retain(|id| present.contains(id.as_str()));
        if cursor_lost {
            self.tracking.cursor = reposition(self.visible_ids(), old_position);
        }
    }

    /// Inherent so `ItemsState::cursor_row` works as a path.
    pub(crate) fn cursor_row(&self) -> Option<&ItemRow> {
        self.tracking.cursor_row()
    }

    /// The rows the list draws, after the filter and the closed toggle, grouped
    /// and sorted. Group headers appear only when grouping is on.
    pub(crate) fn visible(&self) -> Vec<VisibleRow> {
        let mut shown: Vec<&ItemRow> = self.rows.iter().filter(|r| self.shows(r)).collect();
        shown.sort_by(|a, b| compare(self.sort, a, b));
        if self.group == Group::None {
            return shown
                .into_iter()
                .map(|r| VisibleRow::Item(r.id.clone()))
                .collect();
        }
        let mut groups: BTreeMap<(GroupRank, String), Vec<&ItemRow>> = BTreeMap::new();
        for row in shown {
            groups
                .entry(group_key(self.group, row))
                .or_default()
                .push(row);
        }
        let mut out = Vec::new();
        for ((_, label), members) in groups {
            out.push(VisibleRow::Header {
                label,
                count: members.len(),
            });
            out.extend(members.into_iter().map(|r| VisibleRow::Item(r.id.clone())));
        }
        out
    }

    /// Moves the cursor `delta` item rows through [`ItemsState::visible`],
    /// clamped to its ends. A cursor that is not visible lands on the first row.
    pub(crate) fn move_cursor(&mut self, delta: isize) {
        let items = self.visible_ids();
        self.tracking.step_cursor(&items, delta);
    }

    /// Toggles the cursor row's mark, then advances the cursor. A read-only row
    /// is never marked, since no write can address it.
    pub(crate) fn toggle_mark(&mut self) {
        if let Some(row) = self.cursor_row().filter(|r| !r.read_only) {
            let id = row.id.clone();
            if !self.marks.remove(&id) {
                self.marks.insert(id);
            }
        }
        self.move_cursor(1);
    }

    pub(crate) fn mark_visible(&mut self) {
        let ids: Vec<String> = self
            .visible_ids()
            .into_iter()
            .filter(|id| self.row(id).is_some_and(|r| !r.read_only))
            .collect();
        self.marks.extend(ids);
    }

    pub(crate) fn clear_marks(&mut self) {
        self.marks.clear();
    }

    /// The ids an action acts on: the marks, or else the cursor row.
    pub(crate) fn targets(&self) -> Vec<String> {
        if self.marks.is_empty() {
            self.cursor_row()
                .map(|r| r.id.clone())
                .into_iter()
                .collect()
        } else {
            self.marks.iter().cloned().collect()
        }
    }

    pub(crate) fn cycle_group(&mut self) {
        self.group = next_of(self.surface.groups(), self.group);
    }

    pub(crate) fn cycle_sort(&mut self) {
        self.sort = next_of(self.surface.sorts(), self.sort);
    }

    fn shows(&self, row: &ItemRow) -> bool {
        if !self.show_closed && matches!(row.class, StatusClass::Done | StatusClass::Declined) {
            return false;
        }
        if self.filter.is_empty() {
            return true;
        }
        let needle = self.filter.to_lowercase();
        let place = match &row.anchor {
            Anchor::Code { file, .. } => file.as_str(),
            Anchor::Area(area) => area.as_str(),
            Anchor::Section(_) | Anchor::None => "",
        };
        [row.id.as_str(), row.summary.as_str(), place]
            .iter()
            .any(|field| field.to_lowercase().contains(&needle))
    }

    fn visible_ids(&self) -> Vec<String> {
        item_ids(self.visible())
    }

    fn cursor_position(&self) -> Option<usize> {
        self.tracking.position_in(&self.visible_ids())
    }
}

/// The Inbox: unanswered agent questions first, then the other records grouped
/// by status. Keyed by record id like [`ItemsState`], so a store refresh keeps
/// the cursor and saving set.
#[derive(Debug, Clone, Default)]
pub(crate) struct InboxState {
    /// Reached through `Deref`, so `state.rows` and `state.cursor` read as its own fields.
    pub(crate) tracking: Tracking<InputRow>,
    /// Shows `handled` and `withdrawn` records.
    pub(crate) show_closed: bool,
}

impl Deref for InboxState {
    type Target = Tracking<InputRow>;
    fn deref(&self) -> &Tracking<InputRow> {
        &self.tracking
    }
}

impl DerefMut for InboxState {
    fn deref_mut(&mut self) -> &mut Tracking<InputRow> {
        &mut self.tracking
    }
}

const QUESTIONS_LABEL: &str = "questions";

impl InboxState {
    /// Swaps in a fresh read of the store, flashing each new record and each
    /// lifecycle change exactly as [`ItemsState::apply_ledger`] does for rows.
    pub(crate) fn apply_inputs(
        &mut self,
        rows: Vec<InputRow>,
        revision: Option<String>,
        viewing: bool,
        now: Instant,
    ) {
        if self.tracking.is_current(&revision) {
            return;
        }
        let old_position = self.cursor_position();
        if self.tracking.refresh(rows, revision, viewing, now) {
            self.tracking.cursor = reposition(self.visible_ids(), old_position);
        }
    }

    /// The questions still waiting on the user, for the Inbox tab's count.
    pub(crate) fn unanswered(&self) -> usize {
        self.rows
            .iter()
            .filter(|r| r.is_unanswered_question())
            .count()
    }

    /// The pending records that target `item` in a `ledger` ledger, in id order.
    /// Records carry no flow or scope match here, so an id shared by two flows'
    /// ledgers finds both flows' records.
    pub(crate) fn pending_for(&self, ledger: Kind, item: &str) -> Vec<&InputRow> {
        let mut found: Vec<&InputRow> = self
            .rows
            .iter()
            .filter(|r| {
                r.is_pending()
                    && r.ledger_kind() == Some(ledger)
                    && r.items.iter().any(|id| id == item)
            })
            .collect();
        found.sort_by(|a, b| id_key(&a.id).cmp(&id_key(&b.id)));
        found
    }

    /// The cursor record when it is a question the user can answer.
    pub(crate) fn cursor_question(&self) -> Option<&InputRow> {
        self.cursor_row()
            .filter(|r| !r.read_only && r.is_unanswered_question())
    }

    /// The cursor record when the user can withdraw it: their own and still `new`.
    pub(crate) fn cursor_withdrawable(&self) -> Option<&InputRow> {
        self.cursor_row()
            .filter(|r| !r.read_only && r.author == "user" && r.status == "new")
    }

    /// The rows the Inbox draws, each section under a header.
    pub(crate) fn visible(&self) -> Vec<VisibleRow> {
        let mut groups: BTreeMap<(u8, String), Vec<&InputRow>> = BTreeMap::new();
        for row in &self.rows {
            if !self.show_closed && matches!(row.status.as_str(), "handled" | "withdrawn") {
                continue;
            }
            groups.entry(inbox_section(row)).or_default().push(row);
        }
        let mut out = Vec::new();
        for ((_, label), mut members) in groups {
            members.sort_by(|a, b| id_key(&a.id).cmp(&id_key(&b.id)));
            out.push(VisibleRow::Header {
                label,
                count: members.len(),
            });
            out.extend(members.into_iter().map(|r| VisibleRow::Item(r.id.clone())));
        }
        out
    }

    pub(crate) fn move_cursor(&mut self, delta: isize) {
        let items = self.visible_ids();
        self.tracking.step_cursor(&items, delta);
    }

    fn visible_ids(&self) -> Vec<String> {
        item_ids(self.visible())
    }

    fn cursor_position(&self) -> Option<usize> {
        self.tracking.position_in(&self.visible_ids())
    }
}

/// The Inbox section a record falls in, ordered by rank; an unknown status
/// sorts after the known ones under its own label.
fn inbox_section(row: &InputRow) -> (u8, String) {
    if row.is_unanswered_question() {
        return (0, QUESTIONS_LABEL.to_owned());
    }
    let rank = match row.status.as_str() {
        "new" => 1,
        "acknowledged" => 2,
        "handled" => 3,
        "withdrawn" => 4,
        "" => return (u8::MAX, EMPTY_LABEL.to_owned()),
        _ => 5,
    };
    (rank, row.status.clone())
}

/// A row whose arrival or status change an item list flashes.
pub(crate) trait Tracked: PartialEq {
    fn id(&self) -> &str;
    fn status(&self) -> &str;
}

impl Tracked for ItemRow {
    fn id(&self) -> &str {
        &self.id
    }
    fn status(&self) -> &str {
        &self.status
    }
}

impl Tracked for InputRow {
    fn id(&self) -> &str {
        &self.id
    }
    fn status(&self) -> &str {
        &self.status
    }
}

/// The ids in `new` that are new or changed status since `old` (they flash),
/// and the ids whose row differs at all (their saving mark clears).
fn arrivals<R: Tracked>(old: &[R], new: &[R]) -> (Vec<String>, BTreeSet<String>) {
    let old: HashMap<&str, &R> = old.iter().map(|r| (r.id(), r)).collect();
    let mut arrived = Vec::new();
    let mut changed = BTreeSet::new();
    for row in new {
        match old.get(row.id()) {
            None => {
                arrived.push(row.id().to_owned());
                changed.insert(row.id().to_owned());
            }
            Some(prev) => {
                if prev.status() != row.status() {
                    arrived.push(row.id().to_owned());
                }
                if *prev != row {
                    changed.insert(row.id().to_owned());
                }
            }
        }
    }
    (arrived, changed)
}

/// The id at the cursor's old visible position, clamped to the end, or the
/// first id when there was no position.
fn reposition(items: Vec<String>, old_position: Option<usize>) -> Option<String> {
    match old_position {
        Some(index) if !items.is_empty() => Some(items[index.min(items.len() - 1)].clone()),
        _ => items.into_iter().next(),
    }
}

fn item_ids(visible: Vec<VisibleRow>) -> Vec<String> {
    visible
        .into_iter()
        .filter_map(|row| match row {
            VisibleRow::Item(id) => Some(id),
            VisibleRow::Header { .. } => None,
        })
        .collect()
}

/// Orders groups by vocabulary rank, then by label; an empty value groups last.
type GroupRank = u8;

const EMPTY_LABEL: &str = "(none)";

fn group_key(group: Group, row: &ItemRow) -> (GroupRank, String) {
    let file = match &row.anchor {
        Anchor::Code { file, .. } => file.as_str(),
        _ => "",
    };
    let area = match &row.anchor {
        Anchor::Area(area) => area.as_str(),
        _ => "",
    };
    let (rank, value) = match group {
        Group::None => (0, ""),
        Group::Severity => (severity_rank(&row.severity), row.severity.as_str()),
        Group::Effort => (effort_rank(&row.effort), row.effort.as_str()),
        Group::Status => (row.class as u8, row.status.as_str()),
        Group::Category => (0, row.category.as_str()),
        Group::File => (0, file),
        Group::Kind => (0, row.kind.as_str()),
        Group::Area => (0, area),
    };
    if value.is_empty() {
        (u8::MAX, EMPTY_LABEL.to_owned())
    } else {
        (rank, value.to_owned())
    }
}

/// A value's position in its ordered vocabulary; an unknown value ranks after every known one.
fn rank_in(vocabulary: &[&str], value: &str) -> u8 {
    let position = vocabulary.iter().position(|known| *known == value);
    u8::try_from(position.unwrap_or(vocabulary.len())).unwrap_or(u8::MAX)
}

fn severity_rank(severity: &str) -> u8 {
    rank_in(&SEVERITIES, severity)
}

fn effort_rank(effort: &str) -> u8 {
    rank_in(&EFFORTS, effort)
}

fn compare(sort: Sort, a: &ItemRow, b: &ItemRow) -> Ordering {
    let primary = match sort {
        Sort::Id => Ordering::Equal,
        Sort::Severity => severity_rank(&a.severity).cmp(&severity_rank(&b.severity)),
        Sort::Effort => effort_rank(&a.effort).cmp(&effort_rank(&b.effort)),
        Sort::Newest => b.created.cmp(&a.created),
    };
    primary.then_with(|| id_key(&a.id).cmp(&id_key(&b.id)))
}

/// Splits an id into its prefix and trailing number, so `R10` sorts after `R2`.
fn id_key(id: &str) -> (&str, Option<u64>, &str) {
    let split = id.trim_end_matches(|c: char| c.is_ascii_digit()).len();
    let (prefix, digits) = id.split_at(split);
    (prefix, digits.parse().ok(), id)
}

fn next_of<T: Copy + PartialEq>(options: &[T], current: T) -> T {
    let index = options.iter().position(|o| *o == current);
    match index {
        Some(i) => options[(i + 1) % options.len()],
        None => options.first().copied().unwrap_or(current),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn item(id: &str, status: &str, severity: &str) -> ItemRow {
        ItemRow {
            id: id.to_owned(),
            status: status.to_owned(),
            class: StatusClass::of(status),
            severity: severity.to_owned(),
            ..ItemRow::default()
        }
    }

    fn rev(n: u32) -> Option<String> {
        Some(format!("rev{n}"))
    }

    fn items(state: &ItemsState) -> Vec<String> {
        state.visible_ids()
    }

    fn loaded(rows: Vec<ItemRow>) -> ItemsState {
        let mut state = ItemsState::new(Surface::Review);
        state.apply_ledger(rows, rev(0), true, Instant::now());
        state
    }

    #[test]
    fn digits_map_to_surfaces_in_order() {
        assert_eq!(Surface::from_digit('1'), Some(Surface::Tasks));
        assert_eq!(Surface::from_digit('6'), Some(Surface::Inbox));
        assert_eq!(Surface::from_digit('0'), None);
        assert_eq!(Surface::from_digit('7'), None);
        for surface in Surface::ALL {
            assert_eq!(Surface::from_digit(surface.digit()), Some(surface));
        }
        assert_eq!(Surface::PlanReview.label(), "Plan-review");
    }

    #[test]
    fn saved_keys_stay_fixed() {
        let surfaces = Surface::ALL.map(Surface::key);
        assert_eq!(
            surfaces,
            [
                "tasks",
                "review",
                "optimise",
                "plan-review",
                "backlog",
                "inbox"
            ]
        );
        for surface in Surface::ALL {
            assert_eq!(Surface::from_key(surface.key()), Some(surface));
        }
        assert_eq!(Surface::from_key("Review"), None);
        let groups = [
            Group::None,
            Group::Severity,
            Group::Category,
            Group::Effort,
            Group::File,
            Group::Status,
            Group::Kind,
            Group::Area,
        ]
        .map(Group::key);
        assert_eq!(
            groups,
            [
                "none", "severity", "category", "effort", "file", "status", "kind", "area"
            ]
        );
        let sorts = [Sort::Id, Sort::Severity, Sort::Effort, Sort::Newest].map(Sort::key);
        assert_eq!(sorts, ["id", "severity", "effort", "newest"]);
    }

    #[test]
    fn a_new_row_flashes_and_keeps_the_cursor() {
        let mut state = loaded(vec![
            item("R1", "open", "warning"),
            item("R2", "open", "warning"),
        ]);
        assert!(state.flashes.is_empty(), "the first read flashes nothing");
        state.move_cursor(1);
        assert_eq!(state.cursor.as_deref(), Some("R2"));

        let t = Instant::now();
        state.apply_ledger(
            vec![
                item("R0", "open", "warning"),
                item("R1", "fixed", "warning"),
                item("R2", "open", "warning"),
            ],
            rev(1),
            false,
            t,
        );
        assert_eq!(state.cursor.as_deref(), Some("R2"));
        assert!(state.is_flashing("R0", t));
        assert!(state.is_flashing("R1", t), "a status change flashes");
        assert!(!state.is_flashing("R2", t));
        assert!(!state.is_flashing("R0", t + FLASH + Duration::from_millis(1)));
        assert_eq!(state.new_since_view, 2);
        state.viewed();
        assert_eq!(state.new_since_view, 0);
    }

    #[test]
    fn an_unchanged_revision_is_a_no_op() {
        let mut state = loaded(vec![item("R1", "open", "warning")]);
        state.apply_ledger(
            vec![item("R9", "open", "warning")],
            rev(0),
            false,
            Instant::now(),
        );
        assert_eq!(items(&state), ["R1"]);
        assert_eq!(state.new_since_view, 0);
    }

    #[test]
    fn a_vanished_cursor_takes_the_row_at_its_position() {
        let mut state = loaded(vec![
            item("R1", "open", "warning"),
            item("R2", "open", "warning"),
            item("R3", "open", "warning"),
        ]);
        state.move_cursor(1);
        state.apply_ledger(
            vec![item("R1", "open", "warning"), item("R3", "open", "warning")],
            rev(1),
            true,
            Instant::now(),
        );
        assert_eq!(state.cursor.as_deref(), Some("R3"));
    }

    #[test]
    fn marks_survive_a_ledger_refresh_but_drop_vanished_ids() {
        let mut state = loaded(vec![
            item("R1", "open", "warning"),
            item("R2", "open", "warning"),
            item("R3", "open", "warning"),
        ]);
        state.toggle_mark();
        state.toggle_mark();
        assert_eq!(state.cursor.as_deref(), Some("R3"), "marking advances");
        state.apply_ledger(
            vec![
                item("R1", "open", "critical"),
                item("R3", "open", "warning"),
            ],
            rev(1),
            true,
            Instant::now(),
        );
        assert_eq!(state.marks.iter().collect::<Vec<_>>(), ["R1"]);
    }

    #[test]
    fn saving_clears_only_for_rows_that_changed() {
        let mut state = loaded(vec![
            item("R1", "open", "warning"),
            item("R2", "open", "warning"),
        ]);
        state.saving.extend(["R1".to_owned(), "R2".to_owned()]);
        state.apply_ledger(
            vec![
                item("R1", "deferred", "warning"),
                item("R2", "open", "warning"),
            ],
            rev(1),
            true,
            Instant::now(),
        );
        assert_eq!(state.saving.iter().collect::<Vec<_>>(), ["R2"]);
    }

    #[test]
    fn grouping_by_severity_orders_critical_first() {
        let mut state = loaded(vec![
            item("R1", "open", "suggestion"),
            item("R2", "open", ""),
            item("R3", "open", "critical"),
            item("R4", "open", "warning"),
            item("R5", "open", "critical"),
        ]);
        state.cycle_group();
        assert_eq!(state.group, Group::Severity);
        let header = |label: &str| VisibleRow::Header {
            label: label.to_owned(),
            count: if label == "critical" { 2 } else { 1 },
        };
        let row = |id: &str| VisibleRow::Item(id.to_owned());
        assert_eq!(
            state.visible(),
            [
                header("critical"),
                row("R3"),
                row("R5"),
                header("warning"),
                row("R4"),
                header("suggestion"),
                row("R1"),
                header(EMPTY_LABEL),
                row("R2"),
            ]
        );
    }

    #[test]
    fn closed_rows_are_hidden_until_toggled() {
        let mut state = loaded(vec![
            item("R1", "open", "warning"),
            item("R2", "fixed", "warning"),
            item("R3", "deferred", "warning"),
            item("R4", "wontfix", "warning"),
        ]);
        assert_eq!(items(&state), ["R1", "R3"]);
        state.show_closed = true;
        assert_eq!(items(&state), ["R1", "R2", "R3", "R4"]);
    }

    #[test]
    fn targets_fall_back_to_the_cursor_row() {
        let mut state = loaded(vec![
            item("R1", "open", "warning"),
            item("R2", "open", "warning"),
        ]);
        assert_eq!(state.targets(), ["R1"]);
        state.move_cursor(1);
        state.toggle_mark();
        state.move_cursor(-5);
        assert_eq!(state.targets(), ["R2"]);
        state.clear_marks();
        assert_eq!(state.targets(), ["R1"]);
    }

    #[test]
    fn ids_sort_numerically_and_the_filter_matches_summary() {
        let mut state = loaded(vec![
            item("R10", "open", "warning"),
            item("R2", "open", "warning"),
            ItemRow {
                summary: "Cache miss".into(),
                ..item("R3", "open", "warning")
            },
        ]);
        assert_eq!(items(&state), ["R2", "R3", "R10"]);
        state.filter = "cache".into();
        assert_eq!(items(&state), ["R3"]);
    }

    #[test]
    fn read_only_rows_are_never_marked() {
        let mut state = loaded(vec![
            ItemRow {
                read_only: true,
                ..item("#1", "open", "warning")
            },
            item("R2", "open", "warning"),
        ]);
        state.mark_visible();
        assert_eq!(state.marks.iter().collect::<Vec<_>>(), ["R2"]);
    }

    fn input(id: &str, kind: &str, author: &str, status: &str) -> InputRow {
        InputRow {
            id: id.to_owned(),
            kind: kind.to_owned(),
            author: author.to_owned(),
            status: status.to_owned(),
            ..InputRow::default()
        }
    }

    fn inbox(rows: Vec<InputRow>) -> InboxState {
        let mut state = InboxState::default();
        state.apply_inputs(rows, rev(0), true, Instant::now());
        state
    }

    #[test]
    fn unanswered_questions_sort_first() {
        let mut state = inbox(vec![
            input("I1", "request", "user", "acknowledged"),
            input("I2", "note", "user", "new"),
            input("I3", "question", "review", "handled"),
            input("I4", "answer", "user", "new"),
            input("I10", "question", "optimise", "new"),
            input("I5", "question", "review", "new"),
            input("I6", "capture", "user", "withdrawn"),
        ]);
        let header = |label: &str, count| VisibleRow::Header {
            label: label.to_owned(),
            count,
        };
        let row = |id: &str| VisibleRow::Item(id.to_owned());
        assert_eq!(
            state.visible(),
            [
                header(QUESTIONS_LABEL, 2),
                row("I5"),
                row("I10"),
                header("new", 2),
                row("I2"),
                row("I4"),
                header("acknowledged", 1),
                row("I1"),
            ]
        );
        assert_eq!(state.cursor.as_deref(), Some("I5"));
        assert_eq!(state.unanswered(), 2);
        assert!(state.cursor_question().is_some());
        assert!(state.cursor_withdrawable().is_none(), "an agent's question");
        state.move_cursor(2);
        assert_eq!(
            state.cursor_withdrawable().map(|r| r.id.as_str()),
            Some("I2")
        );

        state.show_closed = true;
        let ids = item_ids(state.visible());
        assert_eq!(ids, ["I5", "I10", "I2", "I4", "I1", "I3", "I6"]);
    }

    #[test]
    fn pending_inputs_are_found_by_item() {
        let on = |row: InputRow, ledger: &str, items: &[&str]| InputRow {
            ledger: ledger.to_owned(),
            items: items.iter().map(|s| (*s).to_owned()).collect(),
            ..row
        };
        let state = inbox(vec![
            on(input("I1", "note", "user", "new"), "review", &["R3", "R4"]),
            on(
                input("I2", "note", "user", "acknowledged"),
                "review",
                &["R3"],
            ),
            on(input("I8", "capture", "user", "new"), "", &["R3"]),
            on(input("I3", "note", "user", "handled"), "review", &["R3"]),
            on(input("I4", "note", "user", "withdrawn"), "review", &["R3"]),
            on(input("I5", "request", "user", "new"), "optimise", &["R3"]),
            on(input("I6", "request", "user", "new"), "review", &["R30"]),
            on(input("I7", "question", "review", "new"), "review", &["R3"]),
        ]);
        let ids = |ledger, item| -> Vec<String> {
            state
                .pending_for(ledger, item)
                .into_iter()
                .map(|r| r.id.clone())
                .collect()
        };
        assert_eq!(ids(Kind::Review, "R3"), ["I1", "I2", "I7"]);
        assert_eq!(ids(Kind::Review, "R4"), ["I1"]);
        assert_eq!(ids(Kind::Optimise, "R3"), ["I5"]);
        assert!(ids(Kind::Backlog, "R3").is_empty());
    }

    #[test]
    fn a_lifecycle_change_flashes_an_inbox_record() {
        let mut state = inbox(vec![
            input("I1", "request", "user", "new"),
            input("I2", "note", "user", "new"),
        ]);
        assert!(state.flashes.is_empty(), "the first read flashes nothing");
        state.saving.insert("I1".to_owned());
        let t = Instant::now();
        state.apply_inputs(
            vec![
                input("I1", "request", "user", "acknowledged"),
                input("I2", "note", "user", "new"),
                input("I3", "question", "review", "new"),
            ],
            rev(1),
            false,
            t,
        );
        assert!(state.is_flashing("I1", t));
        assert!(state.is_flashing("I3", t));
        assert!(!state.is_flashing("I2", t));
        assert!(state.saving.is_empty());
        assert_eq!(state.new_since_view, 2);
        assert_eq!(state.cursor.as_deref(), Some("I1"));
    }

    #[test]
    fn group_and_sort_cycle_through_the_surface_options() {
        let mut state = ItemsState::new(Surface::Backlog);
        let mut seen = Vec::new();
        for _ in 0..4 {
            state.cycle_group();
            seen.push(state.group);
        }
        assert_eq!(seen, [Group::Kind, Group::Area, Group::Status, Group::None]);
        state.cycle_sort();
        assert_eq!(state.sort, Sort::Newest);
        state.cycle_sort();
        assert_eq!(state.sort, Sort::Id);
    }
}
