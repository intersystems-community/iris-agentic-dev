use anyhow::{Context, Result};
use reqwest::Client;

pub struct SkillRegistry {
    skills: Vec<Skill>,
    kb_items: Vec<KbItem>,
}

pub struct Skill {
    pub name: String,
    pub description: String,
    pub content: String,
    pub source_repo: String,
}

pub struct KbItem {
    pub title: String,
    pub content: String,
    pub source_repo: String,
}

impl Default for SkillRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl SkillRegistry {
    pub fn new() -> Self {
        Self {
            skills: vec![],
            kb_items: vec![],
        }
    }

    pub fn list_skills(&self) -> &[Skill] {
        &self.skills
    }
    pub fn list_kb_items(&self) -> &[KbItem] {
        &self.kb_items
    }

    pub async fn load_from_github(&mut self, owner_repo: &str) -> Result<()> {
        let client = Client::builder()
            .user_agent("iris-agentic-dev/0.3.1")
            .build()?;

        let (owner, repo) = owner_repo
            .split_once('/')
            .with_context(|| format!("invalid owner/repo: {}", owner_repo))?;

        let manifest = self.fetch_manifest(owner, repo, &client).await?;

        for skill_path in &manifest.provides_skills {
            let skill_md_url = format!(
                "https://raw.githubusercontent.com/{}/{}/HEAD/{}/SKILL.md",
                owner, repo, skill_path
            );
            if let Ok(content) = fetch_text(&skill_md_url, &client).await {
                if let Some(name) = extract_frontmatter_field(&content, "name") {
                    let description = extract_frontmatter_field(&content, "description")
                        .unwrap_or_else(|| name.clone());
                    self.skills.push(Skill {
                        name,
                        description,
                        content,
                        source_repo: owner_repo.to_string(),
                    });
                }
            }
        }

        for kb_path in &manifest.provides_kb_items {
            let kb_url = format!(
                "https://raw.githubusercontent.com/{}/{}/HEAD/{}",
                owner, repo, kb_path
            );
            if let Ok(content) = fetch_text(&kb_url, &client).await {
                let title = extract_frontmatter_field(&content, "title")
                    .or_else(|| extract_h1_title(&content))
                    .unwrap_or_else(|| kb_path.clone());
                self.kb_items.push(KbItem {
                    title,
                    content,
                    source_repo: owner_repo.to_string(),
                });
            }
        }

        tracing::info!(
            "Loaded {} skills + {} KB items from {}",
            self.skills.len(),
            self.kb_items.len(),
            owner_repo
        );
        Ok(())
    }

    async fn fetch_manifest(&self, owner: &str, repo: &str, client: &Client) -> Result<Manifest> {
        let url = format!(
            "https://raw.githubusercontent.com/{}/{}/HEAD/iris-agentic-dev.toml",
            owner, repo
        );
        let text = fetch_text(&url, client)
            .await
            .with_context(|| format!("no iris-agentic-dev.toml found in {}/{}", owner, repo))?;
        let manifest: TomlManifest = toml::from_str(&text)
            .with_context(|| format!("invalid iris-agentic-dev.toml in {}/{}", owner, repo))?;
        Ok(Manifest {
            provides_skills: manifest
                .provides
                .as_ref()
                .map(|p| p.skills.clone())
                .unwrap_or_default(),
            provides_kb_items: manifest
                .provides
                .as_ref()
                .map(|p| p.kb_items.clone())
                .unwrap_or_default(),
        })
    }
}

struct Manifest {
    provides_skills: Vec<String>,
    provides_kb_items: Vec<String>,
}

#[derive(serde::Deserialize)]
struct TomlManifest {
    provides: Option<TomlProvides>,
}

#[derive(serde::Deserialize)]
struct TomlProvides {
    #[serde(default)]
    skills: Vec<String>,
    #[serde(default)]
    kb_items: Vec<String>,
}

async fn fetch_text(url: &str, client: &Client) -> Result<String> {
    let resp = client.get(url).send().await?;
    if !resp.status().is_success() {
        anyhow::bail!("HTTP {} fetching {}", resp.status(), url);
    }
    Ok(resp.text().await?)
}

fn extract_frontmatter_field(content: &str, field: &str) -> Option<String> {
    let inside = content.strip_prefix("---")?.split("---").next()?;
    for line in inside.lines() {
        if let Some(val) = line.strip_prefix(&format!("{}:", field)) {
            return Some(val.trim().trim_matches('"').to_string());
        }
    }
    None
}

