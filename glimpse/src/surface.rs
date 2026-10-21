//! The surfaces switched with the digit keys, the per-surface item state (cursor, marks and
//! arrivals), and the Inbox's state over the input store.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::ops::{Deref, DerefMut};
use std::time::Instant;

use crate::app::{FLASH, Scroll};
use crate::config::ItemColumn;
use crate::ledger::{Anchor, EFFORTS, InputRow, ItemRow, Kind, SEVERITIES, Seen, StatusClass};
use crate::scroll;

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

    /// The sort orders `S` cycles through, starting at `Sort::Id`. Each lists only
    /// the fields that surface's rows carry. The Inbox sorts by [`InboxColumn`] instead.
    pub(crate) fn sorts(self) -> &'static [Sort] {
        match self {
            Self::Review | Self::Optimise => &[
                Sort::Id,
                Sort::Severity,
                Sort::Effort,
                Sort::Newest,
                Sort::Status,
                Sort::Category,
                Sort::Summary,
                Sort::Area,
            ],
            Self::PlanReview => &[
                Sort::Id,
                Sort::Severity,
                Sort::Newest,
                Sort::Status,
                Sort::Category,
                Sort::Summary,
                Sort::Area,
            ],
            Self::Backlog => &[
                Sort::Id,
                Sort::Newest,
                Sort::Kind,
                Sort::Status,
                Sort::Summary,
                Sort::Area,
            ],
            Self::Tasks | Self::Inbox => &[Sort::Id],
        }
    }

    /// The order a click on `column`'s header sorts by, when this surface offers one.
    /// The kind column shows a finding's severity, so it sorts by severity where the
    /// rows carry one.
    pub(crate) fn column_sort(self, column: ItemColumn) -> Option<Sort> {
        let sort = match column {
            ItemColumn::Status => Sort::Status,
            ItemColumn::Id => Sort::Id,
            ItemColumn::Kind if self.sorts().contains(&Sort::Severity) => Sort::Severity,
            ItemColumn::Kind => Sort::Kind,
            ItemColumn::Category => Sort::Category,
            ItemColumn::Effort => Sort::Effort,
            ItemColumn::Summary => Sort::Summary,
            ItemColumn::Area => Sort::Area,
        };
        self.sorts().contains(&sort).then_some(sort)
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
    Kind,
    Status,
    Category,
    Summary,
    Area,
}

impl Sort {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Id => "id",
            Self::Severity => "severity",
            Self::Effort => "effort",
            Self::Newest => "newest",
            Self::Kind => "kind",
            Self::Status => "status",
            Self::Category => "category",
            Self::Summary => "summary",
            Self::Area => "area",
        }
    }

    /// The stable name `state.toml` saves, independent of the label.
    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::Id => "id",
            Self::Severity => "severity",
            Self::Effort => "effort",
            Self::Newest => "newest",
            Self::Kind => "kind",
            Self::Status => "status",
            Self::Category => "category",
            Self::Summary => "summary",
            Self::Area => "area",
        }
    }
}

/// Which way a sort runs. `Desc` reverses the order's own comparison only: rows that
/// compare equal still follow ascending id, and blank text values stay last.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub(crate) enum SortDir {
    #[default]
    Asc,
    Desc,
}

impl SortDir {
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn label(self) -> &'static str {
        self.key()
    }

    /// The stable name `state.toml` saves, independent of the label.
    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::Asc => "asc",
            Self::Desc => "desc",
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn from_key(key: &str) -> Option<SortDir> {
        [Self::Asc, Self::Desc].into_iter().find(|d| d.key() == key)
    }

    pub(crate) fn flipped(self) -> SortDir {
        match self {
            Self::Asc => Self::Desc,
            Self::Desc => Self::Asc,
        }
    }

    fn apply(self, ordering: Ordering) -> Ordering {
        match self {
            Self::Asc => ordering,
            Self::Desc => ordering.reverse(),
        }
    }
}

/// A column of the Inbox's rows, each also the order its header sorts by.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub(crate) enum InboxColumn {
    #[default]
    Id,
    Kind,
    Status,
    /// A question's prompt, or another record's text.
    Text,
}

impl InboxColumn {
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) const ALL: [InboxColumn; 4] = [Self::Id, Self::Kind, Self::Status, Self::Text];

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn label(self) -> &'static str {
        self.key()
    }

    /// The stable name `state.toml` saves, independent of the label.
    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::Id => "id",
            Self::Kind => "kind",
            Self::Status => "status",
            Self::Text => "text",
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn from_key(key: &str) -> Option<InboxColumn> {
        Self::ALL.into_iter().find(|column| column.key() == key)
    }
}

