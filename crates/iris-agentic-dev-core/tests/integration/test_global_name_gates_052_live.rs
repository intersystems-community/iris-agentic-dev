//! Spec 052 amendment (2026-10-10): the global-name gates against a live IRIS.
//!
//! Read actions only for the refusals. A regression that lets a name through must not write or kill
//! anything, so the blocked names here are only ever read or previewed. The one write is to
//! `^IadGate052.Probe`, an application global in both spellings, to show the gate is not a wall.
//!
//! Run with:
//!   IRIS_HOST=localhost IRIS_WEB_PORT=52780 IRIS_USERNAME=_SYSTEM IRIS_PASSWORD=SYS \
//!   cargo test --features testing --test integration test_global_name_gates_052_live -- \
//!     --ignored --test-threads=1

use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};
use iris_agentic_dev_core::tools::IrisTools;

fn tools() -> Option<IrisTools> {
    let host = std::env::var("IRIS_HOST").unwrap_or_default();
    if host.is_empty() {
        if std::env::var("IAD_ALLOW_SKIP").is_ok() {
            eprintln!("SKIP (IAD_ALLOW_SKIP set): IRIS_HOST unset");
            return None;
        }
        panic!(
            "IRIS_HOST unset. These tests prove a gate against live IRIS; without IRIS they assert \
             nothing. Set IRIS_HOST=localhost IRIS_WEB_PORT=52780, or IAD_ALLOW_SKIP=1."
        );
    }
    let port = std::env::var("IRIS_WEB_PORT").unwrap_or_else(|_| "52780".into());
    let user = std::env::var("IRIS_USERNAME").unwrap_or_else(|_| "_SYSTEM".into());
    let pass = std::env::var("IRIS_PASSWORD").unwrap_or_else(|_| "SYS".into());
    let conn = IrisConnection::new(
        format!("http://{host}:{port}"),
        "USER",
        user,
        pass,
        DiscoverySource::EnvVar,
    );
    Some(IrisTools::new(Some(conn)).expect("IrisTools::new"))
}

async fn call(t: &IrisTools, tool: &'static str, args: serde_json::Value) -> serde_json::Value {
    let r = t.call_for_test(tool, args).await.expect("call");
    let text = r.content[0].as_text().unwrap().text.clone();
    serde_json::from_str(&text).unwrap_or_else(|_| serde_json::json!({"raw": text}))
}

#[tokio::test]
#[ignore]
async fn the_system_global_is_refused_in_both_spellings_on_every_read_path() {
    let Some(t) = tools() else { return };
    for name in ["oddDEF", "^oddDEF", "ROUTINE", "^ROUTINE", "^rOBJ"] {
        let get = call(&t, "iris_global", serde_json::json!({"action": "get", "global_name": name, "subscripts": ["IadGate052NoSuchNode"]})).await;
        assert_eq!(get["error_code"], "SYSTEM_BLOCKLIST", "get {name}: {get}");
        let list = call(
            &t,
            "iris_global",
            serde_json::json!({"action": "list", "global_name": name}),
        )
        .await;
        assert_eq!(
            list["error_code"], "SYSTEM_BLOCKLIST",
            "list {name}: {list}"
        );
        let preview = call(
            &t,
            "global_preview",
            serde_json::json!({"global": name, "count": 1}),
        )
        .await;
        assert_eq!(
            preview["error_code"], "SYSTEM_BLOCKLIST",
            "preview {name}: {preview}"
        );
        assert!(preview.get("confirm_token").is_none(), "{name}: {preview}");
        assert!(
            preview.get("entries").is_none(),
            "no data came back for {name}: {preview}"
        );
    }
}

#[tokio::test]
#[ignore]
async fn a_phi_named_global_is_refused_in_both_spellings_until_acknowledged() {
    let Some(t) = tools() else { return };
    for name in ["PAPMIPatient", "^PAPMIPatient"] {
        let get = call(
            &t,
            "iris_global",
            serde_json::json!({"action": "get", "global_name": name, "subscripts": ["1"]}),
        )
        .await;
        assert_eq!(get["error_code"], "PHI_GATE_BLOCKED", "{name}: {get}");
        let preview = call(
            &t,
            "global_preview",
            serde_json::json!({"global": name, "count": 1}),
        )
        .await;
        assert_eq!(
            preview["error_code"], "PHI_GATE_BLOCKED",
            "preview {name}: {preview}"
        );
        let ack = call(&t, "iris_global", serde_json::json!({"action": "get", "global_name": name, "subscripts": ["1"], "acknowledgePhi": true})).await;
        assert_eq!(ack["success"], true, "acknowledged {name}: {ack}");
    }
}

