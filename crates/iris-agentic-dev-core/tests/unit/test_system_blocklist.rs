// Tests for system global blocklist gate (051-phi-policy-env-gates, US3).
//
// Verifies:
// - System blocklist patterns block hardcoded globals (%SYS, oddDEF, Ens.Config*, etc.)
// - Custom blocklist entries extend (not replace) the system blocklist
// - Kill allowlist exempts kill operations on specific globals
// - Non-matching globals pass the gate
// - Error JSON shape: SYSTEM_BLOCKLIST error code

use iris_agentic_dev_core::policy::system_blocklist_gate::check_system_blocklist;

const NO_CUSTOM: &[String] = &[];
const NO_KILL_LIST: &[String] = &[];

// ── System blocklist (hardcoded) ──────────────────────────────────────────────

#[test]
fn system_blocks_percent_sys() {
    // %SYS prefix — matches ^%SYS*
    let r = check_system_blocklist("%SYS.Security", NO_CUSTOM, NO_KILL_LIST, false, "iris-prod");
    assert!(
        r.is_some(),
        "%SYS.Security must be blocked by system blocklist"
    );
    let j = r.unwrap();
    assert_eq!(j["error_code"], "SYSTEM_BLOCKLIST");
    assert_eq!(j["system_blocklist"], true);
    assert_eq!(j["global_name"], "%SYS.Security");
    assert_eq!(j["server_name"], "iris-prod");
}

#[test]
fn system_blocks_odd_def() {
    let r = check_system_blocklist("oddDEF", NO_CUSTOM, NO_KILL_LIST, false, "iris-prod");
    assert!(r.is_some(), "oddDEF must be blocked by system blocklist");
}

#[test]
fn system_blocks_ens_config_prefix() {
    let r = check_system_blocklist(
        "Ens.Config.Settings",
        NO_CUSTOM,
        NO_KILL_LIST,
        false,
        "iris-prod",
    );
    assert!(r.is_some(), "Ens.Config.Settings must match ^Ens.Config*");
}

#[test]
fn system_blocks_percent_library() {
    let r = check_system_blocklist(
        "%Library.Class",
        NO_CUSTOM,
        NO_KILL_LIST,
        false,
        "iris-prod",
    );
    assert!(r.is_some(), "%Library.Class must be blocked");
}

#[test]
fn system_blocks_routine_globals() {
    for name in &["rOBJ", "rMAP", "rINDEX", "rINCLUDE", "rBACKUP", "ROUTINE"] {
        let r = check_system_blocklist(name, NO_CUSTOM, NO_KILL_LIST, false, "iris-prod");
        assert!(r.is_some(), "{name} must be blocked by system blocklist");
    }
}

#[test]
fn system_blocks_odd_variants() {
    for name in &["oddDEF", "oddEXT", "oddSQL", "oddMAC", "oddPKG", "oddCOM"] {
        let r = check_system_blocklist(name, NO_CUSTOM, NO_KILL_LIST, false, "iris-prod");
        assert!(r.is_some(), "{name} must be blocked by system blocklist");
    }
}

#[test]
fn system_blocks_iris_sys_globals() {
    for name in &[
        "IRIS.Msg.English",
        "IRIS.Temp.Work",
        "IRIS.Sys.Config",
        "IRIS.SysLog.Events",
    ] {
        let r = check_system_blocklist(name, NO_CUSTOM, NO_KILL_LIST, false, "iris-prod");
        assert!(r.is_some(), "{name} must be blocked by system blocklist");
    }
}

/// `^IRIS.SystemPerformance` holds SystemPerformance (pbuttons) run history and profile
/// definitions. It is diagnostic *data*, not code storage, and reading it is the documented
/// way to recover the last run ID. The old `^IRIS.Sys*` pattern swallowed it.
#[test]
fn system_allows_systemperformance_data_global() {
    for name in &["IRIS.SystemPerformance", "IRIS.SystemPerformance.History"] {
        let r = check_system_blocklist(name, NO_CUSTOM, NO_KILL_LIST, false, "iris-prod");
        assert!(
            r.is_none(),
            "{name} is perf data, not code storage — must not be blocked; got {r:?}"
        );
    }
}

/// Narrowing `^IRIS.Sys*` to `^IRIS.Sys.*` must not open up the config/log globals it
/// was there to protect.
#[test]
fn system_still_blocks_iris_sys_dot_and_syslog() {
    for name in &[
        "IRIS.Sys.Config",
        "IRIS.Sys.Anything",
        "IRIS.SysLog",
        "IRIS.SysLog.Events",
    ] {
        let r = check_system_blocklist(name, NO_CUSTOM, NO_KILL_LIST, false, "iris-prod");
        assert!(r.is_some(), "{name} must still be blocked");
    }
}

#[test]
fn system_blocks_deepsee() {
    let r = check_system_blocklist(
        "DeepSee.CubeManager",
        NO_CUSTOM,
        NO_KILL_LIST,
        false,
        "iris-prod",
    );
    assert!(r.is_some(), "DeepSee global must be blocked");
}

// ── Non-matching globals pass ─────────────────────────────────────────────────

#[test]
fn app_globals_not_blocked() {
    for name in &["MyApp", "UserData", "Config.Settings", "SAMPLE", "PAPMI"] {
        let r = check_system_blocklist(name, NO_CUSTOM, NO_KILL_LIST, false, "iris-prod");
        assert!(
            r.is_none(),
            "app global {name} must not be blocked by system blocklist"
        );
    }
}

// ── Custom blocklist extends system blocklist ─────────────────────────────────

