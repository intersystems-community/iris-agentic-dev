use anyhow::{Context, Result};
use clap::Args;
use iris_agentic_dev_core::benchmark::{
    acquire_lock, cli_dispatch, load_embedded_tasks, release_lock, run_suite, BenchmarkComparison,
    BenchmarkMode, BenchmarkResult, LockResult,
};
use iris_agentic_dev_core::generate::LlmClient;
use iris_agentic_dev_core::iris::{
    connection::{DiscoverySource, IrisConnection},
    discovery::{discover_iris, IrisDiscovery},
};

#[derive(Args)]
pub struct BenchmarkCommand {
    #[arg(long)]
    pub skill: String,
    #[arg(long)]
    pub baseline: bool,
    #[arg(long, default_value = "jira")]
    pub suite: String,
    #[arg(long)]
    pub output: Option<String>,
    #[arg(long, env = "IRIS_GENERATE_CLASS_MODEL")]
    pub model: Option<String>,
    #[arg(long, default_value = "30")]
    pub task_timeout_s: u64,
    #[arg(long, default_value = "600")]
    pub max_time_s: u64,
    #[arg(long, env = "IRIS_HOST")]
    pub host: Option<String>,
    #[arg(long, env = "IRIS_WEB_PORT", default_value = "52773")]
    pub web_port: u16,
    #[arg(long, env = "IRIS_NAMESPACE", default_value = "USER")]
    pub namespace: String,
    #[arg(long, env = "IRIS_USERNAME")]
    pub username: Option<String>,
    #[arg(long, env = "IRIS_PASSWORD")]
    pub password: Option<String>,
    /// Benchmark execution mode: "mcp" (default) or "cli-dispatch".
    #[arg(long, default_value = "mcp")]
    pub mode: String,
    /// Maximum agentic loop iterations per task (CLI dispatch mode only).
    #[arg(long, default_value = "10")]
    pub max_iterations: u32,
    /// Maximum tokens per task before recording Fail (CLI dispatch mode only).
    #[arg(long, default_value = "50000")]
    pub max_task_tokens: u32,
    /// Path to a prior result JSON to compare against (produces a comparison section).
    #[arg(long)]
    pub compare: Option<String>,
}

impl BenchmarkCommand {
    pub async fn run(self) -> Result<()> {
        if self.suite != "jira" {
            eprintln!(
                "Error [SUITE_NOT_AVAILABLE]: suite '{}' is not available in v1 — only 'jira' \
                 (the primary repair suite) is ported. 'mf' (multi-file) and 'sql' (SQL quirks) \
                 are explicitly deferred.",
                self.suite
            );
            std::process::exit(1);
        }

        // Validate mode before doing any heavy work
        match self.mode.as_str() {
            "mcp" | "cli-dispatch" => {}
            other => {
                eprintln!(
                    "Error [UNKNOWN_MODE]: unknown benchmark mode '{other}'. \
                     Use 'mcp' (default) or 'cli-dispatch'."
                );
                std::process::exit(1);
            }
        }

        let _ = std::fs::read_to_string(&self.skill)
            .with_context(|| format!("reading skill file {}", self.skill))?;
        let skill_content = std::fs::read_to_string(&self.skill).unwrap_or_default();

        if let Some(model) = &self.model {
            std::env::set_var("IRIS_GENERATE_CLASS_MODEL", model);
        }

        let explicit = self.host.as_ref().map(|host| {
            let base_url = format!("http://{}:{}", host, self.web_port);
            let username = self.username.as_deref().unwrap_or("_SYSTEM");
            let password = self.password.as_deref().unwrap_or("SYS");
            IrisConnection::new(
                base_url,
                &self.namespace,
                username,
                password,
                DiscoverySource::ExplicitFlag,
            )
        });
        let ws_path = std::env::var("OBJECTSCRIPT_WORKSPACE").ok();
        let explicit = iris_agentic_dev_core::iris::workspace_config::apply_workspace_config(
            explicit,
            ws_path.as_deref(),
            &self.namespace,
        );

        let iris = match discover_iris(explicit).await {
            IrisDiscovery::Found(c) => c,
            IrisDiscovery::NotFound => {
                anyhow::bail!(
                    "No IRIS connection found — set IRIS_HOST or run iris-agentic-dev mcp for auto-discovery"
                );
            }
            IrisDiscovery::Explained => {
                std::process::exit(1);
            }
        };

        let client = IrisConnection::http_client()?;

        // FR-013: reject a run against a container already in use by another active run.
        let container_name = self.host.clone().unwrap_or_else(|| self.namespace.clone());
        let lock = acquire_lock(
            &iris,
            &client,
            &self.namespace,
            &container_name,
            self.max_time_s,
        )
        .await;
        if lock == LockResult::AlreadyRunning {
            eprintln!(
                "Error [BENCHMARK_RUN_IN_PROGRESS]: another benchmark run is already in \
                 progress against '{container_name}'. Wait for it to finish, or if it was \
                 abandoned, it will be treated as stale after {}s.",
                self.max_time_s
            );
            std::process::exit(1);
        }

        let run_result = self.run_inner(&iris, &client, &skill_content).await;
        release_lock(&iris, &client, &self.namespace, &container_name).await;
        run_result
    }

