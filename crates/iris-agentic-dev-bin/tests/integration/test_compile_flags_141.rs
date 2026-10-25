//! Spec 141 C1: `iris-agentic-dev compile --flags` refuses a flags string outside the qualifier
//! charset before it resolves a connection.
//!
//! A loopback listener stands in for IRIS. The refusal has to come first, so the listener must see
//! no connection at all; a connection is the IRIS request this test rules out. No live IRIS needed.
//!
//! Run with:
//!   cargo build -p iris-agentic-dev && IAD_BINARY=./target/debug/iris-agentic-dev \
//!   cargo test --features testing --test bin_integration compile_flags_141 -- --include-ignored

use iris_agentic_dev_core::testing::{clean_command, require_iad_binary};
use std::net::TcpListener;

/// Each value would close the ObjectScript literal, start a new source line, or add a qualifier.
const OUT_OF_CHARSET: &[&str] = &["cuk\") Kill ^X //", "cuk\nSet x=1", "cu k"];

#[test]
#[ignore = "spawns the built binary; needs IAD_BINARY or a prior cargo build"]
fn compile_bad_flags_refused_before_iris() {
    let Some(bin) = require_iad_binary() else {
        return;
    };
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind a loopback port");
    listener
        .set_nonblocking(true)
        .expect("a nonblocking listener");
    let port = listener.local_addr().expect("the bound address").port();

    for flags in OUT_OF_CHARSET {
        let out = clean_command(&bin)
            .args(["compile", "--host", "127.0.0.1", "--web-port"])
            .arg(port.to_string())
            .arg(format!("--flags={flags}"))
            .output()
            .expect("spawning the binary should succeed");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_ne!(
            out.status.code(),
            Some(0),
            "{flags:?} must not compile: {stderr}"
        );
        assert!(
            stderr.contains("compile qualifier set"),
            "{flags:?} must get the charset refusal, not a connection error: {stderr}"
        );
    }

    assert!(
        listener.accept().is_err(),
        "the binary connected to a server before refusing the flags"
    );
}
