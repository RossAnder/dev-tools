//! lumina-companion: the git-EXECUTION plane of the ADR-0006 control/execution
//! split. The companion performs the git operations the control plane
//! (lumina-server) only RECORDS; the two meet over the `lumina-protocol` wire
//! types (companion dials the server over WebSocket — Task 6).
//!
//! Dependency rule (load-bearing): this crate depends on `lumina-protocol`
//! ONLY — never on `lumina-core`, `lumina-server`, or any DB type. The
//! `cargo tree` gate in the Step-1b plan keeps this honest.
//!
//! Module map: [`git`] holds the engine-neutral `GitBackend` seam (+ the
//! `FakeGitBackend` double); [`executor`] maps one protocol `Intent` to one
//! `Outcome` over that seam; [`connection`] is the WS dial loop that feeds
//! intents to the executor (Task 6).

pub mod connection;
pub mod executor;
pub mod git;
