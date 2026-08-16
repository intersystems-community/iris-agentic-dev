use anyhow::{Context, Result};
use clap::Args;
use std::io::Read;
use std::path::PathBuf;

use super::connection_args::ConnectionArgs;
use super::dispatch::dispatch_tool;

/// How the ObjectScript code is supplied.
pub enum CodeSource {
    Inline(String),
    Stdin,
    File(PathBuf),
}

#[derive(Args)]
pub struct ExecCommand {
    /// ObjectScript code to execute. Use `-` to read from stdin.
    #[arg(value_name = "CODE")]
    pub code: Option<String>,

    /// Read code from a file (mutually exclusive with inline CODE argument)
    #[arg(long, short = 'f', value_name = "FILE", conflicts_with = "code")]
    pub file: Option<PathBuf>,

    /// Route this call to a named registered IRIS instance instead of the default connection.
    #[arg(long)]
    pub server: Option<String>,

    /// Enable the `%ctx` session carrier. Prints a `session_state` token after the raw
    /// output that a later invocation can pass back via `--session-state` to resume the
    /// same variables. Nothing is written to IRIS — the token is entirely client-held,
    /// so (unlike the WS terminal or an elicitation resume) this round-trips correctly
    /// across two separate CLI invocations.
    #[arg(long)]
    pub use_session: bool,

    /// A `session_state` token from a prior `--use-session` invocation. Restores `%ctx`
    /// before running. Ignored unless `--use-session` is also set.
    #[arg(long, requires = "use_session")]
    pub session_state: Option<String>,

    #[command(flatten)]
    pub conn: ConnectionArgs,
}

impl ExecCommand {
    /// Resolve which code source applies.
    pub fn source(&self) -> CodeSource {
        if let Some(path) = &self.file {
            return CodeSource::File(path.clone());
        }
        match &self.code {
            Some(s) if s == "-" => CodeSource::Stdin,
            Some(s) => CodeSource::Inline(s.clone()),
            None => CodeSource::Stdin,
        }
    }

    pub async fn run(self) -> Result<()> {
        let namespace = self.conn.namespace.clone();

        let code = match self.source() {
            CodeSource::Inline(s) => s,
            CodeSource::Stdin => {
                let mut buf = String::new();
                std::io::stdin()
                    .read_to_string(&mut buf)
                    .context("reading code from stdin")?;
                buf
            }
            CodeSource::File(path) => std::fs::read_to_string(&path)
                .with_context(|| format!("reading {}", path.display()))?,
        };

        let iris = self.conn.resolve().await?;

        // iris_execute's own role-gate only fires for a fleet "operate mode" Subject
        // instance — it does not enforce this check for the default connection role, so
        // this pre-check must stay here rather than being dropped in favor of whatever
        // the delegated tool call does internally. See dispatch.rs's doc comment.
        if !iris.is_write_allowed() {
            eprintln!(
                "error: write operations are suppressed on production IRIS instances.\n\
                 Set IRIS_ALLOW_PROD=1 to override."
            );
            std::process::exit(1);
        }

        let mut args = serde_json::json!({ "code": code, "namespace": namespace });
        if let Some(server) = &self.server {
            args["server"] = serde_json::Value::String(server.clone());
        }
        if self.use_session {
            args["use_session"] = serde_json::Value::Bool(true);
        }
        if let Some(state) = &self.session_state {
            args["session_state"] = serde_json::Value::String(state.clone());
        }

        let body = dispatch_tool(iris, "iris_execute", args).await?;
        let success = body["success"].as_bool().unwrap_or(false);

        // `output` carries both the normal case and an ObjectScript runtime error
        // caught by the executor's own Try/Catch (embedded as "ERROR: ..." text) — print
        // it raw either way, matching the prior behavior of printing whatever IRIS sent
        // back with no framing.
        if let Some(out) = body["output"].as_str() {
            print!("{}", out);
        } else if let Some(err) = body["error"].as_str() {
            // Failure modes with no `output` at all (TIMEOUT, HTTP_EXECUTION_FAILED,
            // SESSION_INVALID, ...) — print the error message alone, matching the prior
            // behavior of printing just the error string, no JSON framing.
            println!("{}", err);
        }

        // Only printed when the caller opted into sessions, so the raw-output-only
        // contract for the common non-session case is unchanged.
        if self.use_session {
            if let Some(tok) = body["session_state"].as_str() {
                println!("\nsession_state: {}", tok);
            }
        }

        if !success {
            std::process::exit(1);
        }
        Ok(())
    }
}
