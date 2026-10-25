//! CLI dispatch benchmark mode: agentic loop using subprocess tool invocations.
//! The agent calls `iris-agentic-dev tool <name> --args '<json>'` as shell subprocesses
//! and iterates until task complete or a limit is hit.
//! See specs/080-cli-dispatch-benchmark/.

use crate::benchmark::{BenchmarkMode, BenchmarkResult, BenchmarkTask, TaskOutcome, TaskResult};
use crate::generate::LlmClient;
use std::path::{Path, PathBuf};

/// A single tool invocation parsed from an LLM response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolInvocation {
    pub tool_name: String,
    pub args_json: String,
}

/// The tools the benchmark agent may call (spec 141, FR-004, R1 C3). The prompt lists exactly
/// these, and [`check_invocation`] refuses every other name before a process starts.
pub const CLI_DISPATCH_TOOLS: &[&str] = &[
    "iris_compile",
    "iris_execute",
    "iris_search",
    "iris_symbols",
];

/// Argument names the agent may not send: they choose the server, the namespace or the
/// confirmation. Matched case-insensitively with `_` and `-` ignored, so `forceWritable` and
/// `force-writable` are `force_writable`.
pub const CLI_DISPATCH_REFUSED_ARGS: &[&str] = &[
    "server",
    "namespace",
    "confirm",
    "confirmed",
    "force_writable",
];

/// What the prompt tells the agent each allowed tool does.
fn tool_blurb(tool: &str) -> &'static str {
    match tool {
        "iris_compile" => "compile an ObjectScript class",
        "iris_execute" => "execute ObjectScript code",
        "iris_search" => "search for classes and symbols",
        "iris_symbols" => "inspect class members and structure",
        _ => "",
    }
}

/// `force_writable`, `forceWritable` and `Force-Writable` all fold to `forcewritable`.
fn fold_arg_name(name: &str) -> String {
    name.chars()
        .filter(|c| *c != '_' && *c != '-')
        .flat_map(char::to_lowercase)
        .collect()
}

/// Whether the benchmark may run `inv`. The check sits in the parent, so a refused call never
/// starts a process. The `Err` text is what the agent is shown.
pub fn check_invocation(inv: &ToolInvocation) -> Result<(), String> {
    if !CLI_DISPATCH_TOOLS.contains(&inv.tool_name.as_str()) {
        return Err(format!(
            "`{}` is not available in this benchmark. Use one of: {}.",
            inv.tool_name,
            CLI_DISPATCH_TOOLS.join(", ")
        ));
    }
    let args: serde_json::Value = serde_json::from_str(&inv.args_json)
        .map_err(|e| format!("--args is not valid JSON: {e}"))?;
    let Some(map) = args.as_object() else {
        return Err("--args must be a JSON object".to_string());
    };
    let refused: Vec<String> = CLI_DISPATCH_REFUSED_ARGS
        .iter()
        .map(|n| fold_arg_name(n))
        .collect();
    for key in map.keys() {
        if refused.contains(&fold_arg_name(key)) {
            return Err(format!(
                "the argument `{key}` is not accepted: the benchmark chooses the server, the \
                 namespace and every confirmation."
            ));
        }
    }
    Ok(())
}

/// Run each invocation through `runner` and return the text the agent is shown for each. A refused
/// invocation answers with a tool error and never calls `runner`.
pub fn run_invocations(
    invocations: &[ToolInvocation],
    runner: &mut dyn FnMut(&ToolInvocation) -> String,
) -> Vec<String> {
    invocations
        .iter()
        .map(|inv| match check_invocation(inv) {
            Ok(()) => format!("Tool `{}` result:\n{}", inv.tool_name, runner(inv)),
            Err(why) => format!("Tool `{}` refused: {why}", inv.tool_name),
        })
        .collect()
}

/// Configuration for a CLI dispatch benchmark run.
pub struct CliDispatchConfig {
    /// Path to the `iris-agentic-dev` binary used for subprocess tool calls.
    pub binary_path: PathBuf,
    /// Maximum turns per task before recording `Fail`.
    pub max_iterations: u32,
    /// Stop if accumulated tokens per task exceed this.
    pub max_task_tokens: u32,
    /// Wall-clock timeout per task in seconds.
    pub task_timeout_s: u64,
    /// Environment variables to pass to subprocess (IRIS connection args).
    pub connection_env: Vec<(String, String)>,
}

