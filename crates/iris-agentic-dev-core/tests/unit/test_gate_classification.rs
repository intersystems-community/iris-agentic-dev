// Spec 085 write-gate integrity — the classification table and the tool surface it has to cover.
//
// These are unit tests on purpose: they read the router and the `CLASSIFICATION` table, neither of
// which touches IRIS. Enforcement itself is asserted against a live container in
// `tests/integration/test_gate_enforcement_live.rs`.

use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};
use iris_agentic_dev_core::tools::write_gate::{
    tool_class, DeclaredGates, TargetKind, WriteClass, CLASSIFICATION,
};
use iris_agentic_dev_core::tools::{IrisTools, Toolset};

/// A connection object, not a connection. Nothing here dials IRIS — but the constructor's
/// tool-pruning only ran on the `Some(connection)` branch, so passing `None` would make every
/// assertion below pass for free. That is the whole reason this fixture exists.
fn offline_conn() -> IrisConnection {
    IrisConnection::new(
        "http://localhost:52780",
        "USER",
        "_SYSTEM",
        "SYS",
        DiscoverySource::ExplicitFlag,
    )
}

fn tools_with_declared_skills(
    toolset: Toolset,
    no_skills: bool,
    declared: DeclaredGates,
) -> IrisTools {
    IrisTools::with_registry_and_toolset(
        Some(offline_conn()),
        iris_agentic_dev_core::skills::SkillRegistry::new(),
        toolset,
        None,
        None,
        no_skills,
        declared,
    )
    .expect("IrisTools construction must not fail")
}

fn tools_with_declared(toolset: Toolset, declared: DeclaredGates) -> IrisTools {
    tools_with_declared_skills(toolset, true, declared)
}

const WRITES_OFF: DeclaredGates = DeclaredGates {
    write_tools_enabled: Some(false),
    destructive_tools_enabled: None,
    write_source: None,
    destructive_source: None,
    withhold_launch_loosening: false,
    read_only_strict: false,
};

/// Writes declared off — the state in which the old code stripped tools out of the router.
fn tools_with_writes_off(toolset: Toolset) -> IrisTools {
    tools_with_declared(toolset, WRITES_OFF)
}

/// Every tool surface the binary can serve: three toolsets × skills present or stripped by
/// `--no-skills`. Completeness has to hold over the union, not over one configuration. A tool
/// registered in only one of these is still a tool a caller can invoke — `iris_admin` and
/// `iris_credential_manage` exist only in Merged, the four skill stubs only in Baseline, and the
/// fifteen skill/kb/agent tools disappear under `--no-skills`, which is the mode most of this
/// file's other fixtures happen to use.
fn every_surface() -> Vec<(String, IrisTools)> {
    let mut out = Vec::new();
    for (name, toolset) in [
        ("baseline", Toolset::Baseline),
        ("nostub", Toolset::Nostub),
        ("merged", Toolset::Merged),
    ] {
        for no_skills in [false, true] {
            let label = if no_skills {
                format!("{name}+no-skills")
            } else {
                name.to_string()
            };
            out.push((
                label,
                tools_with_declared_skills(toolset, no_skills, WRITES_OFF),
            ));
        }
    }
    out
}

/// T024. `iris_production_item` and `iris_credential_manage` used to be *removed* from the router
/// when writes were off. That is the failure mode this asserts against, and it is subtle: with the
/// tools absent, the Phase 5 completeness test passes for the wrong reason — it can only check the
/// classification of tools the router actually registered, so a removed tool is a tool nothing
/// verifies. Removal is also invisible to a later reload: the router is built once at startup,
/// while the gate re-resolves on every config change, so an operator who turned writes back on got
/// a gate that said yes and a tool list that had already forgotten the tool existed.
///
/// Visible-but-refusing is the contract. The caller gets `WRITE_TOOLS_DISABLED`, which says *why*;
/// absence says nothing at all.
#[test]
fn write_gated_tools_stay_registered_when_writes_are_off() {
    // Merged is the shipped default (`IRIS_TOOLSET` unset → merged in `mcp.rs`) and the only tier
    // where iris_credential_manage exists at all.
    let names = tools_with_writes_off(Toolset::Merged).registered_tool_names();

    for tool in ["iris_production_item", "iris_credential_manage"] {
        assert!(
            names.contains(tool),
            "{tool} is missing from the router with writes off — it is being removed rather than \
             gated, so the completeness test cannot see it and a reload that turns writes back on \
             will not bring it back. Registered: {} tools.",
            names.len()
        );
    }
}

