//! PTY supervisor subsystem — interactive `claude` REPL sessions.
//!
//! Module bodies are filled in by Tasks 2–8 of the lumina-pty-service plan.
//! `mod.rs` is frozen after T2: subsequent tasks only add content to their
//! owned files. Public re-exports from `protocol` are listed below; future
//! tasks add re-exports to this list as they land.

pub mod protocol;
pub mod transport;        // T3
pub mod pty_transport;    // T4
pub mod parser;           // T5
pub mod session;          // T6
pub mod registry;         // T6
pub mod queue;            // T6
pub mod supervisor;       // T8

pub use protocol::{
    InputFrame, InputKind, MessageKind, SessionId, SessionStatus, TypedMessage,
};
