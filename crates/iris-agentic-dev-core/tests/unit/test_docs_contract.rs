//! Spec 085 US5 (T050–T055) — the documentation contract.
//!
//! A documented security control with no reader is worse than no control. The reporter for this
//! spec read `write_allowed_servers` out of `docs/tools.md`, put it in their toml, got no error,
//! and believed their writes were confined to two servers. The key is not a field on any config
//! struct, so serde dropped it silently — the #110 pattern, applied to a security boundary.
//!
//! So: pull the identifiers out of the shipped surfaces and require each one to exist in
//! `crates/*/src`. Five extractors, because "exists" means something different for each kind:
//!
//! | Extractor        | What it pulls out                      | "Exists" means                     |
//! | ---------------- | -------------------------------------- | ---------------------------------- |
//! | error codes      | `SCREAMING_SNAKE_CASE` tokens          | emitted as a string literal        |
//! | config keys      | `### \`key\`` headings, toml fences    | deserializes **and** has a reader  |
//! | env vars         | `IRIS_*` / `IAD_*` / `OBJECTSCRIPT_*`  | read, not merely written           |
//! | tool parameters  | parameter-table rows under a tool      | present in the tool's inputSchema  |
//! | counts           | the `read_only_hint` sentence          | equals what the router registers   |
//!
//! Presence in the sources is deliberately not the test for config keys and env vars.
//! `IRIS_DESTRUCTIVE_TOOLS_ENABLED` was in the sources for five releases — as a `set_var` with no
//! corresponding read. A presence grep is green on the exact defect this spec exists to fix.
//!
//! An identifier that is documented ahead of its implementation carries `PLANNED(spec-NNN)` on the
//! same line, and the marker has to name a spec directory that exists. Exemptions live inline in
//! the documentation, where the reader of the documentation sees them, rather than in a list buried
//! in this file.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use regex::Regex;

use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};
use iris_agentic_dev_core::iris::workspace_config::load_fleet_config_from_str;
use iris_agentic_dev_core::tools::write_gate::DeclaredGates;
use iris_agentic_dev_core::tools::{IrisTools, Toolset};

// ── the two sides of the contract ────────────────────────────────────────────

/// `CARGO_MANIFEST_DIR` is `<root>/crates/iris-agentic-dev-core`, so the root is two up. This is a
/// source-tree test by nature — it reads the docs and the sources — so a build-time path is the
/// right tool here, unlike in shipped code where it is a bug.
fn repo_root() -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .and_then(Path::parent)
        .expect("CARGO_MANIFEST_DIR should be <root>/crates/<crate>")
        .to_path_buf()
}

fn walk(dir: &Path, keep: &dyn Fn(&Path) -> bool, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, keep, out);
        } else if keep(&p) {
            out.push(p);
        }
    }
}

/// Every markdown surface a user can read without cloning the repo: the two reference documents and
/// every bundled skill.
fn all_doc_files() -> Vec<PathBuf> {
    let root = repo_root();
    let mut files = vec![root.join("docs/tools.md"), root.join("docs/connecting.md")];
    let mut skills = Vec::new();
    walk(
        &root.join("skills"),
        &|p| p.file_name().is_some_and(|n| n == "SKILL.md"),
        &mut skills,
    );
    skills.sort();
    files.extend(skills);
    for f in &files {
        assert!(
            f.is_file(),
            "{} is missing — this test reads the shipped docs, so a moved file makes every \
             extractor below pass by finding nothing",
            f.display()
        );
    }
    files
}

/// Does this bundled skill document *iad itself*, as opposed to IRIS, ObjectScript or SQL?
///
/// Mechanical rule rather than a hand-kept list: the skill named `iris-agentic-dev` is the one whose
/// subject is this server's own controls, so it is held to the same contract as `docs/`.
fn documents_iad(path: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(path) else {
        return false;
    };
    text.lines().any(|l| l.trim() == "name: iris-agentic-dev")
}

/// The surfaces this contract applies to: the reference docs plus the bundled skill that documents
/// iad's own controls (FR-016a).
///
/// The other 37 bundled skills document IRIS, ObjectScript and SQL. Their screaming-snake tokens are
/// IRIS syntax (`TO_VECTOR`, `VECTOR_COSINE`, `SESSION_USER`) and container environment variables
/// (`ISC_CPF_MERGE_FILE`, `IRIS_LICENSE_KEY`, `TC_HOST`) — identifiers owned by IRIS, not emitted or
/// read by this binary, so requiring them to exist in `crates/*/src` would report ~25 failures that
/// are all correct documentation. They are named out loud in
/// [`the_contract_scope_is_stated_out_loud`] rather than dropped quietly.
fn contract_doc_files() -> Vec<PathBuf> {
    all_doc_files()
        .into_iter()
        .filter(|p| !p.ends_with("SKILL.md") || documents_iad(p))
        .collect()
}

