//! Spec 141 T183: the same isolation guard, run inside the integration target's process.
//!
//! That target spawns the most test threads against live services and is the one that hung, so it
//! checks its own process rather than trusting the unit target's.

#[path = "../unit/test_isolation_141.rs"]
mod shared;
