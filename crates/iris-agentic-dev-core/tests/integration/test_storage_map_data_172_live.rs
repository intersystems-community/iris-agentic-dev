//! T083–T089: `compare`, `preview` and the sampler against the real globals the fixtures seed
//! (spec 172 US5). Every expected figure comes from the fixture's seed file.
//!
//! Run: `IRIS_HOST=localhost IRIS_WEB_PORT=52780 cargo test --features testing --test integration \
//!   test_storage_map_data_172 -- --test-threads=1 --include-ignored`

use crate::live_rig_171::{rig, Rig, Where};
use crate::storage172_rig::{drop_fixture, load_fixture, Fixture};
use iris_agentic_dev_core::tools::storage_map::globals::GlobalRef;
use iris_agentic_dev_core::tools::storage_map::sample::{caps, sample};
use iris_agentic_dev_core::tools::storage_map::shape::ShapeReport;
use iris_agentic_dev_core::tools::Toolset;
use serde_json::json;

async fn live_conn() -> (
    iris_agentic_dev_core::iris::connection::IrisConnection,
    reqwest::Client,
) {
    use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};
    let t = iris_agentic_dev_core::testing::LiveTarget::from_env();
    t.assert_running();
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
    (conn, client)
}

fn g(name: &str) -> GlobalRef {
    GlobalRef {
        name: name.into(),
        extended: None,
        subscripts_prefix: None,
    }
}

/// Load fixture `name`, sample `global`, drop the fixture.
async fn sampled(r: &Rig, name: &str, global: &str) -> ShapeReport {
    let f = Fixture::read(name);
    load_fixture(r, &f).await;
    let (conn, client) = live_conn().await;
    let got = sample(&conn, &client, "USER", &g(global), caps(None), false).await;
    let problems = drop_fixture(r, &f).await;
    assert!(problems.is_empty(), "{name}: cleanup: {problems:?}");
    got.unwrap_or_else(|e| panic!("{name}: {e}"))
}

#[tokio::test]
#[ignore = "live IRIS"]
async fn test_storage_map_data_172_sample_member() {
    let r = rig(&Where::live(), Toolset::Merged).await;
    let s = sampled(&r, "Member", "^IadSm172Member").await;
    // Seed: the root holds the counter 3; rows 1-3 hold name^city^age.
    assert_eq!(s.depths.get("0"), Some(&1), "{s:?}");
    assert_eq!(s.depths.get("1"), Some(&3), "{s:?}");
    let row = s.formats.iter().find(|f| f.depth == 1).expect("row format");
    assert_eq!(row.format, "delimited", "{s:?}");
    assert_eq!(row.delimiter.as_deref(), Some("^"), "{s:?}");
    assert_eq!((row.pieces.min, row.pieces.max), (3, 3), "{s:?}");
    // T084: every row node holds a value and nothing below it.
    assert_eq!(s.data_states["1"], [("1".to_string(), 3)].into(), "{s:?}");
    assert!(!s.capped && s.values.is_none(), "{s:?}");
}

#[tokio::test]
#[ignore = "live IRIS"]
async fn test_storage_map_data_172_sample_list_and_sub_nodes() {
    let r = rig(&Where::live(), Toolset::Merged).await;
    let list = sampled(&r, "LoanList", "^IadSm172LoanList").await;
    let row = list.formats.iter().find(|f| f.depth == 1).expect("row");
    assert_eq!(row.format, "$LIST", "{list:?}");
    assert_eq!((row.pieces.min, row.pieces.max), (2, 3), "{list:?}");

    // T084: MemberD11's rows hold a value and one sub-node, "contact".
    let d11 = sampled(&r, "MemberD11", "^IadSm172MemberD").await;
    assert_eq!(
        d11.data_states["1"],
        [("11".to_string(), 2)].into(),
        "{d11:?}"
    );
    assert_eq!(d11.sub_nodes["2"], vec!["contact".to_string()], "{d11:?}");

    // Loan's rows hold a value and an "EV" node that holds only children.
    let loan = sampled(&r, "Loan", "^IadSm172Loan").await;
    assert_eq!(
        loan.data_states["1"],
        [("11".to_string(), 2)].into(),
        "{loan:?}"
    );
    assert_eq!(loan.data_states["2"].get("10"), Some(&2), "{loan:?}");
    assert_eq!(loan.sub_nodes["2"], vec!["EV".to_string()], "{loan:?}");
}

