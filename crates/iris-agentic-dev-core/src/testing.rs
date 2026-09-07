//! Test-support helpers, compiled only under the `testing` feature.
//!
//! # Why this module exists
//!
//! Five `#[ignore]` tests in `nopws_101` shipped in 1.3.0 having never executed a single
//! assertion. Each one opened with:
//!
//! ```text
//! let binary = std::env::var("IAD_BINARY")
//!     .unwrap_or_else(|_| "./target/debug/iris-agentic-dev".to_string());
//! if !std::path::Path::new(&binary).exists() {
//!     eprintln!("IAD_BINARY not found at {binary}, skipping");
//!     return;
//! }
//! ```
//!
//! Two independent faults compounded. The default path is *relative*, and a test binary's working
//! directory is the crate root, not the workspace root — so `./target/debug/iris-agentic-dev` never
//! resolves in a workspace. And the miss is reported by returning `Ok`, so a test that ran nothing
//! is indistinguishable in the summary from a test that verified everything. CI does not set
//! `IAD_BINARY`, so all five took the skip branch on every run, for months, printing `ok`.
//!
//! The fix is structural, not a better default: resolution must not depend on the working
//! directory, and a missing prerequisite must be loud unless an operator explicitly asked for
//! quiet.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;

/// Absolute path to the workspace `target/` directory.
///
/// `CARGO_MANIFEST_DIR` is the crate root (`.../crates/iris-agentic-dev-core`), which cargo sets at
/// compile time, so this is correct no matter where the test process is started from. Walking up to
/// the directory holding the workspace `Cargo.toml` keeps it correct if a crate is ever nested
/// deeper.
fn workspace_root() -> PathBuf {
    let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    while dir.pop() {
        if dir.join("Cargo.toml").exists() && dir.join("crates").is_dir() {
            return dir;
        }
    }
    // A single-crate checkout: the manifest dir *is* the root.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Where the `iris-agentic-dev` binary is expected to be.
///
/// `IAD_BINARY` wins when set. A relative `IAD_BINARY` is resolved against the workspace root
/// rather than the process working directory, because CI passes `./target/debug/iris-agentic-dev`
/// and the two differ. Otherwise both `debug` and `release` profiles are tried, so a
/// `cargo test --release` run finds its own binary.
pub fn iad_binary_path() -> PathBuf {
    if let Ok(v) = std::env::var("IAD_BINARY") {
        let p = PathBuf::from(&v);
        return if p.is_absolute() {
            p
        } else {
            workspace_root().join(p)
        };
    }
    let root = workspace_root();
    let debug = root.join("target/debug/iris-agentic-dev");
    if debug.exists() {
        return debug;
    }
    root.join("target/release/iris-agentic-dev")
}

/// The binary, or a panic naming what to run.
///
/// Panicking is the point. A binary-invocation test with no binary has verified nothing, and the
/// only honest outcomes are "ran" and "failed". `IAD_ALLOW_SKIP=1` opts into the old behaviour for
/// a developer who knowingly wants the rest of a suite to run; it returns `None` and prints why, so
/// the skip appears in the log as a deliberate choice rather than as a pass.
///
/// ```text
/// let Some(bin) = require_iad_binary() else { return };
/// ```
pub fn require_iad_binary() -> Option<PathBuf> {
    let path = iad_binary_path();
    if path.exists() {
        return Some(path);
    }
    if std::env::var("IAD_ALLOW_SKIP").is_ok() {
        eprintln!(
            "SKIP (IAD_ALLOW_SKIP set): no iris-agentic-dev binary at {}",
            path.display()
        );
        return None;
    }
    panic!(
        "no iris-agentic-dev binary at {}\n\
         This test spawns the binary; without it the test asserts nothing, so it fails instead of \
         passing quietly.\n\
         Build it:      cargo build -p iris-agentic-dev\n\
         Or point at one: IAD_BINARY=/abs/path/to/iris-agentic-dev\n\
         Or opt into skipping deliberately: IAD_ALLOW_SKIP=1",
        path.display()
    );
}

/// Every process-environment variable that changes what the server does.
///
/// Kept as one list so a spawn helper can clear all of them in a single call. `std::env::var` in
/// `crates/*/src/` is the source of truth; a var read there and absent here is a bleed waiting to
/// happen, which is what `antipatterns.sh env-pinning` checks.
pub const BEHAVIOR_ENV_VARS: &[&str] = &[
    // connection
    "IRIS_HOST",
    "IRIS_WEB_PORT",
    "IRIS_SCHEME",
    "IRIS_WEB_PREFIX",
    "IRIS_NAMESPACE",
    "IRIS_USERNAME",
    "IRIS_PASSWORD",
    "IRIS_CONTAINER",
    "IRIS_SERVICE_USERNAME",
    "IRIS_SERVICE_PASSWORD",
    "IRIS_SERVER_NAME",
    "IRIS_INSECURE",
    "IRIS_TLS_VERIFY",
    // gates
    "IRIS_WRITE_TOOLS_ENABLED",
    "IRIS_DESTRUCTIVE_TOOLS_ENABLED",
    "IRIS_ALLOW_PROD",
    "IRIS_ADMIN_TOOLS",
    "IRIS_SCM_ALLOW_CHECKIN",
    // tool surface
    "IRIS_ENABLED_TOOLS",
    "IRIS_DISABLED_TOOLS",
    "IRIS_TOOLSET",
    "IRIS_NO_SKILLS",
    "IRIS_LIST_TOOLS_PAGE_SIZE",
    "IRIS_SUPPRESS_TOOL_DESCRIPTION",
    // skills
    "OBJECTSCRIPT_LEARNING",
    "IRIS_AGENTIC_DEV_SKILLS_DIR",
    "OBJECTSCRIPT_SKILLMCP_NAMESPACE",
    // config discovery
    "OBJECTSCRIPT_WORKSPACE",
    // attribution
    "IRIS_AGENT_LABEL",
    // disclosure thresholds
    "IRIS_INLINE_SEARCH",
    "IRIS_INLINE_COMPILE",
    "IRIS_INLINE_ERROR_LOGS",
    "IRIS_INLINE_INFO",
    "IRIS_LOG_STORE_MAX",
    "IRIS_LOG_TTL_MINUTES",
    // timeouts
    "IRIS_SEARCH_SYNC_TIMEOUT",
    "OBJECTSCRIPT_TEST_TIMEOUT",
    "IRIS_GENERATE_TIMEOUT",
    // generation
    "IRIS_GENERATE_CLASS_MODEL",
    "OPENAI_API_KEY",
    "OPENAI_BASE_URL",
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_BASE_URL",
    // skill fetch
    "GITHUB_RAW_BASE_URL",
    "GITHUB_API_BASE_URL",
    // eval envelope identity
    "GAUNTLET_RUN_ID",
    "GAUNTLET_TASK_ID",
    "GAUNTLET_CONDITION",
];

/// A `Command` for the binary with every behavior-changing variable removed.
///
/// The deny-list approach every existing spawn site uses is unmaintainable: a new `std::env::var`
/// in `src/` instantly becomes a bleed at ~60 call sites, and the CI e2e job sets nine of these at
/// job level, so the same test means different things in the `test` job and the `e2e-tests` job.
/// Start from nothing and add back only what the test is about; then the test's meaning is written
/// down in the test.
///
/// `PATH`, `HOME` and the profiler's `LLVM_PROFILE_FILE` are left alone — the child needs them to
/// run at all, and `HOME` isolation is a per-test decision.
pub fn clean_command(bin: &Path) -> std::process::Command {
    let mut cmd = std::process::Command::new(bin);
    for var in BEHAVIOR_ENV_VARS {
        cmd.env_remove(var);
    }
    cmd
}

/// `clean_command` plus the `mcp` subcommand.
///
/// The MCP server is `iris-agentic-dev mcp`. Spawning the bare binary prints the usage banner and
/// exits 2, which a test reading stdout for JSON-RPC sees as empty output — the second half of the
/// `nopws_101` failure. Going through this helper makes forgetting it impossible.
pub fn clean_mcp_command(bin: &Path) -> std::process::Command {
    let mut cmd = clean_command(bin);
    cmd.arg("mcp");
    cmd
}

// ── 113 typed-tool-schemas: parameter-contract test support ───────────────────
//
// Three helpers the schema tests share. They live here rather than in a test file because every
// test file in this crate is its own cargo target with its own `path` — there is no shared test
// module to put them in, and copying a source-scanning helper into eight files is how the copies
// drift apart.

/// Every `.rs` file under `crates/*/src`, concatenated.
///
/// The params structs are spread across `src/tools/*.rs`, so a single-file read would miss most of
/// them and `struct_fields` would report "no such struct" for a struct that exists.
pub fn rust_sources() -> &'static str {
    static SRC: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    SRC.get_or_init(|| {
        fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return;
            };
            for e in entries.flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(&p, out);
                } else if p.extension().is_some_and(|x| x == "rs") {
                    out.push(p);
                }
            }
        }
        let root = workspace_root();
        let mut files = Vec::new();
        for crate_dir in std::fs::read_dir(root.join("crates"))
            .expect("crates/ must be readable")
            .flatten()
        {
            walk(&crate_dir.path().join("src"), &mut files);
        }
        assert!(
            files.len() > 20,
            "only {} rust source file(s) under crates/*/src — every source-side assertion below \
             would pass for free",
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

/// `crates/iris-agentic-dev-core/src/tools/mod.rs` — where every `#[tool]` method lives.
pub fn tools_mod_source() -> &'static str {
    static SRC: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    SRC.get_or_init(|| {
        let path = workspace_root().join("crates/iris-agentic-dev-core/src/tools/mod.rs");
        std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("must read {}: {e}", path.display()))
    })
}

