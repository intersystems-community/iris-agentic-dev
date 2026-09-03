// 101-nopws-connectivity: Skill keyword presence test (FR-015).
// Verifies the nopws-setup skill file exists and contains required keywords.

use std::path::PathBuf;

fn skill_path() -> PathBuf {
    let root = std::env::var("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."));
    // Navigate from crates/iris-agentic-dev-core to repo root
    root.parent()
        .and_then(|p| p.parent())
        .map(|p| p.join("skills/skills/iris-agentic-dev/nopws-setup/SKILL.md"))
        .unwrap_or_else(|| PathBuf::from("skills/skills/iris-agentic-dev/nopws-setup/SKILL.md"))
}

#[test]
fn test_nopws_skill_file_exists() {
    let path = skill_path();
    assert!(
        path.exists(),
        "nopws-setup skill file must exist at: {}",
        path.display()
    );
}

#[test]
fn test_nopws_skill_contains_required_keywords() {
    let path = skill_path();
    if !path.exists() {
        // Skip if skill file not yet created (pre-Phase 7)
        return;
    }
    let content = std::fs::read_to_string(&path).expect("must read skill file");

    let required_keywords = [
        "NoPWS",
        "No Private Web Server",
        "AI branch",
        "connection refused",
        "webgateway sidecar",
        "irishealth-ai",
    ];

    for kw in &required_keywords {
        assert!(
            content.contains(kw),
            "nopws-setup skill must contain keyword: '{kw}'"
        );
    }
}

#[test]
fn test_nopws_skill_is_under_300_lines() {
    let path = skill_path();
    if !path.exists() {
        return;
    }
    let content = std::fs::read_to_string(&path).expect("must read skill file");
    let line_count = content.lines().count();
    assert!(
        line_count <= 300,
        "nopws-setup skill must be under 300 lines, got: {line_count}"
    );
}

#[test]
fn test_nopws_skill_contains_detection_commands() {
    let path = skill_path();
    if !path.exists() {
        return;
    }
    let content = std::fs::read_to_string(&path).expect("must read skill file");
    assert!(
        content.contains("iris.cpf") || content.contains("WebServer"),
        "skill must describe NoPWS detection via iris.cpf"
    );
}

#[test]
fn test_nopws_skill_contains_password_clearing() {
    let path = skill_path();
    if !path.exists() {
        return;
    }
    let content = std::fs::read_to_string(&path).expect("must read skill file");
    assert!(
        content.contains("ChangePassword") || content.contains("_SYSTEM"),
        "skill must describe first-boot password clearing"
    );
}
