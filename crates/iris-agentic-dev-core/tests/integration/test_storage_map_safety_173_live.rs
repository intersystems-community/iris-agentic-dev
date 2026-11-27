//! Spec 173 T043 to T047: `propose` with `index_globals` runs under the controls the sampler
//! already had. A hostile name is `INVALID_PARAMS` and sends IRIS nothing; strict read-only
//! refuses before any request; a PHI-named candidate is judged by the global gate and listed
//! `refused`; the node cap and the time cap say which candidate they stopped; no value read from
//! a global reaches an answer; the call stays inside its budget; nothing the call touches changes.
//!
//! The "request counter" is a TCP proxy between the tool and the container that parses each HTTP
//! request line, so a call that never reaches IRIS is seen as zero requests, not inferred.
//!
//! Run: `IRIS_HOST=localhost IRIS_WEB_PORT=52780 nice -n 15 cargo test --features testing -j 2 \
//!   --test integration test_storage_map_safety_173 -- --test-threads=1 --include-ignored`

use crate::live_rig_171::{rig_with, Rig, Where};
use crate::storage173_rig::*;
use crate::test_storage_map_link_173_live::live;
use iris_agentic_dev_core::testing::LiveTarget;
use iris_agentic_dev_core::tools::Toolset;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

const OPEN: &str = "write_tools_enabled = true\ndestructive_tools_enabled = true\n";
const STRICT: &str =
    "write_tools_enabled = true\ndestructive_tools_enabled = true\nread_only = \"strict\"\n";

// ---- the request counter ------------------------------------------------------------------------

