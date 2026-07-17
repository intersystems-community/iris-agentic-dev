use anyhow::Result;
use clap::{Args, Subcommand, ValueEnum};

#[derive(Args)]
pub struct SkillCommand {
    #[command(subcommand)]
    pub subcommand: SkillSubcommand,
}

#[derive(Subcommand)]
pub enum SkillSubcommand {
    /// Install the official InterSystems skill pack into AI agent directories
    Install(SkillInstallArgs),
    /// List skills in the official pack and their install status
    List(SkillListArgs),
    /// Show install paths and managed-by status for all local skill files
    Status,
}

#[derive(Args)]
pub struct SkillInstallArgs {
    /// Skill names to install (omit for full pack)
    pub skills: Vec<String>,

    /// Target agent(s)
    #[arg(long, default_value = "all-user-global")]
    pub agent: AgentTarget,

    /// Show what would be installed without writing files
    #[arg(long)]
    pub dry_run: bool,

    /// Overwrite user-authored skills
    #[arg(long)]
    pub force: bool,

    /// Mirror installed skills to a connected IRIS instance
    #[arg(long)]
    pub mirror_to_iris: bool,
}

#[derive(Args)]
pub struct SkillListArgs {
    /// Filter by agent
    #[arg(long)]
    pub agent: Option<AgentTarget>,
}

#[derive(Debug, Clone, ValueEnum, PartialEq, Eq)]
pub enum AgentTarget {
    /// Claude Code only
    #[value(name = "claude-code")]
    ClaudeCode,
    /// OpenCode only
    #[value(name = "opencode")]
    OpenCode,
    /// Copilot (repo-scoped, cwd must be a git repo)
    #[value(name = "copilot")]
    Copilot,
    /// All agents
    #[value(name = "all")]
    All,
    /// Claude Code + OpenCode (user-global; default)
    #[value(name = "all-user-global")]
    AllUserGlobal,
}

impl SkillCommand {
    pub async fn run(self) -> Result<()> {
        match self.subcommand {
            SkillSubcommand::Install(args) => run_install(args).await,
            SkillSubcommand::List(args) => run_list(args),
            SkillSubcommand::Status => run_status(),
        }
    }
}

async fn run_install(args: SkillInstallArgs) -> Result<()> {
    use iris_agentic_dev_core::skill_install::{install_skill, InstallOutcome, SkillPackInstaller};

    if args.mirror_to_iris {
        eprintln!(
            "warning: --mirror-to-iris is not yet implemented; install will proceed to file targets only"
        );
    }

    let installer = SkillPackInstaller::new();

    let skill_paths = installer.fetch_pack_manifest().await?;

    let skill_names_to_install: Vec<String> = if args.skills.is_empty() {
        skill_paths
            .iter()
            .filter_map(|p| p.split('/').next_back().map(|s| s.to_string()))
            .collect()
    } else {
        args.skills.clone()
    };

    let mut written = 0usize;
    let mut updated = 0usize;
    let mut skipped = 0usize;
    let mut failed = 0usize;

    for skill_path in &skill_paths {
        let skill_name = match skill_path.split('/').next_back() {
            Some(n) => n.to_string(),
            None => continue,
        };

        if !skill_names_to_install.contains(&skill_name) {
            continue;
        }

        let content = match installer.fetch_skill_content(skill_path).await {
            Ok(c) => c,
            Err(e) => {
                eprintln!("error: could not fetch {}: {}", skill_name, e);
                failed += 1;
                continue;
            }
        };

        let targets = build_targets(&skill_name, &args);

        let results = install_skill(&skill_name, &content, &targets, args.dry_run);

        for result in &results {
            let path_str = result.target.target_path.display();
            match &result.outcome {
                InstallOutcome::Written => {
                    println!("Installing {} → {} ... written", skill_name, path_str);
                    written += 1;
                }
                InstallOutcome::Updated => {
                    println!("Installing {} → {} ... updated", skill_name, path_str);
                    updated += 1;
                }
                InstallOutcome::Skipped => {
                    println!(
                        "Skipped: {} (user-authored — use --force to overwrite)",
                        path_str
                    );
                    skipped += 1;
                }
                InstallOutcome::Failed(msg) => {
                    eprintln!("Failed: {} — {}", path_str, msg);
                    failed += 1;
                }
            }
        }
    }

    println!();
    println!(
        "{} written, {} updated, {} skipped.",
        written, updated, skipped
    );

    if failed > 0 {
        std::process::exit(1);
    }
    Ok(())
}

