//! Every configuration key is classified (E5).
//!
//! `config_sources::SETTINGS` says, for each key a configuration file can carry, whether it
//! loosens, tightens or is neutral to the limits, or is the operator's alone; `LAUNCH_SETTINGS`
//! says the same for each environment variable. What this file checks:
//!
//! 1. The keys the code reads are the keys the table lists. The serde structs of
//!    `workspace_config.rs` (field names, renames, aliases) and the keys `config_sources.rs` and
//!    `operator_hold.rs` read off the raw table are collected from the source, so a key added to
//!    either, and not to the table, fails by name. The source is scanned because nothing else
//!    lists the raw reads: that is how `mode` was missed.
//! 2. The table is true. Each row's example document goes through `loosening_settings` and
//!    `operator_only_items`: a `Loosening` row's is reported, a `Tightening` or `Neutral` row's
//!    is not, an `OperatorOnly` row's is set aside.
//! 3. The environment variables the process reads (`BEHAVIOR_ENV_VARS`) are the ones
//!    `LAUNCH_SETTINGS` lists, and the product never writes the environment, so a file's value
//!    cannot become a launch setting.
//!
//! Nothing here touches IRIS.

use iris_agentic_dev_core::iris::config_sources::{
    loosening_settings, operator_only_items, SettingClass, LAUNCH_SETTINGS, OPERATOR_ONLY_KEYS,
    SETTINGS,
};
use iris_agentic_dev_core::testing::BEHAVIOR_ENV_VARS;
use regex::Regex;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn src(rel: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join(rel);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    // Production code only: a test module may use any key it likes.
    let end = text.find("#[cfg(test)]").unwrap_or(text.len());
    text[..end].to_string()
}

fn table(doc: &str) -> toml::Table {
    doc.parse::<toml::Table>()
        .unwrap_or_else(|e| panic!("example {doc:?} is not TOML: {e}"))
}

fn declared_keys() -> BTreeSet<&'static str> {
    SETTINGS.iter().map(|d| d.key).collect()
}

fn leaf(key: &str) -> &str {
    key.rsplit('.').next().unwrap_or(key)
}

// ── (1) the keys the code reads ─────────────────────────────────────────────────────────────────

