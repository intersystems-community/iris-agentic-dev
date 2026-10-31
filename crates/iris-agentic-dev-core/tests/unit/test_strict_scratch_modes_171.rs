//! Strict read-only refuses the modes of a read-only tool that compile a scratch class, and the
//! scratch-class runner itself refuses under strict whatever the classification says (1.7.1, S17).
//!
//! The live strict sweep found `iris_doc mode=compiled` putting `IrisDevTmp.IrisDevRun…` on the
//! server and compiling it under `read_only = "strict"`, with no error: `iris_doc` is a read-only
//! tool whose row carries no `.scratch()`, because only that one mode reads the INT routine through
//! a generated class. Strict judged the row, not the call.
//!
//! Nothing listens at the rig's addresses, so a call that got past every check fails on the network.

use crate::support_gate_141::*;
use iris_agentic_dev_core::tools::write_gate::is_scratch_call;
use iris_agentic_dev_core::tools::Toolset;
use serde_json::{json, Value};

const STRICT: &str = "write_tools_enabled = true\nread_only = \"strict\"\n";

async fn call(rig: &Rig, tool: &str, args: Value) -> Value {
    match rig.tools.call_gated(tool, args).await {
        Ok(r) => {
            let mut b = body_of(&r);
            b["is_error"] = json!(r.is_error == Some(true));
            b
        }
        Err(e) => json!({"error": e, "went_past_the_gates": true}),
    }
}

fn rigs() -> Vec<Rig> {
    [Toolset::Baseline, Toolset::Merged]
        .into_iter()
        .map(|t| rig_with_skills(STRICT, t, &[("prod", UNROUTABLE)], false))
        .collect()
}

#[test]
fn the_compiled_mode_is_a_scratch_call() {
    for mode in ["compiled", "Compiled", " compiled "] {
        let args = json!({"mode": mode, "name": "%Library.String.cls"});
        let mut m = args.as_object().cloned().unwrap();
        iris_agentic_dev_core::tools::write_gate::normalize_dispatch_args(&mut m);
        assert!(is_scratch_call("iris_doc", Some(&m)), "{mode:?}");
    }
    for mode in ["get", "head", "list", "fragment"] {
        let m = json!({"mode": mode, "name": "%Library.String.cls"})
            .as_object()
            .cloned();
        assert!(!is_scratch_call("iris_doc", m.as_ref()), "{mode}");
    }
    // A row marked scratch is a scratch call whatever its arguments.
    assert!(is_scratch_call("iris_execute", None));
}

#[tokio::test]
async fn strict_refuses_iris_doc_compiled_in_both_spellings() {
    for rig in rigs() {
        for mode in ["compiled", " COMPILED "] {
            let b = call(
                &rig,
                "iris_doc",
                json!({"mode": mode, "name": "%Library.String.cls"}),
            )
            .await;
            assert_eq!(b["error_code"], "READ_ONLY_STRICT", "{mode:?}: {b}");
            assert_eq!(b["is_error"], true, "{mode:?}: {b}");
            assert_eq!(b["refusal"]["kind"], "operator_limit", "{mode:?}: {b}");
        }
    }
}

#[tokio::test]
async fn strict_leaves_the_other_iris_doc_reads() {
    for rig in rigs() {
        let b = call(
            &rig,
            "iris_doc",
            json!({"mode": "get", "name": "%Library.String.cls"}),
        )
        .await;
        assert_ne!(b["error_code"], "READ_ONLY_STRICT", "{b}");
    }
}

/// The backstop: with the per-call rule switched off, the call reaches the handler, and the scratch
/// class runner refuses before it sends anything. A request would have failed on the network.
#[tokio::test(flavor = "current_thread")]
async fn the_runner_refuses_under_strict_when_a_row_is_wrong() {
    let rig = rig_with_skills(STRICT, Toolset::Merged, &[("prod", UNROUTABLE)], false);
    iris_agentic_dev_core::testing::ignore_scratch_call_rule(true);
    let b = call(
        &rig,
        "iris_doc",
        json!({"mode": "compiled", "name": "%Library.String.cls"}),
    )
    .await;
    iris_agentic_dev_core::testing::ignore_scratch_call_rule(false);
    let text = b.to_string();
    assert!(text.contains("READ_ONLY_STRICT"), "{b}");
    assert!(
        !text.contains("error sending request") && b["went_past_the_gates"].is_null(),
        "the runner sent a request under strict: {b}"
    );
}