/// The same assertion for the tier that has to keep working: `iris_production_item` is in Baseline
/// too, and the removal block ran regardless of toolset.
#[test]
fn production_item_stays_registered_in_baseline_with_writes_off() {
    let names = tools_with_writes_off(Toolset::Baseline).registered_tool_names();
    assert!(
        names.contains("iris_production_item"),
        "iris_production_item is missing from the Baseline router with writes off"
    );
}

/// Turning writes *on* must not change the tool list either. If these two counts ever differ, the
/// gate has leaked back into router construction — which is the shape of the defect, not the fix.
#[test]
fn the_advertised_tool_list_does_not_depend_on_the_gate() {
    let off = tools_with_writes_off(Toolset::Merged).registered_tool_names();

    let on = tools_with_declared(
        Toolset::Merged,
        DeclaredGates {
            write_tools_enabled: Some(true),
            destructive_tools_enabled: Some(true),
            ..Default::default()
        },
    )
    .registered_tool_names();

    let only_on: Vec<_> = on.difference(&off).collect();
    let only_off: Vec<_> = off.difference(&on).collect();
    assert!(
        only_on.is_empty() && only_off.is_empty(),
        "the tool list changed with the gate: present only with writes on {only_on:?}, only with \
         writes off {only_off:?}"
    );
}

/// T033 (FR-007, US3 scenario 1). Forward completeness: every registered tool has a
/// `CLASSIFICATION` entry.
///
/// This is the test that makes the gate structural instead of a convention. `gate_check` fails an
/// unclassified tool closed as `Write`, so a missing entry is not an open door — but it is a tool
/// whose refusal nobody chose, which for a read-only tool means a working feature silently stops
/// working the moment writes are off. Either way the classification is a decision someone has to
/// make on purpose, and this is where CI demands it.
///
/// Writes are declared *off* for the fixture because that is the state in which the old code pruned
/// tools out of the router: with a tool absent, this test would pass by never seeing it.
#[test]
fn every_registered_tool_is_classified() {
    for (label, tools) in every_surface() {
        let names = tools.registered_tool_names();
        let mut missing: Vec<&String> = names
            .iter()
            .filter(|n| !CLASSIFICATION.iter().any(|e| e.tool == n.as_str()))
            .collect();
        missing.sort();
        assert!(
            missing.is_empty(),
            "{} tool(s) in the {label} toolset have no write_gate::CLASSIFICATION entry: \
             {missing:?}. Add each one to CLASSIFICATION in \
             crates/iris-agentic-dev-core/src/tools/write_gate.rs — ro() if it only reads, wr() if \
             it can mutate anything, de() for the destructive tier, mixed() if that depends on the \
             action. Until then gate_check fails them closed as Write.",
            missing.len()
        );
    }
}

/// T034. Reverse completeness: every `CLASSIFICATION` entry names a tool some toolset registers.
///
/// A stale entry is the quiet failure mode. `classify` matches on the exact registered name, so
/// renaming a tool without updating the table leaves the old row matching nothing: the rename
/// compiles, the tests that assert *this* tool refuses still pass under whatever name they use, and
/// the renamed tool falls through to the unclassified path. Nothing anywhere says the row went
/// dead. So the union across all three tiers must account for every row.
#[test]
fn every_classification_entry_names_a_registered_tool() {
    let mut registered = std::collections::HashSet::new();
    for (_, tools) in every_surface() {
        registered.extend(tools.registered_tool_names());
    }

    let mut stale: Vec<&str> = CLASSIFICATION
        .iter()
        .map(|e| e.tool)
        .filter(|t| !registered.contains(*t))
        .collect();
    stale.sort_unstable();
    assert!(
        stale.is_empty(),
        "write_gate::CLASSIFICATION has {} entr(ies) that no tool surface registers: {stale:?}. \
         Either the tool was renamed — in which case the entry no longer matches anything and the \
         tool is now unclassified — or it was removed and the row is dead weight. {} distinct tools \
         registered across every toolset/--no-skills combination.",
        stale.len(),
        registered.len()
    );

    // Duplicate rows are the other way a rename goes wrong: `classify` takes the *first* match, so
    // a second entry for the same tool is unreachable and editing it changes nothing.
    let mut seen = std::collections::HashSet::new();
    let dupes: Vec<&str> = CLASSIFICATION
        .iter()
        .map(|e| e.tool)
        .filter(|t| !seen.insert(*t))
        .collect();
    assert!(
        dupes.is_empty(),
        "duplicate CLASSIFICATION entries: {dupes:?}. classify() returns the first match, so the \
         later row is dead code — merge them into one entry."
    );
}