/// Every `#[tool]`-attributed method name in `src/tools/mod.rs`, sorted.
///
/// Anchored on the `#[tool(` attribute rather than on `async fn`, because the `ServerHandler`
/// overrides (`call_tool`, `list_tools`) are `async fn` in the same file and are not tools. A
/// hand-written list of 81 names is the alternative, and it drifts silently — the drift shows up as
/// "fewer tools checked", which no assertion notices.
pub fn tool_names() -> Vec<String> {
    let src = tools_mod_source();
    let re = regex::Regex::new(r"(?s)#\[tool\(.*?\)\]\s*(?:pub\s+)?async fn ([a-z_][a-z0-9_]*)\(")
        .expect("static regex");
    let mut names: Vec<String> = re.captures_iter(src).map(|c| c[1].to_string()).collect();
    names.sort();
    names.dedup();
    names
}

/// The body of the `#[tool]` method serving `tool`, up to the next method.
///
/// Panics when the handler cannot be located. A `None` here used to mean "this tool's parameters
/// were not checked", which reads identically to "checked and correct" in a test summary.
pub fn handler_body(tool: &str) -> &'static str {
    let src = tools_mod_source();
    let needle = format!("async fn {tool}(");
    let start = src.find(&needle).unwrap_or_else(|| {
        panic!(
            "no `{needle}` in src/tools/mod.rs — either the tool was renamed or the handler moved \
             out of mod.rs; this helper must find it or the assertion means nothing"
        )
    });
    let rest = &src[start..];
    let end = rest[1..]
        .find("    async fn ")
        .map(|i| i + 1)
        .unwrap_or(rest.len());
    &rest[..end]
}

/// The type named in `Parameters<T>` for this tool, last path segment only.
pub fn params_type(tool: &str) -> String {
    let body = handler_body(tool);
    let re = regex::Regex::new(r"Parameters<([A-Za-z0-9_:]+)>").expect("static regex");
    let caps = re.captures(body).unwrap_or_else(|| {
        panic!("no `Parameters<…>` in the signature of `{tool}` — cannot classify this tool")
    });
    caps[1]
        .rsplit("::")
        .next()
        .expect("rsplit yields at least one segment")
        .to_string()
}

