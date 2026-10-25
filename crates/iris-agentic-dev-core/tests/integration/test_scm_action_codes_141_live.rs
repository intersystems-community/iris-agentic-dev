//! Spec 141 T197, live: the answer a source-control hook gives to `UserAction` is read from a
//! labelled record and every action code is classified; an unknown or unreadable one refuses.
//!
//! Before, the code was read with `parse::<u8>().ok().unwrap_or(0)` from the first line of output,
//! and 0 means "no dialog needed". A hook that wrote a banner first, answered with text, or
//! answered with a code the tool does not handle (2 to 5, or 42) therefore read as "proceed", and
//! `iris_doc put` wrote a document it had not checked out.
//!
//! The fixture is a throwaway source-control class, `Iad141.ScmHook`, installed in a scratch
//! namespace `IAD141SC` that shares the USER database. What its `UserAction` answers is read from
//! `^Iad141ScmMode` on every call. The namespace's source-control setting, the class, the probe
//! document, the globals and the namespace are all put back when the fixture is dropped, a failed
//! assertion included.
//!
//! Run, serialised behind the shared lock:
//!
//! ```text
//! lockf -k /tmp/iad-141-iris.lock cargo test --features testing --test integration \
//!     scm_action_codes_141 -- --ignored --test-threads=1
//! ```

use iris_agentic_dev_core::elicitation::{CheckoutCache, ElicitationStore};
use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};
use iris_agentic_dev_core::testing::LiveTarget;
use iris_agentic_dev_core::tools::scm::{handle_iris_source_control, ScmParams};
use iris_agentic_dev_core::tools::{IrisTools, Toolset};
use serde_json::{json, Value};

const NS: &str = "IAD141SC";
const HOOK: &str = "Iad141.ScmHook";
const PROBE: &str = "Iad141.Probe";
const PROBE_DOC: &str = "Iad141.Probe.cls";
const PROBE_BODY: &str = "Class Iad141.Probe\n{\nClassMethod Run() As %String\n{\n  Quit 1\n}\n}\n";

/// What the hook does is chosen by `^Iad141ScmMode`:
///
/// - `code:N` answers with the number N, `scode:N` with the string "N" (what a hook that parses its
///   answer out of a reply gives);
/// - `noise` and `noise_nonl` write provider chatter before answering 1 (the second leaves the
///   line unterminated, so the tool's record shares a line with it);
/// - `spoof` writes a record of its own that claims code 0, then answers 1;
/// - `text` answers "abc", `empty` answers "", `error` answers 0 with an error status.
///
/// - `nonew` makes `Login` fail (the final `%OnNew` returns its status), so `SourceControlCreate` leaves no source-control object.
///
/// `^Iad141ScmAfter` picks what `AfterUserAction` does: `fail`, `noise`, or anything else for OK.
const HOOK_SOURCE: &[&str] = &[
    "Class Iad141.ScmHook Extends %Studio.Extension.Base",
    "{",
    "Method UserAction(Type As %Integer, Name As %String, InternalName As %String, SelectedText As %String, ByRef Action As %String, ByRef Target As %String, ByRef Msg As %String, ByRef Reload As %Boolean) As %Status",
    "{",
    "  Set tMode = $Get(^Iad141ScmMode)",
    "  Set Target = \"hook target for \" _ tMode",
    "  Set Msg = \"\"",
    "  Set Reload = 0",
    "  Set Action = 0",
    "  If tMode = \"noise\" { Write \"Connected to repo X\",! Write \"NOTICE: no Perforce user defined\",! Set Action = 1 Quit $$$OK }",
    "  If tMode = \"noise_nonl\" { Write \"CMD: p4 opened\" Set Action = 1 Quit $$$OK }",
    "  If tMode = \"spoof\" { Write \"IADSCM|{\"\"kind\"\":\"\"action\"\",\"\"code\"\":0,\"\"ok\"\":1}\",! Set Action = 1 Quit $$$OK }",
    "  If tMode = \"text\" { Set Action = \"abc\" Quit $$$OK }",
    "  If tMode = \"empty\" { Set Action = \"\" Quit $$$OK }",
    "  If tMode = \"error\" { Quit $$$ERROR($$$GeneralError, \"hook failed\") }",
    "  If $Piece(tMode, \":\", 1) = \"code\" { Set Action = +$Piece(tMode, \":\", 2) Quit $$$OK }",
    "  If $Piece(tMode, \":\", 1) = \"scode\" { Set Action = $Piece(tMode, \":\", 2) Quit $$$OK }",
    "  Quit $$$OK",
    "}",
    "Method Login(Name As %String, Password As %String) As %Status",
    "{",
    "  If $Get(^Iad141ScmMode) = \"nonew\" { Quit $$$ERROR($$$GeneralError, \"hook will not start\") }",
    "  Quit $$$OK",
    "}",
    "Method AfterUserAction(Type As %Integer, Name As %String, InternalName As %String, Answer As %Integer, Msg As %String = \"\", ByRef Reload As %Boolean) As %Status",
    "{",
    "  Set Reload = 0",
    "  Set tAfter = $Get(^Iad141ScmAfter)",
    "  If tAfter = \"fail\" { Quit $$$ERROR($$$GeneralError, \"after failed\") }",
    "  If tAfter = \"noise\" { Write \"NOTICE: committed to the depot\",! }",
    "  Quit $$$OK",
    "}",
    "}",
];

