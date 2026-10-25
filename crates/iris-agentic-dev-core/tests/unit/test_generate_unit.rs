// Unit tests for generate.rs — pure functions and constants.
// No IRIS connection, no network required.

use iris_agentic_dev_core::generate::{
    extract_class_name, validate_cls_syntax, GENERATE_CLASS_SYSTEM, GENERATE_TEST_SYSTEM,
    RETRY_TEMPLATE,
};
use iris_agentic_dev_core::testing::EnvGuard;

// ── validate_cls_syntax ──────────────────────────────────────────────────────

#[test]
fn test_validate_cls_syntax_valid_class() {
    let cls = "Class MyApp.Foo { ClassMethod Run() { } }";
    assert!(
        validate_cls_syntax(cls),
        "valid class should pass syntax check"
    );
}

#[test]
fn test_validate_cls_syntax_empty_fails() {
    assert!(!validate_cls_syntax(""), "empty string should fail");
}

#[test]
fn test_validate_cls_syntax_not_a_class() {
    assert!(
        !validate_cls_syntax("function foo() { return 1; }"),
        "non-ObjectScript should fail — missing 'Class '"
    );
}

#[test]
fn test_validate_cls_syntax_minimal_class() {
    assert!(
        validate_cls_syntax("Class Foo { }"),
        "minimal Class {{}} should pass"
    );
}

#[test]
fn test_validate_cls_syntax_missing_class_keyword() {
    // Has braces but no Class keyword
    assert!(
        !validate_cls_syntax("{ something }"),
        "no Class keyword fails"
    );
}

#[test]
fn test_validate_cls_syntax_unbalanced_open_brace() {
    // Extra open brace means counts don't match
    assert!(
        !validate_cls_syntax("Class Foo { Method Bar() { }"),
        "unbalanced braces should fail"
    );
}

#[test]
fn test_validate_cls_syntax_unbalanced_close_brace() {
    assert!(
        !validate_cls_syntax("Class Foo { } }"),
        "extra close brace should fail"
    );
}

#[test]
fn test_validate_cls_syntax_class_keyword_case_sensitive() {
    // "class" lowercase should fail — ObjectScript uses "Class"
    assert!(
        !validate_cls_syntax("class Foo { }"),
        "lowercase 'class' should fail"
    );
}

#[test]
fn test_validate_cls_syntax_multiline_valid() {
    let cls = "Class MyApp.Patient Extends %Persistent {\n\nProperty Name As %String;\n\n}";
    assert!(validate_cls_syntax(cls), "multiline class should pass");
}

#[test]
fn test_validate_cls_syntax_with_extends() {
    let cls = "Class MyApp.Foo Extends %RegisteredObject { ClassMethod Test() { Quit 1 } }";
    assert!(validate_cls_syntax(cls));
}

#[test]
fn test_validate_cls_syntax_nested_braces_balanced() {
    let cls = "Class Foo { ClassMethod Bar() { if 1 { do something } } }";
    assert!(
        validate_cls_syntax(cls),
        "balanced nested braces should pass"
    );
}

#[test]
fn test_validate_cls_syntax_no_braces() {
    assert!(
        !validate_cls_syntax("Class Foo"),
        "Class with no braces should fail"
    );
}

// ── extract_class_name (additional cases beyond test_tools_fixes.rs) ─────────

#[test]
fn test_extract_class_name_with_extends() {
    assert_eq!(
        extract_class_name("Class Ens.Director Extends %RegisteredObject { }"),
        Some("Ens.Director".to_string())
    );
}

#[test]
fn test_extract_class_name_multiline() {
    let cls = "/// Some doc\nClass MyPkg.MyClass [\n  ClassType = persistent\n] {\n}";
    assert_eq!(extract_class_name(cls), Some("MyPkg.MyClass".to_string()));
}

#[test]
fn test_extract_class_name_deep_package() {
    assert_eq!(
        extract_class_name("Class A.B.C.D { }"),
        Some("A.B.C.D".to_string())
    );
}

#[test]
fn test_extract_class_name_single_segment() {
    assert_eq!(extract_class_name("Class Foo { }"), Some("Foo".to_string()));
}

#[test]
fn test_extract_class_name_no_class_line() {
    assert_eq!(extract_class_name("// just a comment\n{ }"), None);
}

#[test]
fn test_extract_class_name_leading_whitespace() {
    assert_eq!(
        extract_class_name("  Class MyApp.Foo { }"),
        Some("MyApp.Foo".to_string())
    );
}

#[test]
fn test_extract_class_name_invalid_special_chars() {
    assert_eq!(extract_class_name("Class <Bad> {}"), None);
}

#[test]
fn test_extract_class_name_numeric_first_char() {
    // Class names starting with a digit are invalid ObjectScript
    assert_eq!(extract_class_name("Class 1Bad.Foo {}"), None);
}

