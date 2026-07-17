use std::path::Path;

fn extract_frontmatter_field<'a>(content: &'a str, field: &str) -> Option<String> {
    let inside = content.strip_prefix("---")?.split("---").next()?;
    for line in inside.lines() {
        if let Some(val) = line.strip_prefix(&format!("{}:", field)) {
            return Some(val.trim().trim_matches('"').to_string());
        }
    }
    None
}

fn check_skill_md(path: &Path) -> Result<(), String> {
    let content =
        std::fs::read_to_string(path).map_err(|e| format!("{}: {}", path.display(), e))?;
    if extract_frontmatter_field(&content, "name").is_none() {
        return Err(format!("{}: missing 'name' in frontmatter", path.display()));
    }
    if extract_frontmatter_field(&content, "description").is_none() {
        return Err(format!(
            "{}: missing 'description' in frontmatter",
            path.display()
        ));
    }
    Ok(())
}

#[test]
fn all_skills_have_name_and_description() {
    let skills_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("skills")
        .join("skills");

    let mut errors = Vec::new();

    let entries = std::fs::read_dir(&skills_dir)
        .unwrap_or_else(|e| panic!("cannot read {}: {}", skills_dir.display(), e));

    for entry in entries.flatten() {
        if entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false) {
            let skill_md = entry.path().join("SKILL.md");
            if skill_md.exists() {
                if let Err(e) = check_skill_md(&skill_md) {
                    errors.push(e);
                }
            } else {
                errors.push(format!("{}: no SKILL.md found", entry.path().display()));
            }
        }
    }

    if !errors.is_empty() {
        panic!("frontmatter validation failures:\n{}", errors.join("\n"));
    }
}
