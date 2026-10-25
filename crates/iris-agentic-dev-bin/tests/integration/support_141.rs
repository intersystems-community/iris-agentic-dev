//! Shared by the spawn tests: an empty `HOME`, so no test reads the developer's own operator file.
//!
//! Spec 141 reads `~/.iris-agentic-dev.toml` on every start. The developer's own file (it can set
//! `destructive_tools_enabled = true`) would otherwise change what every spawned server does.

use std::path::Path;
use std::sync::OnceLock;

/// A directory that lives as long as the test process and holds nothing.
pub fn home() -> &'static Path {
    static HOME: OnceLock<tempfile::TempDir> = OnceLock::new();
    HOME.get_or_init(|| tempfile::tempdir().expect("an empty HOME"))
        .path()
}

/// `HOME` and `USERPROFILE` set to [`home`].
pub fn isolate(cmd: &mut std::process::Command) -> &mut std::process::Command {
    cmd.env("HOME", home()).env("USERPROFILE", home())
}
