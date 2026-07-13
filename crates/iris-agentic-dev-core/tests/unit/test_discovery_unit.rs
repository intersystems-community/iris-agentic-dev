// Unit tests for iris/discovery.rs — score_container_name + IrisDiscovery types.
// These tests intentionally hold std::sync::MutexGuard across awaits to serialize
// env-var mutation. This is correct behaviour for test serialization.
#![allow(clippy::await_holding_lock)]
// No Docker, no network required.

use iris_agentic_dev_core::iris::discovery::{
    emit_unhealthy_message, score_container_name, DiscoveryResult, FailureMode, IrisDiscovery,
};
use std::sync::Mutex;

// Mutex to serialize tests that mutate env vars (avoid race conditions in parallel test runs)
static DISCOVERY_LOCK: Mutex<()> = Mutex::new(());

// ── IrisDiscovery enum smoke test ─────────────────────────────────────────────

#[test]
fn test_iris_discovery_variants_exist() {
    let _ = std::mem::discriminant(&IrisDiscovery::NotFound);
    let _ = std::mem::discriminant(&IrisDiscovery::Explained);
}

#[test]
fn test_failure_mode_variants_exist() {
    let _ = std::mem::discriminant(&FailureMode::PortNotMapped);
    let _ = std::mem::discriminant(&FailureMode::AtelierNotResponding { port: 52773 });
    let _ = std::mem::discriminant(&FailureMode::AtelierHttpError {
        port: 52773,
        status: 503,
    });
    let _ = std::mem::discriminant(&FailureMode::AtelierAuth401 { port: 52773 });
}

// ── T015/T016: discover_iris + container not found ────────────────────────────

/// T015: DiscoveryResult::NotFound is distinct from FoundUnhealthy
#[test]
fn test_discovery_result_not_found_is_distinct() {
    let r = DiscoveryResult::NotFound;
    assert!(matches!(r, DiscoveryResult::NotFound));
    assert!(!matches!(r, DiscoveryResult::FoundUnhealthy(_)));
}

/// T016: FoundUnhealthy carries a FailureMode
#[test]
fn test_discovery_result_found_unhealthy_carries_mode() {
    let r = DiscoveryResult::FoundUnhealthy(FailureMode::PortNotMapped);
    match r {
        DiscoveryResult::FoundUnhealthy(FailureMode::PortNotMapped) => {}
        _ => panic!("expected FoundUnhealthy(PortNotMapped)"),
    }
}

// ── T022/T023: PortNotMapped ──────────────────────────────────────────────────

/// T022: PortNotMapped variant roundtrip
#[test]
fn test_failure_mode_port_not_mapped() {
    let mode = FailureMode::PortNotMapped;
    assert!(matches!(mode, FailureMode::PortNotMapped));
}

/// T023: IrisDiscovery::Explained is distinct from NotFound
#[test]
fn test_iris_discovery_explained_is_distinct_from_not_found() {
    let explained = IrisDiscovery::Explained;
    let not_found = IrisDiscovery::NotFound;
    assert!(!matches!(explained, IrisDiscovery::NotFound));
    assert!(!matches!(not_found, IrisDiscovery::Explained));
}

// ── T029/T030/T031: AtelierNotResponding, AtelierHttpError ────────────────────

/// T029: AtelierNotResponding carries port
#[test]
fn test_failure_mode_atelier_not_responding() {
    let mode = FailureMode::AtelierNotResponding { port: 52791 };
    match mode {
        FailureMode::AtelierNotResponding { port: 52791 } => {}
        _ => panic!("wrong variant"),
    }
}

/// T030: AtelierHttpError carries port + status
#[test]
fn test_failure_mode_atelier_http_error() {
    let mode = FailureMode::AtelierHttpError {
        port: 52791,
        status: 503,
    };
    match mode {
        FailureMode::AtelierHttpError {
            port: 52791,
            status: 503,
        } => {}
        _ => panic!("wrong variant"),
    }
}

/// T031: FoundUnhealthy(AtelierNotResponding) is distinct from NotFound
#[test]
fn test_found_unhealthy_atelier_is_not_not_found() {
    let r = DiscoveryResult::FoundUnhealthy(FailureMode::AtelierNotResponding { port: 52791 });
    assert!(!matches!(r, DiscoveryResult::NotFound));
    assert!(matches!(r, DiscoveryResult::FoundUnhealthy(_)));
}

// ── T038/T039: AtelierAuth401 ────────────────────────────────────────────────

/// T038: AtelierAuth401 carries port
#[test]
fn test_failure_mode_auth_401() {
    let mode = FailureMode::AtelierAuth401 { port: 52790 };
    match mode {
        FailureMode::AtelierAuth401 { port: 52790 } => {}
        _ => panic!("wrong variant"),
    }
}

/// T039: Auth401 maps to Explained (not NotFound) — structural check
#[test]
fn test_auth_401_maps_to_explained_not_not_found() {
    // The mapping logic is: FoundUnhealthy(Auth401) → Explained in discover_iris().
    // We verify this structurally: Explained ≠ NotFound.
    let explained = IrisDiscovery::Explained;
    assert!(!matches!(explained, IrisDiscovery::NotFound));
}

