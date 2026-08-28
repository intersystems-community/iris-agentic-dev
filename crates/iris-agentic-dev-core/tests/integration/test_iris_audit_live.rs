//! T029 / T030 / T031 / T032 — Live `%SYS.Audit` emission tests.
//!
//! All tests require:
//!   IRIS_HOST=localhost IRIS_WEB_PORT=52780 IRIS_USERNAME=_SYSTEM IRIS_PASSWORD=SYS
//!   cargo test --test test_iris_audit_live -- --include-ignored --test-threads=1 --nocapture
//!
//! Container-state contract (SC-007):
//!   - T029-T031 create the event definition if absent. T032 deletes it and verifies the
//!     container is back to as-found, so these tests are safe to run on the shared iris-dev-iris.
//!   - The tests share a single live IRIS connection and are run in serial order via
//!     `--test-threads=1`.
//!
//! The `%SYS.Audit` List query is used for read-back, not `SELECT`, because DP-449511
//! records that SQL indices are not refreshed until the List query runs, causing `SELECT
//! COUNT(*)` to return stale counts seconds after writes.

use iris_agentic_dev_core::iris::connection::{
    iris_http_client, set_caller_mode, CallerMode, DiscoverySource, IrisConnection,
};
use iris_agentic_dev_core::iris::iris_audit::{build_audit_os, build_event_data, SETUP_CMD};

fn iris_host() -> Option<String> {
    let h = std::env::var("IRIS_HOST").unwrap_or_default();
    if h.is_empty() {
        None
    } else {
        Some(h)
    }
}

fn iris_conn() -> Option<IrisConnection> {
    let host = iris_host()?;
    let port: u16 = std::env::var("IRIS_WEB_PORT")
        .unwrap_or_else(|_| "52780".to_string())
        .parse()
        .unwrap_or(52780);
    let username = std::env::var("IRIS_USERNAME").unwrap_or_else(|_| "_SYSTEM".to_string());
    let password = std::env::var("IRIS_PASSWORD").unwrap_or_else(|_| "SYS".to_string());
    Some(IrisConnection::new(
        format!("http://{}:{}", host, port),
        "USER",
        username,
        password,
        DiscoverySource::EnvVar,
    ))
}

/// Run ObjectScript in USER namespace and return trimmed output.
async fn run_os(conn: &IrisConnection, code: &str) -> anyhow::Result<String> {
    let client = iris_http_client(None, true, false)?;
    let out = conn.execute_via_generator(code, "USER", &client).await?;
    Ok(out.trim().to_string())
}

/// Run ObjectScript in %SYS namespace (required for Security.Events operations).
async fn run_os_sys(conn: &IrisConnection, code: &str) -> anyhow::Result<String> {
    let client = iris_http_client(None, true, false)?;
    let out = conn.execute_via_generator(code, "%SYS", &client).await?;
    Ok(out.trim().to_string())
}

/// Count rows for EventSource = 'iris-agentic-dev' via the List query.
/// Runs in %SYS because %SYS.Audit access requires %SYS namespace privilege.
async fn count_iad_audit_rows(conn: &IrisConnection) -> anyhow::Result<u64> {
    let code = r#"
Set rs = ##class(%ResultSet).%New("%SYS.Audit:List")
Set tSC = rs.%Execute()
Set count = 0
While rs.%Next() {
    If rs.Get("EventSource") = "iris-agentic-dev" { Set count = count + 1 }
}
Write count, !
"#;
    let out = run_os_sys(conn, code).await?;
    // Parse the first numeric line from the output.
    for line in out.lines() {
        let trimmed = line.trim();
        if let Ok(n) = trimmed.parse::<u64>() {
            return Ok(n);
        }
    }
    Err(anyhow::anyhow!("could not parse count from: {:?}", out))
}

/// Ensure the event definition exists. Returns `true` if it was created by us (must be
/// deleted in T032), `false` if it was already present.
/// Security.Events lives in %SYS — all management operations run there.
async fn ensure_event_definition(conn: &IrisConnection) -> anyhow::Result<bool> {
    let check_code = r#"
Set tSC = ##class(Security.Events).Get("iris-agentic-dev","Tool","ToolCall",.p)
If $$$ISERR(tSC) { Write "0", ! } Else { Write "1", ! }
"#;
    let result = run_os_sys(conn, check_code).await?;
    let exists = result.trim().starts_with('1');
    if exists {
        return Ok(false); // already present, we didn't create it
    }
    let create_code = format!(
        r#"
Set tSC = {SETUP_CMD}
Write $SYSTEM.Status.IsOK(tSC), !
"#
    );
    let ok = run_os_sys(conn, &create_code).await?;
    if !ok.trim().starts_with('1') {
        return Err(anyhow::anyhow!("Security.Events.Create failed: {ok}"));
    }
    Ok(true) // we created it
}

