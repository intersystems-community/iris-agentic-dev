use iris_agentic_dev_core::skill_install::{
    install_skill, is_managed, InstallOutcome, InstallTarget, SkillPackInstaller,
};
use tempfile::TempDir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const MOCK_TOML: &str = r#"
[provides]
skills = ["skills/skills/objectscript-review", "skills/skills/iris-sql"]
"#;

const MOCK_SKILL_MD: &str = r#"---
name: objectscript-review
description: Reviews ObjectScript code for common LLM mistakes
managed_by: "iris-agentic-dev"
---
# objectscript-review

Use this skill when writing or reviewing ObjectScript code.
"#;

const MOCK_IRIS_SQL_MD: &str = r#"---
name: iris-sql
description: SQL patterns for IRIS
managed_by: "iris-agentic-dev"
---
# iris-sql

Use for SQL queries in IRIS.
"#;

// T019: end-to-end install flow — wiremock replaces live GitHub
// Tests: manifest fetch → skill content fetch → file write → managed_by marker present
#[tokio::test]
async fn e2e_install_from_manifest_to_files() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path(
            "/intersystems-community/iris-agentic-dev/HEAD/iris-agentic-dev.toml",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_string(MOCK_TOML))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path(
            "/intersystems-community/iris-agentic-dev/HEAD/skills/skills/objectscript-review/SKILL.md",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_string(MOCK_SKILL_MD))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path(
            "/intersystems-community/iris-agentic-dev/HEAD/skills/skills/iris-sql/SKILL.md",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_string(MOCK_IRIS_SQL_MD))
        .mount(&server)
        .await;

    std::env::set_var("GITHUB_RAW_BASE_URL", server.uri());

    let installer = SkillPackInstaller::new();
    let skill_paths = installer
        .fetch_pack_manifest()
        .await
        .expect("fetch_pack_manifest failed");

    assert_eq!(skill_paths.len(), 2);

    let tmp_home = TempDir::new().unwrap();
    let tmp_config = TempDir::new().unwrap();

    for skill_path in &skill_paths {
        let skill_name = skill_path.split('/').next_back().unwrap();
        let content = installer
            .fetch_skill_content(skill_path)
            .await
            .expect("fetch_skill_content failed");

        let targets = vec![
            InstallTarget::for_claude_code(skill_name, Some(tmp_home.path())).unwrap(),
            InstallTarget::for_opencode(skill_name, Some(tmp_config.path())).unwrap(),
        ];

        let results = install_skill(skill_name, &content, &targets, false);

        for result in &results {
            assert_eq!(
                result.outcome,
                InstallOutcome::Written,
                "expected Written for {:?}",
                result.target.target_path
            );
            assert!(
                result.target.target_path.exists(),
                "file not created: {}",
                result.target.target_path.display()
            );
            assert!(
                is_managed(&result.target.target_path),
                "managed_by marker missing in {}",
                result.target.target_path.display()
            );
        }
    }

    std::env::remove_var("GITHUB_RAW_BASE_URL");
}

// T019-live: live GitHub — only works after iris-agentic-dev.toml is on main branch
// Run: cargo test -p iris-agentic-dev-core --test test_skill_install_e2e -- --ignored --test-threads=1
#[tokio::test]
#[ignore]
async fn e2e_install_from_live_github() {
    let tmp_home = TempDir::new().unwrap();
    let tmp_config = TempDir::new().unwrap();

    let installer = SkillPackInstaller::new();
    let skill_paths = match installer.fetch_pack_manifest().await {
        Ok(paths) => paths,
        Err(e) if e.to_string().contains("404") => {
            // iris-agentic-dev.toml not yet on main branch — skip gracefully
            eprintln!("SKIP: iris-agentic-dev.toml not on main branch yet ({})", e);
            return;
        }
        Err(e) => panic!("fetch_pack_manifest failed: {}", e),
    };

    assert!(!skill_paths.is_empty(), "pack manifest returned no skills");

    let first_path = &skill_paths[0];
    let skill_name = first_path.split('/').next_back().unwrap();

    let content = installer
        .fetch_skill_content(first_path)
        .await
        .expect("fetch_skill_content failed");

    let targets = vec![
        InstallTarget::for_claude_code(skill_name, Some(tmp_home.path())).unwrap(),
        InstallTarget::for_opencode(skill_name, Some(tmp_config.path())).unwrap(),
    ];

    let results = install_skill(skill_name, &content, &targets, false);

    for result in &results {
        assert_eq!(
            result.outcome,
            InstallOutcome::Written,
            "expected Written for {:?}",
            result.target.target_path
        );
        assert!(
            result.target.target_path.exists(),
            "file not created: {}",
            result.target.target_path.display()
        );
        assert!(
            is_managed(&result.target.target_path),
            "managed_by marker missing in {}",
            result.target.target_path.display()
        );
    }
}

// T036: live IRIS mirror — requires iris-dev-iris container on port 52780
// Run: IRIS_HOST=localhost IRIS_WEB_PORT=52780 cargo test -p iris-agentic-dev-core --test test_skill_install_e2e -- --ignored --test-threads=1
#[tokio::test]
#[ignore]
async fn e2e_mirror_to_iris() {
    let iris_host = std::env::var("IRIS_HOST").unwrap_or_else(|_| "localhost".to_string());
    let iris_port = std::env::var("IRIS_WEB_PORT").unwrap_or_else(|_| "52780".to_string());
    println!("IRIS target: {}:{}", iris_host, iris_port);
    // Full implementation pending T038
    println!("T036 stub — mirror_to_iris not yet implemented");
}
