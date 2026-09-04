//! Hardcoded PHI name patterns and system global blocklist (051-phi-policy-env-gates).
//!
//! Sources: Pierre Abdelsayed's servermanager-3.13.0-build.0D05.vsix `mcp-server.js`
//! arrays `N` (PHI patterns) and `D` (system blocklist).

/// Hardcoded system global blocklist. Non-configurable, enforced regardless of `dataPolicy`.
/// Per-connection `globalBlocklist` entries EXTEND (not replace) this list.
pub const SYSTEM_BLOCKLIST: &[&str] = &[
    "^%SYS*",
    "^%Library*",
    "^%Dictionary*",
    "^%SYSTEM*",
    "^rOBJ",
    "^rMAP",
    "^rINDEX",
    "^rINCLUDE",
    "^rBACKUP",
    "^ROUTINE",
    "^oddDEF",
    "^oddEXT",
    "^oddSQL",
    "^oddMAC",
    "^oddPKG",
    "^oddCOM",
    "^ROLE",
    "^USER",
    "^Ens.Config*",
    "^Ens.Rule*",
    "^Ens.Rules*",
    "^Ens.MessageHeader*",
    "^Ens.MessageBody*",
    // `^SYS` and `^SYS.*`, not `^SYS*`. The blocklist is a hard block with no bypass, so an
    // over-broad prefix permanently hides application globals: `^SYS*` matched `^SYSCONFIG`,
    // `^SYSDATA`, `^SYSTOTALS` — any application global whose name happens to start with those
    // three letters. Enumerating `^$GLOBAL` in %SYS, USER, IRISLIB and HSLIB on 2026.2 finds
    // exactly one global in that space: `^SYS`. The dotted form is here because that is the
    // convention IRIS uses when it adds one, same reasoning as `^IRIS.Sys.*` above.
    "^SYS",
    "^SYS.*",
    "^SYSTEM",
    "^SYSTEM.*",
    "^DeepSee*",
    "^IRIS.Msg*",
    "^IRIS.Temp*",
    // `^IRIS.Sys.*` (dotted), not `^IRIS.Sys*` — the broader prefix also swallowed
    // `^IRIS.SystemPerformance`, which holds pbuttons run history and profile definitions.
    // That global is diagnostic data, not code storage, and reading it is the documented way
    // to recover the last run ID.
    "^IRIS.Sys.*",
    "^IRIS.SysLog*",
];

/// Hardcoded PHI name patterns. Globals matching these require `acknowledgePhi: true`
/// for individual reads. Does NOT apply to bulk-PHI tools (`journal_search`, `iris_message_body`).
pub const PHI_NAME_PATTERNS: &[&str] = &[
    "^PAPMI*",
    "^PAADM*",
    "^PAAPT*",
    "^PAPER*",
    "^MRADM*",
    "^OE*",
    "^ORDER*",
    "^Ens.MessageHeader*",
    "^Ens.MessageBody*",
];

/// Returns `true` if `global_name` matches `pattern`.
///
/// Pattern rules:
/// - Leading `^` is stripped (IRIS global naming convention, not a regex anchor).
/// - If pattern ends with `*`: prefix match against the stripped pattern.
/// - Otherwise: exact match.
/// - Matching is case-insensitive.
pub fn matches_pattern(global_name: &str, pattern: &str) -> bool {
    let p = pattern.strip_prefix('^').unwrap_or(pattern);
    let name_upper = global_name.to_uppercase();
    if let Some(prefix) = p.strip_suffix('*') {
        name_upper.starts_with(&prefix.to_uppercase())
    } else {
        name_upper == p.to_uppercase()
    }
}

/// Returns `true` if `global_name` matches any pattern in `patterns`.
pub fn matches_any(global_name: &str, patterns: &[&str]) -> bool {
    patterns.iter().any(|p| matches_pattern(global_name, p))
}

/// Returns `true` if `global_name` matches any pattern in a `Vec<String>`.
pub fn matches_any_owned(global_name: &str, patterns: &[String]) -> bool {
    patterns
        .iter()
        .any(|p| matches_pattern(global_name, p.as_str()))
}

/// Returns the first matching pattern from `patterns`, or `None`.
pub fn first_match<'a>(global_name: &str, patterns: &[&'a str]) -> Option<&'a str> {
    patterns
        .iter()
        .copied()
        .find(|p| matches_pattern(global_name, p))
}

/// Returns the first matching pattern from an owned slice, cloned.
pub fn first_match_owned(global_name: &str, patterns: &[String]) -> Option<String> {
    patterns
        .iter()
        .find(|p| matches_pattern(global_name, p.as_str()))
        .cloned()
}