/// The `serde` field names of a params struct, honoring `#[serde(rename = "…")]`.
pub fn struct_fields(type_name: &str) -> BTreeSet<String> {
    let src = rust_sources();
    let needle = format!("pub struct {type_name} {{");
    let start = src.find(&needle).unwrap_or_else(|| {
        panic!(
            "no `{needle}` under crates/*/src — a params struct the signature names must be \
             findable, or this helper silently reports an empty parameter set"
        )
    });
    // The block must be brace-balanced, not cut at the next `\n}`. `pub struct NoParams {}` closes
    // on the same line, so a newline-anchored search ran past it into the following declaration and
    // reported `GetLogParams`'s four fields as the fields of every no-argument tool — which made
    // those tools look like they read four keys they do not advertise.
    let brace = start + needle.len() - 1;
    let block = balanced_block_local(src, brace).unwrap_or(&src[brace..]);

    let rename = regex::Regex::new(r#"rename\s*=\s*"([^"]+)""#).expect("static regex");
    let field = regex::Regex::new(r"^\s*pub\s+([a-z_][a-z0-9_]*)\s*:").expect("static regex");
    let mut out = BTreeSet::new();
    let mut pending_rename: Option<String> = None;
    for line in block.lines() {
        if let Some(c) = rename.captures(line) {
            pending_rename = Some(c[1].to_string());
            continue;
        }
        if let Some(c) = field.captures(line) {
            out.insert(pending_rename.take().unwrap_or_else(|| c[1].to_string()));
        }
    }
    out
}

/// The parameter names a tool's handler actually reads.
///
/// Two sources, in this order:
///
/// 1. The `p.get("…")` literals in the handler body. Converted handlers keep reading from a
///    re-serialized `Value`, so this stays available after the struct exists — which is what keeps
///    the FR-003 comparison honest. If the read set came from the struct for a converted tool, it
///    and the advertised schema would both derive from the same struct and the comparison would be
///    a tautology (Principle XI).
/// 2. Struct fields, for the handlers that destructure typed parameters directly and never touch a
///    `Value`.
///
/// Panics when neither source yields anything: a tool this helper cannot classify must fail the
/// test, not return an empty set that compares equal to nothing.
pub fn read_keys(tool: &str) -> BTreeSet<String> {
    let body = handler_body(tool);
    // Anchored on the `p` binding, not on any `.get(` call. All 31 `AnyParams` handlers
    // destructure as `Parameters(p)`, and converted handlers keep reading from a `p` re-serialized
    // from the typed struct. An unanchored `.get("…")` matches every HashMap lookup in a body and
    // reports them as parameters.
    // `\s*` around the dot because rustfmt breaks `p.get("server")` across lines whenever the
    // chain is long, which is most of them.
    //
    // The name class is deliberately mixed-case. It was `[a-z_][a-z0-9_]*` until batch 5, which
    // made `p.get("acknowledgePhi")` and `p.get("dataPolicy")` — two real parameters of
    // `iris_message_body` — invisible to every contract comparison built on this function. A
    // helper that silently under-reports the read set makes "the schema matches what the handler
    // reads" pass by omission, which is the same failure mode as the missing schema itself.
    let re = regex::Regex::new(r#"(?s)\bp\s*\.\s*get\("([A-Za-z_][A-Za-z0-9_]*)"\)"#)
        .expect("static regex");
    let from_body: BTreeSet<String> = re
        .captures_iter(body)
        .map(|c| c[1].to_string())
        .collect::<BTreeSet<_>>();
    if !from_body.is_empty() {
        return from_body;
    }
    let fields = struct_fields(&params_type(tool));
    assert!(
        !fields.is_empty() || params_type(tool) == "NoParams",
        "`{tool}` reads no `p.get(\"…\")` keys and its params struct has no fields — one of the \
         two sources must answer or the read set is unknown, not empty"
    );
    fields
}

/// Does `tool`'s handler body reference `field` at all?
///
/// The one check that survives conversion. Once a tool is typed, `read_keys` and the advertised
/// schema can both derive from the struct; this reads the handler instead, so a declared parameter
/// no code touches still fails.
pub fn handler_uses_field(tool: &str, field: &str) -> bool {
    let body = handler_body(tool);
    let reads_key = regex::Regex::new(&format!(
        r#"(?s)\bp\s*\.\s*get\("{}"\)"#,
        regex::escape(field)
    ))
    .expect("field names are plain identifiers");
    reads_key.is_match(body)
        || body.contains(&format!(".{field}"))
        || body.contains(&format!("{field}:"))
}

/// Split a comma-separated argument or pattern list at top level, ignoring commas nested inside
/// brackets or string literals.
fn split_top_level(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut in_str = false;
    let mut escaped = false;
    let mut cur = String::new();
    for c in text.chars() {
        if in_str {
            cur.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        match c {
            '"' => {
                in_str = true;
                cur.push(c);
            }
            '(' | '[' | '{' | '<' => {
                depth += 1;
                cur.push(c);
            }
            ')' | ']' | '}' | '>' => {
                depth -= 1;
                cur.push(c);
            }
            ',' if depth == 0 => out.push(std::mem::take(&mut cur)),
            _ => cur.push(c),
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur);
    }
    out
}

/// The brace-balanced block starting at the first `{` at or after `from`, string-aware.
fn balanced_block(hay: &'static str, from: usize) -> Option<&'static str> {
    let open = hay[from..].find('{').map(|i| from + i)?;
    let mut depth = 0usize;
    let mut in_str = false;
    let mut escaped = false;
    for (i, c) in hay[open..].char_indices() {
        if in_str {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        match c {
            '"' => in_str = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&hay[open..open + i + 1]);
                }
            }
            _ => {}
        }
    }
    None
}

/// Every function named `name` reachable in the crate sources, as (body, parameter names).
///
/// A list rather than one hit, and scoped to `impl <Type>` when the call site names a type, because
/// `SystemPerfMode::parse` is one of several `fn parse(` in the crate and the first one in file order
/// is somebody else's. Resolving by bare name is how a chain walk ends up reading the wrong function
/// and reporting its literals as this parameter's value set.
fn callee_candidates(type_prefix: Option<&str>, name: &str) -> Vec<(&'static str, Vec<String>)> {
    let src = rust_sources();
    let mut regions: Vec<&'static str> = Vec::new();
    if let Some(ty) = type_prefix {
        let needle = format!("impl {ty} {{");
        let mut from = 0usize;
        while let Some(rel) = src[from..].find(&needle) {
            let at = from + rel;
            if let Some(block) = balanced_block(src, at) {
                regions.push(block);
            }
            from = at + needle.len();
        }
    }
    if regions.is_empty() {
        regions.push(src);
    }

    let needle = format!("fn {name}(");
    let mut out = Vec::new();
    for region in regions {
        let mut from = 0usize;
        while let Some(rel) = region[from..].find(&needle) {
            let sig = from + rel;
            let args_start = sig + needle.len();
            // The signature's parameter list, up to the matching close paren.
            let mut depth = 1i32;
            let mut end = args_start;
            for (i, c) in region[args_start..].char_indices() {
                match c {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            end = args_start + i;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            let params: Vec<String> = split_top_level(&region[args_start..end])
                .iter()
                .map(|p| {
                    p.split(':')
                        .next()
                        .unwrap_or("")
                        .trim()
                        .trim_start_matches('&')
                        .trim_start_matches("mut ")
                        .trim()
                        .to_string()
                })
                .collect();
            if let Some(body) = balanced_block(region, end) {
                out.push((body, params));
            }
            from = args_start;
        }
    }
    out
}

/// Identifiers that carry a tracked value inside `body`.
///
/// `iris_test` reads `p.test_type` into `force_type` and matches on that; `iris_doc` pulls
/// `elicitation_answer` out of a tuple `if let`. Tracking only the wire name finds neither.
fn expand_aliases(body: &str, seeds: &BTreeSet<String>) -> BTreeSet<String> {
    const NOT_A_BINDING: &[&str] = &["Some", "None", "Ok", "Err", "ref", "mut", "if", "let"];
    let mut tracked = seeds.clone();
    let mentions = |text: &str, names: &BTreeSet<String>| {
        names.iter().any(|t| {
            regex::Regex::new(&format!(r"\b{}\b", regex::escape(t)))
                .expect("identifier")
                .is_match(text)
        })
    };
    let idents = |text: &str| -> Vec<String> {
        regex::Regex::new(r"\b([a-z_][a-z0-9_]*)\b")
            .expect("static regex")
            .captures_iter(text)
            .map(|c| c[1].to_string())
            .filter(|i| !NOT_A_BINDING.contains(&i.as_str()))
            .collect()
    };

    let plain = regex::Regex::new(
        r"(?s)let\s+(?:mut\s+)?([a-z_][a-z0-9_]*)\s*(?::[^=;]{0,80})?=\s*([^;]{0,400});",
    )
    .expect("static regex");
    let if_let = regex::Regex::new(r"(?s)\bif\s+let\s+(.{1,160}?)\s*=\s*([^{;]{1,240}?)\s*\{")
        .expect("static regex");

    // Two passes: a binding can be built from an earlier binding
    // (`let cat = category.as_str()`).
    for _ in 0..2 {
        for c in plain.captures_iter(body) {
            if !tracked.contains(&c[1]) && mentions(&c[2], &tracked) {
                tracked.insert(c[1].to_string());
            }
        }
        for c in if_let.captures_iter(body) {
            let pattern = c[1].trim();
            let expr = c[2].trim();
            if !mentions(expr, &tracked) {
                continue;
            }
            // A tuple `if let` binds each element separately — `(Some(eid), Some(answer)) =
            // (&p.elicitation_id, &p.elicitation_answer)` must map `answer` to the second element,
            // not to both.
            let pat_parts = split_top_level(pattern.trim_start_matches('(').trim_end_matches(')'));
            let expr_parts = split_top_level(expr.trim_start_matches('(').trim_end_matches(')'));
            if pat_parts.len() > 1 && pat_parts.len() == expr_parts.len() {
                for (p, e) in pat_parts.iter().zip(expr_parts.iter()) {
                    if mentions(e, &tracked) {
                        for id in idents(p) {
                            tracked.insert(id);
                        }
                    }
                }
            } else {
                for id in idents(pattern) {
                    tracked.insert(id);
                }
            }
        }
    }
    tracked
}

/// The string literals `body` compares any of `tracked` against.
///
/// Three shapes, because the codebase uses all three for closed value sets:
/// `match action { "enable" => … }`, `if ct.to_uppercase() != "INT"`, and
/// `let allowed = ["CLS", …]; if !allowed.contains(&category.as_str())`.
fn branch_literals(body: &str, tracked: &BTreeSet<String>) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mentions = |text: &str| {
        tracked.iter().any(|t| {
            regex::Regex::new(&format!(r"\b{}\b", regex::escape(t)))
                .expect("identifier")
                .is_match(text)
        })
    };
    let lit = regex::Regex::new(r#""([^"]*)""#).expect("static regex");

    // (a) match arms. The optional `ident @ (…)` prefix is `iris_macro`'s shape —
    // `action @ ("signature" | "location" | "definition" | "expand") =>` binds the value it also
    // matches on, and a pattern that required the literal to open the line read that arm as absent.
    let arm = regex::Regex::new(
        r#"(?m)^\s*(?:[a-z_][a-z0-9_]*\s*@\s*)?\(?\s*((?:"[^"]*"\s*\|\s*)*"[^"]*")\s*\)?\s*(?:if [^=]*)?=>"#,
    )
    .expect("static regex");
    for m in regex::Regex::new(r"match\s+([^\n{]{1,200})\{")
        .expect("static regex")
        .captures_iter(body)
    {
        if !mentions(&m[1]) {
            continue;
        }
        let Some(block) = balanced_block_local(body, m.get(0).expect("whole match").end() - 1)
        else {
            continue;
        };
        // Only this match's own arms. An arm body may match on a different parameter —
        // `iris_info`'s `what == "documents"` arm matches `doc_type` — and reading the block as flat
        // text donates the inner arms to the outer parameter. `ALL` reached `iris_info.what` that
        // way: a value of `doc_type`, in a set that was otherwise right.
        for a in arm.captures_iter(&outermost_arms_only(block)) {
            for l in lit.captures_iter(&a[1]) {
                out.insert(l[1].to_string());
            }
        }
    }

    // (b) equality against a literal in the same statement.
    for t in tracked {
        let eq = regex::Regex::new(&format!(
            r#"\b{}\b[^;\n{{}}]{{0,80}}?[=!]=\s*"([^"]+)""#,
            regex::escape(t)
        ))
        .expect("identifier");
        for c in eq.captures_iter(body) {
            out.insert(c[1].to_string());
        }
    }

    // (c) membership in an array of literals.
    let arr = regex::Regex::new(
        r#"(?s)let\s+([a-z_][a-z0-9_]*)\s*(?::[^=;]{0,60})?=\s*&?\[((?:\s*"[^"]*"\s*,?)+)\]\s*;"#,
    )
    .expect("static regex");
    for c in arr.captures_iter(body) {
        let used = regex::Regex::new(&format!(
            r"\b{}\s*\.\s*contains\s*\(([^)]{{0,120}})",
            regex::escape(&c[1])
        ))
        .expect("identifier");
        if !used.captures_iter(body).any(|u| mentions(&u[1])) {
            continue;
        }
        for l in lit.captures_iter(&c[2]) {
            out.insert(l[1].to_string());
        }
    }

    out
}

/// A match block with everything inside an arm's own braces blanked out.
///
/// Newlines survive so line-anchored arm patterns still line up, and every other nested character
/// becomes a space. What is left is the arms of this match and nothing they contain.
fn outermost_arms_only(block: &str) -> String {
    let mut out = String::with_capacity(block.len());
    let mut depth = 0usize;
    let mut in_str = false;
    let mut escaped = false;
    for c in block.chars() {
        if in_str {
            out.push(if depth > 1 && c != '\n' { ' ' } else { c });
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        match c {
            '"' => {
                in_str = true;
                out.push(if depth > 1 { ' ' } else { c });
            }
            '{' => {
                depth += 1;
                out.push(if depth > 1 { ' ' } else { c });
            }
            '}' => {
                out.push(if depth > 1 { ' ' } else { c });
                depth = depth.saturating_sub(1);
            }
            '\n' => out.push('\n'),
            _ => out.push(if depth > 1 { ' ' } else { c }),
        }
    }
    out
}

/// `balanced_block` over a borrowed body rather than the `'static` blob.
fn balanced_block_local(hay: &str, open: usize) -> Option<&str> {
    let mut depth = 0usize;
    let mut in_str = false;
    let mut escaped = false;
    for (i, c) in hay[open..].char_indices() {
        if in_str {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        match c {
            '"' => in_str = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&hay[open..open + i + 1]);
                }
            }
            _ => {}
        }
    }
    None
}

/// The text between an already-consumed `(` and its matching `)`.
fn balanced_parens(hay: &str, after_open: usize) -> &str {
    let mut depth = 1i32;
    let mut in_str = false;
    let mut escaped = false;
    for (i, c) in hay[after_open..].char_indices() {
        if in_str {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        match c {
            '"' => in_str = true,
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return &hay[after_open..after_open + i];
                }
            }
            _ => {}
        }
    }
    &hay[after_open..]
}

/// Does the code branch on `param` with a catch-all arm?
///
/// A `_ =>` arm is how two of the seventeen enums reach a value that never appears as a literal:
/// `iris_query`'s `read` and `iris_generate`'s `class` are the default, so the match names the other
/// three and four. A declared value justified as "the default arm" is only justified if there is one.
pub fn handler_branch_has_default(tool: &str, param: &str) -> bool {
    let (body, tracked) = match branch_site(tool, param) {
        Some(x) => x,
        None => return false,
    };
    let catch_all = regex::Regex::new(r"(?m)^\s*_\s*(?:if [^=]*)?=>").expect("static regex");
    for m in regex::Regex::new(r"match\s+([^\n{]{1,200})\{")
        .expect("static regex")
        .captures_iter(&body)
    {
        let scrutinee = m[1].to_string();
        if !tracked.iter().any(|t| {
            regex::Regex::new(&format!(r"\b{}\b", regex::escape(t)))
                .expect("identifier")
                .is_match(&scrutinee)
        }) {
            continue;
        }
        if let Some(block) = balanced_block_local(&body, m.get(0).expect("whole match").end() - 1) {
            if catch_all.is_match(block) {
                return true;
            }
        }
    }
    false
}

/// Functions that consume a value rather than branch on it. Following them turns the chain walk into
/// a crawl of the whole crate.
const NOT_A_BRANCH: &[&str] = &[
    "and_then",
    "as_bytes",
    "as_deref",
    "as_object",
    "as_str",
    "as_u64",
    "clone",
    "collect",
    "contains",
    // The policy gate reads `action` out of a params_json to decide whether a kill allowlist
    // applies, so it compares the parameter against `"kill"` without being its dispatcher. Following
    // it made the gate's two literals the whole advertised set for `iris_global` and
    // `iris_source_control`, both of which gate before they dispatch.
    "dispatch_gate",
    "err_json",
    "err_result",
    "expect",
    "filter",
    "format",
    "from",
    "get",
    "insert",
    "is_empty",
    "is_match",
    "iter",
    "json",
    "len",
    "map",
    "new",
    "ok_json",
    "push",
    "push_str",
    "record_call",
    "to_lowercase",
    "to_owned",
    "to_string",
    "to_uppercase",
    "to_value",
    "trim",
    "unwrap",
    "unwrap_or",
    "unwrap_or_default",
    "unwrap_or_else",
    "write",
];

/// The body that branches on `param`, plus the identifiers carrying its value there.
fn branch_site(tool: &str, param: &str) -> Option<(String, BTreeSet<String>)> {
    fn walk(
        body: &'static str,
        seeds: BTreeSet<String>,
        depth: usize,
        seen: &mut BTreeSet<String>,
    ) -> Option<(String, BTreeSet<String>)> {
        let tracked = expand_aliases(body, &seeds);
        if !branch_literals(body, &tracked).is_empty() {
            return Some((body.to_string(), tracked));
        }
        if depth == 0 {
            return None;
        }
        let call = regex::Regex::new(
            r"(?s)\b((?:[A-Za-z_][A-Za-z0-9_]*\s*::\s*)*)([a-z_][a-z0-9_]*)\s*\(",
        )
        .expect("static regex");
        let passthrough = regex::Regex::new(r"^\s*&?\s*(?:p|params)\s*$").expect("static regex");
        for c in call.captures_iter(body) {
            let name = c[2].to_string();
            if NOT_A_BRANCH.contains(&name.as_str()) || seen.contains(&name) {
                continue;
            }
            // The argument list must end at its own close paren. Reading to the next `;` instead
            // swallows `).await` into the final argument, so a params binding passed as the last
            // argument stops looking like one — which is how three of the seventeen parameters
            // reported "nothing branches on this".
            let args = split_top_level(balanced_parens(body, c.get(0).expect("whole match").end()));
            // Which argument carries the value: one naming a tracked identifier, or the whole
            // params binding passed through (`handle_iris_coverage(&iris, &client, &p)`).
            let mentions = |text: &str| {
                tracked.iter().any(|t| {
                    regex::Regex::new(&format!(r"\b{}\b", regex::escape(t)))
                        .expect("identifier")
                        .is_match(text)
                })
            };
            let Some(idx) = args
                .iter()
                .position(|a| mentions(a) || passthrough.is_match(a))
            else {
                continue;
            };
            // A whole params binding handed through keeps the field name on the other side
            // (`params.mode`), so the seeds already name it. Adding the callee's parameter name for
            // that case would put `p` in the tracked set, and `p` appears in every line of a handler
            // — every literal in the callee would read as this parameter's value.
            let positional = !passthrough.is_match(&args[idx]);
            let segments: Vec<&str> = c[1]
                .split("::")
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .collect();
            let type_prefix = segments
                .last()
                .copied()
                .filter(|s| s.starts_with(char::is_uppercase));
            let candidates = callee_candidates(type_prefix, &name);
            if candidates.is_empty() {
                continue;
            }
            seen.insert(name);
            for (callee, params) in candidates {
                let mut next = seeds.clone();
                // Positional hand-off: `SystemPerfMode::parse(mode)` binds `mode` to `s`, and the
                // match inside is on `s`. Without the mapping the callee looks like it branches on
                // nothing.
                if positional {
                    if let Some(p) = params.get(idx) {
                        if !p.is_empty() && p != "self" && p != "p" && p != "params" {
                            next.insert(p.clone());
                        }
                    }
                }
                if let Some(hit) = walk(callee, next, depth - 1, seen) {
                    return Some(hit);
                }
            }
        }
        None
    }

    let mut seeds = BTreeSet::new();
    seeds.insert(param.to_string());
    let mut seen = BTreeSet::new();
    seen.insert(tool.to_string());
    walk(handler_body(tool), seeds, 3, &mut seen)
}

/// The string literals a tool's code branches on for `param`, following the call chain.
///
/// This is the source of truth for every declared enum: FR-006 requires the advertised value set to
/// be the set the code accepts, and a value list copied from the tool description is exactly the
/// drift this feature exists to remove. The walk starts at the handler, tracks the identifiers
/// carrying the parameter's value, and follows calls that receive one of them (or the whole params
/// binding) up to three levels deep. It stops at the first body that branches — `iris_admin` hands
/// several actions to helpers that branch on their own `action`, and collecting every level would
/// report those as `iris_admin`'s values.
///
/// Returns an empty set when nothing in the chain branches on the parameter. That is a real answer —
/// `iris_add_server.scheme` is stored and interpolated into a URL, never compared — and callers must
/// decide what it means rather than reading it as agreement.
pub fn handler_match_arms(tool: &str, param: &str) -> BTreeSet<String> {
    match branch_site(tool, param) {
        Some((body, tracked)) => branch_literals(&body, &tracked),
        None => BTreeSet::new(),
    }
}

/// Spawn the binary, read `tools/list`, return name → `inputSchema`.
///
/// Asserts on the listing a running server emits, after `normalize_schema_openapi3` — not on
/// `schema_for!`, which would test schemars rather than this server (FR-013).
pub fn advertised_schemas() -> BTreeMap<String, serde_json::Value> {
    advertised_tools()
        .into_iter()
        .map(|(name, tool)| {
            let schema = tool
                .get("inputSchema")
                .cloned()
                .unwrap_or_else(|| panic!("`{name}` has no inputSchema in tools/list"));
            (name, schema)
        })
        .collect()
}

/// Every entry of `tools/list`, keyed by name — the whole tool object, not just its schema.
pub fn advertised_tools() -> BTreeMap<String, serde_json::Value> {
    advertised_tools_in_toolset(None)
}

/// The same listing, from a server started in a named toolset.
///
/// The default tier is Merged, which prunes nine tools that the Baseline and Nostub tiers still
/// advertise (`iris_list_containers`, `debug_source_map`, `agent_info`, …). A census that only ever
/// reads the default listing cannot see them, which is how those nine kept an open parameter set
/// through a feature whose whole point was closing them.
pub fn advertised_tools_in_toolset(toolset: Option<&str>) -> BTreeMap<String, serde_json::Value> {
    advertised_tools_in_toolset_with(toolset, &[])
}

/// The tool whose description the suppression tests blank.
///
/// Named as a constant so the test that asserts "exactly one tool lost its description" and the one
/// that asserts "this tool lost its description" cannot disagree about which tool that is.
/// `stream_inspect` because it is where the `max_chars` bug lived: the parameter was in the prose
/// and nowhere else, so it is the tool whose schema most needed to stand on its own.
pub const TOOL_UNDER_SUPPRESSION: &str = "stream_inspect";

/// `advertised_tools_in_toolset`, with named tools' descriptions suppressed in the listing.
///
/// `suppress` is passed as a list rather than a pre-joined string so a caller cannot accidentally
/// depend on the separator; the joining happens here, once.
pub fn advertised_tools_in_toolset_with(
    toolset: Option<&str>,
    suppress: &[&str],
) -> BTreeMap<String, serde_json::Value> {
    advertised_tools_spawned(toolset, suppress, &[])
}

/// `advertised_tools_in_toolset`, with extra arguments handed to the `mcp` subcommand.
///
/// Exists so the CLI flag can be tested through the CLI. Setting the env var proves `list_tools`
/// reads it; only passing `--suppress-tool-description` proves the flag reaches `list_tools` at
/// all, which is the half that broke in #111.
pub fn advertised_tools_with_flags(flags: &[&str]) -> BTreeMap<String, serde_json::Value> {
    advertised_tools_spawned(None, &[], flags)
}

fn advertised_tools_spawned(
    toolset: Option<&str>,
    suppress: &[&str],
    flags: &[&str],
) -> BTreeMap<String, serde_json::Value> {
    let bin = iad_binary_path();
    assert!(
        bin.exists(),
        "no iris-agentic-dev binary at {}\n\
         This test reads the listing a running server emits, so without the binary it asserts \
         nothing.\n\
         Build it: cargo build -p iris-agentic-dev",
        bin.display()
    );

    let mut cmd = clean_mcp_command(&bin);
    if let Some(tier) = toolset {
        cmd.env("IRIS_TOOLSET", tier);
    }
    if !suppress.is_empty() {
        cmd.env("IRIS_SUPPRESS_TOOL_DESCRIPTION", suppress.join(","));
    }
    cmd.args(flags);
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("must spawn the binary");

    {
        let stdin = child.stdin.as_mut().expect("piped stdin");
        let init = serde_json::json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {"name": "schema-census", "version": "0"}
            }
        });
        writeln!(stdin, "{init}").expect("must write initialize");
        let list = serde_json::json!({
            "jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}
        });
        writeln!(stdin, "{list}").expect("must write tools/list");
    }
    let stdout = child.stdout.take().expect("piped stdout");

    let mut listing = None;
    for line in BufReader::new(stdout).lines().map_while(Result::ok) {
        let Ok(msg) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        if msg.get("id").and_then(serde_json::Value::as_u64) == Some(2) {
            listing = msg.get("result").cloned();
            break;
        }
    }
    let _ = child.kill();
    let _ = child.wait();

    let listing = listing.expect("tools/list must answer with a result");
    assert!(
        listing.get("nextCursor").map(serde_json::Value::is_null) != Some(false),
        "tools/list paginated (IRIS_LIST_TOOLS_PAGE_SIZE default is 200): this helper reads one \
         page, so a non-null nextCursor means the census saw a subset. Follow the cursor or raise \
         the page size."
    );
    let tools = listing
        .get("tools")
        .and_then(serde_json::Value::as_array)
        .expect("tools/list result must carry a tools array");
    assert!(
        tools.len() >= 81,
        "tools/list returned {} tools, expected at least 81 — a shrunken listing makes every \
         per-tool assertion below vacuous",
        tools.len()
    );

    tools
        .iter()
        .map(|t| {
            let name = t
                .get("name")
                .and_then(serde_json::Value::as_str)
                .expect("every tool has a name")
                .to_string();
            (name, t.clone())
        })
        .collect()
}