#[tokio::test]
#[ignore = "live IRIS"]
async fn test_storage_map_data_172_sample_stops_at_the_cap() {
    let r = rig(&Where::live(), Toolset::Merged).await;
    let seed = r
        .call(
            "iris_execute",
            json!({"code": "Kill ^IadSm172Big For i=1:1:2000 { Set ^IadSm172Big(i)=\"\" } Write \"OK\"",
                   "namespace": "USER"}),
        )
        .await;
    let (conn, client) = live_conn().await;
    let got = sample(
        &conn,
        &client,
        "USER",
        &g("^IadSm172Big"),
        caps(None),
        false,
    )
    .await;
    let kill = r
        .call(
            "iris_global",
            json!({"action": "kill", "global_name": "^IadSm172Big", "namespace": "USER"}),
        )
        .await;
    assert!(seed["output"].to_string().contains("OK"), "{seed}");
    assert_eq!(kill["success"], true, "{kill}");
    let s = got.expect("sample");
    assert_eq!(s.nodes_scanned, 200, "{s:?}");
    assert!(s.capped, "{s:?}");
    // An empty value is plain, with no pieces.
    let row = s.formats.iter().find(|f| f.depth == 1).expect("row");
    assert_eq!((row.format.as_str(), row.pieces.max), ("plain", 0), "{s:?}");
}

// ── T085, T086: compare ──────────────────────────────────────────────────────

async fn compare(r: &Rig, f: &Fixture) -> serde_json::Value {
    r.call(
        "iris_storage_map",
        json!({"action": "compare", "class": f.class, "namespace": "USER"}),
    )
    .await
}

/// `f` with its class text changed by `edit`, loaded, compared, dropped.
async fn compared_variant(r: &Rig, name: &str, edit: impl Fn(&str) -> String) -> serde_json::Value {
    let mut f = Fixture::read(name);
    f.cls_text = edit(&f.cls_text);
    load_fixture(r, &f).await;
    let got = compare(r, &f).await;
    let problems = drop_fixture(r, &f).await;
    assert!(problems.is_empty(), "{name}: cleanup: {problems:?}");
    got
}

#[tokio::test]
#[ignore = "live IRIS"]
async fn test_storage_map_data_172_compare_finds_a_field_on_the_wrong_piece() {
    let r = rig(&Where::live(), Toolset::Merged).await;
    let matching = compared_variant(&r, "Member", |t| t.to_string()).await;
    // The seed holds three pieces; a map that reads Age from piece 4 reads nothing.
    let moved = compared_variant(&r, "Member", |t| {
        t.replacen("<Piece>3</Piece>", "<Piece>4</Piece>", 1)
    })
    .await;

    assert_eq!(matching["success"], true, "{matching}");
    assert_eq!(matching["mismatches"], json!([]), "{matching}");
    assert_eq!(matching["shape"]["depths"]["1"], 3, "{matching}");
    assert!(matching["identity"]["user"].is_string(), "{matching}");
    assert_eq!(moved["success"], true, "{moved}");
    let m = &moved["mismatches"];
    assert_eq!(m.as_array().map(Vec::len), Some(1), "{moved}");
    assert_eq!(m[0]["path"], "/maps/Master/data/Age", "{moved}");
    assert_eq!(m[0]["expected"], "piece 4", "{moved}");
    assert_eq!(m[0]["seen"], "at most 3 piece(s)", "{moved}");
}

/// Remove `fields` (properties and their Data elements) from MemberD11's class text.
fn without(text: &str, fields: &[&str]) -> String {
    let mut out = text.to_string();
    for f in fields {
        out = out.replacen(&format!("Property {f} As %String;\n\n"), "", 1);
        let start = out
            .find(&format!("<Data name=\"{f}\">"))
            .unwrap_or_else(|| panic!("no Data {f}"));
        let end = out[start..].find("</Data>\n").unwrap() + start + "</Data>\n".len();
        out.replace_range(start..end, "");
    }
    out
}

