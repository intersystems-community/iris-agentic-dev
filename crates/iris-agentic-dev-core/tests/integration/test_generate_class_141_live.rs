//! Spec 141 T211, FR-012 to FR-017: `iris_generate_class` stores the text the model wrote and
//! compiles that, through the gated path `iris_doc` put with `compile` takes, against a real IRIS.
//!
//! The tool used to compile whatever class already had the name the model wrote. A description that
//! steered the name ran an existing class's generator, with no compile-time gate, no stored-source
//! check and no superclass check, and the generated text was never saved.
//!
//! The model is a local HTTP stub that answers like the OpenAI API: the only thing stubbed. Every
//! class and global a test makes is removed, also when it fails.
//!
//! Requires `iris-dev-iris`: `IRIS_HOST=localhost IRIS_WEB_PORT=52780`. Serial, `#[ignore]`: run
//! `lockf -k /tmp/iad-141-iris.lock cargo test --features testing --test integration generate_class_141 -- --ignored --test-threads=1`.

use iris_agentic_dev_core::iris::connection::{
    is_generator_error, DiscoverySource, IrisConnection,
};
use iris_agentic_dev_core::iris::connection_pool::ConnectionPool;
use iris_agentic_dev_core::skills::SkillRegistry;
use iris_agentic_dev_core::testing::EnvGuard;
use iris_agentic_dev_core::tools::{IrisTools, Toolset};
use serde_json::{json, Value};
use std::sync::Arc;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const NS: &str = "USER";
const MARKER: &str = "^IadGen141Marker";

fn dev_url() -> String {
    let host = std::env::var("IRIS_HOST").expect(
        "IRIS_HOST unset. Set IRIS_HOST=localhost IRIS_WEB_PORT=52780 for the iris-dev-iris container",
    );
    let port = std::env::var("IRIS_WEB_PORT").unwrap_or_else(|_| "52780".into());
    format!("http://{host}:{port}")
}

fn conn() -> IrisConnection {
    IrisConnection::new(
        dev_url(),
        NS,
        "_SYSTEM",
        "SYS",
        DiscoverySource::ExplicitFlag,
    )
}

/// A generator that sets the marker while the class compiles.
const MARKING_GENERATOR: &str =
    "ClassMethod Gen() As %String [ CodeMode = objectgenerator ]\n{\n    \
     Set ^IadGen141Marker = $USERNAME\n    Do %code.WriteLine(\"    Quit 1\")\n    Quit $$$OK\n}\n";

fn clean_class(name: &str, extends: &str) -> String {
    let ext = if extends.is_empty() {
        String::new()
    } else {
        format!(" Extends {extends}")
    };
    format!("Class {name}{ext}\n{{\n\nClassMethod Hello() As %String\n{{\n    Quit 41\n}}\n\n}}\n")
}

fn class_with_generator(name: &str, extends: &str) -> String {
    let ext = if extends.is_empty() {
        String::new()
    } else {
        format!(" Extends {extends}")
    };
    format!("Class {name}{ext}\n{{\n\n{MARKING_GENERATOR}\n}}\n")
}

/// A class that parses and does not compile: the method body is a bare `Set x =`.
fn broken_class(name: &str) -> String {
    format!("Class {name}\n{{\n\nClassMethod Hello() As %String\n{{\n    Set x = \n    Quit x\n}}\n\n}}\n")
}

struct Rig {
    tools: IrisTools,
    conn: IrisConnection,
    client: reqwest::Client,
    _env: EnvGuard,
}

