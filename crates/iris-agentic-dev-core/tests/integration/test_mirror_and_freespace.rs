//! Live IRIS integration tests for iris_mirror_status, iris_database_list
//! free space, and iris_system_performance (089). All tests require
//! iris-dev-iris and are #[ignore] by default.
//!
//! Note: iris_system_performance tests require a live IRIS instance with the
//! SystemPerformance routine available (run^SystemPerformance).
//!
//! Run with:
//!   IRIS_HOST=localhost IRIS_WEB_PORT=52780 IRIS_USERNAME=_SYSTEM IRIS_PASSWORD=SYS \
//!   cargo test --test test_mirror_and_freespace -- --include-ignored --test-threads=1 --nocapture

use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};

fn make_conn() -> Option<(IrisConnection, reqwest::Client)> {
    let iris_host = std::env::var("IRIS_HOST").unwrap_or_default();
    if iris_host.is_empty() {
        return None;
    }
    let web_port = std::env::var("IRIS_WEB_PORT").unwrap_or_else(|_| "52780".to_string());
    let username = std::env::var("IRIS_USERNAME").unwrap_or_else(|_| "_SYSTEM".to_string());
    let password = std::env::var("IRIS_PASSWORD").unwrap_or_else(|_| "SYS".to_string());
    let base_url = format!("http://{}:{}", iris_host, web_port);
    let conn = IrisConnection::new(
        base_url,
        "USER",
        username,
        password,
        DiscoverySource::EnvVar,
    );
    let client = reqwest::Client::new();
    Some((conn, client))
}

fn parse_json(r: rmcp::model::CallToolResult) -> serde_json::Value {
    let text = r
        .content
        .first()
        .map(|c| c.as_text().unwrap().text.clone())
        .expect("no text content");
    serde_json::from_str(&text).expect("json parse failed")
}

// T011 / T006: iris_mirror_status on community iris-dev-iris (not in a mirror)
#[tokio::test]
#[ignore]
async fn e2e_mirror_status_non_member() {
    use iris_agentic_dev_core::tools::admin_tools::iris_mirror_status_impl;

    let (conn, client) = match make_conn() {
        Some(c) => c,
        None => {
            eprintln!("IRIS_HOST not set — skipping e2e_mirror_status_non_member");
            return;
        }
    };

    let result = iris_mirror_status_impl(&conn, &client)
        .await
        .expect("iris_mirror_status_impl failed");
    let v = parse_json(result);

    eprintln!("mirror_status response: {v}");

    assert_eq!(
        v["success"].as_bool(),
        Some(true),
        "expected success=true, got: {v}"
    );
    assert_eq!(
        v["is_member"].as_bool(),
        Some(false),
        "community iris-dev-iris is not in a mirror; expected is_member=false, got: {v}"
    );
    assert!(
        v["mirror_name"].is_null(),
        "expected mirror_name=null for non-member, got: {v}"
    );
    assert_eq!(
        v["is_primary"].as_bool(),
        Some(false),
        "expected is_primary=false for non-member, got: {v}"
    );
}

