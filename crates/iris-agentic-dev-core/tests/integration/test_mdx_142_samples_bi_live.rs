//! PR 142: the `iris-mdx` claims that disagree with IRIS, run as the PR wrote them on the cubes
//! the PR names (Samples-BI `HoleFoods` and `Patients`).
//!
//! iad spec 131 ("MDX cube facts", on iad's own `131-mdx-cube-facts` branch, not part of this PR)
//! first measured each claim on an 8-row cube built for the purpose. Two claims that failed there
//! hold on Samples-BI (R1a, R7), so each discrepancy is checked here, on the author's cubes, with
//! the author's queries. The cases live in `tests/fixtures/mdx142/IADRepro.MDX142.cls`,
//! which anyone can import into a Samples-BI namespace and run with
//! `Do ##class(IADRepro.MDX142).Run()`. This file loads it, asserts each outcome, and deletes it.
//!
//! Samples-BI data comes from an unseeded generator, so only shapes, labels, errors and relations
//! between cells are asserted, never a count.
//!
//! Run with (the namespace must hold a built Samples-BI):
//!   IRIS_HOST=localhost IRIS_WEB_PORT=52780 IRIS_USERNAME=_SYSTEM IRIS_PASSWORD=SYS \
//!   IAD_SAMPLES_BI_NS=IADSBI \
//!   cargo test --features testing --test integration test_mdx_142_samples_bi_live -- \
//!     --ignored --test-threads=1

use iris_agentic_dev_core::iris::connection::{
    is_generator_error, DiscoverySource, IrisConnection,
};

const DOC: &str = "IADRepro.MDX142.cls";
const SRC: &str = include_str!("../fixtures/mdx142/IADRepro.MDX142.cls");

fn conn() -> Option<(IrisConnection, reqwest::Client, String)> {
    let host = std::env::var("IRIS_HOST").unwrap_or_default();
    let ns = std::env::var("IAD_SAMPLES_BI_NS").unwrap_or_default();
    if host.is_empty() || ns.is_empty() {
        if std::env::var("IAD_ALLOW_SKIP").is_ok() {
            eprintln!("SKIP (IAD_ALLOW_SKIP set): IRIS_HOST or IAD_SAMPLES_BI_NS unset");
            return None;
        }
        panic!(
            "IRIS_HOST or IAD_SAMPLES_BI_NS unset. This test runs PR 142's MDX on Samples-BI; \
             without it it asserts nothing, so it fails instead of passing quietly.\n\
             Set: IRIS_HOST=localhost IRIS_WEB_PORT=52780 IRIS_USERNAME=_SYSTEM IRIS_PASSWORD=SYS \
             IAD_SAMPLES_BI_NS=<namespace with Samples-BI built>\n\
             Or opt into skipping deliberately: IAD_ALLOW_SKIP=1"
        );
    }
    let port = std::env::var("IRIS_WEB_PORT").unwrap_or_else(|_| "52780".into());
    let user = std::env::var("IRIS_USERNAME").unwrap_or_else(|_| "_SYSTEM".into());
    let pass = std::env::var("IRIS_PASSWORD").unwrap_or_else(|_| "SYS".into());
    let conn = IrisConnection::new(
        format!("http://{host}:{port}"),
        &ns,
        user,
        pass,
        DiscoverySource::EnvVar,
    );
    Some((conn, reqwest::Client::new(), ns))
}

async fn load(c: &IrisConnection, client: &reqwest::Client, ns: &str) {
    let url = c.versioned_ns_url(
        ns,
        &format!("/doc/{}?ignoreConflict=1", urlencoding::encode(DOC)),
    );
    let lines: Vec<&str> = SRC.lines().collect();
    let resp = client
        .put(&url)
        .basic_auth(&c.username, Some(&c.password))
        .json(&serde_json::json!({"enc": false, "content": lines}))
        .send()
        .await
        .expect("PUT must reach IRIS");
    let status = resp.status().as_u16();
    assert!((200..300).contains(&status), "PUT {DOC}: HTTP {status}");
    let r = c
        .compile_document(DOC, ns, "cuk", client)
        .await
        .expect("compile request must reach IRIS");
    assert!(
        r.errors.is_empty(),
        "{DOC} did not compile: {}\n{}",
        r.errors.join("\n"),
        r.console.join("\n")
    );
}

async fn unload(c: &IrisConnection, client: &reqwest::Client, ns: &str) {
    let _ = c
        .execute_via_generator(
            " Do $system.OBJ.Delete(\"IADRepro.MDX142\",\"-d\")",
            ns,
            client,
        )
        .await;
}

/// The `Outcome` line for one case id.
async fn outcome(c: &IrisConnection, client: &reqwest::Client, ns: &str, id: &str) -> String {
    run(c, client, ns, "Outcome", id).await
}

/// `method` (`Outcome` or `Twice`) on one case id.
async fn run(
    c: &IrisConnection,
    client: &reqwest::Client,
    ns: &str,
    method: &str,
    id: &str,
) -> String {
    let code = format!(
        " Do ##class(IADRepro.MDX142).Cases(.c) \
         Write \"~[\",##class(IADRepro.MDX142).{method}($ListGet(c(\"{id}\"),2)),\"]~\""
    );
    let out = c
        .execute_via_generator(&code, ns, client)
        .await
        .expect("execute request must reach IRIS");
    assert!(!is_generator_error(&out), "case {id} failed to run: {out}");
    let start = out.rfind("~[").unwrap_or_else(|| panic!("no ~[ in {out}"));
    let end = out[start..]
        .find("]~")
        .unwrap_or_else(|| panic!("no ]~ in {out}"));
    out[start + 2..start + end].to_string()
}