fn rel(path: &Path) -> String {
    let root = repo_root();
    path.strip_prefix(&root)
        .unwrap_or(path)
        .display()
        .to_string()
}

/// The scope decision above, asserted and printed. A contract that silently covers three files out
/// of thirty-nine reads as "the docs are checked" when it is not.
#[test]
fn the_contract_scope_is_stated_out_loud() {
    let all = all_doc_files();
    let in_scope: Vec<String> = contract_doc_files().iter().map(|p| rel(p)).collect();
    let skipped: Vec<String> = all
        .iter()
        .filter(|p| !contract_doc_files().contains(p))
        .map(|p| rel(p))
        .collect();

    assert_eq!(
        in_scope,
        vec![
            "docs/tools.md".to_string(),
            "docs/connecting.md".to_string(),
            "skills/skills/iris-agentic-dev/SKILL.md".to_string(),
        ],
        "the identifier contract covers iad's own surfaces; if one was renamed the extractors below \
         are reading less than they claim"
    );
    assert!(
        !skipped.is_empty(),
        "no bundled skill was skipped, which means either the skills moved or this scope note is \
         describing a filter that no longer does anything"
    );
    eprintln!(
        "note: the identifier contract covers {} file(s): {}.\n\
         note: {} bundled skill(s) are OUT of scope — they document IRIS/ObjectScript/SQL, whose \
         SCREAMING_SNAKE tokens are IRIS syntax and container env vars, not iad identifiers: {}",
        in_scope.len(),
        in_scope.join(", "),
        skipped.len(),
        skipped.join(", ")
    );
}

/// Every `.rs` file under `crates/*/src`, concatenated. Tests are excluded on purpose: a code that
/// only ever appears in an assertion is not a code the binary can return.
fn sources() -> &'static str {
    static SRC: OnceLock<String> = OnceLock::new();
    SRC.get_or_init(|| {
        let root = repo_root();
        let mut files = Vec::new();
        for crate_dir in std::fs::read_dir(root.join("crates"))
            .expect("crates/ must be readable")
            .flatten()
        {
            walk(
                &crate_dir.path().join("src"),
                &|p| p.extension().is_some_and(|e| e == "rs"),
                &mut files,
            );
        }
        assert!(
            files.len() > 20,
            "only {} rust source file(s) found under crates/*/src — the sources side of this \
             contract is empty and every check below would pass for free",
            files.len()
        );
        files.sort();
        let mut blob = String::new();
        for f in files {
            blob.push_str(&std::fs::read_to_string(&f).unwrap_or_default());
            blob.push('\n');
        }
        blob
    })
}

// ── documentation lines, and the inline exemption ────────────────────────────

/// One line of one shipped document. Carried through the extractors so a failure names the file and
/// line rather than just the identifier.
#[derive(Clone)]
struct Line {
    file: String,
    no: usize,
    text: String,
}

impl Line {
    fn at(&self) -> String {
        format!("{}:{}", self.file, self.no)
    }
}

const EXEMPT_MARKER: &str = "PLANNED(spec-";

/// The spec id an exemption marker cites, if the line carries one.
///
/// Pure so it can be tested on synthetic input: the real docs may legitimately carry no markers at
/// all, and a mechanism only exercised by whatever happens to be in the tree today is a mechanism
/// that breaks silently the first time someone needs it.
fn planned_marker(text: &str) -> Option<String> {
    let start = text.find(EXEMPT_MARKER)? + EXEMPT_MARKER.len();
    let rest = &text[start..];
    let end = rest.find(')')?;
    let id = rest[..end].trim();
    (!id.is_empty()).then(|| id.to_string())
}

fn is_exempt(text: &str) -> bool {
    planned_marker(text).is_some()
}

fn lines_of(files: Vec<PathBuf>) -> Vec<Line> {
    let mut out = Vec::new();
    for path in files {
        let file = rel(&path);
        let text = std::fs::read_to_string(&path).expect("doc must be readable");
        for (i, l) in text.lines().enumerate() {
            out.push(Line {
                file: file.clone(),
                no: i + 1,
                text: l.to_string(),
            });
        }
    }
    out
}

/// Every line of every shipped markdown surface, exemptions included. Used by the exemption test,
/// which validates markers repo-wide even where the identifier contract does not reach.
fn all_doc_lines() -> Vec<Line> {
    lines_of(all_doc_files())
}

/// The lines the contract applies to — in-scope files, minus the ones claiming an exemption.
fn contract_lines() -> Vec<Line> {
    lines_of(contract_doc_files())
        .into_iter()
        .filter(|l| !is_exempt(&l.text))
        .collect()
}

// ── the router side ──────────────────────────────────────────────────────────

fn offline_conn() -> IrisConnection {
    IrisConnection::new(
        "http://localhost:52780",
        "USER",
        "_SYSTEM",
        "SYS",
        DiscoverySource::ExplicitFlag,
    )
}

