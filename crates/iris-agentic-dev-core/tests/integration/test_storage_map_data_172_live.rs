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

// ── T092–T094: the data checks a write runs, and the opt-ins ──────────────────

const LOAN_WALK: &str = r#"Class IadSm172Test.LoanWalk Extends %Persistent [ StorageStrategy = SQLStorage ]
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
SKIP</Subscript>
<Type>data</Type>
</SQLMap>
<StreamLocation>^IadSm172LoanWalkS</StreamLocation>
<Type>%Storage.SQL</Type>
}

}
"#;

/// T092 (FR-023b): a map that walks `Loan`'s level 2 as row IDs would read its `"EV"` node as a
/// row (T026 b), so the write is refused, naming the level and the node; with a NextCode that
/// skips non-numeric subscripts it goes through.
#[tokio::test]
#[ignore = "live IRIS"]
async fn test_storage_map_data_172_a_walk_over_named_nodes_is_refused() {
    let r = rig(&Where::live(), Toolset::Merged).await;
    let loan = Fixture::read("Loan");
    load_fixture(&r, &loan).await;
    let put = |text: String| {
        let r = &r;
        async move {
            r.call(
                "iris_doc",
                json!({"mode": "put", "name": "IadSm172Test.LoanWalk.cls", "content": text,
                       "compile": true, "namespace": "USER"}),
            )
            .await
        }
    };
    let walking = put(LOAN_WALK.replace("SKIP", "")).await;
    let skip = "<NextCode> s {L2}=$o(^IadSm172Loan({L1},{L2}))\n f  {\n\tq:{L2}=\"\"\n\tq:{L2}=+{L2}\n\ts {L2}=$o(^IadSm172Loan({L1},{L2}))\n }</NextCode>\n";
    let skipping = put(LOAN_WALK.replace("SKIP", skip)).await;
    let del = r
        .call(
            "iris_doc",
            json!({"mode": "delete", "name": "IadSm172Test.LoanWalk.cls", "namespace": "USER"}),
        )
        .await;
    let problems = drop_fixture(&r, &loan).await;

    assert_eq!(walking["error_code"], "STORAGE_MAP_INVALID", "{walking}");
    assert_eq!(
        walking["rules"][0]["rule"], "walks_literal_nodes",
        "{walking}"
    );
    assert_eq!(walking["level"], 2, "{walking}");
    assert_eq!(walking["nodes"], json!(["EV"]), "{walking}");
    assert_eq!(skipping["success"], true, "{skipping}");
    assert_eq!(del["success"], true, "{del}");
    assert!(problems.is_empty(), "cleanup: {problems:?}");
}

/// T093/T094: a counter behind the highest ID is refused; `seed_counter` sets it and reports the
/// value it replaced, and the next INSERT does not overwrite row 1.
#[tokio::test]
#[ignore = "live IRIS"]
async fn test_storage_map_data_172_id_reuse_and_seed_counter() {
    let r = rig(&Where::live(), Toolset::Merged).await;
    let f = Fixture::read("Member");
    load_fixture(&r, &f).await;
    r.call(
        "iris_execute",
        json!({"code": "ZKill ^IadSm172Member Write 1", "namespace": "USER"}),
    )
    .await;
    // Setting the ID expression is when the check runs (a counter that was already behind is
    // not an unrelated edit's doing).
    let edit = |seed: bool, version: serde_json::Value| {
        json!({"action": "edit", "class": f.class, "version": version, "namespace": "USER",
               "seed_counter": seed,
               "ops": [{"op": "set", "path": "/keywords/SqlIdExpression",
                        "value": "$INCREMENT(^IadSm172Member)"}]})
    };
    let read = r
        .call(
            "iris_storage_map",
            json!({"action": "read", "class": f.class, "namespace": "USER"}),
        )
        .await;
    let version = read["storage"]["version"].clone();
    let refused = r
        .call("iris_storage_map", edit(false, version.clone()))
        .await;
    let seeded = r.call("iris_storage_map", edit(true, version)).await;
    let insert = r
        .call(
            "iris_query",
            json!({"query": "INSERT INTO IadSm172.Member (Name, City, Age) VALUES ('Zed', 'Nowhere', 1)",
                   "mode": "write", "namespace": "USER"}),
        )
        .await;
    let rows = r
        .call(
            "iris_query",
            json!({"query": "SELECT ID, Name FROM IadSm172.Member ORDER BY ID", "namespace": "USER"}),
        )
        .await;
    let problems = drop_fixture(&r, &f).await;

    assert_eq!(refused["error_code"], "STORAGE_MAP_ID_REUSE", "{refused}");
    assert_eq!(refused["highest_id"], "3", "{refused}");
    assert_eq!(refused["counter"], "^IadSm172Member", "{refused}");
    assert_eq!(seeded["success"], true, "{seeded}");
    assert_eq!(
        seeded["opt_ins_run"]["seed_counter"]["set_to"], "3",
        "{seeded}"
    );
    assert_eq!(
        seeded["opt_ins_run"]["seed_counter"]["replaced"], "",
        "{seeded}"
    );
    assert_eq!(insert["success"], true, "{insert}");
    let names: Vec<(i64, String)> = rows["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            (
                r["ID"].as_i64().unwrap_or(0),
                r["Name"].as_str().unwrap_or("").to_string(),
            )
        })
        .collect();
    assert_eq!(
        names[0],
        (1, "Alice".to_string()),
        "row 1 was overwritten: {rows}"
    );
    assert_eq!(names.last().unwrap(), &(4, "Zed".to_string()), "{rows}");
    assert!(problems.is_empty(), "cleanup: {problems:?}");
}