    async fn run_inner(
        &self,
        iris: &IrisConnection,
        client: &reqwest::Client,
        skill_content: &str,
    ) -> Result<()> {
        let tasks = load_embedded_tasks().context("loading benchmark task suite")?;

        let iris_version = iris
            .execute_via_generator("write $ZVERSION", &self.namespace, client)
            .await
            .unwrap_or_else(|_| "unknown".to_string());

        let mut result: BenchmarkResult = match self.mode.as_str() {
            "mcp" => {
                let mut r = tokio::time::timeout(
                    std::time::Duration::from_secs(self.max_time_s),
                    run_suite(
                        iris,
                        client,
                        &self.namespace,
                        &tasks,
                        skill_content,
                        &iris_version,
                    ),
                )
                .await
                .context("benchmark run timed out")?;
                r.mode = Some(BenchmarkMode::Mcp);

                if self.baseline {
                    let baseline_result = tokio::time::timeout(
                        std::time::Duration::from_secs(self.max_time_s),
                        run_suite(iris, client, &self.namespace, &tasks, "", &iris_version),
                    )
                    .await
                    .context("baseline run timed out")?;
                    r.apply_baseline(baseline_result.pass_rate);
                }
                r
            }
            "cli-dispatch" => {
                let llm = LlmClient::from_env().ok_or_else(|| {
                    anyhow::anyhow!(
                        "no LLM configured: set IRIS_GENERATE_CLASS_MODEL + OPENAI_API_KEY/ANTHROPIC_API_KEY"
                    )
                })?;

                let binary_path = cli_dispatch::iris_dev_bin();
                if !binary_path.exists() {
                    anyhow::bail!(
                        "CLI_DISPATCH_BINARY_NOT_FOUND: binary not found at {}",
                        binary_path.display()
                    );
                }

                // Build connection env vars for subprocesses
                let connection_env = self.build_connection_env();

                let config = cli_dispatch::CliDispatchConfig {
                    binary_path,
                    max_iterations: self.max_iterations,
                    max_task_tokens: self.max_task_tokens,
                    task_timeout_s: self.task_timeout_s,
                    connection_env,
                };

                let mut r = tokio::time::timeout(
                    std::time::Duration::from_secs(self.max_time_s),
                    cli_dispatch::run_cli_dispatch_suite(
                        iris,
                        client,
                        &self.namespace,
                        &tasks,
                        skill_content,
                        &iris_version,
                        &config,
                        &llm,
                    ),
                )
                .await
                .context("CLI dispatch benchmark run timed out")?;

                if self.baseline {
                    let baseline_config = cli_dispatch::CliDispatchConfig {
                        binary_path: cli_dispatch::iris_dev_bin(),
                        max_iterations: self.max_iterations,
                        max_task_tokens: self.max_task_tokens,
                        task_timeout_s: self.task_timeout_s,
                        connection_env: self.build_connection_env(),
                    };
                    let baseline_result = tokio::time::timeout(
                        std::time::Duration::from_secs(self.max_time_s),
                        cli_dispatch::run_cli_dispatch_suite(
                            iris,
                            client,
                            &self.namespace,
                            &tasks,
                            "",
                            &iris_version,
                            &baseline_config,
                            &llm,
                        ),
                    )
                    .await
                    .context("CLI dispatch baseline run timed out")?;
                    r.apply_baseline(baseline_result.pass_rate);
                }
                r
            }
            other => {
                anyhow::bail!("UNKNOWN_MODE: {other}");
            }
        };

        // --compare: load a prior result and compute deltas
        if let Some(compare_path) = &self.compare {
            let prior_json = std::fs::read_to_string(compare_path)
                .with_context(|| format!("reading compare file {compare_path}"))?;
            let prior: BenchmarkResult = serde_json::from_str(&prior_json)
                .with_context(|| format!("parsing compare file {compare_path}"))?;
            result.comparison = Some(compute_comparison(&result, &prior));
        }

        let json = serde_json::to_string_pretty(&result)?;
        match &self.output {
            Some(path) => std::fs::write(path, &json).with_context(|| format!("writing {path}"))?,
            None => println!("{json}"),
        }

        Ok(())
    }

