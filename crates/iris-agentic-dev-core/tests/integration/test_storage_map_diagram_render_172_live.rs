//! T047: Render check for Mermaid diagrams using mmdc (spec 172 US2).
//!
//! Run: `IRIS_HOST=localhost IRIS_WEB_PORT=52780 cargo test --features testing --test integration \
//!   test_storage_map_diagram_render_172 -- --test-threads=1 --include-ignored`

use crate::live_rig_171::{rig, Where};
use crate::storage172_rig::{drop_fixture, load_fixture, Fixture};
use iris_agentic_dev_core::tools::storage_map::diagram::mermaid;
use iris_agentic_dev_core::tools::storage_map::model::{from_tree, StorageModel};
use iris_agentic_dev_core::tools::storage_map::tree::{parse_body, split_class};
use iris_agentic_dev_core::tools::Toolset;
use std::fs;
use std::process::Command;

fn model_from_class_text(class: &str, text: &str) -> StorageModel {
    let spans = split_class(text).expect("Failed to split class");
    let span = spans.first().expect("No storage block found");
    let nodes = parse_body(&text[span.body_start..span.body_end]).expect("Failed to parse body");
    from_tree(class, &span.name, &nodes)
}

fn check_mmdc_available() -> bool {
    let allow_skip = std::env::var("IAD_ALLOW_SKIP").is_ok();

    // Check if mmdc exists
    let mmdc_check = Command::new("mmdc").arg("--version").output();

    match mmdc_check {
        Err(_) => {
            if allow_skip {
                eprintln!("SKIP: mmdc not found");
                false
            } else {
                panic!("mmdc is required for this test. Install with: npm install -g @mermaid-js/mermaid-cli\nOr set IAD_ALLOW_SKIP=1 to skip this test.");
            }
        }
        Ok(output) if !output.status.success() => {
            if allow_skip {
                eprintln!("SKIP: mmdc not working");
                false
            } else {
                panic!("mmdc failed version check");
            }
        }
        Ok(_) => true,
    }
}

fn render_mermaid(mmd: &str) -> Result<(), String> {
    let temp_dir = std::env::temp_dir();
    let input_path = temp_dir.join(format!("test_mermaid_{}.mmd", std::process::id()));
    let output_path = temp_dir.join(format!("test_mermaid_{}.svg", std::process::id()));

    fs::write(&input_path, mmd).map_err(|e| format!("Failed to write temp file: {}", e))?;

    // The test process has an empty HOME (testing::isolate_process); mmdc's Puppeteer keeps its
    // Chrome under the real one, so point the child at it.
    let mut cmd = Command::new("mmdc");
    if let Some(home) = iris_agentic_dev_core::testing::real_home() {
        cmd.env("PUPPETEER_CACHE_DIR", home.join(".cache/puppeteer"));
    }
    let output = cmd
        .arg("-i")
        .arg(&input_path)
        .arg("-o")
        .arg(&output_path)
        .output()
        .map_err(|e| format!("Failed to run mmdc: {}", e))?;

    // Clean up temp files
    let _ = fs::remove_file(&input_path);
    let _ = fs::remove_file(&output_path);

    if !output.status.success() {
        let stderr_str = String::from_utf8_lossy(&output.stderr);
        // If Chrome/Puppeteer is missing and IAD_ALLOW_SKIP is set, skip rather than fail
        if stderr_str.contains("Could not find Chrome") && std::env::var("IAD_ALLOW_SKIP").is_ok() {
            eprintln!("SKIP: mmdc needs Chrome/Puppeteer installed");
            return Ok(());
        }
        return Err(format!(
            "mmdc failed with status {}: {}",
            output.status, stderr_str
        ));
    }

    Ok(())
}

#[tokio::test]
#[ignore]
async fn render_all_fixtures() {
    if !check_mmdc_available() {
        return;
    }

    let r = rig(&Where::live(), Toolset::Merged).await;

    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/storage172/classes");
    let mut fixtures: Vec<String> = std::fs::read_dir(&dir)
        .expect("fixtures dir")
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            e.path()
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
        })
        .collect();
    fixtures.sort();
    assert!(
        fixtures.len() >= 15,
        "expected every fixture, found {}",
        fixtures.len()
    );

    for fixture_name in &fixtures {
        let f = Fixture::read(fixture_name);
        load_fixture(&r, &f).await;

        let class_name = format!("IadSm172.{}", fixture_name);
        let model = model_from_class_text(&class_name, &f.cls_text);
        let mmd = mermaid(&model, None);

        match render_mermaid(&mmd) {
            Ok(()) => println!("✓ {fixture_name} rendered successfully"),
            Err(e) => {
                drop_fixture(&r, &f).await;
                panic!("{fixture_name} failed to render: {e}\nMermaid:\n{mmd}");
            }
        }

        let problems = drop_fixture(&r, &f).await;
        assert!(problems.is_empty(), "{problems:?}");
    }
}
