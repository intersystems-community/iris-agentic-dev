use std::path::Path;

pub fn install_base(repo_dir: &Path) -> std::path::PathBuf {
    repo_dir.join(".github").join("instructions")
}

pub fn wrap_content(skill_name: &str, body: &str) -> String {
    let description = extract_description(body).unwrap_or_else(|| skill_name.to_string());
    let stripped = strip_frontmatter(body);
    format!(
        "---\nname: \"{}\"\ndescription: \"{}\"\napplyTo: \"**\"\nmanaged_by: \"iris-agentic-dev\"\n---\n{}",
        skill_name, description, stripped
    )
}

fn extract_description(content: &str) -> Option<String> {
    let inside = content.strip_prefix("---")?.split("---").next()?;
    for line in inside.lines() {
        if let Some(val) = line.strip_prefix("description:") {
            return Some(val.trim().trim_matches('"').to_string());
        }
    }
    None
}

fn strip_frontmatter(content: &str) -> &str {
    if let Some(rest) = content.strip_prefix("---") {
        if let Some(pos) = rest.find("\n---") {
            return rest[pos + 4..].trim_start_matches('\n');
        }
    }
    content
}