/// Run `code` in `ns` on the live server's container, the way a terminal would. The fixture is
/// built here and called over HTTP at `base_url()`: both read one `LiveTarget`, so they are one
/// instance.
fn session(ns: &str, code: &str) -> String {
    LiveTarget::from_env().session(ns, code)
}

fn base_url() -> String {
    LiveTarget::from_env().base_url()
}

fn conn() -> IrisConnection {
    let t = LiveTarget::from_env();
    IrisConnection::new(
        t.base_url(),
        NS,
        &t.username,
        &t.password,
        DiscoverySource::EnvVar,
    )
}

/// The scratch namespace with the hook installed; puts everything back on drop.
struct Fixture {
    original_class: String,
    tools: IrisTools,
    client: reqwest::Client,
}

impl Fixture {
    async fn new() -> Self {
        LiveTarget::from_env().assert_running();
        // A run killed before its cleanup leaves the namespace and the class behind.
        cleanup("");
        let out = session(
            "%SYS",
            &format!(
                "Set p(\"Globals\")=\"USER\",p(\"Routines\")=\"USER\" Write \"NS:\",##class(Config.Namespaces).Create(\"{NS}\",.p),!"
            ),
        );
        assert!(out.contains("NS:1"), "scratch namespace not created: {out}");
        let original_class = session(
            NS,
            &format!("Write \"SC[\",##class(%Studio.SourceControl.Interface).SourceControlClassGet(\"{NS}\"),\"]\",!"),
        )
        .split("SC[")
        .nth(1)
        .and_then(|s| s.split(']').next())
        .unwrap_or("")
        .to_string();

        let client = reqwest::Client::new();
        let target = LiveTarget::from_env();
        let put = client
            .put(format!("{}/api/atelier/v1/{NS}/doc/{HOOK}.cls", base_url()))
            .basic_auth(&target.username, Some(&target.password))
            .json(&json!({"enc": false, "content": HOOK_SOURCE}))
            .send()
            .await
            .expect("PUT hook");
        assert!(put.status().is_success(), "PUT hook: {}", put.status());
        let compile: Value = client
            .post(format!(
                "{}/api/atelier/v1/{NS}/action/compile?flags=cuk",
                base_url()
            ))
            .basic_auth(&target.username, Some(&target.password))
            .json(&json!([format!("{HOOK}.cls")]))
            .send()
            .await
            .expect("compile hook")
            .json()
            .await
            .unwrap();
        assert!(
            compile["status"]["errors"]
                .as_array()
                .is_some_and(|e| e.is_empty()),
            "hook did not compile: {compile}"
        );
        let set = session(
            NS,
            &format!(
                "Write \"SET:\",##class(%Studio.SourceControl.Interface).SourceControlClassSet(\"{HOOK}\",\"{NS}\"),!"
            ),
        );
        assert!(set.contains("SET:1"), "hook not installed: {set}");

        let tools = IrisTools::for_test_gates_open(Some(conn()), Toolset::Merged).unwrap();
        Fixture {
            original_class,
            tools,
            client,
        }
    }