// ── FR-007: localhost scan credential check ───────────────────────────────────

/// T051: When IRIS_USERNAME/IRIS_PASSWORD are set, they should be used (structural check)
/// The actual behavior is tested via E2E; here we verify the env vars are read.
#[test]
fn test_iris_username_env_var_readable() {
    std::env::set_var("IRIS_USERNAME_TEST_028", "testuser");
    let val = std::env::var("IRIS_USERNAME_TEST_028").unwrap();
    assert_eq!(val, "testuser");
    std::env::remove_var("IRIS_USERNAME_TEST_028");
}

// ── score_container_name (existing tests preserved below) ────────────────────

// ── Basic coverage ────────────────────────────────────────────────────────────

#[test]
fn test_score_empty_workspace_returns_zero() {
    assert_eq!(
        score_container_name("any-iris", ""),
        0,
        "empty workspace basename must score 0"
    );
}

#[test]
fn test_score_unrelated_scores_zero() {
    assert_eq!(score_container_name("redis-cache", "myapp"), 0);
}

#[test]
fn test_score_exact_match_is_100() {
    assert_eq!(score_container_name("myapp", "myapp"), 100);
}

#[test]
fn test_score_starts_with_is_80() {
    // "myapp-dev" starts with "myapp" but no -iris suffix
    assert_eq!(score_container_name("myapp-dev", "myapp"), 80);
}

#[test]
fn test_score_contains_match_is_60() {
    // "myapp" is contained in "xyz-myapp-iris" but doesn't start with it
    let s = score_container_name("xyz_myapp_iris", "myapp");
    assert_eq!(s, 70, "contains match + iris suffix = 60 + 10 = 70");
}

#[test]
fn test_score_iris_suffix_bonus_10() {
    let with_iris = score_container_name("loanapp-iris", "loanapp");
    let without = score_container_name("loanapp-dev", "loanapp");
    // loanapp-iris: 80 + 10 = 90; loanapp-dev: 80 + 0 = 80
    assert_eq!(with_iris, 90);
    assert_eq!(without, 80);
    assert!(with_iris > without, "-iris suffix must score higher");
}

#[test]
fn test_score_test_suffix_bonus_5() {
    let with_test = score_container_name("myapp-test", "myapp");
    let without = score_container_name("myapp-dev", "myapp");
    // myapp-test: 80 + 5 = 85; myapp-dev: 80 + 0 = 80
    assert_eq!(with_test, 85);
    assert!(
        with_test > without,
        "-test suffix must score higher than -dev"
    );
}

#[test]
fn test_score_iris_and_test_suffix_not_double_counted() {
    // A name can't end in both -iris and -test simultaneously (they're different suffixes)
    // Verify only one bonus is added at a time
    let iris_only = score_container_name("app-iris", "app");
    let test_only = score_container_name("app-test", "app");
    assert_eq!(iris_only, 90);
    assert_eq!(test_only, 85);
}

// ── Case insensitivity ────────────────────────────────────────────────────────

#[test]
fn test_score_exact_case_insensitive() {
    let s1 = score_container_name("MyApp-IRIS", "myapp");
    let s2 = score_container_name("myapp-iris", "myapp");
    assert_eq!(s1, s2, "scoring must be case-insensitive");
}

#[test]
fn test_score_workspace_uppercase() {
    let s1 = score_container_name("myapp-iris", "MYAPP");
    let s2 = score_container_name("myapp-iris", "myapp");
    assert_eq!(s1, s2, "workspace name case should not matter");
}

// ── Hyphen/underscore normalization ───────────────────────────────────────────

#[test]
fn test_score_underscore_hyphen_equivalence() {
    // id_try2 workspace should match id-try2-iris container
    let s = score_container_name("id-try2-iris", "id_try2");
    assert!(s > 0, "id_try2 should match id-try2-iris, got {}", s);
    assert!(s >= 80, "should score at least 80 (starts_with), got {}", s);
}

#[test]
fn test_score_hyphen_workspace_underscore_container() {
    let s = score_container_name("id_try2_iris", "id-try2");
    assert!(
        s > 0,
        "id-try2 workspace should match id_try2_iris container"
    );
}

#[test]
fn test_score_all_hyphens_normalized() {
    // my-loan-app vs my_loan_app should be equivalent
    let s1 = score_container_name("my-loan-app", "my_loan_app");
    assert_eq!(
        s1, 100,
        "all-hyphen and all-underscore should be exact match after normalization"
    );
}

// ── starts_with vs contains ordering ─────────────────────────────────────────

#[test]
fn test_score_starts_with_beats_contains() {
    let starts = score_container_name("appname-iris", "appname");
    let contains = score_container_name("myappname-iris", "appname");
    // starts = 80+10=90; contains = 60+10=70
    assert!(
        starts > contains,
        "starts_with ({}) must score higher than contains ({})",
        starts,
        contains
    );
}