/// Tools, annotations and schemas from every surface the binary can serve, unioned.
///
/// The docs describe the product, not one toolset: `iris_admin` exists only in Merged, the four
/// skill stubs only in Baseline. Checking a single tier would let a documented tool go unverified
/// because the fixture happened not to register it.
struct Router {
    input_schemas: BTreeMap<String, serde_json::Value>,
    annotations: BTreeMap<String, serde_json::Value>,
}

fn router() -> &'static Router {
    static R: OnceLock<Router> = OnceLock::new();
    R.get_or_init(|| {
        let mut input_schemas = BTreeMap::new();
        let mut annotations = BTreeMap::new();
        for toolset in [Toolset::Baseline, Toolset::Nostub, Toolset::Merged] {
            for no_skills in [false, true] {
                let tools = IrisTools::with_registry_and_toolset(
                    Some(offline_conn()),
                    iris_agentic_dev_core::skills::SkillRegistry::new(),
                    toolset,
                    None,
                    None,
                    no_skills,
                    DeclaredGates {
                        write_tools_enabled: Some(true),
                        destructive_tools_enabled: Some(true),
                    },
                )
                .expect("IrisTools construction must not fail");
                for name in tools.registered_tool_names() {
                    if let Some(s) = tools.tool_input_schema(&name) {
                        input_schemas.insert(name.clone(), s);
                    }
                    if let Some(a) = tools.tool_annotations(&name) {
                        annotations.insert(name, a);
                    }
                }
            }
        }
        assert!(
            input_schemas.len() > 50,
            "only {} tools were read off the router — the schema side of this contract is empty",
            input_schemas.len()
        );
        Router {
            input_schemas,
            annotations,
        }
    })
}

fn tool_names() -> BTreeSet<&'static str> {
    router().input_schemas.keys().map(|s| s.as_str()).collect()
}

// ── T050: error codes ────────────────────────────────────────────────────────

fn screaming_idents(text: &str) -> Vec<&str> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"\b[A-Z][A-Z0-9]*(?:_[A-Z0-9]+)+\b").unwrap());
    re.find_iter(text).map(|m| m.as_str()).collect()
}

/// Screaming-snake tokens that are prose, IRIS syntax, or third-party names rather than something
/// iad emits. Kept short on purpose: every entry here is a hole in the check.
const NOT_OUR_CODES: &[&str] = &[
    // ObjectScript / IRIS / SQL vocabulary that happens to be shaped like an error code.
    "ORDER_BY",
    "SELECT_TOP",
    "SQL_CODE",
    // HTTP and protocol words used as prose.
    "NOT_FOUND",
    // Third-party env/CI names documented for context, not emitted by iad.
    "GITHUB_TOKEN",
    // An IRIS `CSP.ini` section name (`[APP_PATH:/api]`) quoted in the IIS setup instructions.
    "APP_PATH",
];

/// Is this token an environment variable rather than an error code?
///
/// Shape alone is not enough — `IRIS_UNREACHABLE` is an error code and `IRIS_WEB_PORT` is a
/// variable, and both match the prefix. So ask the sources: a token the binary passes to
/// `env::var`/`set_var` is a variable, and [`every_documented_env_var_is_read_somewhere`] holds it to
/// the harder standard. Everything else is checked here as a code. The two tests partition the
/// tokens between them, so none falls through both.
fn is_env_var(tok: &str) -> bool {
    (tok.starts_with("IRIS_") || tok.starts_with("IAD_") || tok.starts_with("OBJECTSCRIPT_"))
        && env_var_mentions(tok)
            .iter()
            .any(|m| *m == Mention::Read || *m == Mention::Written)
}

/// T050 / FR-015, FR-016a. Every error code the docs name is emitted somewhere in the binary.
#[test]
fn every_documented_error_code_is_emitted_by_the_binary() {
    let src = sources();
    let mut missing: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut checked: BTreeSet<String> = BTreeSet::new();

    for line in contract_lines() {
        for tok in screaming_idents(&line.text) {
            if NOT_OUR_CODES.contains(&tok) || is_env_var(tok) {
                continue;
            }
            checked.insert(tok.to_string());
            if !src.contains(&format!("\"{tok}\"")) {
                missing.entry(tok.to_string()).or_default().push(line.at());
            }
        }
    }

    assert!(
        missing.is_empty(),
        "{} documented identifier(s) are emitted nowhere in crates/*/src. Either the binary should \
         emit them, or the documentation is describing a control that does not exist — delete it, \
         or mark the line PLANNED(spec-NNN) citing the spec that will implement it:\n  {}",
        missing.len(),
        missing
            .iter()
            .map(|(k, v)| format!("{k} — {}", v.join(", ")))
            .collect::<Vec<_>>()
            .join("\n  ")
    );

    // An extractor that stops matching passes silently. The three in-scope docs yield 54
    // screaming-snake tokens; 21 of them are env vars that
    // [`every_documented_env_var_is_read_somewhere`] owns and one is IRIS syntax, leaving 32 codes
    // here. The floor sits under that so deleting a code is fine and losing the regex is not.
    assert!(
        checked.len() >= 25,
        "only {} candidate identifier(s) were extracted from {} doc line(s) — the extractor has \
         stopped reading the documentation and this test is now asserting nothing",
        checked.len(),
        contract_lines().len()
    );
}

