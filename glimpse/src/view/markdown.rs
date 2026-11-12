//! Markdown to styled ratatui text.
//!
//! Covers what task bodies and notes use: paragraphs, headings, nested
//! lists, inline code, strong, emphasis, links, code blocks, rules and
//! tables. Each block becomes whole `Line`s; wrapping is left to
//! `Paragraph::wrap`, so a wrapped list item's continuation rows are not
//! indented under its marker. Tables are the exception: their cells are
//! wrapped here, inside their columns, to the width the caller gives.

use pulldown_cmark::{Alignment, Event, Options, Parser, Tag, TagEnd};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};

use crate::theme::Theme;

const BULLET: &str = "• ";
const RULE: &str = "────────";
const COLUMN_SEPARATOR: &str = " │ ";
const RULE_JOINT: &str = "─┼─";

/// Renders with no width limit: table columns take their content's width.
pub(crate) fn to_text(source: &str, theme: &Theme) -> Text<'static> {
    to_text_within(source, theme, usize::MAX)
}

/// Renders with tables fitted to `width` columns, their cells wrapped when
/// the content is wider. Text outside tables is not wrapped here.
pub(crate) fn to_text_within(source: &str, theme: &Theme, width: usize) -> Text<'static> {
    let mut writer = Writer::new(theme, width);
    for event in Parser::new_ext(source, Options::ENABLE_TABLES) {
        writer.event(event);
    }
    writer.finish()
}

/// A table being collected: each cell's spans, row by row, header first.
struct Table {
    aligns: Vec<Alignment>,
    rows: Vec<Vec<Vec<Span<'static>>>>,
    row: Vec<Vec<Span<'static>>>,
    has_head: bool,
}

struct Writer {
    lines: Vec<Line<'static>>,
    spans: Vec<Span<'static>>,
    /// Inline style stack; the top applies to new text.
    styles: Vec<Style>,
    /// One entry per open list: the next ordinal, `None` for a bullet list.
    lists: Vec<Option<u64>>,
    /// Width of the open item's marker, for lines the item continues onto.
    continuation: usize,
    in_code_block: bool,
    /// A top-level block ended, so the next one starts after a blank line.
    pending_blank: bool,
    strong: Style,
    code: Style,
    width: usize,
    table: Option<Table>,
}

impl Writer {
    fn new(theme: &Theme, width: usize) -> Writer {
        Writer {
            lines: Vec::new(),
            spans: Vec::new(),
            styles: Vec::new(),
            lists: Vec::new(),
            continuation: 0,
            in_code_block: false,
            pending_blank: false,
            strong: theme.strong,
            code: theme.inline_code,
            width,
            table: None,
        }
    }

    fn style(&self) -> Style {
        self.styles.last().copied().unwrap_or_default()
    }

    fn push_style(&mut self, patch: Style) {
        let next = self.style().patch(patch);
        self.styles.push(next);
    }

    fn flush(&mut self) {
        if !self.spans.is_empty() {
            self.lines.push(Line::from(std::mem::take(&mut self.spans)));
        }
    }

    /// Ends the current line, and inside a list item re-indents the next one
    /// under the item's marker.
    fn break_line(&mut self) {
        self.flush();
        if !self.lists.is_empty() && self.continuation > 0 {
            self.spans.push(Span::raw(" ".repeat(self.continuation)));
        }
    }

    fn start_block(&mut self) {
        self.flush();
        if self.pending_blank && !self.lines.is_empty() {
            self.lines.push(Line::default());
        }
        self.pending_blank = false;
    }

    fn end_block(&mut self) {
        self.flush();
        if self.lists.is_empty() {
            self.pending_blank = true;
        }
    }

    fn text(&mut self, content: String, style: Style) {
        self.spans.push(Span::styled(content, style));
    }

