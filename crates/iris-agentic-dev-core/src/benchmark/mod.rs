//! Native benchmark harness: loads a skill, runs it against the ported `jira_bugs`
//! task suite using iris-agentic-dev's own compile/execute/test tools in-process, and
//! reports pass_rate/baseline_pass_rate/lift. See specs/059-tool-telemetry-benchmark/.

pub mod container;
pub mod llm;

use serde::{Deserialize, Serialize};
use std::path::Path;

/// A single scoring unit, ported 1:1 from objectscript-coder's
/// `bench/eval_tasks/jira_bugs/*.json` schema (unchanged — Assumption 2 requires content
/// preservation, only the home repository changes).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BenchmarkTask {
    pub task_id: String,
    pub category: String,
    pub difficulty: String,
    pub description: String,
    pub goal: String,
    pub initial_code: InitialCode,
    pub test_code: TestCode,
    pub expected_behavior: String,
    #[serde(default)]
    pub hints: Vec<String>,
    pub success_criteria: SuccessCriteria,
    #[serde(default)]
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct InitialCode {
    pub files: Vec<SourceFile>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SourceFile {
    pub path: String,
    pub content: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TestCode {
    pub path: String,
    pub content: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SuccessCriteria {
    pub compile_success: bool,
    pub tests_pass: bool,
    #[serde(default)]
    pub max_patch_lines: u32,
    #[serde(default)]
    pub requires_symbol_preservation: bool,
}

/// Loads all `*.json` task files from `dir`, deserializing each via `serde_json`.
pub fn load_tasks(dir: &Path) -> anyhow::Result<Vec<BenchmarkTask>> {
    let entries = std::fs::read_dir(dir)
        .map_err(|e| anyhow::anyhow!("failed to read task directory {}: {e}", dir.display()))?;
    let mut tasks = Vec::new();
    let mut paths: Vec<std::path::PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    paths.sort();
    for path in paths {
        let contents = std::fs::read_to_string(&path)?;
        let task: BenchmarkTask = serde_json::from_str(&contents)
            .map_err(|e| anyhow::anyhow!("failed to parse task file {}: {e}", path.display()))?;
        tasks.push(task);
    }
    Ok(tasks)
}

/// Per-task outcome. `Error` (FR-012) is distinguishable from a normal `Fail` — used when
/// the task's expected tool/skill surface no longer matches the current system (e.g. a
/// tool-level error occurred before the task's own fix was ever exercised), so a stale
/// task is not silently counted against a skill's score.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskOutcome {
    Pass,
    Fail,
    Error,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TaskResult {
    pub task_id: String,
    pub outcome: TaskOutcome,
    pub iterations: u32,
    pub elapsed_s: f64,
    #[serde(default)]
    pub reason: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BenchmarkResult {
    pub pass_rate: f64,
    pub baseline_pass_rate: Option<f64>,
    pub lift: Option<f64>,
    pub tasks_passed: u32,
    pub tasks_total: u32,
    pub tasks_errored: u32,
    pub iris_version: String,
    pub elapsed_s: f64,
    pub task_results: Vec<TaskResult>,
}

impl BenchmarkResult {
    /// Computes `pass_rate` as `tasks_passed / (tasks_passed + tasks_failed)` — errored
    /// tasks are excluded from the denominator (FR-012: an errored task must not silently
    /// count against pass_rate) and reported separately via `tasks_errored`.
    pub fn from_task_results(
        task_results: Vec<TaskResult>,
        iris_version: String,
        elapsed_s: f64,
    ) -> Self {
        let tasks_passed = task_results
            .iter()
            .filter(|t| t.outcome == TaskOutcome::Pass)
            .count() as u32;
        let tasks_failed = task_results
            .iter()
            .filter(|t| t.outcome == TaskOutcome::Fail)
            .count() as u32;
        let tasks_errored = task_results
            .iter()
            .filter(|t| t.outcome == TaskOutcome::Error)
            .count() as u32;
        let denom = tasks_passed + tasks_failed;
        let pass_rate = if denom == 0 {
            0.0
        } else {
            tasks_passed as f64 / denom as f64
        };
        Self {
            pass_rate,
            baseline_pass_rate: None,
            lift: None,
            tasks_passed,
            tasks_total: task_results.len() as u32,
            tasks_errored,
            iris_version,
            elapsed_s,
            task_results,
        }
    }

    /// Sets `baseline_pass_rate` and computes `lift = pass_rate - baseline_pass_rate`
    /// (absolute difference, FR-004 — matches `BENCHMARKING.md`'s worked example and the
    /// already-in-production `objectscript_mcp` runner's formula).
    pub fn apply_baseline(&mut self, baseline_pass_rate: f64) {
        self.baseline_pass_rate = Some(baseline_pass_rate);
        self.lift = Some(self.pass_rate - baseline_pass_rate);
    }
}

/// Extracts the dotted ObjectScript class name from a `Class X.Y.Z ... {` declaration
/// (first line matching, case-insensitive on the `Class` keyword). Returns `None` if no
/// such declaration is found — the caller treats this as a stale/malformed task.
pub fn extract_class_name(source: &str) -> Option<String> {
    source
        .lines()
        .find(|l| l.trim_start().to_lowercase().starts_with("class "))
        .and_then(|l| l.split_whitespace().nth(1))
        .map(|s| s.to_string())
}

/// Runs `task` (compile its files, apply the LLM-proposed fix, compile again, run its
/// test) and returns a `TaskResult`. `skill_content` is empty for the baseline pass.
///
/// A tool-level error before ever reaching the task's own fix (e.g. a compile-endpoint
/// connection failure, or a task whose source has no extractable class name — signalling
/// a stale/malformed task, FR-012) maps to `TaskOutcome::Error`, distinct from a normal
/// `TaskOutcome::Fail` (the fix compiled and ran but didn't satisfy the test).
pub async fn run_task(
    iris: &crate::iris::connection::IrisConnection,
    client: &reqwest::Client,
    namespace: &str,
    task: &BenchmarkTask,
    skill_content: &str,
) -> TaskResult {
    let start = std::time::Instant::now();
    let result = run_task_inner(iris, client, namespace, task, skill_content).await;
    let elapsed_s = start.elapsed().as_secs_f64();
    match result {
        Ok(outcome) => TaskResult {
            task_id: task.task_id.clone(),
            outcome,
            iterations: 1,
            elapsed_s,
            reason: String::new(),
        },
        Err(e) => TaskResult {
            task_id: task.task_id.clone(),
            outcome: TaskOutcome::Error,
            iterations: 1,
            elapsed_s,
            reason: e.to_string(),
        },
    }
}

async fn run_task_inner(
    iris: &crate::iris::connection::IrisConnection,
    client: &reqwest::Client,
    namespace: &str,
    task: &BenchmarkTask,
    skill_content: &str,
) -> anyhow::Result<TaskOutcome> {
    // Compile the initial (buggy) files first, to confirm the task's own fixture is
    // valid before applying any fix — a task whose fixture doesn't compile as shipped is
    // stale (FR-012), not a normal fail.
    for file in &task.initial_code.files {
        let class_name = extract_class_name(&file.content)
            .ok_or_else(|| anyhow::anyhow!("no Class declaration found in {}", file.path))?;
        container::write_and_compile(
            iris,
            client,
            namespace,
            &format!("{class_name}.cls"),
            &file.content,
        )
        .await?;
    }

    let fixed_classes = llm::propose_fix(task, skill_content).await?;
    for fixed in &fixed_classes {
        let class_name = extract_class_name(fixed)
            .ok_or_else(|| anyhow::anyhow!("LLM fix contained no Class declaration"))?;
        let errors = container::write_and_compile(
            iris,
            client,
            namespace,
            &format!("{class_name}.cls"),
            fixed,
        )
        .await?;
        if !errors.is_empty() && task.success_criteria.compile_success {
            return Ok(TaskOutcome::Fail);
        }
    }

    let test_class_name = extract_class_name(&task.test_code.content)
        .ok_or_else(|| anyhow::anyhow!("no Class declaration found in test_code"))?;
    container::write_and_compile(
        iris,
        client,
        namespace,
        &format!("{test_class_name}.cls"),
        &task.test_code.content,
    )
    .await?;
    let (passed, _detail) =
        container::run_class_tests(iris, client, namespace, &test_class_name).await?;
    if passed == task.success_criteria.tests_pass {
        Ok(TaskOutcome::Pass)
    } else {
        Ok(TaskOutcome::Fail)
    }
}

/// Runs the full task suite once (skill or baseline pass) and aggregates into a
/// `BenchmarkResult` (without baseline/lift applied — caller applies via
/// `apply_baseline` when `--baseline` was requested).
pub async fn run_suite(
    iris: &crate::iris::connection::IrisConnection,
    client: &reqwest::Client,
    namespace: &str,
    tasks: &[BenchmarkTask],
    skill_content: &str,
    iris_version: &str,
) -> BenchmarkResult {
    let start = std::time::Instant::now();
    let mut task_results = Vec::with_capacity(tasks.len());
    for task in tasks {
        task_results.push(run_task(iris, client, namespace, task, skill_content).await);
    }
    BenchmarkResult::from_task_results(
        task_results,
        iris_version.to_string(),
        start.elapsed().as_secs_f64(),
    )
}

/// Result of attempting to acquire the concurrent-run lock (FR-013): `Acquired` means the
/// caller now holds the lock and must call `release_lock` when done; `AlreadyRunning`
/// means another run holds it (age below `max_age_secs`, so not treated as abandoned).
#[derive(Debug, PartialEq, Eq)]
pub enum LockResult {
    Acquired,
    AlreadyRunning,
}

/// Pure decision function: given the existing lock's age in seconds (`None` if no lock
/// exists), decide whether a new run may proceed. A lock older than `max_age_secs` is
/// treated as abandoned and overridden (contracts/benchmark-cli.md).
pub fn decide_lock(existing_lock_age_secs: Option<u64>, max_age_secs: u64) -> LockResult {
    match existing_lock_age_secs {
        None => LockResult::Acquired,
        Some(age) if age > max_age_secs => LockResult::Acquired,
        Some(_) => LockResult::AlreadyRunning,
    }
}

/// Acquires the run lock for `container_name` at `^IRISDEV("telemetry","benchmark_lock",container_name)`,
/// per contracts/benchmark-cli.md. Best-effort — if the lock read/write itself fails
/// (e.g. no IRIS available), proceeds as `Acquired` rather than blocking a run on lock
/// infrastructure failure.
pub async fn acquire_lock(
    iris: &crate::iris::connection::IrisConnection,
    client: &reqwest::Client,
    namespace: &str,
    container_name: &str,
    max_age_secs: u64,
) -> LockResult {
    let safe = container_name.replace('"', "\"\"");
    let read_code =
        format!("set ts=$Get(^IRISDEV(\"telemetry\",\"benchmark_lock\",\"{safe}\"))\nwrite ts,!");
    let existing = iris
        .execute_via_generator(&read_code, namespace, client)
        .await
        .ok();
    let age_secs = existing.and_then(|out| {
        let trimmed = out.trim();
        if trimmed.is_empty() {
            return None;
        }
        chrono::DateTime::parse_from_rfc3339(trimmed)
            .ok()
            .map(|ts| {
                chrono::Utc::now()
                    .signed_duration_since(ts.with_timezone(&chrono::Utc))
                    .num_seconds()
                    .max(0) as u64
            })
    });
    let decision = decide_lock(age_secs, max_age_secs);
    if decision == LockResult::Acquired {
        let now = chrono::Utc::now().to_rfc3339();
        let write_code =
            format!("set ^IRISDEV(\"telemetry\",\"benchmark_lock\",\"{safe}\")=\"{now}\"\n");
        let _ = iris
            .execute_via_generator(&write_code, namespace, client)
            .await;
    }
    decision
}

/// Releases the run lock for `container_name`. Best-effort.
pub async fn release_lock(
    iris: &crate::iris::connection::IrisConnection,
    client: &reqwest::Client,
    namespace: &str,
    container_name: &str,
) {
    let safe = container_name.replace('"', "\"\"");
    let code = format!("kill ^IRISDEV(\"telemetry\",\"benchmark_lock\",\"{safe}\")\n");
    let _ = iris.execute_via_generator(&code, namespace, client).await;
}

#[cfg(test)]
mod lock_tests {
    use super::*;

    #[test]
    fn no_existing_lock_acquires() {
        assert_eq!(decide_lock(None, 600), LockResult::Acquired);
    }

    #[test]
    fn fresh_lock_blocks_new_run() {
        assert_eq!(decide_lock(Some(10), 600), LockResult::AlreadyRunning);
    }

    #[test]
    fn stale_lock_older_than_max_age_is_overridden() {
        assert_eq!(decide_lock(Some(700), 600), LockResult::Acquired);
    }

    #[test]
    fn lock_exactly_at_max_age_still_blocks() {
        assert_eq!(decide_lock(Some(600), 600), LockResult::AlreadyRunning);
    }
}

#[cfg(test)]
mod class_name_tests {
    use super::*;

    #[test]
    fn extracts_dotted_class_name() {
        let src = "Class Healthcare.PatientService Extends %RegisteredObject\n{\n}\n";
        assert_eq!(
            extract_class_name(src),
            Some("Healthcare.PatientService".to_string())
        );
    }

    #[test]
    fn extracts_class_name_ignoring_leading_whitespace_and_case() {
        let src = "  class MyApp.Foo Extends %RegisteredObject\n{\n}\n";
        assert_eq!(extract_class_name(src), Some("MyApp.Foo".to_string()));
    }

    #[test]
    fn returns_none_when_no_class_declaration_present() {
        assert_eq!(extract_class_name("just some text, no class here"), None);
    }
}
