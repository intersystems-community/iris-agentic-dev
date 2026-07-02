//! Age-based pruning of the durable telemetry sink. Never prunes the active session —
//! see specs/059-tool-telemetry-benchmark/research.md's "Pruning policy" decision: only
//! already-exited sessions are ever eligible, satisfying FR-011/SC-006 by construction.

use crate::iris::connection::IrisConnection;
use chrono::{DateTime, Utc};
use std::path::Path;
use std::sync::Arc;
use uuid::Uuid;

pub const DEFAULT_RETENTION_DAYS: u64 = 30;

/// Pure decision function: given a session's first-record timestamp (RFC3339) and the
/// current time, should this session be pruned? `active_session`/`session_id` equality
/// is checked by the caller before invoking this — this function only judges age.
pub fn is_expired(first_record_timestamp: &str, now: DateTime<Utc>, retention_days: u64) -> bool {
    match DateTime::parse_from_rfc3339(first_record_timestamp) {
        Ok(ts) => {
            let age = now.signed_duration_since(ts.with_timezone(&Utc));
            age.num_days() >= retention_days as i64
        }
        // Malformed/unparseable timestamp: do not prune (fail safe, never data-loss).
        Err(_) => false,
    }
}

/// Filters a list of `(session_id, first_record_timestamp)` pairs down to those eligible
/// for pruning: expired AND not the active session.
pub fn sessions_to_prune(
    sessions: &[(Uuid, String)],
    active_session: Uuid,
    now: DateTime<Utc>,
    retention_days: u64,
) -> Vec<Uuid> {
    sessions
        .iter()
        .filter(|(id, _)| *id != active_session)
        .filter(|(_, ts)| is_expired(ts, now, retention_days))
        .map(|(id, _)| *id)
        .collect()
}

/// Deletes durable-sink data for the given session ids from whichever sink is active.
/// Best-effort — swallows errors (pruning failure must never be a hard error).
pub async fn prune_sessions(
    session_ids: &[Uuid],
    iris: Option<Arc<IrisConnection>>,
    client: &reqwest::Client,
    config_dir: &Path,
) {
    match iris {
        Some(iris) => {
            for sid in session_ids {
                let code = format!("kill ^IRISDEV(\"telemetry\",\"{sid}\")\n");
                if let Err(e) = iris.execute_via_generator(&code, "USER", client).await {
                    tracing::debug!("telemetry prune (IRIS) failed for {sid}: {e}");
                }
            }
        }
        None => {
            for sid in session_ids {
                let path = config_dir.join("telemetry").join(format!("{sid}.jsonl"));
                if let Err(e) = std::fs::remove_file(&path) {
                    if e.kind() != std::io::ErrorKind::NotFound {
                        tracing::debug!("telemetry prune (local file) failed for {sid}: {e}");
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(days_ago: i64, now: DateTime<Utc>) -> String {
        (now - chrono::Duration::days(days_ago)).to_rfc3339()
    }

    #[test]
    fn is_expired_true_past_retention_window() {
        let now = Utc::now();
        assert!(is_expired(&ts(31, now), now, 30));
    }

    #[test]
    fn is_expired_false_within_retention_window() {
        let now = Utc::now();
        assert!(!is_expired(&ts(5, now), now, 30));
    }

    #[test]
    fn is_expired_false_for_malformed_timestamp() {
        let now = Utc::now();
        assert!(!is_expired("not-a-timestamp", now, 30));
    }

    #[test]
    fn sessions_to_prune_excludes_active_session_regardless_of_age() {
        let now = Utc::now();
        let active = Uuid::new_v4();
        let old_other = Uuid::new_v4();
        let sessions = vec![
            (active, ts(365, now)), // very old but active — must never be pruned
            (old_other, ts(365, now)),
        ];
        let result = sessions_to_prune(&sessions, active, now, 30);
        assert_eq!(result, vec![old_other]);
    }

    #[test]
    fn sessions_to_prune_only_removes_entries_older_than_window() {
        let now = Utc::now();
        let active = Uuid::new_v4();
        let fresh = Uuid::new_v4();
        let stale = Uuid::new_v4();
        let sessions = vec![(fresh, ts(1, now)), (stale, ts(60, now))];
        let result = sessions_to_prune(&sessions, active, now, 30);
        assert_eq!(result, vec![stale]);
    }
}
