//! Manifest ⇄ disk reconciliation for the shipped skill pack.
//!
//! Three independent consumers each carry their own list of skills:
//!
//! * `iris-agentic-dev.toml` — `[provides] skills`, what `iris-agentic-dev skill install` fetches
//! * `skills.sh.json` — `groupings`, what the skills.sh registry page renders
//! * `skills/skills/<name>/SKILL.md` on disk — what actually exists
//!
//! Nothing kept them in agreement, so they drifted in both directions. On 2026-07-25 the
//! install manifest named two skills that had been deleted (`iris-vector-graph`,
//! `iris-vector-rag` — ownership moved to their own repos in commit `1fb7a8c`), which made
//! `skill install` 404 on 2 of 31 skills for every user; `skills.sh.json` named four that
//! did not exist and listed two of them twice, while omitting `objectscript-coverage`
//! entirely. Both files were "verified" by tasks that never compared them to disk.
//!
//! These tests are the comparison. A skill added or removed on disk fails here until every
//! manifest is updated to match.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

/// Skill directories that actually ship: `skills/skills/<name>/SKILL.md`.
fn skills_on_disk() -> BTreeSet<String> {
    let dir = repo_root().join("skills").join("skills");
    let mut found = BTreeSet::new();
    for entry in std::fs::read_dir(&dir)
        .expect("skills/skills must exist")
        .flatten()
    {
        if entry.path().join("SKILL.md").is_file() {
            found.insert(entry.file_name().to_string_lossy().to_string());
        }
    }
    assert!(
        !found.is_empty(),
        "no skills discovered under {} — the pack cannot be empty",
        dir.display()
    );
    found
}

/// `[provides] skills = [...]` from the install manifest, reduced to bare skill names.
fn skills_in_install_manifest() -> Vec<String> {
    let path = repo_root().join("iris-agentic-dev.toml");
    let raw = std::fs::read_to_string(&path).expect("iris-agentic-dev.toml must exist");
    let parsed: toml::Value = raw
        .parse()
        .expect("iris-agentic-dev.toml must be valid TOML");

    parsed
        .get("provides")
        .and_then(|p| p.get("skills"))
        .and_then(|s| s.as_array())
        .expect("iris-agentic-dev.toml must define [provides] skills")
        .iter()
        .map(|v| {
            let path = v.as_str().expect("every skills entry must be a string");
            path.rsplit('/')
                .next()
                .expect("path must have a final segment")
                .to_string()
        })
        .collect()
}

/// Every skill named in `skills.sh.json` groupings, in declaration order.
fn skills_in_registry_manifest() -> Vec<String> {
    let path = repo_root().join("skills.sh.json");
    let raw = std::fs::read_to_string(&path).expect("skills.sh.json must exist");
    let parsed: serde_json::Value =
        serde_json::from_str(&raw).expect("skills.sh.json must be valid JSON");

    parsed
        .get("groupings")
        .and_then(|g| g.as_array())
        .expect("skills.sh.json must have a groupings array")
        .iter()
        .flat_map(|group| {
            group
                .get("skills")
                .and_then(|s| s.as_array())
                .expect("every grouping must have a skills array")
                .iter()
                .map(|s| {
                    s.as_str()
                        .expect("every grouping skill must be a string")
                        .to_string()
                })
        })
        .collect()
}

#[test]
fn install_manifest_names_only_skills_that_exist() {
    let disk = skills_on_disk();
    let phantom: Vec<_> = skills_in_install_manifest()
        .into_iter()
        .filter(|s| !disk.contains(s))
        .collect();

    assert!(
        phantom.is_empty(),
        "iris-agentic-dev.toml names {} skill(s) with no directory on disk: {:?}\n\
         `skill install` fetches these paths from HEAD and will 404 for every user.",
        phantom.len(),
        phantom
    );
}

#[test]
fn install_manifest_covers_every_skill_on_disk() {
    let listed: BTreeSet<String> = skills_in_install_manifest().into_iter().collect();
    let missing: Vec<_> = skills_on_disk()
        .into_iter()
        .filter(|s| !listed.contains(s))
        .collect();

    assert!(
        missing.is_empty(),
        "{} skill(s) exist on disk but are absent from [provides] skills in \
         iris-agentic-dev.toml, so `skill install` will never install them: {:?}",
        missing.len(),
        missing
    );
}

