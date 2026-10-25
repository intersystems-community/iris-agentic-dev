//! The unit target's gate tests must not depend on the shell that launched them. The CI e2e job
//! exports `IRIS_WRITE_TOOLS_ENABLED=1` and `IRIS_DESTRUCTIVE_TOOLS_ENABLED=1`; the gate code keeps
//! the first reading of those for the life of the process, so the unit binary seeds an empty
//! operator environment before any test runs (`main.rs`).

use crate::support_gate_141::*;
use iris_agentic_dev_core::testing::EnvGuard;
use iris_agentic_dev_core::tools::write_gate::{operator_env_gates, OperatorEnvGates};
use serde_json::json;

#[tokio::test]
async fn a_shell_that_exports_both_gates_open_does_not_open_the_rig() {
    let mut env = EnvGuard::clean();
    env.set("IRIS_WRITE_TOOLS_ENABLED", "1")
        .set("IRIS_DESTRUCTIVE_TOOLS_ENABLED", "1");

    assert_eq!(
        *operator_env_gates(),
        OperatorEnvGates::default(),
        "the operator snapshot must hold an empty environment, not the launching shell's"
    );

    let rig = rig("write_tools_enabled = false\n");
    let r = rig
        .tools
        .call_gated("iris_execute", json!({"code": "Write 1"}))
        .await
        .unwrap();
    assert_eq!(
        code_of(&body_of(&r)),
        "WRITE_TOOLS_DISABLED",
        "writes are declared off in the file and the rig must see them closed: {:?}",
        body_of(&r)
    );
}
