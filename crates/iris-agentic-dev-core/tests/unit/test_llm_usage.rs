//! Tests for `LlmClient::complete_with_usage`.
//!
//! `complete` had wiremock coverage for all four of its outcomes. `complete_with_usage` is the
//! twin that the benchmark harness actually calls — it carries the token accounting that every
//! per-task budget is enforced against — and had none, so a wrong field name in `usage` would
//! have shown up as every task costing zero tokens.
//!
//! The mock server here stands in for an HTTP API on the public internet. Nothing about IRIS is
//! mocked or asserted.

use iris_agentic_dev_core::generate::LlmClient;
use std::sync::{Mutex, MutexGuard, OnceLock};
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const VARS: &[&str] = &[
    "IRIS_GENERATE_CLASS_MODEL",
    "OPENAI_API_KEY",
    "ANTHROPIC_API_KEY",
    "OPENAI_BASE_URL",
    "ANTHROPIC_BASE_URL",
    "IRIS_GENERATE_TIMEOUT",
];

/// `from_env` reads process-global variables, so the tests take turns and put back what they
/// found. A leaked `OPENAI_BASE_URL` would point a later test at a dead mock server.
struct EnvGuard {
    _lock: MutexGuard<'static, ()>,
    saved: Vec<(&'static str, Option<String>)>,
}

impl EnvGuard {
    fn new(model: &str, base_var: &str, base_url: &str) -> Self {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        let lock = LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let saved = VARS.iter().map(|k| (*k, std::env::var(k).ok())).collect();
        for k in VARS {
            std::env::remove_var(k);
        }
        std::env::set_var("IRIS_GENERATE_CLASS_MODEL", model);
        std::env::set_var("ANTHROPIC_API_KEY", "sk-test-key");
        std::env::set_var(base_var, base_url);
        EnvGuard { _lock: lock, saved }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (k, v) in &self.saved {
            match v {
                Some(val) => std::env::set_var(k, val),
                None => std::env::remove_var(k),
            }
        }
    }
}

#[tokio::test]
async fn anthropic_usage_is_read_from_input_and_output_tokens() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(header("anthropic-version", "2023-06-01"))
        .and(header("x-api-key", "sk-test-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "content": [{"text": "Class Generated.A Extends %RegisteredObject {}"}],
            "usage": {"input_tokens": 1234, "output_tokens": 56},
        })))
        .mount(&server)
        .await;

    let _g = EnvGuard::new("claude-3-5-sonnet", "ANTHROPIC_BASE_URL", &server.uri());
    let client = LlmClient::from_env().expect("client from env");
    let (text, usage) = client
        .complete_with_usage("system", "user")
        .await
        .expect("anthropic success path");

    assert!(text.contains("Class Generated.A"));
    let usage = usage.expect("Anthropic returns usage on every completion");
    assert_eq!(
        (usage.input, usage.output),
        (1234, 56),
        "input/output must map to input_tokens/output_tokens — swapping them silently \
         misreports every task budget"
    );
}

/// Anthropic omits `usage` on some streaming and cached responses, and the field is `#[serde
/// (default)]` for that reason. A missing field must be `None`, not a deserialization error.
#[tokio::test]
async fn a_response_without_usage_still_returns_the_text() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "content": [{"text": "no usage here"}],
        })))
        .mount(&server)
        .await;

    let _g = EnvGuard::new("claude-3-5-sonnet", "ANTHROPIC_BASE_URL", &server.uri());
    let client = LlmClient::from_env().expect("client from env");
    let (text, usage) = client
        .complete_with_usage("system", "user")
        .await
        .expect("a missing usage field is not an error");

    assert_eq!(text, "no usage here");
    assert!(usage.is_none());
}