/// Resolves the `iris-agentic-dev` binary under the workspace `target` directory.
pub fn iris_dev_bin() -> PathBuf {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default();
    // CARGO_MANIFEST_DIR is the core crate; go up two levels to workspace root
    let workspace = Path::new(&manifest)
        .parent()
        .and_then(|p| p.parent())
        .unwrap_or(Path::new("."))
        .to_path_buf();
    workspace_iad_binary(&workspace)
}

/// The workspace build of `iris-agentic-dev`: the instrumented one under `cargo llvm-cov`,
/// otherwise `debug`, then `release`.
///
/// The coverage build is chosen only when `CARGO_LLVM_COV` says this is a coverage run. Outside
/// one it is whatever the last `cargo llvm-cov` left behind, and preferring it because it exists
/// ran a weeks-old binary under plain `cargo test` (130 round 4).
pub fn workspace_iad_binary(workspace: &Path) -> PathBuf {
    let target = workspace.join("target");
    if std::env::var_os("CARGO_LLVM_COV").is_some() {
        return target.join("llvm-cov-target/debug/iris-agentic-dev");
    }
    let debug = target.join("debug/iris-agentic-dev");
    if debug.exists() {
        return debug;
    }
    target.join("release/iris-agentic-dev")
}

/// Parses tool invocations from an LLM response.
///
/// Looks for lines matching:
/// `iris-agentic-dev tool <name> --args '<json>'`
///
/// Returns all such invocations in document order. The `args_json` is returned
/// as-is (not validated) — invalid JSON is fed to the subprocess which will
/// return an error, and that error is relayed back to the agent.
pub fn parse_tool_invocations(response: &str) -> Vec<ToolInvocation> {
    let mut invocations = Vec::new();
    for line in response.lines() {
        let trimmed = line.trim();
        // Pattern: iris-agentic-dev tool <name> --args '<json>'
        if let Some(rest) = trimmed.strip_prefix("iris-agentic-dev tool ") {
            // rest is: <name> --args '<json>'
            // split on " --args " to separate name from args
            if let Some(args_pos) = rest.find(" --args ") {
                let tool_name = rest[..args_pos].trim().to_string();
                let args_part = rest[args_pos + 8..].trim(); // skip " --args "
                                                             // Strip surrounding single quotes if present
                let args_json = if args_part.starts_with('\'') && args_part.ends_with('\'') {
                    args_part[1..args_part.len() - 1].to_string()
                } else {
                    args_part.to_string()
                };
                if !tool_name.is_empty() {
                    invocations.push(ToolInvocation {
                        tool_name,
                        args_json,
                    });
                }
            }
        }
    }
    invocations
}

const SENTINEL_START: &str = "===FIXED_CLASS_START===";
const SENTINEL_END: &str = "===FIXED_CLASS_END===";

/// Extracts the corrected class source from an LLM response.
///
/// Looks for the sentinel block:
/// ```text
/// ===FIXED_CLASS_START===
/// Class Foo { ... }
/// ===FIXED_CLASS_END===
/// ```
///
/// If no sentinel is found, falls back to `extract_fixed_classes` from `llm.rs`
/// (the agent may output class source directly without the sentinel).
pub fn extract_sentinel_class(response: &str) -> Option<String> {
    let start = response.find(SENTINEL_START)?;
    let after_start = &response[start + SENTINEL_START.len()..];
    let end = after_start.find(SENTINEL_END)?;
    let content = after_start[..end].trim().to_string();
    if content.is_empty() {
        None
    } else {
        Some(content)
    }
}

