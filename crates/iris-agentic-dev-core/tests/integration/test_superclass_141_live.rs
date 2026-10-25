//! Spec 141 T209, FR-015a: a compile whose target has an out-of-date superclass outside the library
//! packages is refused, naming the superclass to compile first.
//!
//! IRIS compiles a target's out-of-date superclasses with it. The stored-source check reads the
//! classes named in the compile and nothing else, so a superclass stored by a path the gate never
//! saw (a straight Atelier PUT here) holds a `CodeMode = objectgenerator` method that runs when a
//! subclass is compiled.
//!
//! The first test measures that with no iad in the way, so the sentinel in the others is known to
//! be settable. The rest send the compile through every path that has one (`iris_compile` by name,
//! by wildcard, by local file, on a docker-only connection, and `iris_doc put` with compile) and
//! assert a refusal with `isError`, a sentinel that stays unset, and that compiling the superclass
//! and then the subclass succeeds. Everything stored is removed, also on failure.
//!
//! Requires `iris-dev-iris`: `IRIS_HOST=localhost IRIS_WEB_PORT=52780 IRIS_USERNAME=_SYSTEM
//! IRIS_PASSWORD=SYS`. Run serial, behind the shared lock.

use iris_agentic_dev_core::iris::connection::{
    is_generator_error, CompileResult, DiscoverySource, IrisConnection,
};
use iris_agentic_dev_core::testing::{live_env, require_iad_binary, McpSession};

const NS: &str = "USER";
const PRED: &str = "IadSup.Pred.cls";
const CHILD: &str = "IadSup.Child.cls";
const GRAND: &str = "IadSup.Grand.cls";

/// A generator that sets the sentinel for `tag` while it compiles.
fn generator(tag: &str) -> String {
    format!(
        "ClassMethod Gen() As %String [ CodeMode = objectgenerator ]\n{{\n    \
         Set ^IadSupProbe(\"{tag}\") = $USERNAME\n    \
         Do %code.WriteLine(\"    Quit 1\")\n    Quit $$$OK\n}}\n"
    )
}

/// A superclass with a generator for `tag`.
fn pred_with_generator(tag: &str) -> String {
    format!("Class IadSup.Pred\n{{\n\n{}\n}}\n", generator(tag))
}

/// The same superclass, fixed: no generator.
const PRED_CLEAN: &str =
    "Class IadSup.Pred\n{\n\nClassMethod Hello() As %String\n{\n    Quit 1\n}\n\n}\n";

const CHILD_SRC: &str =
    "Class IadSup.Child Extends IadSup.Pred\n{\n\nClassMethod Mine() As %String\n{\n    Quit 2\n}\n\n}\n";

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

    /// Compile straight over Atelier, no iad gate in the way.
    async fn compile(&self, doc: &str) -> CompileResult {
        self.conn
            .compile_document(doc, NS, "cuk", &self.client)
            .await
            .expect("compile request must reach IRIS")
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

    /// Has the sentinel for `tag` been set?
    async fn ran(&self, tag: &str) -> bool {
        let out = self
            .run(&format!(
                " Write \"~[\",$Data(^IadSupProbe(\"{tag}\")),\"]~\""
            ))
            .await;
        out.contains("~[1]~")
    }

    async fn clean(&self) {
        self.run(" Kill ^IadSupProbe\n Do $system.OBJ.DeletePackage(\"IadSup\",\"-d\")")
            .await;
    }

    /// The class dictionary's up-to-date answer for a class.
    async fn up_to_date(&self, class: &str) -> bool {
        self.run(&format!(
            " Write \"~[\",$SYSTEM.OBJ.IsUpToDate(\"{class}\"),\"]~\""
        ))
        .await
        .contains("~[1]~")
    }
}

impl Drop for Harness {
    /// Remove what a test stored, also when it failed half way.
    fn drop(&mut self) {
        let conn = self.conn.clone();
        let client = self.client.clone();
        let _ = std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            let _ = rt.block_on(conn.execute_via_generator(
                " Kill ^IadSupProbe\n Do $system.OBJ.DeletePackage(\"IadSup\",\"-d\")",
                NS,
                &client,
            ));
        })
        .join();
    }
}

