//! The document and link types a details builder returns.

use ratatui::text::Text;

use crate::nav::Target;

/// A details builder's output: the text the panel draws and the records it relates to.
#[derive(Debug, Clone, Default)]
pub(crate) struct Doc {
    pub(crate) text: Text<'static>,
    /// In the order the text mentions them.
    pub(crate) links: Vec<Link>,
}

impl From<Text<'static>> for Doc {
    fn from(text: Text<'static>) -> Self {
        Self {
            text,
            links: Vec::new(),
        }
    }
}

/// One related record, as the links list names it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct Link {
    pub(crate) label: String,
    pub(crate) target: Target,
}
