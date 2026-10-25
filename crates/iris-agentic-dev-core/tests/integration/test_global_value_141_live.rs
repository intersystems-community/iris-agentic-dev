//! Spec 141 C3: `iris_global set` keeps a `value` on one source line.
//!
//! Before 141 the value went into the scratch method as a quoted literal with its newlines intact,
//! so a newline began a new source line. `#execute` on that line would run in the compiled method.
//! The first test records the severity verdict on the code it runs against: whether the sentinel
//! global gets set. The fix keeps the value as one expression, so the sentinel stays undefined and
//! the value round-trips.
//!
//! Run: `IRIS_HOST=localhost IRIS_WEB_PORT=52780 IRIS_USERNAME=_SYSTEM IRIS_PASSWORD=SYS
//! cargo test --features testing --test integration test_global_value_141 -- --include-ignored
//! --test-threads=1`

use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};
use iris_agentic_dev_core::tools::global::{handle_iris_global, IrisGlobalParams};

fn make_conn() -> Option<(IrisConnection, reqwest::Client)> {
    let iris_host = std::env::var("IRIS_HOST").unwrap_or_default();
    if iris_host.is_empty() {
        return None;
    }
    let web_port = std::env::var("IRIS_WEB_PORT").unwrap_or_else(|_| "52780".to_string());
    let username = std::env::var("IRIS_USERNAME").unwrap_or_else(|_| "_SYSTEM".to_string());
    let password = std::env::var("IRIS_PASSWORD").unwrap_or_else(|_| "SYS".to_string());
    let conn = IrisConnection::new(
        format!("http://{}:{}", iris_host, web_port),
        "USER",
        username,
        password,
        DiscoverySource::EnvVar,
    );
    Some((conn, reqwest::Client::new()))
}

fn params(action: &str, global_name: &str, value: Option<&str>) -> IrisGlobalParams {
    IrisGlobalParams {
        confirm: false,
        action: action.to_string(),
        global_name: global_name.to_string(),
        subscripts: None,
        value: value.map(str::to_string),
        namespace: None,
        subtree: None,
        max_nodes: None,
        max_subscripts: None,
        acknowledge_phi: None,
        server: None,
    }
}

#[tokio::test]
#[ignore]
async fn a_newline_in_value_cannot_add_a_source_line() {
    let Some((conn, client)) = make_conn() else {
        eprintln!("IRIS_HOST not set; skipping");
        return;
    };
    handle_iris_global(
        &conn,
        &client,
        &params("kill", "IadSentinel141", None),
        Ok(()),
    )
    .await;

    let value = "a\n#execute Set ^IadSentinel141=1";
    let set = handle_iris_global(
        &conn,
        &client,
        &params("set", "IrisDevTest.Glob141Newline", Some(value)),
        Ok(()),
    )
    .await;
    let sentinel = handle_iris_global(
        &conn,
        &client,
        &params("get", "IadSentinel141", None),
        Ok(()),
    )
    .await;
    // The verdict for R5 C3: `set_success` false and `sentinel_defined` true would mean a newline
    // in `value` runs as source; any other pair means it cannot.
    eprintln!(
        "C3 verdict: set_success={} sentinel_defined={} set={set}",
        set["success"], sentinel["defined"]
    );

    handle_iris_global(
        &conn,
        &client,
        &params("kill", "IadSentinel141", None),
        Ok(()),
    )
    .await;
    handle_iris_global(
        &conn,
        &client,
        &params("kill", "IrisDevTest.Glob141Newline", None),
        Ok(()),
    )
    .await;
    assert_eq!(
        sentinel["defined"], false,
        "the value ran as source: {sentinel}"
    );
}

#[tokio::test]
#[ignore]
async fn a_value_with_newlines_round_trips() {
    let Some((conn, client)) = make_conn() else {
        eprintln!("IRIS_HOST not set; skipping");
        return;
    };
    // The get path drops `\r` and trims trailing whitespace on the way back (a read-side gap,
    // recorded in the 141 notes); the newlines are what this item fixes.
    let value = "l1\nl2\nl3";
    let set = handle_iris_global(
        &conn,
        &client,
        &params("set", "IrisDevTest.Glob141Lines", Some(value)),
        Ok(()),
    )
    .await;
    assert_eq!(set["success"], true, "set failed: {set}");

    let got = handle_iris_global(
        &conn,
        &client,
        &params("get", "IrisDevTest.Glob141Lines", None),
        Ok(()),
    )
    .await;
    handle_iris_global(
        &conn,
        &client,
        &params("kill", "IrisDevTest.Glob141Lines", None),
        Ok(()),
    )
    .await;
    assert_eq!(got["value"], value, "round trip: {got}");
}

/// Spec 141 T170: `get` returns the stored value exactly. The read side used to trim the whole
/// generator output, which drops a trailing `\r\n  ` from the value, and the line reader on the
/// way back does not keep a `\r`.
#[tokio::test]
#[ignore]
async fn a_value_with_cr_and_trailing_space_round_trips() {
    let Some((conn, client)) = make_conn() else {
        eprintln!("IRIS_HOST not set; skipping");
        return;
    };
    let value = "l1\nl2\r\n  ";
    let set = handle_iris_global(
        &conn,
        &client,
        &params("set", "IrisDevTest.Glob141Cr", Some(value)),
        Ok(()),
    )
    .await;
    assert_eq!(set["success"], true, "set failed: {set}");

    let got = handle_iris_global(
        &conn,
        &client,
        &params("get", "IrisDevTest.Glob141Cr", None),
        Ok(()),
    )
    .await;
    handle_iris_global(
        &conn,
        &client,
        &params("kill", "IrisDevTest.Glob141Cr", None),
        Ok(()),
    )
    .await;
    assert_eq!(got["value"], value, "round trip: {got}");
}

