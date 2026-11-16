//! The timeout of a run over HTTP (issue #191): held to a cap, and the request that runs the code
//! waits longer than the caller does, so the caller's own `TIMEOUT` is the one that fires.

use iris_agentic_dev_core::iris::connection::REQUEST_TIMEOUT_MARGIN;
use iris_agentic_dev_core::tools::exec::{http_timeout, MAX_HTTP_TIMEOUT_S};
use std::time::Duration;

#[test]
fn a_timeout_within_the_cap_is_kept() {
    for secs in [0, 1, 30, 120, MAX_HTTP_TIMEOUT_S] {
        assert_eq!(
            http_timeout(Duration::from_secs(secs)),
            Duration::from_secs(secs)
        );
    }
}

#[test]
fn a_timeout_above_the_cap_is_held_to_it() {
    assert_eq!(MAX_HTTP_TIMEOUT_S, 600, "the documented cap");
    for secs in [MAX_HTTP_TIMEOUT_S + 1, 3_600, 100_000, u64::MAX / 2] {
        assert_eq!(
            http_timeout(Duration::from_secs(secs)),
            Duration::from_secs(MAX_HTTP_TIMEOUT_S)
        );
    }
}

#[test]
fn the_request_outlasts_the_callers_timeout() {
    assert!(REQUEST_TIMEOUT_MARGIN >= Duration::from_secs(1));
}

#[test]
fn the_docs_state_the_cap_and_the_field_that_reports_it() {
    let tools =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/tools.md"))
            .expect("docs/tools.md");
    let row = tools
        .lines()
        .find(|l| l.starts_with("| `timeout`") && l.contains("OBJECTSCRIPT_TEST_TIMEOUT"))
        .expect("the iris_execute timeout row");
    assert!(
        row.contains("600") && row.contains("timeout_effective_s"),
        "{row}"
    );
}