/// T035 (US3 scenario 3). The annotation cross-check, in both directions.
///
/// `read_only_hint` and `destructive_hint` are what a client reads off `tools/list` to decide
/// whether a call needs confirmation; `CLASSIFICATION` is what the server enforces. They are two
/// independent declarations of the same fact, deliberately kept independent — deriving the
/// annotation from the table would make one lie propagate to both, and it is precisely a
/// one-sided lie that shipped: `c641d79` (#94) had to strip `read_only_hint = true` from six
/// mutating tools that advertised themselves read-only for several releases. With both declared by
/// hand, mislabelling a tool takes two edits in two files, and this test names the disagreement.
///
/// Read over every surface, so a tool that only exists in one tier is still checked.
#[test]
fn annotations_agree_with_the_classification() {
    let mut disagreements: Vec<String> = Vec::new();
    let mut checked: std::collections::BTreeMap<String, serde_json::Value> = Default::default();
    for (_, tools) in every_surface() {
        for name in tools.registered_tool_names() {
            if let Some(ann) = tools.tool_annotations(&name) {
                checked.insert(name, ann);
            }
        }
    }

    let mut saw_read_only = 0usize;
    let mut saw_destructive = 0usize;

    for (name, ann) in &checked {
        let name = name.as_str();
        let class = iris_agentic_dev_core::tools::write_gate::classify(name, None);
        let read_only = ann.get("readOnlyHint").and_then(|v| v.as_bool());
        let destructive = ann.get("destructiveHint").and_then(|v| v.as_bool());
        saw_read_only += usize::from(read_only == Some(true));
        saw_destructive += usize::from(destructive == Some(true));

        // readOnlyHint = true is a promise the tool cannot mutate. A tool whose *default* class is
        // ReadOnly but which has write actions (iris_doc, iris_query) is not read-only, so this
        // checks the whole entry rather than the default alone.
        if read_only == Some(true) {
            let entry = CLASSIFICATION.iter().find(|e| e.tool == name);
            let mutating: Vec<&str> = entry
                .map(|e| {
                    e.actions
                        .iter()
                        .filter(|(_, c)| *c != WriteClass::ReadOnly)
                        .map(|(a, _)| *a)
                        .collect()
                })
                .unwrap_or_default();
            if class != Some(WriteClass::ReadOnly) || !mutating.is_empty() {
                disagreements.push(format!(
                    "{name}: annotations say readOnlyHint = true but CLASSIFICATION says \
                     {class:?}{}",
                    if mutating.is_empty() {
                        String::new()
                    } else {
                        format!(" with mutating action(s) {mutating:?}")
                    }
                ));
            }
        }

        // destructiveHint = true is the ☠ marker in docs/tools.md. If the annotation claims it, the
        // destructive tier has to gate it, or the tool warns the caller while the server waves it
        // through on the write gate alone.
        if destructive == Some(true) && class != Some(WriteClass::Destructive) {
            disagreements.push(format!(
                "{name}: annotations say destructiveHint = true but CLASSIFICATION says {class:?} \
                 — the destructive tier does not gate it"
            ));
        }
    }

    assert!(
        disagreements.is_empty(),
        "the router's annotations and write_gate::CLASSIFICATION disagree on {} tool(s). Fix \
         whichever one is wrong — do not derive one from the other, the point is that a \
         mislabelled tool has to be mislabelled twice:\n  {}",
        disagreements.len(),
        disagreements.join("\n  ")
    );

    // A cross-check that reads nothing passes for free, and the ways it could read nothing are all
    // silent: an `annotations` field the router stops populating, a serde rename from camelCase to
    // snake_case, an accessor that returns `None`. Both hint counts are floors well under the
    // present 47/6, so removing a hint from a tool is fine and losing the whole mechanism is not.
    assert!(
        saw_read_only >= 40,
        "only {saw_read_only} tools were seen declaring readOnlyHint = true (expected 40+). Either \
         the annotations stopped reaching the router or `tool_annotations` no longer reads them — \
         this cross-check is now asserting nothing. Keys are camelCase on the wire; a serde rename \
         looks exactly like this."
    );
    assert!(
        saw_destructive >= 5,
        "only {saw_destructive} tools were seen declaring destructiveHint = true (expected 5+); \
         same failure mode as above"
    );
}

