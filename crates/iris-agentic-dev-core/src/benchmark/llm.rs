//! Thin wrapper reusing `generate.rs`'s `LlmClient` for skill-vs-baseline fix prompts.
//! No new HTTP/LLM SDK crate is introduced (Constitution VII).

use crate::benchmark::BenchmarkTask;
use crate::generate::LlmClient;

const SYSTEM_PROMPT: &str = "You are an expert InterSystems ObjectScript developer fixing \
a reported bug. You will be given the buggy source file(s), a description of the bug, and \
the goal. Respond with ONLY the corrected ObjectScript class source (one or more `Class ... \
{ ... }` blocks), no prose, no markdown code fences.";

/// Builds the user-facing fix-request prompt for a task, optionally prefixed with skill
/// guidance (empty string for the baseline pass).
pub fn build_prompt(task: &BenchmarkTask, skill_content: &str) -> String {
    let mut parts = Vec::new();
    if !skill_content.is_empty() {
        parts.push(format!("# Skill guidance\n\n{skill_content}\n"));
    }
    parts.push(format!("# Bug report\n\n{}\n", task.description));
    parts.push(format!("# Goal\n\n{}\n", task.goal));
    parts.push(format!(
        "# Expected behavior\n\n{}\n",
        task.expected_behavior
    ));
    if !task.hints.is_empty() {
        parts.push(format!("# Hints\n\n{}\n", task.hints.join("\n")));
    }
    parts.push("# Buggy source file(s)\n".to_string());
    for file in &task.initial_code.files {
        parts.push(format!("## {}\n\n```\n{}\n```\n", file.path, file.content));
    }
    parts.join("\n")
}

/// Extracts `Class ... { ... }` blocks from an LLM response, stripping any markdown code
/// fences the model might add despite instructions not to.
pub fn extract_fixed_classes(response: &str) -> Vec<String> {
    let stripped = response
        .replace("```objectscript", "")
        .replace("```cls", "")
        .replace("```", "");
    split_classes(&stripped)
}

fn split_classes(content: &str) -> Vec<String> {
    let mut classes = Vec::new();
    let mut current = String::new();
    let mut in_class = false;
    let mut brace_depth = 0i32;
    for line in content.lines() {
        let trimmed = line.trim_start();
        if !in_class && trimmed.starts_with("Class ") {
            in_class = true;
        }
        if in_class {
            current.push_str(line);
            current.push('\n');
            brace_depth += line.matches('{').count() as i32;
            brace_depth -= line.matches('}').count() as i32;
            if brace_depth <= 0 && current.contains('{') {
                classes.push(current.trim().to_string());
                current.clear();
                in_class = false;
                brace_depth = 0;
            }
        }
    }
    classes
}

/// Runs the LLM against `task` with the given skill content, returning the extracted
/// fixed class source(s). Errors if no `LlmClient` can be constructed from env, if the
/// LLM call fails, or if no `Class ... { ... }` block is found in the response.
pub async fn propose_fix(task: &BenchmarkTask, skill_content: &str) -> anyhow::Result<Vec<String>> {
    let client = LlmClient::from_env().ok_or_else(|| {
        anyhow::anyhow!(
            "no LLM configured: set IRIS_GENERATE_CLASS_MODEL + OPENAI_API_KEY/ANTHROPIC_API_KEY"
        )
    })?;
    let prompt = build_prompt(task, skill_content);
    let response = client.complete(SYSTEM_PROMPT, &prompt).await?;
    let classes = extract_fixed_classes(&response);
    if classes.is_empty() {
        anyhow::bail!("LLM response contained no Class ... {{ ... }} block");
    }
    Ok(classes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::benchmark::{InitialCode, SourceFile, SuccessCriteria, TestCode};

    fn sample_task() -> BenchmarkTask {
        BenchmarkTask {
            task_id: "jira-001".to_string(),
            category: "jira_bugs".to_string(),
            difficulty: "easy".to_string(),
            description: "desc".to_string(),
            goal: "goal".to_string(),
            initial_code: InitialCode {
                files: vec![SourceFile {
                    path: "src/Foo.cls".to_string(),
                    content: "Class Foo {}".to_string(),
                }],
            },
            test_code: TestCode {
                path: "tests/TestFoo.cls".to_string(),
                content: "Class TestFoo {}".to_string(),
            },
            expected_behavior: "behaves".to_string(),
            hints: vec!["hint1".to_string()],
            success_criteria: SuccessCriteria {
                compile_success: true,
                tests_pass: true,
                max_patch_lines: 30,
                requires_symbol_preservation: true,
            },
            metadata: serde_json::Value::Null,
        }
    }

    #[test]
    fn build_prompt_includes_description_goal_and_source() {
        let task = sample_task();
        let prompt = build_prompt(&task, "");
        assert!(prompt.contains("desc"));
        assert!(prompt.contains("goal"));
        assert!(prompt.contains("Class Foo {}"));
        assert!(!prompt.contains("Skill guidance"));
    }

    #[test]
    fn build_prompt_includes_skill_content_when_present() {
        let task = sample_task();
        let prompt = build_prompt(&task, "use idiom X");
        assert!(prompt.contains("Skill guidance"));
        assert!(prompt.contains("use idiom X"));
    }

    #[test]
    fn extract_fixed_classes_finds_single_class() {
        let response =
            "Class MyApp.Foo Extends %RegisteredObject\n{\nMethod Bar() {\n Quit 1\n}\n}\n";
        let classes = extract_fixed_classes(response);
        assert_eq!(classes.len(), 1);
        assert!(classes[0].contains("MyApp.Foo"));
    }

    #[test]
    fn extract_fixed_classes_strips_markdown_fences() {
        let response = "```objectscript\nClass MyApp.Foo\n{\nMethod Bar() {\n Quit 1\n}\n}\n```\n";
        let classes = extract_fixed_classes(response);
        assert_eq!(classes.len(), 1);
        assert!(!classes[0].contains("```"));
    }

    #[test]
    fn extract_fixed_classes_finds_multiple_classes() {
        let response =
            "Class A\n{\nMethod M() {\n Quit\n}\n}\nClass B\n{\nMethod N() {\n Quit\n}\n}\n";
        let classes = extract_fixed_classes(response);
        assert_eq!(classes.len(), 2);
    }

    #[test]
    fn extract_fixed_classes_returns_empty_for_no_class_blocks() {
        let classes = extract_fixed_classes("no classes here, just prose");
        assert!(classes.is_empty());
    }
}