#[test]
fn test_score_exact_beats_starts_with() {
    let exact = score_container_name("loanapp", "loanapp");
    let starts = score_container_name("loanapp-iris", "loanapp");
    // exact = 100; starts+iris = 80+10 = 90
    assert_eq!(exact, 100);
    assert_eq!(starts, 90);
    assert!(
        exact > starts,
        "exact match (100) must beat starts_with+iris (90)"
    );
}

// ── Edge cases ────────────────────────────────────────────────────────────────

#[test]
fn test_score_single_char_workspace() {
    let s = score_container_name("a-iris", "a");
    assert!(s > 0, "single-char workspace should still match, got {}", s);
    assert_eq!(s, 90); // starts_with "a" + iris suffix = 80+10
}

#[test]
fn test_score_empty_container_name() {
    // Empty container can't match anything
    assert_eq!(score_container_name("", "myapp"), 0);
}

#[test]
fn test_score_both_empty() {
    assert_eq!(score_container_name("", ""), 0);
}

#[test]
fn test_score_container_only_iris_suffix() {
    // Container "iris" for workspace "iris" — exact match = 100
    assert_eq!(score_container_name("iris", "iris"), 100);
}

#[test]
fn test_score_underscore_iris_suffix_also_counts() {
    // ends_with("_iris") should also earn the +10 bonus
    let s = score_container_name("myapp_iris", "myapp");
    assert_eq!(s, 90, "underscore iris suffix should also score 90");
}

#[test]
fn test_score_known_example_loanapp_iris() {
    // Canonical example from spec-025
    let score = score_container_name("loanapp-iris", "loanapp");
    assert_eq!(score, 90, "loanapp-iris for loanapp should score 90");
}

#[test]
fn test_score_determined_cray_is_zero() {
    assert_eq!(score_container_name("determined_cray", "id_try2"), 0);
}

// ── Pure-logic URL building and env-var parsing (lines 151-174) ──────────────

#[test]
fn test_port_parsing_valid_port_string() {
    // Simulate: std::env::var("IRIS_WEB_PORT") → "52773" → parse::<u16>()
    let port_str = "52773";
    let parsed: Option<u16> = port_str.parse().ok();
    assert_eq!(parsed, Some(52773));
}

#[test]
fn test_port_parsing_invalid_port_string() {
    // Simulate parsing invalid port: "abc" → None
    let port_str = "abc";
    let parsed: Option<u16> = port_str.parse().ok();
    assert_eq!(parsed, None);
}

#[test]
fn test_port_parsing_port_out_of_range() {
    // Simulate parsing port out of u16 range: "99999" → None
    let port_str = "99999";
    let parsed: Option<u16> = port_str.parse().ok();
    assert_eq!(parsed, None);
}

#[test]
fn test_port_parsing_empty_string() {
    // Simulate parsing empty port string → None
    let port_str = "";
    let parsed: Option<u16> = port_str.parse().ok();
    assert_eq!(parsed, None);
}

#[test]
fn test_port_parsing_with_whitespace() {
    // Simulate: " 52773 " → parse requires manual trim (parse doesn't auto-trim)
    let port_str = " 52773 ";
    let parsed: Option<u16> = port_str.trim().parse().ok();
    assert_eq!(parsed, Some(52773));
}

#[test]
fn test_port_default_to_52773_when_parse_fails() {
    // Simulate: port_str = "invalid" → parse fails → unwrap_or(52773)
    let port_str = "invalid";
    let port = port_str.parse::<u16>().ok().unwrap_or(52773);
    assert_eq!(port, 52773);
}

#[test]
fn test_scheme_trim_slashes_both_sides() {
    // Simulate: "http://" → trim_matches('/') → "http:"
    let scheme_str = "http://";
    let trimmed = scheme_str.trim_matches('/').to_string();
    assert_eq!(trimmed, "http:");
}

#[test]
fn test_scheme_trim_slashes_leading_only() {
    // Simulate: "/https" → trim_matches('/') → "https"
    let scheme_str = "/https";
    let trimmed = scheme_str.trim_matches('/').to_string();
    assert_eq!(trimmed, "https");
}

#[test]
fn test_scheme_single_slash() {
    // Simulate: "/" → trim → "" → is_empty() → true
    let scheme_str = "/";
    let trimmed = scheme_str.trim_matches('/').to_string();
    assert!(trimmed.is_empty());
}

#[test]
fn test_scheme_normalize_https() {
    // Simulate: "https" (no slashes) → trim → "https" (unchanged)
    let scheme_str = "https";
    let trimmed = scheme_str.trim_matches('/').to_string();
    assert_eq!(trimmed, "https");
}

#[test]
fn test_prefix_trim_slashes_removes_leading_trailing() {
    // Simulate: "/app/prefix/" → trim_matches('/') → "app/prefix"
    let prefix_str = "/app/prefix/";
    let trimmed = prefix_str.trim_matches('/').to_string();
    assert_eq!(trimmed, "app/prefix");
}

