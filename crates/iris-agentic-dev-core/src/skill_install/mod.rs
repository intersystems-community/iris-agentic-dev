pub mod claude_code;
pub mod copilot;
pub mod opencode;

use anyhow::Result;

use crate::skills::bundled::{embedded_tier, parse_skill_md, SkillTier};
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
    let Ok(f) = std::fs::File::open(path) else {
        return false;
    };
    // The marker goes at the end of the frontmatter, which for some skills runs well past the
    // first few hundred bytes, so read the whole frontmatter (bounded) rather than a fixed prefix.
    let mut buf = Vec::new();
    let _ = f.take(64 * 1024).read_to_end(&mut buf);
    let text = String::from_utf8_lossy(&buf);
    let Some(rest) = text.strip_prefix("---\n") else {
        return false;
    };
    let frontmatter = rest.find("\n---").map_or(&rest[..0], |end| &rest[..end]);
    frontmatter.contains(r#"managed_by: "iris-agentic-dev""#)
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

/// A skill name becomes a file or directory name. One with a path separator, a `..` or a control
/// character would land somewhere else.
fn valid_skill_name(name: &str) -> Result<(), String> {
    if name.is_empty()
        || name == ".."
        || name.contains(['/', '\\'])
        || name.chars().any(char::is_control)
    {
        return Err(format!(
            "refusing skill name {name:?}: not a plain file name"
        ));
    }
    Ok(())
}

/// A Copilot target sits inside the repository, at `<repo>/.github/instructions/<name>`. If a
/// directory on the way is a symlink the repository committed, the file would be created outside
/// it, so the target's real parent must still be under the real repository directory (FR-026).
fn stays_in_repository(target: &InstallTarget) -> Result<(), String> {
    if target.agent != AgentKind::Copilot {
        return Ok(());
    }
    let path = &target.target_path;
    let repo = path
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .ok_or_else(|| format!("{} is not under a repository", path.display()))?;
    let real_repo = repo.canonicalize().map_err(|e| e.to_string())?;
    // The deepest part of the parent that exists now: what a later `create_dir_all` builds on.
    let mut existing = path.parent();
    while let Some(dir) = existing {
        if dir.exists() {
            break;
        }
        existing = dir.parent();
    }
    let real_parent = existing
        .ok_or_else(|| format!("{} has no parent", path.display()))?
        .canonicalize()
        .map_err(|e| e.to_string())?;
    if real_parent.starts_with(&real_repo) {
        Ok(())
    } else {
        Err(format!(
            "{} resolves to {}, outside the repository {}; not writing there",
            path.display(),
            real_parent.display(),
            real_repo.display()
        ))
    }
}

fn write_skill_file(
    target: &InstallTarget,
    skill_name: &str,
    content: &str,
    dry_run: bool,
) -> InstallOutcome {
    let path = &target.target_path;

    if let Err(e) = valid_skill_name(skill_name) {
        return InstallOutcome::Failed(e);
    }

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

    if let Err(e) = stays_in_repository(target) {
        return InstallOutcome::Failed(e);
    }
    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            return InstallOutcome::Failed(e.to_string());
        }
    }

    match crate::iris::private_file::write_atomic(path, final_content.as_bytes(), 0o644) {
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

/// What a `skill install` run asks for. See `docs/adr/0001-skill-tiers.md`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallMode {
    /// No names and no `--all`: the core tier.
    Core,
    /// `--all`: core and extra.
    All,
    /// Skills named on the command line, whatever their tier.
    Named(Vec<String>),
}

impl InstallMode {
    /// Whether a skill of this tier belongs in a manifest-driven install. A skill with no tier
    /// counts as extra. Named installs take every tier, so this is only asked of `Core` and `All`.
    pub fn wants(&self, tier: Option<SkillTier>) -> bool {
        match self {
            InstallMode::Core => tier == Some(SkillTier::Core),
            InstallMode::All => tier != Some(SkillTier::Internal),
            InstallMode::Named(_) => true,
        }
    }
}

fn path_name(path: &str) -> Option<&str> {
    path.rsplit('/').next().filter(|n| !n.is_empty())
}

/// The `(name, repo path)` pairs to fetch. A bare install skips manifest entries this binary
/// knows are not core, and fetches the ones it does not know, because only their frontmatter can
/// say. A named skill missing from the manifest is fetched from `skills/skills/<name>`, which is
/// how an internal skill installs.
pub fn select_paths(manifest: &[String], mode: &InstallMode) -> Vec<(String, String)> {
    match mode {
        InstallMode::Named(names) => names
            .iter()
            .map(|n| {
                let path = manifest
                    .iter()
                    .find(|p| path_name(p) == Some(n.as_str()))
                    .cloned()
                    .unwrap_or_else(|| format!("skills/skills/{n}"));
                (n.clone(), path)
            })
            .collect(),
        _ => manifest
            .iter()
            .filter_map(|p| Some((path_name(p)?.to_string(), p.clone())))
            .filter(|(n, _)| match embedded_tier(n) {
                Some(t) => mode.wants(Some(t)),
                None => true,
            })
            .collect(),
    }
}

/// A fetched skill's tier: its own frontmatter, or this binary's copy when the fetched file has
/// none (a `HEAD` from before tiers).
pub fn effective_tier(name: &str, content: &str) -> Option<SkillTier> {
    parse_skill_md(content, name)
        .and_then(|s| s.tier)
        .or_else(|| embedded_tier(name))
}

/// Whether to install a skill once its content is fetched.
pub fn keep_after_fetch(mode: &InstallMode, name: &str, content: &str) -> bool {
    mode.wants(effective_tier(name, content))
}

/// Managed installs, under `<base>/<name>/SKILL.md`, of skills this binary ships that `mode`
/// would not install: what a bare install leaves from an older full-pack install. User-authored
/// files and skills this binary does not ship are never leftovers. Named installs have none.
pub fn find_leftovers(bases: &[PathBuf], mode: &InstallMode) -> Vec<(String, PathBuf)> {
    if matches!(mode, InstallMode::Named(_)) {
        return Vec::new();
    }
    let mut out = Vec::new();
    for base in bases {
        for name in crate::skills::bundled::embedded_skill_dirs() {
            if mode.wants(embedded_tier(name)) {
                continue;
            }
            let path = base.join(name).join("SKILL.md");
            if path.is_file() && is_managed(&path) {
                out.push((name.to_string(), path));
            }
        }
    }
    out.sort();
    out
}

/// Delete each leftover file, then its directory if nothing else is in it. `dry_run` deletes
/// nothing and reports each as `Ok`.
pub fn prune_leftovers(
    leftovers: &[(String, PathBuf)],
    dry_run: bool,
) -> Vec<(PathBuf, std::io::Result<()>)> {
    leftovers
        .iter()
        .map(|(_, path)| {
            if dry_run {
                return (path.clone(), Ok(()));
            }
            let r = std::fs::remove_file(path).map(|_| {
                if let Some(dir) = path.parent() {
                    // Fails, harmlessly, when the directory still holds other files.
                    let _ = std::fs::remove_dir(dir);
                }
            });
            (path.clone(), r)
        })
        .collect()
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
