//! T026: D11 behavior tests for spec 172.
//!
//! Three tests that measure what IRIS actually does with D11 (nested) storage maps:
//! (a) DELETE one Loan row: are its "EV" children removed? (yes: the whole subtree goes)
//! (b) A map that walks level 2 of ^IadSm172Loan as row IDs: does it return the "EV" node as a row? (yes)
//! (c) UPDATE one MemberD11 field: are the contact fields kept? (yes)

use crate::live_rig_171::{rig, Where};
use crate::storage172_rig::{drop_fixture, load_fixture, Fixture};
use iris_agentic_dev_core::tools::Toolset;
use serde_json::json;

/// (a) DELETE one Loan row through SQL: are its "EV" children removed?
#[tokio::test]
#[ignore]
async fn d11_delete_of_a_parent_row_kills_its_child_rows() {
    let r = rig(&Where::live(), Toolset::Merged).await;
    let loan = Fixture::read("Loan");
    load_fixture(&r, &loan).await;
    let data = |sub: &'static str| {
        let r = &r;
        async move {
            let out = r
                .call(
                    "iris_execute",
                    json!({"code": format!("Write $Data(^IadSm172Loan({sub}))"), "namespace": "USER"}),
                )
                .await;
            out["output"].as_str().unwrap_or("?").trim().to_string()
        }
    };
    let row_before = data("1").await;
    let child_before = data("1,\"EV\",1").await;
    let del = r
        .call(
            "iris_query",
            json!({"query": "DELETE FROM IadSm172.Loan WHERE ID = 1", "mode": "write",
                "namespace": "USER"}),
        )
        .await;
    let row_after = data("1").await;
    let child_after = data("1,\"EV\",1").await;
    let problems = drop_fixture(&r, &loan).await;
    println!(
        "OBSERVED (a): $Data(^IadSm172Loan(1)) {row_before} -> {row_after}; \
         $Data(^IadSm172Loan(1,\"EV\",1)) {child_before} -> {child_after}"
    );
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(del["success"], true, "{del}");
    assert_eq!(
        (row_before.as_str(), child_before.as_str()),
        ("11", "1"),
        "fixture not nested"
    );
    // Measured on IRIS 2026.2: the filer kills the parent row's whole subtree, so a SQL DELETE of
    // one Loan also deletes its LoanEvent rows (another table), with no error or warning.
    assert_eq!((row_after.as_str(), child_after.as_str()), ("0", "0"));
}

/// (b) A map whose level 2 is an ordinary subscript walks every level-2 node, so the parent's
/// literal "EV" node comes back as a row.
#[tokio::test]
#[ignore]
async fn d11_a_level2_walk_returns_the_literal_node_as_a_row() {
    let r = rig(&Where::live(), Toolset::Merged).await;
    let loan = Fixture::read("Loan");
    let walk = r#"Class IadSm172Test.LoanWalk Extends %Persistent [ StorageStrategy = SQLStorage ]
{

Property LoanNo As %Integer;

Property Part As %String;

Index IDKEY On (LoanNo, Part) [ IdKey, Unique ];

Storage SQLStorage
{
<SQLMap name="Master">
<Global>^IadSm172Loan</Global>
<Subscript name="1">
<Expression>{LoanNo}</Expression>
</Subscript>
<Subscript name="2">
<Expression>{Part}</Expression>
</Subscript>
<Type>data</Type>
</SQLMap>
<StreamLocation>^IadSm172LoanWalkS</StreamLocation>
<Type>%Storage.SQL</Type>
}

}
"#;
    load_fixture(&r, &loan).await;
    // This measures IRIS, not iad: iad now refuses this map (FR-023b, it reads "EV" as a row), so
    // the class goes in through Atelier directly.
    let put = raw_put_and_compile("IadSm172Test.LoanWalk.cls", walk).await;
    let q = r
        .call(
            "iris_query",
            json!({"query": "SELECT LoanNo, Part FROM IadSm172Test.LoanWalk ORDER BY LoanNo, Part",
                "namespace": "USER"}),
        )
        .await;
    let del = r
        .call(
            "iris_doc",
            json!({"mode": "delete", "name": "IadSm172Test.LoanWalk.cls", "namespace": "USER"}),
        )
        .await;
    let problems = drop_fixture(&r, &loan).await;
    println!("OBSERVED (b): {}", q["rows"]);
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(put["success"], true, "{put}");
    assert_eq!(del["success"], true, "{del}");
    assert_eq!(q["success"], true, "{q}");
    let rows = q["rows"].as_array().cloned().unwrap_or_default();
    assert!(
        rows.iter().any(|row| row["Part"] == "EV"),
        "expected the literal EV node as a row: {rows:?}"
    );
}