// ── 099: fresh container setup actions must classify as Write, not Destructive ──

fn args_with_action(action: &str) -> serde_json::Map<String, serde_json::Value> {
    let mut m = serde_json::Map::new();
    m.insert(
        "action".to_string(),
        serde_json::Value::String(action.to_string()),
    );
    m
}

#[test]
fn test_fresh_container_setup_actions_classify_as_write() {
    for action in &[
        "clear_password_change_flag",
        "unlock_user",
        "fresh_container_setup",
    ] {
        let args = args_with_action(action);
        let class = iris_agentic_dev_core::tools::write_gate::classify("iris_admin", Some(&args));
        assert_eq!(
            class,
            Some(WriteClass::Write),
            "iris_admin action={action} must classify as WriteClass::Write, got {:?}",
            class
        );
    }
}

// ── 097: mirror management actions must classify at correct tiers ──

#[test]
fn test_mirror_add_async_classifies_as_write() {
    let args = args_with_action("mirror_add_async");
    let class = iris_agentic_dev_core::tools::write_gate::classify("iris_admin", Some(&args));
    assert_eq!(
        class,
        Some(WriteClass::Write),
        "iris_admin action=mirror_add_async must classify as WriteClass::Write, got {:?}",
        class
    );
}

#[test]
fn test_mirror_failover_classifies_as_destructive() {
    let args = args_with_action("mirror_failover");
    let class = iris_agentic_dev_core::tools::write_gate::classify("iris_admin", Some(&args));
    assert_eq!(
        class,
        Some(WriteClass::Destructive),
        "iris_admin action=mirror_failover must classify as WriteClass::Destructive, got {:?}",
        class
    );
}

// ── spec 141 R1 D3: an elicitation resume is judged on what its pending entry recorded ──────────

mod elicitation_resume_141 {
    use crate::support_gate_141::*;
    use iris_agentic_dev_core::elicitation::{ElicitationAction, Origin};
    use iris_agentic_dev_core::tools::write_gate::WriteClass;
    use serde_json::json;

    fn pend(
        rig: &Rig,
        tool: &str,
        target: Option<&str>,
        class: WriteClass,
        action: ElicitationAction,
    ) -> String {
        rig.tools.elicitation_store.insert_from(
            Origin {
                tool: tool.to_string(),
                target_key: target.map(str::to_string),
                class,
            },
            "Sentinel141.cls",
            action,
            Some("Class Sentinel141 {}".to_string()),
            Some("1".to_string()),
            "USER",
        )
    }

    async fn resume_doc(rig: &Rig, id: &str, server: Option<&str>) -> serde_json::Value {
        let mut args = json!({"elicitation_id": id, "elicitation_answer": "yes"});
        if let Some(s) = server {
            args["server"] = json!(s);
        }
        let r = rig.tools.call_gated("iris_doc", args).await;
        match r {
            Ok(r) => body_of(&r),
            Err(e) => json!({"error": e}),
        }
    }

    #[tokio::test]
    async fn a_put_pending_on_dev_cannot_be_finished_on_prod() {
        let rig = rig(PROD_LOCKED_WRITES_DECLARED);
        let id = pend(
            &rig,
            "iris_doc",
            Some("dev"),
            WriteClass::Write,
            ElicitationAction::Put,
        );
        let body = resume_doc(&rig, &id, Some("prod")).await;
        assert_eq!(
            code_of(&body),
            "ELICITATION_SERVER_MISMATCH",
            "the resume names another server than the call that opened the dialog: {body}"
        );
        assert!(
            rig.tools.elicitation_store.lookup(&id).is_some(),
            "a refused resume must leave the entry for the server it belongs to"
        );
    }

