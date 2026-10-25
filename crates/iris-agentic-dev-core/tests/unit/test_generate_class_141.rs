//! Spec 141 T211: `iris_generate_class` stores the text the model wrote and compiles that, by the
//! path `iris_doc` put with `compile` takes.
//!
//! It used to compile whatever class already had the name the model wrote, so a description that
//! steered the name ran an existing class's generators with no compile-time gate, no stored-source
//! check and no superclass check, and never saved the generated text.
//!
//! The model is a local HTTP stub that answers like the OpenAI API (the only thing stubbed here;
//! IRIS is never faked). The default connection is `http://127.0.0.1:2`, where nothing listens, so a
//! refusal that came from the gate is told apart from a request that reached the network: the
//! second is a connection error, and a connection error is never an accepted answer.
//! The live half is `tests/integration/test_generate_class_141_live.rs`.

use crate::support_gate_141::{body_of, code_of, rig_with};
use iris_agentic_dev_core::testing::EnvGuard;
use iris_agentic_dev_core::tools::Toolset;
use serde_json::{json, Value};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// A class the gate refuses: a generator runs while the class compiles.
const WITH_GENERATOR: &str = "Class IadGen141.Gen Extends %RegisteredObject\n{\n\n\
ClassMethod G() As %String [ CodeMode = objectgenerator ]\n{\n    \
Do %code.WriteLine(\"    Quit 1\")\n    Quit $$$OK\n}\n\n}\n";

/// The same for the other construct the gate names first: a preprocessor `#execute`.
const WITH_HASH_EXECUTE: &str = "Class IadGen141.Hash Extends %RegisteredObject\n{\n\n\
ClassMethod M() As %String\n{\n    #execute Set ^IadGen141Marker = 1\n    Quit 1\n}\n\n}\n";

fn llm_env(base_url: &str) -> EnvGuard {
    let mut g = EnvGuard::new();
    g.set("IRIS_GENERATE_CLASS_MODEL", "gpt-4o");
    g.set("OPENAI_API_KEY", "sk-test-key");
    g.unset("ANTHROPIC_API_KEY");
    g.set("OPENAI_BASE_URL", base_url);
    g
}

/// A stub model that answers every request with `text`.
async fn stub_llm(text: &str) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{"message": {"role": "assistant", "content": text}}],
        })))
        .mount(&server)
        .await;
    server
}

fn assert_gate_refusal(label: &str, b: &Value, matched: &str) {
    assert_eq!(
        code_of(b),
        "COMPILE_TIME_EXEC_BLOCKED",
        "{label}: the compile-time gate must refuse the generated text. Body: {b}"
    );
    assert!(
        b["matched"].as_str().unwrap_or("").contains(matched),
        "{label}: the refusal names the construct ({matched}). Body: {b}"
    );
    assert_ne!(
        code_of(b),
        "IRIS_UNREACHABLE",
        "{label}: reached the network: {b}"
    );
}

#[tokio::test]
async fn a_generated_text_with_a_generator_is_refused_before_anything_is_sent() {
    let llm = stub_llm(WITH_GENERATOR).await;
    let _env = llm_env(&llm.uri());
    let rig = rig_with("", Toolset::Merged, &[]);
    // `server` plain and padded, and no `server` at all: the gate reads the one the pool resolves.
    for server in [None, Some("dev"), Some(" dev ")] {
        let mut args = json!({"description": "a class", "namespace": "USER"});
        if let Some(s) = server {
            args["server"] = json!(s);
        }
        let r = rig
            .tools
            .call_gated("iris_generate_class", args)
            .await
            .unwrap();
        assert_eq!(
            r.is_error,
            Some(true),
            "server={server:?}: a refusal is an error result"
        );
        assert_gate_refusal(
            &format!("server={server:?}"),
            &body_of(&r),
            "objectgenerator",
        );
    }
}

#[tokio::test]
async fn a_hash_execute_in_the_generated_text_is_refused_the_same_way() {
    let llm = stub_llm(WITH_HASH_EXECUTE).await;
    let _env = llm_env(&llm.uri());
    let rig = rig_with("", Toolset::Merged, &[]);
    let r = rig
        .tools
        .call_gated("iris_generate_class", json!({"description": "a class"}))
        .await
        .unwrap();
    assert_eq!(r.is_error, Some(true));
    assert_gate_refusal("#execute", &body_of(&r), "#execute");
}

#[tokio::test]
async fn the_refusal_does_not_become_compiled_false() {
    // The old shape: `success: true, compiled: false` with the reason lost.
    let llm = stub_llm(WITH_GENERATOR).await;
    let _env = llm_env(&llm.uri());
    let rig = rig_with("", Toolset::Merged, &[]);
    let r = rig
        .tools
        .call_gated("iris_generate_class", json!({"description": "a class"}))
        .await
        .unwrap();
    let b = body_of(&r);
    assert_ne!(b["success"], json!(true), "{b}");
    assert!(
        b.get("compiled").is_none(),
        "a refusal is not a compile result: {b}"
    );
}

#[tokio::test]
async fn without_an_llm_the_tool_says_so_and_touches_nothing() {
    let mut env = EnvGuard::new();
    env.unset("IRIS_GENERATE_CLASS_MODEL");
    env.unset("OPENAI_API_KEY");
    env.unset("ANTHROPIC_API_KEY");
    let rig = rig_with("", Toolset::Merged, &[]);
    let answer = rig
        .tools
        .call_gated("iris_generate_class", json!({"description": "a class"}))
        .await;
    let text = match answer {
        Err(e) => e,
        Ok(r) => body_of(&r).to_string(),
    };
    assert!(text.contains("LLM_UNAVAILABLE"), "{text}");
    assert!(
        !text.contains("IRIS_UNREACHABLE") && !text.contains("error sending request"),
        "no request may have been sent: {text}"
    );
}