// ── T051: config keys ────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct KeyMention {
    key: String,
    /// The literal right-hand side when the mention came from a toml fence — the docs know the
    /// type, so the deserialization probe should use it rather than guess.
    value: Option<String>,
    at: String,
}

fn strip_inline_comment(value: &str) -> &str {
    match value.find(" #") {
        // Only when the value is not itself a quoted string containing a hash.
        Some(i) if value.matches('"').count().is_multiple_of(2) => value[..i].trim_end(),
        _ => value.trim_end(),
    }
}

/// Level-3 heading text with the markdown and the ☠ / 🔒 / ✦ markers taken off.
fn heading_subject(text: &str) -> Option<String> {
    let rest = text.strip_prefix("### ")?;
    let subject = rest
        .trim()
        .trim_start_matches('`')
        .split('`')
        .next()
        .unwrap_or("")
        .trim();
    (!subject.is_empty()).then(|| subject.to_string())
}

fn is_snake_key(s: &str) -> bool {
    !s.is_empty()
        && s.contains('_')
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// Config keys the docs claim exist: level-3 headings naming a snake_case key that is not a tool,
/// plus top-level assignments inside toml fences.
///
/// Keys inside a `[table]` block are skipped — `[instance.dev]` and `[policy.prod]` keys belong to
/// nested structures whose validity a single top-level probe cannot decide. That is a stated gap,
/// not an oversight: the security keys this spec is about are all top-level.
fn documented_config_keys() -> Vec<KeyMention> {
    static ASSIGN: OnceLock<Regex> = OnceLock::new();
    let assign = ASSIGN.get_or_init(|| Regex::new(r"^([a-z][a-z0-9_]*)\s*=\s*(\S.*)$").unwrap());

    let tools = tool_names();
    let mut out = Vec::new();
    let mut in_toml_fence = false;
    let mut in_table = false;
    let mut fence_file = String::new();

    for line in contract_lines() {
        let t = line.text.trim();
        if line.file != fence_file {
            // A fence never spans two files; resetting keeps one unterminated fence from swallowing
            // the next document.
            in_toml_fence = false;
            in_table = false;
            fence_file = line.file.clone();
        }
        if t.starts_with("```") {
            if in_toml_fence {
                in_toml_fence = false;
                in_table = false;
            } else {
                in_toml_fence = t.starts_with("```toml");
                in_table = false;
            }
            continue;
        }

        if in_toml_fence {
            if t.starts_with('[') {
                in_table = true;
                continue;
            }
            if in_table || t.starts_with('#') {
                continue;
            }
            if let Some(c) = assign.captures(t) {
                out.push(KeyMention {
                    key: c[1].to_string(),
                    value: Some(strip_inline_comment(&c[2]).to_string()),
                    at: line.at(),
                });
            }
            continue;
        }

        if let Some(subject) = heading_subject(&line.text) {
            if is_snake_key(&subject) && !tools.contains(subject.as_str()) {
                out.push(KeyMention {
                    key: subject,
                    value: None,
                    at: line.at(),
                });
            }
        }
    }
    out
}

/// Does `key` reach a field of the config structure at all?
///
/// Parses a one-line toml through the real entry point and compares the resulting struct against
/// the empty parse. Identical means serde dropped the key on the floor — which is not an error, and
/// is exactly how a documented security key can have no effect.
fn key_deserializes(key: &str, value: Option<&str>) -> bool {
    let baseline = format!(
        "{:?}",
        load_fleet_config_from_str("").expect("empty toml must parse")
    );
    let mut candidates: Vec<String> = Vec::new();
    if let Some(v) = value {
        candidates.push(v.to_string());
    }
    // The documented value may be the wrong shape for a probe (a multi-line table, a placeholder),
    // so a key is only reported phantom when no plausible type reaches a field either.
    candidates.extend(
        ["true", "\"probe\"", "1", "[\"probe\"]"]
            .iter()
            .map(|s| s.to_string()),
    );
    for v in candidates {
        if let Ok(cfg) = load_fleet_config_from_str(&format!("{key} = {v}\n")) {
            if format!("{cfg:?}") != baseline {
                return true;
            }
        }
    }
    false
}

/// T051 / FR-014. Every documented config key is a real field **and** something reads it.
#[test]
fn every_documented_config_key_deserializes_and_is_read() {
    let src = sources();
    let mut phantom: Vec<String> = Vec::new();
    let mut unread: Vec<String> = Vec::new();
    let mut checked: BTreeSet<String> = BTreeSet::new();

    for m in documented_config_keys() {
        if !checked.insert(m.key.clone()) {
            continue;
        }
        if !key_deserializes(&m.key, m.value.as_deref()) {
            phantom.push(format!(
                "{} ({}) — no field of the config structure accepts it; serde ignores the key",
                m.key, m.at
            ));
            continue;
        }
        // A field with no reader is the IRIS_DESTRUCTIVE_TOOLS_ENABLED shape: it parses, it is
        // reported, and nothing acts on it. Field access is the proxy — a struct literal or a
        // Default impl mentions the name without ever reading it.
        if !src.contains(&format!(".{}", m.key)) {
            unread.push(format!(
                "{} ({}) — deserializes, but nothing in crates/*/src reads `.{}`",
                m.key, m.at, m.key
            ));
        }
    }

    assert!(
        phantom.is_empty() && unread.is_empty(),
        "{} documented config key(s) do not do what the documentation says:\n  {}",
        phantom.len() + unread.len(),
        phantom
            .iter()
            .chain(unread.iter())
            .cloned()
            .collect::<Vec<_>>()
            .join("\n  ")
    );
    assert!(
        checked.len() >= 10,
        "only {checked:?} config key(s) were extracted — the extractor has stopped reading the toml \
         fences and headings"
    );
}

// ── T052: environment variables ──────────────────────────────────────────────

fn documented_env_vars() -> BTreeMap<String, Vec<String>> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"\b(?:IRIS|IAD|OBJECTSCRIPT)_[A-Z0-9_]+\b").unwrap());
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for line in contract_lines() {
        for m in re.find_iter(&line.text) {
            out.entry(m.as_str().to_string())
                .or_default()
                .push(line.at());
        }
    }
    out
}

