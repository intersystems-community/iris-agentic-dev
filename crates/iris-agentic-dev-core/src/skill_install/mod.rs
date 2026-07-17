pub mod claude_code;
pub mod copilot;
pub mod opencode;

use anyhow::Result;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentKind {
    ClaudeCode,
    OpenCode,
    Copilot,
}

#[derive(Debug, Clone)]
pub struct InstallTarget {
    pub agent: AgentKind,
    pub skill_name: String,
    pub target_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallOutcome {
    Written,
    Updated,
    Skipped,
    Failed(String),
}

#[derive(Debug, Clone)]
pub struct SkillInstallResult {
    pub target: InstallTarget,
    pub outcome: InstallOutcome,
}

impl SkillInstallResult {
    pub fn agent(&self) -> &AgentKind {
        &self.target.agent
    }
}

impl InstallTarget {
    pub fn for_claude_code(skill_name: &str, home_override: Option<&Path>) -> Option<Self> {
        let base = claude_code::install_base(home_override)?;
        Some(Self {
            agent: AgentKind::ClaudeCode,
            skill_name: skill_name.to_string(),
            target_path: base.join(skill_name).join("SKILL.md"),
        })
    }

    pub fn for_opencode(skill_name: &str, config_override: Option<&Path>) -> Option<Self> {
        let base = opencode::install_base(config_override)?;
        Some(Self {
            agent: AgentKind::OpenCode,
            skill_name: skill_name.to_string(),
            target_path: base.join(skill_name).join("SKILL.md"),
        })
    }

    pub fn for_copilot(skill_name: &str, repo_dir: &Path) -> Self {
        let target_path = repo_dir
            .join(".github")
            .join("instructions")
            .join(format!("{}.instructions.md", skill_name));
        Self {
            agent: AgentKind::Copilot,
            skill_name: skill_name.to_string(),
            target_path,
        }
    }
}

pub fn is_managed(path: &Path) -> bool {
    use std::io::Read;
    let Ok(mut f) = std::fs::File::open(path) else {
        return false;
    };
    let mut buf = [0u8; 512];
    let n = f.read(&mut buf).unwrap_or(0);
    let preview = std::str::from_utf8(&buf[..n]).unwrap_or("");
    preview.contains(r#"managed_by: "iris-agentic-dev""#)
}

pub fn install_skill(
    skill_name: &str,
    content: &str,
    targets: &[InstallTarget],
    dry_run: bool,
) -> Vec<SkillInstallResult> {
    targets
        .iter()
        .map(|target| {
            let outcome = write_skill_file(target, skill_name, content, dry_run);
            SkillInstallResult {
                target: target.clone(),
                outcome,
            }
        })
        .collect()
}

/// Inject `managed_by: "iris-agentic-dev"` into the YAML frontmatter so
/// `is_managed()` can recognise files written by this installer and overwrite
/// them on upgrade.  If no frontmatter is present the marker is prepended.
fn inject_managed_by(content: &str) -> String {
    const MARKER: &str = r#"managed_by: "iris-agentic-dev""#;
    if content.contains(MARKER) {
        return content.to_string();
    }
    if let Some(rest) = content.strip_prefix("---\n") {
        if let Some(close) = rest.find("\n---\n") {
            let frontmatter = &rest[..close];
            let body = &rest[close + 5..];
            return format!("---\n{}\n{}\n---\n{}", frontmatter, MARKER, body);
        }
    }
    format!("---\n{}\n---\n{}", MARKER, content)
}

fn write_skill_file(
    target: &InstallTarget,
    skill_name: &str,
    content: &str,
    dry_run: bool,
) -> InstallOutcome {
    let path = &target.target_path;

    let already_exists = path.exists();

    if already_exists && !is_managed(path) {
        return InstallOutcome::Skipped;
    }

    if dry_run {
        return if already_exists {
            InstallOutcome::Updated
        } else {
            InstallOutcome::Written
        };
    }

    let final_content = match target.agent {
        AgentKind::Copilot => copilot::wrap_content(skill_name, content),
        _ => inject_managed_by(content),
    };

    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            return InstallOutcome::Failed(e.to_string());
        }
    }

    match std::fs::write(path, final_content) {
        Ok(()) => {
            if already_exists {
                InstallOutcome::Updated
            } else {
                InstallOutcome::Written
            }
        }
        Err(e) => InstallOutcome::Failed(e.to_string()),
    }
}

pub struct InstalledSkill {
    pub name: String,
    pub content: String,
}

pub fn mirror_to_iris(
    _skills: &[InstalledSkill],
    iris: Option<&crate::iris::connection::IrisConnection>,
) -> anyhow::Result<()> {
    let _conn =
        iris.ok_or_else(|| anyhow::anyhow!("IRIS_UNREACHABLE: no connection configured"))?;
    // Full implementation in T038
    anyhow::bail!("IRIS_UNREACHABLE: mirror_to_iris not yet implemented")
}

pub struct SkillPackInstaller {
    raw_base: String,
}

impl Default for SkillPackInstaller {
    fn default() -> Self {
        Self::new()
    }
}

impl SkillPackInstaller {
    pub fn new() -> Self {
        let raw_base = std::env::var("GITHUB_RAW_BASE_URL")
            .unwrap_or_else(|_| "https://raw.githubusercontent.com".to_string());
        Self { raw_base }
    }

    pub async fn fetch_pack_manifest(&self) -> Result<Vec<String>> {
        let client = reqwest::Client::builder()
            .user_agent("iris-agentic-dev/0.3.1")
            .build()?;
        let url = format!(
            "{}/intersystems-community/iris-agentic-dev/HEAD/iris-agentic-dev.toml",
            self.raw_base
        );
        let text = fetch_text(&url, &client).await?;
        let manifest: TomlManifest = toml::from_str(&text)?;
        let skills = manifest.provides.map(|p| p.skills).unwrap_or_default();
        Ok(skills)
    }

    pub async fn fetch_skill_content(&self, skill_path: &str) -> Result<String> {
        let client = reqwest::Client::builder()
            .user_agent("iris-agentic-dev/0.3.1")
            .build()?;
        let url = format!(
            "{}/intersystems-community/iris-agentic-dev/HEAD/{}/SKILL.md",
            self.raw_base, skill_path
        );
        fetch_text(&url, &client).await
    }
}

async fn fetch_text(url: &str, client: &reqwest::Client) -> Result<String> {
    let resp = client.get(url).send().await?;
    if !resp.status().is_success() {
        anyhow::bail!("HTTP {} fetching {}", resp.status(), url);
    }
    Ok(resp.text().await?)
}

#[derive(serde::Deserialize)]
struct TomlManifest {
    provides: Option<TomlProvides>,
}

#[derive(serde::Deserialize)]
struct TomlProvides {
    #[serde(default)]
    skills: Vec<String>,
}
