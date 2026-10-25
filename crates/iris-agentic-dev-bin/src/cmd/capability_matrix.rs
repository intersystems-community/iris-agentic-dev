use anyhow::Result;
use clap::Args;
use iris_agentic_dev_core::tools::admin_tools;

use super::connection_args::ConnectionArgs;

#[derive(Args)]
pub struct CapabilityMatrixCommand {
    #[command(flatten)]
    pub conn: ConnectionArgs,
    /// Emit raw JSON instead of human-readable text
    #[arg(long)]
    pub json: bool,
}

impl CapabilityMatrixCommand {
    pub async fn run(self) -> Result<()> {
        let iris = self.conn.resolve().await?;
        let client = iris.client(iris_agentic_dev_core::iris::http::ClientKind::Call)?;

        match admin_tools::capability_matrix_impl(&iris, &client, None).await {
            Ok(result) => {
                for content in &result.content {
                    if let Some(text) = content.as_text() {
                        if self.json {
                            // Re-serialize as compact JSON
                            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text.text) {
                                println!("{}", serde_json::to_string(&v)?);
                            } else {
                                println!("{}", text.text);
                            }
                        } else {
                            // Pretty-print
                            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text.text) {
                                println!("{}", serde_json::to_string_pretty(&v)?);
                            } else {
                                println!("{}", text.text);
                            }
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!("error: {e}");
                crate::exit(1);
            }
        }
        Ok(())
    }
}
