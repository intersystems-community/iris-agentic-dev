//! Unit tests for pruning decision logic (T031). No live IRIS required.

use chrono::Utc;
use iris_agentic_dev_core::telemetry::prune::sessions_to_prune;
use uuid::Uuid;

fn ts_days_ago(days: i64) -> String {
    (Utc::now() - chrono::Duration::days(days)).to_rfc3339()
}

#[test]
fn removes_only_entries_older_than_retention_window() {
    let active = Uuid::new_v4();
    let fresh = Uuid::new_v4();
    let stale = Uuid::new_v4();
    let sessions = vec![(fresh, ts_days_ago(1)), (stale, ts_days_ago(60))];
    let result = sessions_to_prune(&sessions, active, Utc::now(), 30);
    assert_eq!(result, vec![stale]);
}

#[test]
fn never_removes_active_session_regardless_of_simulated_age() {
    let active = Uuid::new_v4();
    let other = Uuid::new_v4();
    let sessions = vec![(active, ts_days_ago(365)), (other, ts_days_ago(365))];
    let result = sessions_to_prune(&sessions, active, Utc::now(), 30);
    assert_eq!(result, vec![other]);
    assert!(!result.contains(&active));
}
