// Tests for PHI data policy gate (051-phi-policy-env-gates, US2).
//
// Verifies:
// - Bulk-PHI tools blocked when dataPolicy=block or dataPolicy=redact
// - Bulk-PHI tools permitted when dataPolicy=allow
// - Non-bulk tools not blocked by bulk-PHI gate
// - PHI name gate blocks globals matching PHI patterns without acknowledgePhi
// - PHI name gate permits with acknowledgePhi=true
// - Non-PHI globals pass name gate regardless
// - Error JSON shapes: DATA_POLICY_BLOCKED, PHI_GATE_BLOCKED

use iris_agentic_dev_core::iris::workspace_config::DataPolicy;
use iris_agentic_dev_core::policy::data_policy_gate::{check_bulk_phi_gate, check_phi_name_gate};

// ── Gate [2]: bulk-PHI tools ─────────────────────────────────────────────────

#[test]
fn bulk_phi_blocked_when_policy_block() {
    let r = check_bulk_phi_gate("journal_search", &DataPolicy::Block, "iris-health");
    assert!(
        r.is_some(),
        "journal_search must be blocked with policy=block"
    );
    let j = r.unwrap();
    assert_eq!(j["error_code"], "DATA_POLICY_BLOCKED");
    assert_eq!(j["data_policy_blocked"], true);
    assert_eq!(j["tool_name"], "journal_search");
    assert_eq!(j["server_name"], "iris-health");
}

#[test]
fn iris_message_body_blocked_when_policy_block() {
    let r = check_bulk_phi_gate("iris_message_body", &DataPolicy::Block, "iris-hl7");
    assert!(
        r.is_some(),
        "iris_message_body must be blocked with policy=block"
    );
    assert_eq!(r.unwrap()["error_code"], "DATA_POLICY_BLOCKED");
}

#[test]
fn bulk_phi_blocked_when_policy_redact() {
    // acknowledgePhi does NOT apply to bulk-PHI tools — still blocked under redact
    let r = check_bulk_phi_gate("journal_search", &DataPolicy::Redact, "iris-health");
    assert!(
        r.is_some(),
        "journal_search blocked under redact policy too"
    );
}

#[test]
fn bulk_phi_permitted_when_policy_allow() {
    let r = check_bulk_phi_gate("journal_search", &DataPolicy::Allow, "iris-health");
    assert!(
        r.is_none(),
        "journal_search must be permitted when policy=allow"
    );
}

#[test]
fn iris_message_body_permitted_when_policy_allow() {
    let r = check_bulk_phi_gate("iris_message_body", &DataPolicy::Allow, "iris-hl7");
    assert!(r.is_none(), "iris_message_body permitted when policy=allow");
}

/// Every name in the gate list has to be a name the router actually dispatches on.
///
/// This is the test that was missing. `BULK_PHI_TOOLS` carried `view_message_body` from 051 until
/// 1.3.2; no tool by that name was ever registered, so `check_bulk_phi_gate` never matched and PHI
/// message bodies came back with no policy check at all. The two tests above passed the whole time
/// because they asserted the same wrong string the constant held — a gate compared against itself
/// agrees with itself. Comparing against `CLASSIFICATION`, which is the router's own list, is the
/// only version of this test that can fail.
#[test]
fn every_bulk_phi_tool_is_a_registered_tool() {
    use iris_agentic_dev_core::policy::data_policy_gate::BULK_PHI_TOOLS;
    use iris_agentic_dev_core::tools::write_gate::CLASSIFICATION;

    for tool in BULK_PHI_TOOLS {
        assert!(
            CLASSIFICATION.iter().any(|c| c.tool == *tool),
            "BULK_PHI_TOOLS names {tool:?}, which is not a registered tool — the bulk-PHI gate \
             silently permits everything it was supposed to block. Registered names live in \
             tools::write_gate::CLASSIFICATION."
        );
    }
}

#[test]
fn non_bulk_tool_not_blocked_by_bulk_phi_gate() {
    for tool in &[
        "iris_query",
        "iris_execute",
        "iris_compile",
        "iris_search",
        "docs_introspect",
    ] {
        let r = check_bulk_phi_gate(tool, &DataPolicy::Block, "iris-health");
        assert!(
            r.is_none(),
            "non-bulk tool {tool} must not be blocked by bulk-PHI gate"
        );
    }
}