#[tokio::test]
#[ignore = "live IRIS"]
async fn test_storage_map_data_172_compare_finds_unread_nodes() {
    let r = rig(&Where::live(), Toolset::Merged).await;
    let full = compared_variant(&r, "MemberD11", |t| t.to_string()).await;
    let no_contact = compared_variant(&r, "MemberD11", |t| without(t, &["Email", "Phone"])).await;
    let only_contact = compared_variant(&r, "MemberD11", |t| without(t, &["Name", "City"])).await;

    assert_eq!(full["mismatches"], json!([]), "{full}");
    let m = no_contact["mismatches"].to_string();
    assert!(
        m.contains("row node has children the map does not read"),
        "{no_contact}"
    );
    assert!(m.contains("\\\"contact\\\""), "{no_contact}");
    let m = only_contact["mismatches"].to_string();
    assert!(
        m.contains("the row node holds a value the map does not read"),
        "{only_contact}"
    );
}

// ── T087, T088: preview, and values under the data policy ───────────────────

#[tokio::test]
#[ignore = "live IRIS"]
async fn test_storage_map_data_172_preview_returns_the_known_rows() {
    use crate::live_rig_171::rig_with;
    let r = rig_with(
        &Where::live(),
        Toolset::Merged,
        "write_tools_enabled = true\ndestructive_tools_enabled = true\n\n[policy.default]\ndataPolicy = \"allow\"\n",
    )
    .await;
    let f = Fixture::read("Member");
    load_fixture(&r, &f).await;
    let base = json!({"action": "preview", "class": f.class, "namespace": "USER"});
    let mut with_values = base.clone();
    with_values["include_values"] = json!(true);
    with_values["acknowledgePhi"] = json!(true);
    let shown = r.call("iris_storage_map", with_values.clone()).await;
    let masked = r.call("iris_storage_map", base.clone()).await;
    let mut unacked = with_values.clone();
    unacked["acknowledgePhi"] = json!(false);
    let refused = r.call("iris_storage_map", unacked).await;
    // A proposed map that swaps City and Age reads the age as the city. (Two fields on one piece
    // are refused by IRIS, #5001 "Duplicate element".)
    let read = r
        .call(
            "iris_storage_map",
            json!({"action": "read", "class": f.class, "namespace": "USER"}),
        )
        .await;
    let mut wrong = read["storage"]["maps"][0].clone();
    wrong["data"]["Age"]["piece"] = json!("2");
    wrong["data"]["City"]["piece"] = json!("3");
    let mut proposed = with_values.clone();
    proposed["model"] = wrong;
    let through_proposed = r.call("iris_storage_map", proposed).await;
    let left = r
        .call(
            "iris_query",
            json!({"query": "SELECT COUNT(*) AS n FROM %Dictionary.ClassDefinition WHERE Name %STARTSWITH 'IrisDevTmp.Sm'",
                   "namespace": "USER"}),
        )
        .await;
    let problems = drop_fixture(&r, &f).await;

    assert_eq!(shown["success"], true, "{shown}");
    assert_eq!(shown["row_count"], 3, "{shown}");
    let rows = shown["rows"].as_array().unwrap();
    let alice = rows
        .iter()
        .find(|row| row["Name"] == "Alice")
        .unwrap_or_else(|| panic!("no Alice row: {shown}"));
    assert_eq!(alice["City"], "Portland", "{shown}");
    assert_eq!(alice["Age"], 35, "{shown}");
    assert!(shown["identity"]["user"].is_string(), "{shown}");

    assert_eq!(masked["success"], true, "{masked}");
    assert_eq!(masked["row_count"], 3, "{masked}");
    assert_eq!(masked["values_masked"], true, "{masked}");
    assert!(!masked["rows"].to_string().contains("Alice"), "{masked}");
    assert_eq!(masked["rows"][0]["Name"], "<masked>", "{masked}");

    assert_eq!(refused["error_code"], "PHI_ACK_REQUIRED", "{refused}");
    assert_eq!(
        refused["refusal"]["kind"], "acknowledgement_required",
        "{refused}"
    );

    assert_eq!(through_proposed["success"], true, "{through_proposed}");
    let swapped = through_proposed["rows"]
        .as_array()
        .and_then(|rows| rows.iter().find(|row| row["Name"] == "Alice"))
        .unwrap_or_else(|| panic!("no Alice row: {through_proposed}"));
    assert_eq!(swapped["City"], "35", "{through_proposed}");
    assert_eq!(left["rows"][0]["n"], 0, "a scratch class stayed: {left}");
    assert!(problems.is_empty(), "cleanup: {problems:?}");
}

