//! S8: every path that reaches a server by name uses that server's own client, so its certificate
//! setting is the server's own and not the launch environment's.
//!
//! The process is launched with `IRIS_TLS_VERIFY=false`, which applies to the connection the launch
//! settings describe and to nothing else. The listener behind the pool member `strict` presents a
//! self-signed certificate and `servers.json` does not turn checks off for it, so a client that
//! checks certificates fails the handshake and the listener reads no request. A tool that sent one
//! used the startup client's setting. The listener plays a web server for the TLS handshake only;
//! no IRIS is involved.
//!
//! Run: `IAD_BINARY=$PWD/target/debug/iris-agentic-dev cargo test --features testing --test binary
//! by_name_tls_171 -- --ignored --test-threads=1`

use crate::test_creds_binary_141::{basic_401, home_env, self_signed_config, Recorder};
use iris_agentic_dev_core::testing::{require_iad_binary, McpSession};
use serde_json::json;

#[test]
#[ignore = "spawns the built binary and needs openssl; run with --include-ignored"]
fn a_tool_that_names_a_server_checks_that_servers_certificate() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let tmp = tempfile::tempdir().unwrap();
    let config = self_signed_config(tmp.path());
    let strict = Recorder::tls(config.clone(), basic_401);
    // The control: the same listener kind, with its certificate checks turned off in its own entry.
    // Every call below must reach it, or a call that never got as far as a request would pass for
    // the strict server too.
    let lax = Recorder::tls(config, basic_401);

    let home = tmp.path().join("home");
    let cfg_dir = home.join(".config").join("iris-agentic-dev");
    std::fs::create_dir_all(&cfg_dir).unwrap();
    std::fs::write(
        cfg_dir.join("servers.json"),
        format!(
            "{{\"version\":1,\"servers\":{{\"strict\":{{\"host\":\"127.0.0.1\",\"port\":{},\
             \"namespace\":\"USER\",\"username\":\"u\",\"password\":\"p\",\"scheme\":\"https\",\
             \"password_endpoint\":\"https://127.0.0.1:{}\"}},\"lax\":{{\"host\":\"127.0.0.1\",\"port\":{},\
             \"namespace\":\"USER\",\"username\":\"u\",\"password\":\"p\",\"scheme\":\"https\",\
             \"password_endpoint\":\"https://127.0.0.1:{}\",\"tls_verify\":false}}}}}}",
            strict.port, strict.port, lax.port, lax.port
        ),
    )
    .unwrap();
    // The write tier is declared per server, because a member whose SystemMode cannot be read
    // starts write-closed and the mirror actions are write-capable.
    std::fs::write(
        home.join(".iris-agentic-dev.toml"),
        "[policy.lax]\nwriteTools = true\ndestructiveTools = true\n\
         [policy.strict]\nwriteTools = true\ndestructiveTools = true\n",
    )
    .unwrap();
    let cwd = tmp.path().join("cwd");
    std::fs::create_dir_all(&cwd).unwrap();

    let mut s = McpSession::start_in(
        Some(&cwd),
        &home_env(
            &home,
            &[("IAD_NO_KEYCHAIN", "1"), ("IRIS_TLS_VERIFY", "false")],
        ),
    );
    // Each call names its server as `$S`.
    let calls = [
        ("iris_test_server", json!({"name": "$S"})),
        ("iris_execute", json!({"code": "Write 1", "server": "$S"})),
        ("iris_namespace_list", json!({"server": "$S"})),
        ("iris_database_list", json!({"server": "$S"})),
        (
            "compare_document",
            json!({"server_a": "$S", "server_b": "$S", "document": "A.B.cls"}),
        ),
        (
            "compare_namespace",
            json!({"server_a": "$S", "server_b": "$S", "namespace": "USER"}),
        ),
        (
            "global_preview",
            json!({"global": "^Zq9171", "server": "$S"}),
        ),
        (
            "iris_global",
            json!({"action": "get", "global_name": "^Zq9171", "server": "$S"}),
        ),
        (
            "iris_admin",
            json!({"action": "mirror_failover", "confirm": true, "server": "$S"}),
        ),
        (
            "iris_admin",
            json!({"action": "mirror_add_async", "mirror_name": "M", "primary_host": "h", "server": "$S"}),
        ),
    ];
    let named = |args: &serde_json::Value, server: &str| {
        serde_json::from_str::<serde_json::Value>(&args.to_string().replace("$S", server)).unwrap()
    };
    let mut offenders = Vec::new();
    for (tool, args) in &calls {
        let before = lax.heads().len();
        let answer = s.call(tool, &named(args, "lax"));
        assert!(
            lax.heads().len() > before,
            "{tool} sent nothing to the server whose checks are off, so it proves nothing about \
             the strict one: {answer}"
        );
        let before = strict.heads().len();
        s.call(tool, &named(args, "strict"));
        if strict.heads().len() != before {
            offenders.push(*tool);
        }
    }
    assert!(
        offenders.is_empty(),
        "these tools sent a request to a server whose certificate does not validate: {offenders:?}"
    );
}