impl Rig {
    async fn new(llm_url: &str) -> Rig {
        let mut env = EnvGuard::new();
        env.set("IRIS_GENERATE_CLASS_MODEL", "gpt-4o");
        env.set("OPENAI_API_KEY", "sk-test-key");
        env.unset("ANTHROPIC_API_KEY");
        env.set("OPENAI_BASE_URL", llm_url);
        let mut c = conn();
        c.probe().await;
        let tools = IrisTools::with_registry_and_toolset(
            Some(c.clone()),
            SkillRegistry::new(),
            Toolset::Merged,
            None,
            None,
            true,
            Default::default(),
        )
        .unwrap();
        let mut pool = ConnectionPool::builder();
        pool.add("dev".to_string(), c, true);
        *tools.pool.write().unwrap() = Arc::new(pool.build());
        let rig = Rig {
            tools,
            conn: conn(),
            client: reqwest::Client::new(),
            _env: env,
        };
        rig.clean().await;
        rig
    }

    async fn run(&self, code: &str) -> String {
        let out = self
            .conn
            .execute_via_generator(code, NS, &self.client)
            .await
            .expect("execute request must reach IRIS");
        assert!(!is_generator_error(&out), "IRIS run failed: {out}");
        out.trim().to_string()
    }

    async fn clean(&self) {
        self.run(&format!(
            " Kill {MARKER}\n Do $system.OBJ.DeletePackage(\"IadGen141\",\"-d\")"
        ))
        .await;
    }

    /// Atelier PUT: stores the document and compiles nothing, with no iad gate in the way.
    async fn put(&self, doc: &str, src: &str) {
        let url = self.conn.versioned_ns_url(
            NS,
            &format!("/doc/{}?ignoreConflict=1", urlencoding::encode(doc)),
        );
        let lines: Vec<&str> = src.lines().collect();
        let resp = self
            .client
            .put(&url)
            .basic_auth(&self.conn.username, Some(&self.conn.password))
            .json(&json!({"enc": false, "content": lines}))
            .send()
            .await
            .expect("PUT must reach IRIS");
        assert!(resp.status().is_success(), "PUT {doc}: {}", resp.status());
    }

    /// The stored text of a document, or `None` when IRIS has no such document.
    async fn stored(&self, doc: &str) -> Option<String> {
        let url = self
            .conn
            .versioned_ns_url(NS, &format!("/doc/{}", urlencoding::encode(doc)));
        let resp = self
            .client
            .get(&url)
            .basic_auth(&self.conn.username, Some(&self.conn.password))
            .send()
            .await
            .expect("GET must reach IRIS");
        if resp.status().as_u16() == 404 {
            return None;
        }
        let body: Value = resp.json().await.unwrap_or_default();
        let lines = body["result"]["content"].as_array()?;
        Some(
            lines
                .iter()
                .filter_map(|l| l.as_str())
                .collect::<Vec<_>>()
                .join("\n"),
        )
    }

    async fn marker_set(&self) -> bool {
        self.run(&format!(" Write \"~[\",$Data({MARKER}),\"]~\""))
            .await
            .contains("~[1]~")
    }

    /// What `Class.Hello()` answers, or the empty string when the class does not run.
    async fn hello(&self, class: &str) -> String {
        self.run(&format!(
            " Try {{ Write \"~[\",##class({class}).Hello(),\"]~\" }} Catch e {{ Write \"~[\",e.Name,\"]~\" }}"
        ))
        .await
    }

    async fn generate(&self) -> (bool, Value) {
        let r = self
            .tools
            .call_gated(
                "iris_generate_class",
                json!({"description": "a class", "namespace": NS}),
            )
            .await
            .expect("the call must reach the tool");
        let body = r.structured_content.clone().unwrap_or_else(|| {
            r.content
                .iter()
                .find_map(|c| c.as_text())
                .and_then(|t| serde_json::from_str(&t.text).ok())
                .unwrap_or(json!({}))
        });
        (r.is_error == Some(true), body)
    }
}

impl Drop for Rig {
    /// Remove what a test stored, also when it failed half way.
    fn drop(&mut self) {
        let conn = self.conn.clone();
        // A client of its own: this test's client has connections owned by the runtime that is
        // blocked in `join` below, and a request on one of them never completes.
        let _ = std::thread::spawn(move || {
            let client = reqwest::Client::new();
            let rt = tokio::runtime::Runtime::new().unwrap();
            let _ = rt.block_on(conn.execute_via_generator(
                &format!(" Kill {MARKER}\n Do $system.OBJ.DeletePackage(\"IadGen141\",\"-d\")"),
                NS,
                &client,
            ));
        })
        .join();
    }
}

