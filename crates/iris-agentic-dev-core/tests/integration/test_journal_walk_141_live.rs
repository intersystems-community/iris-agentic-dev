//! Spec 141 T180, live: `journal_search` walks the journal without writing to `messages.log`.
//!
//! `%SYS.Journal.File:Search` writes one `[Utility.Event] thisrec:` line to `messages.log` for every
//! journal record it visits. Measured on a fresh `intersystemsdc/iris-community:2026.2` the journal
//! holds a few hundred records, and one search wrote 733 lines; on a busy instance one search wrote
//! about 500,000 lines, `messages.log` rolled over and `iris-main` exited, which stopped IRIS. The
//! search is now a walk over `FirstRecordGet()` / `Next`, which logs nothing.
//!
//! Every case here runs on the throwaway container, never on `iris-dev-iris`: a journal search
//! against a large journal is exactly what took that container down.
//!
//! The control calls `Search` itself and asserts the line count grows, so a `messages.log` that
//! never logs (or a grep that reads the wrong file) cannot make the walk look clean.
//!
//! Run, serialised behind the shared lock:
//!
//! ```text
//! lockf -k /tmp/iad-141-iris.lock cargo test --features testing --test integration \
//!     test_journal_walk_141 -- --ignored --test-threads=1
//! ```

use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};
use iris_agentic_dev_core::testing::{ThrowawayIris, THROWAWAY_IRIS_NAME};
use iris_agentic_dev_core::tools::observability::{
    journal_entries_impl, journal_search_impl, journal_search_with_limits, JournalQuery,
    JournalWalkLimits,
};

const MESSAGES_LOG: &str = "/usr/irissys/mgr/messages.log";

fn conn(iris: &ThrowawayIris) -> IrisConnection {
    IrisConnection::new(
        format!("http://{}:{}", iris.host, iris.web_port),
        "USER",
        iris.username,
        iris.password,
        DiscoverySource::EnvVar,
    )
}

/// How many `thisrec` lines `messages.log` holds. `grep -c` exits 1 on zero matches and still
/// prints `0`, so the exit status is ignored and the printed count read.
fn thisrec_lines() -> usize {
    let out = std::process::Command::new("docker")
        .args([
            "exec",
            THROWAWAY_IRIS_NAME,
            "grep",
            "-c",
            "thisrec",
            MESSAGES_LOG,
        ])
        .output()
        .expect("docker exec grep");
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse()
        .unwrap_or_else(|_| {
            panic!(
                "grep printed no count for {MESSAGES_LOG}: {}",
                String::from_utf8_lossy(&out.stderr)
            )
        })
}

fn body(result: rmcp::model::CallToolResult) -> serde_json::Value {
    let text = result
        .content
        .first()
        .map(|c| c.as_text().unwrap().text.clone())
        .expect("text content");
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("not JSON ({e}): {text}"))
}

/// Sets `^IadWalk141(1..3)` to `v1`..`v3` in USER, so each lands in the journal as a SET.
fn set_sentinels(iris: &ThrowawayIris) {
    let out = iris.session(
        "USER",
        "Set ^IadWalk141(1)=\"v1\",^IadWalk141(2)=\"v2\",^IadWalk141(3)=\"v3\"\n\
         Write \"SET:\",$Get(^IadWalk141(3)),!",
    );
    assert!(out.contains("SET:v3"), "sentinels were not set: {out}");
}

/// Sets `^IadCap141(1..6)` in USER: six SET records of this test's own, so the `max_entries` cap
/// has records to stop at whatever else the journal holds (T182). The name shares no substring with
/// `IadWalk141`, so the sentinel counts above do not move.
fn set_cap_records(iris: &ThrowawayIris) {
    let out = iris.session(
        "USER",
        "For tI=1:1:6 { Set ^IadCap141(tI)=\"c\"_tI }\nWrite \"CAP:\",$Get(^IadCap141(6)),!",
    );
    assert!(out.contains("CAP:c6"), "cap records were not set: {out}");
}