/// Which of these tools advertise an open parameter set while their handler reads named keys.
///
/// The census's assertion is this list compared against zero. It is a function taking the schemas
/// and the read-set lookup rather than reading both itself, so a test can hand it a fixture tool
/// and prove the assertion fails on one — a guard that has never been shown to fire is a guard
/// nobody has checked.
///
/// "Open" means no `properties` key at all. `properties: {}` is the positive statement "this tool
/// takes nothing", which is a different and correct thing to advertise.
pub fn tools_with_open_parameter_sets(
    schemas: &BTreeMap<String, serde_json::Value>,
    read_keys_of: &dyn Fn(&str) -> BTreeSet<String>,
) -> Vec<String> {
    let mut open: Vec<String> = schemas
        .iter()
        .filter(|(_, schema)| schema.get("properties").is_none())
        .filter(|(name, _)| !read_keys_of(name).is_empty())
        .map(|(name, _)| name.clone())
        .collect();
    open.sort();
    open
}

/// A property from an advertised schema, following `anyOf[0]` when `list_tools` moved it there.
///
/// `normalize_schema_openapi3` rewrites a nullable type array into
/// `anyOf: [{…}, {"type": "null"}]`, relocating `enum`, `format`, `minimum`, `items`, `properties`,
/// `required` and `additionalProperties` into the non-null branch. A test reading
/// `properties.mode.enum` therefore finds nothing on any optional parameter, and written in the
/// natural "compare if present" style it passes vacuously forever.
///
/// Panics when the property is absent, for the same reason.
pub fn resolve_property(schema: &serde_json::Value, name: &str) -> serde_json::Value {
    let prop = schema
        .get("properties")
        .and_then(|p| p.get(name))
        .unwrap_or_else(|| {
            panic!(
                "no property `{name}` in this schema: {}",
                serde_json::to_string(schema).unwrap_or_default()
            )
        });
    let Some(branch) = prop
        .get("anyOf")
        .and_then(serde_json::Value::as_array)
        .and_then(|a| a.first())
    else {
        return prop.clone();
    };
    // Merge: the sibling keys `normalize_schema_openapi3` leaves outside (`description`,
    // `default`) still belong to the property.
    let mut merged = branch.clone();
    if let (Some(m), Some(o)) = (merged.as_object_mut(), prop.as_object()) {
        for (k, v) in o {
            if k != "anyOf" && !m.contains_key(k) {
                m.insert(k.clone(), v.clone());
            }
        }
    }
    merged
}