fn completion(text: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "choices": [{"message": {"role": "assistant", "content": text}}],
    }))
}

/// A stub model that answers each request with the next of `texts`, then the last one again.
async fn stub_llm(texts: &[String]) -> MockServer {
    let server = MockServer::start().await;
    for (i, text) in texts.iter().enumerate() {
        let mock = Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(completion(text));
        let mock = if i + 1 < texts.len() {
            mock.up_to_n_times(1).with_priority(i as u8 + 1)
        } else {
            mock.with_priority(i as u8 + 1)
        };
        mock.mount(&server).await;
    }
    server
}

#[tokio::test]
#[ignore = "requires live IRIS (iris-dev-iris)"]
async fn a_clean_generated_text_is_stored_and_compiled() {
    let llm = stub_llm(&[clean_class("IadGen141.Clean", "")]).await;
    let rig = Rig::new(&llm.uri()).await;
    assert!(
        rig.stored("IadGen141.Clean.cls").await.is_none(),
        "dirty start"
    );

    let (is_error, b) = rig.generate().await;

    assert!(!is_error, "{b}");
    assert_eq!(b["success"], json!(true), "{b}");
    assert_eq!(b["class_name"], "IadGen141.Clean", "{b}");
    assert_eq!(b["compiled"], json!(true), "{b}");
    assert_eq!(b["retried"], json!(false), "{b}");
    assert_eq!(b["compile_errors"], json!([]), "{b}");
    let stored = rig
        .stored("IadGen141.Clean.cls")
        .await
        .expect("the generated class must exist in IRIS afterwards");
    assert!(
        stored.contains("Quit 41"),
        "the stored text is the generated text: {stored}"
    );
    assert!(
        rig.hello("IadGen141.Clean").await.contains("~[41]~"),
        "the class must be compiled and callable"
    );
}

#[tokio::test]
#[ignore = "requires live IRIS (iris-dev-iris)"]
async fn a_generated_text_with_a_generator_is_refused_and_nothing_is_stored() {
    let llm = stub_llm(&[class_with_generator("IadGen141.Gen", "%RegisteredObject")]).await;
    let rig = Rig::new(&llm.uri()).await;

    let (is_error, b) = rig.generate().await;

    assert!(is_error, "a refusal is an error result: {b}");
    assert_eq!(b["error_code"], "COMPILE_TIME_EXEC_BLOCKED", "{b}");
    assert!(b.get("compiled").is_none(), "not a compile result: {b}");
    assert!(!rig.marker_set().await, "the generator must not have run");
    assert!(
        rig.stored("IadGen141.Gen.cls").await.is_none(),
        "a refused text is not stored"
    );
}

/// The defect itself. A class named like the model's text already exists, with a generator, stored
/// by a path the gate never saw. The old tool compiled that stored class and ran its generator.
#[tokio::test]
#[ignore = "requires live IRIS (iris-dev-iris)"]
async fn a_name_that_matches_a_stored_generator_class_replaces_it_and_runs_nothing() {
    let llm = stub_llm(&[clean_class("IadGen141.Victim", "")]).await;
    let rig = Rig::new(&llm.uri()).await;
    rig.put(
        "IadGen141.Victim.cls",
        &class_with_generator("IadGen141.Victim", ""),
    )
    .await;
    assert!(
        !rig.marker_set().await,
        "setup: nothing compiled the victim"
    );

    let (is_error, b) = rig.generate().await;

    assert!(!is_error, "{b}");
    assert_eq!(b["compiled"], json!(true), "{b}");
    assert!(
        !rig.marker_set().await,
        "the stored generator ran: the tool compiled the stored class, not the generated text"
    );
    let stored = rig.stored("IadGen141.Victim.cls").await.unwrap();
    assert!(
        !stored.contains("objectgenerator") && stored.contains("Quit 41"),
        "the generated text replaces the stored source: {stored}"
    );
    assert!(rig.hello("IadGen141.Victim").await.contains("~[41]~"));
}

