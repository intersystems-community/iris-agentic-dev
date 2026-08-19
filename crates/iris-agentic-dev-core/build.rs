//! Embeds a `+<git-describe>` build-metadata suffix (semver-compatible) into
//! `SERVER_VERSION` (see `tools/mod.rs`), so `check_config` can distinguish a
//! local/fork build from an official tagged release even when `Cargo.toml`'s
//! version hasn't been bumped.
//!
//! The distinguishing signal is NOT "was `.git` available at build time" —
//! release builds (`.github/workflows/release.yml`) use `actions/checkout`,
//! which leaves a real `.git` directory in place, so that check would tag
//! official releases too. Instead: a build is a clean, official release only
//! when it's checked out exactly at the tag matching this crate's own
//! version, with no local modifications — `git describe --tags` naturally
//! returns the bare tag name in that exact case (no `-N-g<hash>` suffix), and
//! something else (ahead of the tag, dirty, or no matching tag) otherwise.
//! `--tags` is required: this repo's release tags (e.g. `v1.0.0`) are
//! lightweight, and `git describe` ignores lightweight tags without it,
//! silently falling back to an older *annotated* tag instead.

use std::process::Command;

fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
    let pkg_version = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();

    let describe = Command::new("git")
        .args(["describe", "--tags", "--always", "--dirty"])
        .current_dir(&manifest_dir)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let suffix = match describe {
        // Exactly at the tag matching this crate's own version, clean tree:
        // a genuine tagged release build - no build metadata needed.
        Some(ref d) if *d == format!("v{pkg_version}") => String::new(),
        Some(d) => format!("+{d}"),
        None => String::new(),
    };

    println!("cargo:rustc-env=IRIS_AGENTIC_DEV_BUILD_SUFFIX={suffix}");
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs");
}