#[tokio::test]
#[ignore]
async fn an_embedded_subscript_or_extended_reference_is_not_a_name() {
    let Some(t) = tools() else { return };
    for name in [
        "oddDEF(\"IadGate052NoSuchNode\")",
        "^|\"%SYS\"|ROUTINE",
        "^^ROUTINE",
        "ROUTINE ",
    ] {
        let get = call(
            &t,
            "iris_global",
            serde_json::json!({"action": "get", "global_name": name}),
        )
        .await;
        assert_eq!(get["error_code"], "INVALID_GLOBAL_NAME", "{name:?}: {get}");
    }
}

#[tokio::test]
#[ignore]
async fn an_application_global_round_trips_in_both_spellings() {
    let Some(t) = tools() else { return };
    // The write and destructive tiers are the operator's; the dev container's default allows both.
    for (i, name) in ["IadGate052.Probe", "^IadGate052.Probe"].iter().enumerate() {
        let value = format!("v{i}");
        let set = call(&t, "iris_global", serde_json::json!({"action": "set", "global_name": name, "subscripts": ["k"], "value": value})).await;
        assert_eq!(set["success"], true, "set {name}: {set}");
        let get = call(
            &t,
            "iris_global",
            serde_json::json!({"action": "get", "global_name": name, "subscripts": ["k"]}),
        )
        .await;
        assert_eq!(get["value"], value.as_str(), "get {name}: {get}");
        let kill = call(
            &t,
            "iris_global",
            serde_json::json!({"action": "kill", "global_name": name, "subscripts": ["k"]}),
        )
        .await;
        assert_eq!(kill["success"], true, "kill {name}: {kill}");
    }
}

/// FR-022: the blocklist names every code-storage global family this instance holds. Enumerates
/// `^$GLOBAL` in USER and %SYS (read only) and checks each `^odd*` and `^r<Upper>*` global against
/// the gate in both spellings, so a family a newer IRIS adds fails here instead of going unnoticed.
#[tokio::test]
#[ignore]
async fn the_blocklist_covers_every_code_storage_global_on_this_instance() {
    use iris_agentic_dev_core::policy::system_blocklist_gate::check_system_blocklist;
    let host = std::env::var("IRIS_HOST").unwrap_or_default();
    if host.is_empty() {
        if std::env::var("IAD_ALLOW_SKIP").is_ok() {
            return;
        }
        panic!("IRIS_HOST unset; set IRIS_HOST=localhost IRIS_WEB_PORT=52780 or IAD_ALLOW_SKIP=1");
    }
    let port = std::env::var("IRIS_WEB_PORT").unwrap_or_else(|_| "52780".into());
    let conn = IrisConnection::new(
        format!("http://{host}:{port}"),
        "USER",
        std::env::var("IRIS_USERNAME").unwrap_or_else(|_| "_SYSTEM".into()),
        std::env::var("IRIS_PASSWORD").unwrap_or_else(|_| "SYS".into()),
        DiscoverySource::EnvVar,
    );
    let client = reqwest::Client::new();
    let mut missing = Vec::new();
    let mut seen = 0;
    for ns in ["USER", "%SYS"] {
        let out = conn
            .execute_via_generator(
                " Set g=\"\" For { Set g=$Order(^$GLOBAL(g)) Quit:g=\"\"  Write g,$C(10) }",
                ns,
                &client,
            )
            .await
            .expect("enumerate globals");
        for line in out.lines() {
            let name = line.trim().trim_start_matches('^');
            let mut chars = name.chars();
            let code_storage = name.starts_with("odd")
                || (name.starts_with('r') && chars.nth(1).is_some_and(|c| c.is_ascii_uppercase()))
                || name == "ROUTINE";
            if !code_storage {
                continue;
            }
            seen += 1;
            for form in [name.to_string(), format!("^{name}")] {
                if check_system_blocklist(&form, &[], &[], false, "s").is_none() {
                    missing.push(format!("{ns}: {form}"));
                }
            }
        }
    }
    assert!(
        seen > 10,
        "the enumeration found only {seen} code-storage globals; the check is not running"
    );
    assert!(
        missing.is_empty(),
        "code-storage globals not on SYSTEM_BLOCKLIST: {missing:?}"
    );
}