/// The public fields of `struct name` in `text`, as the keys serde reads: the rename if there is
/// one, the field name otherwise, and every alias. A field that is `skip`ped or `flatten`ed is not
/// a key.
fn struct_keys(text: &str, name: &str) -> Vec<String> {
    let head = format!("pub struct {name} {{");
    let start = text
        .find(&head)
        .unwrap_or_else(|| panic!("no `{head}` in workspace_config.rs"))
        + head.len();
    let body = &text[start..];
    let body = &body[..body.find("\n}\n").expect("struct end")];
    let rename = Regex::new(r#"rename\s*=\s*"([^"]+)""#).unwrap();
    let alias = Regex::new(r#"alias\s*=\s*"([^"]+)""#).unwrap();
    let field = Regex::new(r"^\s*pub (?:r#)?(\w+)\s*:").unwrap();
    let mut keys = Vec::new();
    let mut pending = String::new();
    for line in body.lines() {
        let t = line.trim_start();
        if t.starts_with("///") || t.starts_with("//") {
            continue;
        }
        if let Some(c) = field.captures(line) {
            let skipped = pending.contains("skip") || pending.contains("flatten");
            if !skipped {
                match rename.captures(&pending) {
                    Some(r) => keys.push(r[1].to_string()),
                    None => keys.push(c[1].to_string()),
                }
                keys.extend(alias.captures_iter(&pending).map(|a| a[1].to_string()));
            }
            pending.clear();
        } else {
            pending.push_str(line);
            pending.push('\n');
        }
    }
    keys
}

/// Names that hold other keys, not settings themselves.
const CONTAINERS: &[&str] = &["policy", "instance", "transport"];

fn struct_scan() -> BTreeSet<String> {
    let text = src("iris/workspace_config.rs");
    let mut out = BTreeSet::new();
    for (name, prefix) in [
        ("WorkspaceConfig", ""),
        ("FleetConfig", ""),
        ("InstanceConfig", "instance.*."),
        ("ConnectionPolicyRaw", "policy.*."),
    ] {
        for k in struct_keys(&text, name) {
            if prefix.is_empty() && CONTAINERS.contains(&k.as_str()) {
                continue;
            }
            out.insert(format!("{prefix}{k}"));
        }
    }
    out
}

/// Leaf names the loader reads from the raw table, by the idioms it reads them with.
fn raw_reads() -> BTreeSet<String> {
    let patterns = [
        r#"\.get\("([A-Za-z_][\w-]*)"\)"#,
        r#"\("([A-Za-z_]\w*)",\s*(?:true|false)\)"#,
        r#"\bb\("(\w+)"\)"#,
        r#"lookup\(table,\s*"([\w.]+)"\)"#,
        r#"policy_flag_servers\(table,\s*"(\w+)"\)"#,
    ];
    let mut out = BTreeSet::new();
    for file in ["iris/config_sources.rs", "iris/operator_hold.rs"] {
        let text = src(file);
        for p in patterns {
            for c in Regex::new(p).unwrap().captures_iter(&text) {
                out.insert(c[1].to_string());
            }
        }
    }
    for k in OPERATOR_ONLY_KEYS {
        out.insert(k.to_string());
    }
    out
}

/// Keys of the records inside a table that is itself one setting (`[[trust]]` entries), and of
/// the nested lookups that are not settings.
const INNER: &[&str] = &["path", "format", "settings", "default", "http_token"];

#[test]
fn every_key_the_code_reads_is_classified() {
    let declared = declared_keys();
    let declared_leaves: BTreeSet<&str> = declared.iter().map(|k| leaf(k)).collect();

    // The scans have to see what they claim to, or an empty result would pass.
    let structs = struct_scan();
    let raw = raw_reads();
    assert!(structs.len() >= 30, "the struct scan found {structs:?}");
    for k in [
        "instance.*.role",
        "policy.*.allow",
        "write_tools_enabled",
        "port",
    ] {
        assert!(
            structs.contains(k),
            "the struct scan missed {k}: {structs:?}"
        );
    }
    for k in [
        "mode",
        "read_only",
        "trust",
        "telemetry_sink",
        "execAsOsUser",
    ] {
        assert!(raw.contains(k), "the raw-read scan missed {k}: {raw:?}");
    }

    let mut unclassified: Vec<String> = Vec::new();
    for key in struct_scan() {
        if !declared.contains(key.as_str()) {
            unclassified.push(format!("{key} (a field of a config struct)"));
        }
    }
    for key in raw_reads() {
        let l = leaf(&key);
        if CONTAINERS.contains(&l) || INNER.contains(&l) {
            continue;
        }
        if !declared.contains(key.as_str()) && !declared_leaves.contains(l) {
            unclassified.push(format!("{key} (read off the raw table)"));
        }
    }
    assert!(
        unclassified.is_empty(),
        "these configuration keys are read and not in config_sources::SETTINGS: {unclassified:?}.\n\
         Say what the key does to the limits: Loosening (a file that is not trusted has it set \
         aside, and `loosening_settings` must report it), Tightening, Neutral (with the reason) or \
         OperatorOnly."
    );
}

#[test]
fn every_classified_key_is_one_the_code_reads() {
    let mut known: BTreeSet<String> = struct_scan();
    known.extend(raw_reads());
    let known_leaves: BTreeSet<String> = known.iter().map(|k| leaf(k).to_string()).collect();
    let stale: Vec<_> = SETTINGS
        .iter()
        .filter(|d| !known.contains(d.key) && !known_leaves.contains(leaf(d.key)))
        .map(|d| d.key)
        .collect();
    assert!(
        stale.is_empty(),
        "config_sources::SETTINGS lists keys nothing reads: {stale:?}"
    );
    let mut seen = BTreeSet::new();
    for d in SETTINGS {
        assert!(seen.insert(d.key), "{} is classified twice", d.key);
        assert!(d.reason.len() >= 12, "{} has no reason", d.key);
    }
}

// ── (2) the table is true ───────────────────────────────────────────────────────────────────────

#[test]
fn each_example_is_treated_as_its_class_says() {
    for d in SETTINGS {
        let t = table(d.example);
        let loosening = loosening_settings(&t);
        let operator_only = operator_only_items(&t, Path::new("/project/.iris-agentic-dev.toml"));
        let l = leaf(d.key);
        match d.class {
            SettingClass::Loosening => {
                assert!(
                    loosening.iter().any(|line| line.contains(l)),
                    "{} is declared Loosening and `loosening_settings` does not report it for \
                     {:?}: a workspace file that is not trusted would set it. Got {loosening:?}",
                    d.key,
                    d.example
                );
                assert!(
                    operator_only.is_empty(),
                    "{} is Loosening and operator-only",
                    d.key
                );
            }
            SettingClass::OperatorOnly => {
                assert!(
                    !operator_only.is_empty(),
                    "{} is declared OperatorOnly and `operator_only_items` sets nothing aside \
                     for {:?}: a workspace file could set it",
                    d.key,
                    d.example
                );
            }
            SettingClass::Tightening | SettingClass::Neutral => {
                assert!(
                    loosening.is_empty(),
                    "{} is declared {:?} and `loosening_settings` reports {loosening:?} for {:?}",
                    d.key,
                    d.class,
                    d.example
                );
                assert!(
                    operator_only.is_empty(),
                    "{} is declared {:?} and is set aside as operator-only for {:?}",
                    d.key,
                    d.class,
                    d.example
                );
            }
        }
    }
}

#[test]
fn the_operator_only_keys_are_the_operator_only_rows() {
    let rows: BTreeSet<&str> = SETTINGS
        .iter()
        .filter(|d| d.class == SettingClass::OperatorOnly)
        .map(|d| d.key)
        .filter(|k| !k.contains('*'))
        .collect();
    let keys: BTreeSet<&str> = OPERATOR_ONLY_KEYS.iter().copied().collect();
    assert_eq!(
        rows, keys,
        "OPERATOR_ONLY_KEYS (what a workspace file has set aside) and the OperatorOnly rows of \
         SETTINGS must name the same keys"
    );
}

// ── (3) the environment ─────────────────────────────────────────────────────────────────────────

#[test]
fn every_behaviour_variable_is_classified() {
    let declared: BTreeSet<&str> = LAUNCH_SETTINGS.iter().map(|(n, _, _)| *n).collect();
    let read: BTreeSet<&str> = BEHAVIOR_ENV_VARS.iter().copied().collect();
    let unclassified: Vec<_> = read.difference(&declared).collect();
    assert!(
        unclassified.is_empty(),
        "these environment variables are read and not in config_sources::LAUNCH_SETTINGS: \
         {unclassified:?}"
    );
    let stale: Vec<_> = declared.difference(&read).collect();
    assert!(
        stale.is_empty(),
        "LAUNCH_SETTINGS lists variables BEHAVIOR_ENV_VARS does not: {stale:?}"
    );
    assert_eq!(declared.len(), LAUNCH_SETTINGS.len(), "a variable twice");
    for (name, _, reason) in LAUNCH_SETTINGS {
        assert!(reason.len() >= 8, "{name} has no reason");
    }
}

/// A variable the code reads with `std::env::var("NAME")` and that is not in `BEHAVIOR_ENV_VARS`
/// is a bleed (a test's shell reaches the code under test) and an unclassified launch setting.
#[test]
fn every_variable_the_product_reads_is_listed() {
    let read = Regex::new(r#"env::var(?:_os)?\(\s*"([A-Z][A-Z0-9_]+)"\s*\)"#).unwrap();
    let listed: BTreeSet<&str> = BEHAVIOR_ENV_VARS.iter().copied().collect();
    // Variables of the platform, not settings of ours.
    let platform = [
        "HOME",
        "USERPROFILE",
        "PATH",
        "XDG_CONFIG_HOME",
        "APPDATA",
        "SHELL",
        "USER",
        "USERNAME",
        "TMPDIR",
        "TEMP",
        "TMP",
        "LOCALAPPDATA",
        "RUST_LOG",
        "NO_COLOR",
        "TERM",
        "CI",
        "LLVM_PROFILE_FILE",
        "CARGO_LLVM_COV",
        "DOCKER_HOST",
        "SSH_AUTH_SOCK",
        "CARGO_MANIFEST_DIR",
        "PWD",
        "LANG",
    ];
    let mut stray = BTreeSet::new();
    for path in rust_sources() {
        if path.ends_with("src/testing.rs") {
            continue; // test hooks (IAD_BINARY, IAD_ALLOW_SKIP): read by tests only
        }
        let text = std::fs::read_to_string(&path).unwrap();
        let end = text.find("#[cfg(test)]").unwrap_or(text.len());
        for c in read.captures_iter(&text[..end]) {
            let name = c[1].to_string();
            if !listed.contains(name.as_str()) && !platform.contains(&name.as_str()) {
                stray.insert(format!("{name} ({})", path.display()));
            }
        }
    }
    assert!(
        stray.is_empty(),
        "the product reads environment variables that are in neither BEHAVIOR_ENV_VARS nor \
         LAUNCH_SETTINGS: {stray:?}"
    );
}

fn rust_sources() -> Vec<PathBuf> {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut out = Vec::new();
    let mut stack: Vec<PathBuf> = ["iris-agentic-dev-core", "iris-agentic-dev-bin"]
        .iter()
        .map(|c| crates.join(c).join("src"))
        .collect();
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }
    out
}

/// Where the product writes the process environment, and why that is not a file's value becoming
/// a setting the operator is taken to have made. Each is a variable that only narrows or is
/// neutral, set from something that is not a looser limit. This list may only shrink.
const ENV_WRITES: &[(&str, &str, &str)] = &[
    (
        "iris/workspace_config.rs",
        "IRIS_DISABLED_TOOLS",
        "a file's disabled_tools; it can only remove tools",
    ),
    (
        "iris/workspace_config.rs",
        "IRIS_ENABLED_TOOLS",
        "a file's enabled_tools; an allowlist can only remove tools",
    ),
    (
        "cmd/mcp.rs",
        "IRIS_SUPPRESS_TOOL_DESCRIPTION",
        "the --suppress-tool-description flag of this launch",
    ),
    (
        "cmd/benchmark.rs",
        "IRIS_GENERATE_CLASS_MODEL",
        "the --model flag of this launch",
    ),
];

/// A file's value reaches a launch setting only by being written into the environment. The product
/// writes it in the places listed, each for a variable that cannot loosen anything; a write of a
/// `Loosening` or `OperatorOnly` variable, or one whose name is not a literal, fails.
#[test]
fn the_product_writes_only_variables_that_cannot_loosen() {
    let set = Regex::new(r#"(?:set_var|remove_var)\(\s*("?[A-Za-z_0-9]+"?)"#).unwrap();
    let mut seen = BTreeSet::new();
    let mut failures = Vec::new();
    for path in rust_sources() {
        if path.ends_with("src/testing.rs") {
            continue; // the test helpers: EnvGuard and isolate_process
        }
        let text = std::fs::read_to_string(&path).unwrap();
        let end = text.find("#[cfg(test)]").unwrap_or(text.len());
        let code: String = text[..end]
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        for c in set.captures_iter(&code) {
            let arg = c[1].to_string();
            let file = path.to_string_lossy().replace('\\', "/");
            let Some(name) = arg.strip_prefix('"').and_then(|a| a.strip_suffix('"')) else {
                failures.push(format!(
                    "{file} writes the environment under a name that is not a literal ({arg})"
                ));
                continue;
            };
            let Some(listed) = ENV_WRITES
                .iter()
                .find(|(f, n, _)| file.ends_with(f) && *n == name)
            else {
                failures.push(format!(
                    "{file} writes {name}, which ENV_WRITES does not list"
                ));
                continue;
            };
            seen.insert((listed.0, listed.1));
            let class = LAUNCH_SETTINGS
                .iter()
                .find(|(n, _, _)| *n == name)
                .map(|(_, c, _)| *c);
            if !matches!(
                class,
                Some(SettingClass::Tightening | SettingClass::Neutral)
            ) {
                failures.push(format!(
                    "{file} writes {name}, classified {class:?}: a value from a file or flag \
                     must not become a launch setting that loosens"
                ));
            }
        }
    }
    for (f, n, reason) in ENV_WRITES {
        assert!(reason.len() >= 20, "{n} is listed without a reason");
        assert!(
            seen.contains(&(*f, *n)),
            "{f} no longer writes {n}: remove it from ENV_WRITES"
        );
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