/// The property names a tool advertises.
pub fn advertised_properties(schema: &serde_json::Value) -> BTreeSet<String> {
    schema
        .get("properties")
        .and_then(serde_json::Value::as_object)
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default()
}

/// One live stdio session against the built binary, held open across several tool calls.
///
/// Held open because a good deal of what these tests must verify lives in the server process and
/// dies with it: `global_preview` mints a confirm_token into an in-process map that `global_kill`
/// looks up, `iris_ws_open` returns a handle into a pool, `iris_execute` carries `%ctx`. A
/// preview-then-kill pair sent as two spawns always answers `CONFIRM_REQUIRED`, which looks exactly
/// like a dropped parameter.
///
/// Each call is written and its answer drained before the next is written, so a large response
/// (`compare_namespace` over a real namespace) cannot fill the pipe while both sides are writing.
pub struct McpSession {
    child: std::process::Child,
    reader: BufReader<std::process::ChildStdout>,
    next_id: u64,
}

impl McpSession {
    /// Spawn `iris-agentic-dev mcp` with exactly `env` set and complete the handshake.
    ///
    /// `env` is added back on top of `clean_command`'s empty slate, so a test states its own
    /// environment. Nothing is inherited.
    pub fn start(env: &[(String, String)]) -> Self {
        let bin = iad_binary_path();
        assert!(
            bin.exists(),
            "no iris-agentic-dev binary at {} — build it: cargo build -p iris-agentic-dev",
            bin.display()
        );

        let mut cmd = clean_mcp_command(&bin);
        for (k, v) in env {
            cmd.env(k, v);
        }
        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("must spawn the binary");

        {
            let stdin = child.stdin.as_mut().expect("piped stdin");
            let init = serde_json::json!({
                "jsonrpc": "2.0", "id": 1, "method": "initialize",
                "params": {
                    "protocolVersion": "2024-11-05",
                    "capabilities": {},
                    "clientInfo": {"name": "params-roundtrip", "version": "0"}
                }
            });
            writeln!(stdin, "{init}").expect("must write initialize");
        }
        let reader = BufReader::new(child.stdout.take().expect("piped stdout"));
        let mut session = Self {
            child,
            reader,
            next_id: 1,
        };
        session.read_answer(1, "initialize");
        session
    }

