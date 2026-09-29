//! Markdown to styled ratatui text.
//!
//! Covers what task bodies use: paragraphs, headings, nested lists, inline
//! code, strong, emphasis, links, code blocks and rules. Each block becomes
//! whole `Line`s; wrapping is left to `Paragraph::wrap`, so a wrapped list
//! item's continuation rows are not indented under its marker.

use pulldown_cmark::{Event, Parser, Tag, TagEnd};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};

use crate::theme::Theme;

const BULLET: &str = "• ";
const RULE: &str = "────────";

pub(crate) fn to_text(source: &str, theme: &Theme) -> Text<'static> {
    let mut writer = Writer::new(theme);
    for event in Parser::new(source) {
        writer.event(event);
    }
    writer.finish()
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
}

impl Writer {
    fn new(theme: &Theme) -> Writer {
        Writer {
            lines: Vec::new(),
            spans: Vec::new(),
            styles: Vec::new(),
            lists: Vec::new(),
            continuation: 0,
            in_code_block: false,
            pending_blank: false,
            strong: theme.badge,
            code: theme.inline_code,
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
            _ => {}
        }
    }

    fn finish(mut self) -> Text<'static> {
        self.flush();
        Text::from(self.lines)
    }
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
        assert!(style_of("b").add_modifier.contains(Modifier::BOLD));
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
}
