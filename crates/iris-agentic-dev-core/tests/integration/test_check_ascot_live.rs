//! `iris_check` against iris-dev-iris: a name the file does not define is resolved through iad's
//! connection, so a wrong member of a system class is an error rather than a hint.
//!
//! Run: `IRIS_HOST=localhost IRIS_WEB_PORT=52780 IRIS_USERNAME=_SYSTEM IRIS_PASSWORD=SYS
//! IAD_BINARY=$PWD/target/debug/iris-agentic-dev cargo test --features testing --test integration
//! test_check_ascot -- --include-ignored --test-threads=1`

use iris_agentic_dev_core::testing::{live_env, McpSession};

#[test]
#[ignore = "requires live IRIS"]
fn a_wrong_member_of_a_system_class_is_an_error() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("Live.CheckProbe.cls"),
        "Class Live.CheckProbe\n{\nClassMethod Run()\n{\n Write ##class(%SYSTEM.Version).GetVersion()\n Write ##class(%SYSTEM.Version).NoSuchMethod()\n}\n}\n",
    )
    .unwrap();
    let mut env = live_env();
    env.retain(|(k, _)| k != "IRIS_CONTAINER");
    env.push((
        "OBJECTSCRIPT_WORKSPACE".to_string(),
        dir.path().to_string_lossy().into_owned(),
    ));
    let answer = McpSession::start_in(Some(dir.path()), &env).call(
        "iris_check",
        &serde_json::json!({"path": "Live.CheckProbe.cls", "namespace": "USER"}),
    );
    let text = answer["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("no text content: {answer}"));
    let b: serde_json::Value = serde_json::from_str(text).expect("JSON body");
    assert_eq!(b["success"], true, "{b}");
    assert_eq!(b["iris"], true, "{b}");
    assert_eq!(b["errors"], 1, "{b}");
    assert_eq!(b["hints"], 0, "{b}");
    assert!(
        b["diagnostics"][0]["message"]
            .as_str()
            .unwrap_or("")
            .contains("NoSuchMethod"),
        "{b}"
    );
}