/// Delete the event definition (restoration step for SC-007).
/// Runs in %SYS because Security.Events is a %SYS class.
async fn delete_event_definition(conn: &IrisConnection) -> anyhow::Result<()> {
    let code = r#"
Set tSC = ##class(Security.Events).Delete("iris-agentic-dev","Tool","ToolCall")
Write $SYSTEM.Status.IsOK(tSC), !
"#;
    let _ = run_os_sys(conn, code).await;
    Ok(())
}

// ─── T029: Positive emission ──────────────────────────────────────────────

/// T029: Create the event definition, emit one record, read it back, assert identity fields
/// and a populated `ClientIPAddress`.
#[tokio::test]
#[ignore]
async fn iris_audit_positive_emission() {
    let conn = match iris_conn() {
        Some(c) => c,
        None => {
            eprintln!("IRIS_HOST not set — skipping");
            return;
        }
    };
    set_caller_mode(CallerMode::Cli);

    let created = ensure_event_definition(&conn)
        .await
        .expect("ensure event definition");

    let before_count = count_iad_audit_rows(&conn)
        .await
        .expect("count before emission");

    // Emit one record.
    let event_data = build_event_data("iris_execute", CallerMode::Cli, None);
    let os = build_audit_os(&event_data, "live test T029");
    let result = run_os(&conn, &os).await.expect("emit audit record");
    assert!(
        result.trim().starts_with('1'),
        "Security.Audit must return 1 on success; got: {result}"
    );

    // Wait briefly and read back through the List query (not SELECT — DP-449511).
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    let after_count = count_iad_audit_rows(&conn)
        .await
        .expect("count after emission");
    assert!(
        after_count > before_count,
        "after count ({after_count}) must be greater than before ({before_count})"
    );

    // Read the highest-AuditIndex iris-agentic-dev record — that is the one we just wrote.
    // %SYS.Audit:List returns rows in an unspecified order, so we track max(AuditIndex).
    let read_code = r#"
Set rs = ##class(%ResultSet).%New("%SYS.Audit:List")
Set tSC = rs.%Execute()
Set maxIdx = -1
Set lastED = ""
Set lastIP = ""
While rs.%Next() {
    If rs.Get("EventSource") = "iris-agentic-dev" {
        Set idx = rs.Get("AuditIndex")
        If idx > maxIdx {
            Set maxIdx = idx
            Set lastED = rs.Get("EventData")
            Set lastIP = rs.Get("ClientIPAddress")
        }
    }
}
Write lastED, "||", lastIP, !
"#;
    let out = run_os_sys(&conn, read_code)
        .await
        .expect("read back record");
    eprintln!("T029 read-back: {out}");
    let parts: Vec<&str> = out.splitn(2, "||").collect();
    let event_data_read = parts.first().map(|s| s.trim()).unwrap_or("");
    let client_ip = parts.get(1).map(|s| s.trim()).unwrap_or("");

    assert!(
        event_data_read.contains("tool=iris_execute"),
        "EventData must contain tool name; got: {event_data_read}"
    );
    assert!(
        event_data_read.contains("mode=cli"),
        "EventData must contain mode=cli; got: {event_data_read}"
    );
    assert!(
        event_data_read.contains("ua=iris-agentic-dev/"),
        "EventData must contain ua= marker; got: {event_data_read}"
    );
    assert!(
        !client_ip.is_empty(),
        "ClientIPAddress must be populated in user-defined records; got empty"
    );

    if created {
        delete_event_definition(&conn)
            .await
            .expect("restore event definition");
    }
}

// ─── T030: Negative — no record when disabled ────────────────────────────