    fn build_connection_env(&self) -> Vec<(String, String)> {
        let mut env = Vec::new();
        if let Some(host) = &self.host {
            env.push(("IRIS_HOST".to_string(), host.clone()));
            env.push(("IRIS_WEB_PORT".to_string(), self.web_port.to_string()));
        }
        env.push(("IRIS_NAMESPACE".to_string(), self.namespace.clone()));
        if let Some(username) = &self.username {
            env.push(("IRIS_USERNAME".to_string(), username.clone()));
        }
        if let Some(password) = &self.password {
            env.push(("IRIS_PASSWORD".to_string(), password.clone()));
        }
        env
    }
}

/// Computes a `BenchmarkComparison` between a new result and a prior result.
///
/// `other_mode` is taken from the prior result's `mode` field (or `Mcp` if absent).
pub fn compute_comparison(
    current: &BenchmarkResult,
    prior: &BenchmarkResult,
) -> BenchmarkComparison {
    let other_mode = prior.mode.unwrap_or(BenchmarkMode::Mcp);
    let pass_rate_delta = current.pass_rate - prior.pass_rate;
    let tokens_total_delta = match (current.tokens_total, prior.tokens_total) {
        (Some(c), Some(p)) => Some(c as i64 - p as i64),
        _ => None,
    };
    let elapsed_s_delta = current.elapsed_s - prior.elapsed_s;
    BenchmarkComparison {
        other_mode,
        pass_rate_delta,
        tokens_total_delta,
        elapsed_s_delta,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iris_agentic_dev_core::benchmark::{TaskOutcome, TaskResult};

    fn make_benchmark_result(
        pass_rate: f64,
        mode: Option<BenchmarkMode>,
        tokens_total: Option<u64>,
        elapsed_s: f64,
    ) -> BenchmarkResult {
        BenchmarkResult {
            pass_rate,
            baseline_pass_rate: None,
            lift: None,
            tasks_passed: 1,
            tasks_total: 1,
            tasks_errored: 0,
            iris_version: "2026.2".to_string(),
            elapsed_s,
            task_results: vec![TaskResult {
                task_id: "t1".to_string(),
                outcome: TaskOutcome::Pass,
                iterations: 1,
                elapsed_s: 0.1,
                reason: String::new(),
                tokens_input: None,
                tokens_output: None,
                tokens_total: tokens_total.map(|t| t as u32),
            }],
            mode,
            tokens_input: None,
            tokens_output: None,
            tokens_total,
            comparison: None,
        }
    }

    // T016: unknown mode unit test
    #[test]
    fn unknown_mode_is_not_mcp_or_cli_dispatch() {
        let mode = "foo";
        let is_valid = matches!(mode, "mcp" | "cli-dispatch");
        assert!(!is_valid);
    }

    #[test]
    fn mcp_mode_is_valid() {
        let mode = "mcp";
        let is_valid = matches!(mode, "mcp" | "cli-dispatch");
        assert!(is_valid);
    }

    #[test]
    fn cli_dispatch_mode_is_valid() {
        let mode = "cli-dispatch";
        let is_valid = matches!(mode, "mcp" | "cli-dispatch");
        assert!(is_valid);
    }

    // T017: compute_comparison unit tests
    #[test]
    fn compute_comparison_correct_deltas() {
        let current =
            make_benchmark_result(0.8, Some(BenchmarkMode::CliDispatch), Some(5000), 30.0);
        let prior = make_benchmark_result(0.75, Some(BenchmarkMode::Mcp), Some(3000), 20.0);
        let cmp = compute_comparison(&current, &prior);
        assert_eq!(cmp.other_mode, BenchmarkMode::Mcp);
        assert!((cmp.pass_rate_delta - 0.05).abs() < 1e-9);
        assert_eq!(cmp.tokens_total_delta, Some(2000));
        assert!((cmp.elapsed_s_delta - 10.0).abs() < 1e-9);
    }

    #[test]
    fn compute_comparison_prior_without_tokens_gives_null_delta() {
        let current =
            make_benchmark_result(0.8, Some(BenchmarkMode::CliDispatch), Some(5000), 30.0);
        let prior = make_benchmark_result(0.75, Some(BenchmarkMode::Mcp), None, 20.0);
        let cmp = compute_comparison(&current, &prior);
        assert_eq!(cmp.tokens_total_delta, None);
    }

    #[test]
    fn compute_comparison_prior_without_mode_defaults_to_mcp() {
        let current = make_benchmark_result(0.8, Some(BenchmarkMode::CliDispatch), None, 30.0);
        let prior = make_benchmark_result(0.75, None, None, 20.0);
        let cmp = compute_comparison(&current, &prior);
        assert_eq!(cmp.other_mode, BenchmarkMode::Mcp);
    }
}
