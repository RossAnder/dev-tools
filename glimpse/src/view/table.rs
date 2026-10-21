//! The table core the item, Inbox and layer lists share: rows drawn fill, then line,
//! then overlay; a header row of column titles carrying the sort mark; and the cell
//! rects a click is tested against.
//!
//! Rows stay hand-rolled because ratatui's `Table` reports no cell rects. A row opens
//! with [`LEAD`] cells for the cursor and mark, and each drawn column follows a
//! one-cell gap; a zero-width column is skipped along with its gap.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Line;

use super::{text_width, truncate};
use crate::surface::SortDir;
use crate::theme::Theme;

/// The cursor and mark cells every row starts with.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) const LEAD: u16 = 2;

/// A drawn row, the styles laid under and over its full width, and what a click on it
/// selects.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct Row<T> {
    pub(crate) line: Line<'static>,
    pub(crate) fill: Option<Style>,
    pub(crate) overlay: Option<Style>,
    pub(crate) target: Option<T>,
}

#[cfg_attr(not(test), allow(dead_code))]
impl<T> Row<T> {
    /// A row with no styling of its own and nothing to click.
    pub(crate) fn plain(line: Line<'static>) -> Row<T> {
        Row {
            line,
            fill: None,
            overlay: None,
            target: None,
        }
    }
}

/// Draws `rows` top-down into `area`, one per line, and hands back each targeted row's
/// rect. Rows past the bottom of `area` are not drawn.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn draw_rows<T: Clone>(buf: &mut Buffer, area: Rect, rows: &[Row<T>]) -> Vec<(Rect, T)> {
    let mut targets = Vec::new();
    for (row, y) in rows.iter().zip(area.y..area.bottom()) {
        let rect = Rect::new(area.x, y, area.width, 1);
        // The fill goes under the spans so a chip keeps its own background.
        if let Some(style) = row.fill {
            buf.set_style(rect, style);
        }
        buf.set_line(area.x, y, &row.line, area.width);
        if let Some(style) = row.overlay {
            buf.set_style(rect, style);
        }
        if let Some(target) = &row.target {
            targets.push((rect, target.clone()));
        }
    }
    targets
}

/// One column: the title its header shows and the cells its values take.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct Column {
    pub(crate) title: &'static str,
    pub(crate) width: usize,
}

/// The rect of column `index` within `row`, clipped to it. A zero-width or missing
/// column gets a zero-width rect.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn cell(row: Rect, columns: &[Column], index: usize) -> Rect {
    let right = usize::from(row.right());
    let clip = |x: usize, width: usize| {
        let left = x.min(right);
        let end = x.saturating_add(width).min(right);
        Rect::new(
            u16::try_from(left).unwrap_or(row.right()),
            row.y,
            u16::try_from(end - left).unwrap_or(0),
            row.height,
        )
    };
    let mut x = usize::from(row.x) + usize::from(LEAD);
    for (at, column) in columns.iter().enumerate() {
        if column.width == 0 {
            if at == index {
                return clip(x, 0);
            }
            continue;
        }
        x += 1;
        if at == index {
            return clip(x, column.width);
        }
        x += column.width;
    }
    clip(right, 0)
}

/// Draws the column titles on `area`'s first line in `table_header`, with `▲`/`▼` in
/// `sort_mark` right after the title of the column `sorted` names. Returns each drawn
/// column's rect and index, for header clicks.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn header(
    buf: &mut Buffer,
    area: Rect,
    columns: &[Column],
    sorted: Option<(usize, SortDir)>,
    theme: &Theme,
) -> Vec<(Rect, usize)> {
    if area.width == 0 || area.height == 0 {
        return Vec::new();
    }
    let row = Rect::new(area.x, area.y, area.width, 1);
    let mut rects = Vec::new();
    for (index, column) in columns.iter().enumerate() {
        let rect = cell(row, columns, index);
        if rect.width == 0 {
            continue;
        }
        let title = truncate(column.title, usize::from(rect.width));
        buf.set_stringn(
            rect.x,
            rect.y,
            &title,
            usize::from(rect.width),
            theme.table_header,
        );
        if let Some((_, dir)) = sorted.filter(|(at, _)| *at == index) {
            // A title that fills its cell puts the mark in the gap after it.
            let after = u16::try_from(text_width(&title)).unwrap_or(rect.width);
            let x = rect.x.saturating_add(after).min(row.right() - 1);
            buf.set_string(x, row.y, mark(dir), theme.sort_mark);
        }
        rects.push((rect, index));
    }
    rects
}