/// How the sources mention an env-var name, judged by what immediately precedes the literal.
#[derive(Debug, PartialEq)]
enum Mention {
    /// `env::var("X")`, `env::var_os("X")`, clap's `env = "X"` — something acts on the value.
    Read,
    /// `set_var("X")` / `remove_var("X")` — the process writes or clears it and may never read it.
    Written,
    /// A plain string literal: a comment, a doc string, or a token that is not an env var at all.
    /// Says nothing either way about reading.
    Other,
}

fn classify_mention(before: &str) -> Mention {
    let tail = before.trim_end();
    // `set_var(` and `remove_var(` both end in `var(`, so they have to be tested first or every
    // write would be classified as a read and this whole test would assert nothing.
    if tail.ends_with("set_var(") || tail.ends_with("remove_var(") {
        Mention::Written
    } else if tail.ends_with("var(")
        || tail.ends_with("var_os(")
        || tail.ends_with("env!(")
        || tail.ends_with("env =")
        || tail.ends_with("env=")
        // The one in-repo helper that takes a variable *name* as an argument and reads it:
        // `log_store::read_inline_threshold(env_var, default)`. Verified as the only such wrapper
        // (`grep -rn 'env::var[_a-z]*([a-z]' crates/*/src` matches its body and nothing else), so
        // this list stays short by construction rather than by hope.
        || tail.ends_with("read_inline_threshold(")
    {
        Mention::Read
    } else {
        Mention::Other
    }
}

fn env_var_mentions(name: &str) -> Vec<Mention> {
    let needle = format!("\"{name}\"");
    let src = sources();
    let mut out = Vec::new();
    let mut from = 0usize;
    while let Some(i) = src[from..].find(&needle) {
        let at = from + i;
        let window = &src[at.saturating_sub(32)..at];
        out.push(classify_mention(window));
        from = at + needle.len();
    }
    out
}

