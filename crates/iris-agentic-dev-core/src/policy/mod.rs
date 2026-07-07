//! Security gate layer (051-phi-policy-env-gates).
//!
//! Single dispatch point enforcing four ordered gates before any tool executes:
//! (1) env template, (2) bulk-PHI hard-block, (3) system blocklist, (4) per-global PHI name.

pub mod code_edit_gate;
pub mod data_policy_gate;
pub mod env_gate;
pub mod gate;
pub mod patterns;
pub mod system_blocklist_gate;