#[tokio::test]
#[ignore]
async fn d11_update_preserves_other_fields() {
    let r = rig(&Where::live(), Toolset::Merged).await;
    let member_d11 = Fixture::read("MemberD11");

    load_fixture(&r, &member_d11).await;

    // Read original values
    let before = r
        .call(
            "iris_query",
            json!({
                "query": "SELECT Name, City, Email, Phone FROM IadSm172.MemberD11 WHERE ID = 1",
                "namespace": "USER"
            }),
        )
        .await;
    assert_eq!(before["success"], true, "Query before failed: {}", before);
    let before_row = &before["rows"][0];
    println!("Before UPDATE: {:?}", before_row);

    // UPDATE only the Name field
    let upd = r
        .call(
            "iris_query",
            json!({
                "query": "UPDATE IadSm172.MemberD11 SET Name = 'Updated Name' WHERE ID = 1",
                "mode": "write",
                "namespace": "USER"
            }),
        )
        .await;
    assert_eq!(upd["success"], true, "UPDATE failed: {}", upd);

    // Read after update
    let after = r
        .call(
            "iris_query",
            json!({
                "query": "SELECT Name, City, Email, Phone FROM IadSm172.MemberD11 WHERE ID = 1",
                "namespace": "USER"
            }),
        )
        .await;
    assert_eq!(after["success"], true, "Query after failed: {}", after);
    let after_row = &after["rows"][0];
    println!("After UPDATE: {:?}", after_row);

    let email_kept = before_row["Email"] == after_row["Email"];
    let phone_kept = before_row["Phone"] == after_row["Phone"];
    let city_kept = before_row["City"] == after_row["City"];

    println!(
        "OBSERVED (c): UPDATE Name field: Email kept={}, Phone kept={}, City kept={}",
        email_kept, phone_kept, city_kept
    );

    let problems = drop_fixture(&r, &member_d11).await;
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(after_row["Name"], "Updated Name", "{after_row}");
    // Measured on IRIS 2026.2: updating a row-node field keeps the fields on the "contact" node.
    assert!(
        email_kept && phone_kept && city_kept,
        "{before_row} -> {after_row}"
    );
}

/// PUT and compile `doc` through Atelier with the connection's own client, bypassing iad's tools.
async fn raw_put_and_compile(doc: &str, text: &str) -> serde_json::Value {
    use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};
    let t = iris_agentic_dev_core::testing::LiveTarget::from_env();
    let mut conn = IrisConnection::new(
        t.base_url(),
        &t.namespace,
        &t.username,
        &t.password,
        DiscoverySource::ExplicitFlag,
    );
    conn.probe().await;
    let client = conn
        .client(iris_agentic_dev_core::iris::http::ClientKind::Call)
        .expect("client");
    let url = conn.versioned_ns_url(
        "USER",
        &format!("/doc/{}?ignoreConflict=1", urlencoding::encode(doc)),
    );
    let lines: Vec<&str> = text.lines().collect();
    let put = client
        .put(&url)
        .basic_auth(&conn.username, Some(&conn.password))
        .json(&serde_json::json!({"enc": false, "content": lines}))
        .send()
        .await
        .expect("PUT");
    if !put.status().is_success() {
        return serde_json::json!({"success": false, "status": put.status().as_u16()});
    }
    let compiled = conn
        .compile_document(doc, "USER", "cuk", &client)
        .await
        .expect("compile");
    serde_json::json!({"success": compiled.success(), "errors": compiled.errors})
}