#[tokio::test]
async fn a_text_with_no_class_name_is_invalid_output_and_stores_nothing() {
    let llm = stub_llm("Class {\n}\n").await;
    let _env = llm_env(&llm.uri());
    let rig = rig_with("", Toolset::Merged, &[]);
    let r = rig
        .tools
        .call_gated("iris_generate_class", json!({"description": "a class"}))
        .await
        .unwrap();
    let b = body_of(&r);
    assert_eq!(code_of(&b), "INVALID_OUTPUT", "{b}");
    assert_eq!(r.is_error, Some(true), "{b}");
}

/// The tool is a write, so a server on the `live` or `test` template refuses it before the model
/// is asked anything, and the refusal comes from the gate rather than from a refused connection.
#[tokio::test]
async fn live_and_test_templates_refuse_the_tool_before_the_model_is_called() {
    use crate::support_gate_141::{PROD_LOCKED, PROD_LOCKED_WRITES_DECLARED};
    let llm = stub_llm(WITH_GENERATOR).await;
    let _env = llm_env(&llm.uri());
    let test_template = PROD_LOCKED_WRITES_DECLARED.replace("\"live\"", "\"test\"");
    for (label, toml) in [
        ("live", PROD_LOCKED),
        ("live, writes declared", PROD_LOCKED_WRITES_DECLARED),
        ("test, writes declared", test_template.as_str()),
    ] {
        let rig = rig_with(toml, Toolset::Merged, &[("prod", "http://127.0.0.1:1")]);
        for server in ["prod", " prod "] {
            let r = rig
                .tools
                .call_gated(
                    "iris_generate_class",
                    json!({"description": "a class", "server": server}),
                )
                .await
                .unwrap();
            let b = body_of(&r);
            assert_eq!(r.is_error, Some(true), "{label}/{server:?}: {b}");
            let expected = if label == "live" {
                "WRITE_TOOLS_DISABLED"
            } else {
                "ENV_GATE_BLOCKED"
            };
            assert_eq!(code_of(&b), expected, "{label}/{server:?}: {b}");
            assert_ne!(code_of(&b), "IRIS_UNREACHABLE", "{label}/{server:?}: {b}");
        }
    }
    assert!(
        llm.received_requests().await.unwrap().is_empty(),
        "a refused call must not reach the model"
    );
}

// ── classify: what a put with `compile` came to ─────────────────────────────────────────────────

mod classify_answers {
    use iris_agentic_dev_core::tools::generate_class::{classify, Stored};
    use rmcp::model::CallToolResult;
    use serde_json::{json, Value};

    fn ok(v: Value) -> CallToolResult {
        CallToolResult::structured(v)
    }

    #[test]
    fn a_compiled_put_is_compiled() {
        let r = ok(json!({"success": true, "compiled": true, "compile_errors": []}));
        assert!(matches!(classify(r), Stored::Compiled));
    }

    #[test]
    fn compile_errors_are_the_only_answer_worth_a_retry() {
        let r = ok(json!({
            "success": false, "compiled": false,
            "compile_errors": ["ERROR #5002: boom"],
        }));
        match classify(r) {
            Stored::CompileErrors(e) => assert_eq!(e, ["ERROR #5002: boom"]),
            other => panic!("expected CompileErrors, got {other:?}"),
        }
    }

    #[test]
    fn a_refusal_is_a_stop_and_keeps_its_error_flag() {
        let r = CallToolResult::structured_error(
            json!({"success": false, "error_code": "COMPILE_SUPERCLASS_STALE"}),
        );
        match classify(r) {
            Stored::Stopped(r) => assert_eq!(r.is_error, Some(true)),
            other => panic!("expected Stopped, got {other:?}"),
        }
    }

    #[test]
    fn a_checkout_prompt_is_a_stop_not_a_compile_failure() {
        let r = ok(json!({"success": false, "elicitation_required": true, "elicitation_id": "e1"}));
        assert!(matches!(classify(r), Stored::Stopped(_)));
    }

    #[test]
    fn a_failed_compile_with_no_errors_listed_is_a_stop() {
        // Nothing for the model to fix, so nothing to retry on.
        let r = ok(json!({"success": false, "compiled": false, "compile_errors": []}));
        assert!(matches!(classify(r), Stored::Stopped(_)));
    }
}

// ── the Atelier guard ───────────────────────────────────────────────────────────────────────────

/// Storing the text is an Atelier PUT, so a connection with no Atelier is told so before the model
/// is asked to do anything the tool cannot finish.
#[test]
fn a_docker_only_or_no_private_web_server_connection_has_no_atelier() {
    use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};
    use iris_agentic_dev_core::tools::nopws::atelier_unavailable;
    let conn = |url: &str, version: Option<&str>| {
        let mut c =
            IrisConnection::new(url, "USER", "_SYSTEM", "SYS", DiscoverySource::ExplicitFlag);
        c.version = version.map(str::to_string);
        c
    };
    assert!(atelier_unavailable(&conn("http://127.0.0.1:1", None)));
    assert!(atelier_unavailable(&conn("http://127.0.0.1:1/", None)));
    assert!(atelier_unavailable(&conn(
        "http://localhost:52773",
        Some("IRIS for UNIX 2026.2.0AI (Build 1)")
    )));
    assert!(!atelier_unavailable(&conn("http://localhost:52773", None)));
    assert!(!atelier_unavailable(&conn(
        "http://localhost:52773",
        Some("IRIS for UNIX 2026.2 (Build 1)")
    )));
}