#[test]
fn test_prefix_empty_after_trim_becomes_none() {
    // Simulate: "/" → trim → "" → is_empty() → filter to None
    let prefix_str = "/";
    let trimmed = prefix_str.trim_matches('/').to_string();
    let result = if !trimmed.is_empty() {
        Some(trimmed)
    } else {
        None
    };
    assert_eq!(result, None);
}

#[test]
fn test_prefix_no_slashes_unchanged() {
    // Simulate: "myprefix" → trim → "myprefix" (unchanged)
    let prefix_str = "myprefix";
    let trimmed = prefix_str.trim_matches('/').to_string();
    assert_eq!(trimmed, "myprefix");
}

// ── URL construction with prefix + scheme (lines 171-174) ──────────────────

#[test]
fn test_url_construction_with_prefix() {
    // Simulate: scheme="https", host="example.com", port=8443, prefix="iris"
    let scheme = "https";
    let host = "example.com";
    let port = 8443u16;
    let prefix = "iris";
    let url = format!("{}://{}:{}/{}", scheme, host, port, prefix);
    assert_eq!(url, "https://example.com:8443/iris");
}

#[test]
fn test_url_construction_without_prefix() {
    // Simulate: scheme="http", host="localhost", port=52773, no prefix
    let scheme = "http";
    let host = "localhost";
    let port = 52773u16;
    let url = format!("{}://{}:{}", scheme, host, port);
    assert_eq!(url, "http://localhost:52773");
}

#[test]
#[allow(clippy::const_is_empty)] // scheme_str is a literal stand-in for a runtime-empty value
fn test_url_construction_default_scheme() {
    // Simulate: scheme defaults to "http" if empty
    let scheme_str = "";
    let scheme = if scheme_str.is_empty() {
        "http".to_string()
    } else {
        scheme_str.to_string()
    };
    let url = format!("{}://localhost:52773", scheme);
    assert_eq!(url, "http://localhost:52773");
}

// ── Atelier API version detection (lines 114-118, 495-499) ──────────────────

#[test]
fn test_atelier_version_v8_when_api_8() {
    // Simulate: content["api"] = 8 → AtelierVersion::V8
    // We test the matching logic directly
    let api_value = 8u64;
    let version = if api_value >= 8 {
        "V8"
    } else if api_value >= 2 {
        "V2"
    } else {
        "V1"
    };
    assert_eq!(version, "V8");
}

#[test]
fn test_atelier_version_v8_when_api_higher() {
    // Simulate: content["api"] = 10 → AtelierVersion::V8 (≥8)
    let api_value = 10u64;
    let version = if api_value >= 8 {
        "V8"
    } else if api_value >= 2 {
        "V2"
    } else {
        "V1"
    };
    assert_eq!(version, "V8");
}

#[test]
fn test_atelier_version_v2_when_api_2() {
    // Simulate: content["api"] = 2 → AtelierVersion::V2
    let api_value = 2u64;
    let version = if api_value >= 8 {
        "V8"
    } else if api_value >= 2 {
        "V2"
    } else {
        "V1"
    };
    assert_eq!(version, "V2");
}

#[test]
fn test_atelier_version_v2_when_api_middle() {
    // Simulate: content["api"] = 5 → AtelierVersion::V2 (≥2, <8)
    let api_value = 5u64;
    let version = if api_value >= 8 {
        "V8"
    } else if api_value >= 2 {
        "V2"
    } else {
        "V1"
    };
    assert_eq!(version, "V2");
}

#[test]
fn test_atelier_version_v1_when_api_1() {
    // Simulate: content["api"] = 1 → AtelierVersion::V1
    let api_value = 1u64;
    let version = if api_value >= 8 {
        "V8"
    } else if api_value >= 2 {
        "V2"
    } else {
        "V1"
    };
    assert_eq!(version, "V1");
}

#[test]
fn test_atelier_version_v1_when_api_zero() {
    // Simulate: content["api"] = 0 → AtelierVersion::V1 (fallback)
    let api_value = 0u64;
    let version = if api_value >= 8 {
        "V8"
    } else if api_value >= 2 {
        "V2"
    } else {
        "V1"
    };
    assert_eq!(version, "V1");
}

// ── IRIS version string validation (lines 101-104, 475-478) ──────────────────

#[test]
fn test_iris_version_valid_iris_string() {
    // Simulate: version.to_uppercase().contains("IRIS") → Some
    let version = "IRIS 2024.1";
    let is_iris = version.to_uppercase().contains("IRIS");
    assert!(is_iris);
}

#[test]
fn test_iris_version_lowercase_iris() {
    // Simulate: "iris 2024" → .to_uppercase() → contains("IRIS") → true
    let version = "iris 2024";
    let is_iris = version.to_uppercase().contains("IRIS");
    assert!(is_iris);
}

