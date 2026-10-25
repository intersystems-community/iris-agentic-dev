//! Spec 141 T201, live: a scratch class IRIS would not delete is reported and not forgotten.
//!
//! On the throwaway container a source-control class refuses every delete (`OnBeforeDelete`). The
//! executor's scratch class (`IrisDevTmp.*`) then cannot be removed, and IRIS says so the way it
//! does for a checked-out document: HTTP 200 and no error status. This container puts the reason in
//! `result.status` and leaves `status.errors` empty (a locked document uses `status.errors`). Before
//! 141 the cleanup read only the HTTP status, so the class stayed and nobody was told.
//!
//! - the call's own result stands, and carries a `warnings` entry naming the class;
//! - the raw DELETE reply is a 200 carrying the reason, which `delete_outcome` reads as a refusal;
//! - a DELETE of a document that is not there answers 404, which it reads as gone;
//! - with the refusing class removed, the leftover class can be deleted and the next call is clean.
//!
//! Run, serialised behind the shared lock:
//!
//! ```text
//! lockf -k /tmp/iad-141-iris.lock cargo test --features testing --test integration \
//!     scratch_cleanup_141 -- --ignored --test-threads=1
//! ```

use iris_agentic_dev_core::iris::connection::{
    delete_outcome, DeleteOutcome, DiscoverySource, IrisConnection,
};
use iris_agentic_dev_core::testing::ThrowawayIris;
use iris_agentic_dev_core::tools::{IrisTools, Toolset};
use serde_json::{json, Value};

/// A source-control class that lets everything through except a delete.
const REFUSING_SC: &[&str] = &[
    "Class Iad.ScratchSC141 Extends %Studio.Extension.Base",
    "{",
    "Method GetStatus(InternalName As %String, ByRef IsInSourceControl As %Boolean, ByRef Editable As %Boolean, ByRef IsCheckedOut As %Boolean, ByRef UserCheckedOut As %String) As %Status",
    "{",
    "  Set IsInSourceControl = 1, Editable = 1, IsCheckedOut = 1, UserCheckedOut = ..Username",
    "  Quit $$$OK",
    "}",
    "Method OnBeforeDelete(InternalName As %String) As %Status",
    "{",
    "  Quit $$$ERROR($$$GeneralError, \"ScratchSC141 refuses to delete \"_InternalName)",
    "}",
    "}",
];

fn url(iris: &ThrowawayIris) -> String {
    format!("http://{}:{}", iris.host, iris.web_port)
}

fn conn(iris: &ThrowawayIris) -> IrisConnection {
    IrisConnection::new(
        url(iris),
        "USER",
        iris.username,
        iris.password,
        DiscoverySource::EnvVar,
    )
}

/// Names of the executor's scratch classes now in USER, one per line.
fn scratch_classes(iris: &ThrowawayIris) -> Vec<String> {
    let out = iris.session(
        "USER",
        "Set rs=##class(%SQL.Statement).%ExecDirect(,\"SELECT Name FROM %Dictionary.ClassDefinition WHERE Name %STARTSWITH 'IrisDevTmp.'\")\nWhile rs.%Next() { Write \"SCRATCH|\"_rs.%Get(\"Name\"),! }",
    );
    out.lines()
        .filter_map(|l| l.strip_prefix("SCRATCH|"))
        .map(|l| l.trim().to_string())
        .collect()
}

async fn install_refusing_sc(iris: &ThrowawayIris, client: &reqwest::Client) {
    let put = client
        .put(format!(
            "{}/api/atelier/v1/USER/doc/Iad.ScratchSC141.cls",
            url(iris)
        ))
        .basic_auth(iris.username, Some(iris.password))
        .json(&json!({"enc": false, "content": REFUSING_SC}))
        .send()
        .await
        .expect("PUT stub");
    assert!(put.status().is_success(), "PUT stub: {}", put.status());
    let compile = client
        .post(format!(
            "{}/api/atelier/v1/USER/action/compile?flags=cuk",
            url(iris)
        ))
        .basic_auth(iris.username, Some(iris.password))
        .json(&json!(["Iad.ScratchSC141.cls"]))
        .send()
        .await
        .expect("compile stub");
    let result: Value = compile.json().await.unwrap();
    assert!(
        result["status"]["errors"]
            .as_array()
            .is_some_and(|e| e.is_empty()),
        "stub did not compile: {result}"
    );
    set_sc(iris, "Iad.ScratchSC141");
}

fn set_sc(iris: &ThrowawayIris, class: &str) {
    let out = iris.session(
        "USER",
        &format!(
            "Write \"SET:\",##class(%Studio.SourceControl.Interface).SourceControlClassSet(\"{class}\",\"USER\"),!"
        ),
    );
    assert!(
        out.contains("SET:1"),
        "source-control class not set to {class:?}: {out}"
    );
}

async fn delete_raw(
    iris: &ThrowawayIris,
    client: &reqwest::Client,
    doc: &str,
) -> (reqwest::StatusCode, String) {
    let resp = client
        .delete(format!("{}/api/atelier/v1/USER/doc/{doc}", url(iris)))
        .basic_auth(iris.username, Some(iris.password))
        .send()
        .await
        .expect("DELETE");
    let status = resp.status();
    (status, resp.text().await.unwrap_or_default())
}

