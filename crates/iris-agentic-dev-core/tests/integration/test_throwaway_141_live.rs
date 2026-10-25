//! The throwaway IRIS container helper (spec 141, T003).
//!
//! Live: needs Docker and pulls `intersystemsdc/iris-community:latest` on first use. Run it under
//! the shared lock, single-threaded:
//!
//! ```text
//! lockf -k /tmp/iad-141-iris.lock cargo test --features testing --test integration \
//!     throwaway_141 -- --ignored --test-threads=1
//! ```

use iris_agentic_dev_core::testing::{throwaway_iris_present, ThrowawayIris, THROWAWAY_IRIS_NAME};

#[test]
#[ignore]
fn throwaway_iris_answers_http_and_is_removed_on_drop() {
    let Some(iris) = ThrowawayIris::start() else {
        return;
    };
    assert_eq!(iris.superserver_port, 39780);
    assert_eq!(iris.web_port, 39781);

    let status = iris
        .atelier_status()
        .expect("the web gateway must answer GET /api/atelier/");
    assert!(
        status.starts_with("HTTP/1."),
        "expected an HTTP status line from {THROWAWAY_IRIS_NAME}, got {status:?}"
    );
    assert!(
        throwaway_iris_present(),
        "{THROWAWAY_IRIS_NAME} must be running while the handle is alive"
    );

    drop(iris);

    assert!(
        !throwaway_iris_present(),
        "{THROWAWAY_IRIS_NAME} must be removed when the handle is dropped"
    );
}
