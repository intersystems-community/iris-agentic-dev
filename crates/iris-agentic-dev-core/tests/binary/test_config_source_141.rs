//! Layer 2 for spec 141 T194: `connection_source` and `config_file` name a project file only when
//! it names a host or a container, at startup and after a reload.
//!
//! The server is pointed at a port nothing listens on (`IRIS_HOST` and `IRIS_WEB_PORT` together,
//! so the spawn cannot adopt a real IRIS).

use crate::support_mcp_141::Env;
use serde_json::json;

const LAUNCH: &[(&str, &str)] = &[("IRIS_HOST", "127.0.0.1"), ("IRIS_WEB_PORT", "9")];

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn a_project_file_is_the_source_only_while_it_names_a_target() {
    let env = Env::new("");
    let ws = env.proj.join(".iris-agentic-dev.toml");
    std::fs::write(&ws, "namespace = \"APP\"\n").unwrap();
    let ws_text = std::fs::canonicalize(&ws).unwrap().display().to_string();
    let mut s = env.start_with(LAUNCH);

    // Startup: the file decorates the server IRIS_HOST names.
    let cfg = s.body("check_config", json!({}));
    assert_eq!(cfg["connection_source"], "env_vars", "{cfg}");
    assert!(cfg["config_file"].is_null(), "{cfg}");
    assert_eq!(cfg["config_file_named_no_target"], ws_text, "{cfg}");
    let w = cfg["config_file_warning"]
        .as_str()
        .unwrap_or_else(|| panic!("{cfg}"));
    assert!(w.contains("neither a host nor a container"), "{w}");

    // A reload that gives the file a host: it is the source now, and the warning goes.
    std::thread::sleep(std::time::Duration::from_millis(1100));
    std::fs::write(
        &ws,
        "host = \"127.0.0.1\"\nweb_port = 9\nnamespace = \"APP\"\n",
    )
    .unwrap();
    let cfg = s.body("check_config", json!({}));
    assert_eq!(cfg["connection_source"], "config_file", "{cfg}");
    assert_eq!(cfg["config_file"], ws_text, "{cfg}");
    assert!(cfg["config_file_named_no_target"].is_null(), "{cfg}");
    assert!(cfg["config_file_warning"].is_null(), "{cfg}");

    // And a reload that takes the host away again.
    std::thread::sleep(std::time::Duration::from_millis(1100));
    std::fs::write(&ws, "namespace = \"APP\"\n").unwrap();
    let cfg = s.body("check_config", json!({}));
    assert_eq!(cfg["connection_source"], "env_vars", "{cfg}");
    assert!(cfg["config_file"].is_null(), "{cfg}");
    assert_eq!(cfg["config_file_named_no_target"], ws_text, "{cfg}");
    assert!(cfg["config_file_warning"].is_string(), "{cfg}");
}