/// T052 / FR-015. Every documented environment variable is *read*, not merely written.
///
/// The distinction is the whole point. `IRIS_DESTRUCTIVE_TOOLS_ENABLED` was documented as the
/// environment form of the destructive gate, was present in the sources, and was only ever a
/// `set_var` — so every grep-shaped check was green while the variable did nothing at all.
#[test]
fn every_documented_env_var_is_read_somewhere() {
    // The classifier is the whole test; a regression in it is invisible from the outside.
    assert_eq!(classify_mention("    std::env::var("), Mention::Read);
    assert_eq!(classify_mention("        env::var_os("), Mention::Read);
    assert_eq!(
        classify_mention("    unsafe { env::set_var("),
        Mention::Written
    );
    assert_eq!(
        classify_mention("        env::remove_var("),
        Mention::Written
    );
    assert_eq!(
        classify_mention("    log_store::read_inline_threshold("),
        Mention::Read
    );
    assert_eq!(classify_mention("    return err("), Mention::Other);

    let mut bad: Vec<String> = Vec::new();
    let mut not_a_variable: Vec<String> = Vec::new();
    let documented = documented_env_vars();

    for (name, at) in &documented {
        let mentions = env_var_mentions(name);
        if mentions.contains(&Mention::Read) {
            continue;
        }
        // The `IRIS_` prefix also fits error codes (`IRIS_UNREACHABLE`) and header names. A token
        // that appears as a plain literal is *something* the binary emits, so it is T050's business,
        // not this test's. Only "written and never read" and "absent entirely" are this test's
        // failures — which is exactly the IRIS_DESTRUCTIVE_TOOLS_ENABLED shape.
        if mentions.contains(&Mention::Other) {
            not_a_variable.push(format!("{name} ({})", at.join(", ")));
            continue;
        }
        let why = if mentions.is_empty() {
            "the name does not appear as a string literal anywhere in crates/*/src"
        } else {
            "only ever written with set_var/remove_var, never read — the documented variable has no \
             effect"
        };
        bad.push(format!("{name} ({}) — {why}", at.join(", ")));
    }

    assert!(
        bad.is_empty(),
        "{} documented environment variable(s) are not read by the binary:\n  {}\n\
         Reading means `env::var(\"NAME\")`, `env::var_os`, clap's `env = \"NAME\"`, or \
         `log_store::read_inline_threshold`.",
        bad.len(),
        bad.join("\n  ")
    );
    assert!(
        documented.len() >= 10,
        "only {} environment variable(s) were extracted from the docs — the extractor has stopped \
         matching",
        documented.len()
    );
    if !not_a_variable.is_empty() {
        eprintln!(
            "note: {} token(s) matched the env-var shape but are emitted as plain literals (error \
             codes, header names), so they were checked as codes instead: {}",
            not_a_variable.len(),
            not_a_variable.join(", ")
        );
    }
}

// ── T053: tool parameters ────────────────────────────────────────────────────

#[derive(Debug)]
struct ParamMention {
    /// Every tool the parameter's section covers. Usually one; a heading like
    /// `### \`kb\` / \`kb_index\` / \`kb_recall\`` covers three, and `workspace_path` is a real
    /// parameter of the second one. Treating that as a parameter of `kb` alone reports a defect that
    /// is not there.
    tools: Vec<String>,
    param: String,
    at: String,
}

fn table_cells(line: &str) -> Vec<String> {
    line.trim()
        .trim_start_matches('|')
        .trim_end_matches('|')
        .split('|')
        .map(|c| c.trim().trim_matches('`').trim().to_string())
        .collect()
}

