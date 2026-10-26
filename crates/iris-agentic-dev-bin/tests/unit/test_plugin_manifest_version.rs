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

/// The plugin launched `iris-dev`, a name no install channel has shipped since the rename: Cargo
/// builds `iris-agentic-dev` and the Homebrew formula installs it under that name. On a clean
/// Homebrew install the plugin's MCP server failed to start; on a machine with an old
/// `~/.local/bin/iris-dev` it started silently on 0.9.10 while 1.4.2 was installed.
///
/// Checks every `mcpServers.*.command` against the Cargo `[[bin]]` name and against the name the
/// formula installs, so a rename in any one of the three fails here.
#[test]
fn plugin_json_command_matches_installed_binary_name() {
    let mut root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    root.pop(); // iris-agentic-dev-bin → crates
    root.pop(); // crates → workspace root
    let read = |rel: &str| -> String {
        let path = root.join(rel);
        std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()))
    };

    let cargo = read("crates/iris-agentic-dev-bin/Cargo.toml");
    let sections: Vec<&str> = cargo.split("[[bin]]").skip(1).collect();
    assert_eq!(
        sections.len(),
        1,
        "expected exactly one [[bin]] in the bin crate"
    );
    let cargo_bin = sections[0]
        .lines()
        .find_map(|l| l.trim().strip_prefix("name"))
        .and_then(|rest| rest.trim().strip_prefix('='))
        .map(|s| s.trim().trim_matches('"'))
        .expect("[[bin]] needs a name");

    let formula = read("Formula/iris-agentic-dev.rb");
    let installed = formula
        .lines()
        .find_map(|l| l.trim().strip_prefix("bin.install "))
        .and_then(|rest| rest.rsplit("=>").next())
        .map(|s| s.trim().trim_matches('"'))
        .expect("formula must have a `bin.install <src> => \"<name>\"` line");
    assert_eq!(
        installed, cargo_bin,
        "Formula installs `{installed}` but Cargo builds `{cargo_bin}`"
    );

    let manifest: serde_json::Value = serde_json::from_str(&read(".claude-plugin/plugin.json"))
        .expect(".claude-plugin/plugin.json must be valid JSON");
    let servers = manifest["mcpServers"]
        .as_object()
        .expect("plugin.json must have an mcpServers object");
    assert!(!servers.is_empty(), "plugin.json declares no MCP servers");
    for (name, server) in servers {
        let command = server["command"]
            .as_str()
            .unwrap_or_else(|| panic!("mcpServers.{name}.command must be a string"));
        assert_eq!(
            command, cargo_bin,
            "plugin.json mcpServers.{name} launches `{command}`, but the installed binary is \
             `{cargo_bin}` — the plugin cannot start its server on a clean install"
        );
    }
}

/// The direct-download instructions pin a release tag so the binary a reader checks against
/// `SHA256SUMS` is the version the docs describe. A pinned tag is one more place the version is
/// written by hand, so each of these files has to move with the workspace version on a release
/// bump, and none of them may go back to `releases/latest/download`.
#[test]
fn install_docs_pin_the_workspace_version() {
    let mut root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    root.pop(); // iris-agentic-dev-bin → crates
    root.pop(); // crates → workspace root
    let workspace_version = env!("CARGO_PKG_VERSION");

    // Any `vX.Y.Z` in these files is a release pin (URL, link text, or prose naming the
    // version the commands fetch), and so is the setup skill's `VERSION=X.Y.Z`.
    let markers = ["v", "VERSION="];

    for rel in [
        "README.md",
        "docs/cursor-quickstart.md",
        "skills/BENCHMARKING.md",
        "skills/iris-agentic-dev-setup/SKILL.md",
    ] {
        let path = root.join(rel);
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));

        assert!(
            !text.contains("releases/latest/download"),
            "{rel} downloads from releases/latest; pin a tag so the checksum check means something"
        );
        assert!(
            text.contains("SHA256SUMS"),
            "{rel} gives a download command but no SHA256SUMS check"
        );

        let mut pins = Vec::new();
        for marker in markers {
            for (at, _) in text.match_indices(marker) {
                // `v` only counts at the start of a token, not inside `dev1.2` or `nv1.2`.
                if text[..at]
                    .chars()
                    .next_back()
                    .is_some_and(|c| c.is_ascii_alphanumeric())
                {
                    continue;
                }
                let rest = &text[at + marker.len()..];
                let end = rest
                    .find(|c: char| !(c.is_ascii_digit() || c == '.'))
                    .unwrap_or(rest.len());
                let pin = rest[..end].trim_end_matches('.');
                if pin.contains('.') {
                    pins.push(pin.to_string());
                }
            }
        }
        assert!(!pins.is_empty(), "{rel} names no pinned release version");
        for pin in &pins {
            assert_eq!(
                pin, workspace_version,
                "{rel} pins v{pin} but the workspace is {workspace_version} — update the install \
                 commands alongside the release bump"
            );
        }
    }
}