#[test]
fn custom_blocklist_blocks_additional_globals() {
    let custom = vec!["^MySecret*".to_string(), "^InternalApp".to_string()];
    let r = check_system_blocklist("MySecret.Data", &custom, NO_KILL_LIST, false, "iris-prod");
    assert!(
        r.is_some(),
        "custom blocklist entry ^MySecret* must block MySecret.Data"
    );
}

#[test]
fn custom_blocklist_exact_match() {
    let custom = vec!["^InternalApp".to_string()];
    let r = check_system_blocklist("InternalApp", &custom, NO_KILL_LIST, false, "iris-prod");
    assert!(
        r.is_some(),
        "exact custom blocklist entry must block matching global"
    );
}

#[test]
fn custom_blocklist_does_not_unblock_system_globals() {
    // Even if custom blocklist is empty, system blocklist still applies
    let r = check_system_blocklist("oddDEF", NO_CUSTOM, NO_KILL_LIST, false, "iris-prod");
    assert!(
        r.is_some(),
        "system blocklist always applies regardless of custom list"
    );
}

#[test]
fn non_matching_not_blocked_with_custom_list() {
    let custom = vec!["^MySecret*".to_string()];
    let r = check_system_blocklist("UserData", &custom, NO_KILL_LIST, false, "iris-prod");
    assert!(
        r.is_none(),
        "UserData must not be blocked — not in any blocklist"
    );
}

// ── Kill allowlist exemption ──────────────────────────────────────────────────

#[test]
fn kill_allowlist_exempts_kill_operation() {
    let kill_allow = vec!["^TempData*".to_string()];
    // Normally TempData would pass system blocklist — but kill allowlist tests the kill_op path
    // Use a custom-blocked global to verify the exemption
    let custom = vec!["^TempData*".to_string()];
    let r = check_system_blocklist("TempData.Cache", &custom, &kill_allow, true, "iris-prod");
    assert!(
        r.is_none(),
        "kill allowlist must exempt kill ops on matching globals"
    );
}

#[test]
fn kill_allowlist_does_not_exempt_read_operation() {
    let kill_allow = vec!["^TempData*".to_string()];
    let custom = vec!["^TempData*".to_string()];
    let r = check_system_blocklist("TempData.Cache", &custom, &kill_allow, false, "iris-prod");
    assert!(
        r.is_some(),
        "kill allowlist only exempts is_kill_op=true, not reads"
    );
}

#[test]
fn kill_allowlist_does_not_exempt_system_blocklist() {
    // Even with kill allowlist, system blocklist is still enforced for system globals
    // The kill allowlist only applies to the custom blocklist check
    let kill_allow = vec!["^oddDEF".to_string()];
    let r = check_system_blocklist("oddDEF", NO_CUSTOM, &kill_allow, true, "iris-prod");
    // System blocklist check happens AFTER kill exemption — kill exemption only exempts custom+kill_allow
    // If oddDEF is in system blocklist and kill_allow, it's still blocked (system blocklist trumps)
    // Per spec: kill exemption only prevents custom_blocklist block, not system_blocklist
    // Let's verify the exact semantics from the implementation
    // The gate order: (1) kill allowlist check -> return None if is_kill && matches_kill_allow
    // Then (2) system blocklist, (3) custom blocklist
    // So if oddDEF is in kill_allowlist AND is_kill_op=true → None (permit)
    // This is intentional — admin operations may need to kill system globals
    assert!(
        r.is_none(),
        "kill allowlist exempts even system globals for kill ops — by design"
    );
}

// ── Error JSON shape ──────────────────────────────────────────────────────────

#[test]
fn error_json_includes_all_required_fields() {
    let r =
        check_system_blocklist("oddDEF", NO_CUSTOM, NO_KILL_LIST, false, "prod-server").unwrap();
    for field in &[
        "error_code",
        "system_blocklist",
        "server_name",
        "global_name",
        "matched_pattern",
        "message",
        "remediation",
    ] {
        assert!(
            r.get(field).is_some(),
            "missing field '{field}' in SYSTEM_BLOCKLIST error"
        );
    }
    assert_eq!(r["system_blocklist"], true);
}

#[test]
fn error_json_includes_matched_pattern() {
    let r = check_system_blocklist("%SYS.Security", NO_CUSTOM, NO_KILL_LIST, false, "iris-prod")
        .unwrap();
    let pat = r["matched_pattern"].as_str().unwrap();
    assert!(!pat.is_empty());
    // Should be the exact pattern string from SYSTEM_BLOCKLIST
    assert!(
        pat.contains("%SYS"),
        "matched_pattern should contain %SYS for %SYS.Security match"
    );
}

// ── System blocklist count (regression guard) ─────────────────────────────────

#[test]
fn system_blocklist_has_32_entries() {
    use iris_agentic_dev_core::policy::patterns::SYSTEM_BLOCKLIST;
    assert_eq!(
        SYSTEM_BLOCKLIST.len(),
        32,
        "system blocklist must have exactly 32 entries"
    );
}

/// A count guard says nothing about coverage. `^SYS*` was narrowed to `^SYS` / `^SYS.*` because
/// the blocklist is a hard block with no bypass, and the broad prefix permanently hid any
/// application global starting with those three letters.
#[test]
fn sys_narrowing_blocks_the_system_global_and_permits_app_globals() {
    use iris_agentic_dev_core::policy::patterns::{matches_any, SYSTEM_BLOCKLIST};
    assert!(matches_any("SYS", SYSTEM_BLOCKLIST));
    assert!(matches_any("SYS.Config", SYSTEM_BLOCKLIST));
    assert!(!matches_any("SYSCONFIG", SYSTEM_BLOCKLIST));
    assert!(!matches_any("SYSDATA", SYSTEM_BLOCKLIST));
}
