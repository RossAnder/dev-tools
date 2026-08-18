//! Style implementations. All three are pure `payload -> String`: nothing here
//! reads the filesystem, spawns a process or reads the clock. Everything a
//! renderer needs from outside the payload — the branch and diff counts
//! (`crate::git`), teammate metadata and inbox depths (`crate::teamdata`), the
//! current time — is resolved in `main` and threaded in as an argument.
//!
//! That is the invariant, and it is what makes the styles testable at all;
//! `full::render` in particular is only coverable at its width tiers because
//! its git facts are parameters. Keep it: if a style needs a new fact from
//! outside, add a parameter rather than a call into an I/O module.

pub mod agents;
pub mod full;
pub mod min;
