use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

pub struct LlmClient {
    model: String,
    api_key: String,
    timeout_secs: u64,
}

// ── OpenAI request/response types ────────────────────────────────────────────

#[derive(Serialize)]
struct OpenAiRequest {
    model: String,
    messages: Vec<OpenAiMessage>,
}

#[derive(Serialize)]
struct OpenAiMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct OpenAiResponse {
    choices: Vec<OpenAiChoice>,
}

#[derive(Deserialize)]
struct OpenAiChoice {
    message: OpenAiResponseMessage,
}

#[derive(Deserialize)]
struct OpenAiResponseMessage {
    content: String,
}

// ── Anthropic request/response types ─────────────────────────────────────────

#[derive(Serialize)]
struct AnthropicRequest {
    model: String,
    max_tokens: u32,
    system: String,
    messages: Vec<AnthropicMessage>,
}

#[derive(Serialize)]
struct AnthropicMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct AnthropicResponse {
    content: Vec<AnthropicContent>,
}

#[derive(Deserialize)]
struct AnthropicContent {
    text: String,
}

// ─────────────────────────────────────────────────────────────────────────────

impl LlmClient {
    pub fn from_env() -> Option<Self> {
        let model = std::env::var("IRIS_GENERATE_CLASS_MODEL").ok()?;
        let api_key = std::env::var("OPENAI_API_KEY")
            .or_else(|_| std::env::var("ANTHROPIC_API_KEY"))
            .ok()?;
        let timeout_secs = std::env::var("IRIS_GENERATE_TIMEOUT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(60);
        Some(Self {
            model,
            api_key,
            timeout_secs,
        })
    }

    pub async fn complete(&self, system: &str, user: &str) -> Result<String> {
        #[cfg(any(test, feature = "testing"))]
        if self.model == "mock" {
            let _ = (system, user);
            return Ok(
                "Class Generated.MockClass Extends %RegisteredObject {\nMethod Hello() As %String { Quit \"hello\" }\n}".to_string(),
            );
        }

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(self.timeout_secs))
            .build()?;

        if self.model.starts_with("claude") {
            // Bug 6: Anthropic API requires:
            //   - x-api-key header (not Authorization: Bearer)
            //   - anthropic-version header
            //   - max_tokens field (required, causes 400 if absent)
            //   - system as top-level field (not in messages[])
            //   - response is content[].text (not choices[].message.content)
            let resp = client
                .post("https://api.anthropic.com/v1/messages")
                .header("x-api-key", &self.api_key)
                .header("anthropic-version", "2023-06-01")
                .header("content-type", "application/json")
                .json(&AnthropicRequest {
                    model: self.model.clone(),
                    max_tokens: 4096,
                    system: system.to_string(),
                    messages: vec![AnthropicMessage {
                        role: "user".to_string(),
                        content: user.to_string(),
                    }],
                })
                .send()
                .await
                .context("Anthropic API request failed")?;

            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                anyhow::bail!("Anthropic API error {}: {}", status, body);
            }

            let parsed: AnthropicResponse =
                resp.json().await.context("parsing Anthropic response")?;
            return parsed
                .content
                .into_iter()
                .next()
                .map(|c| c.text)
                .context("empty Anthropic response");
        }

        // OpenAI-compatible path
        let resp = client
            .post("https://api.openai.com/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&OpenAiRequest {
                model: self.model.clone(),
                messages: vec![
                    OpenAiMessage {
                        role: "system".to_string(),
                        content: system.to_string(),
                    },
                    OpenAiMessage {
                        role: "user".to_string(),
                        content: user.to_string(),
                    },
                ],
            })
            .send()
            .await
            .context("OpenAI API request failed")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("LLM API error {}: {}", status, body);
        }

        let parsed: OpenAiResponse = resp.json().await.context("parsing OpenAI response")?;
        parsed
            .choices
            .into_iter()
            .next()
            .map(|c| c.message.content)
            .context("empty LLM response")
    }
}

pub const GENERATE_CLASS_SYSTEM: &str = r#"You are an InterSystems ObjectScript expert. Generate a complete, compilable ObjectScript class in UDL format.

Rules:
- Start with: Class <ClassName> Extends <Superclass>
- Use { } for method bodies, NOT begin/end
- All methods must have a closing }
- The class block must end with a single }
- Return ONLY the class definition — no explanations, no markdown fences"#;

pub const GENERATE_TEST_SYSTEM: &str = r#"You are an InterSystems ObjectScript testing expert. Generate a complete %UnitTest.TestCase subclass in UDL format.

Rules:
- Extend %UnitTest.TestCase
- Test methods MUST start with "Test" prefix
- Use $$$AssertEquals, $$$AssertTrue, $$$AssertNotNull macros
- Return ONLY the test class definition — no explanations, no markdown fences"#;

