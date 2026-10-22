//! `resolve_dynamic_dispatch` built its JSON by joining strings, so a FormalSpec holding a quote
//! (a default such as `pX As %String = "a"`) made the output unparseable. Atelier then failed
//! the query (ERROR #5035), and before that was surfaced the tool reported zero candidates as a
//! success; once Atelier returned it as text, iad's own parse fell back to an empty list. Two
//! classes in a scratch package each define `Ping` with such a default; both must come back.

use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};
use iris_agentic_dev_core::tools::IrisTools;

fn conn() -> (IrisConnection, reqwest::Client) {
    let get = |k: &str, d: &str| std::env::var(k).unwrap_or_else(|_| d.to_string());
    let host = std::env::var("IRIS_HOST").expect("IRIS_HOST names the live server");
    let c = IrisConnection::new(
        format!("http://{host}:{}", get("IRIS_WEB_PORT", "52780")),
        "USER",
        get("IRIS_USERNAME", "_SYSTEM"),
        get("IRIS_PASSWORD", "SYS"),
        DiscoverySource::EnvVar,
    );
    (c, reqwest::Client::new())
}

const MAKE: &str = r#" For c="IadDispatch140.A","IadDispatch140.B" {
   Set d=##class(%Dictionary.ClassDefinition).%New() Set d.Name=c
   Set m=##class(%Dictionary.MethodDefinition).%New() Set m.Name="Ping", m.FormalSpec="pX:%String=""a"""
   Do m.Implementation.WriteLine(" Quit") Do d.Methods.Insert(m)
   Set sc=d.%Save() Set:sc sc=$system.OBJ.Compile(c,"ck-d") Write:'sc $system.Status.GetErrorText(sc),!
 }
 Write "made""#;

#[tokio::test]
#[ignore = "requires live IRIS (iris-dev-iris); creates and deletes IadDispatch140.*"]
async fn a_formal_spec_with_a_quote_still_resolves() {
    let (c, client) = conn();
    let made = c
        .execute_via_generator(MAKE, "USER", &client)
        .await
        .unwrap();
    assert!(made.contains("made"), "fixture: {made}");

    let tools = IrisTools::new(Some(c.clone())).expect("IrisTools::new");
    let result = tools
        .call_for_test(
            "resolve_dynamic_dispatch",
            serde_json::json!({"method_name": "Ping", "package_prefix": "IadDispatch140.", "namespace": "USER"}),
        )
        .await;
    let _ = c
        .execute_via_generator(
            r#" Do $system.OBJ.DeletePackage("IadDispatch140","-d")"#,
            "USER",
            &client,
        )
        .await;

    let r = result.expect("resolve_dynamic_dispatch");
    let text = r.content[0].as_text().unwrap().text.clone();
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["candidate_count"], 2, "{v}");
    assert!(
        v["candidates"][0]["formal_spec"]
            .as_str()
            .is_some_and(|f| f.contains("\"a\"")),
        "the quote survives: {v}"
    );
}