// T018 / T014: iris_database_list includes free space fields on iris-dev-iris
#[tokio::test]
#[ignore]
async fn e2e_database_list_free_space() {
    use iris_agentic_dev_core::tools::admin_tools::iris_database_list_impl;

    let (conn, client) = match make_conn() {
        Some(c) => c,
        None => {
            eprintln!("IRIS_HOST not set — skipping e2e_database_list_free_space");
            return;
        }
    };

    let result = iris_database_list_impl(&conn, &client)
        .await
        .expect("iris_database_list_impl failed");
    let v = parse_json(result);

    eprintln!("database_list response: {v}");

    assert_eq!(
        v["success"].as_bool(),
        Some(true),
        "expected success=true, got: {v}"
    );

    // No free_space_note at root — free space query should succeed on iris-dev-iris
    assert!(
        v["free_space_note"].is_null() || !v.as_object().unwrap().contains_key("free_space_note"),
        "unexpected free_space_note: {}",
        v["free_space_note"]
    );

    let databases = v["databases"]
        .as_array()
        .expect("databases should be array");
    assert!(
        !databases.is_empty(),
        "expected at least one database, got empty array"
    );

    // At least one entry should have size_mb as a positive number
    let has_size = databases.iter().any(|db| {
        db["size_mb"].as_i64().is_some_and(|n| n > 0)
            || db["size_mb"].as_f64().is_some_and(|n| n > 0.0)
    });
    assert!(
        has_size,
        "expected at least one database with size_mb > 0, got: {databases:?}"
    );

    // At least one entry should have free_space_mb as a non-negative number
    let has_free = databases
        .iter()
        .any(|db| db["free_space_mb"].as_f64().is_some() || db["free_space_mb"].as_i64().is_some());
    assert!(
        has_free,
        "expected at least one database with free_space_mb field, got: {databases:?}"
    );

    // max_size_mb should be null or a positive integer — never an error
    for db in databases {
        let max = &db["max_size_mb"];
        assert!(
            max.is_null() || max.as_i64().is_some_and(|n| n > 0),
            "max_size_mb should be null or positive, got: {max} in {db}"
        );
    }
}

// iris_system_performance: last_runid on community iris-dev-iris
// Expected: success=true, run_id=null (no runs have been started)
// On Enterprise with prior runs, run_id will be a non-empty string.
#[tokio::test]
#[ignore]
async fn e2e_system_performance_last_runid_community() {
    use iris_agentic_dev_core::tools::admin_tools::{iris_system_performance_impl, SysPerfRequest};

    let (conn, client) = match make_conn() {
        Some(c) => c,
        None => {
            eprintln!("IRIS_HOST not set — skipping e2e_system_performance_last_runid_community");
            return;
        }
    };

    let result = iris_system_performance_impl(&conn, &client, &SysPerfRequest::new("last_runid"))
        .await
        .expect("iris_system_performance_impl failed");
    let v = parse_json(result);

    eprintln!("system_performance last_runid response: {v}");

    // success must be present — value depends on whether SystemPerformance global exists
    assert!(
        v.get("success").is_some(),
        "expected success field, got: {v}"
    );
    // mode must be last_runid on success path
    if v["success"].as_bool() == Some(true) {
        assert_eq!(
            v["mode"].as_str(),
            Some("last_runid"),
            "expected mode=last_runid, got: {v}"
        );
        assert!(
            v["in_progress"].is_boolean(),
            "last_runid must report whether the run is still collecting, got: {v}"
        );
    }
}