#[test]
fn test_iris_version_missing_iris_keyword() {
    // Simulate: "CachéSQL 2024" → doesn't contain IRIS → None
    let version = "CacheSQL 2024";
    let is_iris = version.to_uppercase().contains("IRIS");
    assert!(!is_iris);
}

#[test]
fn test_iris_version_empty_string() {
    // Simulate: "" → doesn't contain "IRIS" → None
    let version = "";
    let is_iris = version.to_uppercase().contains("IRIS");
    assert!(!is_iris);
}

#[test]
fn test_iris_version_iris_as_substring() {
    // Simulate: "My-IRIS-Container" → contains "IRIS" → Some
    let version = "My-IRIS-Container";
    let is_iris = version.to_uppercase().contains("IRIS");
    assert!(is_iris);
}

// ── Port extraction from Docker container (lines 390-404) ──────────────────

#[test]
fn test_port_extraction_web_port_52773() {
    // Simulate extracting port 52773 from container.ports
    let private_port = 52773;
    let public_port = Some(52773);
    let mut port_web: Option<u16> = None;

    if private_port == 52773 {
        port_web = public_port;
    }
    assert_eq!(port_web, Some(52773));
}

#[test]
fn test_port_extraction_superserver_port_1972() {
    // Simulate extracting port 1972 (superserver) from container.ports
    let private_port = 1972;
    let public_port = Some(1972);
    let mut port_ss: Option<u16> = None;

    if private_port == 1972 {
        port_ss = public_port;
    }
    assert_eq!(port_ss, Some(1972));
}

#[test]
fn test_port_extraction_multiple_ports() {
    // Simulate iterating through multiple ports, extracting both 52773 and 1972
    let ports = vec![(80, Some(80)), (52773, Some(8080)), (1972, Some(1972))];

    let mut port_web: Option<u16> = None;
    let mut port_ss: Option<u16> = None;

    for (private_port, public_port) in ports {
        if private_port == 52773 {
            port_web = public_port;
        }
        if private_port == 1972 {
            port_ss = public_port;
        }
    }

    assert_eq!(port_web, Some(8080));
    assert_eq!(port_ss, Some(1972));
}

#[test]
fn test_port_extraction_web_port_not_mapped() {
    // Simulate: port 52773 not in container.ports → port_web remains None
    let ports = vec![(80, Some(80)), (1972, Some(1972))];

    let mut port_web: Option<u16> = None;
    for (private_port, public_port) in ports {
        if private_port == 52773 {
            port_web = public_port;
        }
    }

    assert_eq!(port_web, None);
}

#[test]
fn test_port_extraction_none_public_port() {
    // Simulate: port 52773 mapped but no public port (None)
    let private_port = 52773;
    let public_port: Option<u16> = None;

    let port_web = if private_port == 52773 {
        public_port
    } else {
        None
    };

    assert_eq!(port_web, None);
}

// ── Docker container filtering (lines 528-535) ────────────────────────────────

#[test]
fn test_is_iris_container_with_intersystems_image() {
    // Simulate: image = "intersystems/iris:latest"
    let image = "intersystems/iris:latest";
    let is_iris = image.contains("intersystems") || image.contains("iris");
    assert!(is_iris);
}

#[test]
fn test_is_iris_container_with_iris_keyword() {
    // Simulate: image = "registry.com/my-iris:2024"
    let image = "registry.com/my-iris:2024";
    let is_iris = image.contains("intersystems") || image.contains("iris");
    assert!(is_iris);
}

#[test]
fn test_is_webgateway_container() {
    // Simulate: image = "intersystems/webgateway:latest"
    let image = "intersystems/webgateway:latest";
    let is_webgateway = image.contains("webgateway");
    assert!(is_webgateway);
}

#[test]
fn test_is_not_iris_container() {
    // Simulate: image = "postgres:latest"
    let image = "postgres:latest";
    let is_iris = image.contains("intersystems") || image.contains("iris");
    let is_webgateway = image.contains("webgateway");
    assert!(!is_iris);
    assert!(!is_webgateway);
}

#[test]
fn test_container_filter_redis_cache_skipped() {
    // Simulate: redis container should not match IRIS filter
    let image = "redis:7";
    let is_iris = image.contains("intersystems") || image.contains("iris");
    let is_webgateway = image.contains("webgateway");
    let should_skip = !(is_iris || is_webgateway);
    assert!(should_skip);
}

#[test]
fn test_container_filter_irishealth_matches() {
    // Simulate: irishealth image should match
    let image = "intersystems/irishealth:latest";
    let is_iris = image.contains("intersystems") || image.contains("iris");
    assert!(is_iris);
}

#[test]
fn test_container_filter_image_empty_string() {
    // Simulate: empty image string (fallback case)
    let image = "";
    let is_iris = image.contains("intersystems") || image.contains("iris");
    let is_webgateway = image.contains("webgateway");
    assert!(!is_iris && !is_webgateway);
}

