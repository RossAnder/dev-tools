//! A modal form of text, single-select and multi-select fields, with its key routing and submitted values.

use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use tui_input::Input;
use tui_input::backend::crossterm::EventHandler;

#[derive(Debug, Clone)]
pub(crate) enum Field {
    Text {
        label: String,
        input: Input,
        required: bool,
    },
    /// With `free` set, `cursor == options.len()` is the "other…" choice and
    /// keys typed while it is chosen and the list is closed edit `free`.
    Select {
        label: String,
        options: Vec<String>,
        cursor: usize,
        open: bool,
        free: Option<Input>,
        required: bool,
    },
    /// `checked` runs parallel to `options`.
    Multi {
        label: String,
        options: Vec<String>,
        checked: Vec<bool>,
        cursor: usize,
        required: bool,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct Form {
    pub(crate) title: String,
    pub(crate) prompt: Option<String>,
    pub(crate) fields: Vec<Field>,
    pub(crate) focus: usize,
    pub(crate) error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FormOutcome {
    Pending,
    /// One value per field, in field order.
    Submit(Vec<FieldValue>),
    Cancel,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FieldValue {
    Text(String),
    One(String),
    Many(Vec<String>),
}

impl Field {
    pub(crate) fn text(label: impl Into<String>) -> Self {
        Field::Text {
            label: label.into(),
            input: Input::default(),
            required: false,
        }
    }

    pub(crate) fn select(label: impl Into<String>, options: Vec<String>) -> Self {
        Field::Select {
            label: label.into(),
            options,
            cursor: 0,
            open: false,
            free: None,
            required: false,
        }
    }

    pub(crate) fn multi(label: impl Into<String>, options: Vec<String>) -> Self {
        let checked = vec![false; options.len()];
        Field::Multi {
            label: label.into(),
            options,
            checked,
            cursor: 0,
            required: false,
        }
    }

    pub(crate) fn required(mut self) -> Self {
        match &mut self {
            Field::Text { required, .. }
            | Field::Select { required, .. }
            | Field::Multi { required, .. } => *required = true,
        }
        self
    }

    /// Adds the "other…" choice to a Select; no effect on other fields.
    pub(crate) fn with_other(mut self) -> Self {
        if let Field::Select { free, .. } = &mut self {
            *free = Some(Input::default());
        }
        self
    }

    pub(crate) fn opened(mut self) -> Self {
        if let Field::Select { open, .. } = &mut self {
            *open = true;
        }
        self
    }

    /// Prefills the field. A Select picks the matching option, else puts the
    /// value in "other…" when it has one; a Multi checks each listed option.
    pub(crate) fn with_value(mut self, value: &[&str]) -> Self {
        match &mut self {
            Field::Text { input, .. } => *input = Input::new(value.join(" ")),
            Field::Select {
                options,
                cursor,
                free,
                ..
            } => {
                if let Some(v) = value.first() {
                    if let Some(at) = options.iter().position(|o| o == v) {
                        *cursor = at;
                    } else if let Some(free) = free {
                        *free = Input::new((*v).to_owned());
                        *cursor = options.len();
                    }
                }
            }
            Field::Multi {
                options, checked, ..
            } => {
                for (option, check) in options.iter().zip(checked.iter_mut()) {
                    *check = value.contains(&option.as_str());
                }
            }
        }
        self
    }

    pub(crate) fn label(&self) -> &str {
        match self {
            Field::Text { label, .. }
            | Field::Select { label, .. }
            | Field::Multi { label, .. } => label,
        }
    }

    fn is_required(&self) -> bool {
        match self {
            Field::Text { required, .. }
            | Field::Select { required, .. }
            | Field::Multi { required, .. } => *required,
        }
    }

    /// True for a Select whose "other…" choice is picked.
    pub(crate) fn on_other(&self) -> bool {
        matches!(self, Field::Select { options, cursor, free: Some(_), .. } if *cursor == options.len())
    }

    pub(crate) fn value(&self) -> FieldValue {
        match self {
            Field::Text { input, .. } => FieldValue::Text(input.value().trim().to_owned()),
            Field::Select {
                options,
                cursor,
                free,
                ..
            } => FieldValue::One(match (options.get(*cursor), free) {
                (Some(option), _) => option.clone(),
                (None, Some(free)) => free.value().trim().to_owned(),
                (None, None) => String::new(),
            }),
            Field::Multi {
                options, checked, ..
            } => FieldValue::Many(
                options
                    .iter()
                    .zip(checked)
                    .filter(|(_, c)| **c)
                    .map(|(o, _)| o.clone())
                    .collect(),
            ),
        }
    }

    fn is_empty(&self) -> bool {
        match self.value() {
            FieldValue::Text(s) | FieldValue::One(s) => s.is_empty(),
            FieldValue::Many(v) => v.is_empty(),
        }
    }

    fn close(&mut self) {
        if let Field::Select { open, .. } = self {
            *open = false;
        }
    }

    fn step(&mut self, down: bool) {
        let (cursor, len) = match self {
            Field::Select {
                options,
                cursor,
                free,
                ..
            } => (cursor, options.len() + usize::from(free.is_some())),
            Field::Multi {
                options, cursor, ..
            } => (cursor, options.len()),
            Field::Text { .. } => return,
        };
        *cursor = if down {
            (*cursor + 1).min(len.saturating_sub(1))
        } else {
            cursor.saturating_sub(1)
        };
    }
}

impl Form {
    pub(crate) fn new(title: impl Into<String>, fields: Vec<Field>) -> Self {
        Form {
            title: title.into(),
            prompt: None,
            fields,
            focus: 0,
            error: None,
        }
    }

    pub(crate) fn with_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.prompt = Some(prompt.into());
        self
    }

    /// The text input keys currently go to, for placing the terminal cursor.
    pub(crate) fn active_input(&self) -> Option<&Input> {
        match self.fields.get(self.focus)? {
            Field::Text { input, .. } => Some(input),
            Field::Select {
                open: false,
                free: Some(free),
                ..
            } if self.fields[self.focus].on_other() => Some(free),
            _ => None,
        }
    }

    /// Release events are ignored, since Windows reports both edges of a key.
    pub(crate) fn handle_key(&mut self, key: KeyEvent) -> FormOutcome {
        if key.kind == KeyEventKind::Release {
            return FormOutcome::Pending;
        }
        let Some(field) = self.fields.get_mut(self.focus) else {
            return match key.code {
                KeyCode::Esc => FormOutcome::Cancel,
                KeyCode::Enter => FormOutcome::Submit(Vec::new()),
                _ => FormOutcome::Pending,
            };
        };
        let open = matches!(field, Field::Select { open: true, .. });
        let typing = matches!(field, Field::Text { .. }) || (!open && field.on_other());
        match key.code {
            KeyCode::Tab => self.move_focus(true),
            KeyCode::BackTab => self.move_focus(false),
            KeyCode::Esc if open => field.close(),
            KeyCode::Esc => return FormOutcome::Cancel,
            KeyCode::Enter => return self.enter(),
            KeyCode::Down => field.step(true),
            KeyCode::Up => field.step(false),
            KeyCode::Char('j') if !typing && key.modifiers == KeyModifiers::NONE => {
                field.step(true)
            }
            KeyCode::Char('k') if !typing && key.modifiers == KeyModifiers::NONE => {
                field.step(false)
            }
            KeyCode::Char(' ') if matches!(field, Field::Multi { .. }) => {
                if let Field::Multi {
                    checked, cursor, ..
                } = field
                    && let Some(check) = checked.get_mut(*cursor)
                {
                    *check = !*check;
                }
            }
            _ => match field {
                Field::Text { input, .. } => {
                    input.handle_event(&Event::Key(key));
                }
                Field::Select {
                    free: Some(free), ..
                } if typing => {
                    free.handle_event(&Event::Key(key));
                }
                _ => {}
            },
        }
        self.error = None;
        FormOutcome::Pending
    }

    fn move_focus(&mut self, forward: bool) {
        let len = self.fields.len();
        if len == 0 {
            return;
        }
        self.fields[self.focus].close();
        self.focus = if forward {
            (self.focus + 1) % len
        } else {
            (self.focus + len - 1) % len
        };
    }

    /// A lone Select submits on confirm, so a menu needs no second Enter.
    fn enter(&mut self) -> FormOutcome {
        let lone = self.fields.len() == 1;
        if let Field::Select { open, .. } = &mut self.fields[self.focus] {
            if !*open {
                *open = true;
                return FormOutcome::Pending;
            }
            *open = false;
            if !lone {
                return FormOutcome::Pending;
            }
        }
        self.submit()
    }

    fn submit(&mut self) -> FormOutcome {
        if let Some(at) = self
            .fields
            .iter()
            .position(|f| f.is_required() && f.is_empty())
        {
            self.error = Some(format!("{} is required", self.fields[at].label()));
            self.focus = at;
            return FormOutcome::Pending;
        }
        self.error = None;
        FormOutcome::Submit(self.fields.iter().map(Field::value).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn type_str(form: &mut Form, s: &str) {
        for c in s.chars() {
            assert_eq!(
                form.handle_key(press(KeyCode::Char(c))),
                FormOutcome::Pending
            );
        }
    }

    fn options(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn tab_moves_focus_and_enter_submits() {
        let mut form = Form::new("t", vec![Field::text("a"), Field::text("b")]);
        type_str(&mut form, "x");
        form.handle_key(press(KeyCode::Tab));
        assert_eq!(form.focus, 1);
        type_str(&mut form, "y");
        form.handle_key(press(KeyCode::BackTab));
        assert_eq!(form.focus, 0);
        assert_eq!(
            form.handle_key(press(KeyCode::Enter)),
            FormOutcome::Submit(vec![
                FieldValue::Text("x".into()),
                FieldValue::Text("y".into())
            ])
        );
    }

    #[test]
    fn j_typed_into_a_text_field_is_text() {
        let mut form = Form::new("t", vec![Field::text("a"), Field::text("b")]);
        type_str(&mut form, "jkq ");
        assert_eq!(form.focus, 0);
        let Field::Text { input, .. } = &form.fields[0] else {
            unreachable!()
        };
        assert_eq!(input.value(), "jkq ");
    }

    #[test]
    fn j_typed_into_other_is_text() {
        let field = Field::select("category", options(&["a"]))
            .with_other()
            .with_value(&["zz"]);
        let mut form = Form::new("t", vec![field]);
        type_str(&mut form, "j");
        assert_eq!(form.fields[0].value(), FieldValue::One("zzj".into()));
    }

    #[test]
    fn space_toggles_a_multi_option() {
        let mut form = Form::new("t", vec![Field::multi("m", options(&["a", "b", "c"]))]);
        form.handle_key(press(KeyCode::Char(' ')));
        form.handle_key(press(KeyCode::Char('j')));
        form.handle_key(press(KeyCode::Char('j')));
        form.handle_key(press(KeyCode::Char(' ')));
        form.handle_key(press(KeyCode::Char('k')));
        form.handle_key(press(KeyCode::Char(' ')));
        form.handle_key(press(KeyCode::Char(' ')));
        assert_eq!(
            form.fields[0].value(),
            FieldValue::Many(vec!["a".into(), "c".into()])
        );
    }

    #[test]
    fn an_empty_required_field_blocks_submit() {
        let mut form = Form::new(
            "t",
            vec![Field::text("note"), Field::text("reason").required()],
        );
        assert_eq!(form.handle_key(press(KeyCode::Enter)), FormOutcome::Pending);
        assert_eq!(form.error.as_deref(), Some("reason is required"));
        assert_eq!(form.focus, 1);
        type_str(&mut form, "ok");
        assert_eq!(form.error, None);
        assert!(matches!(
            form.handle_key(press(KeyCode::Enter)),
            FormOutcome::Submit(_)
        ));
    }

    #[test]
    fn esc_closes_an_open_dropdown_before_cancelling() {
        let select = Field::select("sev", options(&["critical", "warning"]));
        let mut form = Form::new("t", vec![select, Field::text("x")]);
        assert_eq!(form.handle_key(press(KeyCode::Enter)), FormOutcome::Pending);
        assert!(matches!(form.fields[0], Field::Select { open: true, .. }));
        assert_eq!(form.handle_key(press(KeyCode::Esc)), FormOutcome::Pending);
        assert!(matches!(form.fields[0], Field::Select { open: false, .. }));
        assert_eq!(form.handle_key(press(KeyCode::Esc)), FormOutcome::Cancel);
    }

    #[test]
    fn a_lone_open_select_submits_on_enter() {
        let menu = Field::select("action", options(&["defer", "wontfix"])).opened();
        let mut form = Form::new("t", vec![menu]);
        form.handle_key(press(KeyCode::Down));
        assert_eq!(
            form.handle_key(press(KeyCode::Enter)),
            FormOutcome::Submit(vec![FieldValue::One("wontfix".into())])
        );
    }

    #[test]
    fn other_needs_text_when_required() {
        let field = Field::select("category", options(&["a"]))
            .with_other()
            .required();
        let mut form = Form::new("t", vec![field, Field::text("x")]);
        form.handle_key(press(KeyCode::Down));
        assert!(form.fields[0].on_other());
        assert_eq!(form.handle_key(press(KeyCode::Tab)), FormOutcome::Pending);
        assert_eq!(form.handle_key(press(KeyCode::Enter)), FormOutcome::Pending);
        assert_eq!(form.error.as_deref(), Some("category is required"));
    }

    #[test]
    fn release_events_are_ignored() {
        let mut form = Form::new("t", vec![Field::text("a")]);
        let mut release = press(KeyCode::Char('x'));
        release.kind = KeyEventKind::Release;
        form.handle_key(release);
        assert_eq!(form.fields[0].value(), FieldValue::Text(String::new()));
        release.code = KeyCode::Esc;
        assert_eq!(form.handle_key(release), FormOutcome::Pending);
    }
}