    fn mode(&self, mode: &str) {
        self.set_globals(mode, "");
    }

    fn set_globals(&self, mode: &str, after: &str) {
        let out = session(
            NS,
            &format!("Set ^Iad141ScmMode=\"{mode}\",^Iad141ScmAfter=\"{after}\" Write \"MODE:\",^Iad141ScmMode,!"),
        );
        assert!(out.contains(&format!("MODE:{mode}")), "mode not set: {out}");
    }

    /// Is the probe document stored in the scratch namespace?
    fn probe_exists(&self) -> bool {
        let out = session(
            NS,
            &format!("Write \"EXISTS:\",##class(%Library.RoutineMgr).Exists(\"{PROBE_DOC}\"),!"),
        );
        assert!(out.contains("EXISTS:"), "cannot read existence: {out}");
        out.contains("EXISTS:1")
    }

    fn forget_probe(&self) {
        session(
            NS,
            &format!("Do $SYSTEM.OBJ.Delete(\"{PROBE}\",\"/displaylog=0\")"),
        );
        assert!(
            !self.probe_exists(),
            "the probe document could not be removed"
        );
    }

    async fn put(&self) -> Value {
        body(
            self.tools
                .call_gated(
                    "iris_doc",
                    json!({"mode": "put", "name": PROBE_DOC, "namespace": NS, "content": PROBE_BODY}),
                )
                .await,
        )
    }

    async fn scm(&self, args: Value) -> Value {
        body(self.tools.call_gated("iris_source_control", args).await)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        cleanup(&self.original_class);
    }
}

/// Put the namespace's source-control setting back, delete the class, the probe document, the
/// globals and the scratch namespace. Safe to run when nothing is there.
fn cleanup(original_class: &str) {
    session(
        "%SYS",
        &format!(
            "If ##class(Config.Namespaces).Exists(\"{NS}\") {{ Do ##class(%Studio.SourceControl.Interface).SourceControlClassSet(\"{original_class}\",\"{NS}\") }}"
        ),
    );
    session(
        "USER",
        &format!(
            "Do $SYSTEM.OBJ.Delete(\"{PROBE}\",\"/displaylog=0\") Do $SYSTEM.OBJ.Delete(\"{HOOK}\",\"/displaylog=0\") Kill ^Iad141ScmMode,^Iad141ScmAfter"
        ),
    );
    session(
        "%SYS",
        &format!("If ##class(Config.Namespaces).Exists(\"{NS}\") {{ Do ##class(Config.Namespaces).Delete(\"{NS}\") }}"),
    );
}

fn body<E: std::fmt::Debug>(r: Result<rmcp::model::CallToolResult, E>) -> Value {
    let r = r.unwrap();
    if let Some(v) = r.structured_content {
        return v;
    }
    let text = r.content[0].as_text().unwrap().text.clone();
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("not JSON ({e}): {text}"))
}

/// The refusals `iris_doc put` and `iris_source_control checkout` give for an answer they cannot
/// act on, by the mode the hook answers with.
const REFUSED: &[(&str, &str)] = &[
    ("code:2", "SCM_NEEDS_UI"),
    ("code:3", "SCM_NEEDS_UI"),
    ("code:4", "SCM_NEEDS_UI"),
    ("code:5", "SCM_NEEDS_UI"),
    ("scode:2", "SCM_NEEDS_UI"),
    ("code:7", "SCM_NEEDS_INPUT"),
    ("code:6", "SCM_REJECTED"),
    ("code:8", "SCM_ACTION_UNKNOWN"),
    ("code:42", "SCM_ACTION_UNKNOWN"),
    ("code:255", "SCM_ACTION_UNKNOWN"),
    ("code:-1", "SCM_ACTION_UNKNOWN"),
    ("scode:42", "SCM_ACTION_UNKNOWN"),
    ("code:1.5", "SCM_ACTION_UNPARSEABLE"),
    ("text", "SCM_ACTION_UNPARSEABLE"),
    ("empty", "SCM_ACTION_UNPARSEABLE"),
    ("error", "SCM_ERROR"),
];