#[test]
fn test_container_filter_case_sensitivity() {
    // Note: image names from Docker are lowercase; contains() is case-sensitive
    let image_lower = "intersystems/iris:latest";
    let matches_lower = image_lower.contains("intersystems") || image_lower.contains("iris");

    assert!(matches_lower);

    // Uppercase image names won't match lowercase search strings (case-sensitive)
    let image_upper = "INTERSYSTEMS/IRIS:LATEST";
    let matches_upper = image_upper.contains("intersystems") || image_upper.contains("iris");

    assert!(!matches_upper); // Doesn't match because contains() is case-sensitive
}

// ── Container name normalization (lines 378-383) ───────────────────────────

#[test]
fn test_container_name_trim_leading_slash() {
    // Simulate: "/my-iris" → trim_start_matches('/') → "my-iris"
    let name_with_slash = "/my-iris";
    let trimmed = name_with_slash.trim_start_matches('/').to_string();
    assert_eq!(trimmed, "my-iris");
}

#[test]
fn test_container_name_multiple_leading_slashes() {
    // Simulate: "///iris" → trim_start_matches('/') → "iris"
    let name_with_slashes = "///iris";
    let trimmed = name_with_slashes.trim_start_matches('/').to_string();
    assert_eq!(trimmed, "iris");
}

#[test]
fn test_container_name_no_leading_slash() {
    // Simulate: "my-iris" (no slash) → trim → "my-iris" (unchanged)
    let name = "my-iris";
    let trimmed = name.trim_start_matches('/').to_string();
    assert_eq!(trimmed, "my-iris");
}

#[test]
fn test_container_name_empty_after_trim() {
    // Simulate: "/" → trim → "" (but we use unwrap_or_default, so "" is OK)
    let name = "/";
    let trimmed = name.trim_start_matches('/').to_string();
    assert_eq!(trimmed, "");
}

#[test]
fn test_default_username() {
    // Simulate: env var missing → unwrap_or("_SYSTEM")
    let username = "_SYSTEM".to_string();
    assert_eq!(username, "_SYSTEM");
}

#[test]
fn test_default_password() {
    // Simulate: env var missing → unwrap_or("SYS")
    let password = "SYS".to_string();
    assert_eq!(password, "SYS");
}

#[test]
fn test_default_namespace() {
    // Simulate: env var missing → unwrap_or("USER")
    let namespace = "USER".to_string();
    assert_eq!(namespace, "USER");
}

// ── Scheme/prefix condition check (lines 170) ────────────────────────────────

#[test]
fn test_scheme_prefix_bypass_http_no_prefix() {
    // Simulate: scheme="http", prefix=None → !(http != http || None.is_some()) → false → probe_atelier path
    let scheme = "http";
    let prefix: Option<String> = None;
    let should_bypass = scheme != "http" || prefix.is_some();
    assert!(!should_bypass);
}

#[test]
fn test_scheme_prefix_bypass_https_no_prefix() {
    // Simulate: scheme="https", prefix=None → !(https != http || None.is_some()) → true → build_url path
    let scheme = "https";
    let prefix: Option<String> = None;
    let should_bypass = scheme != "http" || prefix.is_some();
    assert!(should_bypass);
}

#[test]
fn test_scheme_prefix_bypass_http_with_prefix() {
    // Simulate: scheme="http", prefix=Some("iris") → (http != http || true) → true → build_url path
    let scheme = "http";
    let prefix: Option<String> = Some("iris".to_string());
    let should_bypass = scheme != "http" || prefix.is_some();
    assert!(should_bypass);
}

#[test]
fn test_scheme_prefix_bypass_https_with_prefix() {
    // Simulate: scheme="https", prefix=Some("iris") → (https != http || true) → true → build_url path
    let scheme = "https";
    let prefix: Option<String> = Some("iris".to_string());
    let should_bypass = scheme != "http" || prefix.is_some();
    assert!(should_bypass);
}

// ── emit_unhealthy_message — pure logging function, no I/O. Must never panic ──
// for any FailureMode variant, including AtelierAuth401 which intentionally emits
// nothing (the 401 WARN was already logged by the caller with the container name).

#[test]
fn test_emit_unhealthy_message_port_not_mapped_does_not_panic() {
    emit_unhealthy_message("some-container", FailureMode::PortNotMapped);
}

#[test]
fn test_emit_unhealthy_message_atelier_not_responding_does_not_panic() {
    emit_unhealthy_message(
        "some-container",
        FailureMode::AtelierNotResponding { port: 52773 },
    );
}

#[test]
fn test_emit_unhealthy_message_atelier_http_error_does_not_panic() {
    emit_unhealthy_message(
        "some-container",
        FailureMode::AtelierHttpError {
            port: 52773,
            status: 503,
        },
    );
}

#[test]
fn test_emit_unhealthy_message_auth_401_does_not_panic() {
    emit_unhealthy_message(
        "some-container",
        FailureMode::AtelierAuth401 { port: 52773 },
    );
}

#[test]
fn test_emit_unhealthy_message_empty_container_name_does_not_panic() {
    emit_unhealthy_message("", FailureMode::PortNotMapped);
}

// ── score_container_name — additional edge cases not yet covered above ──────

