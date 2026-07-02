use anyhow::{Context, Result};
use clap::Args;
use iris_agentic_dev_core::benchmark::{
    acquire_lock, load_tasks, release_lock, run_suite, BenchmarkResult, LockResult,
};
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

        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let tasks_dir = std::path::Path::new(manifest_dir)
            .parent()
            .unwrap()
            .join("iris-agentic-dev-core/src/benchmark/tasks/jira_bugs");
        let run_result = self
            .run_inner(&iris, &client, &tasks_dir, &skill_content)
            .await;
        release_lock(&iris, &client, &self.namespace, &container_name).await;
        run_result
    }

    async fn run_inner(
        &self,
        iris: &IrisConnection,
        client: &reqwest::Client,
        tasks_dir: &std::path::Path,
        skill_content: &str,
    ) -> Result<()> {
        let tasks = load_tasks(tasks_dir).context("loading benchmark task suite")?;

        let iris_version = iris
            .execute_via_generator("write $ZVERSION", &self.namespace, client)
            .await
            .unwrap_or_else(|_| "unknown".to_string());

        let mut result: BenchmarkResult = tokio::time::timeout(
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

        if self.baseline {
            let baseline_result = tokio::time::timeout(
                std::time::Duration::from_secs(self.max_time_s),
                run_suite(iris, client, &self.namespace, &tasks, "", &iris_version),
            )
            .await
            .context("baseline run timed out")?;
            result.apply_baseline(baseline_result.pass_rate);
        }

        let json = serde_json::to_string_pretty(&result)?;
        match &self.output {
            Some(path) => std::fs::write(path, &json).with_context(|| format!("writing {path}"))?,
            None => println!("{json}"),
        }

        Ok(())
    }
}
