use anyhow::Result;
use clap::{Args, Subcommand};
use std::path::PathBuf;

#[derive(Args)]
pub struct TelemetryCommand {
    #[command(subcommand)]
    pub subcommand: TelemetrySubcommand,
}

#[derive(Subcommand)]
pub enum TelemetrySubcommand {
    /// Export telemetry records from local JSONL sink
    Export(ExportArgs),
}

#[derive(Args)]
pub struct ExportArgs {
    /// Filter by Gauntlet run ID (matches eval_run_id field)
    #[arg(long)]
    pub run_id: Option<String>,
    /// Output format: jsonl or text (default: text)
    #[arg(long, default_value = "text")]
    pub format: String,
    /// Override config directory (default: ~/.config/iris-agentic-dev)
    #[arg(long)]
    pub config_dir: Option<PathBuf>,
}

impl TelemetryCommand {
    pub async fn run(self) -> Result<()> {
        match self.subcommand {
            TelemetrySubcommand::Export(args) => args.run(),
        }
    }
}

impl ExportArgs {
    pub fn run(self) -> Result<()> {
        let config_dir = self.config_dir.unwrap_or_else(|| {
            dirs::config_dir()
                .unwrap_or_else(|| {
                    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
                    PathBuf::from(home).join(".config")
                })
                .join("iris-agentic-dev")
        });

        let telemetry_dir = config_dir.join("telemetry");
        if !telemetry_dir.exists() {
            return Ok(());
        }

        let files: Vec<PathBuf> = std::fs::read_dir(&telemetry_dir)?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "jsonl"))
            .collect();

        let mut records: Vec<serde_json::Value> = Vec::new();
        for path in files {
            let contents = std::fs::read_to_string(&path)?;
            for line in contents.lines() {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
                    // Filter by run_id if provided
                    if let Some(ref run_id) = self.run_id {
                        if v.get("eval_run_id").and_then(|v| v.as_str()) != Some(run_id.as_str()) {
                            continue;
                        }
                    }
                    records.push(v);
                }
            }
        }

        match self.format.as_str() {
            "jsonl" => {
                for rec in &records {
                    println!("{}", serde_json::to_string(rec)?);
                }
            }
            _ => {
                // Text table
                println!(
                    "{:<32} {:<30} {:<8} {:<12} {:<24} {:<16} {}",
                    "timestamp", "tool", "ok", "duration_ms", "run_id", "task_id", "condition"
                );
                println!("{}", "-".repeat(140));
                for rec in &records {
                    let ts = rec.get("timestamp").and_then(|v| v.as_str()).unwrap_or("-");
                    let tool = rec.get("tool").and_then(|v| v.as_str()).unwrap_or("-");
                    let ok = rec
                        .get("success")
                        .and_then(|v| v.as_bool())
                        .map(|b| if b { "yes" } else { "no" })
                        .unwrap_or("-");
                    let ms = rec
                        .get("duration_ms")
                        .and_then(|v| v.as_u64())
                        .map(|n| n.to_string())
                        .unwrap_or_else(|| "-".to_string());
                    let run_id = rec
                        .get("eval_run_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("-");
                    let task_id = rec
                        .get("eval_task_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("-");
                    let condition = rec
                        .get("eval_condition")
                        .and_then(|v| v.as_str())
                        .unwrap_or("-");
                    println!(
                        "{:<32} {:<30} {:<8} {:<12} {:<24} {:<16} {}",
                        ts, tool, ok, ms, run_id, task_id, condition
                    );
                }
                println!("\n{} record(s)", records.len());
            }
        }

        Ok(())
    }
}