fn build_targets(
    skill_name: &str,
    args: &SkillInstallArgs,
) -> Vec<iris_agentic_dev_core::skill_install::InstallTarget> {
    use iris_agentic_dev_core::skill_install::InstallTarget;

    let mut targets = Vec::new();
    match args.agent {
        AgentTarget::ClaudeCode | AgentTarget::AllUserGlobal | AgentTarget::All => {
            if let Some(t) = InstallTarget::for_claude_code(skill_name, None) {
                targets.push(t);
            }
        }
        _ => {}
    }
    match args.agent {
        AgentTarget::OpenCode | AgentTarget::AllUserGlobal | AgentTarget::All => {
            if let Some(t) = InstallTarget::for_opencode(skill_name, None) {
                targets.push(t);
            }
        }
        _ => {}
    }
    match args.agent {
        AgentTarget::Copilot | AgentTarget::All => {
            let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
            if !cwd.join(".git").exists() && !cwd.join(".github").exists() {
                eprintln!(
                    "error: COPILOT_NO_REPO — cwd is not a git repo; copilot install skipped"
                );
            } else {
                targets.push(InstallTarget::for_copilot(skill_name, &cwd));
                eprintln!(
                    "Note: .github/instructions/ is repo-scoped. Commit this directory to share with your team."
                );
            }
        }
        _ => {}
    }
    targets
}

fn run_list(args: SkillListArgs) -> Result<()> {
    use iris_agentic_dev_core::skill_install::InstallTarget;

    let skill_names = &[
        "objectscript-review",
        "objectscript-guardrails",
        "iris-sql",
        "iris-vector-ai",
        "objectscript-list-patterns",
        "objectscript-loop-patterns",
        "objectscript-sql-patterns",
        "objectscript-tdd",
        "objectscript-unit-test",
        "iris-connectivity",
        "iris-product-features",
        "iris-vector-graph",
        "iris-embedded-python",
        "iris-vector-rag",
    ];

    let show_cc = matches!(
        args.agent,
        None | Some(AgentTarget::ClaudeCode)
            | Some(AgentTarget::AllUserGlobal)
            | Some(AgentTarget::All)
    );
    let show_oc = matches!(
        args.agent,
        None | Some(AgentTarget::OpenCode)
            | Some(AgentTarget::AllUserGlobal)
            | Some(AgentTarget::All)
    );
    let show_co = matches!(
        args.agent,
        None | Some(AgentTarget::Copilot) | Some(AgentTarget::All)
    );

    println!(
        "{:<28} {:<16} {:<12} COPILOT",
        "SKILL", "CLAUDE CODE", "OPENCODE"
    );

    for name in skill_names {
        let cc = if show_cc {
            InstallTarget::for_claude_code(name, None)
                .map(|t| {
                    if t.target_path.exists() {
                        "installed"
                    } else {
                        "not installed"
                    }
                })
                .unwrap_or("n/a")
        } else {
            "n/a"
        };

        let oc = if show_oc {
            InstallTarget::for_opencode(name, None)
                .map(|t| {
                    if t.target_path.exists() {
                        "installed"
                    } else {
                        "not installed"
                    }
                })
                .unwrap_or("n/a")
        } else {
            "n/a"
        };

        let co = "n/a";
        let _ = show_co;

        println!("{:<28} {:<16} {:<12} {}", name, cc, oc, co);
    }

    Ok(())
}

fn run_status() -> Result<()> {
    use iris_agentic_dev_core::skill_install::{is_managed, InstallTarget};

    let skill_names = &[
        "objectscript-review",
        "objectscript-guardrails",
        "iris-sql",
        "iris-vector-ai",
        "objectscript-list-patterns",
    ];

    for name in skill_names {
        for target in [
            InstallTarget::for_claude_code(name, None),
            InstallTarget::for_opencode(name, None),
        ]
        .into_iter()
        .flatten()
        {
            let path = &target.target_path;
            if path.exists() {
                let managed = if is_managed(path) {
                    "managed"
                } else {
                    "user-authored"
                };
                println!("{}: {} ({})", name, path.display(), managed);
            }
        }
    }

    Ok(())
}
