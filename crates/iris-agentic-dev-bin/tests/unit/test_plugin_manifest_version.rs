//! Regression test: `.claude-plugin/plugin.json`'s `version` field is a plain JSON string
//! with nothing tying it to the workspace version — it silently drifted to `0.2.0` while
//! the workspace moved to `1.0.0` (three major versions, caught during a 2026-08 field-report
//! follow-up, not by any test). This pins the two together so the next release bump doesn't
//! repeat it.
//!
//! Deliberately does NOT check anything about tool/skill counts in the description field —
//! that used to hardcode "20 tools... 21 validated coding skills" (both wrong; the real
//! counts are 81 baseline tools and 33 skills at the time of writing, and both numbers move
//! independently of this file). The fix there was to stop asserting a specific count in
//! prose, not to pin a new one — a hardcoded count in a manifest description is exactly the
//! kind of thing that drifts silently again.

#[test]
fn plugin_json_version_matches_workspace_version() {
    let mut path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop(); // iris-agentic-dev-bin → crates
    path.pop(); // crates → workspace root
    path.push(".claude-plugin/plugin.json");

    let content = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    let manifest: serde_json::Value =
        serde_json::from_str(&content).expect(".claude-plugin/plugin.json must be valid JSON");
    let plugin_version = manifest["version"]
        .as_str()
        .expect(".claude-plugin/plugin.json must have a string \"version\" field");

    // Workspace version, resolved at compile time from this crate's own Cargo.toml
    // (which inherits `version.workspace = true` from the workspace root).
    let workspace_version = env!("CARGO_PKG_VERSION");

    assert_eq!(
        plugin_version, workspace_version,
        ".claude-plugin/plugin.json version ({plugin_version}) does not match the workspace \
         version ({workspace_version}) — update plugin.json alongside the next release bump."
    );
}

/// The extension's `package-lock.json` carries the extension's own version in two places, and
/// nothing bumped them: the lock sat at `0.4.31` while `package.json` shipped `0.4.32`. `npm ci`
/// tolerates that today, which is why it went unnoticed for a release, but the two disagreeing is
/// the same silent drift the test above exists to stop.
///
/// This checks the lock against `package.json`, not against the workspace version — the extension
/// semver (`0.4.x`) and the server version (`1.4.x`) are separate sequences on purpose.
#[test]
fn extension_package_lock_version_matches_package_json() {
    let mut root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    root.pop(); // iris-agentic-dev-bin → crates
    root.pop(); // crates → workspace root
    let dir = root.join("vscode-iris-agentic-dev");

    let read = |name: &str| -> serde_json::Value {
        let path = dir.join(name);
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("{} is not valid JSON: {e}", name))
    };

    let pkg = read("package.json");
    let lock = read("package-lock.json");
    let declared = pkg["version"]
        .as_str()
        .expect("package.json must have a string \"version\"");

    assert_eq!(
        lock["version"].as_str(),
        Some(declared),
        "package-lock.json top-level version does not match package.json ({declared})"
    );
    assert_eq!(
        lock["packages"][""]["version"].as_str(),
        Some(declared),
        "package-lock.json packages.\"\".version does not match package.json ({declared}) — npm \
         writes the root version in both places and both have to move"
    );
}