    fn event(&mut self, event: Event<'_>) {
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(text) if self.in_code_block => {
                let indent = " ".repeat(self.continuation);
                for line in text.lines() {
                    self.lines.push(Line::from(vec![
                        Span::raw(indent.clone()),
                        Span::styled(line.to_string(), self.code),
                    ]));
                }
            }
            Event::Text(text) | Event::Html(text) | Event::InlineHtml(text) => {
                self.text(text.into_string(), self.style());
            }
            Event::Code(code) => self.text(code.into_string(), self.style().patch(self.code)),
            Event::SoftBreak => self.text(" ".to_string(), self.style()),
            Event::HardBreak => self.break_line(),
            Event::Rule => {
                self.start_block();
                self.lines.push(Line::raw(RULE));
                self.end_block();
            }
            _ => {}
        }
    }

    fn start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Paragraph if self.lists.is_empty() => self.start_block(),
            // The first paragraph of a loose list item shares the marker's
            // line; a later one starts a line indented under the marker.
            Tag::Paragraph if self.spans.is_empty() => self.break_line(),
            Tag::Heading { .. } => {
                self.start_block();
                self.push_style(Style::new().add_modifier(Modifier::BOLD));
            }
            Tag::CodeBlock(_) => {
                if self.lists.is_empty() {
                    self.start_block();
                } else {
                    self.flush();
                }
                self.in_code_block = true;
            }
            Tag::List(first) => {
                if self.lists.is_empty() {
                    self.start_block();
                } else {
                    self.flush();
                }
                self.lists.push(first);
            }
            Tag::Item => {
                self.flush();
                let depth = self.lists.len().saturating_sub(1);
                let marker = match self.lists.last_mut() {
                    Some(Some(ordinal)) => {
                        let marker = format!("{ordinal}. ");
                        *ordinal += 1;
                        marker
                    }
                    _ => BULLET.to_string(),
                };
                let prefix = format!("{}{marker}", "  ".repeat(depth));
                self.continuation = prefix.chars().count();
                self.spans.push(Span::raw(prefix));
            }
            Tag::Emphasis => self.push_style(Style::new().add_modifier(Modifier::ITALIC)),
            Tag::Strong => self.push_style(self.strong),
            Tag::Strikethrough => self.push_style(Style::new().add_modifier(Modifier::CROSSED_OUT)),
            Tag::Link { .. } => self.push_style(Style::new().add_modifier(Modifier::UNDERLINED)),
            Tag::Table(aligns) => {
                if self.lists.is_empty() {
                    self.start_block();
                } else {
                    self.flush();
                }
                self.table = Some(Table {
                    aligns,
                    rows: Vec::new(),
                    row: Vec::new(),
                    has_head: false,
                });
            }
            Tag::TableHead => {
                if let Some(table) = self.table.as_mut() {
                    table.has_head = true;
                }
                self.push_style(Style::new().add_modifier(Modifier::BOLD));
            }
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => self.end_block(),
            TagEnd::Heading(_) => {
                self.styles.pop();
                self.end_block();
            }
            TagEnd::CodeBlock => {
                self.in_code_block = false;
                self.end_block();
            }
            TagEnd::List(_) => {
                self.flush();
                self.lists.pop();
                // Back in the enclosing item, whose marker is two columns
                // shallower than the nested one.
                self.continuation = self.continuation.saturating_sub(2);
                if self.lists.is_empty() {
                    self.continuation = 0;
                    self.pending_blank = true;
                }
            }
            TagEnd::Item => self.flush(),
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough | TagEnd::Link => {
                self.styles.pop();
            }
            TagEnd::TableCell => {
                let cell = std::mem::take(&mut self.spans);
                if let Some(table) = self.table.as_mut() {
                    table.row.push(cell);
                }
            }
            TagEnd::TableHead => {
                self.styles.pop();
                self.end_table_row();
            }
            TagEnd::TableRow => self.end_table_row(),
            TagEnd::Table => {
                if let Some(table) = self.table.take() {
                    self.draw_table(table);
                }
                self.end_block();
            }
            _ => {}
        }
    }

    fn end_table_row(&mut self) {
        if let Some(table) = self.table.as_mut() {
            let row = std::mem::take(&mut table.row);
            table.rows.push(row);
        }
    }

    /// Emits the header row, a rule and the body rows, with columns sized to
    /// their content and shrunk to fit `width`, wrapping the cells that no
    /// longer fit.
    fn draw_table(&mut self, table: Table) {
        let columns = table.rows.iter().map(Vec::len).max().unwrap_or(0);
        if columns == 0 {
            return;
        }
        let indent = if self.lists.is_empty() {
            0
        } else {
            self.continuation
        };
        let natural: Vec<usize> = (0..columns)
            .map(|column| {
                table
                    .rows
                    .iter()
                    .filter_map(|row| row.get(column))
                    .map(|cell| spans_width(cell))
                    .max()
                    .unwrap_or(0)
                    .max(1)
            })
            .collect();
        let separators = str_width(COLUMN_SEPARATOR) * (columns - 1);
        let budget = self.width.saturating_sub(indent + separators).max(columns);
        let widths = fit_columns(&natural, budget);
        let pad = " ".repeat(indent);
        for (index, row) in table.rows.iter().enumerate() {
            let wrapped: Vec<Vec<Vec<Span<'static>>>> = (0..columns)
                .map(|column| wrap_cell(row.get(column).map_or(&[], Vec::as_slice), widths[column]))
                .collect();
            let height = wrapped.iter().map(Vec::len).max().unwrap_or(0).max(1);
            for line_index in 0..height {
                let mut spans = Vec::new();
                if indent > 0 {
                    spans.push(Span::raw(pad.clone()));
                }
                for (column, cell) in wrapped.iter().enumerate() {
                    if column > 0 {
                        spans.push(Span::raw(COLUMN_SEPARATOR));
                    }
                    let content = cell.get(line_index).cloned().unwrap_or_default();
                    let slack = widths[column].saturating_sub(spans_width(&content));
                    let (left, right) = match table.aligns.get(column) {
                        Some(Alignment::Right) => (slack, 0),
                        Some(Alignment::Center) => (slack / 2, slack - slack / 2),
                        _ => (0, slack),
                    };
                    if left > 0 {
                        spans.push(Span::raw(" ".repeat(left)));
                    }
                    spans.extend(content);
                    if right > 0 && column + 1 < columns {
                        spans.push(Span::raw(" ".repeat(right)));
                    }
                }
                self.lines.push(Line::from(spans));
            }
            if index == 0 && table.has_head {
                let rule = widths
                    .iter()
                    .map(|width| "─".repeat(*width))
                    .collect::<Vec<_>>()
                    .join(RULE_JOINT);
                self.lines.push(Line::raw(format!("{pad}{rule}")));
            }
        }
    }

    fn finish(mut self) -> Text<'static> {
        self.flush();
        Text::from(self.lines)
    }
}