/// T170: a long value with CRs and line breaks comes back whole. The code-point read side writes one
/// line per `get`, so a long value must not hit a line limit in the generator's reader.
#[tokio::test]
#[ignore]
async fn a_long_value_with_line_breaks_round_trips() {
    let Some((conn, client)) = make_conn() else {
        eprintln!("IRIS_HOST not set; skipping");
        return;
    };
    let value = "ab\r\n".repeat(1500);
    let set = handle_iris_global(
        &conn,
        &client,
        &params("set", "IrisDevTest.Glob141Long", Some(&value)),
        Ok(()),
    )
    .await;
    assert_eq!(set["success"], true, "set failed: {set}");

    let got = handle_iris_global(
        &conn,
        &client,
        &params("get", "IrisDevTest.Glob141Long", None),
        Ok(()),
    )
    .await;
    handle_iris_global(
        &conn,
        &client,
        &params("kill", "IrisDevTest.Glob141Long", None),
        Ok(()),
    )
    .await;
    assert_eq!(
        got["value"].as_str().map(str::len),
        Some(value.len()),
        "length: {got}"
    );
    assert!(
        got["value"] == value,
        "the long value changed on the way back"
    );
}

fn subtree_params(global_name: &str) -> IrisGlobalParams {
    IrisGlobalParams {
        subtree: Some(true),
        ..params("get", global_name, None)
    }
}

/// T171: a subtree `get` returns each node's value exactly, as single `get` does since T170. The
/// read side used to trim the generator output, which dropped a trailing `\r\n  ` from the last
/// node, and the line reader does not keep a `\r` anywhere.
#[tokio::test]
#[ignore]
async fn a_subtree_get_keeps_cr_and_trailing_space() {
    let Some((conn, client)) = make_conn() else {
        eprintln!("IRIS_HOST not set; skipping");
        return;
    };
    let name = "IrisDevTest.Glob141Tree";
    let values = ["l1\r\nl2", "a|b", "é☃", "last\r\n  "];
    for (i, v) in values.iter().enumerate() {
        let set = handle_iris_global(
            &conn,
            &client,
            &IrisGlobalParams {
                subscripts: Some(vec![(i + 1).to_string()]),
                ..params("set", name, Some(v))
            },
            Ok(()),
        )
        .await;
        assert_eq!(set["success"], true, "set failed: {set}");
    }
    let got = handle_iris_global(&conn, &client, &subtree_params(name), Ok(())).await;
    handle_iris_global(&conn, &client, &params("kill", name, None), Ok(())).await;
    assert_eq!(got["success"], true, "subtree get failed: {got}");
    let nodes = got["nodes"].as_array().expect("nodes array");
    assert_eq!(nodes.len(), 4, "node count: {got}");
    for (node, want) in nodes.iter().zip(values) {
        assert_eq!(node["value"], want, "node: {node}");
    }
}

/// Build `^name(1)` inside IRIS with `reps` copies of `xy\r\n`. The set path puts the value into
/// generated source, which has its own size limit, so a large value is built on the IRIS side.
async fn build_big_node(conn: &IrisConnection, client: &reqwest::Client, name: &str, reps: u32) {
    let build = format!(
        " Set v = \"\" For i = 1:1:{reps} {{ Set v = v_\"xy\"_$Char(13,10) }}\n Set ^{name}(1) = v"
    );
    conn.execute_via_generator(&build, "USER", client)
        .await
        .expect("build the big value");
}

/// T171: a 100 KB value in a subtree node, and the same value by single `get`, come back whole.
/// The generator's line reader returns 32,000 characters per line and puts a line feed after each
/// piece, so the answer arrives in pieces that the parsers join.
#[tokio::test]
#[ignore]
async fn a_100kb_value_comes_back_whole_from_subtree_and_single_get() {
    let Some((conn, client)) = make_conn() else {
        eprintln!("IRIS_HOST not set; skipping");
        return;
    };
    let name = "IrisDevTest.Glob141TreeBig";
    let value = "xy\r\n".repeat(25_000);
    build_big_node(&conn, &client, name, 25_000).await;
    let started = std::time::Instant::now();
    let tree = handle_iris_global(&conn, &client, &subtree_params(name), Ok(())).await;
    let tree_took = started.elapsed();
    let single = handle_iris_global(
        &conn,
        &client,
        &IrisGlobalParams {
            subscripts: Some(vec!["1".to_string()]),
            ..params("get", name, None)
        },
        Ok(()),
    )
    .await;
    handle_iris_global(&conn, &client, &params("kill", name, None), Ok(())).await;
    eprintln!("T171 100 KB subtree get took {tree_took:?}");
    assert_eq!(tree["success"], true, "subtree get failed: {tree}");
    let nodes = tree["nodes"].as_array().expect("nodes array");
    assert_eq!(nodes.len(), 1, "node count");
    let back = nodes[0]["value"].as_str().expect("string value");
    assert_eq!(back.len(), value.len(), "subtree length");
    assert!(back == value, "the subtree value changed on the way back");
    assert_eq!(single["success"], true, "single get failed: {single}");
    let back = single["value"].as_str().expect("string value");
    assert_eq!(back.len(), value.len(), "single length");
    assert!(
        back == value,
        "the single-get value changed on the way back"
    );
}