/// `mode=start` shipped calling `Do run^SystemPerformance` with no profile, which throws
/// `<UNDEFINED> pname`. This is the test that was missing: start a real run against live IRIS,
/// assert a run ID comes back, then confirm `last_runid` and `status` can see it.
///
/// Uses the `test` profile (5 minutes) and leaves it collecting — SystemPerformance has no
/// cancel entry point, and the run is harmless on a dev container.
#[tokio::test]
#[ignore]
async fn e2e_system_performance_start_returns_run_id() {
    use iris_agentic_dev_core::tools::admin_tools::{iris_system_performance_impl, SysPerfRequest};

    let (conn, client) = match make_conn() {
        Some(c) => c,
        None => {
            eprintln!("IRIS_HOST not set — skipping e2e_system_performance_start_returns_run_id");
            return;
        }
    };

    let started = parse_json(
        iris_system_performance_impl(
            &conn,
            &client,
            &SysPerfRequest {
                profile: Some("test"),
                ..SysPerfRequest::new("start")
            },
        )
        .await
        .expect("start call failed"),
    );
    eprintln!("system_performance start response: {started}");

    assert_eq!(
        started["success"].as_bool(),
        Some(true),
        "mode=start must succeed against live IRIS, got: {started}"
    );
    assert_eq!(started["profile"].as_str(), Some("test"));
    let run_id = started["run_id"]
        .as_str()
        .unwrap_or_default()
        .trim()
        .to_string();
    assert!(
        !run_id.is_empty(),
        "start must return the run ID that run^SystemPerformance produced, got: {started}"
    );

    // The freshly started run is in flight, so it has no ("history") node yet. last_runid
    // must still see it and flag it as in progress.
    let last = parse_json(
        iris_system_performance_impl(&conn, &client, &SysPerfRequest::new("last_runid"))
            .await
            .expect("last_runid call failed"),
    );
    eprintln!("system_performance last_runid after start: {last}");
    assert_eq!(
        last["run_id"].as_str(),
        Some(run_id.as_str()),
        "last_runid must report the in-flight run, not the newest completed one, got: {last}"
    );
    assert_eq!(
        last["in_progress"].as_bool(),
        Some(true),
        "a run that just started is still collecting, got: {last}"
    );

    // status resolves the run ID rather than reporting "no such runid".
    let status = parse_json(
        iris_system_performance_impl(
            &conn,
            &client,
            &SysPerfRequest {
                run_id: Some(&run_id),
                ..SysPerfRequest::new("status")
            },
        )
        .await
        .expect("status call failed"),
    );
    eprintln!("system_performance status for {run_id}: {status}");
    assert_eq!(status["success"].as_bool(), Some(true));
    let wait = status["wait_time"].as_str().unwrap_or_default();
    assert!(
        !wait.contains("no such runid"),
        "status must recognise the run ID returned by start, got: {status}"
    );
}