    #[tokio::test]
    async fn a_resume_that_names_no_server_does_not_fall_back_to_the_default() {
        let rig = rig(PROD_LOCKED_WRITES_DECLARED);
        let id = pend(
            &rig,
            "iris_doc",
            Some("prod"),
            WriteClass::Write,
            ElicitationAction::Put,
        );
        let body = resume_doc(&rig, &id, None).await;
        assert_eq!(code_of(&body), "ELICITATION_SERVER_MISMATCH", "{body}");
    }

    #[tokio::test]
    async fn a_resume_carries_no_mode_and_is_classified_as_the_pending_entrys_write() {
        // Writes are off on the default. A resume has no `mode`, so it classified as a read and
        // went through; it finishes a write, and the write gate must say so.
        let rig = rig("write_tools_enabled = false\n");
        let id = pend(
            &rig,
            "iris_doc",
            None,
            WriteClass::Write,
            ElicitationAction::Put,
        );
        let body = resume_doc(&rig, &id, None).await;
        assert_eq!(code_of(&body), "WRITE_TOOLS_DISABLED", "{body}");
    }

    #[tokio::test]
    async fn an_scm_id_cannot_be_answered_through_iris_doc_and_the_reverse() {
        let rig = rig("");
        let scm = pend(
            &rig,
            "iris_source_control",
            None,
            WriteClass::Write,
            ElicitationAction::ScmExecute,
        );
        let body = resume_doc(&rig, &scm, None).await;
        assert_eq!(code_of(&body), "ELICITATION_NOT_FOUND", "{body}");

        let doc = pend(
            &rig,
            "iris_doc",
            None,
            WriteClass::Write,
            ElicitationAction::Put,
        );
        let r = rig
            .tools
            .call_gated(
                "iris_source_control",
                json!({"elicitation_id": doc, "answer": "yes"}),
            )
            .await
            .map(|r| body_of(&r))
            .unwrap_or_else(|e| json!({"error": e}));
        assert_eq!(code_of(&r), "ELICITATION_NOT_FOUND", "{r}");
    }
}

// ── 141 C14: the scratch column ────────────────────────────────────────────────────────────────