fn open_gates() -> Vec<(String, String)> {
    let mut env = live_env();
    env.push(("IRIS_WRITE_TOOLS_ENABLED".to_string(), "1".to_string()));
    env.push((
        "IRIS_DESTRUCTIVE_TOOLS_ENABLED".to_string(),
        "1".to_string(),
    ));
    env
}

/// A session on a `docker_only` connection: no Atelier, every call goes through `docker exec`.
fn docker_only_session() -> (McpSession, tempfile::TempDir) {
    let container = iris_agentic_dev_core::testing::live_container();
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join(".iris-agentic-dev.toml"),
        format!("docker_only = true\ncontainer = \"{container}\"\nnamespace = \"USER\"\n"),
    )
    .unwrap();
    let env: Vec<(String, String)> = [
        ("IRIS_CONTAINER", container.as_str()),
        ("IRIS_USERNAME", "_SYSTEM"),
        ("IRIS_PASSWORD", "SYS"),
        ("IRIS_WRITE_TOOLS_ENABLED", "1"),
        ("IRIS_DESTRUCTIVE_TOOLS_ENABLED", "1"),
    ]
    .iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    (McpSession::start_in(Some(dir.path()), &env), dir)
}

fn is_error(answer: &serde_json::Value) -> bool {
    answer
        .pointer("/result/isError")
        .and_then(serde_json::Value::as_bool)
        == Some(true)
}

/// The tool's own JSON: `structuredContent` when present, else the first text block parsed.
fn body(answer: &serde_json::Value) -> serde_json::Value {
    let result = answer
        .get("result")
        .unwrap_or_else(|| panic!("no result in {answer}"));
    if let Some(sc) = result.get("structuredContent") {
        return sc.clone();
    }
    let text = result
        .pointer("/content/0/text")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_else(|| panic!("no content in {answer}"));
    serde_json::from_str(text).unwrap_or_else(|e| panic!("content was not JSON ({e}): {text}"))
}

fn short(v: &serde_json::Value) -> String {
    v.to_string().chars().take(300).collect()
}

/// Why `answer` is not a refusal for stale superclass(es) `names`, or `None` when it is one.
fn not_a_stale_refusal(answer: &serde_json::Value, names: &[&str]) -> Option<String> {
    if !is_error(answer) {
        return Some(format!("not an isError result: {}", short(answer)));
    }
    let b = body(answer);
    if b["error_code"] != "COMPILE_SUPERCLASS_STALE" {
        return Some(format!("wrong error_code: {}", short(answer)));
    }
    let text =
        b["message"].as_str().unwrap_or("").to_string() + b["remediation"].as_str().unwrap_or("");
    for n in names {
        if !text.contains(n) || !b["superclasses"].to_string().contains(n) {
            return Some(format!("{n} not named: {}", short(answer)));
        }
    }
    if b["refusal"]["kind"] != "fixed_rule" {
        return Some(format!("not a fixed rule: {}", short(answer)));
    }
    None
}

fn succeeded(answer: &serde_json::Value) -> Option<String> {
    (is_error(answer) || body(answer)["success"] != true)
        .then(|| format!("did not succeed: {}", short(answer)))
}

fn iris_compile(mcp: &mut McpSession, target: &str) -> serde_json::Value {
    mcp.call("iris_compile", &serde_json::json!({"target": target}))
}

/// Store a stale superclass with a generator and its subclass; nothing is compiled.
async fn store_stale_pair(h: &Harness, tag: &str) {
    h.clean().await;
    h.put(PRED, &pred_with_generator(tag)).await;
    h.put(CHILD, CHILD_SRC).await;
}

#[test]
#[ignore]
fn iris_compiles_a_stale_superclass_with_its_subclass() {
    // The measurement. If this fails, the other tests prove nothing.
    let rt = tokio::runtime::Runtime::new().unwrap();
    let h = Harness::new();
    rt.block_on(async {
        store_stale_pair(&h, "base").await;
        assert!(
            !h.ran("base").await,
            "storing the pair must compile nothing"
        );
        let r = h.compile(CHILD).await;
        let ran = h.ran("base").await;
        assert!(r.success(), "{:?}", r.errors);
        assert!(
            ran,
            "compiling the subclass did not compile its out-of-date superclass"
        );
    });
}

