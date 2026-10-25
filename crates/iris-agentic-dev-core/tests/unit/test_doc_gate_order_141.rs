//! Spec 141: `iris_doc` put runs the compile-time gate before it sends anything.
//!
//! The source-control probe is a request to the server (it compiles a scratch class), and it ran
//! before the gate saw the text, so a put the gate refuses still reached IRIS first. The default
//! connection here is `http://127.0.0.1:2`, where nothing listens: a refusal from the gate is told
//! apart from a request that reached the network, which fails as a probe or connection error.

use crate::support_gate_141::{body_of, code_of, rig_with};
use iris_agentic_dev_core::tools::Toolset;
use serde_json::json;

const WITH_GENERATOR: &str = "Class IadOrder141.Gen Extends %RegisteredObject\n{\n\n\
ClassMethod G() As %String [ CodeMode = objectgenerator ]\n{\n    \
Do %code.WriteLine(\"    Quit 1\")\n    Quit $$$OK\n}\n\n}\n";

#[tokio::test]
async fn a_put_the_gate_refuses_is_refused_before_the_source_control_probe() {
    let rig = rig_with("", Toolset::Merged, &[]);
    for compile in [false, true] {
        let r = rig
            .tools
            .call_gated(
                "iris_doc",
                json!({"mode": "put", "name": "IadOrder141.Gen.cls", "namespace": "USER",
                       "content": WITH_GENERATOR, "compile": compile}),
            )
            .await
            .unwrap();
        let b = body_of(&r);
        assert_eq!(r.is_error, Some(true), "compile={compile}: {b}");
        assert_eq!(
            code_of(&b),
            "COMPILE_TIME_EXEC_BLOCKED",
            "compile={compile}: the gate must answer before any request: {b}"
        );
    }
}