/// The row labels of a grid outcome: the text before each `: ` after `; `.
fn row_labels(o: &str) -> Vec<String> {
    o.split("; ")
        .skip(1)
        .map(|r| r.split(": ").next().unwrap_or("").to_string())
        .collect()
}

#[tokio::test]
#[ignore = "requires live IRIS with Samples-BI"]
async fn pr142_claims_on_samples_bi() {
    let Some((c, client, ns)) = conn() else {
        return;
    };
    load(&c, &client, &ns).await;
    let mut o = std::collections::BTreeMap::new();
    for id in [
        "R1a", "R2", "R3", "R4", "R5a", "R5b", "R6", "R7a", "R7b", "R7c", "R8a", "R8b", "R9",
        "R10a", "R10b", "R10c",
    ] {
        o.insert(id, outcome(&c, &client, &ns, id).await);
    }
    for id in ["R1b", "R1c", "R1d"] {
        o.insert(id, run(&c, &client, &ns, "Twice", id).await);
    }
    unload(&c, &client, &ns).await;
    for (k, v) in &o {
        eprintln!("{k}: {v}");
    }
    let get = |id: &str| o[id].as_str();

    // R1: holds, as an IRIS results-cache bug. A %MDX() axis query returns its value on the first
    // run of a text and nothing (no axis, no cell, no error) on every run after. The same query
    // as plain MDX returns its value both times. R1a, the PR's text, has run before, so it is
    // empty.
    assert_eq!(get("R1a"), "cols=[] rows=0 cells: ", "R1a");
    for id in ["R1b", "R1c"] {
        let (first, second) = get(id).split_once(" || ").unwrap();
        assert!(
            first.starts_with("1: cols=[Results] rows=0 cells: "),
            "{id}: {}",
            get(id)
        );
        assert!(!first.ends_with("cells: "), "{id} run 1 empty: {}", get(id));
        assert_eq!(second, "2: cols=[] rows=0 cells: ", "{id}");
    }
    let (first, second) = get("R1d").split_once(" || ").unwrap();
    assert_eq!(first[3..], second[3..], "R1d: {}", get("R1d"));
    assert!(!first.ends_with("cells: "), "R1d: {}", get("R1d"));

    // R2: false. MEASURES.MEMBERS starts with Count.
    assert!(get("R2").starts_with("cols=[Count|"), "R2: {}", get("R2"));

    // R3: false. The PR's fix raises the error it is meant to avoid.
    assert_eq!(
        get("R3"),
        "PREPARE ERROR ERROR #5001: Measures cannot exist on multiple axes"
    );

    // R4: false. The PR's own query, without %LABEL, is headed with the member's name.
    assert!(
        get("R4").starts_with("cols=[Units Sold|PrevUnits] "),
        "R4: {}",
        get("R4")
    );

    // R5: WHERE {a, b} and %OR give one count, as the PR says IRIS rewrites one into the other.
    let r5a = get("R5a");
    assert!(
        r5a.starts_with("cols=[Patient Count] rows=0 cells: "),
        "R5a: {r5a}"
    );
    assert_eq!(r5a, get("R5b"));

    // R6: false. The %OR row is labelled "asthma...diabetes", not "asthma+".
    assert_eq!(
        row_labels(get("R6")),
        ["asthma...diabetes"],
        "R6: {}",
        get("R6")
    );

    // R7: holds. HoleFoods Channel is keyed by number.
    assert!(!get("R7a").ends_with("cells: "), "R7a: {}", get("R7a"));
    assert!(get("R7b").ends_with("cells: "), "R7b: {}", get("R7b"));
    assert!(
        get("R7c").contains("; Online: 2; Retail: 1"),
        "R7c: {}",
        get("R7c")
    );

    // R8: a Patients member key in a HoleFoods query fails at prepare. A Patients dimension's
    // .MEMBERS on an axis raises nothing and the axis is dropped (D2GMDX "Nonexistent Members").
    assert!(
        get("R8a").starts_with("cols=[Count] "),
        "R8a: {}",
        get("R8a")
    );
    assert!(
        get("R8b").starts_with("PREPARE ERROR ERROR #5001: %GetDimensionInfo: Invalid Member spec"),
        "R8b: {}",
        get("R8b")
    );

    // R9: the unknown measure passes %PrepareMDX and fails at %Execute.
    assert_eq!(
        get("R9"),
        "EXECUTE ERROR ERROR #5001: Measure not found: HOLEFOODS:Nope"
    );

    // R10: [NOW-1y10m] on a month level lands one month back, not 22.
    let (a, b, one) = (
        row_labels(get("R10a")),
        row_labels(get("R10b")),
        row_labels(get("R10c")),
    );
    assert_eq!(a, one, "R10a vs R10c: {} / {}", get("R10a"), get("R10c"));
    assert_ne!(a, b, "R10a vs R10b");
}