/// Could this table cell be a parameter name? `snake_case` and `camelCase` both, because the wire
/// names are not uniform — `iris_message_body` really does take `acknowledgePhi`. Anything with a
/// space, a quote or an `=` is prose or a `mode="put"` qualifier.
fn is_param_ident(s: &str) -> bool {
    s.starts_with(|c: char| c.is_ascii_lowercase())
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// The registered tools a `### ` heading names, in order.
fn heading_tools(text: &str) -> Vec<String> {
    let Some(rest) = text.strip_prefix("### ") else {
        return Vec::new();
    };
    let tools = tool_names();
    rest.split('`')
        .map(str::trim)
        .filter(|s| tools.contains(s))
        .map(|s| s.to_string())
        .collect()
}

/// A bold label that names one registered tool, as in `**kb_index**:` — a sub-heading that narrows a
/// multi-tool section to a single tool. `**\`action=list\`**` names an action, not a tool, and leaves
/// the section as it was.
fn bold_tool_label(text: &str) -> Option<String> {
    let t = text.trim().trim_end_matches(':');
    let inner = t.strip_prefix("**")?.strip_suffix("**")?.trim_matches('`');
    tool_names().contains(inner).then(|| inner.to_string())
}

/// Parameter rows under a level-3 heading that names a registered tool.
///
/// A table only counts when its own header row starts with `Parameter` — tool sections also carry
/// tables of modes, actions and error codes, and reading those as parameters would produce
/// failures about identifiers nobody claimed were parameters.
fn documented_tool_params() -> Vec<ParamMention> {
    let mut out = Vec::new();
    let mut section: Vec<String> = Vec::new();
    let mut current: Vec<String> = Vec::new();
    let mut in_param_table = false;

    for line in contract_lines() {
        if line.text.starts_with("### ") {
            section = heading_tools(&line.text);
            current = section.clone();
            in_param_table = false;
            continue;
        }
        if line.text.starts_with("## ") || line.text.starts_with("# ") {
            section.clear();
            current.clear();
            in_param_table = false;
            continue;
        }
        if let Some(narrowed) = bold_tool_label(&line.text) {
            if section.contains(&narrowed) {
                current = vec![narrowed];
                in_param_table = false;
                continue;
            }
        }
        if current.is_empty() {
            continue;
        }
        let tool = current.clone();
        let t = line.text.trim();
        if !t.starts_with('|') {
            in_param_table = false;
            continue;
        }
        let cells = table_cells(t);
        let first = cells.first().cloned().unwrap_or_default();
        if first.eq_ignore_ascii_case("parameter") {
            in_param_table = true;
            continue;
        }
        if !in_param_table || first.chars().all(|c| c == '-' || c == ':') {
            continue;
        }
        // `mode="put"` style qualifiers and prose in the first cell are not parameter names.
        if is_param_ident(&first) {
            out.push(ParamMention {
                tools: tool,
                param: first,
                at: line.at(),
            });
        }
    }
    out
}

/// T053 / FR-016b. Every documented parameter is in the tool's advertised input schema.
///
/// The schema is what a conforming client reads before it builds a call, so a parameter missing
/// from it cannot be passed — `stream_inspect`'s documented `max_chars` was never a field on the
/// request struct, which means callers asking for 10 000 characters silently got 2 000.
#[test]
fn every_documented_tool_parameter_is_in_the_input_schema() {
    let schemas = &router().input_schemas;
    let mut missing: Vec<String> = Vec::new();
    let mut checked = 0usize;
    let mut by_handler = 0usize;
    let mut unlocatable: BTreeSet<String> = BTreeSet::new();

    for m in documented_tool_params() {
        let mut accepted = false;
        let mut examined = false;
        let mut open_handler = false;
        let mut declares: BTreeSet<String> = BTreeSet::new();

        for tool in &m.tools {
            let Some(schema) = schemas.get(tool) else {
                continue;
            };
            match schema
                .get("properties")
                .and_then(|p| p.as_object())
                .filter(|p| !p.is_empty())
            {
                Some(props) => {
                    examined = true;
                    declares.extend(props.keys().cloned());
                    accepted |= props.contains_key(&m.param);
                }
                // An `AnyParams` tool advertises an open object, so the schema cannot answer the
                // question — and this is the case the reporter hit. `stream_inspect` is documented
                // with a `max_chars` parameter that appears nowhere in `crates/*/src`, so a caller
                // asking for 10 000 characters silently gets everything anyway. Falling through to
                // the handler is what catches it; skipping these tools is what let it ship.
                None => match handler_body(tool) {
                    Some(body) => {
                        examined = true;
                        open_handler = true;
                        accepted |= body.contains(&format!("\"{}\"", m.param));
                    }
                    None => {
                        unlocatable.insert(tool.clone());
                    }
                },
            }
        }

        if !examined {
            continue;
        }
        checked += 1;
        if open_handler {
            by_handler += 1;
        }
        if !accepted {
            let how = if open_handler {
                format!(
                    "no handler among {:?} looks up \"{}\", so the documented parameter is ignored",
                    m.tools, m.param
                )
            } else {
                format!("the input schema of {:?} declares {declares:?}", m.tools)
            };
            missing.push(format!("{:?}({}) at {} — {how}", m.tools, m.param, m.at));
        }
    }

    assert!(
        missing.is_empty(),
        "{} documented parameter(s) cannot actually be passed:\n  {}",
        missing.len(),
        missing.join("\n  ")
    );
    assert!(
        checked >= 100,
        "only {checked} documented parameter(s) were checked — the extractor has stopped reading \
         the parameter tables"
    );
    assert!(
        by_handler > 0,
        "no documented parameter reached the handler check, so the open-parameter path — the one \
         `max_chars` slipped through — is no longer exercised"
    );
    // Not a failure, but it is coverage this test does not have, and silence would read as
    // "everything was checked".
    if !unlocatable.is_empty() {
        eprintln!(
            "note: {} tool(s) take open parameters and no `async fn <name>(` handler was found for \
             them, so their documented parameters were not checked: {unlocatable:?}",
            unlocatable.len()
        );
    }
}

/// The body of the `#[tool]` method that serves `tool`, up to the next method.
///
/// Tools that take `Parameters<AnyParams>` read their arguments by name out of the map, so the
/// literal appearing in this slice is the evidence that the parameter is honoured.
fn handler_body(tool: &str) -> Option<&'static str> {
    let src = sources();
    let start = src.find(&format!("async fn {tool}("))?;
    let rest = &src[start..];
    let end = rest[1..]
        .find("    async fn ")
        .map(|i| i + 1)
        .unwrap_or(rest.len());
    Some(&rest[..end])
}

// ── T054: the annotation counts ──────────────────────────────────────────────

fn documented_annotation_count(annotation: &str) -> (usize, String) {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"(\d+)\s+tools?\b").unwrap());
    for line in contract_lines() {
        let t = line.text.trim();
        if t.starts_with('|') && t.contains(&format!("`{annotation}`")) {
            if let Some(c) = re.captures(t) {
                return (c[1].parse().expect("digits"), line.at());
            }
            panic!(
                "the {annotation} row at {} no longer states a tool count — T054 has nothing to \
                 compare against: {t}",
                line.at()
            );
        }
    }
    panic!("no `{annotation}` row found in the annotations table — it moved or was renamed");
}