#[test]
fn bulk_phi_error_includes_remediation_message() {
    let r = check_bulk_phi_gate("journal_search", &DataPolicy::Block, "iris-health").unwrap();
    assert!(r.get("message").is_some(), "message field required");
    assert!(r.get("remediation").is_some(), "remediation field required");
    assert!(r.get("data_policy").is_some(), "data_policy field required");
}

// ── Gate [4]: PHI name pattern gate ──────────────────────────────────────────
//
// Note: global names are passed WITHOUT the leading ^ (IRIS convention strips it at call sites).
// The pattern table keeps ^ for readability; matches_pattern strips ^ from patterns, not names.

#[test]
fn phi_gate_blocks_papmi_global() {
    let r = check_phi_name_gate("PAPMI", false, "iris-health");
    assert!(r.is_some(), "PAPMI must be blocked without acknowledgePhi");
    let j = r.unwrap();
    assert_eq!(j["error_code"], "PHI_GATE_BLOCKED");
    assert_eq!(j["phi_gate_blocked"], true);
    assert_eq!(j["global_name"], "PAPMI");
    assert_eq!(j["server_name"], "iris-health");
}

#[test]
fn phi_gate_blocks_paadm_global() {
    let r = check_phi_name_gate("PAADM", false, "iris-health");
    assert!(r.is_some(), "PAADM matches PHI pattern");
}

#[test]
fn phi_gate_blocks_order_global() {
    let r = check_phi_name_gate("ORDER", false, "iris-health");
    assert!(r.is_some(), "ORDER matches PHI pattern");
}

#[test]
fn phi_gate_blocks_case_insensitive() {
    let r = check_phi_name_gate("papmi", false, "iris-health");
    assert!(
        r.is_some(),
        "papmi (lowercase) must match PHI pattern case-insensitively"
    );
}

#[test]
fn phi_gate_blocks_papmi_with_suffix() {
    // Prefix match: ^PAPMI* pattern matches PAPMI followed by anything
    let r = check_phi_name_gate("PAPMI1234", false, "iris-health");
    assert!(
        r.is_some(),
        "PAPMI1234 must match PHI prefix pattern ^PAPMI*"
    );
}

#[test]
fn phi_gate_permits_with_acknowledge_phi() {
    let r = check_phi_name_gate("PAPMI", true, "iris-health");
    assert!(r.is_none(), "acknowledgePhi=true must bypass PHI name gate");
}

#[test]
fn phi_gate_permits_non_phi_global() {
    for name in &["MyApp", "USERDATA", "Config", "SAMPLE"] {
        let r = check_phi_name_gate(name, false, "iris-health");
        assert!(r.is_none(), "non-PHI global {name} must pass gate");
    }
}

#[test]
fn phi_gate_error_includes_matched_pattern() {
    let r = check_phi_name_gate("PAPMI", false, "iris-health").unwrap();
    assert!(
        r.get("matched_pattern").is_some(),
        "matched_pattern field required"
    );
    let pat = r["matched_pattern"].as_str().unwrap();
    assert!(!pat.is_empty(), "matched_pattern must not be empty");
}

#[test]
fn phi_gate_error_includes_all_required_fields() {
    let r = check_phi_name_gate("PAADM", false, "prod-server").unwrap();
    for field in &[
        "error_code",
        "phi_gate_blocked",
        "server_name",
        "global_name",
        "matched_pattern",
        "message",
        "remediation",
    ] {
        assert!(
            r.get(field).is_some(),
            "missing field '{field}' in PHI_GATE_BLOCKED error"
        );
    }
}

// ── PHI name patterns coverage (all 9 known patterns) ────────────────────────

#[test]
fn phi_patterns_coverage() {
    // All 9 PHI name patterns from Pierre's VSIX must block (names without leading ^)
    let phi_globals = [
        "PAPMI",               // patient master index
        "PAADM",               // patient admission
        "PAAPT",               // patient appointment
        "PAPER",               // patient paper/doc
        "MRADM",               // MR admission
        "OEHeader",            // order entry (OE* prefix)
        "ORDER123",            // orders
        "Ens.MessageHeader.1", // ensemble message header
        "Ens.MessageBody.2",   // ensemble message body
    ];
    for global in &phi_globals {
        let r = check_phi_name_gate(global, false, "iris-health");
        assert!(r.is_some(), "PHI global {global} must be blocked");
    }
}
