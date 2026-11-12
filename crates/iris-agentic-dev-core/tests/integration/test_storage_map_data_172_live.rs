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
