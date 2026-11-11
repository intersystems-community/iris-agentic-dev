//! Spec 123 US3: the published todo-app example still builds and works, driven through iad.
//!
//! `docs/examples/todo-app/` claims two classes and one web application make a working app. This
//! test does what `STEPS.md` does, through a real MCP session so every call meets the gate in
//! `call_tool`: put and compile both classes, create the web application, then exercise step 5
//! over HTTP and watch the outstanding count move.
//!
//! The live demo sits at `Demo.*` on `/todo` and has to survive, so the test rewrites the package
//! to `IADEx123.` and serves it at `/iadex123-todo` (research R5). Every reference, including the
//! `^Demo.TodoD` storage globals, shares the prefix, and the test refuses to start if a `Demo.`
//! survives the rewrite.
//!
//! Requires `iris-dev-iris`: `IRIS_HOST=localhost IRIS_WEB_PORT=52780 IRIS_USERNAME=_SYSTEM
//! IRIS_PASSWORD=SYS`. The test sets the write, destructive and admin tiers in the server it
//! spawns, so the tiers are never the reason it cannot run.

use std::path::{Path, PathBuf};

use iris_agentic_dev_core::testing::{answer_text, live_env, require_iad_binary, McpSession};

const PACKAGE: &str = "IADEx123";
const WEB_PATH: &str = "/iadex123-todo";
const NAMESPACE: &str = "USER";

fn example_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("CARGO_MANIFEST_DIR should be <root>/crates/<crate>")
        .join("docs/examples/todo-app")
}

/// A published class with its package renamed. Panics if any `Demo.` is left, since a missed
/// reference would reach into the live demo's classes or globals.
fn renamed(file: &str) -> String {
    let path = example_dir().join(file);
    let source = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    assert!(
        source.contains("Demo."),
        "{file} no longer uses the Demo package"
    );
    let out = source.replace("Demo.", &format!("{PACKAGE}."));
    assert!(
        !out.contains("Demo."),
        "{file}: a `Demo.` reference survived the rename; running would touch the live demo"
    );
    out
}

fn tier_env() -> Vec<(String, String)> {
    let mut env = live_env();
    env.retain(|(k, _)| k != "IRIS_NAMESPACE");
    for (k, v) in [
        ("IRIS_NAMESPACE", NAMESPACE),
        ("IRIS_WRITE_TOOLS_ENABLED", "1"),
        ("IRIS_DESTRUCTIVE_TOOLS_ENABLED", "1"),
        ("IRIS_ADMIN_TOOLS", "1"),
    ] {
        env.push((k.to_string(), v.to_string()));
    }
    env
}

/// The parsed `result.content[0].text`, or `None` for a refusal or a non-JSON answer.
fn payload(answer: &serde_json::Value) -> Option<serde_json::Value> {
    let text = answer.pointer("/result/content/0/text")?.as_str()?;
    serde_json::from_str(text).ok()
}

fn is_error(answer: &serde_json::Value) -> bool {
    answer.get("error").is_some()
        || answer.pointer("/result/isError") == Some(&serde_json::json!(true))
        || payload(answer)
            .and_then(|p| p.get("success").cloned())
            .is_some_and(|s| s == serde_json::json!(false))
}

/// Owns the session and removes everything the test made, on success or panic.
struct Example {
    session: McpSession,
}

impl Example {
    fn call_ok(&mut self, tool: &str, args: serde_json::Value) -> serde_json::Value {
        let answer = self.session.call(tool, &args);
        assert!(
            !is_error(&answer),
            "{tool} {args} failed: {}",
            answer_text(&answer)
        );
        answer
    }

