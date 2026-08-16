use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use std::io::{BufRead, Read, Write};
use std::path::PathBuf;

use super::connection_args::ConnectionArgs;
use super::dispatch;

#[derive(Subcommand)]
pub enum DocAction {
    /// Fetch a class document and print to stdout
    Get {
        /// Class name (e.g. Config.MapMirrors or %Dictionary.ClassDefinition)
        #[arg(value_name = "CLASSNAME")]
        name: String,
    },
    /// Write a class document from file or stdin.
    /// Use `-` as CLASSNAME to read content from stdin.
    Put {
        /// Class name (e.g. MyApp.MyClass), or `-` to read class content from stdin
        #[arg(value_name = "CLASSNAME", allow_hyphen_values = true)]
        name: String,

        /// Read content from file
        #[arg(long, short = 'f', value_name = "FILE")]
        file: Option<PathBuf>,
    },
}

#[derive(Args)]
pub struct DocCommand {
    #[command(subcommand)]
    pub action: DocAction,

    /// Route this call to a named registered IRIS instance instead of the default connection.
    #[arg(long)]
    pub server: Option<String>,

    #[command(flatten)]
    pub conn: ConnectionArgs,
}

impl DocCommand {
    pub async fn run(self) -> Result<()> {
        let namespace = self.conn.namespace.clone();
        let iris = self.conn.resolve().await?;

        match self.action {
            DocAction::Get { name } => {
                let doc_name = ensure_cls_extension(&name);
                let mut args =
                    serde_json::json!({ "mode": "get", "name": doc_name, "namespace": namespace });
                if let Some(server) = &self.server {
                    args["server"] = serde_json::Value::String(server.clone());
                }

                let body = dispatch::dispatch_tool(iris, "iris_doc", args).await?;
                if body["success"].as_bool() != Some(true) {
                    let msg = body["error"].as_str().unwrap_or("get failed");
                    eprintln!("error: {}", msg);
                    std::process::exit(1);
                }
                let content = body["content"].as_str().unwrap_or("");
                // Print raw UDL source — no framing, pipe-safe
                print!("{}", content);
                if !content.ends_with('\n') {
                    println!();
                }
            }
            DocAction::Put { name, file } => {
                // iris_doc's own role-gate only fires for a fleet "operate mode" Subject
                // instance, same gap as iris_execute — see dispatch.rs's doc comment.
                if !iris.is_write_allowed() {
                    eprintln!(
                        "error: write operations are suppressed on production IRIS instances.\n\
                         Set IRIS_ALLOW_PROD=1 to override."
                    );
                    std::process::exit(1);
                }

                let doc_name = ensure_cls_extension(&name);
                let content = if name == "-" {
                    let mut buf = String::new();
                    std::io::stdin()
                        .read_to_string(&mut buf)
                        .context("reading doc content from stdin")?;
                    buf
                } else if let Some(path) = file {
                    std::fs::read_to_string(&path)
                        .with_context(|| format!("reading {}", path.display()))?
                } else {
                    eprintln!("error: `doc put` requires --file <path> or `-` to read from stdin");
                    std::process::exit(1);
                };

                let mut args = serde_json::json!({
                    "mode": "put",
                    "name": doc_name,
                    "content": content,
                    "namespace": namespace,
                });
                if let Some(server) = &self.server {
                    args["server"] = serde_json::Value::String(server.clone());
                }

                // Built once, reused for both the initial write and (if needed) the
                // elicitation resume — both calls must land in the same in-process
                // ElicitationStore, which a fresh IrisTools per call would not provide.
                let tools = dispatch::build_tools(iris)?;
                let mut body = dispatch::call(&tools, "iris_doc", args).await?;

                // SCM checkout dialog: prompt right here, in this same process, and
                // resume immediately — the only way this can work at all, since the
                // elicitation_id is a key into an in-memory store that would already be
                // gone by the time a second CLI invocation looked it up.
                while body["elicitation_required"].as_bool() == Some(true) {
                    let eid = body["elicitation_id"].as_str().unwrap_or("").to_string();
                    let message = body["message"].as_str().unwrap_or("Proceed?");
                    let answer = prompt_yes_no(message)?;

                    let resume_args = serde_json::json!({
                        "elicitation_id": eid,
                        "elicitation_answer": if answer { "yes" } else { "no" },
                    });
                    body = dispatch::call(&tools, "iris_doc", resume_args).await?;
                }

                if body["success"].as_bool() != Some(true) {
                    let msg = body["error"].as_str().unwrap_or("write failed");
                    eprintln!("error: {}", msg);
                    std::process::exit(1);
                }
                println!("OK: {}", body["name"].as_str().unwrap_or(&doc_name));
            }
        }
        Ok(())
    }
}

/// Prompt `message` on stderr and read a yes/no answer from stdin.
/// Defaults to "no" on EOF (non-interactive stdin — e.g. piped from /dev/null in a
/// script) rather than blocking forever or guessing "yes" for a destructive-adjacent
/// SCM checkout.
fn prompt_yes_no(message: &str) -> Result<bool> {
    eprint!("{} [y/N]: ", message);
    std::io::stderr().flush().ok();
    let mut line = String::new();
    let n = std::io::stdin().lock().read_line(&mut line)?;
    if n == 0 {
        eprintln!("(no input — declining)");
        return Ok(false);
    }
    let answer = line.trim().to_ascii_lowercase();
    Ok(answer == "y" || answer == "yes")
}

fn ensure_cls_extension(name: &str) -> String {
    if name.contains('.')
        && (name.ends_with(".cls")
            || name.ends_with(".CLS")
            || name.ends_with(".mac")
            || name.ends_with(".inc"))
    {
        name.to_string()
    } else if !name.contains('.') {
        // No dot at all — treat as-is
        name.to_string()
    } else {
        // Has dots but no known extension — append .cls
        format!("{}.cls", name)
    }
}
