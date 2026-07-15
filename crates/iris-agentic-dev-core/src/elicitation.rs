//! Elicitation state management for MCP source control dialogs.
//! Stores pending elicitations keyed by UUID, expires after 5 minutes.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use uuid::Uuid;

const EXPIRY: Duration = Duration::from_secs(300); // 5 minutes

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ElicitationAction {
    /// Resume a iris_doc(mode=put) write
    Put,
    /// Resume an iris_source_control execute action
    ScmExecute,
}

/// Serde-friendly mirror of [`PendingElicitation`] without the non-serializable [`Instant`].
/// Used for serialization, deserialization, and persistence of elicitation state.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PendingElicitationRecord {
    pub id: String,
    pub document: String,
    pub action: ElicitationAction,
    pub content: Option<String>,
    pub scm_action_id: Option<String>,
    pub namespace: String,
}

#[derive(Debug, Clone)]
pub struct PendingElicitation {
    pub id: String,
    pub document: String,
    pub action: ElicitationAction,
    /// Document content to write on resume (Put only)
    pub content: Option<String>,
    /// SCM action id to execute on resume (ScmExecute only)
    pub scm_action_id: Option<String>,
    pub namespace: String,
    pub expires_at: Instant,
}

/// TTL for a cached "this document is checked out by me" entry. Kept short so a
/// stale entry (e.g. someone reverted the checkout out-of-band) self-heals quickly;
/// the IRIS-side write rejection is the ultimate backstop, and any SCM action we run
/// on the doc invalidates the entry immediately (see [`CheckoutCache::invalidate`]).
const CHECKOUT_CACHE_TTL: Duration = Duration::from_secs(60);

/// Session-scoped cache of documents we have already checked out under this connection.
///
/// The pre-write SCM probe (query MenuItems / UserAction) is one IRIS round-trip per write.
/// On a chained surgical edit (repeated insert/delete_lines on the same doc) that probe
/// returns the same "already checked out by me → proceed" answer every time, so we cache it.
///
/// Keyed by `(namespace, document)`. Entries expire after [`CHECKOUT_CACHE_TTL`] and are
/// cleared explicitly whenever an SCM action (undo checkout, check-in, disconnect) runs on
/// the doc. A cached write that IRIS still rejects must clear its own entry so the retry
/// re-probes rather than looping on a bad cache.
#[derive(Clone, Default)]
pub struct CheckoutCache(Arc<Mutex<HashMap<(String, String), Instant>>>);

impl CheckoutCache {
    pub fn new() -> Self {
        Self::default()
    }

    fn key(namespace: &str, document: &str) -> (String, String) {
        (namespace.to_string(), document.to_string())
    }

    /// Record that `document` in `namespace` is checked out by us (or freely writable),
    /// so subsequent writes can skip the pre-write SCM probe until the entry expires.
    pub fn mark(&self, namespace: &str, document: &str) {
        self.0.lock().unwrap().insert(
            Self::key(namespace, document),
            Instant::now() + CHECKOUT_CACHE_TTL,
        );
    }

    /// Returns true if we have a live (non-expired) checkout entry for this document.
    /// Expired entries are removed on access so a cache miss always re-probes IRIS.
    pub fn is_checked_out(&self, namespace: &str, document: &str) -> bool {
        let mut store = self.0.lock().unwrap();
        let key = Self::key(namespace, document);
        match store.get(&key) {
            Some(expires) if Instant::now() <= *expires => true,
            Some(_) => {
                store.remove(&key);
                false
            }
            None => false,
        }
    }

    /// Drop the cached checkout entry for this document — call after any SCM action that
    /// changes checkout state (undo/checkin/disconnect) or after a write IRIS rejected.
    pub fn invalidate(&self, namespace: &str, document: &str) {
        self.0
            .lock()
            .unwrap()
            .remove(&Self::key(namespace, document));
    }
}

#[derive(Clone, Default)]
pub struct ElicitationStore(Arc<Mutex<HashMap<String, PendingElicitation>>>);