    /// Call one tool and return the whole JSON-RPC response message.
    ///
    /// The whole message, not `result`, because the rejection paths this feature cares about land in
    /// different places: a gate refusal and an `UNKNOWN_PARAMETER` refusal come back as a
    /// `CallToolResult` with `isError`, while a deserialization failure from
    /// `#[serde(deny_unknown_fields)]` comes back as a JSON-RPC `error`. A helper returning only
    /// `result` would make the second case look like "no answer".
    pub fn call(&mut self, tool: &str, args: &serde_json::Value) -> serde_json::Value {
        self.next_id += 1;
        let id = self.next_id;
        {
            let stdin = self.child.stdin.as_mut().expect("piped stdin");
            let call = serde_json::json!({
                "jsonrpc": "2.0", "id": id, "method": "tools/call",
                "params": {"name": tool, "arguments": args}
            });
            writeln!(stdin, "{call}").expect("must write tools/call");
        }
        self.read_answer(id, tool)
    }

    fn read_answer(&mut self, id: u64, what: &str) -> serde_json::Value {
        let mut line = String::new();
        loop {
            line.clear();
            let read = self
                .reader
                .read_line(&mut line)
                .unwrap_or_else(|e| panic!("reading the answer to `{what}` failed: {e}"));
            assert!(
                read != 0,
                "the server closed stdout before answering `{what}` — it exited early, which a \
                 test reading for JSON-RPC otherwise sees as an empty answer"
            );
            let Ok(msg) = serde_json::from_str::<serde_json::Value>(&line) else {
                continue;
            };
            if msg.get("id").and_then(serde_json::Value::as_u64) == Some(id) {
                return msg;
            }
        }
    }
}

impl Drop for McpSession {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// One tool call in its own session — the common case.
pub fn call_tool_with_env(
    tool: &str,
    args: &serde_json::Value,
    env: &[(String, String)],
) -> serde_json::Value {
    McpSession::start(env).call(tool, args)
}

/// The connection variables a live tool call needs, read from the test process.
///
/// Panics when `IRIS_HOST` is unset. A live test that silently runs against no server is the failure
/// mode this whole feature exists to remove; the `#[ignore]` attribute is how a run opts out, not an
/// empty environment.
pub fn live_env() -> Vec<(String, String)> {
    const CONNECTION: &[&str] = &[
        "IRIS_HOST",
        "IRIS_WEB_PORT",
        "IRIS_SCHEME",
        "IRIS_NAMESPACE",
        "IRIS_USERNAME",
        "IRIS_PASSWORD",
        "IRIS_CONTAINER",
    ];
    assert!(
        !std::env::var("IRIS_HOST").unwrap_or_default().is_empty(),
        "IRIS_HOST must be set for a live tool call. Expected the iris-dev-iris container: \
         IRIS_HOST=localhost IRIS_WEB_PORT=52780 IRIS_USERNAME=_SYSTEM IRIS_PASSWORD=SYS"
    );
    CONNECTION
        .iter()
        .filter_map(|k| std::env::var(k).ok().map(|v| ((*k).to_string(), v)))
        .collect()
}

/// The text of a `tools/call` answer, whichever shape it arrived in.
///
/// Refusals, successes and JSON-RPC errors all get flattened to one string so a test can assert on
/// an error code without first knowing which of the three it got.
pub fn answer_text(answer: &serde_json::Value) -> String {
    serde_json::to_string(answer).unwrap_or_default()
}

/// One tool's parameter contract: the tool name and its wire name → advertised JSON type pairs.
///
/// Eight conversion batches assert the same five things about their tools. Stating those five in one
/// place keeps them stated identically — the alternative is eight copies that drift, and a batch
/// whose copy quietly lost the `additionalProperties` check would look just as green.
pub struct ParamContract {
    pub tool: &'static str,
    pub params: &'static [(&'static str, &'static str)],
    /// Wire names the tool advertises as unconditionally required. Almost always empty: a parameter
    /// that can be omitted today must stay omittable (FR-004), and a parameter whose necessity
    /// depends on another parameter's value must never be required (FR-007). `iris_admin.action` is
    /// the exception — without it there is no tool to dispatch to.
    pub required: &'static [&'static str],
}