async fn genre_storage(r: &Rig) -> serde_json::Value {
    r.call(
        "iris_storage_map",
        json!({"action": "read", "class": "IadSm172.Genre", "namespace": "USER"}),
    )
    .await["storage"]
        .clone()
}

/// T093/T094: a new index map over existing data with an empty index is refused; with
/// `build_indexes` it is built and the known question answers; a build over an index that already
/// holds nodes is refused (FR-023a). With the destructive tier off the opt-in is refused: that is
/// `binary::test_storage_map_gates_172`, because this process's launch env may hold the tier open.
#[tokio::test]
#[ignore = "live IRIS"]
async fn test_storage_map_data_172_unbuilt_index_and_build_indexes() {
    use crate::storage172_rig::ask;
    let r = rig(&Where::live(), Toolset::Merged).await;
    let f = Fixture::read("Genre");
    load_fixture(&r, &f).await;
    let s0 = genre_storage(&r).await;
    let bitmap = s0["maps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["name"] == "PopularBitmap")
        .cloned()
        .expect("the bitmap map");
    let removed = r
        .call(
            "iris_storage_map",
            json!({"action": "edit", "class": f.class, "version": bitmap["version"], "namespace": "USER",
                   "ops": [{"op": "remove", "path": "/maps/PopularBitmap"}]}),
        )
        .await;
    r.call(
        "iris_global",
        json!({"action": "kill", "global_name": "^IadSm172GenreBit", "namespace": "USER"}),
    )
    .await;
    let add = |build: bool, version: serde_json::Value| {
        json!({"action": "edit", "class": "IadSm172.Genre", "version": version, "namespace": "USER",
               "build_indexes": build,
               "ops": [{"op": "add", "path": "/maps/PopularBitmap",
                        "value": {"type": "bitmap", "global": "^IadSm172GenreBit",
                                  "subscripts": [{"level": 1, "expression": "{Popular}"},
                                                 {"level": 2, "expression": "{ID}"}]}}]})
    };
    let v1 = genre_storage(&r).await["version"].clone();
    let unbuilt = r.call("iris_storage_map", add(false, v1.clone())).await;
    let built = r.call("iris_storage_map", add(true, v1)).await;
    let rows = ask(&r, &f).await;
    // The index is built now; take the map out again and try a build over the filled range.
    let v2 = genre_storage(&r).await["maps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["name"] == "PopularBitmap")
        .map(|m| m["version"].clone())
        .unwrap_or_default();
    r.call(
        "iris_storage_map",
        json!({"action": "edit", "class": f.class, "version": v2, "namespace": "USER",
               "ops": [{"op": "remove", "path": "/maps/PopularBitmap"}]}),
    )
    .await;
    let v3 = genre_storage(&r).await["version"].clone();
    let over_filled = r.call("iris_storage_map", add(true, v3)).await;
    let problems = drop_fixture(&r, &f).await;

    assert_eq!(removed["success"], true, "{removed}");
    assert_eq!(
        unbuilt["error_code"], "STORAGE_MAP_INDEX_UNBUILT",
        "{unbuilt}"
    );
    assert_eq!(built["success"], true, "{built}");
    assert_eq!(
        built["opt_ins_run"]["build_indexes"]["built"],
        json!(["PopularBitmap"]),
        "{built}"
    );
    let expected = f.seed["answer"].as_array().cloned().unwrap_or_default();
    assert_eq!(rows, expected, "the built index does not answer");
    assert_eq!(
        over_filled["rules"][0]["rule"], "index_range_in_use",
        "{over_filled}"
    );
    assert!(problems.is_empty(), "cleanup: {problems:?}");
}

/// FR-023: a new index map whose leading subscripts are constants is checked over that range
/// only, the constants re-quoted by iad: the empty range `^IadSm172GenreIdx(1,"Pop")` under a
/// table with rows is an unbuilt index, and nothing is written.
#[tokio::test]
#[ignore = "live IRIS"]
async fn test_storage_map_data_172_index_range_under_constant_subscripts() {
    let r = rig(&Where::live(), Toolset::Merged).await;
    let f = Fixture::read("Genre");
    load_fixture(&r, &f).await;
    let kill = || async {
        r.call(
            "iris_global",
            json!({"action": "kill", "global_name": "^IadSm172GenreIdx", "namespace": "USER"}),
        )
        .await
    };
    kill().await;
    let before = genre_storage(&r).await["version"].clone();
    let got = r
        .call(
            "iris_storage_map",
            json!({"action": "edit", "class": f.class, "version": before, "namespace": "USER",
                   "ops": [{"op": "add", "path": "/maps/PopularConst",
                            "value": {"type": "bitmap", "global": "^IadSm172GenreIdx",
                                      "subscripts": [{"level": 1, "expression": "1"},
                                                     {"level": 2, "expression": "\"Pop\""},
                                                     {"level": 3, "expression": "{Popular}"},
                                                     {"level": 4, "expression": "{ID}"}]}}]}),
        )
        .await;
    let after = genre_storage(&r).await["version"].clone();
    kill().await;
    let problems = drop_fixture(&r, &f).await;

    assert_eq!(got["error_code"], "STORAGE_MAP_INDEX_UNBUILT", "{got}");
    assert!(
        got["error"]
            .as_str()
            .unwrap_or("")
            .contains("^IadSm172GenreIdx(1,\"Pop\") is empty"),
        "{got}"
    );
    assert_eq!(after, before, "the refused edit changed the class");
    assert!(problems.is_empty(), "cleanup: {problems:?}");
}

// ── T111: propose ────────────────────────────────────────────────────────────

async fn member_crc(r: &Rig) -> serde_json::Value {
    r.call(
        "iris_execute",
        json!({"code": "Set crc=0,n=\"^IadSm172Member\" For { Set n=$Query(@n) Quit:n=\"\"  Set crc=$ZCRC(n_@n,7,crc) } Write crc",
               "namespace": "USER"}),
    )
    .await["output"]
        .clone()
}

/// `propose` drafts a map from the data alone and writes nothing; the draft, with its fields
/// named after the class's properties, saved through `replace`, answers the known question.
#[tokio::test]
#[ignore = "live IRIS"]
async fn test_storage_map_data_172_propose_drafts_a_working_map() {
    use crate::storage172_rig::ask;
    let r = rig(&Where::live(), Toolset::Merged).await;
    let f = Fixture::read("Member");
    load_fixture(&r, &f).await;
    let class_before = r
        .call(
            "iris_doc",
            json!({"mode": "get", "name": "IadSm172.Member.cls", "namespace": "USER"}),
        )
        .await["content"]
        .clone();
    let data_before = member_crc(&r).await;
    let got = r
        .call(
            "iris_storage_map",
            json!({"action": "propose", "global": "^IadSm172Member", "namespace": "USER"}),
        )
        .await;
    let data_after = member_crc(&r).await;
    let class_after = r
        .call(
            "iris_doc",
            json!({"mode": "get", "name": "IadSm172.Member.cls", "namespace": "USER"}),
        )
        .await["content"]
        .clone();

    // The seed: rows are name^city^age at depth 1.
    let mut draft = got["draft"].clone();
    for (guess, prop) in [("Field1", "Name"), ("Field2", "City"), ("Field3", "Age")] {
        let mut field = draft["data"][guess].clone();
        field["name"] = json!(prop);
        draft["data"].as_object_mut().unwrap().remove(guess);
        draft["data"][prop] = field;
    }
    let version = r
        .call(
            "iris_storage_map",
            json!({"action": "read", "class": f.class, "namespace": "USER"}),
        )
        .await["storage"]["maps"][0]["version"]
        .clone();
    let saved = r
        .call(
            "iris_storage_map",
            json!({"action": "replace", "class": f.class, "map": "Master", "version": version,
                   "model": draft, "confirm_removal": true, "namespace": "USER"}),
        )
        .await;
    let rows = ask(&r, &f).await;
    let problems = drop_fixture(&r, &f).await;

    assert_eq!(got["success"], true, "{got}");
    assert_eq!(got["draft"]["subscripts"][0]["expression"], "{ID}", "{got}");
    for (i, guess) in ["Field1", "Field2", "Field3"].iter().enumerate() {
        assert_eq!(
            got["draft"]["data"][guess]["piece"],
            (i + 1).to_string(),
            "{got}"
        );
        assert_eq!(got["draft"]["data"][guess]["delimiter"], "\"^\"", "{got}");
    }
    assert!(
        got["guesses"].as_array().is_some_and(|g| !g.is_empty()),
        "{got}"
    );
    assert_eq!(data_after, data_before, "propose changed the global");
    assert_eq!(class_after, class_before, "propose changed the class");
    assert_eq!(saved["success"], true, "{saved}");
    let expected = f.seed["answer"].as_array().cloned().unwrap_or_default();
    assert_eq!(
        rows, expected,
        "the saved draft does not answer the known question"
    );
    assert!(problems.is_empty(), "cleanup: {problems:?}");
}

// ── T089: compare and preview run as the service account ────────────────────

/// With a service account set, `compare` and `preview` read the globals as that account and say
/// so in `identity`; with an account that cannot log in they refuse, and never fall back to the
/// primary connection's user.
#[tokio::test]
#[ignore = "live IRIS"]
async fn test_storage_map_data_172_compare_and_preview_run_as_the_service_account() {
    use crate::support_identity_141::{Account, Container};
    use iris_agentic_dev_core::testing::EnvGuard;
    let r = rig(&Where::live(), Toolset::Merged).await;
    let f = Fixture::read("Member");
    load_fixture(&r, &f).await;
    let c = Container::live();
    let user = "iadT172Svc";
    let acct = Account::create(&c, user, "Pw172Svcx", "%DB_USER:RW,%Development:U");
    let (compared, previewed) = {
        let _u = EnvGuard::with("IRIS_SERVICE_USERNAME", user);
        let _p = EnvGuard::with("IRIS_SERVICE_PASSWORD", "Pw172Svcx");
        let compared = compare(&r, &f).await;
        let previewed = r
            .call(
                "iris_storage_map",
                json!({"action": "preview", "class": f.class, "namespace": "USER"}),
            )
            .await;
        (compared, previewed)
    };
    drop(acct);
    // An account that cannot log in. (A disabled account is no test of this: IRIS keeps the CSP
    // session it already holds, measured 2026-11-14, so the next call still runs.)
    let refused = {
        let _u = EnvGuard::with("IRIS_SERVICE_USERNAME", "iadT172NoSuch");
        let _p = EnvGuard::with("IRIS_SERVICE_PASSWORD", "Pw172Nonex");
        compare(&r, &f).await
    };
    let problems = drop_fixture(&r, &f).await;

    let ran_as = |v: &serde_json::Value| {
        v["identity"]["user"]
            .as_str()
            .is_some_and(|u| u.eq_ignore_ascii_case(user))
    };
    // Seed: three rows; the account read them.
    assert_eq!(compared["success"], true, "{compared}");
    assert_eq!(compared["shape"]["depths"]["1"], 3, "{compared}");
    assert!(ran_as(&compared), "{compared}");
    assert_eq!(compared["identity"]["verified"], true, "{compared}");
    assert_eq!(previewed["success"], true, "{previewed}");
    assert_eq!(previewed["row_count"], 3, "{previewed}");
    assert!(ran_as(&previewed), "{previewed}");
    // Disabled: refused, no shape, and not answered as the primary user instead.
    assert_ne!(refused["success"], true, "{refused}");
    assert!(refused["shape"].is_null(), "{refused}");
    let refused_user = refused["identity"]["user"].as_str().unwrap_or("");
    assert!(
        refused_user.is_empty() || refused_user.eq_ignore_ascii_case("iadT172NoSuch"),
        "fell back to another identity: {refused}"
    );
    eprintln!("T089 refusal: {refused}");
    assert!(problems.is_empty(), "cleanup: {problems:?}");
}