    /// Delete the web application, the rows and both classes. Each step ignores not-found, so this
    /// also serves as the pre-clean after an aborted run.
    fn teardown(&mut self) {
        let _ = self.session.call(
            "iris_admin",
            &serde_json::json!({"action": "delete_webapp", "path": WEB_PATH}),
        );
        let _ = self.session.call(
            "iris_execute",
            &serde_json::json!({
                "code": format!(
                    "If ##class(%Dictionary.CompiledClass).%ExistsId(\"{PACKAGE}.Todo\") \
                     {{ Do ##class({PACKAGE}.Todo).%KillExtent() }}"
                ),
                "namespace": NAMESPACE
            }),
        );
        let _ = self.session.call(
            "iris_doc",
            &serde_json::json!({
                "mode": "delete",
                "names": [format!("{PACKAGE}.TodoREST.cls"), format!("{PACKAGE}.Todo.cls")],
                "namespace": NAMESPACE
            }),
        );
    }

    /// What the test made that is still on the server. Classes are checked with `iris_doc`, since
    /// `iris_execute` refuses any reference to `%Dictionary.*Definition` (the code-edit gate).
    fn leftovers(&mut self) -> Vec<String> {
        let mut left = Vec::new();
        for class in ["Todo", "TodoREST"] {
            let name = format!("{PACKAGE}.{class}.cls");
            let answer = self.session.call(
                "iris_doc",
                &serde_json::json!({"mode": "get", "name": name, "namespace": NAMESPACE}),
            );
            if !is_error(&answer) {
                left.push(format!("class {name}"));
            }
        }
        let answer = self.session.call(
            "iris_execute",
            &serde_json::json!({
                "code": format!(
                    "Write \"global=\",$Data(^{PACKAGE}.TodoD),! \
                     New $Namespace Set $Namespace=\"%SYS\" \
                     Write \"webapp=\",##class(Security.Applications).Exists(\"{WEB_PATH}\"),!"
                ),
                "namespace": NAMESPACE
            }),
        );
        let out = payload(&answer)
            .and_then(|p| p.get("output").and_then(|o| o.as_str()).map(str::to_string))
            .unwrap_or_else(|| panic!("leftover check failed: {}", answer_text(&answer)));
        for probe in ["global", "webapp"] {
            assert!(
                out.contains(&format!("{probe}=")),
                "leftover probe printed no {probe}: {out}"
            );
            if !out.contains(&format!("{probe}=0")) {
                left.push(format!("{probe}: {}", out.trim()));
            }
        }
        left
    }
}

impl Drop for Example {
    fn drop(&mut self) {
        self.teardown();
    }
}

/// A plain HTTP client against the instance's web port, with the credentials the test runs as.
struct App {
    base: String,
    user: String,
    password: String,
    client: reqwest::Client,
    rt: tokio::runtime::Runtime,
}

impl App {
    fn new() -> Self {
        let var = |k: &str, d: &str| std::env::var(k).unwrap_or_else(|_| d.to_string());
        let base = format!(
            "{}://{}:{}{WEB_PATH}",
            var("IRIS_SCHEME", "http"),
            var("IRIS_HOST", "localhost"),
            var("IRIS_WEB_PORT", "52780"),
        );
        Self {
            base,
            user: var("IRIS_USERNAME", "_SYSTEM"),
            password: var("IRIS_PASSWORD", "SYS"),
            client: iris_agentic_dev_core::testing::live_client(),
            rt: tokio::runtime::Runtime::new().expect("tokio runtime"),
        }
    }

    fn send(&self, method: reqwest::Method, route: &str, form: &[(&str, &str)]) -> String {
        let url = format!("{}{route}", self.base);
        let mut req = self
            .client
            .request(method.clone(), &url)
            .basic_auth(&self.user, Some(&self.password));
        if !form.is_empty() {
            req = req.form(form);
        }
        self.rt.block_on(async {
            let resp = req
                .send()
                .await
                .unwrap_or_else(|e| panic!("{method} {url}: {e}"));
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            assert!(status.is_success(), "{method} {url} → {status}: {body}");
            body
        })
    }
}