#[tokio::test]
#[ignore = "needs the live IRIS container (iris-dev-iris, or IRIS_CONTAINER)"]
async fn a_put_is_refused_for_every_answer_it_cannot_act_on() {
    let f = Fixture::new().await;
    for (mode, code) in REFUSED {
        f.mode(mode);
        f.forget_probe();
        let b = f.put().await;
        assert_eq!(b["error_code"], *code, "{mode}: {b}");
        assert!(
            !f.probe_exists(),
            "{mode}: the document was written although the hook's answer was refused: {b}"
        );
    }
}

#[tokio::test]
#[ignore = "needs the live IRIS container (iris-dev-iris, or IRIS_CONTAINER)"]
async fn hook_chatter_does_not_hide_the_answer() {
    let f = Fixture::new().await;
    for mode in ["noise", "noise_nonl", "spoof", "code:1", "scode:1"] {
        f.mode(mode);
        f.forget_probe();
        let b = f.put().await;
        assert_eq!(
            b["elicitation_required"], true,
            "{mode}: the hook asked for a dialog and the put went on: {b}"
        );
        assert!(!f.probe_exists(), "{mode}: written without the dialog: {b}");
    }
}

#[tokio::test]
#[ignore = "needs the live IRIS container (iris-dev-iris, or IRIS_CONTAINER)"]
async fn code_zero_lets_the_put_through() {
    let f = Fixture::new().await;
    for mode in ["code:0", "scode:0", "off"] {
        f.mode(mode);
        f.forget_probe();
        let b = f.put().await;
        assert_eq!(b["success"], true, "{mode}: {b}");
        assert!(f.probe_exists(), "{mode}: not written: {b}");
    }
}

#[tokio::test]
#[ignore = "needs the live IRIS container (iris-dev-iris, or IRIS_CONTAINER)"]
async fn checkout_and_execute_refuse_the_same_answers() {
    let f = Fixture::new().await;
    for (mode, code) in REFUSED {
        f.mode(mode);
        let checkout = f
            .scm(json!({"action": "checkout", "document": PROBE_DOC, "namespace": NS}))
            .await;
        assert_eq!(checkout["error_code"], *code, "checkout {mode}: {checkout}");
        // The execute arm drives a text prompt (7), so that one answers differently there.
        if *mode == "code:7" {
            continue;
        }
        let execute = f
            .scm(json!({"action": "execute", "document": PROBE_DOC, "action_id": "%CheckOut", "namespace": NS}))
            .await;
        assert_eq!(execute["error_code"], *code, "execute {mode}: {execute}");
    }
}

#[tokio::test]
#[ignore = "needs the live IRIS container (iris-dev-iris, or IRIS_CONTAINER)"]
async fn checkout_and_execute_read_a_dialog_through_chatter() {
    let f = Fixture::new().await;
    for mode in ["noise", "noise_nonl", "spoof", "code:1"] {
        f.mode(mode);
        let checkout = f
            .scm(json!({"action": "checkout", "document": PROBE_DOC, "namespace": NS}))
            .await;
        assert_eq!(
            checkout["elicitation_required"], true,
            "checkout {mode}: {checkout}"
        );
        let execute = f
            .scm(json!({"action": "execute", "document": PROBE_DOC, "action_id": "%CheckOut", "namespace": NS}))
            .await;
        assert_eq!(
            execute["elicitation_required"], true,
            "execute {mode}: {execute}"
        );
    }
    f.mode("code:7");
    let execute = f
        .scm(json!({"action": "execute", "document": PROBE_DOC, "action_id": "%CheckOut", "namespace": NS}))
        .await;
    assert_eq!(execute["input_type"], "text", "{execute}");
}

