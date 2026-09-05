//! Tests for `policy::patterns` — pattern matching and the two hardcoded lists.
//!
//! These lived in an inline `#[cfg(test)] mod tests`. Assertion-message lines inside such a
//! module only execute when a test fails, so they read as permanently uncovered and cap the
//! measured coverage of the file they sit in. `patterns.rs` has no uncovered production line;
//! its shortfall was entirely its own assert messages.

use iris_agentic_dev_core::policy::patterns::{
    first_match, first_match_owned, matches_any, matches_pattern, PHI_NAME_PATTERNS,
    SYSTEM_BLOCKLIST,
};

#[test]
fn prefix_match_star() {
    assert!(matches_pattern("%SYS.Security", "^%SYS*"));
    assert!(matches_pattern("%SYSNotReal", "^%SYS*"));
    assert!(matches_pattern("%SYSOTHER", "^%SYS*"));
}

#[test]
fn no_match_unrelated() {
    assert!(!matches_pattern("MySYS", "^%SYS*"));
    assert!(!matches_pattern("MyAppData", "^PAPMI*"));
}

#[test]
fn exact_match_no_star() {
    assert!(matches_pattern("rOBJ", "^rOBJ"));
    assert!(!matches_pattern("rOBJExtra", "^rOBJ"));
}

#[test]
fn case_insensitive() {
    assert!(matches_pattern("papmi", "^PAPMI*"));
    assert!(matches_pattern("PAPMI123", "^PAPMI*"));
}

#[test]
fn phi_patterns_cover_expected_names() {
    assert!(matches_any("PAPMI", PHI_NAME_PATTERNS));
    assert!(matches_any("PAADM1234", PHI_NAME_PATTERNS));
    assert!(matches_any("ORDER123", PHI_NAME_PATTERNS));
    assert!(!matches_any("MyAppData", PHI_NAME_PATTERNS));
}

#[test]
fn system_blocklist_count() {
    assert_eq!(SYSTEM_BLOCKLIST.len(), 32);
}

/// The blocklist has no bypass, so every false block is permanent. `^SYS*` used to swallow
/// any application global starting with those three letters.
#[test]
fn sys_prefix_no_longer_swallows_application_globals() {
    for permitted in &["SYSCONFIG", "SYSDATA", "SYSTOTALS", "SYSTEMSTATUS"] {
        assert!(
            !matches_any(permitted, SYSTEM_BLOCKLIST),
            "^{permitted} is an application global, not a system one"
        );
    }
    for blocked in &["SYS", "sys", "SYS.Anything", "SYSTEM", "SYSTEM.Anything"] {
        assert!(
            matches_any(blocked, SYSTEM_BLOCKLIST),
            "^{blocked} must stay blocked"
        );
    }
}

#[test]
fn phi_patterns_count() {
    assert_eq!(PHI_NAME_PATTERNS.len(), 9);
}

/// `first_match` and `first_match_owned` are what the gates actually call — they need the name
/// of the pattern that matched for the refusal message, not just a yes.
#[test]
fn first_match_returns_the_pattern_that_matched() {
    assert_eq!(
        first_match("%SYS.Security", SYSTEM_BLOCKLIST),
        Some("^%SYS*")
    );
    assert_eq!(first_match("MyAppData", SYSTEM_BLOCKLIST), None);
}

#[test]
fn first_match_owned_takes_a_configured_list() {
    let configured = vec!["^MyPHI*".to_string(), "^PAPMI*".to_string()];
    assert_eq!(
        first_match_owned("PAPMI1", &configured).as_deref(),
        Some("^PAPMI*"),
        "the reported pattern has to be the one that matched, not the first in the list"
    );
    assert_eq!(first_match_owned("Unrelated", &configured), None);
    assert_eq!(
        first_match_owned("anything", &[]),
        None,
        "an empty per-connection list must not match everything"
    );
}