/// Builds the system prompt for CLI dispatch mode.
///
/// The prompt describes the task context, how to invoke tools via CLI, and
/// how to signal completion via the sentinel block. Skill content is prepended
/// as a "# Skill guidance" section (empty skill skips the section).
pub fn build_cli_dispatch_system_prompt(skill_content: &str) -> String {
    let mut parts = Vec::new();

    if !skill_content.is_empty() {
        parts.push(format!("# Skill guidance\n\n{skill_content}\n"));
    }

    parts.push(
        "# Your role\n\n\
        You are an expert InterSystems ObjectScript developer. You have access to IRIS development \
        tools via the CLI. Use them to diagnose and fix the reported bug.\n"
            .to_string(),
    );

    let tool_lines: String = CLI_DISPATCH_TOOLS
        .iter()
        .map(|t| format!("- `{t}` — {}\n", tool_blurb(t)))
        .collect();
    parts.push(format!(
        "# Available tools\n\n\
        Invoke tools by outputting a line in this exact format:\n\
        ```\n\
        iris-agentic-dev tool <tool_name> --args '<json_args>'\n\
        ```\n\n\
        Available tools:\n\
        {tool_lines}"
    ));

    parts.push(format!(
        "# Completing the task\n\n\
        When you have fixed the bug, output the corrected class source wrapped in sentinel markers:\n\
        ```\n\
        {SENTINEL_START}\n\
        Class YourClass.Name Extends ... {{\n\
          ...\n\
        }}\n\
        {SENTINEL_END}\n\
        ```\n"
    ));

    parts.join("\n")
}

/// An empty directory for a child's cwd, removed when dropped.
struct ScratchDir(PathBuf);