#[test]
#[ignore]
fn compiling_a_subclass_of_a_stale_superclass_is_refused_and_runs_nothing() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let rt = tokio::runtime::Runtime::new().unwrap();
    let h = Harness::new();
    let mut mcp = McpSession::start(&open_gates());
    let mut failures = Vec::new();

    // Never compiled, and compiled then changed: IRIS calls both out of date.
    for variant in ["never compiled", "changed after it was compiled"] {
        rt.block_on(async {
            h.clean().await;
            if variant == "never compiled" {
                h.put(PRED, &pred_with_generator("a")).await;
            } else {
                h.put(PRED, PRED_CLEAN).await;
                assert!(h.compile(PRED).await.success());
                h.put(PRED, &pred_with_generator("a")).await;
            }
            h.put(CHILD, CHILD_SRC).await;
            assert!(
                !h.up_to_date("IadSup.Pred").await,
                "{variant}: the superclass must be out of date for this case"
            );
        });
        // Every spelling of the target.
        for target in [CHILD, "IadSup.Child", "IadSup.Chi*"] {
            let answer = iris_compile(&mut mcp, target);
            if let Some(why) = not_a_stale_refusal(&answer, &["IadSup.Pred"]) {
                failures.push(format!("{variant}, {target}: {why}"));
            }
            if rt.block_on(h.ran("a")) {
                failures.push(format!("{variant}, {target}: the generator ran"));
            }
        }
    }
    rt.block_on(h.clean());
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

#[test]
#[ignore]
fn the_superclass_is_compiled_through_the_gate_and_then_the_subclass_compiles() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let rt = tokio::runtime::Runtime::new().unwrap();
    let h = Harness::new();
    let mut mcp = McpSession::start(&open_gates());
    let mut failures = Vec::new();
    rt.block_on(store_stale_pair(&h, "b"));

    // The refusal sends the agent to the superclass; that compile is judged like any other, and its
    // generator is refused by the compile-time gate.
    let answer = iris_compile(&mut mcp, "IadSup.Pred");
    if !is_error(&answer) || body(&answer)["error_code"] != "COMPILE_TIME_EXEC_BLOCKED" {
        failures.push(format!(
            "the superclass's own check did not run: {}",
            short(&answer)
        ));
    }
    if rt.block_on(h.ran("b")) {
        failures.push("the generator ran".to_string());
    }

    // The agent fixes the superclass, compiles it, then the subclass.
    rt.block_on(h.put(PRED, PRED_CLEAN));
    if let Some(why) = succeeded(&iris_compile(&mut mcp, "IadSup.Pred")) {
        failures.push(format!("clean superclass: {why}"));
    }
    if let Some(why) = succeeded(&iris_compile(&mut mcp, "IadSup.Child")) {
        failures.push(format!("subclass after its superclass: {why}"));
    }
    // And once more with everything up to date.
    if let Some(why) = succeeded(&iris_compile(&mut mcp, "IadSup.Child")) {
        failures.push(format!("subclass, all up to date: {why}"));
    }
    if rt.block_on(h.ran("b")) {
        failures.push("the generator ran".to_string());
    }
    rt.block_on(h.clean());
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

