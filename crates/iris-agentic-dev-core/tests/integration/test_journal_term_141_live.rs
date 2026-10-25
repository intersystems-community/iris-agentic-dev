//! Spec 141 T176, live: a `journal_search` term never reaches the IRIS journal.
//!
//! The search term used to be spliced into the scratch class that the tool compiles, so the class
//! source (a SET of `^oddDEF`) went into the journal and a search for a secret left the secret in
//! the journal. The term now travels in the `X-IAD-Args` header. The test searches for a sentinel
//! on the throwaway container, then reads the current journal file there and asserts the sentinel
//! is not in it.
//!
//! A control generator call with a literal in its source proves the file read can see generator
//! source, so the sentinel's absence means something.
//!
//! Run, serialised behind the shared lock:
//!
//! ```text
//! lockf -k /tmp/iad-141-iris.lock cargo test --features testing --test integration \
//!     test_journal_term_141 -- --ignored --test-threads=1
//! ```

use std::process::Command;

use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};
use iris_agentic_dev_core::testing::{ThrowawayIris, THROWAWAY_IRIS_NAME};
use iris_agentic_dev_core::tools::observability::journal_search_impl;

/// A sentinel unique to this run. The journal is append-only, so a sentinel fixed in advance would
/// still be in it from an earlier run (a failing one, say) and the check would prove nothing.
fn sentinel() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    format!("IadSearchTerm141-{nanos}")
}
const CONTROL: &str = "IadGenSource141Control";

fn make_conn(iris: &ThrowawayIris) -> (IrisConnection, reqwest::Client) {
    let conn = IrisConnection::new(
        format!("http://{}:{}", iris.host, iris.web_port),
        "USER",
        iris.username,
        iris.password,
        DiscoverySource::EnvVar,
    );
    (conn, reqwest::Client::new())
}

/// The path of the current journal file, as IRIS names it.
fn current_journal_file(iris: &ThrowawayIris) -> String {
    let out = iris.session(
        "%SYS",
        "Write ##class(%SYS.Journal.System).GetCurrentFileName(),!",
    );
    out.lines()
        .map(str::trim)
        .find(|l| l.starts_with('/') || l.contains(".00"))
        .unwrap_or_else(|| panic!("no journal file name in: {out}"))
        .to_string()
}

/// How many lines of the journal file at `path` contain `needle`. `grep -a` reads the binary
/// journal as text; exit status 1 from grep means no match and is a count of zero.
fn journal_hits(path: &str, needle: &str) -> usize {
    let out = Command::new("docker")
        .args([
            "exec",
            THROWAWAY_IRIS_NAME,
            "grep",
            "-a",
            "-c",
            "-F",
            needle,
            path,
        ])
        .output()
        .expect("docker exec grep");
    match out.status.code() {
        Some(0) => String::from_utf8_lossy(&out.stdout)
            .trim()
            .parse()
            .expect("grep count"),
        Some(1) => 0,
        _ => panic!(
            "grep failed on {path}: {}",
            String::from_utf8_lossy(&out.stderr)
        ),
    }
}

fn body(result: rmcp::model::CallToolResult) -> serde_json::Value {
    let text = result
        .content
        .first()
        .map(|c| c.as_text().unwrap().text.clone())
        .expect("text content");
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("not JSON ({e}): {text}"))
}

#[tokio::test]
#[ignore]
async fn journal_search_term_never_reaches_the_journal() {
    let Some(iris) = ThrowawayIris::start() else {
        return;
    };
    let (conn, client) = make_conn(&iris);

    // Control: a literal in generator source is journaled, so the file read can see it.
    conn.execute_via_generator(&format!("Write \"{CONTROL}\""), "%SYS", &client)
        .await
        .expect("control generator call");

    let sentinel = sentinel();
    let search = body(
        journal_search_impl(
            Some(&conn),
            "allow",
            Some(&sentinel),
            None,
            Some(10),
            &[],
            false,
        )
        .await
        .expect("Ok"),
    );
    assert_eq!(search["success"], true, "search failed: {search}");

    let journal = current_journal_file(&iris);
    assert!(
        journal_hits(&journal, CONTROL) > 0,
        "the control literal is not in {journal}, so the file read proves nothing"
    );
    let hits = journal_hits(&journal, &sentinel);
    assert_eq!(
        hits, 0,
        "the search term is in the journal file {journal} ({hits} lines)"
    );
}