#[tokio::test]
#[ignore = "live IRIS"]
async fn test_storage_map_data_172_values_are_refused_under_block() {
    use crate::live_rig_171::rig_with;
    let r = rig_with(
        &Where::live(),
        Toolset::Merged,
        "write_tools_enabled = true\ndestructive_tools_enabled = true\n\n[policy.default]\ndataPolicy = \"block\"\n",
    )
    .await;
    let f = Fixture::read("Member");
    load_fixture(&r, &f).await;
    let mut got = Vec::new();
    for action in ["compare", "preview"] {
        got.push(
            r.call(
                "iris_storage_map",
                json!({"action": action, "class": f.class, "namespace": "USER",
                       "include_values": true, "acknowledgePhi": true}),
            )
            .await,
        );
    }
    let shape_only = r
        .call(
            "iris_storage_map",
            json!({"action": "compare", "class": f.class, "namespace": "USER"}),
        )
        .await;
    let problems = drop_fixture(&r, &f).await;
    for g in &got {
        assert_eq!(g["error_code"], "DATA_POLICY_BLOCKED", "{g}");
        assert_eq!(g["refusal"]["kind"], "operator_limit", "{g}");
    }
    assert_eq!(
        shape_only["success"], true,
        "the shape needs no values: {shape_only}"
    );
    assert!(problems.is_empty(), "cleanup: {problems:?}");
}

// ── T091: the damage FR-023 refuses, reproduced (research R5) ──────────────────

/// With rows 1..3 and no counter node, the first INSERT through a `$i` map gets ID 1 and
/// overwrites row 1, without an error.
#[tokio::test]
#[ignore = "live IRIS"]
async fn test_storage_map_data_172_counterless_insert_overwrites_row_1() {
    let r = rig(&Where::live(), Toolset::Merged).await;
    let f = Fixture::read("Member");
    load_fixture(&r, &f).await;
    // Remove only the root node, the counter `$i(^IadSm172Member)` reads.
    let zkill = r
        .call(
            "iris_execute",
            json!({"code": "ZKill ^IadSm172Member Write $Data(^IadSm172Member)", "namespace": "USER"}),
        )
        .await;
    let insert = r
        .call(
            "iris_query",
            json!({"query": "INSERT INTO IadSm172.Member (Name, City, Age) VALUES ('Zed', 'Nowhere', 1)",
                   "mode": "write", "namespace": "USER"}),
        )
        .await;
    let row1 = r
        .call(
            "iris_query",
            json!({"query": "SELECT Name FROM IadSm172.Member WHERE ID = 1", "namespace": "USER"}),
        )
        .await;
    let problems = drop_fixture(&r, &f).await;
    assert_eq!(
        zkill["output"].as_str().map(str::trim),
        Some("10"),
        "{zkill}"
    );
    assert_eq!(insert["success"], true, "{insert}");
    assert_eq!(
        row1["rows"][0]["Name"], "Zed",
        "row 1 was not overwritten: {row1}"
    );
    assert!(problems.is_empty(), "cleanup: {problems:?}");
}

/// An index map over data whose index global is empty answers nothing for a value that is there.
#[tokio::test]
#[ignore = "live IRIS"]
async fn test_storage_map_data_172_unbuilt_index_finds_nothing() {
    let r = rig(&Where::live(), Toolset::Merged).await;
    let f = Fixture::read("MemberByCity");
    load_fixture(&r, &f).await;
    let kill = r
        .call(
            "iris_global",
            json!({"action": "kill", "global_name": "^IadSm172MemberByCityI", "namespace": "USER"}),
        )
        .await;
    let by_index = r
        .call(
            "iris_query",
            json!({"query": "SELECT Name FROM IadSm172.MemberByCity WHERE City = 'Portland'",
                   "namespace": "USER"}),
        )
        .await;
    let by_scan = r
        .call(
            "iris_query",
            json!({"query": "SELECT Name FROM IadSm172.MemberByCity WHERE %NOINDEX City = 'Portland'",
                   "namespace": "USER"}),
        )
        .await;
    let problems = drop_fixture(&r, &f).await;
    assert_eq!(kill["success"], true, "{kill}");
    assert_eq!(
        by_index["rows"],
        json!([]),
        "the empty index still found rows: {by_index}"
    );
    assert!(
        by_scan["rows"].as_array().is_some_and(|r| !r.is_empty()),
        "the rows are there: {by_scan}"
    );
    assert!(problems.is_empty(), "cleanup: {problems:?}");
}
