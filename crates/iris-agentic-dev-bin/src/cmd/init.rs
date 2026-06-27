use anyhow::{Context, Result};
use clap::Args;

#[derive(Args)]
pub struct InitCommand {
    /// Overwrite existing .iris-agentic-dev.toml if present
    #[arg(long)]
    pub force: bool,
    /// Workspace directory (default: current directory)
    #[arg(long, default_value = ".")]
    pub workspace: String,
    /// Output format: text or json
    #[arg(long, default_value = "text")]
    pub format: String,
    /// Config mode: "develop" (default) or "operate" (fleet/multi-instance template)
    #[arg(long, default_value = "develop")]
    pub mode: String,
}

impl InitCommand {
    pub async fn run(self) -> Result<()> {
        let workspace = std::path::Path::new(&self.workspace);
        let config_path = workspace.join(".iris-agentic-dev.toml");

        if config_path.exists() && !self.force {
            anyhow::bail!(
                ".iris-agentic-dev.toml already exists at {}. Use --force to overwrite.",
                config_path.display()
            );
        }

        // Detect best-matching container using workspace basename scoring
        let workspace_basename = workspace
            .canonicalize()
            .unwrap_or_else(|_| workspace.to_path_buf())
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("project")
            .to_string();

        let containers =
            iris_agentic_dev_core::tools::list_iris_containers_pub(&workspace_basename).await;
        if containers.is_empty() {
            eprintln!(
                "⚠ No running IRIS containers found. If your container was started without \
                 IRIS_PASSWORD, restart with: docker run -e IRIS_PASSWORD=SYS ..."
            );
        }
        let suggested_container = containers
            .first()
            .and_then(|c| c["name"].as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("{}-iris", workspace_basename));

        let content = if self.mode == "operate" {
            iris_agentic_dev_core::iris::workspace_config::generate_operate_toml_content(
                &suggested_container,
                "USER",
            )
        } else {
            iris_agentic_dev_core::iris::workspace_config::generate_toml_content(
                &suggested_container,
                "USER",
            )
        };

        std::fs::write(&config_path, &content)
            .with_context(|| format!("writing {}", config_path.display()))?;

        if self.format == "json" {
            println!(
                "{}",
                serde_json::json!({
                    "success": true,
                    "path": config_path.to_string_lossy(),
                    "container": suggested_container,
                })
            );
        } else {
            println!("✓ Created {}", config_path.display());
            println!("  container = \"{}\"", suggested_container);
            println!("  namespace = \"USER\"");
        }
        Ok(())
    }
}