impl ElicitationStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a new pending elicitation and return its UUID.
    pub fn insert(
        &self,
        document: impl Into<String>,
        action: ElicitationAction,
        content: Option<String>,
        scm_action_id: Option<String>,
        namespace: impl Into<String>,
    ) -> String {
        let id = Uuid::new_v4().to_string();
        let entry = PendingElicitation {
            id: id.clone(),
            document: document.into(),
            action,
            content,
            scm_action_id,
            namespace: namespace.into(),
            expires_at: Instant::now() + EXPIRY,
        };
        self.0.lock().unwrap().insert(id.clone(), entry);
        id
    }

    /// Look up a pending elicitation by id. Returns None if expired or missing.
    pub fn lookup(&self, id: &str) -> Option<PendingElicitation> {
        let mut store = self.0.lock().unwrap();
        let entry = store.get(id)?;
        if Instant::now() > entry.expires_at {
            store.remove(id);
            return None;
        }
        Some(entry.clone())
    }

    /// Remove a pending elicitation.
    pub fn clear(&self, id: &str) {
        self.0.lock().unwrap().remove(id);
    }

    /// Remove all expired entries. Returns the count of removed entries.
    pub fn sweep(&self) -> usize {
        let mut store = self.0.lock().unwrap();
        let now = std::time::Instant::now();
        let expired: Vec<String> = store
            .iter()
            .filter(|(_, e)| now > e.expires_at)
            .map(|(k, _)| k.clone())
            .collect();
        let count = expired.len();
        for key in expired {
            store.remove(&key);
        }
        count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_insert_returns_uuid() {
        let store = ElicitationStore::new();
        let id = store.insert("Foo.cls", ElicitationAction::Put, None, None, "USER");
        assert!(!id.is_empty());
    }

    #[test]
    fn test_lookup_finds_inserted() {
        let store = ElicitationStore::new();
        let id = store.insert(
            "Foo.cls",
            ElicitationAction::Put,
            Some("content".into()),
            None,
            "USER",
        );
        let pending = store.lookup(&id).expect("should find it");
        assert_eq!(pending.document, "Foo.cls");
        assert_eq!(pending.namespace, "USER");
        assert_eq!(pending.content.as_deref(), Some("content"));
    }

    #[test]
    fn test_lookup_missing_returns_none() {
        let store = ElicitationStore::new();
        assert!(store.lookup("nonexistent-id").is_none());
    }

    #[test]
    fn test_clear_removes_entry() {
        let store = ElicitationStore::new();
        let id = store.insert(
            "Bar.cls",
            ElicitationAction::ScmExecute,
            None,
            Some("CheckOut".into()),
            "USER",
        );
        store.clear(&id);
        assert!(store.lookup(&id).is_none());
    }

    #[test]
    fn test_sweep_empty_store() {
        let store = ElicitationStore::new();
        assert_eq!(store.sweep(), 0);
    }

    #[test]
    fn test_sweep_removes_nothing_fresh() {
        let store = ElicitationStore::new();
        store.insert("A.cls", ElicitationAction::Put, None, None, "USER");
        store.insert("B.cls", ElicitationAction::Put, None, None, "USER");
        assert_eq!(store.sweep(), 0, "fresh entries should not be swept");
    }

    #[test]
    fn test_scm_execute_action_fields() {
        let store = ElicitationStore::new();
        let id = store.insert(
            "App.cls",
            ElicitationAction::ScmExecute,
            None,
            Some("CheckIn".into()),
            "MYNS",
        );
        let p = store.lookup(&id).unwrap();
        assert!(matches!(p.action, ElicitationAction::ScmExecute));
        assert_eq!(p.scm_action_id.as_deref(), Some("CheckIn"));
        assert_eq!(p.namespace, "MYNS");
    }

    // ── Serde tests ──────────────────────────────────────────────────────────

    #[test]
    fn serde_action_put_roundtrip() {
        let action = ElicitationAction::Put;
        let json = serde_json::to_string(&action).expect("serialize Put");
        let back: ElicitationAction = serde_json::from_str(&json).expect("deserialize Put");
        assert_eq!(back, ElicitationAction::Put);
    }

    #[test]
    fn serde_action_scm_execute_roundtrip() {
        let action = ElicitationAction::ScmExecute;
        let json = serde_json::to_string(&action).expect("serialize ScmExecute");
        let back: ElicitationAction = serde_json::from_str(&json).expect("deserialize ScmExecute");
        assert_eq!(back, ElicitationAction::ScmExecute);
    }

    #[test]
    fn serde_action_put_json_value() {
        let action = ElicitationAction::Put;
        let v: serde_json::Value = serde_json::to_value(&action).unwrap();
        assert_eq!(v, serde_json::Value::String("Put".to_string()));
    }

    #[test]
    fn serde_action_scm_execute_json_value() {
        let action = ElicitationAction::ScmExecute;
        let v: serde_json::Value = serde_json::to_value(&action).unwrap();
        assert_eq!(v, serde_json::Value::String("ScmExecute".to_string()));
    }

    #[test]
    fn serde_action_unknown_variant_fails() {
        let result: Result<ElicitationAction, _> = serde_json::from_str("\"UnknownVariant\"");
        assert!(
            result.is_err(),
            "deserializing an unknown variant must fail"
        );
    }

    #[test]
    fn serde_record_put_roundtrip() {
        let record = PendingElicitationRecord {
            id: "test-id-1".into(),
            document: "Pkg.Foo.cls".into(),
            action: ElicitationAction::Put,
            content: Some("Class Pkg.Foo {}".into()),
            scm_action_id: None,
            namespace: "USER".into(),
        };
        let json = serde_json::to_string(&record).expect("serialize record");
        let back: PendingElicitationRecord =
            serde_json::from_str(&json).expect("deserialize record");
        assert_eq!(back, record);
    }

    #[test]
    fn serde_record_scm_execute_roundtrip() {
        let record = PendingElicitationRecord {
            id: "test-id-2".into(),
            document: "App.Router.cls".into(),
            action: ElicitationAction::ScmExecute,
            content: None,
            scm_action_id: Some("CheckIn".into()),
            namespace: "PRODUCTION".into(),
        };
        let json = serde_json::to_string(&record).expect("serialize scm record");
        let back: PendingElicitationRecord =
            serde_json::from_str(&json).expect("deserialize scm record");
        assert_eq!(back, record);
        assert_eq!(back.scm_action_id.as_deref(), Some("CheckIn"));
        assert!(back.content.is_none());
    }

    #[test]
    fn serde_record_optional_fields_none() {
        let record = PendingElicitationRecord {
            id: "test-id-3".into(),
            document: "X.cls".into(),
            action: ElicitationAction::Put,
            content: None,
            scm_action_id: None,
            namespace: "NS".into(),
        };
        let json = serde_json::to_string(&record).unwrap();
        let back: PendingElicitationRecord = serde_json::from_str(&json).unwrap();
        assert!(back.content.is_none());
        assert!(back.scm_action_id.is_none());
    }

    #[test]
    fn serde_record_missing_required_field_fails() {
        // "namespace" field is intentionally omitted — deserialization must fail.
        let json =
            r#"{"id":"x","document":"X.cls","action":"Put","content":null,"scm_action_id":null}"#;
        let result: Result<PendingElicitationRecord, _> = serde_json::from_str(json);
        assert!(
            result.is_err(),
            "missing required field 'namespace' must fail"
        );
    }

    #[test]
    fn serde_record_from_raw_json() {
        let json = r#"{
            "id": "abc-123",
            "document": "My.Doc.cls",
            "action": "ScmExecute",
            "content": null,
            "scm_action_id": "GetLatest",
            "namespace": "LIVE"
        }"#;
        let record: PendingElicitationRecord = serde_json::from_str(json).expect("parse raw JSON");
        assert_eq!(record.id, "abc-123");
        assert_eq!(record.document, "My.Doc.cls");
        assert_eq!(record.action, ElicitationAction::ScmExecute);
        assert_eq!(record.scm_action_id.as_deref(), Some("GetLatest"));
        assert_eq!(record.namespace, "LIVE");
        assert!(record.content.is_none());
    }

    #[test]
    fn test_lookup_expired_returns_none() {
        let store = ElicitationStore::new();
        let id = "expired-id-123".to_string();
        // Insert directly with an already-expired timestamp
        let entry = PendingElicitation {
            id: id.clone(),
            document: "Exp.cls".into(),
            action: ElicitationAction::Put,
            content: None,
            scm_action_id: None,
            namespace: "USER".into(),
            expires_at: Instant::now() - Duration::from_secs(1),
        };
        store.0.lock().unwrap().insert(id.clone(), entry);
        // Lookup should return None and remove the entry
        assert!(store.lookup(&id).is_none());
        // Entry should be removed from the store
        assert!(store.0.lock().unwrap().get(&id).is_none());
    }

    #[test]
    fn test_sweep_removes_expired_entries() {
        let store = ElicitationStore::new();
        // Insert one expired and one fresh entry
        let expired_id = "expired-sweep-id".to_string();
        let fresh_id = store.insert("Fresh.cls", ElicitationAction::Put, None, None, "USER");
        let expired_entry = PendingElicitation {
            id: expired_id.clone(),
            document: "Old.cls".into(),
            action: ElicitationAction::ScmExecute,
            content: None,
            scm_action_id: None,
            namespace: "USER".into(),
            expires_at: Instant::now() - Duration::from_secs(1),
        };
        store
            .0
            .lock()
            .unwrap()
            .insert(expired_id.clone(), expired_entry);
        let removed = store.sweep();
        assert_eq!(removed, 1, "should have removed exactly 1 expired entry");
        // Expired entry gone, fresh entry still there
        assert!(store.lookup(&expired_id).is_none());
        assert!(store.lookup(&fresh_id).is_some());
    }

    // ── CheckoutCache ─────────────────────────────────────────────────────────
    #[test]
    fn test_checkout_cache_miss_by_default() {
        let cache = CheckoutCache::new();
        assert!(!cache.is_checked_out("USER", "Foo.cls"));
    }

    #[test]
    fn test_checkout_cache_hit_after_mark() {
        let cache = CheckoutCache::new();
        cache.mark("USER", "Foo.cls");
        assert!(cache.is_checked_out("USER", "Foo.cls"));
    }

    #[test]
    fn test_checkout_cache_is_keyed_by_namespace_and_doc() {
        let cache = CheckoutCache::new();
        cache.mark("USER", "Foo.cls");
        // Same doc, different namespace → miss.
        assert!(!cache.is_checked_out("DVP", "Foo.cls"));
        // Same namespace, different doc → miss.
        assert!(!cache.is_checked_out("USER", "Bar.cls"));
    }

    #[test]
    fn test_checkout_cache_invalidate_clears_entry() {
        let cache = CheckoutCache::new();
        cache.mark("USER", "Foo.cls");
        cache.invalidate("USER", "Foo.cls");
        assert!(!cache.is_checked_out("USER", "Foo.cls"));
    }

    #[test]
    fn test_checkout_cache_invalidate_missing_is_noop() {
        let cache = CheckoutCache::new();
        cache.invalidate("USER", "Nonexistent.cls"); // must not panic
        assert!(!cache.is_checked_out("USER", "Nonexistent.cls"));
    }

    #[test]
    fn test_checkout_cache_expired_entry_is_a_miss() {
        let cache = CheckoutCache::new();
        // Insert an already-expired entry directly (expires 1s in the past).
        cache.0.lock().unwrap().insert(
            ("USER".to_string(), "Old.cls".to_string()),
            Instant::now() - Duration::from_secs(1),
        );
        assert!(
            !cache.is_checked_out("USER", "Old.cls"),
            "expired entry must read as a miss"
        );
        // And the expired entry is evicted on access.
        assert!(cache.0.lock().unwrap().is_empty());
    }
}