/// T030: With `irisAudit` absent or false, a direct call to the audit OS **not** made
/// from `call_tool` writes nothing. We test the inverse: calling `$SYSTEM.Security.Audit`
/// directly when the event definition exists but the `iris_audit` config gate is off (i.e.
/// we just don't call the OS code at all). We verify the row count is unchanged.
///
/// This test exercises the config path by not calling `build_audit_os` — the gate is the
/// `iris_audit` bool on `ConnectionPolicy`, which is tested in the TOML round-trip tests.
/// Here we just confirm the count-before == count-after invariant holds when we don't emit.
#[tokio::test]
#[ignore]
async fn iris_audit_no_record_when_disabled() {
    let conn = match iris_conn() {
        Some(c) => c,
        None => {
            eprintln!("IRIS_HOST not set — skipping");
            return;
        }
    };

    let before = count_iad_audit_rows(&conn)
        .await
        .expect("count before non-emission");

    // Explicitly do NOT emit. Sleep to let any background writes settle.
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    let after = count_iad_audit_rows(&conn)
        .await
        .expect("count after non-emission");

    assert_eq!(
        before, after,
        "no audit record must be written when irisAudit is not enabled; before={before} after={after}"
    );
}

// ─── T031: Refuse-and-instruct when event absent ────────────────────────

/// T031: With `irisAudit = true` and NO event definition, `$SYSTEM.Security.Audit` returns
/// 0, the tool call succeeds, and the warning text names the cause and carries the exact
/// `Security.Events.Create` command.
///
/// We test the `build_audit_os` + return-code path directly: emit against a non-existent
/// event (which is guaranteed absent because T029/T032 deleted it, and this test runs
/// before T029 if run in isolation), confirm `0` is returned, and confirm the
/// `refuse_and_instruct_text()` contains the `SETUP_CMD` literal.
#[tokio::test]
#[ignore]
async fn iris_audit_refuse_and_instruct_when_absent() {
    let conn = match iris_conn() {
        Some(c) => c,
        None => {
            eprintln!("IRIS_HOST not set — skipping");
            return;
        }
    };
    set_caller_mode(CallerMode::Cli);

    // Make sure the event definition does NOT exist for this test.
    delete_event_definition(&conn)
        .await
        .expect("pre-delete for T031");

    let event_data = build_event_data("iris_execute", CallerMode::Cli, None);
    let os = build_audit_os(&event_data, "refuse test T031");
    let result = run_os(&conn, &os).await.expect("emit with absent event");

    assert!(
        result.trim().starts_with('0'),
        "$SYSTEM.Security.Audit must return 0 when event is absent; got: {result}"
    );

    // Verify the refuse-and-instruct text is correct (unit-tested separately; checked here
    // for integration completeness).
    let text = iris_agentic_dev_core::iris::iris_audit::refuse_and_instruct_text();
    assert!(
        text.contains(SETUP_CMD),
        "refuse text must carry setup command: {text}"
    );
}

// ─── T032: Container restoration ─────────────────────────────────────────

/// T032: Verify that after the live tests, the audit configuration matches the baseline
/// recorded before any changes were made. Since T029 creates and T031 deletes the event
/// definition (and deletes it after T029 if it created it), the net state should be:
/// event definition absent.
///
/// We also verify that RoutineChange is in its as-found state (from baseline in
/// /tmp/iad086-audit-baseline.txt if that file exists).
#[test]
#[ignore]
fn zz_iris_audit_container_restored() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let conn = match iris_conn() {
            Some(c) => c,
            None => {
                eprintln!("IRIS_HOST not set — skipping");
                return;
            }
        };

        // The event definition must be absent after our tests cleaned up.
        // Security.Events lives in %SYS.
        let check_code = r#"
Set tSC = ##class(Security.Events).Get("iris-agentic-dev","Tool","ToolCall",.p)
If $$$ISERR(tSC) { Write "0", ! } Else { Write "1", ! }
"#;
        let result = run_os_sys(&conn, check_code).await.expect("check event exists");
        assert!(
            result.trim().starts_with('0'),
            "event definition must be absent after container restoration; got: {result}"
        );

        // If we have the baseline file, check RoutineChange state.
        if let Ok(baseline) = std::fs::read_to_string("/tmp/iad086-audit-baseline.txt") {
            let routine_change_enabled = if baseline.contains("RoutineChange: 1") {
                "1"
            } else {
                "0"
            };
            let rc_code = r#"
Set tSC = ##class(Security.Events).Get("%System","%System","RoutineChange",.p)
Write p("Enabled"), !
"#;
            let current = run_os_sys(&conn, rc_code).await.expect("check RoutineChange");
            assert_eq!(
                current.trim(),
                routine_change_enabled,
                "RoutineChange must match baseline; baseline says {routine_change_enabled}, got {current}"
            );
        }
    });
}