fn annotated_count(key: &str) -> BTreeSet<String> {
    router()
        .annotations
        .iter()
        .filter(|(_, a)| a.get(key).and_then(|v| v.as_bool()) == Some(true))
        .map(|(n, _)| n.clone())
        .collect()
}

/// T054 / FR-016. The counts in the annotations table match the router.
///
/// Every identifier in that sentence is real and the sentence is still wrong: it claims 57
/// read-only tools, and `c641d79` stripped `read_only_hint` from six mutating tools without
/// touching the prose. No extractor above can see this — the number is not an identifier.
#[test]
fn the_annotation_counts_match_the_router() {
    for (doc_key, wire_key) in [
        ("read_only_hint", "readOnlyHint"),
        ("destructive_hint", "destructiveHint"),
    ] {
        let (documented, at) = documented_annotation_count(doc_key);
        let actual = annotated_count(wire_key);
        assert_eq!(
            documented,
            actual.len(),
            "{at} says {documented} tools carry {doc_key}; the router declares it on {}: {:?}",
            actual.len(),
            actual
        );
    }
}

/// The `destructive_hint` row names its tools as well as counting them, and a name is checkable.
/// A count that matches while the list is wrong is still a lie about which tools need confirmation.
#[test]
fn the_destructive_hint_row_names_the_right_tools() {
    let actual = annotated_count("destructiveHint");
    let row = contract_lines()
        .into_iter()
        .find(|l| {
            let t = l.text.trim();
            t.starts_with('|') && t.contains("`destructive_hint`")
        })
        .expect("the destructive_hint row must exist");

    let named: BTreeSet<String> = screaming_or_snake_idents(&row.text)
        .into_iter()
        .filter(|s| actual.contains(s) || router().annotations.contains_key(s))
        .collect();

    assert_eq!(
        named,
        actual,
        "{} names {:?} as the destructive tools; the router declares destructiveHint on {:?}",
        row.at(),
        named,
        actual
    );
}

/// Backticked snake_case tokens on a line — the shape tool names take in the docs.
fn screaming_or_snake_idents(text: &str) -> Vec<String> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"`([a-z][a-z0-9_]*)`").unwrap());
    re.captures_iter(text)
        .map(|c| c[1].to_string())
        .filter(|s| is_snake_key(s))
        .collect()
}

// ── T055: the inline exemption ───────────────────────────────────────────────

/// T055. `PLANNED(spec-NNN)` skips a line, and the marker has to cite a spec that exists.
///
/// Exercised on synthetic input as well as the real docs: the tree may carry no markers at all
/// today, and an escape hatch that is only tested when someone happens to use it is one that
/// silently stops working.
#[test]
fn the_planned_exemption_parses_and_must_cite_a_real_spec() {
    assert_eq!(
        planned_marker("returns `WRITE_SERVER_NOT_ALLOWED` PLANNED(spec-074)").as_deref(),
        Some("074")
    );
    assert_eq!(planned_marker("returns `WRITE_TOOLS_DISABLED`"), None);
    assert_eq!(planned_marker("PLANNED(spec-"), None, "unterminated marker");
    assert_eq!(planned_marker("PLANNED(spec-)"), None, "empty spec id");
    assert!(is_exempt("x PLANNED(spec-074) y"));
    assert!(!is_exempt("PLANNED but not the marker"));

    // A line claiming an exemption is genuinely removed from the contract.
    let exempted = "the fictional `NEVER_IMPLEMENTED_CODE` PLANNED(spec-074)";
    assert!(screaming_idents(exempted).contains(&"NEVER_IMPLEMENTED_CODE"));
    assert!(
        is_exempt(exempted),
        "the extractor sees the identifier; the exemption is what keeps it out of the check"
    );

    let specs = repo_root().join("specs");
    let dirs: Vec<String> = std::fs::read_dir(&specs)
        .expect("specs/ must be readable")
        .flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();

    let mut dangling: Vec<String> = Vec::new();
    let mut used = 0usize;
    for line in all_doc_lines() {
        let Some(id) = planned_marker(&line.text) else {
            continue;
        };
        used += 1;
        if !dirs
            .iter()
            .any(|d| d.starts_with(&format!("{id}-")) || *d == id)
        {
            dangling.push(format!(
                "{} cites spec-{id}, which is not a directory under specs/",
                line.at()
            ));
        }
    }
    assert!(
        dangling.is_empty(),
        "{} exemption marker(s) cite a spec that does not exist — an exemption pointing at nothing \
         is an identifier with no plan:\n  {}",
        dangling.len(),
        dangling.join("\n  ")
    );
    eprintln!("note: {used} PLANNED(spec-NNN) exemption(s) in use across the shipped docs");
}
