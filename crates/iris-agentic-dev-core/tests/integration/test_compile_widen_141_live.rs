//! Spec 141 T206: a compile qualifier that widens the set compiled cannot reach a stored class the
//! compile-time gate never read.
//!
//! `iris_compile` by name reads the stored source of its target and of the `.inc` files the target
//! includes. IRIS compiles more than the target when asked to: the flag `b` and the qualifiers
//! `/subclasses` and `/expand` add every subclass. A subclass stored by a path the gate does not
//! see (here a straight Atelier PUT) holds a `CodeMode = objectgenerator` method; compiling its
//! superclass with a widening qualifier compiles it and runs the generator.
//!
//! The first test measures what IRIS does with no iad in the way, so the sentinel in the second is
//! known to be settable. The second sends each widening spelling through `iris_compile` and
//! asserts a refusal and a sentinel that stays unset. Everything stored is removed, also on
//! failure.
//!
//! Requires `iris-dev-iris`: `IRIS_HOST=localhost IRIS_WEB_PORT=52780 IRIS_USERNAME=_SYSTEM
//! IRIS_PASSWORD=SYS`. Run serial, behind the shared lock.

use iris_agentic_dev_core::iris::connection::{
    is_generator_error, DiscoverySource, IrisConnection,
};
use iris_agentic_dev_core::testing::{live_env, require_iad_binary, McpSession};

const NS: &str = "USER";
const SUPER: &str = "IadWiden.Super.cls";
const SUB: &str = "IadWiden.Sub.cls";

const SUPER_SRC: &str =
    "Class IadWiden.Super\n{\n\nClassMethod Hello() As %String\n{\n    Quit 1\n}\n\n}\n";

/// The subclass: a generator that sets the sentinel while it compiles.
const SUB_SRC: &str = "Class IadWiden.Sub Extends IadWiden.Super\n{\n\nClassMethod Gen() As %String [ CodeMode = objectgenerator ]\n{\n    Set ^IadWidenProbe(\"sub\") = $USERNAME\n    Do %code.WriteLine(\"    Quit 1\")\n    Quit $$$OK\n}\n\n}\n";

struct Harness {
    conn: IrisConnection,
    client: reqwest::Client,
}

impl Harness {
    fn new() -> Self {
        let host = std::env::var("IRIS_HOST").unwrap_or_default();
        assert!(
            !host.is_empty(),
            "IRIS_HOST unset. Set IRIS_HOST=localhost IRIS_WEB_PORT=52780 IRIS_USERNAME=_SYSTEM \
             IRIS_PASSWORD=SYS"
        );
        let port = std::env::var("IRIS_WEB_PORT").unwrap_or_else(|_| "52780".into());
        let user = std::env::var("IRIS_USERNAME").unwrap_or_else(|_| "_SYSTEM".into());
        let pass = std::env::var("IRIS_PASSWORD").unwrap_or_else(|_| "SYS".into());
        Self {
            conn: IrisConnection::new(
                format!("http://{host}:{port}"),
                NS,
                user,
                pass,
                DiscoverySource::EnvVar,
            ),
            client: reqwest::Client::new(),
        }
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
            .json(&serde_json::json!({"enc": false, "content": lines}))
            .send()
            .await
            .expect("PUT must reach IRIS");
        assert!(resp.status().is_success(), "PUT {doc}: {}", resp.status());
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

    /// Is the sentinel set?
    async fn ran(&self) -> bool {
        let out = self
            .run(" Write \"~[\",$Data(^IadWidenProbe(\"sub\")),\"]~\"")
            .await;
        out.contains("~[1]~")
    }

    /// Remove the documents, their compiled forms and the sentinel. Before and after each test.
    async fn clean(&self) {
        self.run(" Kill ^IadWidenProbe\n Do $system.OBJ.DeletePackage(\"IadWiden\",\"-d\")")
            .await;
    }

    /// Store both classes, compile the superclass alone, and clear any sentinel that set.
    async fn install(&self) {
        self.clean().await;
        self.put(SUPER, SUPER_SRC).await;
        self.put(SUB, SUB_SRC).await;
        let r = self
            .conn
            .compile_document(SUPER, NS, "cuk", &self.client)
            .await
            .expect("compile must reach IRIS");
        assert!(r.success(), "the superclass must compile: {:?}", r.errors);
        assert!(
            !self.ran().await,
            "compiling the superclass alone must not compile the subclass"
        );
    }
}

/// Qualifiers and flags IRIS reads as "include the subclasses": the flag in either case, the
/// qualifier in any case, abbreviated, with a value, and through the `/expand` alias.
const WIDENING: &[&str] = &[
    "cukb",
    "ckB",
    "cuk/subclasses",
    "ck/SUBCLASSES",
    "ck/sub",
    "ck/subclasses=1",
    "ck/expand",
    "ck/expan=subclasses",
    "ck/checkuptodate=none/subclasses",
];

#[test]
#[ignore]
fn iris_compiles_stored_subclasses_when_asked_to() {
    // The measurement. If this fails, the other test proves nothing.
    let rt = tokio::runtime::Runtime::new().unwrap();
    let h = Harness::new();
    rt.block_on(async {
        h.install().await;
        let r = h
            .conn
            .compile_document(SUPER, NS, "cukb", &h.client)
            .await
            .expect("compile must reach IRIS");
        let ran = h.ran().await;
        h.clean().await;
        assert!(r.success(), "{:?}", r.errors);
        assert!(
            ran,
            "compiling the superclass with the flag b did not compile the stored subclass"
        );
    });
}

#[test]
#[ignore]
fn iris_compile_refuses_a_qualifier_that_adds_classes_the_gate_never_read() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let rt = tokio::runtime::Runtime::new().unwrap();
    let h = Harness::new();
    let mut mcp = McpSession::start(&live_env());
    let mut failures = Vec::new();
    rt.block_on(h.install());

    // The control: the same call with no widening compiles the superclass and nothing else.
    let answer = mcp.call(
        "iris_compile",
        &serde_json::json!({"target": SUPER, "flags": "cuk"}),
    );
    if answer.pointer("/result/isError").and_then(|v| v.as_bool()) == Some(true) {
        failures.push(format!("control refused: {answer}"));
    }
    if rt.block_on(h.ran()) {
        failures.push("control: the subclass compiled without a widening qualifier".to_string());
    }

    for flags in WIDENING {
        rt.block_on(h.install());
        let answer = mcp.call(
            "iris_compile",
            &serde_json::json!({"target": SUPER, "flags": flags}),
        );
        let text = answer.to_string();
        if !text.contains("INVALID_PARAMS") {
            let shown: String = text.chars().take(200).collect();
            failures.push(format!("{flags:?}: not refused as INVALID_PARAMS: {shown}"));
        }
        if rt.block_on(h.ran()) {
            failures.push(format!(
                "{flags:?}: the stored subclass compiled and its generator ran"
            ));
        }
    }
    rt.block_on(h.clean());
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}