#[tokio::test]
#[ignore = "requires live IRIS (iris-dev-iris)"]
async fn a_clean_text_that_extends_a_stale_user_superclass_is_refused() {
    let llm = stub_llm(&[clean_class("IadGen141.Child", "IadGen141.Pred")]).await;
    let rig = Rig::new(&llm.uri()).await;
    rig.put(
        "IadGen141.Pred.cls",
        &class_with_generator("IadGen141.Pred", ""),
    )
    .await;

    let (is_error, b) = rig.generate().await;

    assert!(is_error, "{b}");
    assert_eq!(b["error_code"], "COMPILE_SUPERCLASS_STALE", "{b}");
    assert!(
        b.to_string().contains("IadGen141.Pred"),
        "names the superclass: {b}"
    );
    assert!(
        !rig.marker_set().await,
        "compiling the child must not have compiled the stale superclass"
    );
}

#[tokio::test]
#[ignore = "requires live IRIS (iris-dev-iris)"]
async fn a_failed_compile_returns_the_errors_and_the_retry_is_stored_and_gated_the_same_way() {
    // First text: stores, does not compile. Retry text: clean and compiles.
    let llm = stub_llm(&[
        broken_class("IadGen141.Retry"),
        clean_class("IadGen141.Retry", ""),
    ])
    .await;
    let rig = Rig::new(&llm.uri()).await;

    let (is_error, b) = rig.generate().await;

    assert!(!is_error, "{b}");
    assert_eq!(b["retried"], json!(true), "{b}");
    assert_eq!(b["compiled"], json!(true), "{b}");
    assert!(b["class_text"].as_str().unwrap().contains("Quit 41"), "{b}");
    let requests = llm.received_requests().await.unwrap();
    assert_eq!(requests.len(), 2, "one generation and one retry");
    let retry = String::from_utf8_lossy(&requests[1].body).to_string();
    assert!(
        retry.contains("ERROR #"),
        "the retry prompt carries the errors IRIS returned, not a stock phrase: {retry}"
    );
    assert!(rig.hello("IadGen141.Retry").await.contains("~[41]~"));
}

#[tokio::test]
#[ignore = "requires live IRIS (iris-dev-iris)"]
async fn a_failed_compile_that_the_retry_does_not_fix_reports_the_errors() {
    let llm = stub_llm(&[broken_class("IadGen141.Broken")]).await;
    let rig = Rig::new(&llm.uri()).await;

    let (is_error, b) = rig.generate().await;

    assert!(!is_error, "{b}");
    assert_eq!(b["compiled"], json!(false), "{b}");
    assert_eq!(b["retried"], json!(true), "{b}");
    let errors = b["compile_errors"]
        .as_array()
        .expect("compile_errors is a list");
    assert!(
        !errors.is_empty() && errors[0].as_str().unwrap().contains("ERROR #"),
        "the response gains the errors IRIS returned: {b}"
    );
}

#[tokio::test]
#[ignore = "requires live IRIS (iris-dev-iris)"]
async fn a_retry_text_with_a_generator_is_refused_like_a_first_text() {
    let llm = stub_llm(&[
        broken_class("IadGen141.Retry2"),
        class_with_generator("IadGen141.Retry2", ""),
    ])
    .await;
    let rig = Rig::new(&llm.uri()).await;

    let (is_error, b) = rig.generate().await;

    assert!(is_error, "{b}");
    assert_eq!(b["error_code"], "COMPILE_TIME_EXEC_BLOCKED", "{b}");
    assert!(
        !rig.marker_set().await,
        "the retry's generator must not have run"
    );
    let stored = rig.stored("IadGen141.Retry2.cls").await.unwrap_or_default();
    assert!(
        !stored.contains("objectgenerator"),
        "the refused retry text is not stored: {stored}"
    );
}
