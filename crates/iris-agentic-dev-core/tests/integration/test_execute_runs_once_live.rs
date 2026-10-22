//! Once the generator's query is sent, the code has run, or is running. 1.6.3 made a failed query
//! answer an error instead of an empty success, and `iris_execute` read that error as a broken
//! HTTP path and ran the code again through docker exec: 4 MB of output (`<MAXSTRING>` reading it
//! back) left a counter at 2 and answered success with no output. The retry loop could add three
//! more runs on top. Measured here with `IRIS_CONTAINER` set, so the docker fallback is there to
//! take.

use iris_agentic_dev_core::testing::{
    answer_text, live_container, live_env, require_iad_binary, McpSession,
};

fn session() -> McpSession {
    let mut env: Vec<(String, String)> = live_env()
        .into_iter()
        .filter(|(k, _)| k != "IRIS_CONTAINER")
        .collect();
    env.push(("IRIS_CONTAINER".into(), live_container()));
    env.push(("IRIS_WRITE_TOOLS_ENABLED".into(), "1".into()));
    env.push(("IRIS_DESTRUCTIVE_TOOLS_ENABLED".into(), "1".into()));
    let dir = std::env::temp_dir().join("iad-runs-once");
    std::fs::create_dir_all(&dir).unwrap();
    McpSession::start_in(Some(&dir), &env)
}

fn exec(s: &mut McpSession, code: &str) -> serde_json::Value {
    s.call(
        "iris_execute",
        &serde_json::json!({"code": code, "namespace": "USER"}),
    )
}

#[test]
#[ignore = "requires live IRIS, docker exec into its container, and the built binary"]
fn code_that_fails_after_it_ran_is_not_run_again() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let mut s = session();
    exec(&mut s, "Set ^IadRunsOnce=0");
    let answer = exec(
        &mut s,
        "Set ^IadRunsOnce=^IadRunsOnce+1 For i=1:1:40 Write $Justify(\"\",100000)",
    );
    let runs = answer_text(&exec(
        &mut s,
        "Write \"runs=\",^IadRunsOnce Kill ^IadRunsOnce",
    ));

    assert!(
        runs.contains("runs=1"),
        "the code ran more than once: {runs}"
    );
    let text = answer_text(&answer);
    assert_eq!(
        answer.pointer("/result/isError"),
        Some(&serde_json::Value::Bool(true)),
        "a run whose answer failed is not a success: {text}"
    );
    assert!(text.contains("QUERY_FAILED_AFTER_RUN"), "{text}");
    assert!(
        text.contains("MAXSTRING"),
        "IRIS's own reason is kept: {text}"
    );
}
