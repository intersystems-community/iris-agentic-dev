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
}