fn str_width(text: &str) -> usize {
    Span::raw(text).width()
}

fn spans_width(spans: &[Span<'_>]) -> usize {
    spans.iter().map(Span::width).sum()
}

/// Column widths summing to at most `budget` (which is at least one per
/// column): a column narrower than an even share keeps its natural width,
/// and the columns left over split what remains evenly.
fn fit_columns(natural: &[usize], budget: usize) -> Vec<usize> {
    if natural.iter().sum::<usize>() <= budget {
        return natural.to_vec();
    }
    let mut widths = vec![0; natural.len()];
    let mut open: Vec<usize> = (0..natural.len()).collect();
    let mut remaining = budget;
    while !open.is_empty() {
        let share = remaining / open.len();
        let (fits, rest): (Vec<usize>, Vec<usize>) =
            open.iter().partition(|&&column| natural[column] <= share);
        if fits.is_empty() {
            let extra = remaining % rest.len();
            for (position, column) in rest.iter().enumerate() {
                widths[*column] = share + usize::from(position < extra);
            }
            break;
        }
        for column in fits {
            widths[column] = natural[column];
            remaining -= natural[column];
        }
        open = rest;
    }
    widths
}

/// Splits a cell's spans into lines at most `width` columns wide, breaking
/// between words and inside a word only when it alone is wider. Styles carry
/// through each character, so inline code and emphasis survive the wrap.
fn wrap_cell(cell: &[Span<'static>], width: usize) -> Vec<Vec<Span<'static>>> {
    if spans_width(cell) <= width {
        return vec![cell.to_vec()];
    }
    let mut words: Vec<(Style, Vec<(char, Style)>)> = Vec::new();
    let mut current = Vec::new();
    let mut gap = Style::default();
    for span in cell {
        for character in span.content.chars() {
            if character == ' ' {
                if !current.is_empty() {
                    words.push((gap, std::mem::take(&mut current)));
                }
                gap = span.style;
            } else {
                current.push((character, span.style));
            }
        }
    }
    if !current.is_empty() {
        words.push((gap, current));
    }
    let mut lines = Vec::new();
    let mut line: Vec<(char, Style)> = Vec::new();
    let mut line_width = 0;
    for (gap, word) in words {
        let word_width: usize = word.iter().map(|(c, _)| char_width(*c)).sum();
        if !line.is_empty() && line_width + 1 + word_width <= width {
            line.push((' ', gap));
            line.extend(word);
            line_width += 1 + word_width;
            continue;
        }
        if !line.is_empty() {
            lines.push(std::mem::take(&mut line));
            line_width = 0;
        }
        for (character, style) in word {
            let character_width = char_width(character);
            if !line.is_empty() && line_width + character_width > width {
                lines.push(std::mem::take(&mut line));
                line_width = 0;
            }
            line.push((character, style));
            line_width += character_width;
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines.into_iter().map(|line| styled_runs(&line)).collect()
}

fn char_width(character: char) -> usize {
    str_width(character.encode_utf8(&mut [0; 4]))
}

/// Joins consecutive characters of one style back into a span.
fn styled_runs(characters: &[(char, Style)]) -> Vec<Span<'static>> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut run = String::new();
    let mut run_style = None;
    for &(character, style) in characters {
        if run_style.is_some_and(|current| current != style) {
            spans.push(Span::styled(
                std::mem::take(&mut run),
                run_style.unwrap_or_default(),
            ));
        }
        run_style = Some(style);
        run.push(character);
    }
    if let Some(style) = run_style {
        spans.push(Span::styled(run, style));
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(source: &str) -> Text<'static> {
        to_text(source, &Theme::default())
    }

    fn plain(text: &Text<'_>) -> Vec<String> {
        text.lines
            .iter()
            .map(|line| line.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect()
    }

    #[test]
    fn inline_styles_become_span_styles() {
        let text = render("a **b** *c* `d`");
        assert_eq!(plain(&text), ["a b c d"]);
        let spans = &text.lines[0].spans;
        let style_of = |content: &str| {
            spans
                .iter()
                .find(|s| s.content == content)
                .map(|s| s.style)
                .expect("span present")
        };
        assert_eq!(style_of("b"), Theme::default().strong);
        assert!(style_of("c").add_modifier.contains(Modifier::ITALIC));
        assert_eq!(style_of("d"), Theme::default().inline_code);
        assert_eq!(style_of("a "), Style::default());
    }

    #[test]
    fn nested_bullets_are_indented_per_level() {
        let text = render("- a\n  - b\n    - c\n- d");
        assert_eq!(plain(&text), ["• a", "  • b", "    • c", "• d"]);
    }

    #[test]
    fn ordered_lists_count_from_their_start() {
        let text = render("3. x\n4. y\n");
        assert_eq!(plain(&text), ["3. x", "4. y"]);
    }

    #[test]
    fn soft_breaks_join_and_hard_breaks_split() {
        assert_eq!(plain(&render("one\ntwo")), ["one two"]);
        assert_eq!(plain(&render("one  \ntwo")), ["one", "two"]);
        assert_eq!(plain(&render("- one\\\n  two")), ["• one", "  two"]);
    }

    #[test]
    fn blocks_are_separated_by_one_blank_line() {
        let text = render("First.\n\n- item\n\nLast.");
        assert_eq!(plain(&text), ["First.", "", "• item", "", "Last."]);
    }

    #[test]
    fn loose_list_paragraphs_stay_under_their_marker() {
        let text = render("- a\n\n  more\n- b\n");
        assert_eq!(plain(&text), ["• a", "  more", "• b"]);
    }

    #[test]
    fn empty_input_is_empty_text() {
        assert!(render("").lines.is_empty());
    }

    #[test]
    fn simple_table_draws_header_rule_and_rows() {
        let text = render("Before.\n\n| a | bb |\n|---|---|\n| ccc | d |\n\nAfter.");
        assert_eq!(
            plain(&text),
            [
                "Before.",
                "",
                "a   │ bb",
                "────┼───",
                "ccc │ d",
                "",
                "After."
            ]
        );
        let header = &text.lines[2].spans;
        assert!(header[0].style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn right_and_centre_aligned_table_columns_pad_on_the_left() {
        let text = render("| n | c | x |\n|--:|:-:|---|\n| 100 | a | y |");
        assert_eq!(plain(&text), ["  n │ c │ x", "────┼───┼──", "100 │ a │ y"]);
    }

    #[test]
    fn wide_table_wraps_inside_its_cells() {
        let source = "| name | description |\n|---|---|\n| x | one two three four |";
        let text = to_text_within(source, &Theme::default(), 16);
        assert_eq!(
            plain(&text),
            [
                "name │ descripti",
                "     │ on",
                "─────┼──────────",
                "x    │ one two",
                "     │ three",
                "     │ four",
            ]
        );
        assert!(text.lines.iter().all(|line| line.width() <= 16));
    }

    #[test]
    fn inline_code_in_a_table_cell_keeps_its_style() {
        let text = render("| `h` |\n|---|\n| *y* `z` |");
        let code = Theme::default().inline_code;
        let style_of = |line: usize, content: &str| {
            text.lines[line]
                .spans
                .iter()
                .find(|s| s.content == content)
                .map(|s| s.style)
                .expect("span present")
        };
        assert_eq!(style_of(2, "z"), code);
        assert!(style_of(2, "y").add_modifier.contains(Modifier::ITALIC));
        let header = style_of(0, "h");
        assert!(header.add_modifier.contains(Modifier::BOLD));
        assert_eq!(header.fg, code.fg);

        let wrapped = to_text_within("| h |\n|---|\n| `aa bb` |", &Theme::default(), 2);
        assert_eq!(plain(&wrapped), ["h", "──", "aa", "bb"]);
        assert!(
            wrapped.lines[2..]
                .iter()
                .flat_map(|line| &line.spans)
                .all(|span| span.style == code)
        );
    }

    #[test]
    fn markdown_without_a_table_renders_as_before() {
        let source =
            "# Title\n\nSome `code` and a | pipe.\n\n- a\n  - b\n\n```\nx | y\n```\n\n---\n\nEnd.";
        let expected = [
            "Title",
            "",
            "Some code and a | pipe.",
            "",
            "• a",
            "  • b",
            "",
            "x | y",
            "",
            RULE,
            "",
            "End.",
        ];
        assert_eq!(plain(&render(source)), expected);
        assert_eq!(to_text_within(source, &Theme::default(), 4), render(source));
    }
}