pub const RETRY_TEMPLATE: &str = "The generated class failed to compile with these errors:\n\n{errors}\n\nPlease fix the ObjectScript class. Return ONLY the corrected class definition.";

pub fn validate_cls_syntax(text: &str) -> bool {
    text.contains("Class ")
        && text.contains('{')
        && text.matches('{').count() == text.matches('}').count()
}

pub fn extract_class_name(text: &str) -> Option<String> {
    let name = text
        .lines()
        .find(|l| l.trim_start().starts_with("Class "))
        .and_then(|l| l.split_whitespace().nth(1))
        .map(|s| s.to_string())?;

    // FR-016/Mo2: validate class name matches ObjectScript naming rules.
    // Must start with ASCII alpha and contain only alphanumerics and dots.
    let valid = !name.is_empty()
        && name
            .chars()
            .next()
            .map(|c| c.is_ascii_alphabetic())
            .unwrap_or(false)
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '.');

    if valid {
        Some(name)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_anthropic_request_serializes() {
        let req = AnthropicRequest {
            model: "claude-3-5-sonnet".to_string(),
            max_tokens: 4096,
            system: "You are helpful".to_string(),
            messages: vec![AnthropicMessage {
                role: "user".to_string(),
                content: "Hello".to_string(),
            }],
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("max_tokens"));
        assert!(json.contains("claude-3-5-sonnet"));
        assert!(json.contains("You are helpful"));
    }

    #[test]
    fn test_openai_request_serializes() {
        let req = OpenAiRequest {
            model: "gpt-4".to_string(),
            messages: vec![OpenAiMessage {
                role: "user".to_string(),
                content: "test".to_string(),
            }],
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("gpt-4"));
        assert!(json.contains("messages"));
    }

    #[test]
    fn test_validate_cls_syntax_requires_class_keyword() {
        assert!(validate_cls_syntax("Class Foo { }"));
        assert!(!validate_cls_syntax("function foo() {}"));
        assert!(!validate_cls_syntax(""));
    }

    #[test]
    fn test_validate_cls_syntax_requires_braces() {
        assert!(!validate_cls_syntax("Class Foo"));
        assert!(validate_cls_syntax("Class Foo { }"));
    }

    #[test]
    fn test_extract_class_name_from_generated_text() {
        // LLM typically wraps class in ```objectscript ``` blocks
        let text = "Here is your class:\n```objectscript\nClass MyApp.Test { }\n```";
        let result = extract_class_name(text);
        assert_eq!(result, Some("MyApp.Test".to_string()));
    }

    #[test]
    fn test_llm_client_needs_both_model_and_key() {
        std::env::remove_var("IRIS_GENERATE_CLASS_MODEL");
        std::env::remove_var("OPENAI_API_KEY");
        std::env::remove_var("ANTHROPIC_API_KEY");
        assert!(LlmClient::from_env().is_none());

        std::env::set_var("IRIS_GENERATE_CLASS_MODEL", "gpt-4");
        // Still None — needs API key too
        assert!(LlmClient::from_env().is_none());

        std::env::set_var("OPENAI_API_KEY", "sk-test");
        assert!(LlmClient::from_env().is_some());

        std::env::remove_var("IRIS_GENERATE_CLASS_MODEL");
        std::env::remove_var("OPENAI_API_KEY");
    }

    #[test]
    fn extract_class_name_invalid_starts_with_digit_returns_none() {
        let text = "Class 9Invalid.Name { }";
        assert_eq!(extract_class_name(text), None);
    }

    #[test]
    fn extract_class_name_invalid_contains_special_chars_returns_none() {
        let text = "Class My-Class.Name { }";
        assert_eq!(extract_class_name(text), None);
    }

    #[test]
    fn extract_class_name_no_class_line_returns_none() {
        let text = "This has no class definition at all";
        assert_eq!(extract_class_name(text), None);
    }

    #[test]
    fn extract_class_name_empty_input_returns_none() {
        assert_eq!(extract_class_name(""), None);
    }

    #[test]
    fn extract_class_name_valid_dotted_name() {
        let text = "Class My.Deep.Package.ClassName Extends %Persistent { }";
        assert_eq!(
            extract_class_name(text),
            Some("My.Deep.Package.ClassName".to_string())
        );
    }

    #[test]
    fn validate_cls_syntax_unbalanced_braces_returns_false() {
        assert!(!validate_cls_syntax("Class Foo { { }"));
        assert!(!validate_cls_syntax("Class Foo { } }"));
    }

    #[test]
    fn validate_cls_syntax_balanced_braces_returns_true() {
        let text = "Class Foo { Method Bar() { Quit 1 } }";
        assert!(validate_cls_syntax(text));
    }
}
