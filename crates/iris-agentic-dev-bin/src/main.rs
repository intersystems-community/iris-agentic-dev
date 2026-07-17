use anyhow::Result;
use clap::{Parser, Subcommand};
use iris_agentic_dev::cmd;
use std::ffi::OsString;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(
    name = "iris-agentic-dev",
    version,
    about = "CLI and package manager for InterSystems IRIS developer ecosystem",
    long_about = None,
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Enable debug logging
    #[arg(long, global = true)]
    verbose: bool,

    /// List discovered iris-agentic-dev-* plugin commands on PATH
    #[arg(long)]
    list_plugins: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the MCP server (stdio or HTTP transport)
    Mcp(cmd::mcp::McpCommand),
    /// Compile ObjectScript .cls files on IRIS
    Compile(cmd::compile::CompileCommand),
    /// Execute ObjectScript code on IRIS
    Exec(cmd::exec::ExecCommand),
    /// Run a SQL query on IRIS and print TSV results
    Query(cmd::query::QueryCommand),
    /// Read or write IRIS class documents (doc get / doc put)
    Doc(cmd::doc::DocCommand),
    /// Invoke any MCP tool by name with JSON arguments
    Tool(cmd::tool::ToolCommand),
    /// Initialize a .iris-dev.toml workspace config
    Init(cmd::init::InitCommand),
    /// Install packages from iris-dev.toml
    Install(cmd::install::InstallCommand),
    /// Run the skill/tool benchmark harness (pass_rate/lift scoring against the ported task suite)
    Benchmark(cmd::benchmark::BenchmarkCommand),
    /// Install and manage the official InterSystems skill pack
    Skill(cmd::skill::SkillCommand),
    /// Any unrecognized subcommand — dispatched to an `iris-agentic-dev-<name>` plugin on
    /// PATH if one exists (regression: clap rejects unknown subcommands with exit code 2
    /// before main()'s own dispatch logic ever runs, so plugin dispatch was dead code
    /// without this catch-all variant — see plugin_dispatch_tests.rs).
    #[command(external_subcommand)]
    External(Vec<OsString>),
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive(if cli.verbose {
            tracing::Level::DEBUG.into()
        } else {
            tracing::Level::WARN.into()
        }))
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    if cli.list_plugins {
        cmd::plugin::list_plugins();
        return Ok(());
    }

    match cli.command {
        Some(Commands::Mcp(cmd)) => cmd.run().await,
        Some(Commands::Compile(cmd)) => cmd.run().await,
        Some(Commands::Exec(cmd)) => cmd.run().await,
        Some(Commands::Query(cmd)) => cmd.run().await,
        Some(Commands::Doc(cmd)) => cmd.run().await,
        Some(Commands::Tool(cmd)) => cmd.run().await,
        Some(Commands::Init(cmd)) => cmd.run().await,
        Some(Commands::Install(cmd)) => cmd.run().await,
        Some(Commands::Benchmark(cmd)) => cmd.run().await,
        Some(Commands::Skill(cmd)) => cmd.run().await,
        Some(Commands::External(args)) => {
            let name = args
                .first()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            let rest: Vec<String> = args
                .iter()
                .skip(1)
                .map(|s| s.to_string_lossy().into_owned())
                .collect();
            cmd::plugin::try_dispatch_plugin(&name, &rest)?;
            eprintln!("Run `iris-agentic-dev --help` for usage.");
            std::process::exit(1);
        }
        None => {
            eprintln!("Run `iris-agentic-dev --help` for usage.");
            std::process::exit(1);
        }
    }
}
