//! `tomlctl agents` — hook-written agent lifecycle records over
//! `.claude/flows/<slug>/agents.toml`.
//!
//! The store's array is named `agents`, never `items`: an array named `items`
//! in a file under `.claude/` is the default target of
//! `tomlctl items add|update|apply`, whose dedup stamping would add a
//! `dedup_id` to every record.

pub(crate) mod correlate;
pub(crate) mod dispatch;
pub(crate) mod list;
pub(crate) mod record;
pub(crate) mod schema;
