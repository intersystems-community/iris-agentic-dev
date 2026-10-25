//! Spec 141 R5 C22 and FR-030c, live half: message bodies and streams under a server `dataPolicy`.
//!
//! Against iris-dev-iris (USER is an Interoperability namespace). Two bodies are seeded: an HL7 v2
//! message and a plain-text note, each as an `Ens.StringContainer` behind an `Ens.MessageHeader` and
//! as a `%Stream.GlobalCharacter`. With `[policy.default] dataPolicy = "redact"`:
//!
//! - the HL7 body comes back with PID and MSH-3 redacted from `iris_message_body` and
//!   `stream_inspect`;
//! - the plain note gets `BODY_NOT_REDACTABLE` from `iris_message_body`, and from `stream_inspect`,
//!   which still answers with class, size and line count and none of the content;
//! - `journal_search` stays refused.
//!
//! `block` and `allow` are the controls: block refuses the stream's content with its metadata, and
//! allow returns it as stored. Teardown deletes every seeded row.
//!
//! Run: `IRIS_HOST=localhost IRIS_WEB_PORT=52780 cargo test --features testing --test integration
//! test_message_body_redact_141 -- --include-ignored --test-threads=1`

use iris_agentic_dev_core::iris::config_sources::{ConfigSnapshot, LoadInputs, CONFIG_FILE_NAME};
use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};
use iris_agentic_dev_core::skills::SkillRegistry;
use iris_agentic_dev_core::tools::{IrisTools, Toolset};
use serde_json::{json, Value};

const HL7_PARTS: [&str; 2] = [
    "MSH|^~\\&|SendApp141|SendFac|RecApp|RecFac|20260101||ADT^A01|1|P|2.5",
    "PID|1||MRN141||Doe141^Jane||19800101|F|||1 Main St||555-0141",
];
const PLAIN: &str = "Jane Doe141 was seen on 2026-01-01";

pub fn conn() -> IrisConnection {
    let host = std::env::var("IRIS_HOST").unwrap_or_else(|_| "localhost".into());
    let port = std::env::var("IRIS_WEB_PORT").unwrap_or_else(|_| "52780".into());
    let user = std::env::var("IRIS_USERNAME").unwrap_or_else(|_| "_SYSTEM".into());
    let pass = std::env::var("IRIS_PASSWORD").unwrap_or_else(|_| "SYS".into());
    IrisConnection::new(
        format!("http://{host}:{port}"),
        "USER",
        user,
        pass,
        DiscoverySource::EnvVar,
    )
}

/// Tools on the live connection, with `[policy.default] dataPolicy = <policy>` in an operator file.
pub fn tools_with(policy: &str) -> (IrisTools, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let cwd = dir.path().join("proj");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&cwd).unwrap();
    std::fs::write(
        home.join(CONFIG_FILE_NAME),
        format!("[policy.default]\ndataPolicy = \"{policy}\"\n"),
    )
    .unwrap();
    let inputs = LoadInputs {
        cwd: std::fs::canonicalize(&cwd).unwrap(),
        home: Some(std::fs::canonicalize(&home).unwrap()),
        ..Default::default()
    };
    let snapshot = ConfigSnapshot::load(&inputs);
    let declared = snapshot.declared_gates();
    let tools = IrisTools::with_registry_and_toolset(
        Some(conn()),
        SkillRegistry::new(),
        Toolset::Merged,
        None,
        None,
        true,
        declared,
    )
    .unwrap();
    tools.set_config(inputs, snapshot);
    (tools, dir)
}

pub async fn run(code: &str) -> String {
    let client = IrisConnection::http_client().unwrap();
    conn()
        .execute_via_generator(code, "USER", &client)
        .await
        .expect("IRIS runs the seed code")
}

