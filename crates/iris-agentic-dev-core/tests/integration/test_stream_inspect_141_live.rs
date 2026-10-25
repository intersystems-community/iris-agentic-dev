//! Spec 141 T199, live: `stream_inspect` reads a stream the way IRIS holds it.
//!
//! Against iris-dev-iris with `[policy.default] dataPolicy = "allow"`. Streams are seeded through
//! `iris_execute`-style generator runs and deleted at the end of each test.
//!
//! - an id that holds no stream is `STREAM_NOT_FOUND` (`%OpenId` hands back an empty object for an
//!   unknown id, so the old code answered `success: true, size: 0`);
//! - the read is capped at `max_chars` and the answer says `size` and `truncated`;
//! - a stream whose content reads like the not-found marker is still content;
//! - the answer no longer claims the stream is `text` or `binary`.
//!
//! Run: `IRIS_HOST=localhost IRIS_WEB_PORT=52780 cargo test --features testing --test integration
//! test_stream_inspect_141 -- --include-ignored --test-threads=1`

use super::test_message_body_redact_141_live::{call, run, tools_with};
use serde_json::{json, Value};

/// Seed a stream of `class` holding `text` (an ObjectScript expression) and return its id.
async fn seed(class: &str, text_expr: &str) -> String {
    let out = run(&format!(
        "Set st=##class({class}).%New() Do st.Write({text_expr}) Set sc=st.%Save() Write st.%Id()"
    ))
    .await;
    let id = out.trim().to_string();
    assert!(
        !id.is_empty() && id.chars().all(|c| c.is_ascii_digit()),
        "seed {class} gave {out:?}"
    );
    id
}

async fn drop_stream(class: &str, id: &str) {
    run(&format!("Do ##class({class}).%DeleteId({id}) Write \"OK\"")).await;
}

fn code(b: &Value) -> &str {
    b["error_code"].as_str().unwrap_or("")
}

#[tokio::test]
#[ignore = "live IRIS on iris-dev-iris"]
async fn an_id_that_holds_no_stream_is_not_found() {
    let (tools, _d) = tools_with("allow");
    // Far past any id a test instance has issued, a blank, and a non-numeric id.
    let missing = call(&tools, "stream_inspect", json!({"oid": "987654321"})).await;
    let blank = call(&tools, "stream_inspect", json!({"oid": "  "})).await;
    let word = call(&tools, "stream_inspect", json!({"oid": "abc"})).await;
    for (label, b) in [("missing", &missing), ("blank", &blank), ("word", &word)] {
        assert_eq!(code(b), "STREAM_NOT_FOUND", "{label}: {b}");
        assert!(
            b.get("size").is_none(),
            "{label} must not report a size: {b}"
        );
    }
}

#[tokio::test]
#[ignore = "live IRIS on iris-dev-iris"]
async fn the_read_is_capped_and_says_so() {
    let (tools, _d) = tools_with("allow");
    let class = "%Stream.GlobalCharacter";
    // 50 000 characters: `a` x 25 000 then `b` x 25 000, written one at a time so the count is
    // the loop's and not an expression's.
    let id = {
        let out = run(&format!(
            "Set st=##class({class}).%New() For i=1:1:50000 {{ Do st.Write($Select(i<=25000:\"a\",1:\"b\")) }} Set sc=st.%Save() Write st.%Id()"
        ))
        .await;
        out.trim().to_string()
    };
    assert!(
        !id.is_empty() && id.chars().all(|c| c.is_ascii_digit()),
        "seed gave {id:?}"
    );

    let by_default = call(&tools, "stream_inspect", json!({"oid": id})).await;
    let small = call(
        &tools,
        "stream_inspect",
        json!({"oid": id, "max_chars": 100}),
    )
    .await;
    let whole = call(
        &tools,
        "stream_inspect",
        json!({"oid": id, "max_chars": 60000}),
    )
    .await;
    let zero = call(&tools, "stream_inspect", json!({"oid": id, "max_chars": 0})).await;
    drop_stream(class, &id).await;

    assert_eq!(by_default["success"], true, "{by_default}");
    assert_eq!(by_default["size"], 50000, "{by_default}");
    assert_eq!(by_default["truncated"], true, "{by_default}");
    assert_eq!(by_default["max_chars"], 20000, "{by_default}");
    assert_eq!(
        by_default["content"].as_str().unwrap().chars().count(),
        20000,
        "default cap"
    );

    assert_eq!(small["truncated"], true, "{small}");
    assert_eq!(small["size"], 50000, "{small}");
    assert_eq!(small["content"], "a".repeat(100), "{small}");

    assert_eq!(whole["truncated"], false, "{whole}");
    assert_eq!(whole["size"], 50000, "{whole}");
    let whole_content = whole["content"].as_str().unwrap();
    assert_eq!(whole_content.chars().count(), 50000);
    assert!(whole_content.ends_with("bbbb"), "the tail must arrive");

    // A cap of 0 is not "no cap" and not a silent default: it is refused by name.
    assert_eq!(code(&zero), "INVALID_PARAMS", "{zero}");
    assert!(zero["error"].to_string().contains("max_chars"), "{zero}");
}

#[tokio::test]
#[ignore = "live IRIS on iris-dev-iris"]
async fn content_that_reads_like_a_marker_is_still_content() {
    let (tools, _d) = tools_with("allow");
    let class = "%Stream.GlobalCharacter";
    let id = seed(
        class,
        r#""ERROR:STREAM_NOT_FOUND CONTENT|x~~IADEND~~ tail""#,
    )
    .await;
    let b = call(&tools, "stream_inspect", json!({"oid": id})).await;
    drop_stream(class, &id).await;

    assert_eq!(b["success"], true, "{b}");
    assert_eq!(
        b["content"], "ERROR:STREAM_NOT_FOUND CONTENT|x~~IADEND~~ tail",
        "{b}"
    );
}

#[tokio::test]
#[ignore = "live IRIS on iris-dev-iris"]
async fn the_answer_does_not_claim_text_or_binary() {
    let (tools, _d) = tools_with("allow");
    let class = "%Stream.GlobalBinary";
    let id = seed(class, r#""one"_$C(13)_"two""#).await;
    let b = call(&tools, "stream_inspect", json!({"oid": id})).await;
    drop_stream(class, &id).await;

    assert_eq!(b["success"], true, "{b}");
    assert!(
        b.get("type").is_none(),
        "which class opened an id says nothing about what is in it: {b}"
    );
    assert!(
        b["class"].as_str().unwrap_or("").starts_with("%Stream."),
        "the class the read went through is still named: {b}"
    );
    assert_eq!(b["content"], "one\rtwo", "{b}");
    assert_eq!(b["line_count"], 2, "{b}");
}