impl ParamContract {
    /// The wire names this contract declares.
    pub fn names(&self) -> BTreeSet<String> {
        self.params
            .iter()
            .map(|(name, _)| (*name).to_string())
            .collect()
    }

    /// The wire names this contract declares as required.
    pub fn required_names(&self) -> BTreeSet<String> {
        self.required.iter().map(|n| (*n).to_string()).collect()
    }
}

/// Assert, against a spawned server's `tools/list`, that each contract is exactly what its tool
/// advertises: the same property names, the stated type for each, a closed parameter set, and
/// nothing marked unconditionally required.
///
/// Requires the built binary. Nothing here reads `schema_for!` — the normalized listing a client
/// receives is the only thing worth asserting on (FR-013).
pub fn assert_advertised_contracts(contracts: &[ParamContract]) {
    let schemas = advertised_schemas();
    for contract in contracts {
        let schema = schemas
            .get(contract.tool)
            .unwrap_or_else(|| panic!("`{}` must appear in tools/list", contract.tool));

        assert_eq!(
            advertised_properties(schema),
            contract.names(),
            "`{}` advertises a different parameter set than its contract states; schema was {schema}",
            contract.tool
        );

        for (name, expected_type) in contract.params {
            let prop = resolve_property(schema, name);
            assert_advertised_type(contract.tool, name, expected_type, &prop);
        }

        assert_eq!(
            schema.get("additionalProperties"),
            Some(&serde_json::json!(false)),
            "`{}` must advertise a closed parameter set; schema was {schema}",
            contract.tool
        );

        // FR-004/FR-007: a parameter that can be omitted today must stay omittable, because
        // omitting it reaches a handler error that says what to do next and `required` would replace
        // that with a serde message naming a Rust struct. Contracts state their required set
        // explicitly so requiring one more is a test failure, not a silent tightening.
        let advertised_required: BTreeSet<String> = schema
            .get("required")
            .and_then(serde_json::Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        assert_eq!(
            advertised_required,
            contract.required_names(),
            "`{}` advertises a different required set than its contract states \
             (FR-004/FR-007); schema was {schema}",
            contract.tool
        );
    }
}

/// Compare one property's advertised `type` against what its contract states.
///
/// A contract entry names one JSON type (`"string"`), or several joined by `|`
/// (`"integer|string"`) for a parameter that genuinely accepts more than one form. JSON Schema
/// spells the second case as an array, so the two are compared differently on purpose: a contract
/// saying `"integer|string"` must not pass against a bare `"integer"`, which is exactly how
/// `session_id` would silently narrow if `StringOrI64`'s type override were dropped.
fn assert_advertised_type(tool: &str, name: &str, expected_type: &str, prop: &serde_json::Value) {
    let advertised = prop
        .get("type")
        .unwrap_or_else(|| panic!("`{tool}`.{name} advertises no `type` at all: {prop}"));

    if let Some((first, rest)) = expected_type.split_once('|') {
        let expected: Vec<&str> = std::iter::once(first).chain(rest.split('|')).collect();
        let actual: Vec<&str> = advertised
            .as_array()
            .unwrap_or_else(|| {
                panic!(
                    "`{tool}`.{name} accepts {expected:?}, so its `type` must be an array of those \
                     names; got {advertised}"
                )
            })
            .iter()
            .map(|v| {
                v.as_str()
                    .unwrap_or_else(|| panic!("`{tool}`.{name} has a non-string type entry: {v}"))
            })
            .collect();
        assert_eq!(
            actual, expected,
            "`{tool}`.{name} must advertise types {expected:?}, got {advertised}"
        );
        return;
    }

    assert_eq!(
        advertised.as_str(),
        Some(expected_type),
        "`{tool}`.{name} must advertise type `{expected_type}`, got {prop}"
    );
}

/// Assert, by scanning the handler source, that each contract matches what the handlers read.
///
/// The other half of FR-003 and the half that needs no binary: a parameter declared but never read
/// is the `max_chars` bug, and it passes every schema assertion there is.
pub fn assert_source_contracts(contracts: &[ParamContract]) {
    for contract in contracts {
        assert_eq!(
            read_keys(contract.tool),
            contract.names(),
            "`{}`'s handler reads a different parameter set than its contract states (FR-003)",
            contract.tool
        );
        for (name, _) in contract.params {
            assert!(
                handler_uses_field(contract.tool, name),
                "`{}` declares `{name}` but its handler body never reads it — that is the \
                 `max_chars` shape of bug this feature exists to prevent",
                contract.tool
            );
        }
    }
}

// ─── live interoperability fixture ──────────────────────────────────────────

/// The interoperability data a live test asserts against, and the names it is filed under.
///
/// # Why a fixture and not the container's history
///
/// Seven tests in `params_batch5`, `params_batch6` and `test_mirror_and_freespace` shipped asserting
/// against whatever `iris-dev-iris` happened to hold: "several thousand rows including complete
/// sessions", "an Event Log with error entries", "two seeded rule sets", "seeded lookup tables".
/// That is true of a container a developer has been working in for months and false of a fresh one,
/// so the tests were green on every local run and red the first time CI's `iris-e2e` ran them —
/// `limit=2` returned one row, `limit=20` returned four, the session lookup found no sessioned
/// message, the body join returned nothing.
///
/// A test that depends on accumulated state is not testing the parameter it claims to test. Each of
/// those assertions now runs against rows this function created.
pub struct InteropFixture {
    /// `SourceConfigName` on every seeded message header.
    pub source: &'static str,
    /// `TargetConfigName` on every seeded message header.
    pub target: &'static str,
    /// `MessageBodyClassName` on every seeded header, and a real table to join.
    pub body_class: &'static str,
    /// `ConfigName` on every seeded Event Log entry.
    pub component: &'static str,
    /// The name of the seeded `Ens.Rule.RuleSet` row.
    pub rule_name: &'static str,
    /// The name of the seeded `^Ens.LookupTable` subscript.
    pub lookup_table: &'static str,
    /// A key present in `lookup_table`.
    pub lookup_key: &'static str,
    /// The `SessionId` every seeded header shares. Always nonzero.
    pub session_id: i64,
    /// Seeded headers, at least [`InteropFixture::MESSAGES`].
    pub messages: i64,
    /// Seeded Event Log entries, at least [`InteropFixture::LOG_ENTRIES`].
    pub log_entries: i64,
}

impl InteropFixture {
    /// Headers the fixture guarantees. Chosen so a `limit=20` assertion has room to spare.
    pub const MESSAGES: i64 = 30;
    /// `Ens.Util.Log` entries of type Error the fixture guarantees.
    pub const LOG_ENTRIES: i64 = 25;
}

/// Create the interoperability fixture in the live namespace, or return what is already there.
///
/// Idempotent by count: each of the four kinds of row is created only when the namespace holds fewer
/// than the fixture promises, so a developer container is seeded once and a CI container is seeded on
/// its first run. Nothing is deleted — these are fixtures, not test residue, and the names are all
/// prefixed `IADFixture` so they are distinguishable from real traffic.
///
/// Runs through the `iris_execute` tool in its own session with the write gate open, so the tests
/// that consume the fixture keep whatever gate posture they are actually testing.
///
/// # Panics
///
/// When the seed does not report success. A live test whose fixture silently failed to appear is the
/// failure this function exists to remove, so there is no quiet path.
pub fn seed_interop_fixture() -> InteropFixture {
    const SOURCE: &str = "IADFixtureSource";
    const TARGET: &str = "IADFixtureTarget";
    const BODY_CLASS: &str = "Ens.StringContainer";
    const COMPONENT: &str = "IADFixtureComponent";
    const RULE: &str = "IADFixture.RuleSet";
    const TABLE: &str = "IADFixtureLookup";
    const KEY: &str = "IADFixtureKey";

    let mut env = live_env();
    env.push(("IRIS_WRITE_TOOLS_ENABLED".to_string(), "1".to_string()));

    // One `iris_execute` for all four kinds of row: four round trips would cost four temp-class
    // compiles. `New` is omitted deliberately — the generated method is a procedure block, where
    // `New` of an undeclared variable is a <SYNTAX> error at compile time.
    let code = format!(
        r#"Set tHdrs=0
Set tRS=##class(%SQL.Statement).%ExecDirect(,"SELECT COUNT(*) AS C FROM Ens.MessageHeader WHERE SourceConfigName='{SOURCE}'")
If tRS.%Next() {{ Set tHdrs=tRS.%Get("C") }}
Set tSession=0
If tHdrs<{messages} {{
  For i=1:1:{messages} {{
    Set tBody=##class({BODY_CLASS}).%New("IAD fixture body "_i)
    Set tSC=tBody.%Save()
    If $$$ISERR(tSC) {{ Write "BODY|"_$SYSTEM.Status.GetErrorText(tSC),! Quit }}
    Set tHdr=##class(Ens.MessageHeader).%New()
    Set tHdr.SourceConfigName="{SOURCE}"
    Set tHdr.TargetConfigName="{TARGET}"
    Set tHdr.MessageBodyClassName="{BODY_CLASS}"
    Set tHdr.MessageBodyId=tBody.%Id()
    Set tSC=tHdr.%Save()
    If $$$ISERR(tSC) {{ Write "HEADER|"_$SYSTEM.Status.GetErrorText(tSC),! Quit }}
    If tSession=0 {{ Set tSession=tHdr.%Id() }}
    Set tHdr.SessionId=tSession
    Set tSC=tHdr.%Save()
    If $$$ISERR(tSC) {{ Write "SESSION|"_$SYSTEM.Status.GetErrorText(tSC),! Quit }}
  }}
}}
Set tLogs=0
Set tRS=##class(%SQL.Statement).%ExecDirect(,"SELECT COUNT(*) AS C FROM Ens_Util.Log WHERE ConfigName='{COMPONENT}'")
If tRS.%Next() {{ Set tLogs=tRS.%Get("C") }}
If tLogs<{logs} {{
  For i=1:1:{logs} {{
    Set tLog=##class(Ens.Util.Log).%New()
    Set tLog.Type=2
    Set tLog.ConfigName="{COMPONENT}"
    Set tLog.SourceClass="IADFixture.Seed"
    Set tLog.SourceMethod="seed_interop_fixture"
    Set tLog.Text="IAD fixture error entry "_i
    Set tSC=tLog.%Save()
    If $$$ISERR(tSC) {{ Write "LOG|"_$SYSTEM.Status.GetErrorText(tSC),! Quit }}
  }}
}}
Set tRules=0
Set tRS=##class(%SQL.Statement).%ExecDirect(,"SELECT COUNT(*) AS C FROM Ens_Rule.RuleSet WHERE Name='{RULE}'")
If tRS.%Next() {{ Set tRules=tRS.%Get("C") }}
If tRules=0 {{
  Set tRule=##class(Ens.Rule.RuleSet).%New()
  Set tRule.Name="{RULE}"
  Set tRule.ShortDescription="Rule set seeded by seed_interop_fixture"
  Set tRule.HostClass="EnsLib.MsgRouter.RoutingEngine"
  Set tSC=tRule.%Save()
  If $$$ISERR(tSC) {{ Write "RULE|"_$SYSTEM.Status.GetErrorText(tSC),! }}
}}
If '$DATA(^Ens.LookupTable("{TABLE}","{KEY}")) {{
  Set tSC=##class(Ens.Util.LookupTable).%UpdateValue("{TABLE}","{KEY}","IAD fixture value",1)
  If $$$ISERR(tSC) {{ Write "LOOKUP|"_$SYSTEM.Status.GetErrorText(tSC),! }}
}}
Set tRS=##class(%SQL.Statement).%ExecDirect(,"SELECT COUNT(*) AS C, MAX(SessionId) AS S FROM Ens.MessageHeader WHERE SourceConfigName='{SOURCE}'")
If tRS.%Next() {{ Set tHdrs=tRS.%Get("C") Set tSession=tRS.%Get("S") }}
Set tRS=##class(%SQL.Statement).%ExecDirect(,"SELECT COUNT(*) AS C FROM Ens_Util.Log WHERE ConfigName='{COMPONENT}'")
If tRS.%Next() {{ Set tLogs=tRS.%Get("C") }}
Write "OK|"_tHdrs_"|"_tLogs_"|"_tSession,!"#,
        messages = InteropFixture::MESSAGES,
        logs = InteropFixture::LOG_ENTRIES,
    );

    let answer = call_tool_with_env(
        "iris_execute",
        &serde_json::json!({"code": code, "namespace": "USER"}),
        &env,
    );
    let text = answer
        .pointer("/result/content/0/text")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_else(|| panic!("seeding the interop fixture returned no content: {answer}"));
    let payload: serde_json::Value = serde_json::from_str(text)
        .unwrap_or_else(|e| panic!("seed answer was not JSON ({e}): {text}"));
    let output = payload
        .get("output")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();

    // `OK|<headers>|<logs>|<session>` is the last line; anything before it is a %Save failure the
    // script reported rather than swallowed.
    let summary = output
        .lines()
        .find(|l| l.starts_with("OK|"))
        .unwrap_or_else(|| {
            panic!("the interop fixture did not seed. Output: {output:?}\nAnswer: {payload}")
        });
    let fields: Vec<&str> = summary.trim().split('|').collect();
    let number = |i: usize, what: &str| -> i64 {
        fields
            .get(i)
            .and_then(|f| f.trim().parse().ok())
            .unwrap_or_else(|| panic!("no {what} in the seed summary {summary:?}"))
    };
    let messages = number(1, "header count");
    let log_entries = number(2, "log count");
    let session_id = number(3, "session id");

    assert!(
        messages >= InteropFixture::MESSAGES,
        "the fixture promises {} headers, IRIS reports {messages}: {output:?}",
        InteropFixture::MESSAGES
    );
    assert!(
        log_entries >= InteropFixture::LOG_ENTRIES,
        "the fixture promises {} Event Log entries, IRIS reports {log_entries}: {output:?}",
        InteropFixture::LOG_ENTRIES
    );
    assert!(
        session_id > 0,
        "the fixture's headers must share a nonzero SessionId; got {session_id}: {output:?}"
    );

    InteropFixture {
        source: SOURCE,
        target: TARGET,
        body_class: BODY_CLASS,
        component: COMPONENT,
        rule_name: RULE,
        lookup_table: TABLE,
        lookup_key: KEY,
        session_id,
        messages,
        log_entries,
    }
}