/// An ObjectScript expression for `text` with `\r` segment separators.
fn hl7_expr() -> String {
    HL7_PARTS
        .iter()
        .map(|s| format!("\"{}\"", s.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join("_$C(13)_")
}

struct Seeded {
    hl7_header: String,
    plain_header: String,
    hl7_stream: String,
    plain_stream: String,
}

async fn seed() -> Seeded {
    let code = format!(
        r#"Set ids=""
For kind="hl7","plain" {{
  Set text=$Select(kind="hl7":{hl7},1:"{plain}")
  Set body=##class(Ens.StringContainer).%New(text)
  Set sc=body.%Save()
  Set hdr=##class(Ens.MessageHeader).%New()
  Set hdr.MessageBodyClassName=body.%ClassName(1)
  Set hdr.MessageBodyId=body.%Id()
  Set hdr.SourceConfigName="IadRedact141"
  Set sc=hdr.%Save()
  Set st=##class(%Stream.GlobalCharacter).%New()
  Do st.Write(text)
  Set sc=st.%Save()
  Set ids=ids_hdr.%Id()_","_st.%Id()_";"
}}
Write ids"#,
        hl7 = hl7_expr(),
        plain = PLAIN,
    );
    let out = run(&code).await;
    let rows: Vec<Vec<String>> = out
        .trim()
        .split(';')
        .filter(|r| !r.is_empty())
        .map(|r| r.split(',').map(str::to_string).collect())
        .collect();
    assert_eq!(rows.len(), 2, "seed output: {out:?}");
    Seeded {
        hl7_header: rows[0][0].clone(),
        hl7_stream: rows[0][1].clone(),
        plain_header: rows[1][0].clone(),
        plain_stream: rows[1][1].clone(),
    }
}

async fn teardown() {
    run(r#"Set rs=##class(%SQL.Statement).%ExecDirect(,"SELECT ID, MessageBodyId FROM Ens.MessageHeader WHERE SourceConfigName = 'IadRedact141'")
While rs.%Next() { Do ##class(Ens.StringContainer).%DeleteId(rs.%Get("MessageBodyId")) Do ##class(Ens.MessageHeader).%DeleteId(rs.%Get("ID")) }
Write "OK""#)
    .await;
}

async fn stream_cleanup(s: &Seeded) {
    run(&format!(
        "Do ##class(%Stream.GlobalCharacter).%DeleteId({}) Do ##class(%Stream.GlobalCharacter).%DeleteId({}) Write \"OK\"",
        s.hl7_stream, s.plain_stream
    ))
    .await;
}

pub async fn call(tools: &IrisTools, tool: &str, args: Value) -> Value {
    let r = tools
        .call_for_test(tool, args)
        .await
        .unwrap_or_else(|e| panic!("{tool}: {e}"));
    r.structured_content.unwrap_or(Value::Null)
}

fn code(b: &Value) -> &str {
    b["error_code"].as_str().unwrap_or("")
}

#[tokio::test]
#[ignore = "live IRIS on iris-dev-iris"]
async fn under_redact_hl7_is_redacted_and_everything_else_is_refused() {
    teardown().await;
    let s = seed().await;
    let (tools, _d) = tools_with("redact");

    let hl7_camel = call(
        &tools,
        "iris_message_body",
        json!({"message_id": s.hl7_header, "dataPolicy": "redact"}),
    )
    .await;
    let hl7_snake = call(
        &tools,
        "iris_message_body",
        json!({"message_id": s.hl7_header, "data_policy": "redact"}),
    )
    .await;
    // The caller cannot widen: allow against a redact server is still redact.
    let hl7_wide = call(
        &tools,
        "iris_message_body",
        json!({"message_id": s.hl7_header, "dataPolicy": "allow", "acknowledgePhi": true}),
    )
    .await;
    let hl7_unset = call(
        &tools,
        "iris_message_body",
        json!({"message_id": s.hl7_header}),
    )
    .await;
    let plain_body = call(
        &tools,
        "iris_message_body",
        json!({"message_id": s.plain_header, "dataPolicy": "redact"}),
    )
    .await;
    let near_miss = call(
        &tools,
        "iris_message_body",
        json!({"message_id": s.hl7_header, "dataPolicy": "redacted"}),
    )
    .await;
    let hl7_stream = call(&tools, "stream_inspect", json!({"oid": s.hl7_stream})).await;
    let plain_stream = call(&tools, "stream_inspect", json!({"oid": s.plain_stream})).await;
    let journal = call(
        &tools,
        "journal_search",
        json!({"global_pattern": "^IadRedact141*"}),
    )
    .await;

    teardown().await;
    stream_cleanup(&s).await;

    for (label, b) in [
        ("camel", &hl7_camel),
        ("snake", &hl7_snake),
        ("widened", &hl7_wide),
        // T177: an omitted argument means the server's policy, which here is redact.
        ("unset", &hl7_unset),
    ] {
        assert_eq!(b["success"], true, "{label}: {b}");
        let body = b["body"].as_str().unwrap();
        assert!(body.contains("[REDACTED]"), "{label}: {body:?}");
        for secret in ["Doe141", "MRN141", "SendApp141"] {
            assert!(
                !body.contains(secret),
                "{label}: {secret} survived: {body:?}"
            );
        }
        assert!(
            body.contains('\r'),
            "{label}: segment separators kept: {body:?}"
        );
    }
    assert_eq!(code(&plain_body), "BODY_NOT_REDACTABLE", "{plain_body}");
    assert!(!plain_body.to_string().contains("Doe141"), "{plain_body}");
    assert_eq!(code(&near_miss), "INVALID_PARAMS", "{near_miss}");

    assert_eq!(hl7_stream["success"], true, "{hl7_stream}");
    let content = hl7_stream["content"].as_str().unwrap();
    assert!(
        content.contains("[REDACTED]") && !content.contains("Doe141"),
        "{content:?}"
    );
    assert_eq!(hl7_stream["redacted"], true);

    assert_eq!(code(&plain_stream), "BODY_NOT_REDACTABLE", "{plain_stream}");
    assert_eq!(plain_stream["class"], "%Stream.GlobalCharacter");
    assert_eq!(plain_stream["size"], PLAIN.len() as i64);
    assert_eq!(plain_stream["line_count"], 1);
    assert!(plain_stream.get("content").is_none(), "{plain_stream}");
    assert!(
        !plain_stream.to_string().contains("Doe141"),
        "{plain_stream}"
    );
    assert_eq!(
        plain_stream["refusal"]["kind"], "operator_limit",
        "{plain_stream}"
    );

    assert_eq!(
        code(&journal),
        "DATA_POLICY_BLOCKED",
        "journal_search stays refused: {journal}"
    );
}

#[tokio::test]
#[ignore = "live IRIS on iris-dev-iris"]
async fn under_block_a_stream_answers_with_metadata_only() {
    teardown().await;
    let s = seed().await;
    let (tools, _d) = tools_with("block");
    let hl7_stream = call(&tools, "stream_inspect", json!({"oid": s.hl7_stream})).await;
    let body = call(
        &tools,
        "iris_message_body",
        json!({"message_id": s.hl7_header, "dataPolicy": "allow", "acknowledgePhi": true}),
    )
    .await;
    teardown().await;
    stream_cleanup(&s).await;

    assert_eq!(code(&hl7_stream), "DATA_POLICY_BLOCKED", "{hl7_stream}");
    assert_eq!(hl7_stream["class"], "%Stream.GlobalCharacter");
    assert_eq!(hl7_stream["line_count"], 2);
    assert!(
        hl7_stream.get("content").is_none() && !hl7_stream.to_string().contains("Doe141"),
        "{hl7_stream}"
    );
    assert_eq!(code(&body), "DATA_POLICY_BLOCKED", "{body}");
}

#[tokio::test]
#[ignore = "live IRIS on iris-dev-iris"]
async fn under_allow_the_content_comes_back_as_stored() {
    teardown().await;
    let s = seed().await;
    let (tools, _d) = tools_with("allow");
    let stream = call(&tools, "stream_inspect", json!({"oid": s.plain_stream})).await;
    let body = call(
        &tools,
        "iris_message_body",
        json!({"message_id": s.plain_header, "dataPolicy": "allow", "acknowledgePhi": true}),
    )
    .await;
    let unack = call(
        &tools,
        "iris_message_body",
        json!({"message_id": s.plain_header, "dataPolicy": "Allow"}),
    )
    .await;
    teardown().await;
    stream_cleanup(&s).await;

    assert_eq!(stream["content"], PLAIN, "{stream}");
    // iris_message_body ends a stored body with the newline IRIS writes after it.
    assert_eq!(
        body["body"].as_str().map(str::trim_end),
        Some(PLAIN),
        "{body}"
    );
    assert_eq!(
        code(&unack),
        "PHI_ACK_REQUIRED",
        "a near-miss spelling of allow is allow: {unack}"
    );
}

/// An ObjectScript expression for `text`, with control characters written as `$C(n)`.
fn os_expr(text: &str) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut run = String::new();
    for c in text.chars() {
        if c.is_control() {
            if !run.is_empty() {
                parts.push(format!("\"{run}\""));
                run.clear();
            }
            parts.push(format!("$C({})", c as u32));
        } else {
            run.push(c);
        }
    }
    if !run.is_empty() {
        parts.push(format!("\"{run}\""));
    }
    parts.join("_")
}

/// A segment of `count` fields where field N holds the token `<name>Nz`.
fn tokens(name: &str, count: usize) -> String {
    (1..=count).fold(name.to_string(), |line, n| format!("{line}|{name}{n}z"))
}

/// T195: a stored message whose segments carry the bytes that used to make the redactor skip them
/// (a blank, MLLP start, a file separator, a tab) comes back from the tool with the identifying
/// fields of PID, NK1, GT1 and IN1 masked. The row is deleted afterwards.
#[tokio::test]
#[ignore = "live IRIS on iris-dev-iris"]
async fn a_stored_message_with_padded_segments_is_redacted_through_the_tool() {
    teardown().await;
    let message = [
        "\x0bMSH|^~\\&|SendApp141|SendFac|RecApp|RecFac|20260101||ADT^A01|1|P|2.5".to_string(),
        format!(" {}", tokens("PID", 20)),
        format!("\x1c{}", tokens("NK1", 33)),
        format!("\x0b{}", tokens("GT1", 19)),
        format!("\t{}", tokens("IN1", 49)),
    ]
    .join("\r");
    let code = format!(
        r#"Set body=##class(Ens.StringContainer).%New({text})
Set sc=body.%Save()
Set hdr=##class(Ens.MessageHeader).%New()
Set hdr.MessageBodyClassName=body.%ClassName(1)
Set hdr.MessageBodyId=body.%Id()
Set hdr.SourceConfigName="IadRedact141"
Set sc=hdr.%Save()
Write hdr.%Id()"#,
        text = os_expr(&message)
    );
    let id = run(&code).await.trim().to_string();
    assert!(
        !id.is_empty() && id.chars().all(|c| c.is_ascii_digit()),
        "seed output: {id:?}"
    );
    let (tools, _d) = tools_with("redact");
    let b = call(&tools, "iris_message_body", json!({"message_id": id})).await;
    teardown().await;

    assert_eq!(b["success"], true, "{b}");
    let body = b["body"].as_str().unwrap();
    for secret in [
        "SendApp141",
        "PID2z",
        "PID3z",
        "PID5z",
        "PID13z",
        "PID19z",
        "PID20z",
        "NK12z",
        "NK14z",
        "NK15z",
        "NK133z",
        "GT13z",
        "GT15z",
        "GT112z",
        "IN116z",
        "IN119z",
        "IN136z",
        "IN149z",
    ] {
        assert!(!body.contains(secret), "{secret} survived: {body:?}");
    }
    // Fields outside the mask are as stored, and the framing bytes were kept.
    for kept in ["PID1z", "NK11z", "NK13z", "GT11z", "IN11z", "IN12z"] {
        assert!(body.contains(kept), "{kept} was masked: {body:?}");
    }
    assert!(
        body.contains("\x0bMSH|") && body.contains("\x1cNK1|"),
        "{body:?}"
    );
}