/// Replace comments and the inside of string and char literals with nothing, so a call graph read
/// off the text sees code only. A raw string such as the ObjectScript in `info.rs` holds `//`
/// comments and quotes that would otherwise end the scan early.
fn code_only(src: &str) -> String {
    let b: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        let next = b.get(i + 1).copied();
        if c == '/' && next == Some('/') {
            while i < b.len() && b[i] != '\n' {
                i += 1;
            }
        } else if c == '/' && next == Some('*') {
            i += 2;
            while i + 1 < b.len() && !(b[i] == '*' && b[i + 1] == '/') {
                i += 1;
            }
            i += 2;
        } else if c == 'r' && matches!(next, Some('"') | Some('#')) && {
            let mut j = i + 1;
            while b.get(j) == Some(&'#') {
                j += 1;
            }
            b.get(j) == Some(&'"')
        } {
            let mut hashes = 0;
            let mut j = i + 1;
            while b[j] == '#' {
                hashes += 1;
                j += 1;
            }
            j += 1;
            let close: Vec<char> = std::iter::once('"')
                .chain(std::iter::repeat_n('#', hashes))
                .collect();
            while j + close.len() <= b.len() && b[j..j + close.len()] != close[..] {
                j += 1;
            }
            i = j + close.len();
            out.push_str("\"\"");
        } else if c == '"' {
            i += 1;
            while i < b.len() && b[i] != '"' {
                i += if b[i] == '\\' { 2 } else { 1 };
            }
            i += 1;
            out.push_str("\"\"");
        } else if c == '\'' && next == Some('\\') {
            i += 2;
            while i < b.len() && b[i] != '\'' {
                i += 1;
            }
            i += 1;
        } else if c == '\'' && b.get(i + 2) == Some(&'\'') {
            i += 3;
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}

fn rust_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

/// Calls the scan does not follow. Each reaches the scratch class on its own account, and each has
/// its own answer: telemetry's IRIS sink stops when the target's writes are off (C16), and the
/// `%SYS.Audit` emission is opt-in per server (`irisAudit`). Following them would put every tool in
/// the column.
const NOT_FOLLOWED: &[&str] = &[
    "record_call",
    "write_durable",
    "read_durable",
    "write_audit_entry",
    "write_audit_entry_on",
    "audit_call",
    // Names that appear in many unrelated types. The scan reads names, not types, so a call to
    // `.get(` would join every function called `get`.
    // `run` is not here: `ExecContext::run` is the one function of that name, and it is how every
    // tool that runs the agent's code reaches the generator (spec 141, R2).
    "new",
    "get",
    "from",
    "into",
    "default",
    "fmt",
    "clone",
    "drop",
    "len",
    "is_empty",
    "contains",
    "insert",
    "push",
    "map",
    "read",
    "write",
    "flush",
    "start",
    "stop",
    "list",
    "error",
];

/// The tools in `CLASSIFICATION` whose handler can reach `execute_via_generator`, found by reading
/// the source: a call graph over function names, from each tool method to any function (outside the
/// connection module that defines it) that calls the generator.
fn tools_that_reach_the_generator() -> std::collections::BTreeSet<String> {
    use std::collections::{BTreeMap, BTreeSet};
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_files(&root, &mut files);

    let def = regex::Regex::new(r"\bfn\s+([a-z_][A-Za-z0-9_]*)").unwrap();
    let call = regex::Regex::new(r"\b([a-z_][A-Za-z0-9_]*)\s*(?:::<[^>]*>)?\(").unwrap();
    // name -> (calls, reaches the generator itself)
    let mut graph: BTreeMap<String, (BTreeSet<String>, bool)> = BTreeMap::new();
    for path in &files {
        let is_connection = path.ends_with("iris/connection.rs");
        let text = code_only(&std::fs::read_to_string(path).unwrap());
        for m in def.captures_iter(&text) {
            let whole = m.get(0).unwrap();
            let name = m[1].to_string();
            // The body starts at the first `{` after the signature; a `;` first means no body.
            let rest = &text[whole.end()..];
            let (Some(open), semi) = (rest.find('{'), rest.find(';')) else {
                continue;
            };
            if semi.is_some_and(|s| s < open) {
                continue;
            }
            let mut depth = 0usize;
            let mut end = rest.len();
            for (k, ch) in rest[open..].char_indices() {
                match ch {
                    '{' => depth += 1,
                    '}' => {
                        depth -= 1;
                        if depth == 0 {
                            end = open + k;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            let body = &rest[open..end];
            let entry = graph.entry(name.clone()).or_default();
            if !is_connection && body.contains("execute_via_generator") {
                entry.1 = true;
            }
            for c in call.captures_iter(body) {
                let callee = c[1].to_string();
                if callee != name && !NOT_FOLLOWED.contains(&callee.as_str()) {
                    entry.0.insert(callee);
                }
            }
        }
    }

    let mut reaching: BTreeSet<String> = graph
        .iter()
        .filter(|(_, (_, direct))| *direct)
        .map(|(n, _)| n.clone())
        .collect();
    loop {
        let before = reaching.len();
        for (name, (calls, _)) in &graph {
            if !reaching.contains(name) && calls.iter().any(|c| reaching.contains(c)) {
                reaching.insert(name.clone());
            }
        }
        if reaching.len() == before {
            break;
        }
    }
    CLASSIFICATION
        .iter()
        .map(|e| e.tool.to_string())
        .filter(|t| reaching.contains(t))
        .collect()
}

/// Tools that reach the scratch class only through a branch the write gate already governs: a put
/// that runs a source-control hook, `iris_query` with `mode = "write"` or `force`, and the
/// documented put. A read call never compiles a class, so strict read-only leaves them
/// listed.
const SCRATCH_ONLY_ON_WRITE_BRANCHES: &[&str] = &["iris_doc", "iris_query"];

/// Local tools that reach the generator through the skill and knowledge store on the default
/// connection (`skills_tools::xecute`). They act on no target server, so the column leaves them
/// out: marking them would hide `skill` and `kb`, and with them the guardrail skills, from an
/// operator who asked for no scratch classes on a production server. They still obey the write
/// gate (`kb` and `skill` writes are Write).
const LOCAL_STORE_TOOLS: &[&str] = &["agent_info", "kb", "kb_index", "skill", "skill_community"];

/// T135 (C14). The scratch column agrees with the source: every tool whose handler can reach
/// `execute_via_generator` is marked, and no other tool is. `readOnlyHint` is a separate promise
/// and stays where it is.
#[test]
fn scratch_column_matches_generator_callers() {
    let reaching = tools_that_reach_the_generator();
    assert!(
        reaching.len() > 25
            && reaching.contains("iris_table_info")
            && reaching.contains("iris_execute"),
        "the scan found {} tools; it is reading the wrong thing: {reaching:?}",
        reaching.len()
    );
    for t in SCRATCH_ONLY_ON_WRITE_BRANCHES {
        assert!(
            reaching.contains(*t),
            "{t} is excused as write-branch-only but the scan no longer finds it reaching the \
             generator; drop it from SCRATCH_ONLY_ON_WRITE_BRANCHES"
        );
    }
    let local: Vec<&str> = reaching
        .iter()
        .map(String::as_str)
        .filter(|t| tool_class(t).is_some_and(|e| e.kind == TargetKind::Local))
        .collect();
    assert_eq!(
        local, LOCAL_STORE_TOOLS,
        "the local tools that reach the generator changed; LOCAL_STORE_TOOLS says which ones the \
         column leaves out and why"
    );
    let expected: std::collections::BTreeSet<String> = reaching
        .into_iter()
        .filter(|t| !SCRATCH_ONLY_ON_WRITE_BRANCHES.contains(&t.as_str()))
        .filter(|t| !LOCAL_STORE_TOOLS.contains(&t.as_str()))
        .collect();
    let marked: std::collections::BTreeSet<String> = CLASSIFICATION
        .iter()
        .filter(|e| e.scratch)
        .map(|e| e.tool.to_string())
        .collect();
    let unmarked: Vec<_> = expected.difference(&marked).collect();
    let spurious: Vec<_> = marked.difference(&expected).collect();
    assert!(
        unmarked.is_empty() && spurious.is_empty(),
        "the scratch column and the source disagree. Reach the generator but not marked: \
         {unmarked:?}. Marked but nothing reaches the generator: {spurious:?}. Mark a tool with \
         `.scratch()` in write_gate::CLASSIFICATION."
    );
}

/// `readOnlyHint` stays `true` on the scratch tools that read (R5 C14): flipping about 18 hints
/// would make clients ask for approval on routine reads.
#[test]
fn scratch_tools_that_read_keep_their_read_only_hint() {
    let tools = tools_with_declared(Toolset::Merged, WRITES_OFF);
    for name in [
        "iris_table_info",
        "global_preview",
        "my_access",
        "hl7_schema_list",
    ] {
        let ann = tools
            .tool_annotations(name)
            .unwrap_or_else(|| panic!("{name} has no annotations"));
        assert_eq!(
            ann.get("readOnlyHint").and_then(|v| v.as_bool()),
            Some(true),
            "{name} must keep readOnlyHint = true"
        );
        assert!(
            CLASSIFICATION.iter().any(|e| e.tool == name && e.scratch),
            "{name} is a scratch-class tool"
        );
    }
}

/// T135 (C18). Replacing a whole lookup table is at least as destructive as deleting one row, and
/// `iris_lookup_manage` already puts a single-row `set` in the destructive tier.
#[test]
fn lookup_import_is_destructive() {
    use iris_agentic_dev_core::tools::write_gate::classify;
    for key in ["action", "mode"] {
        let mut m = serde_json::Map::new();
        m.insert(key.to_string(), serde_json::json!("import"));
        assert_eq!(
            classify("iris_lookup_transfer", Some(&m)),
            Some(WriteClass::Destructive),
            "import ({key}) replaces a non-empty table"
        );
    }
    assert_eq!(
        classify("iris_lookup_transfer", Some(&args_with_action("export"))),
        Some(WriteClass::ReadOnly)
    );
    assert_eq!(
        classify("iris_lookup_manage", Some(&args_with_action("set"))),
        Some(WriteClass::Destructive)
    );
}