#[test]
#[ignore]
fn a_stale_grandparent_and_a_stale_second_parent_are_found() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let rt = tokio::runtime::Runtime::new().unwrap();
    let h = Harness::new();
    let mut mcp = McpSession::start(&open_gates());
    let mut failures = Vec::new();

    // Chain: Child -> Pred -> Grand. Pred is compiled and clean; Grand is changed after it was
    // compiled and now holds a generator.
    rt.block_on(async {
        h.clean().await;
        let grand_clean = "Class IadSup.Grand\n{\n\nClassMethod G() As %String\n{\n    Quit 0\n}\n\n}\n";
        h.put(GRAND, grand_clean).await;
        h.put(
            PRED,
            "Class IadSup.Pred Extends IadSup.Grand\n{\n\nClassMethod Hello() As %String\n{\n    Quit 1\n}\n\n}\n",
        )
        .await;
        h.put(CHILD, CHILD_SRC).await;
        for doc in [GRAND, PRED, CHILD] {
            assert!(h.compile(doc).await.success(), "{doc}");
        }
        h.put(
            GRAND,
            &format!("Class IadSup.Grand\n{{\n\n{}\n}}\n", generator("g")),
        )
        .await;
    });
    let answer = iris_compile(&mut mcp, "IadSup.Child");
    if let Some(why) = not_a_stale_refusal(&answer, &["IadSup.Grand"]) {
        failures.push(format!("grandparent: {why}"));
    }
    if rt.block_on(h.ran("g")) {
        failures.push("grandparent: the generator ran".to_string());
    }

    // Multiple inheritance: the second parent is the stale one, the first is up to date.
    rt.block_on(async {
        h.clean().await;
        h.put(
            "IadSup.Fine.cls",
            "Class IadSup.Fine\n{\n\nClassMethod F() As %String\n{\n    Quit 0\n}\n\n}\n",
        )
        .await;
        assert!(h.compile("IadSup.Fine.cls").await.success());
        h.put(PRED, &pred_with_generator("m")).await;
        h.put(
            CHILD,
            "Class IadSup.Child Extends (IadSup.Fine, IadSup.Pred)\n{\n}\n",
        )
        .await;
    });
    let answer = iris_compile(&mut mcp, "IadSup.Child");
    if let Some(why) = not_a_stale_refusal(&answer, &["IadSup.Pred"]) {
        failures.push(format!("second parent: {why}"));
    }
    if body(&answer)["superclasses"]
        .to_string()
        .contains("IadSup.Fine")
    {
        failures.push(format!(
            "the up-to-date parent was named: {}",
            short(&answer)
        ));
    }
    if rt.block_on(h.ran("m")) {
        failures.push("second parent: the generator ran".to_string());
    }
    rt.block_on(h.clean());
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

/// A superclass with no package, which IRIS files under `User`, is the case the first measurement
/// was made on.
#[test]
#[ignore]
fn a_superclass_with_no_package_is_found() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let rt = tokio::runtime::Runtime::new().unwrap();
    let h = Harness::new();
    let mut mcp = McpSession::start(&open_gates());
    let mut failures = Vec::new();
    let clean_top = || {
        " Kill ^IadSupProbe\n Do $system.OBJ.DeletePackage(\"IadSup\",\"-d\")\n \
         Do $system.OBJ.Delete(\"IadSupTopPred,IadSupTopChild\",\"-d\")"
    };
    rt.block_on(async {
        h.run(clean_top()).await;
        h.put(
            "IadSupTopPred.cls",
            &format!("Class IadSupTopPred\n{{\n\n{}\n}}\n", generator("top")),
        )
        .await;
        h.put(
            "IadSupTopChild.cls",
            "Class IadSupTopChild Extends IadSupTopPred\n{\n}\n",
        )
        .await;
    });
    let answer = iris_compile(&mut mcp, "IadSupTopChild");
    if let Some(why) = not_a_stale_refusal(&answer, &["IadSupTopPred"]) {
        failures.push(why);
    }
    if rt.block_on(h.ran("top")) {
        failures.push("the generator ran".to_string());
    }
    rt.block_on(h.run(clean_top()));
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

#[test]
#[ignore]
fn library_and_up_to_date_superclasses_do_not_refuse() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let rt = tokio::runtime::Runtime::new().unwrap();
    let h = Harness::new();
    let mut mcp = McpSession::start(&open_gates());
    let mut failures = Vec::new();
    rt.block_on(h.clean());
    // (document, source). `Ens.BusinessOperation` is the interop parent; `%Persistent` the % one.
    for (doc, src) in [
        (
            "IadSup.Pers.cls",
            "Class IadSup.Pers Extends (%Persistent, %Populate)\n{\n\nProperty Name As %String;\n\n}\n",
        ),
        (
            "IadSup.Reg.cls",
            "Class IadSup.Reg Extends %RegisteredObject\n{\n}\n",
        ),
        (
            "IadSup.Op.cls",
            "Class IadSup.Op Extends Ens.BusinessOperation\n{\n\nParameter INVOCATION = \"Queue\";\n\n}\n",
        ),
    ] {
        rt.block_on(h.put(doc, src));
        let answer = iris_compile(&mut mcp, doc);
        if let Some(why) = succeeded(&answer) {
            failures.push(format!("{doc}: {why}"));
        }
    }
    // A user superclass that is up to date.
    rt.block_on(async {
        h.put(PRED, PRED_CLEAN).await;
        h.put(CHILD, CHILD_SRC).await;
        assert!(h.compile(PRED).await.success());
    });
    if let Some(why) = succeeded(&iris_compile(&mut mcp, "IadSup.Child")) {
        failures.push(format!("up-to-date user superclass: {why}"));
    }
    // A package compile whose members include the stale superclass compiles it itself, and the
    // stored-source check has read it.
    rt.block_on(async {
        h.put(
            PRED,
            "Class IadSup.Pred\n{\n\nClassMethod Hello() As %String\n{\n    Quit 3\n}\n\n}\n",
        )
        .await;
    });
    if let Some(why) = succeeded(&iris_compile(&mut mcp, "IadSup.*.cls")) {
        failures.push(format!("package with a stale member: {why}"));
    }
    rt.block_on(h.clean());
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

