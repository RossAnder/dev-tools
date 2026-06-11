//! Home of the concrete [`crate::stream::TopicResolver`] implementations
//! registered into [`crate::stream::TopicRegistry::with_default_topics`].
//!
//! One submodule per topic family. Adding a new streamable resource =
//! one new submodule implementing `TopicResolver` + one `register` line in
//! `with_default_topics` — no route, handler, or client-socket changes.

pub mod sprint_quiescence;
