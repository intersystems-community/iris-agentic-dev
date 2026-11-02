use anyhow::Result;
use clap::Args;

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
        // `capability_matrix` compiles a scratch class on the server. Strict read-only is checked
        // before anything is discovered, and the call then goes through the same pipeline as the
        // tool, so the template and `allow` answer for it too. It used to call the implementation
        // directly and meet none of them.
        if let Some(refusal) =
            iris_agentic_dev_core::tools::target::cli_strict_refusal("capability_matrix")
        {
            eprintln!("{refusal}");
            crate::exit(1);
        }
        let iris = self.conn.resolve().await?;
        let body =
            match super::dispatch::dispatch_tool(iris, "capability_matrix", serde_json::json!({}))
                .await
            {
                Ok(b) => b,
                Err(e) => {
                    eprintln!("error: {e}");
                    crate::exit(1);
                }
            };
        super::dispatch::exit_if_refused(&body);
        if self.json {
            println!("{}", serde_json::to_string(&body)?);
        } else {
            println!("{}", serde_json::to_string_pretty(&body)?);
        }
        // The body is printed either way; a failed probe is a failed command.
        if super::dispatch::is_failure(&body) {
            crate::exit(1);
        }
        Ok(())
    }
}