/// `name:size` of the newest journal file, read with `docker exec stat` and not through an IRIS
/// session: a session logs in, and a login journals records of its own, so polling through one
/// keeps the journal from ever going quiet.
fn newest_journal_file_size() -> String {
    let out = std::process::Command::new("docker")
        .args([
            "exec",
            THROWAWAY_IRIS_NAME,
            "sh",
            "-c",
            "stat -c '%n:%s' $(ls -t /usr/irissys/mgr/journal/2* | head -1)",
        ])
        .output()
        .expect("docker exec stat");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// Wait until a freshly started container stops journaling its own start-up work (T182). A search
/// that starts while IRIS is still compiling and mounting spends its 20 s time cap on a journal
/// that grows under it, which is the run-time swing from 9 s to 257 s. Settled means the newest
/// journal file has not grown over three reads three seconds apart.
fn wait_until_settled() {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(180);
    let mut last = newest_journal_file_size();
    let mut quiet = 0;
    while quiet < 3 {
        assert!(
            std::time::Instant::now() < deadline,
            "the throwaway container's journal never went quiet (newest file {last})"
        );
        std::thread::sleep(std::time::Duration::from_secs(3));
        let now = newest_journal_file_size();
        if now == last && !now.is_empty() {
            quiet += 1;
        } else {
            quiet = 0;
            last = now;
        }
    }
}

#[tokio::test]
#[ignore]
async fn a_journal_search_writes_no_thisrec_line_and_finds_the_sentinels() {
    let Some(iris) = ThrowawayIris::start() else {
        return;
    };
    set_sentinels(&iris);

    // Control: the Search class query does log, so the count below can move.
    let before_control = thisrec_lines();
    let control = iris.session(
        "%SYS",
        "Set tFile=##class(%SYS.Journal.System).GetCurrentFile()\n\
         Set tRS=##class(%ResultSet).%New(\"%SYS.Journal.File:Search\")\n\
         Set tSC=tRS.Execute(\"IadWalk141\",tFile.Name,tFile.FirstRecordGet().Address,0)\n\
         Set tN=0\n\
         While tRS.Next() { Set tN=tN+1 }\n\
         Write \"CTL:\",tSC,\":\",tN,!",
    );
    assert!(
        control.contains("CTL:1:"),
        "control search failed: {control}"
    );
    let after_control = thisrec_lines();
    assert!(
        after_control > before_control,
        "the control Search wrote no thisrec line ({before_control} -> {after_control}), so the \
         count proves nothing"
    );

    let conn = conn(&iris);
    let result = body(
        journal_search_impl(
            Some(&conn),
            "allow",
            Some("IadWalk141"),
            None,
            Some(50),
            &[],
            false,
        )
        .await
        .expect("Ok"),
    );
    assert_eq!(result["success"], true, "search failed: {result}");
    let after_walk = thisrec_lines();
    assert_eq!(
        after_walk,
        after_control,
        "journal_search wrote {} thisrec lines to messages.log",
        after_walk - after_control
    );

    let records = result["records"].as_array().expect("records");
    for (n, value) in [(1, "v1"), (2, "v2"), (3, "v3")] {
        let rec = records
            .iter()
            .find(|r| {
                r["operation"] == "SET"
                    && r["global_ref"]
                        .as_str()
                        .is_some_and(|g| g.ends_with(&format!("IadWalk141({n})")))
            })
            .unwrap_or_else(|| panic!("no SET record for ^IadWalk141({n}): {result}"));
        assert_eq!(rec["value"], value, "wrong value in {rec}");
        assert_eq!(rec["redacted"], false);
    }
}

/// A `journal_search` over `pattern` with `limits`, through the same entry point the tool uses.
async fn search(
    conn: &IrisConnection,
    query: JournalQuery<'_>,
    limits: JournalWalkLimits,
) -> serde_json::Value {
    body(
        journal_search_with_limits(Some(conn), "allow", query, &[], false, limits)
            .await
            .expect("Ok"),
    )
}

fn sentinel_refs(v: &serde_json::Value) -> usize {
    v["records"]
        .as_array()
        .expect("records")
        .iter()
        .filter(|r| {
            r["global_ref"]
                .as_str()
                .is_some_and(|g| g.contains("IadWalk141"))
        })
        .count()
}

/// A timestamp `days` from now, in the form the record `TimeStamp` has.
fn iris_day(iris: &ThrowawayIris, days: i32) -> String {
    let out = iris.session("%SYS", &format!("Write \"DAY:\",$ZDT($H+({days}),3),!"));
    out.lines()
        .find_map(|l| l.trim().strip_prefix("DAY:"))
        .unwrap_or_else(|| panic!("no DAY: line in {out}"))
        .to_string()
}

#[tokio::test]
#[ignore]
async fn the_walk_stops_at_its_caps_and_applies_its_filters() {
    let Some(iris) = ThrowawayIris::start() else {
        return;
    };
    wait_until_settled();
    set_sentinels(&iris);
    set_cap_records(&iris);
    let conn = conn(&iris);
    let pattern = JournalQuery {
        global_pattern: Some("IadWalk141"),
        time_range: None,
        max_records: Some(50),
    };
    let full = JournalWalkLimits::default();

    // Control: with the default caps the same search finds all three sentinels and ends cleanly,
    // so the capped answers below differ by the cap and nothing else.
    let whole = search(&conn, pattern, full).await;
    assert_eq!(whole["success"], true, "{whole}");
    assert_eq!(sentinel_refs(&whole), 3, "control found: {whole}");
    assert_eq!(whole["truncated"], false, "{whole}");
    assert!(whole["truncated_reason"].is_null(), "{whole}");
    let journal_size = whole["scanned"].as_u64().expect("scanned");
    assert!(
        journal_size > 100,
        "the journal holds only {journal_size} records, too few to test a cap of 100"
    );

    // Scanned-record cap below the journal: the scan stops at the cap, finds none of the
    // sentinels (they are at the end), and says why.
    let capped = search(
        &conn,
        pattern,
        JournalWalkLimits {
            scan_cap: 100,
            ..full
        },
    )
    .await;
    assert_eq!(capped["success"], true, "{capped}");
    assert_eq!(capped["truncated"], true, "{capped}");
    assert_eq!(capped["truncated_reason"], "scan_cap", "{capped}");
    assert_eq!(capped["scanned"], 100, "{capped}");
    assert_eq!(sentinel_refs(&capped), 0, "{capped}");

    // Time cap of zero seconds: the walk stops before the first record.
    let timed = search(
        &conn,
        pattern,
        JournalWalkLimits {
            time_cap_secs: 0,
            ..full
        },
    )
    .await;
    assert_eq!(timed["truncated"], true, "{timed}");
    assert_eq!(timed["truncated_reason"], "time_cap", "{timed}");
    assert_eq!(timed["scanned"], 0, "{timed}");

    // max_records: two of three sentinels come back and the third makes it truncated; a cap of
    // exactly three is not.
    let two = search(
        &conn,
        JournalQuery {
            max_records: Some(2),
            ..pattern
        },
        full,
    )
    .await;
    assert_eq!(two["count"], 2, "{two}");
    assert_eq!(two["truncated"], true, "{two}");
    assert_eq!(two["truncated_reason"], "max_records", "{two}");
    let three = search(
        &conn,
        JournalQuery {
            max_records: Some(3),
            ..pattern
        },
        full,
    )
    .await;
    assert_eq!(three["count"], 3, "{three}");
    assert_eq!(three["truncated"], false, "{three}");

    // Time range. Two days either side of now, so the year and the clock zone do not matter.
    let past = iris_day(&iris, -2);
    let future = iris_day(&iris, 2);
    let range = |from: Option<&str>, to: Option<&str>| {
        let mut tr = serde_json::Map::new();
        if let Some(f) = from {
            tr.insert("from".into(), f.into());
        }
        if let Some(t) = to {
            tr.insert("to".into(), t.into());
        }
        serde_json::Value::Object(tr)
    };
    let inside = range(Some(&past), Some(&future));
    let found = search(
        &conn,
        JournalQuery {
            time_range: Some(&inside),
            ..pattern
        },
        full,
    )
    .await;
    assert_eq!(sentinel_refs(&found), 3, "range around now: {found}");

    let after = range(Some(&future), None);
    let none_after = search(
        &conn,
        JournalQuery {
            time_range: Some(&after),
            ..pattern
        },
        full,
    )
    .await;
    assert_eq!(none_after["success"], true, "{none_after}");
    assert_eq!(none_after["count"], 0, "from in the future: {none_after}");
    // The walk goes newest first, so a `from` after the newest record ends it there, and that is
    // not a truncation: the journal is in time order, so nothing older can match.
    assert_eq!(none_after["scanned"], 1, "{none_after}");
    assert_eq!(none_after["truncated"], false, "{none_after}");

    // A `to` before every record keeps nothing and reads the whole file, back to its start.
    let before = range(None, Some(&past));
    let none_before = search(
        &conn,
        JournalQuery {
            time_range: Some(&before),
            ..pattern
        },
        full,
    )
    .await;
    assert_eq!(none_before["count"], 0, "to in the past: {none_before}");
    assert!(
        none_before["scanned"].as_u64().expect("scanned") >= journal_size,
        "{none_before}"
    );
    assert_eq!(none_before["truncated"], false, "{none_before}");

    // A pattern that matches nothing finishes and reports success: the old scan spun forever on
    // the first record that failed a filter.
    let miss = search(
        &conn,
        JournalQuery {
            global_pattern: Some("IadWalk141NoSuchGlobal"),
            ..pattern
        },
        full,
    )
    .await;
    assert_eq!(miss["success"], true, "{miss}");
    assert_eq!(miss["count"], 0, "{miss}");
    // Each search journals its own scratch class, so the journal is longer than at the control.
    assert!(
        miss["scanned"].as_u64().expect("scanned") >= journal_size,
        "a miss must read to the end of the journal: {miss}"
    );
    assert_eq!(miss["truncated"], false, "{miss}");

    // The standalone tool answers from the same walk.
    let entries = body(
        journal_entries_impl(&conn, None, None, Some("IadWalk141"), 100)
            .await
            .expect("Ok"),
    );
    assert_eq!(entries["success"], true, "{entries}");
    let rows = entries["entries"].as_array().expect("entries");
    assert_eq!(rows.len(), 3, "{entries}");
    assert_eq!(entries["returned"], 3, "{entries}");
    for row in rows {
        assert_eq!(row["type"], "SET", "{row}");
        assert!(
            row["global"].as_str().unwrap().contains("IadWalk141"),
            "{row}"
        );
        assert!(row["job_id"].as_i64().unwrap() > 0, "{row}");
    }
    let entries_after = body(
        journal_entries_impl(&conn, Some(&future), None, Some("IadWalk141"), 100)
            .await
            .expect("Ok"),
    );
    assert_eq!(
        entries_after["returned"], 0,
        "start in the future: {entries_after}"
    );
    let entries_capped = body(
        journal_entries_impl(&conn, None, None, Some("IadCap141"), 4)
            .await
            .expect("Ok"),
    );
    assert_eq!(entries_capped["returned"], 4, "{entries_capped}");
    assert_eq!(
        entries_capped["truncated_reason"], "max_records",
        "{entries_capped}"
    );
}
