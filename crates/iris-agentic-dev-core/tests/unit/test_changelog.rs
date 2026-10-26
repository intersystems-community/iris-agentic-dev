//! Issue #136: VS Code shows an extension's `CHANGELOG.md` as a tab on its page, so the extension
//! carries one. It is built from `docs/release-notes/`, newest first, and these tests fail when it
//! falls behind them. `IAD_REGEN_CHANGELOG=1` rewrites it instead.

use std::path::{Component, Path, PathBuf};

const BLOB: &str = "https://github.com/intersystems-community/iris-agentic-dev/blob/master/";

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(rel: &str) -> String {
    let path = root().join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
}

fn extension_versions() -> (String, String) {
    let pkg: serde_json::Value =
        serde_json::from_str(&read("vscode-iris-agentic-dev/package.json")).expect("package.json");
    (
        pkg["version"].as_str().expect("version").to_string(),
        pkg["irisAgenticDev"]["serverVersion"]
            .as_str()
            .expect("irisAgenticDev.serverVersion")
            .to_string(),
    )
}

/// `docs/release-notes/vX.Y.Z.md`, newest first.
fn release_notes() -> Vec<(String, String)> {
    let dir = root().join("docs/release-notes");
    let mut notes: Vec<((u32, u32, u32), String, String)> = std::fs::read_dir(&dir)
        .expect("docs/release-notes")
        .filter_map(|e| {
            let name = e.ok()?.file_name().into_string().ok()?;
            let version = name.strip_prefix('v')?.strip_suffix(".md")?.to_string();
            let mut parts = version.split('.').map(|p| p.parse::<u32>());
            let key = (
                parts.next()?.ok()?,
                parts.next()?.ok()?,
                parts.next()?.ok()?,
            );
            Some((key, version, read(&format!("docs/release-notes/{name}"))))
        })
        .collect();
    notes.sort_by_key(|n| std::cmp::Reverse(n.0));
    notes.into_iter().map(|(_, v, text)| (v, text)).collect()
}

/// `docs/release-notes/../connecting.md` as a repository path, `docs/connecting.md`.
fn repo_path(target: &str) -> String {
    let mut parts: Vec<String> = vec!["docs".into(), "release-notes".into()];
    for c in Path::new(target).components() {
        match c {
            Component::ParentDir => {
                parts.pop();
            }
            Component::Normal(s) => parts.push(s.to_string_lossy().into_owned()),
            _ => {}
        }
    }
    parts.join("/")
}

fn is_relative(target: &str) -> bool {
    !(target.starts_with("http://")
        || target.starts_with("https://")
        || target.starts_with("mailto:")
        || target.starts_with('#'))
}

/// A link relative to `docs/release-notes/` points nowhere from the extension's page.
fn absolute_links(line: &str) -> String {
    let mut out = String::new();
    let mut rest = line;
    while let Some(i) = rest.find("](") {
        out.push_str(&rest[..i + 2]);
        rest = &rest[i + 2..];
        let end = rest.find(')').unwrap_or(rest.len());
        let target = &rest[..end];
        if is_relative(target) {
            let (path, anchor) = target.split_at(target.find('#').unwrap_or(target.len()));
            out.push_str(BLOB);
            out.push_str(&repo_path(path));
            out.push_str(anchor);
        } else {
            out.push_str(target);
        }
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

/// One release's notes as a changelog entry: every heading one level down, so the file keeps a
/// single `#` title, and links made absolute. Lines inside code fences are left alone.
fn entry(text: &str) -> String {
    let mut fenced = false;
    text.trim_end()
        .lines()
        .map(|line| {
            if line.trim_start().starts_with("```") {
                fenced = !fenced;
                return line.to_string();
            }
            if fenced {
                return line.to_string();
            }
            let line = absolute_links(line);
            if line.starts_with('#') {
                format!("#{line}")
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn expected_changelog() -> String {
    let (extension, server) = extension_versions();
    let mut out = format!(
        "# Changelog\n\n\
         This extension installs the iris-agentic-dev server, and each entry below is a server \
         release. Extension {extension} installs server {server}. Releases with no entry here have \
         their notes on the [releases page](https://github.com/intersystems-community/iris-agentic-dev/releases).\n"
    );
    for (_, text) in release_notes() {
        out.push('\n');
        out.push_str(&entry(&text));
        out.push('\n');
    }
    out
}

#[test]
fn the_changelog_is_built_from_the_release_notes() {
    let path = root().join("vscode-iris-agentic-dev/CHANGELOG.md");
    let expected = expected_changelog();
    if std::env::var("IAD_REGEN_CHANGELOG").is_ok_and(|v| v == "1") {
        std::fs::write(&path, &expected).expect("writing CHANGELOG.md");
        return;
    }
    let found = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        found == expected,
        "vscode-iris-agentic-dev/CHANGELOG.md does not match docs/release-notes/; \
         rerun with IAD_REGEN_CHANGELOG=1"
    );
}

#[test]
fn the_first_entry_is_the_server_the_extension_installs() {
    let (_, server) = extension_versions();
    let first = release_notes().into_iter().next().expect("release notes").0;
    assert_eq!(
        first, server,
        "the newest release notes must be for the server the extension installs"
    );
    assert_eq!(
        first,
        env!("CARGO_PKG_VERSION"),
        "the newest release notes must be for the workspace version"
    );
}

#[test]
fn each_release_note_is_titled_with_its_own_version() {
    for (version, text) in release_notes() {
        assert_eq!(
            text.lines().next(),
            Some(format!("# v{version}").as_str()),
            "docs/release-notes/v{version}.md must start with '# v{version}'"
        );
    }
}

#[test]
fn links_are_made_absolute_and_code_is_left_alone() {
    let text = "# v9.9.9\n\nSee [the docs](../connecting.md#irisaudit) and \
                [install](../../README.md#install) or [web](https://example.com/x).\n\n\
                ```bash\n# a shell comment\n```\n\n## Security\n";
    assert_eq!(
        entry(text),
        format!(
            "## v9.9.9\n\nSee [the docs]({BLOB}docs/connecting.md#irisaudit) and \
             [install]({BLOB}README.md#install) or [web](https://example.com/x).\n\n\
             ```bash\n# a shell comment\n```\n\n### Security"
        )
    );
}