#[tokio::test]
#[ignore = "needs the live IRIS container (iris-dev-iris, or IRIS_CONTAINER)"]
async fn a_checkout_is_finished_only_when_after_user_action_says_so() {
    let f = Fixture::new().await;
    f.set_globals("code:0", "fail");
    let b = f
        .scm(json!({"action": "checkout", "document": PROBE_DOC, "namespace": NS}))
        .await;
    assert_eq!(b["error_code"], "SCM_CHECKOUT_FAILED", "{b}");

    // Chatter from a hook that succeeded is not a failure.
    f.set_globals("code:0", "noise");
    let b = f
        .scm(json!({"action": "checkout", "document": PROBE_DOC, "namespace": NS}))
        .await;
    assert_eq!(b["success"], true, "{b}");

    f.set_globals("code:0", "");
    let b = f
        .scm(json!({"action": "checkout", "document": PROBE_DOC, "namespace": NS}))
        .await;
    assert_eq!(b["success"], true, "{b}");
}

/// The dialog's answer finishes the checkout through `AfterUserAction`; if that fails, the write
/// does not happen.
#[tokio::test]
#[ignore = "needs the live IRIS container (iris-dev-iris, or IRIS_CONTAINER)"]
async fn an_approved_dialog_writes_only_when_the_checkout_finishes() {
    let f = Fixture::new().await;
    for (after, expect_written) in [("fail", false), ("noise", true), ("", true)] {
        f.set_globals("noise", after);
        f.forget_probe();
        // A fresh tool set each time: a finished checkout is remembered per set, and the next
        // write would skip the probe and never meet the dialog.
        let tools = IrisTools::for_test_gates_open(Some(conn()), Toolset::Merged).unwrap();
        let put = |extra: Value| {
            let mut args =
                json!({"mode": "put", "name": PROBE_DOC, "namespace": NS, "content": PROBE_BODY});
            args.as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            args
        };
        let first = body(tools.call_gated("iris_doc", put(json!({}))).await);
        let eid = first["elicitation_id"]
            .as_str()
            .unwrap_or_else(|| panic!("after={after:?}: no dialog: {first}"))
            .to_string();
        let resumed = body(
            tools
                .call_gated(
                    "iris_doc",
                    put(json!({"elicitation_id": eid, "elicitation_answer": "yes"})),
                )
                .await,
        );
        if expect_written {
            assert_eq!(resumed["success"], true, "after={after:?}: {resumed}");
        } else {
            assert_eq!(
                resumed["error_code"], "SCM_CHECKOUT_FAILED",
                "after={after:?}: {resumed}"
            );
        }
        assert_eq!(
            f.probe_exists(),
            expect_written,
            "after={after:?}: {resumed}"
        );
    }
}

/// The handler on its own, without the tool pipeline in front: the same refusals.
#[tokio::test]
#[ignore = "needs the live IRIS container (iris-dev-iris, or IRIS_CONTAINER)"]
async fn the_handler_alone_refuses_an_unknown_code() {
    let f = Fixture::new().await;
    f.mode("code:42");
    let p: ScmParams = serde_json::from_value(
        json!({"action": "checkout", "document": PROBE_DOC, "namespace": NS}),
    )
    .unwrap();
    let r = handle_iris_source_control(
        &conn(),
        &f.client,
        p,
        &ElicitationStore::new(),
        &CheckoutCache::new(),
    )
    .await;
    let b = body(r);
    assert_eq!(b["error_code"], "SCM_ACTION_UNKNOWN", "{b}");
}

/// A namespace with a source-control class whose object cannot be made is not a namespace without
/// source control: the put must not go on as if no checkout were needed.
#[tokio::test]
#[ignore = "needs the live IRIS container (iris-dev-iris, or IRIS_CONTAINER)"]
async fn a_hook_that_will_not_start_refuses_the_put() {
    let f = Fixture::new().await;
    f.mode("nonew");
    f.forget_probe();
    let b = f.put().await;
    // Over HTTP the generator cannot even compile once the hook's `Login` fails, so this is
    // `SCM_PROBE_FAILED`; where the probe does run, the hook answers `SCM_UNAVAILABLE`.
    assert!(
        matches!(
            b["error_code"].as_str(),
            Some("SCM_PROBE_FAILED" | "SCM_UNAVAILABLE")
        ),
        "{b}"
    );
    assert!(
        !f.probe_exists(),
        "written although the source-control hook could not start: {b}"
    );
}