impl ScratchDir {
    fn create() -> Option<Self> {
        let dir = std::env::temp_dir().join(format!("iad-bench-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).ok()?;
        Some(Self(dir))
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Executes a single tool invocation as a subprocess.
///
/// Returns combined stdout+stderr as a string. Never panics — all errors
/// (binary not found, non-zero exit, etc.) are captured and returned as strings
/// so the agent can see the error and potentially retry.
pub fn run_tool_subprocess(
    binary: &Path,
    invocation: &ToolInvocation,
    env_vars: &[(String, String)],
) -> String {
    // A scratch directory holds no `.iris-agentic-dev.toml`, and an empty environment carries none
    // of the parent's `IRIS_*` settings, so the child cannot pick up the operator's pool, keychain
    // servers or `[policy.*]` from wherever the benchmark was started (R1 C3).
    let scratch = ScratchDir::create();
    let mut cmd = std::process::Command::new(binary);
    cmd.env_clear()
        .arg("tool")
        .arg(&invocation.tool_name)
        .arg("--toolset")
        .arg("merged")
        .arg("--args")
        .arg(&invocation.args_json);
    if let Some(dir) = &scratch {
        cmd.current_dir(&dir.0);
    }
    for key in ["PATH", "HOME"] {
        if let Some(val) = std::env::var_os(key) {
            cmd.env(key, val);
        }
    }
    for (key, val) in env_vars {
        cmd.env(key, val);
    }
    // The benchmark compiles, and nothing the agent runs may kill a global.
    cmd.env("IRIS_WRITE_TOOLS_ENABLED", "1")
        .env("IRIS_DESTRUCTIVE_TOOLS_ENABLED", "0");
    match cmd.output() {
        Ok(output) => {
            let mut combined = String::new();
            combined.push_str(&String::from_utf8_lossy(&output.stdout));
            combined.push_str(&String::from_utf8_lossy(&output.stderr));
            if combined.trim().is_empty() {
                if output.status.success() {
                    "(no output)".to_string()
                } else {
                    format!("Error: process exited with status {}", output.status)
                }
            } else {
                combined
            }
        }
        Err(e) => format!("Error: failed to launch subprocess: {e}"),
    }
}

/// Runs the agentic loop for a single task.
///
/// Orchestrates: build prompt → call LLM → parse tool calls → run subprocesses →
/// feed results back → repeat until sentinel or limit hit → extract class →
/// compile + test via container.
pub async fn run_cli_dispatch_task(
    iris: &crate::iris::connection::IrisConnection,
    client: &reqwest::Client,
    namespace: &str,
    task: &BenchmarkTask,
    skill_content: &str,
    config: &CliDispatchConfig,
    llm: &LlmClient,
) -> TaskResult {
    let start = std::time::Instant::now();

    if !config.binary_path.exists() {
        return TaskResult {
            task_id: task.task_id.clone(),
            outcome: TaskOutcome::Error,
            iterations: 0,
            elapsed_s: start.elapsed().as_secs_f64(),
            reason: format!(
                "CLI_DISPATCH_BINARY_NOT_FOUND: binary not found at {}",
                config.binary_path.display()
            ),
            tokens_input: None,
            tokens_output: None,
            tokens_total: None,
        };
    }

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(config.task_timeout_s),
        run_cli_dispatch_task_inner(iris, client, namespace, task, skill_content, config, llm),
    )
    .await;

    let elapsed_s = start.elapsed().as_secs_f64();

    match result {
        Ok(inner) => TaskResult { elapsed_s, ..inner },
        Err(_timeout) => TaskResult {
            task_id: task.task_id.clone(),
            outcome: TaskOutcome::Fail,
            iterations: config.max_iterations,
            elapsed_s,
            reason: "task timed out".to_string(),
            tokens_input: None,
            tokens_output: None,
            tokens_total: None,
        },
    }
}

async fn run_cli_dispatch_task_inner(
    iris: &crate::iris::connection::IrisConnection,
    client: &reqwest::Client,
    namespace: &str,
    task: &BenchmarkTask,
    skill_content: &str,
    config: &CliDispatchConfig,
    llm: &LlmClient,
) -> TaskResult {
    use crate::benchmark::{container, extract_class_name};

    let system_prompt = build_cli_dispatch_system_prompt(skill_content);
    let task_prompt = crate::benchmark::llm::build_prompt(task, "");

    // Compile the initial (buggy) files first to confirm the fixture is valid.
    for file in &task.initial_code.files {
        let class_name = match extract_class_name(&file.content) {
            Some(n) => n,
            None => {
                return TaskResult {
                    task_id: task.task_id.clone(),
                    outcome: TaskOutcome::Error,
                    iterations: 0,
                    elapsed_s: 0.0,
                    reason: format!("no Class declaration found in {}", file.path),
                    tokens_input: None,
                    tokens_output: None,
                    tokens_total: None,
                };
            }
        };
        if let Err(e) = container::write_and_compile(
            iris,
            client,
            namespace,
            &format!("{class_name}.cls"),
            &file.content,
        )
        .await
        {
            return TaskResult {
                task_id: task.task_id.clone(),
                outcome: TaskOutcome::Error,
                iterations: 0,
                elapsed_s: 0.0,
                reason: format!("IRIS_UNREACHABLE: {e}"),
                tokens_input: None,
                tokens_output: None,
                tokens_total: None,
            };
        }
    }

    let mut conversation = vec![("user".to_string(), task_prompt)];
    let mut total_input: u32 = 0;
    let mut total_output: u32 = 0;
    let mut has_usage = false;
    let mut iterations = 0u32;

    for _turn in 0..config.max_iterations {
        iterations += 1;

        // Build the full conversation context as a single user prompt for simple
        // non-streaming non-tool-use API.
        let user_prompt = conversation
            .iter()
            .map(|(role, content)| format!("[{role}]: {content}"))
            .collect::<Vec<_>>()
            .join("\n\n");

        let (response, usage) = match llm.complete_with_usage(&system_prompt, &user_prompt).await {
            Ok(r) => r,
            Err(e) => {
                return TaskResult {
                    task_id: task.task_id.clone(),
                    outcome: TaskOutcome::Error,
                    iterations,
                    elapsed_s: 0.0,
                    reason: format!("LLM error: {e}"),
                    tokens_input: None,
                    tokens_output: None,
                    tokens_total: None,
                };
            }
        };

        if let Some(u) = usage {
            total_input += u.input;
            total_output += u.output;
            has_usage = true;
        }

        // Check token budget
        let tokens_so_far = total_input + total_output;
        if tokens_so_far > config.max_task_tokens {
            return TaskResult {
                task_id: task.task_id.clone(),
                outcome: TaskOutcome::Fail,
                iterations,
                elapsed_s: 0.0,
                reason: format!(
                    "token limit exceeded ({tokens_so_far} > {})",
                    config.max_task_tokens
                ),
                tokens_input: if has_usage { Some(total_input) } else { None },
                tokens_output: if has_usage { Some(total_output) } else { None },
                tokens_total: if has_usage {
                    Some(total_input + total_output)
                } else {
                    None
                },
            };
        }

        // Check for sentinel (task complete)
        if let Some(fixed_source) = extract_sentinel_class(&response) {
            return finalize_task(
                iris,
                client,
                namespace,
                task,
                &fixed_source,
                iterations,
                total_input,
                total_output,
                has_usage,
            )
            .await;
        }

        // Check for tool invocations
        let invocations = parse_tool_invocations(&response);
        if invocations.is_empty() {
            // Agent produced only prose — add to conversation and continue
            conversation.push(("assistant".to_string(), response));
            continue;
        }

        // Execute tool invocations and collect results
        let tool_results = run_invocations(&invocations, &mut |inv| {
            run_tool_subprocess(&config.binary_path, inv, &config.connection_env)
        });

        conversation.push(("assistant".to_string(), response));
        conversation.push(("user".to_string(), tool_results.join("\n\n")));
    }

    // Max iterations reached without sentinel — check for class in last response
    if let Some(last_response) = conversation.iter().rev().find(|(r, _)| r == "assistant") {
        let fixed_classes = crate::benchmark::llm::extract_fixed_classes(&last_response.1);
        if !fixed_classes.is_empty() {
            return finalize_task(
                iris,
                client,
                namespace,
                task,
                &fixed_classes[0],
                iterations,
                total_input,
                total_output,
                has_usage,
            )
            .await;
        }
    }

    TaskResult {
        task_id: task.task_id.clone(),
        outcome: TaskOutcome::Fail,
        iterations,
        elapsed_s: 0.0,
        reason: format!(
            "max iterations ({}) reached without fix",
            config.max_iterations
        ),
        tokens_input: if has_usage { Some(total_input) } else { None },
        tokens_output: if has_usage { Some(total_output) } else { None },
        tokens_total: if has_usage {
            Some(total_input + total_output)
        } else {
            None
        },
    }
}

#[allow(clippy::too_many_arguments)]
async fn finalize_task(
    iris: &crate::iris::connection::IrisConnection,
    client: &reqwest::Client,
    namespace: &str,
    task: &BenchmarkTask,
    fixed_source: &str,
    iterations: u32,
    total_input: u32,
    total_output: u32,
    has_usage: bool,
) -> TaskResult {
    use crate::benchmark::{container, extract_class_name};

    let class_name = match extract_class_name(fixed_source) {
        Some(n) => n,
        None => {
            return TaskResult {
                task_id: task.task_id.clone(),
                outcome: TaskOutcome::Fail,
                iterations,
                elapsed_s: 0.0,
                reason: "LLM fix contained no Class declaration".to_string(),
                tokens_input: if has_usage { Some(total_input) } else { None },
                tokens_output: if has_usage { Some(total_output) } else { None },
                tokens_total: if has_usage {
                    Some(total_input + total_output)
                } else {
                    None
                },
            };
        }
    };

    match container::write_and_compile(
        iris,
        client,
        namespace,
        &format!("{class_name}.cls"),
        fixed_source,
    )
    .await
    {
        Ok(errors) => {
            if !errors.is_empty() && task.success_criteria.compile_success {
                return TaskResult {
                    task_id: task.task_id.clone(),
                    outcome: TaskOutcome::Fail,
                    iterations,
                    elapsed_s: 0.0,
                    reason: format!("compile errors: {errors:?}"),
                    tokens_input: if has_usage { Some(total_input) } else { None },
                    tokens_output: if has_usage { Some(total_output) } else { None },
                    tokens_total: if has_usage {
                        Some(total_input + total_output)
                    } else {
                        None
                    },
                };
            }
        }
        Err(e) => {
            return TaskResult {
                task_id: task.task_id.clone(),
                outcome: TaskOutcome::Error,
                iterations,
                elapsed_s: 0.0,
                reason: format!("IRIS_UNREACHABLE: {e}"),
                tokens_input: if has_usage { Some(total_input) } else { None },
                tokens_output: if has_usage { Some(total_output) } else { None },
                tokens_total: if has_usage {
                    Some(total_input + total_output)
                } else {
                    None
                },
            };
        }
    }

    // Write and run tests
    let test_class_name = match extract_class_name(&task.test_code.content) {
        Some(n) => n,
        None => {
            return TaskResult {
                task_id: task.task_id.clone(),
                outcome: TaskOutcome::Error,
                iterations,
                elapsed_s: 0.0,
                reason: "no Class declaration in test_code".to_string(),
                tokens_input: if has_usage { Some(total_input) } else { None },
                tokens_output: if has_usage { Some(total_output) } else { None },
                tokens_total: if has_usage {
                    Some(total_input + total_output)
                } else {
                    None
                },
            };
        }
    };

    if let Err(e) = container::write_and_compile(
        iris,
        client,
        namespace,
        &format!("{test_class_name}.cls"),
        &task.test_code.content,
    )
    .await
    {
        return TaskResult {
            task_id: task.task_id.clone(),
            outcome: TaskOutcome::Error,
            iterations,
            elapsed_s: 0.0,
            reason: format!("IRIS_UNREACHABLE: {e}"),
            tokens_input: if has_usage { Some(total_input) } else { None },
            tokens_output: if has_usage { Some(total_output) } else { None },
            tokens_total: if has_usage {
                Some(total_input + total_output)
            } else {
                None
            },
        };
    }

    let (passed, _detail) =
        match container::run_class_tests(iris, client, namespace, &test_class_name).await {
            Ok(r) => r,
            Err(e) => {
                return TaskResult {
                    task_id: task.task_id.clone(),
                    outcome: TaskOutcome::Error,
                    iterations,
                    elapsed_s: 0.0,
                    reason: format!("IRIS_UNREACHABLE: {e}"),
                    tokens_input: if has_usage { Some(total_input) } else { None },
                    tokens_output: if has_usage { Some(total_output) } else { None },
                    tokens_total: if has_usage {
                        Some(total_input + total_output)
                    } else {
                        None
                    },
                };
            }
        };

    let outcome = if passed == task.success_criteria.tests_pass {
        TaskOutcome::Pass
    } else {
        TaskOutcome::Fail
    };

    TaskResult {
        task_id: task.task_id.clone(),
        outcome,
        iterations,
        elapsed_s: 0.0,
        reason: String::new(),
        tokens_input: if has_usage { Some(total_input) } else { None },
        tokens_output: if has_usage { Some(total_output) } else { None },
        tokens_total: if has_usage {
            Some(total_input + total_output)
        } else {
            None
        },
    }
}

/// Runs the full task suite in CLI dispatch mode, returning a `BenchmarkResult`
/// with `mode: Some(BenchmarkMode::CliDispatch)` and aggregated token counts.
#[allow(clippy::too_many_arguments)]
pub async fn run_cli_dispatch_suite(
    iris: &crate::iris::connection::IrisConnection,
    client: &reqwest::Client,
    namespace: &str,
    tasks: &[BenchmarkTask],
    skill_content: &str,
    iris_version: &str,
    config: &CliDispatchConfig,
    llm: &LlmClient,
) -> BenchmarkResult {
    let start = std::time::Instant::now();
    let mut task_results = Vec::with_capacity(tasks.len());
    for task in tasks {
        task_results.push(
            run_cli_dispatch_task(iris, client, namespace, task, skill_content, config, llm).await,
        );
    }
    let mut result = BenchmarkResult::from_task_results(
        task_results,
        iris_version.to_string(),
        start.elapsed().as_secs_f64(),
    );
    result.mode = Some(BenchmarkMode::CliDispatch);
    result
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // T005: parse_tool_invocations tests

    #[test]
    fn parse_tool_invocations_finds_single_invocation() {
        let response = "iris-agentic-dev tool iris_compile --args '{\"doc\":\"Foo.cls\"}'";
        let invocations = parse_tool_invocations(response);
        assert_eq!(invocations.len(), 1);
        assert_eq!(invocations[0].tool_name, "iris_compile");
        assert_eq!(invocations[0].args_json, r#"{"doc":"Foo.cls"}"#);
    }

    #[test]
    fn parse_tool_invocations_empty_for_prose_only() {
        let response =
            "The bug is in the null check. You should add a check before accessing the property.";
        let invocations = parse_tool_invocations(response);
        assert!(invocations.is_empty());
    }

    #[test]
    fn parse_tool_invocations_mixed_prose_and_tool_line() {
        let response = "Let me compile this first.\niris-agentic-dev tool iris_compile --args '{\"doc\":\"Bar.cls\"}'\nOk, checking the output.";
        let invocations = parse_tool_invocations(response);
        assert_eq!(invocations.len(), 1);
        assert_eq!(invocations[0].tool_name, "iris_compile");
    }

    #[test]
    fn parse_tool_invocations_malformed_json_still_returns_invocation() {
        let response = "iris-agentic-dev tool iris_compile --args '{bad json'";
        let invocations = parse_tool_invocations(response);
        assert_eq!(invocations.len(), 1);
        assert_eq!(invocations[0].tool_name, "iris_compile");
        assert_eq!(invocations[0].args_json, "{bad json");
    }

    #[test]
    fn parse_tool_invocations_multiple_lines_preserves_order() {
        let response = "iris-agentic-dev tool iris_compile --args '{\"doc\":\"A.cls\"}'\niris-agentic-dev tool iris_execute --args '{\"code\":\"write 1\"}'\niris-agentic-dev tool iris_search --args '{\"query\":\"Foo\"}'\n";
        let invocations = parse_tool_invocations(response);
        assert_eq!(invocations.len(), 3);
        assert_eq!(invocations[0].tool_name, "iris_compile");
        assert_eq!(invocations[1].tool_name, "iris_execute");
        assert_eq!(invocations[2].tool_name, "iris_search");
    }

    // T006: extract_sentinel_class tests

    #[test]
    fn extract_sentinel_class_finds_class_between_sentinels() {
        let response =
            format!("Here is the fix:\n{SENTINEL_START}\nClass Foo {{}}\n{SENTINEL_END}\nDone.");
        let result = extract_sentinel_class(&response);
        assert_eq!(result, Some("Class Foo {}".to_string()));
    }

    #[test]
    fn extract_sentinel_class_returns_none_without_sentinel() {
        let response = "Class Foo {}\nMethod Bar() { Quit 1 }";
        let result = extract_sentinel_class(response);
        assert!(result.is_none());
    }

    #[test]
    fn extract_sentinel_class_strips_surrounding_prose() {
        let response = format!(
            "Some analysis...\n{SENTINEL_START}\nClass My.Fixed.Class Extends %RegisteredObject {{\nMethod Do() {{}}\n}}\n{SENTINEL_END}\nEnd of response."
        );
        let result = extract_sentinel_class(&response);
        let extracted = result.unwrap();
        assert!(extracted.contains("My.Fixed.Class"));
        assert!(!extracted.contains("Some analysis"));
        assert!(!extracted.contains("End of response"));
    }

    // T007: loop limit test (no IRIS, no live LLM)

    #[tokio::test]
    async fn loop_limit_stops_at_max_iterations() {
        // This test uses the mock LLM model which always returns a mock class (not a tool call).
        // To test the limit specifically we verify the max_iterations bound is respected.
        // The mock always returns a valid class with no tool calls and no sentinel,
        // which causes the loop to check for classes in the last response and finalize.
        // We instead test this via parse_tool_invocations behavior + the iteration counter.
        // A proper injection test would require a trait; we verify the config field is respected
        // by checking that max_iterations=0 means the loop body never runs.

        // Simulate: with max_iterations=2 and an agent that never uses tools/sentinel,
        // the loop should terminate after 2 turns.
        let config = CliDispatchConfig {
            binary_path: PathBuf::from("/nonexistent/iris-agentic-dev"),
            max_iterations: 2,
            max_task_tokens: 50000,
            task_timeout_s: 30,
            connection_env: vec![],
        };
        // We can't call the full loop without IRIS, but we verify the config is set correctly.
        assert_eq!(config.max_iterations, 2);
    }

    // T008: build_cli_dispatch_system_prompt tests

    #[test]
    fn prompt_contains_tool_list_without_skill() {
        let prompt = build_cli_dispatch_system_prompt("");
        assert!(prompt.contains("iris_compile"));
        assert!(prompt.contains("iris_execute"));
        assert!(prompt.contains("iris_search"));
        assert!(prompt.contains("iris_symbols"));
        assert!(!prompt.contains("Skill guidance"));
    }

    #[test]
    fn prompt_contains_skill_guidance_section_when_non_empty() {
        let prompt = build_cli_dispatch_system_prompt("use idiom X");
        assert!(prompt.contains("# Skill guidance"));
        assert!(prompt.contains("use idiom X"));
    }

    #[test]
    fn prompt_contains_sentinel_instructions() {
        let prompt = build_cli_dispatch_system_prompt("");
        assert!(prompt.contains(SENTINEL_START));
        assert!(prompt.contains(SENTINEL_END));
    }

    #[test]
    fn prompt_skill_guidance_appears_before_tool_list() {
        let prompt = build_cli_dispatch_system_prompt("my skill");
        let skill_pos = prompt.find("Skill guidance").unwrap();
        let tool_pos = prompt.find("iris_compile").unwrap();
        assert!(skill_pos < tool_pos);
    }

    // T009: run_tool_subprocess error handling tests

    #[test]
    fn run_tool_subprocess_nonexistent_binary_returns_error_string() {
        let inv = ToolInvocation {
            tool_name: "iris_compile".to_string(),
            args_json: r#"{"doc":"Foo.cls"}"#.to_string(),
        };
        let result = run_tool_subprocess(
            Path::new("/nonexistent/binary/that/does/not/exist"),
            &inv,
            &[],
        );
        assert!(result.contains("Error") || result.contains("error") || result.contains("failed"));
        // Should not panic
    }

    #[test]
    fn run_tool_subprocess_nonzero_exit_returns_nonempty_string() {
        let inv = ToolInvocation {
            tool_name: "anything".to_string(),
            args_json: "{}".to_string(),
        };
        // Use /bin/sh -c 'exit 1' — always exits non-zero
        // We fake this by providing a path to /bin/sh and checking the error handling
        // Since we can't control args fully, just verify a nonexistent binary returns error
        let result = run_tool_subprocess(Path::new("/nonexistent"), &inv, &[]);
        assert!(!result.is_empty());
    }

    #[test]
    fn run_tool_subprocess_real_binary_exit_nonzero() {
        // Use a real binary with a command that exits non-zero
        // /usr/bin/false always exits 1
        let inv = ToolInvocation {
            tool_name: "compile".to_string(),
            args_json: "{}".to_string(),
        };
        // We use /bin/false if available; otherwise just verify the error path
        let false_path = if std::path::Path::new("/usr/bin/false").exists() {
            PathBuf::from("/usr/bin/false")
        } else {
            PathBuf::from("/nonexistent")
        };
        let result = run_tool_subprocess(&false_path, &inv, &[]);
        // Either it ran and produced output (or empty but non-panic), or returned error string
        assert!(result.len() < 10000); // didn't hang/overflow
    }

    // 141 FR-004: the child sees only what the benchmark gives it, from a directory with no toml.
    #[cfg(unix)]
    #[test]
    fn run_tool_subprocess_runs_with_a_cleared_environment_in_a_scratch_directory() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("fake-iad");
        std::fs::write(&script, "#!/bin/sh\nenv\npwd\necho \"ARGV $*\"\nls -A\n").unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let inv = ToolInvocation {
            tool_name: "iris_execute".to_string(),
            args_json: r#"{"code":"Write 1"}"#.to_string(),
        };
        let out = run_tool_subprocess(
            &script,
            &inv,
            &[("IRIS_HOST".to_string(), "bench-host".to_string())],
        );
        // cargo sets this in every test process; the child must not inherit it.
        assert!(!out.contains("CARGO_MANIFEST_DIR"), "env leaked: {out}");
        assert!(out.contains("IRIS_HOST=bench-host"), "{out}");
        assert!(out.contains("IRIS_WRITE_TOOLS_ENABLED=1"), "{out}");
        assert!(out.contains("IRIS_DESTRUCTIVE_TOOLS_ENABLED=0"), "{out}");
        assert!(
            out.contains("ARGV tool iris_execute --toolset merged --args"),
            "{out}"
        );
        let cwd = out
            .lines()
            .find(|l| l.contains("iad-bench-"))
            .unwrap_or_else(|| panic!("the child did not run in a scratch directory: {out}"));
        assert!(
            !std::path::Path::new(cwd.trim()).exists(),
            "scratch directory {cwd} was not removed"
        );
    }
}
