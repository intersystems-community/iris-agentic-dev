//! NoPWS (No Private Web Server) helpers for 101-nopws-connectivity.
//!
//! Shared constants and pure functions for routing decisions and error responses.

/// Error code returned by Atelier-required tools when NoPWS is active (FR-010).
pub const NOPWS_ATELIER_REQUIRED: &str = "NOPWS_ATELIER_REQUIRED";

/// Error code returned when docker exec is needed but no container is configured (FR-007).
pub const NOPWS_NO_CONTAINER: &str = "NOPWS_NO_CONTAINER";

/// Returns true when NoPWS routing is active (docker exec must be used instead of Atelier).
///
/// `docker_only`: base URL is the sentinel `http://127.0.0.1:1`.
/// `no_pws_version`: version string contains `"2026.2.0AI"` (from `derive_capabilities`).
pub fn nopws_is_active(docker_only: bool, no_pws_version: bool) -> bool {
    docker_only || no_pws_version
}

/// Build the NOPWS_ATELIER_REQUIRED structured error response (FR-010).
pub fn nopws_atelier_required_error() -> serde_json::Value {
    serde_json::json!({
        "success": false,
        "error_code": NOPWS_ATELIER_REQUIRED,
        // The setup guide is a repo file, not a loadable skill: skill discovery globs
        // <skills dir>/*/SKILL.md, so a file one level deeper is never found, and the
        // NoPWS guide is not in EMBEDDED_SKILLS either. Naming it "skills/nopws-setup"
        // sent agents to `skill(action="describe")`, which answered count: 0.
        "error": "NoPWS: this tool requires Atelier REST API. \
                  Set up a webgateway sidecar for Atelier REST access, \
                  or set docker_only = true in .iris-agentic-dev.toml (a connection key, \
                  not a tool parameter) for supported execution tools. \
                  Setup instructions: skills/skills/iris-agentic-dev/nopws-setup/SKILL.md \
                  in the iris-agentic-dev repo."
    })
}

/// Determine the `execution_path` value for a docker exec response.
///
/// Returns `"docker_exec_ssh"` when `ssh_host` is set, `"docker_exec_local"` otherwise.
pub fn execution_path_docker(ssh_host: Option<&str>) -> &'static str {
    if ssh_host.is_some() {
        "docker_exec_ssh"
    } else {
        "docker_exec_local"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nopws_active_docker_only() {
        assert!(nopws_is_active(true, false));
    }

    #[test]
    fn nopws_active_version_heuristic() {
        assert!(nopws_is_active(false, true));
    }

    #[test]
    fn nopws_inactive_normal() {
        assert!(!nopws_is_active(false, false));
    }

    #[test]
    fn execution_path_local_when_no_ssh() {
        assert_eq!(execution_path_docker(None), "docker_exec_local");
    }

    #[test]
    fn execution_path_ssh_when_set() {
        assert_eq!(execution_path_docker(Some("baystate")), "docker_exec_ssh");
    }

    #[test]
    fn nopws_atelier_required_shape() {
        let v = nopws_atelier_required_error();
        assert_eq!(v["success"], false);
        assert_eq!(v["error_code"], NOPWS_ATELIER_REQUIRED);
        assert!(v["error"].as_str().unwrap().contains("NoPWS"));
    }
}
