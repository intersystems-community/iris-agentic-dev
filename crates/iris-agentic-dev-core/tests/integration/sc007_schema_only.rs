//! SC-007: can a model call each converted tool with its description removed? (113 T037)
//!
//! Every other test in this feature checks that a schema says the right thing. This one checks that
//! what it says is enough. The tool's `description` is blanked in the listing the model reads, so the
//! only parameter documentation left is the schema: property names, types, enums, and per-property
//! descriptions. The model is given one plain-English request and asked for the arguments object.
//!
//! A call is judged against the schema rather than executed. Six of the thirty-one mutate state, and
//! `call_tool` decides acceptance from exactly the facts the schema carries — see
//! `benchmark::schema_tasks` for the whole argument.
//!
//! Run it:
//!
//! ```bash
//! IAD_SC007=1 IRIS_GENERATE_CLASS_MODEL=mlx-community/Qwen3-Coder-Next-4bit \
//! OPENAI_BASE_URL=http://localhost:8006 OPENAI_API_KEY=local \
//! IAD_BINARY=./target/debug/iris-agentic-dev \
//!   cargo test --features testing --test sc007_schema_only -- --include-ignored --test-threads=1 --nocapture
//! ```
//!
//! Pick a model that does not think out loud. `LlmClient::complete` reads
//! `choices[].message.content`, and a reasoning model (Qwen3.6-27B, for one) returns its answer in a
//! separate `reasoning` field, so every response arrives empty and lands in the results table as
//! "response carried no JSON object" — a formatting habit scored as a schema failure.
//!
//! The per-tool table lands in `target/sc007-results.json` and is transcribed into
//! `specs/113-typed-tool-schemas/lift-results.md`.

use iris_agentic_dev_core::benchmark::schema_tasks::{
    build_call_prompt, extract_call, load_schema_tasks, validate_call, CallVerdict,
    SchemaTaskResult,
};
use iris_agentic_dev_core::generate::LlmClient;
use iris_agentic_dev_core::testing::{advertised_tools_in_toolset_with, require_iad_binary};

const SYSTEM_PROMPT: &str = "You are an MCP client choosing arguments for a tool call. You are \
given the tool's JSON input schema and a user request. Reply with ONLY the JSON arguments object.";

#[tokio::test]
#[ignore = "calls an LLM and spawns the binary 31 times; run with --include-ignored"]
async fn every_converted_tool_is_callable_from_its_schema_alone() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    // A model server is not part of the test environment, so this one is opt-in: `--include-ignored`
    // in CI and under `cargo llvm-cov` would otherwise fail every run, and a permanently red gate is
    // a gate nobody reads. The measurement itself is not optional — it is recorded per tool in
    // specs/113-typed-tool-schemas/lift-results.md and must be re-run before any release that
    // changes the advertised surface.
    if std::env::var("IAD_SC007").ok().as_deref() != Some("1") {
        eprintln!(
            "SKIPPED SC-007: set IAD_SC007=1 together with IRIS_GENERATE_CLASS_MODEL, \
             OPENAI_BASE_URL and OPENAI_API_KEY. Last recorded result: 31 of 31 accepted — see \
             specs/113-typed-tool-schemas/lift-results.md."
        );
        return;
    }
    let client = LlmClient::from_env().expect(
        "IAD_SC007=1 asked for the SC-007 run, so a model is required: set \
         IRIS_GENERATE_CLASS_MODEL and OPENAI_API_KEY (OPENAI_BASE_URL for a local server). \
         Without one there is nothing to measure and a silent skip would read as a pass.",
    );
    let tasks = load_schema_tasks().expect("the embedded schema-call suite must parse");

    let mut results: Vec<SchemaTaskResult> = Vec::with_capacity(tasks.len());
    for task in &tasks {
        // One spawn per tool, with that tool's prose blanked and every other tool's left alone —
        // the model only ever sees the one schema, so the others do not matter, and suppressing all
        // 84 would make a failure impossible to attribute.
        let tools = advertised_tools_in_toolset_with(None, &[&task.tool]);
        let tool = tools
            .get(&task.tool)
            .unwrap_or_else(|| panic!("`{}` is not advertised", task.tool));
        assert!(
            tool.get("description")
                .and_then(|d| d.as_str())
                .is_none_or(str::is_empty),
            "`{}` still carries a description, so this run would measure the prose",
            task.tool
        );
        let schema = tool
            .get("inputSchema")
            .unwrap_or_else(|| panic!("`{}` advertises no input schema", task.tool));

        let prompt = build_call_prompt(task, schema);
        let response = match client.complete(SYSTEM_PROMPT, &prompt).await {
            Ok(r) => r,
            Err(e) => {
                results.push(SchemaTaskResult {
                    tool: task.tool.clone(),
                    accepted: false,
                    call: serde_json::Value::Null,
                    reasons: vec![format!("llm call failed: {e}")],
                });
                continue;
            }
        };
        let Some(call) = extract_call(&response) else {
            results.push(SchemaTaskResult {
                tool: task.tool.clone(),
                accepted: false,
                call: serde_json::Value::String(response.clone()),
                reasons: vec!["response carried no JSON object".to_string()],
            });
            continue;
        };
        let verdict = validate_call(schema, &call, &task.expect_keys);
        let (accepted, reasons) = match verdict {
            CallVerdict::Accepted => (true, Vec::new()),
            CallVerdict::Rejected(rs) => (false, rs),
        };
        println!(
            "{:<28} {}{}",
            task.tool,
            if accepted { "accepted" } else { "REJECTED" },
            if accepted {
                String::new()
            } else {
                format!(" — {}", reasons.join("; "))
            }
        );
        results.push(SchemaTaskResult {
            tool: task.tool.clone(),
            accepted,
            call,
            reasons,
        });
    }

    let json = serde_json::to_string_pretty(&results).expect("results must serialize");
    // The test's working directory is the crate, not the workspace, so `target/` alone would land
    // in `crates/iris-agentic-dev-core/target/` — or nowhere, when that directory does not exist.
    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target")
        .join("sc007-results.json");
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir).expect("the target directory must exist to write the table");
    }
    std::fs::write(&out, &json).expect("must write the per-tool table");
    let passed = results.iter().filter(|r| r.accepted).count();
    println!(
        "\nSC-007: {passed}/{} accepted — table at {}",
        results.len(),
        out.display()
    );

    let failed: Vec<&SchemaTaskResult> = results.iter().filter(|r| !r.accepted).collect();
    assert!(
        failed.is_empty(),
        "these tools could not be called from their schema alone:\n{}",
        failed
            .iter()
            .map(|r| format!("  {} — {}", r.tool, r.reasons.join("; ")))
            .collect::<Vec<String>>()
            .join("\n")
    );
}
