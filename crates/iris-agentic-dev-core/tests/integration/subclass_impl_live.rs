//! `find_subclass_implementations` against live IRIS — the four defects that make it lie.
//!
//! Every assertion here has a ground-truth SQL query behind it that I ran against `iris-dev-iris`
//! before writing the test, so a failure means the tool disagrees with the dictionary, not that the
//! expectation was guessed. Run with:
//!   IRIS_HOST=localhost IRIS_WEB_PORT=52780 \
//!   cargo test --features testing --test subclass_impl_live -- --ignored --test-threads=1
//!
//! The four:
//!
//! 1. `limit` reached the SQL as `FETCH FIRST n ROWS ONLY`, so it truncated the namespace-wide set
//!    of classes defining the method *before* the descendant filter ran. `%OnNew` has 644 definers
//!    in %SYS; the first 100 alphabetically end at `%CPT.JS.Runtime.Object`, so every base whose
//!    subclasses sort later reported zero.
//! 2. The result JSON was concatenated by hand, and `FormalSpec` routinely contains `"` (any
//!    `%String(MAXLEN="")` default). That produced malformed JSON, which `unwrap_or(json!([]))`
//!    turned into an empty list. Same symptom as (1), different cause.
//! 3. Hierarchy expansion matched `Super LIKE '%base%'`, an unanchored substring test against a
//!    comma-delimited column, so a class extending `<base><suffix>` was pulled in as a descendant.
//! 4. Expansion issued one SQL query per class discovered, all inside a single Atelier request.
//!    `%Library.Persistent` in USER took 29 s, which is what made the two `_v2`/`_cache_hit` tests
//!    look flaky under a long serial run — they were timing out, not losing the connection.

use iris_agentic_dev_core::elicitation::{CheckoutCache, ElicitationStore};
use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};
use iris_agentic_dev_core::tools::dict::{
    handle_find_subclass_implementations, FindSubclassImplementationsParams, MetadataCache,
};
use iris_agentic_dev_core::tools::doc::{handle_iris_doc, IrisDocParams};

fn make_conn() -> Option<(IrisConnection, reqwest::Client)> {
    let host = std::env::var("IRIS_HOST").unwrap_or_default();
    if host.is_empty() {
        return None;
    }
    let web_port = std::env::var("IRIS_WEB_PORT").unwrap_or_else(|_| "52780".to_string());
    let username = std::env::var("IRIS_USERNAME").unwrap_or_else(|_| "_SYSTEM".to_string());
    let password = std::env::var("IRIS_PASSWORD").unwrap_or_else(|_| "SYS".to_string());
    let conn = IrisConnection::new(
        format!("http://{host}:{web_port}"),
        "USER",
        username,
        password,
        DiscoverySource::EnvVar,
    );
    Some((conn, reqwest::Client::new()))
}

fn new_cache() -> MetadataCache {
    MetadataCache::default()
}

async fn find(
    iris: &IrisConnection,
    client: &reqwest::Client,
    args: serde_json::Value,
) -> serde_json::Value {
    let p: FindSubclassImplementationsParams = serde_json::from_value(args).expect("params");
    let cache = new_cache();
    let r = handle_find_subclass_implementations(iris, client, p, &cache)
        .await
        .expect("handler returned Err");
    let text = r.content[0].as_text().unwrap().text.clone();
    serde_json::from_str(&text).unwrap_or_else(|_| serde_json::json!({"raw": text}))
}

