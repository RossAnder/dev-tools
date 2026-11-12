Status: done — the markdown renderer parses and draws tables, with columns sized to content and cells wrapped to a given width

## What changed

- `glimpse/src/view/markdown.rs`
  - Parses with `Parser::new_ext(source, Options::ENABLE_TABLES)`. Checked against pulldown-cmark 0.13.6 (the locked version) in the local registry source: `Options::ENABLE_TABLES`, `Tag::Table(Vec<Alignment>)`, `Tag::TableHead`, `Tag::TableRow`, `Tag::TableCell` and the matching `TagEnd` variants.
  - New `to_text_within(source, theme, width)`. `to_text(source, theme)` keeps its signature and calls it with `usize::MAX`, so the existing callers in `details.rs` and `ego.rs` are unchanged and see no wrapping: their tables take their content's width.
  - The writer collects each cell's spans, so inline code, emphasis and links keep their styles. The header row is bold, then comes a rule of `─` with `─┼─` joints, then the body rows, with columns separated by ` │ `. Column alignment from the delimiter row is respected: right-aligned and centred columns pad on the left. A left-aligned last column gets no trailing padding.
  - Column widths: each column is first sized to its content. If the total is wider than `width` (less the separators and any list indent), `fit_columns` lets columns narrower than an even share keep their width and splits what is left evenly among the rest. `wrap_cell` breaks a cell between words, breaks inside a word only when that word alone is too wide, and carries each character's style across the break.
  - A table inside a list item is indented by the item's continuation.
  - Tests: `simple_table_draws_header_rule_and_rows`, `right_and_centre_aligned_table_columns_pad_on_the_left`, `wide_table_wraps_inside_its_cells` (every line is at most the width), `inline_code_in_a_table_cell_keeps_its_style` (both unwrapped and wrapped), and `markdown_without_a_table_renders_as_before` (fixed expected lines, and the same output from `to_text_within` at a narrow width).

## Deviations

- Rust has no default parameters. The detail asked for "a width parameter with a default the existing callers keep", so the width went into a second entry point, `to_text_within`, and `to_text` stays as the default with no limit. Changing `to_text`'s signature instead would have meant editing `details.rs` and `ego.rs`, which this task does not hold. No caller passes a panel width yet, so tables in glimpse panels are not wrapped until a caller switches to `to_text_within`.

## Checks

- `cargo clippy --manifest-path glimpse/Cargo.toml --all-targets`: clean.
- `rustfmt --edition 2024 --check glimpse/src/view/markdown.rs`: clean.
- `cargo test --manifest-path glimpse/Cargo.toml --bin glimpse markdown`: 13 passed. This is the task's own narrow test; the accept check of the same name is the gate's.
- Accept, `ENABLE_TABLES` and `Tag::Table` grep: pass.
- Accept, `fn [a-z_]*table` grep: pass.

## Handover

None.

TANGENTIAL: direction | glimpse/src/view/details.rs | The details and doc views still call `markdown::to_text`, so tables there are drawn at their natural width, not wrapped to the panel | A table wider than the panel is clipped or wrapped by `Paragraph::wrap` across columns until a caller passes the panel width to `to_text_within`