fn extract_h1_title(content: &str) -> Option<String> {
    content
        .lines()
        .find(|l| l.starts_with("# "))
        .map(|l| l.trim_start_matches("# ").to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_frontmatter_field_found() {
        let md = "---\nname: my-skill\ndescription: does stuff\n---\n# Title";
        assert_eq!(
            extract_frontmatter_field(md, "name"),
            Some("my-skill".to_string())
        );
    }

    #[test]
    fn test_extract_frontmatter_field_not_found() {
        let md = "---\nname: skill\n---\n# Title";
        assert!(extract_frontmatter_field(md, "version").is_none());
    }

    #[test]
    fn test_extract_frontmatter_field_no_frontmatter() {
        let md = "# Title\n\nSome content";
        assert!(extract_frontmatter_field(md, "name").is_none());
    }

    #[test]
    fn test_extract_frontmatter_field_quoted_value() {
        let md = "---\ndescription: \"quoted value\"\n---";
        assert_eq!(
            extract_frontmatter_field(md, "description"),
            Some("quoted value".to_string())
        );
    }

    #[test]
    fn test_extract_h1_title_found() {
        let md = "---\nname: x\n---\n# My Skill Title\n\nContent here.";
        assert_eq!(extract_h1_title(md), Some("My Skill Title".to_string()));
    }

    #[test]
    fn test_extract_h1_title_not_found() {
        let md = "No h1 here\n## h2 only";
        assert!(extract_h1_title(md).is_none());
    }

    #[test]
    fn test_extract_h1_title_ignores_h2() {
        let md = "## Not h1\n# Actual h1";
        assert_eq!(extract_h1_title(md), Some("Actual h1".to_string()));
    }

    #[test]
    fn test_extract_frontmatter_field_trimmed_whitespace() {
        let md = "---\nname:   padded-value  \n---";
        assert_eq!(
            extract_frontmatter_field(md, "name"),
            Some("padded-value".to_string())
        );
    }

    #[test]
    fn test_extract_frontmatter_field_second_field() {
        let md = "---\nname: first\ndescription: second field\n---";
        assert_eq!(
            extract_frontmatter_field(md, "description"),
            Some("second field".to_string())
        );
    }

    #[test]
    fn test_extract_frontmatter_field_partial_prefix_no_match() {
        // "names:" should not match "name:"
        let md = "---\nnames: wrong\n---";
        assert!(extract_frontmatter_field(md, "name").is_none());
    }

    #[test]
    fn test_extract_h1_title_first_h1_wins() {
        let md = "# First Title\n# Second Title";
        assert_eq!(extract_h1_title(md), Some("First Title".to_string()));
    }

    #[test]
    fn test_extract_h1_title_empty_h1() {
        let md = "# \nsome content";
        assert_eq!(extract_h1_title(md), Some(String::new()));
    }

    #[test]
    fn test_skill_registry_new_is_empty() {
        let registry = SkillRegistry::new();
        assert!(registry.list_skills().is_empty());
        assert!(registry.list_kb_items().is_empty());
    }

    #[test]
    fn test_skill_registry_default_is_empty() {
        let registry = SkillRegistry::default();
        assert!(registry.list_skills().is_empty());
        assert!(registry.list_kb_items().is_empty());
    }

    #[test]
    fn test_toml_manifest_deserialize_full() {
        let toml_str = r#"
[provides]
skills = ["skills/foo", "skills/bar"]
kb_items = ["kb/item1.md"]
"#;
        let manifest: TomlManifest = toml::from_str(toml_str).unwrap();
        let provides = manifest.provides.unwrap();
        assert_eq!(provides.skills, vec!["skills/foo", "skills/bar"]);
        assert_eq!(provides.kb_items, vec!["kb/item1.md"]);
    }

    #[test]
    fn test_toml_manifest_deserialize_empty_provides() {
        let toml_str = "[provides]\n";
        let manifest: TomlManifest = toml::from_str(toml_str).unwrap();
        let provides = manifest.provides.unwrap();
        assert!(provides.skills.is_empty());
        assert!(provides.kb_items.is_empty());
    }

    #[test]
    fn test_toml_manifest_deserialize_no_provides() {
        let toml_str = "# no provides section\n";
        let manifest: TomlManifest = toml::from_str(toml_str).unwrap();
        assert!(manifest.provides.is_none());
    }
}