#[test]
fn test_extract_class_name_hyphen_in_name() {
    // Hyphens are not valid in ObjectScript class names
    assert_eq!(extract_class_name("Class My-App.Foo {}"), None);
}

// ── Constants ────────────────────────────────────────────────────────────────

#[test]
fn test_generate_class_system_not_empty() {
    assert!(
        !GENERATE_CLASS_SYSTEM.is_empty(),
        "GENERATE_CLASS_SYSTEM must not be empty"
    );
    assert!(
        GENERATE_CLASS_SYSTEM.len() > 50,
        "GENERATE_CLASS_SYSTEM must be substantial (len={})",
        GENERATE_CLASS_SYSTEM.len()
    );
}

#[test]
fn test_generate_class_system_mentions_objectscript() {
    assert!(
        GENERATE_CLASS_SYSTEM.contains("ObjectScript") || GENERATE_CLASS_SYSTEM.contains("IRIS"),
        "system prompt should mention ObjectScript or IRIS"
    );
}

#[test]
fn test_generate_class_system_mentions_class_format() {
    assert!(
        GENERATE_CLASS_SYSTEM.contains("Class"),
        "class system prompt should mention Class keyword"
    );
}

#[test]
fn test_generate_test_system_not_empty() {
    assert!(
        !GENERATE_TEST_SYSTEM.is_empty(),
        "GENERATE_TEST_SYSTEM must not be empty"
    );
    assert!(
        GENERATE_TEST_SYSTEM.len() > 50,
        "GENERATE_TEST_SYSTEM must be substantial (len={})",
        GENERATE_TEST_SYSTEM.len()
    );
}

#[test]
fn test_generate_test_system_mentions_unittest() {
    assert!(
        GENERATE_TEST_SYSTEM.contains("UnitTest") || GENERATE_TEST_SYSTEM.contains("TestCase"),
        "test system prompt should mention UnitTest or TestCase"
    );
}

#[test]
fn test_retry_template_not_empty() {
    assert!(
        !RETRY_TEMPLATE.is_empty(),
        "RETRY_TEMPLATE must not be empty"
    );
}

#[test]
fn test_retry_template_contains_errors_placeholder() {
    assert!(
        RETRY_TEMPLATE.contains("{errors}"),
        "RETRY_TEMPLATE must contain {{errors}} placeholder"
    );
}

// ── LlmClient::from_env ──────────────────────────────────────────────────────
// Serialize env-var–touching tests to prevent races between set/remove_var calls.
static LLM_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn test_llm_client_from_env_none_when_model_unset() {
    let mut env_guard = EnvGuard::new();
    let _guard = LLM_ENV_LOCK.lock().unwrap();
    use iris_agentic_dev_core::generate::LlmClient;
    env_guard.unset("IRIS_GENERATE_CLASS_MODEL");
    // OPENAI_API_KEY may or may not be set — without the model var, must return None
    let client = LlmClient::from_env();
    assert!(
        client.is_none(),
        "should be None when IRIS_GENERATE_CLASS_MODEL is unset"
    );
}

#[test]
fn test_llm_client_from_env_none_when_api_key_unset() {
    let mut env_guard = EnvGuard::new();
    let _guard = LLM_ENV_LOCK.lock().unwrap();
    use iris_agentic_dev_core::generate::LlmClient;
    env_guard.set("IRIS_GENERATE_CLASS_MODEL", "gpt-4o");
    env_guard.unset("OPENAI_API_KEY");
    env_guard.unset("ANTHROPIC_API_KEY");
    let client = LlmClient::from_env();
    // If no API key is available, should return None
    // (model is set but key is missing)
    assert!(
        client.is_none(),
        "should be None when both OPENAI_API_KEY and ANTHROPIC_API_KEY are unset"
    );
}

#[test]
fn test_llm_client_from_env_some_when_both_set() {
    let mut env_guard = EnvGuard::new();
    let _guard = LLM_ENV_LOCK.lock().unwrap();
    use iris_agentic_dev_core::generate::LlmClient;
    env_guard.set("IRIS_GENERATE_CLASS_MODEL", "gpt-4o");
    env_guard.set("OPENAI_API_KEY", "sk-test-key");
    let client = LlmClient::from_env();
    assert!(
        client.is_some(),
        "should return Some when model and API key are set"
    );
}

#[test]
fn test_llm_client_from_env_anthropic_key_accepted() {
    let mut env_guard = EnvGuard::new();
    let _guard = LLM_ENV_LOCK.lock().unwrap();
    use iris_agentic_dev_core::generate::LlmClient;
    env_guard.set("IRIS_GENERATE_CLASS_MODEL", "claude-3-5-sonnet-20241022");
    env_guard.unset("OPENAI_API_KEY");
    env_guard.set("ANTHROPIC_API_KEY", "sk-ant-test-key");
    let client = LlmClient::from_env();
    assert!(
        client.is_some(),
        "should return Some when model and ANTHROPIC_API_KEY are set"
    );
}