#[tokio::test]
#[ignore = "live: starts the throwaway IRIS container"]
async fn a_scratch_class_iris_would_not_delete_is_reported_and_not_forgotten() {
    let Some(iris) = ThrowawayIris::start() else {
        return;
    };
    let client = IrisConnection::http_client().unwrap();
    assert!(
        scratch_classes(&iris).is_empty(),
        "a fresh container holds no scratch class"
    );
    install_refusing_sc(&iris, &client).await;

    let tools = IrisTools::for_test_gates_open(Some(conn(&iris)), Toolset::Merged).expect("tools");
    let refused_run = tools
        .call_for_test(
            "iris_execute",
            json!({"code": "Write 42", "namespace": "USER"}),
        )
        .await
        .expect("iris_execute answers");
    let body = refused_run
        .structured_content
        .clone()
        .unwrap_or(Value::Null);
    let left = scratch_classes(&iris);

    // The raw replies, from the same IRIS.
    let leftover = left.first().cloned().unwrap_or_default();
    let (refused_status, refused_body) =
        delete_raw(&iris, &client, &format!("{leftover}.cls")).await;
    let (missing_status, missing_body) =
        delete_raw(&iris, &client, "IrisDevTmp.NoSuchClass141.cls").await;

    // Lift the refusal: the leftover can be deleted, and the next call is clean.
    set_sc(&iris, "");
    let (lifted_status, lifted_body) = delete_raw(&iris, &client, &format!("{leftover}.cls")).await;
    let clean_run = tools
        .call_for_test(
            "iris_execute",
            json!({"code": "Write 43", "namespace": "USER"}),
        )
        .await
        .expect("iris_execute answers");
    let clean = clean_run.structured_content.clone().unwrap_or(Value::Null);
    let left_after = scratch_classes(&iris);

    // IRIS's real reply to the refused delete, and to a delete of nothing. Asserted first: the rest
    // reads them.
    assert_eq!(
        left.len(),
        1,
        "exactly the one scratch class stays: {left:?}"
    );
    assert_eq!(
        refused_status, 200,
        "refused delete: HTTP {refused_status} {refused_body}"
    );
    assert!(
        matches!(delete_outcome(refused_status, &refused_body), DeleteOutcome::Refused(ref m) if m.contains("refuses to delete")),
        "a 200 that carries the reason is a refusal: {refused_body}"
    );
    assert_eq!(missing_status, 404, "{missing_body}");
    assert_eq!(
        delete_outcome(missing_status, &missing_body),
        DeleteOutcome::Gone
    );

    // The call's own result stands, the class stayed, and the answer says so.
    assert_ne!(refused_run.is_error, Some(true), "{body}");
    assert!(
        body.to_string().contains("42"),
        "the output must come back: {body}"
    );
    let warnings = body["warnings"]
        .as_array()
        .unwrap_or_else(|| panic!("a class that stays must be a warning on the response: {body}"));
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    let warning = warnings[0].as_str().unwrap();
    assert!(
        warning.contains(&leftover) && warning.contains("refuses to delete"),
        "the warning names the class and IRIS's reason: {warning}"
    );

    // With the refusal lifted the delete works, and a normal call leaves no warning.
    assert_eq!(
        delete_outcome(lifted_status, &lifted_body),
        DeleteOutcome::Gone,
        "{lifted_body}"
    );
    assert!(
        clean.get("warnings").is_none(),
        "no warning when nothing stays: {clean}"
    );
    assert!(left_after.is_empty(), "nothing stays now: {left_after:?}");
}

/// `iris_doc delete` reads the same reply. Before, it looked only at `status.errors`, so a delete
/// that source control refused in `OnBeforeDelete` came back `success: true`, single or batch,
/// while the document stayed.
#[tokio::test]
#[ignore = "live: starts the throwaway IRIS container"]
async fn iris_doc_delete_reports_a_delete_source_control_refused() {
    let Some(iris) = ThrowawayIris::start() else {
        return;
    };
    let client = IrisConnection::http_client().unwrap();
    let tools = IrisTools::for_test_gates_open(Some(conn(&iris)), Toolset::Merged).expect("tools");
    let doc = "Iad.DeleteRefused141.cls";
    let put = tools
        .call_for_test(
            "iris_doc",
            json!({"mode": "put", "name": doc, "namespace": "USER",
                   "content": "Class Iad.DeleteRefused141\n{\n}\n"}),
        )
        .await
        .expect("put answers");
    install_refusing_sc(&iris, &client).await;

    let single = tools
        .call_for_test(
            "iris_doc",
            json!({"mode": "delete", "name": doc, "namespace": "USER"}),
        )
        .await
        .expect("delete answers");
    let single = single.structured_content.clone().unwrap_or(Value::Null);
    let batch = tools
        .call_for_test(
            "iris_doc",
            json!({"mode": "delete", "names": [doc], "namespace": "USER"}),
        )
        .await
        .expect("batch delete answers");
    let batch = batch.structured_content.clone().unwrap_or(Value::Null);
    set_sc(&iris, "");
    let (still_status, _) = delete_raw(&iris, &client, doc).await;

    assert_ne!(
        put.is_error,
        Some(true),
        "put: {:?}",
        put.structured_content
    );
    assert_eq!(single["error_code"], "DELETE_FAILED", "{single}");
    assert!(
        single.to_string().contains("refuses to delete"),
        "the refusal carries IRIS's reason: {single}"
    );
    assert_eq!(batch["success"], false, "{batch}");
    assert_eq!(batch["deleted"], json!([]), "{batch}");
    assert_eq!(
        still_status, 200,
        "the document was still there once the refusal was lifted"
    );
}
