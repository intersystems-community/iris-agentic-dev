//! Spec 141 T089 (R4a D9): the compile-time gate's call sites read the switch of the server the
//! call names, through the target.
//!
//! `prod` is declared with `compileTimeGate = false` and `stage` is not. The same generator file
//! goes to each: the one whose gate is on is refused with `COMPILE_TIME_EXEC_BLOCKED`; the one
//! whose gate is off is not, and its answer is whatever the unreachable member produces. The
//! `iris_doc` put checks an SCM status over the wire before it gets to the gate, and the response
//! that carries `compile_time_gate: "off"` needs a stored document, so both are in
//! `test_compile_gate_141_live`.

use crate::support_gate_141::*;
use iris_agentic_dev_core::tools::Toolset;
use serde_json::json;

const GENERATOR: &str = "Class Gate141.Gen Extends %RegisteredObject\n{\n\nClassMethod G() As %Status [ CodeMode = objectgenerator ]\n{\n    Quit $$$OK\n}\n\n}\n";

/// Not the docker-only marker (port 1), so a call that gets past the gate takes the HTTP path and
/// fails there.
const HTTP_NOBODY: &str = "http://127.0.0.1:3";

const OPERATOR: &str = r#"
mode = "operate"

[instance.prod]
host = "127.0.0.1"
web_port = 1

[policy.prod]
mcpTemplate = "dev"
writeTools = true
compileTimeGate = false

[instance.stage]
host = "127.0.0.1"
web_port = 1

[policy.stage]
mcpTemplate = "dev"
writeTools = true
"#;

#[tokio::test]
async fn a_local_file_given_to_iris_compile_is_judged_by_the_named_servers_switch() {
    let rig = rig_with(
        OPERATOR,
        Toolset::Merged,
        &[("prod", HTTP_NOBODY), ("stage", HTTP_NOBODY)],
    );
    // Inside the project directory the rig works in (T212).
    let path = rig.dir.path().join("proj").join("Gen141.cls");
    std::fs::write(&path, GENERATOR).unwrap();
    let target = path.to_string_lossy().to_string();
    let mut codes = Vec::new();
    for server in ["stage", "prod"] {
        let answer = rig
            .tools
            .call_gated("iris_compile", json!({"target": target, "server": server}))
            .await;
        codes.push(match answer {
            Ok(r) => code_of(&body_of(&r)),
            Err(e) => format!("error: {e}"),
        });
    }
    assert_eq!(codes[0], "COMPILE_TIME_EXEC_BLOCKED", "stage: {codes:?}");
    assert_ne!(codes[1], "COMPILE_TIME_EXEC_BLOCKED", "prod: {codes:?}");
}