/// A socket on 127.0.0.1 that forwards to the container and logs the request line of every HTTP
/// request the client sends (`PUT /api/atelier/v1/USER/doc/IrisDevTmp.X.cls HTTP/1.1`).
struct Counter {
    port: u16,
    log: Arc<Mutex<Vec<String>>>,
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// Read the client's bytes, pass them on, and log each complete request (head and body).
async fn pump_up(
    mut from: tokio::net::tcp::OwnedReadHalf,
    mut to: tokio::net::tcp::OwnedWriteHalf,
    log: Arc<Mutex<Vec<String>>>,
) {
    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = vec![0u8; 64 * 1024];
    loop {
        let n = match from.read(&mut chunk).await {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        if to.write_all(&chunk[..n]).await.is_err() {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        while let Some(end) = find(&buf, b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&buf[..end]).to_string();
            let len = head
                .lines()
                .find_map(|l| {
                    let (k, v) = l.split_once(':')?;
                    k.eq_ignore_ascii_case("content-length")
                        .then(|| v.trim().parse::<usize>().ok())
                        .flatten()
                })
                .unwrap_or(0);
            if buf.len() < end + 4 + len {
                break;
            }
            let line = head.lines().next().unwrap_or("").to_string();
            if ["GET ", "PUT ", "POST ", "DELETE ", "HEAD "]
                .iter()
                .any(|m| line.starts_with(m))
            {
                log.lock().unwrap().push(line);
            }
            buf.drain(..end + 4 + len);
        }
    }
    let _ = to.shutdown().await;
}

impl Counter {
    async fn start(target: String) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let log: Arc<Mutex<Vec<String>>> = Arc::default();
        let seen = log.clone();
        tokio::spawn(async move {
            loop {
                let Ok((client, _)) = listener.accept().await else {
                    return;
                };
                let (target, seen) = (target.clone(), seen.clone());
                tokio::spawn(async move {
                    let Ok(server) = TcpStream::connect(&target).await else {
                        return;
                    };
                    let (cr, cw) = client.into_split();
                    let (mut sr, sw) = server.into_split();
                    let mut cw = cw;
                    let up = pump_up(cr, sw, seen);
                    let down = async move {
                        let _ = tokio::io::copy(&mut sr, &mut cw).await;
                        let _ = cw.shutdown().await;
                    };
                    tokio::join!(up, down);
                });
            }
        });
        Counter { port, log }
    }

    fn mark(&self) -> usize {
        self.log.lock().unwrap().len()
    }

    fn since(&self, mark: usize) -> Vec<String> {
        self.log.lock().unwrap()[mark..].to_vec()
    }
}

/// One IRIS call, as the plan's budget table counts them: a scratch-class round trip (the
/// document PUT that starts it) or a class read (a document GET). Its compile, query and delete
/// requests belong to the same call.
fn iris_calls(requests: &[String]) -> Vec<&String> {
    requests
        .iter()
        .filter(|l| {
            (l.starts_with("PUT ") && l.contains("/doc/IrisDevTmp."))
                || (l.starts_with("GET ") && l.contains("/doc/"))
        })
        .collect()
}

/// A rig whose requests all pass through the counter.
async fn counted(operator: &str) -> (Rig, Counter) {
    let at = Where::live();
    let t = LiveTarget::from_env();
    let counter = Counter::start(format!("{}:{}", t.host, t.web_port)).await;
    let through = at.through(format!("http://127.0.0.1:{}", counter.port));
    let r = rig_with(&through, Toolset::Merged, operator).await;
    (r, counter)
}

async fn done(r: &Rig, f: &Fixture) {
    drop_fixture_173(r, f).await;
    drop_all(r).await;
}

fn code(out: &Value) -> &str {
    out["error_code"].as_str().unwrap_or("")
}

// ---- T043: hostile names, strict, the global gate, the caps -------------------------------------

/// Names that must never reach generated code. Each is `INVALID_PARAMS` (a list of the wrong
/// shape is refused earlier still, by the parameter types), and the connection sees no request
/// while the call is judged (the counter is checked around the call alone). A clean
/// call afterwards moves the counter, so a zero is a measurement and not a dead counter.
#[tokio::test]
#[ignore]
async fn hostile_index_globals_are_invalid_params_and_send_iris_nothing() {
    let (r, counter) = counted(OPEN).await;
    drop_all(&r).await;
    let f = Fixture::read("F02");
    load_fixture_173(&r, &f).await;
    let master = f.seed["master_global"].as_str().unwrap().to_string();
    let nine: Vec<String> = (1..=9).map(|i| format!("^IadSm173N{i}")).collect();
    let hostile: Vec<(&str, Value)> = vec![
        ("statement break", json!(["X\") Kill ^Y"])),
        ("subscripted", json!(["^A(\"x\")"])),
        ("blank in name", json!(["a b"])),
        ("extended reference", json!(["^|\"OTHER\"|G"])),
        ("nine names", json!(nine)),
        ("empty name", json!([""])),
        ("two carets", json!(["^^G"])),
    ];
    // Shapes the typed parameters refuse before the handler runs (a JSON-RPC invalid-params
    // error over MCP; `bad params for iris_storage_map` through the dispatch tail the tests use).
    let typed: Vec<(&str, Value)> = vec![
        ("empty string, not a list", json!("")),
        ("a name that is not a string", json!([7])),
    ];
    let mut failures = Vec::new();
    for (label, names) in &hostile {
        let before = counter.mark();
        let out = ask_propose(&r, json!({"global": master, "index_globals": names})).await;
        let sent = counter.since(before);
        if code(&out) != "INVALID_PARAMS" || out["success"] == json!(true) {
            failures.push(format!("{label}: not INVALID_PARAMS: {out}"));
        }
        if !sent.is_empty() {
            failures.push(format!("{label}: reached IRIS with {sent:?}"));
        }
    }
    for (label, names) in &typed {
        let before = counter.mark();
        let out = ask_propose(&r, json!({"global": master, "index_globals": names})).await;
        let sent = counter.since(before);
        let said = out["error"].as_str().unwrap_or_default();
        if !said.starts_with("bad params for iris_storage_map") || out["success"] == json!(true) {
            failures.push(format!(
                "{label}: not refused by the parameter types: {out}"
            ));
        }
        if !sent.is_empty() {
            failures.push(format!("{label}: reached IRIS with {sent:?}"));
        }
    }
    // The control: a good name is judged, sampled and linked, and the counter saw it.
    let before = counter.mark();
    let index = f.seed["index_globals"][0].as_str().unwrap();
    let ok = ask_propose(&r, json!({"global": master, "index_globals": [index]})).await;
    let sent = counter.since(before);
    assert_eq!(ok["success"], true, "{ok}");
    assert_eq!(ok["indexes"][0]["verdict"], "inferred", "{ok}");
    assert!(
        iris_calls(&sent).len() >= 3,
        "the counter did not see the control call: {sent:?}"
    );
    done(&r, &f).await;
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Strict read-only refuses `propose` (global mode with `index_globals`, and class mode) before
/// any request is sent, and the answer says so. The same call on an open rig is not refused, so
/// the refusal is the setting's.
#[tokio::test]
#[ignore]
async fn strict_read_only_refuses_propose_with_index_globals_and_class_before_any_request() {
    let (strict, counter) = counted(STRICT).await;
    let f = Fixture::read("F02");
    let master = f.seed["master_global"].as_str().unwrap();
    let index = f.seed["index_globals"][0].as_str().unwrap();
    let calls = [
        json!({"global": master, "index_globals": [index]}),
        json!({"global": master}),
        json!({"class": "IadSm173.F35"}),
        json!({"class": "IadSm173.F35", "index_globals": ["^IadSm173F35X"]}),
    ];
    let mut failures = Vec::new();
    for args in calls {
        let before = counter.mark();
        let got = ask_propose(&strict, args.clone()).await;
        let sent = counter.since(before);
        if code(&got) != "READ_ONLY_STRICT" {
            failures.push(format!("{args}: not refused: {got}"));
        }
        if !sent.is_empty() {
            failures.push(format!("{args}: sent {sent:?} before refusing"));
        }
    }
    // Control: a plain read still answers under strict, so the rig itself is working.
    let open = live().await;
    load_fixture_173(&open, &f).await;
    let ok = ask_propose(&open, json!({"global": master, "index_globals": [index]})).await;
    assert_eq!(ok["success"], true, "an open rig is not refused: {ok}");
    done(&open, &f).await;
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A candidate whose name the global gate protects is judged by that gate and listed `refused`;
/// the other candidate is still examined and linked; nothing is sampled from the refused one. With
/// `acknowledgePhi` the same name is examined, so the refusal is the gate's.
#[tokio::test]
#[ignore]
async fn a_phi_named_candidate_is_refused_by_the_global_gate_and_the_rest_still_link() {
    let (r, counter) = counted(OPEN).await;
    drop_all(&r).await;
    let f = Fixture::read("F02");
    load_fixture_173(&r, &f).await;
    let master = f.seed["master_global"].as_str().unwrap();
    let good = f.seed["index_globals"][0].as_str().unwrap();
    // `^PAPER*` is on the PHI name list. Three nodes the planted index cannot be linked from.
    let phi = "^PAPERIadSm173Idx";
    exec(
        &r,
        "Set ^PAPERIadSm173Idx(\"ZQ1\",1)=\"\",^PAPERIadSm173Idx(\"ZQ2\",2)=\"\" Write \"SET\"",
    )
    .await;
    let before = counter.mark();
    let out = ask_propose(&r, json!({"global": master, "index_globals": [phi, good]})).await;
    let sent = counter.since(before);
    let seen = ask_propose(
        &r,
        json!({"global": master, "index_globals": [phi, good], "acknowledgePhi": true}),
    )
    .await;
    exec(&r, "Kill ^PAPERIadSm173Idx Write \"KILLED\"").await;
    assert_eq!(out["success"], true, "{out}");
    let cand = |o: &Value, g: &str| -> Value {
        o["index_candidates"]["candidates"]
            .as_array()
            .and_then(|a| a.iter().find(|c| c["global"] == json!(g)).cloned())
            .unwrap_or_else(|| panic!("no candidate {g}: {}", o["index_candidates"]))
    };
    assert_eq!(cand(&out, phi)["status"], "refused", "{out}");
    let why = cand(&out, phi)["reason"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    assert!(
        why.contains("PHI") || why.contains("acknowledgePhi"),
        "the reason names the gate: {why}"
    );
    assert_eq!(cand(&out, good)["status"], "examined", "{out}");
    let linked: Vec<&str> = out["indexes"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|i| i["global"].as_str())
        .collect();
    assert_eq!(
        linked,
        vec![good],
        "only the clean candidate is linked: {out}"
    );
    // The refused name never reached a sampler: the same call minus the refused name costs the
    // same number of IRIS calls (candidates are judged before any is run).
    let calls = iris_calls(&sent).len();
    let alone_before = counter.mark();
    let alone = ask_propose(&r, json!({"global": master, "index_globals": [good]})).await;
    let alone_calls = iris_calls(&counter.since(alone_before)).len();
    assert_eq!(alone["success"], true, "{alone}");
    assert_eq!(
        calls, alone_calls,
        "the refused candidate cost IRIS calls: with {calls}, without {alone_calls}"
    );
    // With the acknowledgement the gate lets it through to the sampler.
    assert_eq!(cand(&seen, phi)["status"], "examined", "{seen}");
    done(&r, &f).await;
}

/// `max_nodes = 30` caps each candidate at 30 nodes and `limits.stopped` says which candidate
/// stopped on which cap. F38 holds 1,200 rows behind each of its two index globals, so both stop.
#[tokio::test]
#[ignore]
async fn the_node_cap_names_the_candidate_it_stopped() {
    let r = live().await;
    let f = Fixture::read("F38");
    load_fixture_173(&r, &f).await;
    let master = f.seed["master_global"].as_str().unwrap();
    let globals = f.seed["index_globals"].as_array().unwrap().clone();
    assert_eq!(globals.len(), 2, "the fixture has two index globals");
    let out = ask_propose(
        &r,
        json!({"global": master, "index_globals": globals, "max_nodes": 30}),
    )
    .await;
    assert_eq!(out["success"], true, "{out}");
    assert_eq!(out["limits"]["node_cap"], 30, "{}", out["limits"]);
    let stopped = out["limits"]["stopped"].as_array().unwrap();
    // Each candidate is named with the cap that stopped it. `X` is keyed by 1,200 distinct names,
    // more than the 300 first-subscript values the counting pass may walk (ten times the node
    // cap), so that cap trips first; `Y` has ten cities and stops on the node cap.
    let want = [(&globals[0], "count_cap"), (&globals[1], "nodes")];
    for (g, cap) in want {
        assert!(
            stopped
                .iter()
                .any(|s| s["candidate"] == *g && s["cap"] == cap),
            "{g} not named as stopped on {cap}: {}",
            out["limits"]
        );
    }
    assert_eq!(stopped.len(), 2, "{}", out["limits"]);
    // The cap bounds the work: no candidate's sample is longer than the cap.
    for item in out["indexes"].as_array().unwrap() {
        let nodes = item["nodes_sampled"].as_u64().unwrap_or(0);
        assert!(nodes > 0 && nodes <= 30, "nodes {nodes} in {item}");
    }
    // And a cap that is not reached stops nothing: 1,000 nodes of a 1,200-row index still stop,
    // but a 40-row index under the default cap does not.
    let g = Fixture::read("F02");
    load_fixture_173(&r, &g).await;
    let small = ask_propose(
        &r,
        json!({"global": g.seed["master_global"], "index_globals": g.seed["index_globals"]}),
    )
    .await;
    assert_eq!(small["limits"]["stopped"], json!([]), "{}", small["limits"]);
    drop_fixture_173(&r, &g).await;
    done(&r, &f).await;
}