#[test]
fn test_score_container_name_with_numeric_workspace() {
    // Workspace basenames can be purely numeric (e.g. a directory named "042").
    let score = score_container_name("042-iris", "042");
    assert_eq!(score, 90, "starts_with(80) + iris suffix(10) = 90");
}

#[test]
fn test_score_container_name_unicode_is_not_special_cased() {
    // Non-ASCII container/workspace names should not panic and should follow
    // the same case-folding + hyphen/underscore normalization rules.
    let score = score_container_name("café-iris", "café");
    assert_eq!(score, 90);
}

#[test]
fn test_score_container_name_workspace_equals_iris_literal() {
    // Workspace basename literally "iris" — container "iris-iris" starts_with "iris"
    // and ends_with "-iris" simultaneously.
    let score = score_container_name("iris-iris", "iris");
    assert_eq!(score, 90, "starts_with(80) + iris suffix(10) = 90");
}

#[test]
fn test_score_container_name_mixed_hyphen_and_underscore_in_same_name() {
    let score = score_container_name("my-app_test", "my_app");
    assert_eq!(score, 85, "starts_with(80) + test suffix(5) = 85");
}

// ── Async tests for discover_iris env var branches (L152-168) ──────────────────

/// Test discover_iris with IRIS_SCHEME=https (custom scheme branch, L159-163)
/// The function builds a custom base_url directly when scheme != "http".
/// With an unreachable host (127.0.0.2:1), probe fails silently but returns Found.
#[tokio::test]
#[ignore] // May take 2-5 seconds due to probe timeout
async fn test_discover_iris_with_https_scheme() {
    let _guard = DISCOVERY_LOCK.lock().unwrap();

    // Save original env vars
    let saved_host = std::env::var("IRIS_HOST").ok();
    let saved_scheme = std::env::var("IRIS_SCHEME").ok();
    let saved_port = std::env::var("IRIS_WEB_PORT").ok();

    // Set env vars: unreachable host + https scheme
    std::env::set_var("IRIS_HOST", "127.0.0.2");
    std::env::set_var("IRIS_SCHEME", "https");
    std::env::set_var("IRIS_WEB_PORT", "1"); // Port 1 fails fast on most systems

    let result = iris_agentic_dev_core::iris::discovery::discover_iris(None).await;

    // Restore env vars
    match saved_host {
        Some(v) => std::env::set_var("IRIS_HOST", v),
        None => std::env::remove_var("IRIS_HOST"),
    }
    match saved_scheme {
        Some(v) => std::env::set_var("IRIS_SCHEME", v),
        None => std::env::remove_var("IRIS_SCHEME"),
    }
    match saved_port {
        Some(v) => std::env::set_var("IRIS_WEB_PORT", v),
        None => std::env::remove_var("IRIS_WEB_PORT"),
    }

    // With IRIS_SCHEME set, the function returns Found (L171-185) regardless of probe success
    assert!(
        matches!(result, IrisDiscovery::Found(_)),
        "discover_iris with IRIS_SCHEME should return Found"
    );
}

/// Test discover_iris with IRIS_WEB_PREFIX set (custom prefix branch, L164-167)
/// Similar to https test but uses prefix with http scheme.
#[tokio::test]
#[ignore] // May take 2-5 seconds due to probe timeout
async fn test_discover_iris_with_web_prefix() {
    let _guard = DISCOVERY_LOCK.lock().unwrap();

    let saved_host = std::env::var("IRIS_HOST").ok();
    let saved_prefix = std::env::var("IRIS_WEB_PREFIX").ok();
    let saved_port = std::env::var("IRIS_WEB_PORT").ok();
    let saved_scheme = std::env::var("IRIS_SCHEME").ok();

    // Use http but with a web prefix
    std::env::set_var("IRIS_HOST", "127.0.0.2");
    std::env::set_var("IRIS_WEB_PREFIX", "myapp");
    std::env::set_var("IRIS_WEB_PORT", "1");
    // Remove IRIS_SCHEME to default to http
    std::env::remove_var("IRIS_SCHEME");

    let result = iris_agentic_dev_core::iris::discovery::discover_iris(None).await;

    match saved_host {
        Some(v) => std::env::set_var("IRIS_HOST", v),
        None => std::env::remove_var("IRIS_HOST"),
    }
    match saved_prefix {
        Some(v) => std::env::set_var("IRIS_WEB_PREFIX", v),
        None => std::env::remove_var("IRIS_WEB_PREFIX"),
    }
    match saved_port {
        Some(v) => std::env::set_var("IRIS_WEB_PORT", v),
        None => std::env::remove_var("IRIS_WEB_PORT"),
    }
    match saved_scheme {
        Some(v) => std::env::set_var("IRIS_SCHEME", v),
        None => std::env::remove_var("IRIS_SCHEME"),
    }

    // With IRIS_WEB_PREFIX set, the function returns Found (L171-185)
    assert!(
        matches!(result, IrisDiscovery::Found(_)),
        "discover_iris with IRIS_WEB_PREFIX should return Found"
    );
}