#[test]
fn registry_manifest_names_only_skills_that_exist() {
    let disk = skills_on_disk();
    let phantom: Vec<_> = skills_in_registry_manifest()
        .into_iter()
        .filter(|s| !disk.contains(s))
        .collect();

    assert!(
        phantom.is_empty(),
        "skills.sh.json names {} skill(s) with no directory on disk: {:?}",
        phantom.len(),
        phantom
    );
}

#[test]
fn registry_manifest_covers_every_skill_on_disk() {
    let listed: BTreeSet<String> = skills_in_registry_manifest().into_iter().collect();
    let missing: Vec<_> = skills_on_disk()
        .into_iter()
        .filter(|s| !listed.contains(s))
        .collect();

    assert!(
        missing.is_empty(),
        "{} skill(s) exist on disk but appear in no skills.sh.json grouping, so they are \
         invisible on the registry page: {:?}",
        missing.len(),
        missing
    );
}

#[test]
fn registry_manifest_lists_each_skill_exactly_once() {
    let all = skills_in_registry_manifest();
    let mut duplicated: Vec<_> = {
        let unique: BTreeSet<_> = all.iter().collect();
        unique
            .into_iter()
            .filter(|s| all.iter().filter(|x| x == s).count() > 1)
            .cloned()
            .collect()
    };
    duplicated.sort();

    assert!(
        duplicated.is_empty(),
        "skills.sh.json lists {} skill(s) in more than one grouping: {:?}",
        duplicated.len(),
        duplicated
    );
}

/// The `skills` CLI (`npx skills add`) parses frontmatter as strict YAML and *skips* any
/// SKILL.md that fails to parse — with a warning that is easy to miss in a long list.
///
/// `skills/iris-coverage-run/SKILL.md` was silently dropped this way: its unquoted
/// `description` contained `Prerequisite: iris-coverage-setup`, and the bare `: ` reads as a
/// nested mapping ("Nested mappings are not allowed in compact mappings"). The skill looked
/// completely fine on disk and in git.
///
/// This scans every SKILL.md in the repo, not just the shipped pack, because the CLI globs
/// the whole checkout.
#[test]
fn no_frontmatter_value_breaks_strict_yaml_parsing() {
    fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            if path.is_dir() {
                // Skip build output and vendored trees; keep dotted agent dirs.
                if name == "target" || name == "node_modules" || name == ".git" {
                    continue;
                }
                collect(&path, out);
            } else if name == "SKILL.md" {
                out.push(path);
            }
        }
    }

    let root = repo_root();
    let mut files = Vec::new();
    collect(&root, &mut files);
    assert!(!files.is_empty(), "expected to find SKILL.md files");

    let mut offenders = Vec::new();
    for path in &files {
        let Ok(content) = std::fs::read_to_string(path) else {
            continue;
        };
        let Some(rest) = content.strip_prefix("---") else {
            continue;
        };
        let Some(frontmatter) = rest.split("\n---").next() else {
            continue;
        };

        for line in frontmatter.lines() {
            // Only top-level keys matter here; indented lines are inside nested mappings
            // where the parser applies different rules.
            if line.starts_with(char::is_whitespace) {
                continue;
            }
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            if key.contains(char::is_whitespace) {
                continue;
            }
            let value = value.trim();
            // A value that opens with a quote or block scalar is already safe.
            if value.is_empty() || value.starts_with(['"', '\'', '|', '>']) {
                continue;
            }
            // An unquoted scalar containing ": " parses as a nested mapping and throws.
            if value.contains(": ") {
                offenders.push(format!(
                    "{}: unquoted `{}` contains \": \" — wrap the value in double quotes",
                    path.strip_prefix(&root).unwrap_or(path).display(),
                    key.trim()
                ));
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "{} SKILL.md frontmatter value(s) will fail strict YAML parsing, and `npx skills add` \
         silently skips the skill:\n  {}",
        offenders.len(),
        offenders.join("\n  ")
    );
}