#[cfg_attr(not(test), allow(dead_code))]
fn mark(dir: SortDir) -> &'static str {
    match dir {
        SortDir::Asc => "▲",
        SortDir::Desc => "▼",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Color;
    use ratatui::text::Span;

    const COLUMNS: [Column; 5] = [
        Column {
            title: "",
            width: 1,
        },
        Column {
            title: "id",
            width: 3,
        },
        Column {
            title: "kind",
            width: 0,
        },
        Column {
            title: "summary",
            width: 10,
        },
        Column {
            title: "area",
            width: 5,
        },
    ];

    fn line_of(buf: &Buffer, y: u16) -> String {
        let area = buf.area;
        (area.x..area.right())
            .map(|x| buf[(x, y)].symbol())
            .collect()
    }

    #[test]
    fn header_rects_tile_the_columns_after_the_lead() {
        let area = Rect::new(4, 2, 40, 3);
        let mut buf = Buffer::empty(area);
        let rects = header(&mut buf, area, &COLUMNS, None, &Theme::default());
        let indices: Vec<usize> = rects.iter().map(|(_, at)| *at).collect();
        assert_eq!(indices, [0, 1, 3, 4], "the zero-width column is skipped");
        let row = Rect::new(4, 2, 40, 1);
        assert_eq!(
            rects.iter().map(|(rect, _)| *rect).collect::<Vec<_>>(),
            [
                Rect::new(7, 2, 1, 1),
                Rect::new(9, 2, 3, 1),
                Rect::new(13, 2, 10, 1),
                Rect::new(24, 2, 5, 1),
            ]
        );
        for (rect, at) in &rects {
            assert_eq!(*rect, cell(row, &COLUMNS, *at));
        }
        for pair in rects.windows(2) {
            assert_eq!(pair[1].0.x, pair[0].0.right() + 1, "one-cell gaps");
        }
        let line = line_of(&buf, 2);
        assert_eq!(&line[4..8], " id ", "{line:?}");
        assert_eq!(buf[(9, 2)].style().fg, Theme::default().table_header.fg);
    }

    #[test]
    fn a_narrow_row_clips_the_last_columns() {
        let row = Rect::new(0, 0, 20, 1);
        assert_eq!(cell(row, &COLUMNS, 4), Rect::new(20, 0, 0, 1));
        assert_eq!(cell(row, &COLUMNS, 3), Rect::new(9, 0, 10, 1));
        let narrower = Rect::new(0, 0, 14, 1);
        assert_eq!(cell(narrower, &COLUMNS, 3), Rect::new(9, 0, 5, 1));
        assert_eq!(cell(narrower, &COLUMNS, 9).width, 0, "no such column");
    }

    #[test]
    fn cell_returns_the_rect_the_id_is_drawn_in() {
        let area = Rect::new(2, 5, 30, 1);
        let rows = [Row {
            line: Line::from("▸  ○ R12 Lock held across await"),
            fill: None,
            overlay: None,
            target: Some("R12".to_string()),
        }];
        let mut buf = Buffer::empty(area);
        let targets = draw_rows(&mut buf, area, &rows);
        let id = cell(targets[0].0, &COLUMNS, 1);
        assert_eq!(id, Rect::new(7, 5, 3, 1));
        let drawn: String = (id.x..id.right()).map(|x| buf[(x, 5)].symbol()).collect();
        assert_eq!(drawn, "R12");
    }

    #[test]
    fn the_sort_mark_sits_beside_the_sorted_title() {
        let theme = Theme::default();
        let area = Rect::new(0, 0, 40, 1);
        let draw = |sorted| {
            let mut buf = Buffer::empty(area);
            header(&mut buf, area, &COLUMNS, sorted, &theme);
            buf
        };

        let buf = draw(Some((3, SortDir::Desc)));
        let line = line_of(&buf, 0);
        assert!(line.contains("summary▼"), "{line:?}");
        let at = line.chars().position(|c| c == '▼').expect("mark");
        let x = u16::try_from(at).expect("x");
        assert_eq!(buf[(x, 0)].style().fg, theme.sort_mark.fg);

        let line = line_of(&draw(Some((1, SortDir::Asc))), 0);
        assert!(line.contains(" id▲ "), "{line:?}");
        assert!(!line.contains('▼'));

        let line = line_of(&draw(None), 0);
        assert!(!line.contains('▲') && !line.contains('▼'), "{line:?}");
    }

    #[test]
    fn a_title_filling_its_cell_puts_the_mark_in_the_gap() {
        let columns = [
            Column {
                title: "id",
                width: 2,
            },
            Column {
                title: "kind",
                width: 4,
            },
        ];
        let area = Rect::new(0, 0, 20, 1);
        let mut buf = Buffer::empty(area);
        header(
            &mut buf,
            area,
            &columns,
            Some((0, SortDir::Asc)),
            &Theme::default(),
        );
        assert!(line_of(&buf, 0).starts_with("   id▲kind"));
    }

    #[test]
    fn rows_draw_fill_then_line_then_overlay() {
        let area = Rect::new(0, 0, 12, 3);
        let fill = Style::default().bg(Color::Blue);
        let overlay = Style::default().fg(Color::Red);
        let rows: Vec<Row<u32>> = vec![
            Row::plain(Line::from("── layer 1")),
            Row {
                line: Line::from(vec![
                    Span::raw("7 "),
                    Span::styled("ID", Style::default().bg(Color::Green)),
                ]),
                fill: Some(fill),
                overlay: None,
                target: Some(7),
            },
            Row {
                line: Line::from("8"),
                fill: None,
                overlay: Some(overlay),
                target: Some(8),
            },
        ];
        let mut buf = Buffer::empty(area);
        let targets = draw_rows(&mut buf, area, &rows);
        assert_eq!(
            targets,
            [(Rect::new(0, 1, 12, 1), 7), (Rect::new(0, 2, 12, 1), 8)]
        );
        assert_eq!(buf[(0, 1)].bg, Color::Blue);
        assert_eq!(buf[(11, 1)].bg, Color::Blue, "the fill spans the row");
        assert_eq!(
            buf[(2, 1)].bg,
            Color::Green,
            "the chip keeps its background"
        );
        assert_eq!(buf[(5, 2)].fg, Color::Red, "the overlay spans the row");
    }

    #[test]
    fn rows_past_the_area_are_not_drawn() {
        let area = Rect::new(0, 0, 4, 1);
        let rows: Vec<Row<u32>> = (0..3)
            .map(|n| Row {
                target: Some(n),
                ..Row::plain(Line::from("x"))
            })
            .collect();
        let targets = draw_rows(&mut Buffer::empty(area), area, &rows);
        assert_eq!(targets.len(), 1);
    }
}