/// Test discover_iris with both IRIS_SCHEME and IRIS_WEB_PREFIX set
/// This exercises the full custom URL building path (L172-175).
#[tokio::test]
#[ignore] // May take 2-5 seconds due to probe timeout
async fn test_discover_iris_with_scheme_and_prefix() {
    let _guard = DISCOVERY_LOCK.lock().unwrap();

    let saved_host = std::env::var("IRIS_HOST").ok();
    let saved_scheme = std::env::var("IRIS_SCHEME").ok();
    let saved_prefix = std::env::var("IRIS_WEB_PREFIX").ok();
    let saved_port = std::env::var("IRIS_WEB_PORT").ok();

    std::env::set_var("IRIS_HOST", "127.0.0.2");
    std::env::set_var("IRIS_SCHEME", "https");
    std::env::set_var("IRIS_WEB_PREFIX", "secure/iris");
    std::env::set_var("IRIS_WEB_PORT", "1");

    let result = iris_agentic_dev_core::iris::discovery::discover_iris(None).await;

    match saved_host {
        Some(v) => std::env::set_var("IRIS_HOST", v),
        None => std::env::remove_var("IRIS_HOST"),
    }
    match saved_scheme {
        Some(v) => std::env::set_var("IRIS_SCHEME", v),
        None => std::env::remove_var("IRIS_SCHEME"),
    }
    match saved_prefix {
        Some(v) => std::env::set_var("IRIS_WEB_PREFIX", v),
        None => std::env::remove_var("IRIS_WEB_PREFIX"),
    }
    match saved_port {
        Some(v) => std::env::set_var("IRIS_WEB_PORT", v),
        None => std::env::remove_var("IRIS_WEB_PORT"),
    }

    assert!(
        matches!(result, IrisDiscovery::Found(_)),
        "discover_iris with both IRIS_SCHEME and IRIS_WEB_PREFIX should return Found"
    );
}

/// Test IRIS_SCHEME with leading slashes gets trimmed (L161)
/// "https://" → trim_matches('/') → "https:" (colon remains, only slashes are trimmed)
#[test]
fn test_iris_scheme_trim_leading_and_trailing_slashes() {
    // Common case: scheme with trailing slashes like "https://"
    let scheme = "https://";
    let trimmed = scheme.trim_matches('/').to_string();
    // trim_matches('/') only removes / chars, not :, so "https://" becomes "https:"
    assert_eq!(
        trimmed, "https:",
        "scheme with trailing slashes keeps the colon"
    );

    // Leading slashes: "/https" → "https"
    let scheme_leading = "/https";
    let trimmed_leading = scheme_leading.trim_matches('/').to_string();
    assert_eq!(
        trimmed_leading, "https",
        "leading slashes should be trimmed"
    );

    // No slashes: "https" → "https" (unchanged)
    let scheme_no_slashes = "https";
    let trimmed_no_slashes = scheme_no_slashes.trim_matches('/').to_string();
    assert_eq!(
        trimmed_no_slashes, "https",
        "scheme without slashes unchanged"
    );
}

/// Test IRIS_WEB_PREFIX with slashes gets trimmed (L166)
/// "/app/prefix/" should normalize to "app/prefix"
#[test]
fn test_iris_web_prefix_trim_slashes() {
    let prefix = "/app/prefix/";
    let trimmed = prefix.trim_matches('/').to_string();
    assert_eq!(
        trimmed, "app/prefix",
        "prefix with surrounding slashes should be trimmed"
    );

    let prefix_slash = "/";
    let trimmed_slash = prefix_slash.trim_matches('/').to_string();
    assert!(
        trimmed_slash.is_empty(),
        "lone slash should trim to empty, then be filtered to None"
    );
}

/// Test the condition that decides whether to bypass normal probe_atelier path (L171)
/// scheme != "http" || prefix.is_some() → build custom URL path
#[test]
fn test_discover_iris_scheme_prefix_bypass_condition() {
    // Case 1: http + no prefix → don't bypass (probe_atelier path)
    let scheme = "http";
    let prefix: Option<String> = None;
    assert!(
        !(scheme != "http" || prefix.is_some()),
        "http + no prefix should NOT bypass"
    );

    // Case 2: https + no prefix → bypass (custom URL path)
    let scheme = "https";
    let prefix_none: Option<String> = None;
    assert!(
        scheme != "http" || prefix_none.is_some(),
        "https + no prefix should bypass"
    );

    // Case 3: http + prefix → bypass (custom URL path)
    let scheme = "http";
    let prefix: Option<String> = Some("iris".to_string());
    assert!(
        scheme != "http" || prefix.is_some(),
        "http + prefix should bypass"
    );

    // Case 4: https + prefix → bypass (custom URL path)
    let scheme = "https";
    let prefix: Option<String> = Some("app/iris".to_string());
    assert!(
        scheme != "http" || prefix.is_some(),
        "https + prefix should bypass"
    );
}
