//! Spec 141 T205: `iris_source_control` with `action = "execute"` and the menu id `Diff` is a
//! read. Every other id, known or not, stays a write: the id goes to the source control provider
//! as typed, and the gate reads only the spellings that provider treats as `Diff`.
//!
//! The pipeline tests close the write gate and send the call down `call_gated`. A read gets past
//! the gate and fails at the network (nothing listens at the default connection); a write is
//! refused by the gate. Either way the answer comes from the gate, never from IRIS.

use crate::support_gate_141::*;
use iris_agentic_dev_core::tools::write_gate::{classify, WriteClass};
use serde_json::{json, Map, Value};

const WRITES_OFF: &str = "write_tools_enabled = false\n";

fn args(v: Value) -> Map<String, Value> {
    v.as_object().unwrap().clone()
}

fn class_of(action: &str, id: Option<&str>) -> WriteClass {
    let mut a = json!({"action": action, "document": "A.B.cls"});
    if let Some(id) = id {
        a["action_id"] = json!(id);
    }
    classify("iris_source_control", Some(&args(a))).unwrap()
}

#[test]
fn diff_is_a_read_in_both_spellings() {
    for id in ["Diff", "%Diff"] {
        assert_eq!(class_of("execute", Some(id)), WriteClass::ReadOnly, "{id}");
    }
}

#[test]
fn the_action_is_read_the_way_the_handler_reads_it() {
    for action in ["execute", "EXECUTE", " Execute "] {
        assert_eq!(
            class_of(action, Some("Diff")),
            WriteClass::ReadOnly,
            "action={action:?}"
        );
    }
}

#[test]
fn every_other_id_stays_a_write() {
    for id in [
        "CheckOut",
        "%CheckOut",
        "UndoCheckout",
        "CheckIn",
        "%CheckIn",
        "GetLatest",
        "AddToSourceControl",
        "Disconnect",
        "Reconnect",
        // Not the id the provider would call Diff.
        "Diffs",
        "Diff2",
        "DiffAll",
        "Diff,CheckIn",
        "Diff ",
        " Diff",
        "diff",
        "DIFF",
        "%%Diff",
        "",
        "NoSuchMenuItem",
    ] {
        assert_eq!(class_of("execute", Some(id)), WriteClass::Write, "{id:?}");
    }
    // No id at all.
    assert_eq!(class_of("execute", None), WriteClass::Write);
}

#[test]
fn the_id_makes_nothing_else_a_read() {
    // `action_id` only changes `execute`.
    for action in ["checkout", "unknown"] {
        assert_eq!(
            class_of(action, Some("Diff")),
            WriteClass::Write,
            "{action}"
        );
    }
    assert_eq!(class_of("status", Some("CheckIn")), WriteClass::ReadOnly);
    assert_eq!(class_of("menu", Some("CheckIn")), WriteClass::ReadOnly);
    // A non-string id is not Diff.
    let a = args(json!({"action": "execute", "action_id": ["Diff"]}));
    assert_eq!(
        classify("iris_source_control", Some(&a)).unwrap(),
        WriteClass::Write
    );
}

#[tokio::test]
async fn through_the_pipeline_diff_passes_the_closed_write_gate_and_the_rest_do_not() {
    let rig = rig(WRITES_OFF);
    let call = |id: &'static str| {
        let rig = &rig;
        async move {
            let r = rig
                .tools
                .call_gated(
                    "iris_source_control",
                    json!({"action": "execute", "action_id": id, "document": "A.B.cls"}),
                )
                .await;
            match r {
                Ok(r) => body_of(&r),
                Err(e) => json!({"error": e}),
            }
        }
    };
    for id in ["Diff", "%Diff"] {
        let b = call(id).await;
        assert_ne!(
            code_of(&b),
            "WRITE_TOOLS_DISABLED",
            "{id}: a read must get past the write gate. Body: {b}"
        );
    }
    for id in [
        "CheckIn",
        "Disconnect",
        "NoSuchMenuItem",
        "diff",
        "Diff,CheckIn",
        "",
    ] {
        let b = call(id).await;
        assert_eq!(
            code_of(&b),
            "WRITE_TOOLS_DISABLED",
            "{id:?}: the write gate must refuse it. Body: {b}"
        );
    }
}