fn classes(v: &serde_json::Value) -> Vec<String> {
    v["implementations"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|i| i["class"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

// ── 1. limit truncated before the filter ─────────────────────────────────────

/// Ground truth, `%SYS`:
/// `SELECT m.parent FROM %Dictionary.CompiledMethod m WHERE m.Name='%OnNew' AND m.Origin=m.parent`
/// restricted to the `%Stream.Object` tree returns `%Library.AbstractStream`,
/// `%Stream.DynamicCharacter`, `%Stream.FileBinary` and `%Stream.TmpCharacter`, all direct
/// subclasses. The tool reported zero, because `FETCH FIRST 100` had already consumed the
/// alphabetical head of all 644 `%OnNew` definers.
#[tokio::test]
#[ignore]
async fn limit_does_not_truncate_before_the_descendant_filter() {
    let Some((iris, client)) = make_conn() else {
        return;
    };
    let v = find(
        &iris,
        &client,
        serde_json::json!({
            "method_name": "%OnNew",
            "base_classes": ["%Stream.Object"],
            "namespace": "%SYS"
        }),
    )
    .await;
    let found = classes(&v);
    for expected in [
        "%Library.AbstractStream",
        "%Stream.DynamicCharacter",
        "%Stream.FileBinary",
        "%Stream.TmpCharacter",
    ] {
        assert!(
            found.iter().any(|c| c == expected),
            "{expected} defines %OnNew and extends %Stream.Object, but is missing: {v}"
        );
    }
}

/// `limit` still has to limit. Two calls, same hierarchy, different caps: the capped one must
/// return exactly `limit` rows and they must be a subset of the uncapped answer.
#[tokio::test]
#[ignore]
async fn limit_caps_the_filtered_set_not_the_raw_scan() {
    let Some((iris, client)) = make_conn() else {
        return;
    };
    let all = find(
        &iris,
        &client,
        serde_json::json!({
            "method_name": "%OnNew",
            "base_classes": ["%Stream.Object"],
            "namespace": "%SYS"
        }),
    )
    .await;
    let capped = find(
        &iris,
        &client,
        serde_json::json!({
            "method_name": "%OnNew",
            "base_classes": ["%Stream.Object"],
            "namespace": "%SYS",
            "limit": 2
        }),
    )
    .await;
    let all = classes(&all);
    let capped = classes(&capped);
    assert!(all.len() > 2, "need >2 to test a cap of 2, got {all:?}");
    assert_eq!(capped.len(), 2, "limit 2 should return 2 rows: {capped:?}");
    for c in &capped {
        assert!(all.contains(c), "{c} is not in the uncapped answer {all:?}");
    }
}

// ── 2. a quote in FormalSpec erased the whole result ─────────────────────────

/// `%ASQ.Mutators` is the only class in %SYS defining `setPath` with `Origin = parent`, and its
/// `FormalSpec` is `context:%AbstractSet,path:%String(MAXLEN="")="",value:%Any=""`. One row, well
/// inside any limit, and the tool returned nothing: the embedded `"` broke the hand-built JSON and
/// the parse failure was swallowed by `unwrap_or(json!([]))`.
#[tokio::test]
#[ignore]
async fn a_quote_in_the_formal_spec_survives_as_json() {
    let Some((iris, client)) = make_conn() else {
        return;
    };
    let v = find(
        &iris,
        &client,
        serde_json::json!({
            "method_name": "setPath",
            "base_classes": ["%ASQ.Mutators"],
            "namespace": "%SYS"
        }),
    )
    .await;
    assert_eq!(
        v["implementation_count"], 1,
        "%ASQ.Mutators defines setPath: {v}"
    );
    assert_eq!(classes(&v), vec!["%ASQ.Mutators".to_string()]);
    let spec = v["implementations"][0]["formal_spec"]
        .as_str()
        .unwrap_or_default();
    assert!(
        spec.contains(r#"MAXLEN="""#),
        "the quotes belong in the formal spec, not dropped: {spec:?}"
    );
}

// ── 3. an unanchored LIKE made a sibling look like a descendant ──────────────

/// Three fixture classes, and the name of one is a prefix of the name of another:
///
/// ```text
/// IadSubclassFix.Base          extends %RegisteredObject
/// IadSubclassFix.BaseSibling   extends %RegisteredObject   <- name starts with "…Base"
/// IadSubclassFix.Child         extends IadSubclassFix.BaseSibling, defines IadProbe()
/// ```
///
/// `Child` is not a descendant of `Base`. But `Super LIKE '%IadSubclassFix.Base%'` matches
/// `IadSubclassFix.BaseSibling`, so the expansion claimed it, and `IadProbe` came back attributed
/// to the wrong hierarchy. There is no witness for this shape in stock %SYS, so the test builds one.
#[tokio::test]
#[ignore]
async fn a_sibling_whose_name_extends_the_base_is_not_a_descendant() {
    let Some((iris, client)) = make_conn() else {
        return;
    };
    let store = ElicitationStore::new();
    let checkout = CheckoutCache::new();

    let fixtures = [
        (
            "IadSubclassFix.Base.cls",
            "Class IadSubclassFix.Base Extends %RegisteredObject\n{\n}\n",
        ),
        (
            "IadSubclassFix.BaseSibling.cls",
            "Class IadSubclassFix.BaseSibling Extends %RegisteredObject\n{\n}\n",
        ),
        (
            "IadSubclassFix.Child.cls",
            "Class IadSubclassFix.Child Extends IadSubclassFix.BaseSibling\n{\n\n\
             Method IadProbe() As %Status\n{\n\tSet tSC = $$$OK\n\tQuit tSC\n}\n\n}\n",
        ),
    ];
    for (name, content) in fixtures {
        let p: IrisDocParams = serde_json::from_value(serde_json::json!({
            "mode": "put", "name": name, "content": content,
            "namespace": "USER", "compile": true
        }))
        .unwrap();
        let r = handle_iris_doc(&iris, &client, p, &store, &checkout).await;
        let text = r.unwrap().content[0].as_text().unwrap().text.clone();
        let j: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(j["success"], true, "fixture {name} failed to compile: {j}");
    }

    // Sanity: the real parent does find it. If this fails the fixtures are wrong, not the tool.
    let real = find(
        &iris,
        &client,
        serde_json::json!({
            "method_name": "IadProbe",
            "base_classes": ["IadSubclassFix.BaseSibling"],
            "namespace": "USER"
        }),
    )
    .await;
    assert_eq!(
        classes(&real),
        vec!["IadSubclassFix.Child".to_string()],
        "BaseSibling is Child's actual superclass: {real}"
    );

    let v = find(
        &iris,
        &client,
        serde_json::json!({
            "method_name": "IadProbe",
            "base_classes": ["IadSubclassFix.Base"],
            "namespace": "USER"
        }),
    )
    .await;
    assert_eq!(
        v["implementation_count"], 0,
        "IadSubclassFix.Child extends BaseSibling, not Base — a name prefix is not inheritance: {v}"
    );
}

// ── 4. expansion cost ────────────────────────────────────────────────────────

/// `%Library.Persistent` in USER is the whole class tree: 6,486 classes declare a superclass.
/// Expanding it with one SQL round trip per discovered class took 29 s wall against a healthy
/// container, which is what killed `test_dispatch_find_subclass_implementations_v2` and
/// `..._cache_hit` under `--test-threads=1` with coverage instrumentation attached. Ten seconds is
/// a generous ceiling for one tool call and still an order of magnitude below the old cost.
#[tokio::test]
#[ignore]
async fn expanding_the_whole_class_tree_stays_under_ten_seconds() {
    let Some((iris, client)) = make_conn() else {
        return;
    };
    let start = std::time::Instant::now();
    let v = find(
        &iris,
        &client,
        serde_json::json!({
            "method_name": "OnProcessInput",
            "base_classes": ["%Library.Persistent"],
            "namespace": "USER",
            "limit": 5
        }),
    )
    .await;
    let elapsed = start.elapsed();
    assert_eq!(v["success"], true, "{v}");
    assert!(
        elapsed.as_secs() < 10,
        "expansion took {elapsed:?}; one SQL query per class does not scale"
    );
}