/// A table header a click sorts by.
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum HeaderKey {
    Item(ItemColumn),
    Inbox(InboxColumn),
}

/// The next `(key, direction)` after a click on `clicked`'s header: another key sorts
/// by it ascending, and the same key steps asc → desc → `off`.
fn cycle<T: Copy + PartialEq>(current: (T, SortDir), clicked: T, off: T) -> (T, SortDir) {
    match current {
        (key, SortDir::Asc) if key == clicked => (clicked, SortDir::Desc),
        (key, SortDir::Desc) if key == clicked => (off, SortDir::Asc),
        _ => (clicked, SortDir::Asc),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum VisibleRow {
    Header { label: String, count: usize },
    Item(String),
}

/// A list's rows, readable as a slice anywhere but replaceable only by
/// [`Tracking::refresh`], so the row index, flashes and saving set cannot fall out of
/// step with them.
#[derive(Debug, Clone)]
pub(crate) struct Rows<R>(Vec<R>);

impl<R> Default for Rows<R> {
    fn default() -> Self {
        Rows(Vec::new())
    }
}

impl<R> Deref for Rows<R> {
    type Target = [R];
    fn deref(&self) -> &[R] {
        &self.0
    }
}

/// The rows of a refreshed list and what the user has seen of them: the part
/// [`ItemsState`] and [`InboxState`] share. Keyed by row id, so a refresh keeps
/// the cursor and saving set; the owner supplies the visible order.
#[derive(Debug, Clone, Default)]
pub(crate) struct Tracking<R> {
    pub(crate) rows: Rows<R>,
    pub(crate) revision: Seen,
    pub(crate) cursor: Option<String>,
    /// Row id to the instant it appeared or changed status; live for [`FLASH`].
    pub(crate) flashes: HashMap<String, Instant>,
    /// Rows that appeared or changed status while the surface was not on screen.
    pub(crate) new_since_view: usize,
    /// Ids with a write in flight, cleared when a read shows the row changed.
    pub(crate) saving: BTreeSet<String>,
    /// The first visible-list row the last frame drew, written back by the list view.
    pub(crate) scroll: usize,
    /// Set by the wheel: the list stays where the wheel left it, even with the cursor
    /// off screen, until the cursor or the visible order changes.
    pub(crate) scroll_pinned: bool,
    /// How many list rows the last frame drew, written back by the list view; a page move steps by it.
    pub(crate) page: usize,
    /// Row id to its first position in `rows`, rebuilt by [`Tracking::refresh`].
    index: HashMap<String, usize>,
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

        self.rows = Rows(rows);
        self.index.clear();
        for (i, row) in self.rows.iter().enumerate() {
            self.index.entry(row.id().to_string()).or_insert(i);
        }
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

    /// The first row with `id`.
    pub(crate) fn row(&self, id: &str) -> Option<&R> {
        self.index.get(id).map(|&i| &self.rows[i])
    }

    /// Loads `rows` as a first read of `revision` would, flashing nothing.
    #[cfg(test)]
    pub(crate) fn set_rows(&mut self, rows: Vec<R>, revision: Option<&str>) {
        self.revision = Seen::Unread;
        self.refresh(rows, revision.map(str::to_owned), true, Instant::now());
    }

    /// Scrolls the list `rows` down (negative: up) and pins it there. The view clamps
    /// the offset to the list it draws.
    pub(crate) fn scroll_by(&mut self, rows: i32) {
        let by = isize::try_from(rows).unwrap_or(0);
        self.scroll = self.scroll.saturating_add_signed(by);
        self.scroll_pinned = true;
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

    /// Moves the cursor through `items` by `scroll` and unpins the list. A cursor
    /// not in `items` lands on the first; an empty `items` leaves the cursor alone.
    fn page_through(&mut self, items: &[String], scroll: Scroll) {
        let at = self.position_in(items);
        if let Some(next) = scroll::step(at, items.len(), self.page, scroll) {
            self.cursor = items.get(next).cloned();
            self.scroll_pinned = false;
        }
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
    pub(crate) sort_dir: SortDir,
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
        let needle = self.filter.to_lowercase();
        let mut shown: Vec<&ItemRow> = self
            .rows
            .iter()
            .filter(|r| self.shows(r, &needle))
            .collect();
        shown.sort_by(|a, b| compare(self.sort, self.sort_dir, a, b));
        if self.group == Group::None {
            return shown
                .into_iter()
                .map(|r| VisibleRow::Item(r.id.clone()))
                .collect();
        }
        let mut groups: BTreeMap<(GroupRank, &str), Vec<&ItemRow>> = BTreeMap::new();
        for row in shown {
            groups
                .entry(group_key(self.group, row))
                .or_default()
                .push(row);
        }
        let mut out = Vec::new();
        for ((_, label), members) in groups {
            out.push(VisibleRow::Header {
                label: label.to_owned(),
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

    /// Moves the cursor through [`ItemsState::visible`] by `scroll`, a page being
    /// the rows the last frame drew.
    pub(crate) fn page_cursor(&mut self, scroll: Scroll) {
        let items = self.visible_ids();
        self.tracking.page_through(&items, scroll);
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

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn flip_dir(&mut self) {
        self.sort_dir = self.sort_dir.flipped();
    }

    /// Sorts by `column`'s header as [`cycle`] steps, off being `Sort::Id` ascending.
    /// `false`, changing nothing, for a column this surface cannot sort by.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn cycle_header(&mut self, column: ItemColumn) -> bool {
        let Some(clicked) = self.surface.column_sort(column) else {
            return false;
        };
        (self.sort, self.sort_dir) = cycle((self.sort, self.sort_dir), clicked, Sort::Id);
        true
    }

    /// Whether the rows are sorted by `column`, and which way, for its header's mark.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn header_sort(&self, column: ItemColumn) -> Option<SortDir> {
        (self.surface.column_sort(column) == Some(self.sort)).then_some(self.sort_dir)
    }

    /// `needle` is the lowercased filter.
    fn shows(&self, row: &ItemRow, needle: &str) -> bool {
        if !self.show_closed && matches!(row.class, StatusClass::Done | StatusClass::Declined) {
            return false;
        }
        if self.filter.is_empty() {
            return true;
        }
        let place = match &row.anchor {
            Anchor::Code { file, .. } => file.as_str(),
            Anchor::Area(area) => area.as_str(),
            Anchor::Section(_) | Anchor::None => "",
        };
        [row.id.as_str(), row.summary.as_str(), place]
            .iter()
            .any(|field| field.to_lowercase().contains(needle))
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
    /// The order within each section; the sections themselves keep their fixed order.
    pub(crate) sort: InboxColumn,
    pub(crate) sort_dir: SortDir,
    /// `(ledger, item id)` to the indices in `rows` of the pending records naming it, in
    /// id order; rebuilt by [`InboxState::apply_inputs`].
    pending: HashMap<(Kind, String), Vec<usize>>,
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
        let cursor_lost = self.tracking.refresh(rows, revision, viewing, now);
        self.index_pending();
        if cursor_lost {
            self.tracking.cursor = reposition(self.visible_ids(), old_position);
        }
    }

    fn index_pending(&mut self) {
        let mut pending: HashMap<(Kind, String), Vec<usize>> = HashMap::new();
        for (i, row) in self.tracking.rows.iter().enumerate() {
            let Some(kind) = row.ledger_kind().filter(|_| row.is_pending()) else {
                continue;
            };
            for (n, item) in row.items.iter().enumerate() {
                if !row.items[..n].contains(item) {
                    pending.entry((kind, item.clone())).or_default().push(i);
                }
            }
        }
        for indices in pending.values_mut() {
            indices.sort_by(|&a, &b| {
                id_key(&self.tracking.rows[a].id).cmp(&id_key(&self.tracking.rows[b].id))
            });
        }
        self.pending = pending;
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
        self.pending
            .get(&(ledger, item.to_owned()))
            .map(|indices| indices.iter().map(|&i| &self.tracking.rows[i]).collect())
            .unwrap_or_default()
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
        let mut groups: BTreeMap<(u8, &str), Vec<&InputRow>> = BTreeMap::new();
        for row in self.rows.iter() {
            if !self.show_closed && matches!(row.status.as_str(), "handled" | "withdrawn") {
                continue;
            }
            groups.entry(inbox_section(row)).or_default().push(row);
        }
        let mut out = Vec::new();
        for ((_, label), mut members) in groups {
            members.sort_by(|a, b| compare_input(self.sort, self.sort_dir, a, b));
            out.push(VisibleRow::Header {
                label: label.to_owned(),
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

    /// [`ItemsState::page_cursor`] for the Inbox.
    pub(crate) fn page_cursor(&mut self, scroll: Scroll) {
        let items = self.visible_ids();
        self.tracking.page_through(&items, scroll);
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn flip_dir(&mut self) {
        self.sort_dir = self.sort_dir.flipped();
    }

    /// [`ItemsState::cycle_header`] for the Inbox, off being id ascending.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn cycle_header(&mut self, column: InboxColumn) {
        (self.sort, self.sort_dir) = cycle((self.sort, self.sort_dir), column, InboxColumn::Id);
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn header_sort(&self, column: InboxColumn) -> Option<SortDir> {
        (self.sort == column).then_some(self.sort_dir)
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
fn inbox_section(row: &InputRow) -> (u8, &str) {
    if row.is_unanswered_question() {
        return (0, QUESTIONS_LABEL);
    }
    let rank = match row.status.as_str() {
        "new" => 1,
        "acknowledged" => 2,
        "handled" => 3,
        "withdrawn" => 4,
        "" => return (u8::MAX, EMPTY_LABEL),
        _ => 5,
    };
    (rank, row.status.as_str())
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

fn group_key(group: Group, row: &ItemRow) -> (GroupRank, &str) {
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
        (u8::MAX, EMPTY_LABEL)
    } else {
        (rank, value)
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

fn compare(sort: Sort, dir: SortDir, a: &ItemRow, b: &ItemRow) -> Ordering {
    let primary = match sort {
        Sort::Id => dir.apply(id_key(&a.id).cmp(&id_key(&b.id))),
        Sort::Severity => dir.apply(severity_rank(&a.severity).cmp(&severity_rank(&b.severity))),
        Sort::Effort => dir.apply(effort_rank(&a.effort).cmp(&effort_rank(&b.effort))),
        Sort::Newest => dir.apply(b.created.cmp(&a.created)),
        Sort::Status => dir.apply((a.class as u8, &a.status).cmp(&(b.class as u8, &b.status))),
        Sort::Kind => compare_text(&a.kind, &b.kind, dir),
        Sort::Category => compare_text(&a.category, &b.category, dir),
        Sort::Summary => compare_text(&a.summary, &b.summary, dir),
        Sort::Area => compare_text(place(&a.anchor), place(&b.anchor), dir),
    };
    primary.then_with(|| id_key(&a.id).cmp(&id_key(&b.id)))
}

fn compare_input(sort: InboxColumn, dir: SortDir, a: &InputRow, b: &InputRow) -> Ordering {
    fn text(row: &InputRow) -> &str {
        if row.is_unanswered_question() {
            &row.prompt
        } else {
            &row.text
        }
    }
    let primary = match sort {
        InboxColumn::Id => dir.apply(id_key(&a.id).cmp(&id_key(&b.id))),
        InboxColumn::Kind => compare_text(&a.kind, &b.kind, dir),
        InboxColumn::Status => compare_text(&a.status, &b.status, dir),
        InboxColumn::Text => compare_text(text(a), text(b), dir),
    };
    primary.then_with(|| id_key(&a.id).cmp(&id_key(&b.id)))
}

/// Case-insensitive, with a blank value last whichever way `dir` runs.
fn compare_text(a: &str, b: &str, dir: SortDir) -> Ordering {
    match (a.is_empty(), b.is_empty()) {
        (false, false) => {
            let folded = a
                .chars()
                .flat_map(char::to_lowercase)
                .cmp(b.chars().flat_map(char::to_lowercase));
            dir.apply(folded.then_with(|| a.cmp(b)))
        }
        (a_blank, b_blank) => a_blank.cmp(&b_blank),
    }
}

/// The text an item's anchor shows: a file, a section or an area.
fn place(anchor: &Anchor) -> &str {
    match anchor {
        Anchor::Code { file, .. } => file,
        Anchor::Section(section) => section,
        Anchor::Area(area) => area,
        Anchor::None => "",
    }
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
    fn row_finds_the_first_row_with_an_id_after_every_load() {
        let mut state = loaded(vec![
            item("R1", "open", "warning"),
            item("R2", "open", "critical"),
            item("R2", "fixed", "minor"),
        ]);
        assert_eq!(state.row("R2").map(|r| r.status.as_str()), Some("open"));
        assert!(state.row("R9").is_none());

        state.set_rows(
            vec![
                item("R2", "deferred", "minor"),
                item("R3", "open", "warning"),
            ],
            Some("rev1"),
        );
        assert_eq!(state.row("R2").map(|r| r.status.as_str()), Some("deferred"));
        assert_eq!(state.row("R3").map(|r| r.status.as_str()), Some("open"));
        assert!(state.row("R1").is_none());
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
        let sorts = [
            Sort::Id,
            Sort::Severity,
            Sort::Effort,
            Sort::Newest,
            Sort::Kind,
            Sort::Status,
            Sort::Category,
            Sort::Summary,
            Sort::Area,
        ]
        .map(Sort::key);
        assert_eq!(
            sorts,
            [
                "id", "severity", "effort", "newest", "kind", "status", "category", "summary",
                "area"
            ]
        );
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
    fn pending_inputs_naming_one_item_come_back_in_id_order() {
        let on = |id: &str, status: &str, ledger: &str| InputRow {
            ledger: ledger.to_owned(),
            items: vec!["R3".to_owned(), "R3".to_owned()],
            ..input(id, "note", "user", status)
        };
        let state = inbox(vec![
            on("I10", "new", "review"),
            on("I3", "handled", "review"),
            on("I2", "acknowledged", "review"),
            on("I4", "new", "optimise"),
        ]);
        let ids: Vec<&str> = state
            .pending_for(Kind::Review, "R3")
            .into_iter()
            .map(|r| r.id.as_str())
            .collect();
        assert_eq!(ids, ["I2", "I10"]);
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
        let mut sorts = Vec::new();
        for _ in 0..6 {
            state.cycle_sort();
            sorts.push(state.sort);
        }
        assert_eq!(
            sorts,
            [
                Sort::Newest,
                Sort::Kind,
                Sort::Status,
                Sort::Summary,
                Sort::Area,
                Sort::Id
            ]
        );
    }

    #[test]
    fn a_header_cycles_asc_then_desc_then_off() {
        let mut state = ItemsState::new(Surface::Review);
        assert!(state.cycle_header(ItemColumn::Effort));
        assert_eq!((state.sort, state.sort_dir), (Sort::Effort, SortDir::Asc));
        assert_eq!(state.header_sort(ItemColumn::Effort), Some(SortDir::Asc));
        state.cycle_header(ItemColumn::Effort);
        assert_eq!((state.sort, state.sort_dir), (Sort::Effort, SortDir::Desc));
        state.cycle_header(ItemColumn::Effort);
        assert_eq!((state.sort, state.sort_dir), (Sort::Id, SortDir::Asc));
        assert_eq!(state.header_sort(ItemColumn::Effort), None);

        state.cycle_header(ItemColumn::Kind);
        state.cycle_header(ItemColumn::Kind);
        state.cycle_header(ItemColumn::Summary);
        assert_eq!(
            (state.sort, state.sort_dir),
            (Sort::Summary, SortDir::Asc),
            "another column starts ascending"
        );

        let mut backlog = ItemsState::new(Surface::Backlog);
        assert!(!backlog.cycle_header(ItemColumn::Effort));
        assert_eq!((backlog.sort, backlog.sort_dir), (Sort::Id, SortDir::Asc));
        assert!(backlog.cycle_header(ItemColumn::Kind));
        assert_eq!(backlog.sort, Sort::Kind);
    }

    #[test]
    fn the_kind_column_sorts_a_finding_by_severity() {
        assert_eq!(
            Surface::Review.column_sort(ItemColumn::Kind),
            Some(Sort::Severity)
        );
        assert_eq!(
            Surface::Backlog.column_sort(ItemColumn::Kind),
            Some(Sort::Kind)
        );
        assert_eq!(Surface::PlanReview.column_sort(ItemColumn::Effort), None);
        for surface in Surface::ALL {
            for column in ItemColumn::ALL {
                if let Some(sort) = surface.column_sort(column) {
                    assert!(surface.sorts().contains(&sort), "{surface:?} {column:?}");
                }
            }
        }
    }

    #[test]
    fn desc_reverses_severity_but_ties_stay_in_id_order() {
        let mut state = loaded(vec![
            item("R1", "open", "warning"),
            item("R2", "open", "critical"),
            item("R3", "open", "suggestion"),
            item("R4", "open", "critical"),
        ]);
        state.sort = Sort::Severity;
        assert_eq!(items(&state), ["R2", "R4", "R1", "R3"]);
        state.flip_dir();
        assert_eq!(state.sort_dir, SortDir::Desc);
        assert_eq!(items(&state), ["R3", "R1", "R2", "R4"]);
        state.sort = Sort::Id;
        assert_eq!(items(&state), ["R4", "R3", "R2", "R1"]);
    }

    #[test]
    fn a_blank_value_sorts_last_either_way() {
        let area = |id: &str, area: &str| ItemRow {
            anchor: Anchor::Area(area.to_owned()),
            ..item(id, "open", "warning")
        };
        let mut state = loaded(vec![area("R1", ""), area("R2", "b"), area("R3", "A")]);
        state.sort = Sort::Area;
        assert_eq!(items(&state), ["R3", "R2", "R1"]);
        state.flip_dir();
        assert_eq!(items(&state), ["R2", "R3", "R1"]);
    }

    #[test]
    fn the_inbox_sorts_by_kind_within_its_sections() {
        let mut state = inbox(vec![
            input("I1", "request", "user", "new"),
            input("I2", "capture", "user", "new"),
            input("I3", "note", "user", "new"),
            input("I4", "question", "review", "new"),
            input("I5", "capture", "user", "acknowledged"),
            input("I6", "note", "user", "acknowledged"),
        ]);
        state.cycle_header(InboxColumn::Kind);
        assert_eq!(state.header_sort(InboxColumn::Kind), Some(SortDir::Asc));
        assert_eq!(
            item_ids(state.visible()),
            ["I4", "I2", "I3", "I1", "I5", "I6"]
        );
        state.cycle_header(InboxColumn::Kind);
        assert_eq!(
            item_ids(state.visible()),
            ["I4", "I1", "I3", "I2", "I6", "I5"]
        );
        state.cycle_header(InboxColumn::Kind);
        assert_eq!(
            (state.sort, state.sort_dir),
            (InboxColumn::Id, SortDir::Asc)
        );
        state.flip_dir();
        assert_eq!(
            item_ids(state.visible()),
            ["I4", "I3", "I2", "I1", "I6", "I5"]
        );
    }

    #[test]
    fn sort_directions_and_inbox_columns_round_trip_their_keys() {
        for dir in [SortDir::Asc, SortDir::Desc] {
            assert_eq!(SortDir::from_key(dir.key()), Some(dir));
            assert_eq!(dir.flipped().flipped(), dir);
        }
        for column in InboxColumn::ALL {
            assert_eq!(InboxColumn::from_key(column.label()), Some(column));
        }
        assert_eq!(SortDir::from_key("down"), None);
        assert_eq!(SortDir::Desc.label(), "desc");
        assert_ne!(
            HeaderKey::Item(ItemColumn::Id),
            HeaderKey::Inbox(InboxColumn::Id)
        );
    }

    fn five_items() -> ItemsState {
        loaded(
            (1..=5)
                .map(|n| item(&format!("R{n}"), "open", "warning"))
                .collect(),
        )
    }

    #[test]
    fn page_down_moves_a_drawn_page_of_rows() {
        let mut state = five_items();
        state.cursor = Some("R1".to_owned());
        state.page = 3;
        state.scroll_pinned = true;
        state.page_cursor(Scroll::PageDown);
        assert_eq!(state.cursor.as_deref(), Some("R4"));
        assert!(!state.scroll_pinned);
        state.page_cursor(Scroll::PageUp);
        assert_eq!(state.cursor.as_deref(), Some("R1"));
    }

    #[test]
    fn bottom_lands_on_the_last_visible_id() {
        let mut state = five_items();
        state.page_cursor(Scroll::Bottom);
        assert_eq!(state.cursor.as_deref(), Some("R5"));
        state.page_cursor(Scroll::Top);
        assert_eq!(state.cursor.as_deref(), Some("R1"));
    }

    #[test]
    fn paging_an_empty_list_leaves_the_cursor_none() {
        let mut state = ItemsState::new(Surface::Review);
        state.page_cursor(Scroll::PageDown);
        assert_eq!(state.cursor, None);
        let mut inbox = InboxState::default();
        inbox.page_cursor(Scroll::Bottom);
        assert_eq!(inbox.cursor, None);
    }

    #[test]
    fn the_inbox_pages_through_its_visible_order() {
        let mut state = inbox(vec![
            input("I1", "request", "user", "new"),
            input("I2", "request", "user", "new"),
            input("I3", "request", "user", "new"),
        ]);
        state.cursor = Some("I1".to_owned());
        state.page = 2;
        state.page_cursor(Scroll::PageDown);
        assert_eq!(state.cursor.as_deref(), Some("I3"));
    }
}