/// The header count in a list fragment: `<div class='sub'>N outstanding</div>`, or 0 when the
/// fragment says nothing is outstanding.
fn outstanding(fragment: &str) -> u64 {
    let re = regex::Regex::new(r"<div class='sub'>(\d+) outstanding</div>").expect("count regex");
    match re.captures(fragment) {
        Some(c) => c[1].parse().expect("count"),
        None => {
            assert!(
                fragment.contains("Nothing outstanding"),
                "no outstanding count in the fragment: {fragment}"
            );
            0
        }
    }
}

/// The id of the row whose title is `title`, read from its toggle button.
fn row_id(fragment: &str, title: &str) -> String {
    let re = regex::Regex::new(&format!(
        r"(?s)hx-post='toggle/(\d+)'[^<]*>\s*<span class='t'>{}</span>",
        regex::escape(title)
    ))
    .expect("row regex");
    re.captures(fragment)
        .map(|c| c[1].to_string())
        .unwrap_or_else(|| panic!("no row titled {title:?} in: {fragment}"))
}

#[test]
#[ignore = "requires live IRIS (iris-dev-iris) and the built binary"]
fn the_published_todo_example_builds_and_works() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let todo = renamed("Demo.Todo.cls");
    let rest = renamed("Demo.TodoREST.cls");

    let mut ex = Example {
        session: McpSession::start(&tier_env()),
    };
    ex.teardown();

    // STEPS.md 1 and 3: put and compile both classes, in dependency order.
    for (name, content) in [
        (format!("{PACKAGE}.Todo.cls"), &todo),
        (format!("{PACKAGE}.TodoREST.cls"), &rest),
    ] {
        ex.call_ok(
            "iris_doc",
            serde_json::json!({
                "mode": "put", "name": name, "content": content,
                "compile": true, "namespace": NAMESPACE
            }),
        );
    }

    // STEPS.md 4: the web application, through the tool whose tier the demo tripped over.
    ex.call_ok(
        "iris_admin",
        serde_json::json!({
            "action": "create_webapp", "path": WEB_PATH, "namespace": NAMESPACE,
            "dispatch_class": format!("{PACKAGE}.TodoREST"), "enabled": true
        }),
    );

    // STEPS.md 5: the page, the fragment, add, toggle, delete.
    let app = App::new();
    let page = app.send(reqwest::Method::GET, "/", &[]);
    assert!(
        page.contains("hx-get='rows'"),
        "the page does not load the list fragment: {page}"
    );

    let before = outstanding(&app.send(reqwest::Method::GET, "/rows", &[]));

    let title = "round trip from spec 123";
    let added = app.send(
        reqwest::Method::POST,
        "/add",
        &[("title", title), ("priority", "high")],
    );
    assert_eq!(
        outstanding(&added),
        before + 1,
        "add did not raise the count"
    );
    let id = row_id(&added, title);

    let toggled = app.send(reqwest::Method::POST, &format!("/toggle/{id}"), &[]);
    assert_eq!(
        outstanding(&toggled),
        before,
        "toggle did not lower the count"
    );

    let deleted = app.send(reqwest::Method::DELETE, &format!("/del/{id}"), &[]);
    assert!(
        !deleted.contains(&format!("<span class='t'>{title}</span>")),
        "delete left the row in place: {deleted}"
    );

    // Titles are escaped, as STEPS.md 3 says.
    let raw = "<b>x</b> & \"q\"";
    let escaped = app.send(
        reqwest::Method::POST,
        "/add",
        &[("title", raw), ("priority", "low")],
    );
    assert!(
        escaped.contains("&lt;b&gt;x&lt;/b&gt; &amp; &quot;q&quot;"),
        "title was not HTML-escaped: {escaped}"
    );

    // The leftover probe must see the build while it is still there, or its empty answer after
    // teardown proves nothing.
    let present = ex.leftovers();
    assert_eq!(
        present.len(),
        4,
        "before teardown the probe should see both classes, the global and the web app: {present:?}"
    );

    // Teardown runs in Drop; run it here too so the leftover check sees its effect.
    ex.teardown();
    let left = ex.leftovers();
    assert!(left.is_empty(), "teardown left something behind: {left:?}");
}
