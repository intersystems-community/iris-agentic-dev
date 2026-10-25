//! Spec 141 C6: the `iris_test_server` ad-hoc `host` is a hostname or IP literal, nothing more.
//!
//! The host went into `format!("http://{host}:{port}")`, so `/`, `?`, `#` and `@` reshaped the
//! URL and reached a different address than the one named. A host with any of them, a port, or a
//! string that does not parse as a host is refused before a connection is made.

use iris_agentic_dev_core::tools::server_tools::probe_host_problem;

#[test]
fn ad_hoc_host_rejects_url_syntax() {
    for bad in [
        "a/b", "a#", "a?x", "u@h", "h:1", "h b", "[::1", "", "http://h",
    ] {
        assert!(
            probe_host_problem(bad).is_some(),
            "{bad:?} is not a bare host and must be refused"
        );
    }
}

#[test]
fn ad_hoc_host_accepts_names_and_ip_literals() {
    for good in [
        "localhost",
        "iris.example.com",
        "127.0.0.1",
        "[::1]",
        "10.0.0.5",
    ] {
        assert!(
            probe_host_problem(good).is_none(),
            "{good:?} is a host and must pass"
        );
    }
}