#[tokio::test]
async fn an_anthropic_error_status_names_the_status_and_the_body() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(429).set_body_string(r#"{"error":"rate limited"}"#))
        .mount(&server)
        .await;

    let _g = EnvGuard::new("claude-3-5-sonnet", "ANTHROPIC_BASE_URL", &server.uri());
    let client = LlmClient::from_env().expect("client from env");
    let err = client
        .complete_with_usage("system", "user")
        .await
        .expect_err("429 must not be reported as a completion");

    let msg = format!("{err:?}");
    assert!(
        msg.contains("429"),
        "the status must be in the error: {msg}"
    );
    assert!(
        msg.contains("rate limited"),
        "the body carries the reason the caller needs: {msg}"
    );
}

/// An empty `content` array is a well-formed response with nothing in it. Returning `Ok("")`
/// would look to the caller like the model answered with an empty class.
#[tokio::test]
async fn an_empty_anthropic_content_array_is_an_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"content": []})))
        .mount(&server)
        .await;

    let _g = EnvGuard::new("claude-3-5-sonnet", "ANTHROPIC_BASE_URL", &server.uri());
    let client = LlmClient::from_env().expect("client from env");
    let err = client
        .complete_with_usage("system", "user")
        .await
        .expect_err("an empty content array must not read as a successful completion");
    assert!(
        format!("{err:?}").contains("empty"),
        "the error should say the response was empty: {err:?}"
    );
}

#[tokio::test]
async fn openai_usage_is_read_from_prompt_and_completion_tokens() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(header("authorization", "Bearer sk-test-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{"message": {"content": "Class Generated.O Extends %RegisteredObject {}"}}],
            "usage": {"prompt_tokens": 77, "completion_tokens": 8},
        })))
        .mount(&server)
        .await;

    let _g = EnvGuard::new("gpt-4o-mini", "OPENAI_BASE_URL", &server.uri());
    let client = LlmClient::from_env().expect("client from env");
    let (text, usage) = client
        .complete_with_usage("system", "user")
        .await
        .expect("openai success path");

    assert!(text.contains("Class Generated.O"));
    let usage = usage.expect("OpenAI returns usage on a non-streaming completion");
    assert_eq!((usage.input, usage.output), (77, 8));
}

#[tokio::test]
async fn an_openai_error_status_names_the_status() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(500).set_body_string("upstream exploded"))
        .mount(&server)
        .await;

    let _g = EnvGuard::new("gpt-4o-mini", "OPENAI_BASE_URL", &server.uri());
    let client = LlmClient::from_env().expect("client from env");
    let err = client
        .complete_with_usage("system", "user")
        .await
        .expect_err("500 must not be reported as a completion");

    let msg = format!("{err:?}");
    assert!(
        msg.contains("500"),
        "the status must be in the error: {msg}"
    );
    assert!(msg.contains("upstream exploded"), "body missing: {msg}");
}

#[tokio::test]
async fn an_empty_openai_choices_array_is_an_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"choices": []})))
        .mount(&server)
        .await;

    let _g = EnvGuard::new("gpt-4o-mini", "OPENAI_BASE_URL", &server.uri());
    let client = LlmClient::from_env().expect("client from env");
    assert!(
        client.complete_with_usage("system", "user").await.is_err(),
        "an empty choices array must not read as a successful completion"
    );
}

/// The mock model short-circuits before any HTTP client is built, and reports no usage because
/// nothing was spent. `IRIS_GENERATE_TIMEOUT` is read here too, so a non-numeric value must fall
/// back rather than refuse to build the client.
#[tokio::test]
async fn the_mock_model_returns_a_class_and_no_usage() {
    let _g = EnvGuard::new("mock", "OPENAI_BASE_URL", "http://127.0.0.1:1");
    std::env::set_var("IRIS_GENERATE_TIMEOUT", "not-a-number");

    let client = LlmClient::from_env().expect("a bad timeout must not stop the client");
    let (text, usage) = client
        .complete_with_usage("system", "user")
        .await
        .expect("the mock model never fails");

    assert!(text.contains("Generated.MockClass"));
    assert!(usage.is_none(), "nothing was spent, so there is no usage");
}