#[test]
#[ignore]
fn iris_doc_put_with_compile_and_a_local_file_are_checked_too() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let rt = tokio::runtime::Runtime::new().unwrap();
    let h = Harness::new();
    let mut mcp = McpSession::start(&open_gates());
    let mut failures = Vec::new();

    // iris_doc put with compile=true: the new subclass would compile the stale superclass.
    rt.block_on(async {
        h.clean().await;
        h.put(PRED, &pred_with_generator("d")).await;
    });
    let answer = mcp.call(
        "iris_doc",
        &serde_json::json!({"mode": "put", "name": CHILD, "content": CHILD_SRC, "compile": true}),
    );
    if let Some(why) = not_a_stale_refusal(&answer, &["IadSup.Pred"]) {
        failures.push(format!("iris_doc put: {why}"));
    }
    if rt.block_on(h.ran("d")) {
        failures.push("iris_doc put: the generator ran".to_string());
    }
    // The same put without compile writes and compiles nothing.
    let answer = mcp.call(
        "iris_doc",
        &serde_json::json!({"mode": "put", "name": CHILD, "content": CHILD_SRC}),
    );
    if let Some(why) = succeeded(&answer) {
        failures.push(format!("iris_doc put without compile: {why}"));
    }

    // A local file.
    rt.block_on(async {
        h.clean().await;
        h.put(PRED, &pred_with_generator("f")).await;
    });
    // The file is in the session's project directory: a host file outside it is refused before
    // the superclass check is reached (T212).
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("IadSupChildFile.cls");
    std::fs::write(&path, CHILD_SRC).unwrap();
    let mut in_project = McpSession::start_in(Some(dir.path()), &open_gates());
    let answer = iris_compile(&mut in_project, path.to_str().unwrap());
    if let Some(why) = not_a_stale_refusal(&answer, &["IadSup.Pred"]) {
        failures.push(format!("local file: {why}"));
    }
    if rt.block_on(h.ran("f")) {
        failures.push("local file: the generator ran".to_string());
    }
    rt.block_on(h.clean());
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

#[test]
#[ignore]
fn the_docker_exec_path_refuses_too() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let rt = tokio::runtime::Runtime::new().unwrap();
    let h = Harness::new();
    let (mut mcp, _dir) = docker_only_session();
    let mut failures = Vec::new();
    rt.block_on(store_stale_pair(&h, "x"));
    for target in [CHILD, "IadSup.Child", "IadSup.Chi*"] {
        let answer = iris_compile(&mut mcp, target);
        if let Some(why) = not_a_stale_refusal(&answer, &["IadSup.Pred"]) {
            failures.push(format!("{target}: {why}"));
        }
        if rt.block_on(h.ran("x")) {
            failures.push(format!("{target}: the generator ran"));
        }
    }
    // After the fix, both compile through the exec path.
    rt.block_on(h.put(PRED, PRED_CLEAN));
    for target in ["IadSup.Pred", "IadSup.Child"] {
        let answer = iris_compile(&mut mcp, target);
        if is_error(&answer) || body(&answer)["success"] != true {
            failures.push(format!("{target} after the fix: {}", short(&answer)));
        }
    }
    rt.block_on(h.clean());
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}