/// The profile is interpolated into an ObjectScript string literal, so a hostile value must be
/// rejected in Rust and never reach IRIS.
#[tokio::test]
#[ignore]
async fn e2e_system_performance_start_rejects_bad_profile() {
    use iris_agentic_dev_core::tools::admin_tools::{iris_system_performance_impl, SysPerfRequest};

    let (conn, client) = match make_conn() {
        Some(c) => c,
        None => {
            eprintln!("IRIS_HOST not set — skipping");
            return;
        }
    };

    let v = parse_json(
        iris_system_performance_impl(
            &conn,
            &client,
            &SysPerfRequest {
                profile: Some(r#"test") Do ^%ZSTOP //"#),
                ..SysPerfRequest::new("start")
            },
        )
        .await
        .expect("start call failed"),
    );
    eprintln!("system_performance start with hostile profile: {v}");
    assert_eq!(v["success"].as_bool(), Some(false));
    assert!(
        v["error"].as_str().unwrap_or_default().contains("profile"),
        "error must name the offending parameter, got: {v}"
    );
}

// iris_system_performance: mode=status without run_id returns error
#[tokio::test]
#[ignore]
async fn e2e_system_performance_status_missing_run_id() {
    use iris_agentic_dev_core::tools::admin_tools::{iris_system_performance_impl, SysPerfRequest};

    let (conn, client) = match make_conn() {
        Some(c) => c,
        None => {
            eprintln!("IRIS_HOST not set — skipping e2e_system_performance_status_missing_run_id");
            return;
        }
    };

    let result = iris_system_performance_impl(&conn, &client, &SysPerfRequest::new("status"))
        .await
        .expect("iris_system_performance_impl failed");
    let v = parse_json(result);

    eprintln!("system_performance status (no run_id) response: {v}");

    assert_eq!(
        v["success"].as_bool(),
        Some(false),
        "expected success=false when run_id missing, got: {v}"
    );
    let err = v["error"].as_str().unwrap_or("");
    assert!(
        err.contains("run_id"),
        "expected error mentioning run_id, got: {v}"
    );
}

// ── 097 T020: mirror_add_async on community iris-dev-iris (not a mirror member) ─
//
// Community iris-dev-iris is never a mirror member. The pre-flight `IsMember()` call
// returns 0, so `ALREADY_MEMBER` is NOT returned. The call proceeds to
// `JoinMirrorAsAsyncMember`, which fails with an IRIS-level error.
// Assert: success=false, error field non-empty (not a crash, not ALREADY_MEMBER).
// Skip when IRIS_MIRROR_PRIMARY is set (full round-trip test environment replaces this).
#[tokio::test]
#[ignore]
async fn e2e_mirror_add_async_community_non_member() {
    use iris_agentic_dev_core::tools::admin_tools::iris_mirror_add_async_impl;

    if std::env::var("IRIS_MIRROR_PRIMARY").is_ok() {
        eprintln!("IRIS_MIRROR_PRIMARY set — skipping community non-member test");
        return;
    }

    let (conn, client) = match make_conn() {
        Some(c) => c,
        None => {
            eprintln!("IRIS_HOST not set — skipping e2e_mirror_add_async_community_non_member");
            return;
        }
    };

    let result = iris_mirror_add_async_impl(
        Some(&conn),
        &client,
        "TestMirror097",
        "127.0.0.1",
        2188,
        "IRIS",
        0,
    )
    .await
    .expect("iris_mirror_add_async_impl failed");
    let v = parse_json(result);

    eprintln!("mirror_add_async (community) response: {v}");

    assert_eq!(
        v["success"].as_bool(),
        Some(false),
        "community iris-dev-iris cannot join a mirror; expected success=false, got: {v}"
    );
    assert_ne!(
        v["error_code"].as_str(),
        Some("ALREADY_MEMBER"),
        "community iris-dev-iris is not a mirror member — ALREADY_MEMBER is wrong, got: {v}"
    );
    let error = v["error"].as_str().unwrap_or("");
    assert!(
        !error.is_empty(),
        "expected non-empty error field, got: {v}"
    );
}

// ── 097 T027b: mirror_failover on community iris-dev-iris → NOT_MIRROR_MEMBER ─
//
// Community iris-dev-iris is never a mirror member. mirror_failover with
// IRIS_DESTRUCTIVE_TOOLS_ENABLED set should return NOT_MIRROR_MEMBER (not a crash).
// Skip when IRIS_MIRROR_PRIMARY is not set (requires destructive env to be meaningful).
#[tokio::test]
#[ignore]
async fn e2e_mirror_failover_community_non_member() {
    use iris_agentic_dev_core::tools::admin_tools::iris_mirror_failover_impl;

    let (conn, client) = match make_conn() {
        Some(c) => c,
        None => {
            eprintln!("IRIS_HOST not set — skipping e2e_mirror_failover_community_non_member");
            return;
        }
    };

    let result = iris_mirror_failover_impl(Some(&conn), &client)
        .await
        .expect("iris_mirror_failover_impl failed");
    let v = parse_json(result);

    eprintln!("mirror_failover (community) response: {v}");

    assert_eq!(
        v["success"].as_bool(),
        Some(false),
        "community iris-dev-iris is not a mirror member; expected success=false, got: {v}"
    );
    assert_eq!(
        v["error_code"].as_str(),
        Some("NOT_MIRROR_MEMBER"),
        "expected NOT_MIRROR_MEMBER for non-member failover, got: {v}"
    );
}

// ── 096: profile management and report retrieval, live ────────────────────────
//
// These are the modes 089 left out. Everything they assert was measured on iris-dev-iris
// first — the profile node layout, the `0^profile name exists already` refusal, and the
// `<node name>_<instance>_<run id>.html` report path.

async fn sysperf(
    conn: &IrisConnection,
    client: &reqwest::Client,
    req: iris_agentic_dev_core::tools::admin_tools::SysPerfRequest<'_>,
) -> serde_json::Value {
    parse_json(
        iris_agentic_dev_core::tools::admin_tools::iris_system_performance_impl(conn, client, &req)
            .await
            .expect("iris_system_performance_impl failed"),
    )
}

// Every field `list_profiles` reports, read back off a profile this test defined.
//
// It used to look for the shipped `test` profile and assert its 30-second/10-sample numbers.
// `^IRIS.SystemPerformance("profile",...)` is empty on a fresh instance — the default profiles are
// created lazily, not at install — so on CI there was no `test` to find and the read looked broken
// when it was not. Defining the profile here asserts the same parse against numbers this test
// chose, which is a stronger check anyway: the expected values are not read from the thing under
// test.
#[tokio::test]
#[ignore]
async fn e2e_system_performance_list_profiles() {
    use iris_agentic_dev_core::tools::admin_tools::SysPerfRequest;

    let (conn, client) = match make_conn() {
        Some(c) => c,
        None => {
            eprintln!("IRIS_HOST not set — skipping e2e_system_performance_list_profiles");
            return;
        }
    };

    // Unique per run, so a crashed earlier run cannot make this one fail on the duplicate refusal.
    let name = format!(
        "iad_lp_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    );
    let description = "iris-agentic-dev list_profiles fixture, 30 s samples";
    let added = sysperf(
        &conn,
        &client,
        SysPerfRequest {
            profile: Some(&name),
            description: Some(description),
            interval_seconds: Some(30),
            sample_count: Some(10),
            ..SysPerfRequest::new("add_profile")
        },
    )
    .await;
    assert_eq!(added["success"].as_bool(), Some(true), "got: {added}");

    let v = sysperf(&conn, &client, SysPerfRequest::new("list_profiles")).await;
    eprintln!("system_performance list_profiles response: {v}");

    assert_eq!(v["success"].as_bool(), Some(true), "got: {v}");
    let profiles = v["profiles"].as_array().expect("profiles must be an array");
    assert_eq!(
        v["count"].as_u64(),
        Some(profiles.len() as u64),
        "the DONE count and the number of parsed lines disagree, so a line was dropped: {v}"
    );
    let mine = profiles
        .iter()
        .find(|p| p["name"] == name.as_str())
        .unwrap_or_else(|| panic!("{name} was added but is not in the listing: {v}"));
    assert_eq!(mine["interval_seconds"].as_i64(), Some(30), "got: {mine}");
    assert_eq!(mine["sample_count"].as_i64(), Some(10), "got: {mine}");
    // 30 s × 10 is the 5 minutes the tool derives rather than reads.
    assert_eq!(mine["duration_minutes"].as_f64(), Some(5.0), "got: {mine}");
    assert_eq!(
        mine["description"].as_str(),
        Some(description),
        "got: {mine}"
    );

    let del = sysperf(
        &conn,
        &client,
        SysPerfRequest {
            profile: Some(&name),
            ..SysPerfRequest::new("delete_profile")
        },
    )
    .await;
    assert_eq!(del["success"].as_bool(), Some(true), "got: {del}");
}

// Full add → list → duplicate-refusal → delete round trip. The profile name is unique per run
// so a crashed earlier run cannot make this one fail.
#[tokio::test]
#[ignore]
async fn e2e_system_performance_profile_round_trip() {
    use iris_agentic_dev_core::tools::admin_tools::SysPerfRequest;

    let (conn, client) = match make_conn() {
        Some(c) => c,
        None => {
            eprintln!("IRIS_HOST not set — skipping e2e_system_performance_profile_round_trip");
            return;
        }
    };

    let name = format!(
        "iad_rt_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    );
    let add = SysPerfRequest {
        profile: Some(&name),
        description: Some("iris-agentic-dev round trip, 1 s samples"),
        interval_seconds: Some(1),
        sample_count: Some(60),
        ..SysPerfRequest::new("add_profile")
    };

    let v = sysperf(&conn, &client, add.clone()).await;
    eprintln!("system_performance add_profile response: {v}");
    assert_eq!(v["success"].as_bool(), Some(true), "got: {v}");
    assert_eq!(v["duration_minutes"].as_f64(), Some(1.0), "got: {v}");

    let listed = sysperf(&conn, &client, SysPerfRequest::new("list_profiles")).await;
    let mine = listed["profiles"]
        .as_array()
        .expect("profiles must be an array")
        .iter()
        .find(|p| p["name"] == name.as_str())
        .unwrap_or_else(|| panic!("{name} was added but is not in the listing: {listed}"));
    assert_eq!(mine["interval_seconds"].as_i64(), Some(1), "got: {mine}");
    assert_eq!(mine["sample_count"].as_i64(), Some(60), "got: {mine}");
    assert_eq!(
        mine["description"].as_str(),
        Some("iris-agentic-dev round trip, 1 s samples"),
        "got: {mine}"
    );

    // A second add of the same name is refused with IRIS's own words, not a generic failure.
    let dup = sysperf(&conn, &client, add).await;
    eprintln!("system_performance add_profile (duplicate) response: {dup}");
    assert_eq!(dup["success"].as_bool(), Some(false), "got: {dup}");
    assert!(
        dup["error"].as_str().unwrap_or("").contains("exists"),
        "expected IRIS's 'profile name exists already', got: {dup}"
    );

    let del = sysperf(
        &conn,
        &client,
        SysPerfRequest {
            profile: Some(&name),
            ..SysPerfRequest::new("delete_profile")
        },
    )
    .await;
    eprintln!("system_performance delete_profile response: {del}");
    assert_eq!(del["success"].as_bool(), Some(true), "got: {del}");

    let after = sysperf(&conn, &client, SysPerfRequest::new("list_profiles")).await;
    assert!(
        !after["profiles"]
            .as_array()
            .expect("profiles must be an array")
            .iter()
            .any(|p| p["name"] == name.as_str()),
        "{name} survived delete_profile: {after}"
    );
}

// `addprofile("bad name",...)` returns 1 and stores `badname`. The name is rejected before the
// call, so IRIS never gets the chance to leave a profile the caller cannot find.
#[tokio::test]
#[ignore]
async fn e2e_system_performance_add_profile_rejects_a_name_iris_would_rewrite() {
    use iris_agentic_dev_core::tools::admin_tools::SysPerfRequest;

    let (conn, client) = match make_conn() {
        Some(c) => c,
        None => {
            eprintln!("IRIS_HOST not set — skipping add_profile hostile-name test");
            return;
        }
    };

    let v = sysperf(
        &conn,
        &client,
        SysPerfRequest {
            profile: Some("iad bad name"),
            description: Some("must not be created"),
            interval_seconds: Some(1),
            sample_count: Some(60),
            ..SysPerfRequest::new("add_profile")
        },
    )
    .await;
    eprintln!("system_performance add_profile (bad name) response: {v}");
    assert_eq!(v["success"].as_bool(), Some(false), "got: {v}");

    let listed = sysperf(&conn, &client, SysPerfRequest::new("list_profiles")).await;
    assert!(
        !listed["profiles"]
            .as_array()
            .expect("profiles must be an array")
            .iter()
            .any(|p| p["name"] == "iadbadname"),
        "the rejected name reached IRIS and was stored stripped: {listed}"
    );
}

// list_runs on an instance that has never collected is legitimately empty; the DONE marker is
// what makes that distinguishable from a read that stopped early.
#[tokio::test]
#[ignore]
async fn e2e_system_performance_list_runs() {
    use iris_agentic_dev_core::tools::admin_tools::SysPerfRequest;

    let (conn, client) = match make_conn() {
        Some(c) => c,
        None => {
            eprintln!("IRIS_HOST not set — skipping e2e_system_performance_list_runs");
            return;
        }
    };

    let v = sysperf(&conn, &client, SysPerfRequest::new("list_runs")).await;
    eprintln!("system_performance list_runs response: {v}");

    assert_eq!(v["success"].as_bool(), Some(true), "got: {v}");
    let runs = v["runs"].as_array().expect("runs must be an array");
    assert_eq!(
        v["count"].as_u64(),
        Some(runs.len() as u64),
        "the DONE count and the number of parsed lines disagree: {v}"
    );
    for run in runs {
        let rid = run["run_id"].as_str().unwrap_or("");
        assert!(!rid.is_empty(), "run with no run_id: {run}");
        assert!(
            run["completed_at"].as_str().is_some(),
            "a history node always carries a completion time: {run}"
        );
        assert!(
            run["profile"].as_str().is_some(),
            "run_id {rid} did not yield a profile: {run}"
        );
    }
    // Descending iteration: newest first.
    let ids: Vec<&str> = runs.iter().filter_map(|r| r["run_id"].as_str()).collect();
    let mut sorted = ids.clone();
    sorted.sort_by(|a, b| b.cmp(a));
    assert_eq!(ids, sorted, "runs are not newest-first: {v}");
}

// mode=report with no run_id resolves the newest completed run. On an instance with no history
// that is a clean "nothing to report on", not a crash.
#[tokio::test]
#[ignore]
async fn e2e_system_performance_report_newest() {
    use iris_agentic_dev_core::tools::admin_tools::SysPerfRequest;

    let (conn, client) = match make_conn() {
        Some(c) => c,
        None => {
            eprintln!("IRIS_HOST not set — skipping e2e_system_performance_report_newest");
            return;
        }
    };

    let runs = sysperf(&conn, &client, SysPerfRequest::new("list_runs")).await;
    let newest = runs["runs"]
        .as_array()
        .and_then(|r| r.first())
        .and_then(|r| r["run_id"].as_str())
        .map(str::to_string);

    let v = sysperf(&conn, &client, SysPerfRequest::new("report")).await;
    eprintln!("system_performance report (newest) response: {v}");

    let Some(newest) = newest else {
        assert_eq!(
            v["success"].as_bool(),
            Some(false),
            "no history, so report has nothing to resolve: {v}"
        );
        return;
    };

    assert_eq!(v["success"].as_bool(), Some(true), "got: {v}");
    assert_eq!(v["run_id"].as_str(), Some(newest.as_str()), "got: {v}");
    assert!(
        v["output_dir"].as_str().is_some_and(|d| !d.is_empty()),
        "got: {v}"
    );
    if v["exists"] == serde_json::json!(true) {
        let path = v["report_path"].as_str().unwrap_or("");
        assert!(
            path.ends_with(&format!("_{newest}.html")),
            "the report file name ends in the run ID: {v}"
        );
        assert!(
            v["size_bytes"].as_i64().is_some_and(|n| n > 0),
            "an existing report is not zero bytes: {v}"
        );
    } else {
        // The HTML is written at completion; a run whose file was archived or deleted reports
        // exists=false with the reason rather than a path that is not there.
        assert!(
            v["note"].as_str().is_some_and(|n| !n.is_empty()),
            "exists=false must say why: {v}"
        );
    }
}

// An unknown run ID is a named miss, not an empty success.
#[tokio::test]
#[ignore]
async fn e2e_system_performance_report_unknown_run() {
    use iris_agentic_dev_core::tools::admin_tools::SysPerfRequest;

    let (conn, client) = match make_conn() {
        Some(c) => c,
        None => {
            eprintln!("IRIS_HOST not set — skipping e2e_system_performance_report_unknown_run");
            return;
        }
    };

    let v = sysperf(
        &conn,
        &client,
        SysPerfRequest {
            run_id: Some("19700101_000000_nosuch"),
            ..SysPerfRequest::new("report")
        },
    )
    .await;
    eprintln!("system_performance report (unknown run) response: {v}");

    assert_eq!(v["exists"], serde_json::json!(false), "got: {v}");
}
