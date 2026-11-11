//! Spec 132: AI Hub claims first measured on the licensed 2026.3.0AI.139 instance. Since 2026-10-02
//! `iad-aihub-iris` runs build 154; the names keep 139.
//!
//! Every test here talks to `iad-aihub-iris` through `iad-aihub-webgateway` on 52781, via a spawned
//! iad over HTTP (`testing::aihub_session`). With the instance down each one panics naming the
//! container; `IAD_ALLOW_SKIP=1` turns that into a printed skip. The claims each test backs are in
//! the claim table in `specs/132-aihub-139/research.md`.
//!
//! Run with (quickstart.md):
//!   IAD_BINARY=$PWD/target/debug/iris-agentic-dev IAD_AIHUB_WEB_PORT=52781 \
//!   cargo test --features testing --test integration test_aihub_139 -- \
//!     --include-ignored --test-threads=1

use iris_agentic_dev_core::testing::{
    aihub_env, aihub_session, aihub_vars, answer_text, AihubEnv, McpSession,
};

/// The tool's own JSON, out of the JSON-RPC envelope.
fn payload(answer: &serde_json::Value) -> serde_json::Value {
    let text = answer
        .pointer("/result/content/0/text")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_else(|| panic!("no text content in {}", answer_text(answer)));
    serde_json::from_str(text).unwrap_or_else(|e| panic!("content was not JSON ({e}): {text}"))
}

/// A session with the write and destructive gates open, for tests that put and delete classes.
fn writable(env: &AihubEnv) -> McpSession {
    let mut vars = aihub_vars(env);
    vars.push(("IRIS_WRITE_TOOLS_ENABLED".into(), "1".into()));
    vars.push(("IRIS_DESTRUCTIVE_TOOLS_ENABLED".into(), "1".into()));
    McpSession::start(&vars)
}

/// Run ObjectScript over `iris_execute` and return its output; a failed run fails the test.
fn exec(mcp: &mut McpSession, code: &str) -> String {
    let got = payload(&mcp.call("iris_execute", &serde_json::json!({ "code": code })));
    assert_eq!(
        got["success"], true,
        "iris_execute failed for {code:?}: {got}"
    );
    got["output"]
        .as_str()
        .unwrap_or_default()
        .trim()
        .to_string()
}

// ── US1: a 139 instance the tests can reach ─────────────────────────────────

#[test]
#[ignore = "live iad-aihub-iris"]
fn aihub_version_is_154_or_later() {
    let Some(env) = aihub_env() else { return };
    let mut mcp = aihub_session(&env);
    let got = payload(&mcp.call("iris_info", &serde_json::json!({"what": "metadata"})));
    let version = got
        .pointer("/result/content/version")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_else(|| panic!("no version in iris_info metadata: {got}"));
    assert!(version.contains("2026.3.0AI"), "{version}");
    // The container moved from 139 to 154 on 2026-10-02. The tests' names still say 139, the
    // build the claims were first measured on.
    let build: u32 = version
        .split("Build ")
        .nth(1)
        .and_then(|b| b.split(|c: char| !c.is_ascii_digit()).next())
        .and_then(|b| b.parse().ok())
        .unwrap_or_else(|| panic!("no build number in {version}"));
    assert!(build >= 154, "want build 154 or later: {version}");
}

#[test]
#[ignore = "live iad-aihub-iris"]
fn aihub_has_ai_package() {
    let Some(env) = aihub_env() else { return };
    let mut mcp = aihub_session(&env);
    let got = payload(&mcp.call(
        "iris_query",
        &serde_json::json!({
            "query": "SELECT COUNT(*) AS n FROM %Dictionary.CompiledClass WHERE ID %STARTSWITH '%AI.'"
        }),
    ));
    assert_eq!(got["namespace"], env.namespace, "{got}");
    let n = got
        .pointer("/rows/0/n")
        .and_then(serde_json::Value::as_i64)
        .unwrap_or_else(|| panic!("no count row: {got}"));
    assert!(n >= 60, "%AI classes visible from USER: {n}, want >= 60");
}

const PROBE: &str = "IadAihub139.Probe.cls";
const PROBE_SRC: &str = r#"Class IadAihub139.Probe Extends %AI.Agent
{

ClassMethod Ping() As %String
{
    Quit "pong"
}

}
"#;

/// Deletes the probe class when the test ends, pass or panic.
struct DeleteOnDrop<'a> {
    mcp: &'a mut McpSession,
    doc: &'static str,
}

impl Drop for DeleteOnDrop<'_> {
    fn drop(&mut self) {
        let _ = self.mcp.call(
            "iris_doc",
            &serde_json::json!({"mode": "delete", "name": self.doc}),
        );
    }
}

#[test]
#[ignore = "live iad-aihub-iris"]
fn aihub_http_path() {
    let Some(env) = aihub_env() else { return };
    let mut mcp = writable(&env);
    let guard = DeleteOnDrop {
        mcp: &mut mcp,
        doc: PROBE,
    };
    let put = payload(&guard.mcp.call(
        "iris_doc",
        &serde_json::json!({"mode": "put", "name": PROBE, "content": PROBE_SRC, "compile": true}),
    ));
    assert_eq!(put["success"], true, "put: {put}");
    assert_eq!(
        put["compiled"], true,
        "a %AI.Agent subclass must compile on 139: {put}"
    );

    let run = payload(&guard.mcp.call(
        "iris_execute",
        &serde_json::json!({"code": "Write ##class(IadAihub139.Probe).Ping()"}),
    ));
    assert_eq!(run["execution_path"], "atelier", "{run}");
    assert_eq!(run["output"].as_str().map(str::trim), Some("pong"), "{run}");
    drop(guard);

    let left = payload(&mcp.call(
        "iris_query",
        &serde_json::json!({
            "query": "SELECT COUNT(*) AS n FROM %Dictionary.CompiledClass WHERE ID = 'IadAihub139.Probe'"
        }),
    ));
    assert_eq!(
        left.pointer("/rows/0/n")
            .and_then(serde_json::Value::as_i64),
        Some(0),
        "the probe class must be gone after delete: {left}"
    );
}

// ── US2: the topic map points at files that exist ───────────────────────────

/// One HEAD per recorded path against GitHub raw on `master`. Needs network, not IRIS; a path that
/// upstream moved or deleted since `upstream-files.txt` was recorded fails here, by name.
#[tokio::test]
#[ignore = "live GitHub raw"]
async fn aihub_139_upstream_files() {
    let list = include_str!("../fixtures/aihub139/upstream-files.txt");
    let client = iris_agentic_dev_core::testing::live_client();
    let mut bad = Vec::new();
    let paths: Vec<&str> = list.lines().skip(1).collect();
    assert_eq!(
        paths.len(),
        56,
        "upstream-files.txt holds the 56 paths at 72749d6"
    );
    for path in paths {
        let url = format!(
            "https://raw.githubusercontent.com/intersystems-community/ai-hub-eap/master/{path}"
        );
        match client.head(&url).send().await {
            Ok(r) if r.status().as_u16() == 200 => {}
            Ok(r) => bad.push(format!("{path}: HTTP {}", r.status().as_u16())),
            Err(e) => bad.push(format!("{path}: {e}")),
        }
    }
    assert!(
        bad.is_empty(),
        "not on ai-hub-eap master:\n{}",
        bad.join("\n")
    );
}

// ── US3: every claim the skill makes holds on 139 ───────────────────────────
//
// Each test puts the fixtures it needs, reads what 139 does, and puts everything back: the
// `Scratch` guard runs the test's teardown ObjectScript and deletes the classes it put, in reverse,
// pass or panic. Claim ids for each assertion are in research.md's claim table.

/// `(document name, source)` for one fixture class under `tests/fixtures/aihub139/`.
macro_rules! fixture {
    ($name:literal) => {
        (
            concat!($name, ".cls"),
            include_str!(concat!("../fixtures/aihub139/", $name, ".cls")),
        )
    };
}

/// A writable session that owns what the test put on 139.
struct Scratch {
    mcp: McpSession,
    docs: Vec<String>,
    teardown: &'static str,
}

impl Scratch {
    fn new(env: &AihubEnv, teardown: &'static str) -> Self {
        let mut s = Self {
            mcp: writable(env),
            docs: Vec::new(),
            teardown,
        };
        // A run that panicked before its guard ran would leave state behind; start clean.
        if !teardown.is_empty() {
            s.mcp
                .call("iris_execute", &serde_json::json!({ "code": teardown }));
        }
        s
    }

    /// Put and compile one class; it must compile.
    fn put(&mut self, (name, src): (&str, &str)) {
        let got = self.put_raw(name, src);
        assert_eq!(got["compiled"], true, "{name} must compile on 139: {got}");
    }

    /// Put one class and return iris_doc's answer, compiled or not.
    fn put_raw(&mut self, name: &str, src: &str) -> serde_json::Value {
        self.docs.push(name.to_string());
        payload(&self.mcp.call(
            "iris_doc",
            &serde_json::json!({"mode": "put", "name": name, "content": src, "compile": true}),
        ))
    }

    fn exec(&mut self, code: &str) -> String {
        exec(&mut self.mcp, code)
    }

    /// Run `code` and read its `key=value` lines.
    fn kv(&mut self, code: &str) -> std::collections::HashMap<String, String> {
        let out = self.exec(code);
        out.lines()
            .filter_map(|l| l.split_once('='))
            .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
            .collect()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if !self.teardown.is_empty() {
            let _ = self.mcp.call(
                "iris_execute",
                &serde_json::json!({ "code": self.teardown }),
            );
        }
        for doc in self.docs.iter().rev() {
            let _ = self.mcp.call(
                "iris_doc",
                &serde_json::json!({"mode": "delete", "name": doc}),
            );
        }
    }
}

/// One value from a `kv` map; a missing key fails the test with the whole map.
fn get<'a>(m: &'a std::collections::HashMap<String, String>, k: &str) -> &'a str {
    m.get(k)
        .unwrap_or_else(|| panic!("no {k}= line in {m:?}"))
        .as_str()
}

fn json_of(m: &std::collections::HashMap<String, String>, k: &str) -> serde_json::Value {
    let v = get(m, k);
    serde_json::from_str(v).unwrap_or_else(|e| panic!("{k} is not JSON ({e}): {v}"))
}

/// Rows of an iris_query answer.
fn rows(mcp: &mut McpSession, sql: &str) -> Vec<serde_json::Value> {
    let got = payload(&mcp.call("iris_query", &serde_json::json!({ "query": sql })));
    got["rows"]
        .as_array()
        .unwrap_or_else(|| panic!("no rows for {sql}: {got}"))
        .clone()
}

/// ObjectScript that prints a status as `ok` or its error text.
const ST: &str = "$Select($System.Status.IsOK(sc):\"ok\",1:$System.Status.GetErrorText(sc))";

#[test]
#[ignore = "live iad-aihub-iris"]
fn aihub_139_class_inventory() {
    let Some(env) = aihub_env() else { return };
    let mut mcp = aihub_session(&env);

    let present = [
        "%AI.Agent",
        "%AI.Agent.Session",
        "%AI.Agent.Skill",
        "%AI.Agent.SubAgent",
        "%AI.LLM.Response",
        "%AI.MCP.Service",
        "%AI.Policy.Audit",
        "%AI.Policy.Authorization",
        "%AI.Policy.ConsoleAudit",
        "%AI.Policy.Discovery",
        "%AI.Provider",
        "%AI.RAG.Embedding",
        "%AI.RAG.Embedding.FastEmbed",
        "%AI.RAG.Embedding.OpenAI",
        "%AI.RAG.KnowledgeBase",
        "%AI.RAG.VectorStore.IRIS",
        "%AI.Shell.StreamRenderer",
        "%AI.Tool",
        "%AI.ToolMgr",
        "%AI.ToolSet",
        "%AI.Utils.SettingStore",
        "%ConfigStore.Configuration",
        "%Wallet.Collection",
        "%Wallet.KeyValue",
    ];
    let absent = ["%AI.Skill", "%AI.SubAgent", "%AI.System.StreamRenderer"];
    let list = |names: &[&str]| {
        names
            .iter()
            .map(|n| format!("'{n}'"))
            .collect::<Vec<_>>()
            .join(",")
    };
    let found: Vec<String> = rows(
        &mut mcp,
        &format!(
            "SELECT ID FROM %Dictionary.CompiledClass WHERE ID IN ({},{})",
            list(&present),
            list(&absent)
        ),
    )
    .iter()
    .filter_map(|r| r["ID"].as_str().map(str::to_string))
    .collect();
    for n in present {
        assert!(
            found.iter().any(|f| f == n),
            "{n} must exist on 139: {found:?}"
        );
    }
    for n in absent {
        assert!(!found.iter().any(|f| f == n), "{n} must not exist on 139");
    }

    // Signatures the skill prints: (class, method, class method?, FormalSpec, ReturnType).
    // FormalSpec is as the dictionary stores it: a `{}` default reads back as `{{}}`.
    let want = [
        ("%AI.Agent", "%Init", false, "context:%DynamicObject={{}}", "%Library.Status"),
        ("%AI.Agent", "Chat", false, "session:%AI.Agent.Session,input:%String,feedback:%RegisteredObject=\"\"", "%AI.LLM.Response"),
        ("%AI.Agent", "ChatWithContent", false, "session:%AI.Agent.Session,content:%DynamicArray,feedback:%RegisteredObject=\"\"", "%AI.LLM.Response"),
        ("%AI.Agent", "CreateSession", false, "config:%DynamicObject=\"\"", "%AI.Agent.Session"),
        ("%AI.Agent", "CreateSubAgent", false, "systemPrompt:%String=\"\"", "%AI.Agent"),
        ("%AI.Agent", "Run", false, "session:%AI.Agent.Session,goal:%String=\"\",callbackOref:%RegisteredObject=$$$NULLOREF", "%AI.LLM.Response"),
        ("%AI.Agent", "StreamChat", false, "session:%AI.Agent.Session,input:%String,callbackObj:%RegisteredObject=$$$NULLOREF", "%AI.LLM.Response"),
        ("%AI.Agent", "UseToolSet", false, "className:%String", "%Library.Status"),
        ("%AI.Agent", "UseSkill", false, "skillOrClassName", "%Library.Status"),
        ("%AI.Agent.Session", "GetStats", false, "", "%Library.DynamicObject"),
        ("%AI.Agent.Skill", "ExportSkill", false, "target:%String", "%Library.String"),
        ("%AI.Agent.Skill", "GetSkillFromURI", true, "uri:%String,subpath:%String=\"\",cacheDir:%String=\"\",authProvider:%RegisteredObject=\"\"", "%AI.Agent.Skill"),
        ("%AI.Agent.SubAgent", "Create", true, "parentAgent:%AI.Agent,systemPrompt:%String=\"\",config:%String=\"\"", "%AI.Agent"),
        ("%AI.Policy.Audit", "%LogExecution", false, "call:%DynamicObject,metadata:%DynamicObject,result:%DynamicObject,duration:%Integer,status:%Status", "%Library.Status"),
        ("%AI.Policy.Authorization", "%CanExecute", false, "toolref:%String,call:%DynamicObject,metadata:%DynamicObject", "%Library.Status"),
        ("%AI.Provider", "Create", true, "name:%String,settings:%DynamicObject", "%AI.Provider"),
        ("%AI.RAG.Embedding.FastEmbed", "Create", true, "", "%AI.RAG.Embedding.FastEmbed"),
        ("%AI.RAG.KnowledgeBase", "AddDocument", false, "text:%String,metadata:%DynamicObject={{}}", "%Library.Status"),
        ("%AI.RAG.KnowledgeBase", "AddDocuments", false, "docs:%DynamicArray", "%Library.Integer"),
        ("%AI.RAG.KnowledgeBase", "AddToAgent", false, "agent:%AI.Agent", "%Library.Status"),
        ("%AI.RAG.KnowledgeBase", "Build", false, "embedding:%AI.RAG.Embedding,vectorStore:%AI.RAG.VectorStore.IRIS", "%Library.Status"),
        ("%AI.RAG.KnowledgeBase", "ReindexDocument", false, "source:%String,text:%String,baseMetadata:%DynamicObject=\"\"", "%Library.Integer"),
        ("%AI.RAG.VectorStore.IRIS", "Build", false, "promotedFields:%DynamicArray=\"\"", "%Library.Status"),
        ("%AI.ToolMgr", "AddTool", false, "tool", "%Library.Status"),
        ("%AI.ToolMgr", "ExecuteTool", false, "toolName:%String,arguments:%DynamicObject={{}}", "%Library.DynamicObject"),
        ("%AI.ToolMgr", "RegisterToolSet", false, "className:%String", "%Library.Status"),
        ("%AI.ToolMgr", "SetAuditPolicy", false, "policy:%AI.Policy.Audit", "%Library.Status"),
        ("%AI.ToolMgr", "SetAuthPolicy", false, "policy:%AI.Policy.Authorization", "%Library.Status"),
        ("%AI.ToolMgr", "SetDiscoveryPolicy", false, "policy:%AI.Policy.Discovery", "%Library.Status"),
        ("%ConfigStore.Configuration", "Create", true, "area:%String,type:%String,subtype:%String,name:%String,details:%DynamicObject,displayName:%String=\"\",description:%String=\"\",readResource:%String=$$$AdminConfigStoreResourceName,editResource:%String=$$$AdminConfigStoreResourceName,enabled:%Boolean=1,validateDetails:%Boolean=1", "%Library.Status"),
        ("%ConfigStore.Configuration", "Delete", true, "fqn:%String", "%Library.Status"),
        ("%ConfigStore.Configuration", "Get", true, "fqn:%String,*config:%ConfigStore.Configuration", "%Library.Status"),
        ("%ConfigStore.Configuration", "GetDetails", true, "fqn:%String,*details:%DynamicObject,checkValid:%Boolean=1,resolveSecrets:%Boolean=0", "%Library.Status"),
    ];
    let methods = rows(
        &mut mcp,
        "SELECT parent, Name, ClassMethod, FormalSpec, ReturnType FROM %Dictionary.CompiledMethod \
         WHERE parent %STARTSWITH '%AI.' OR parent = '%ConfigStore.Configuration'",
    );
    let find = |class: &str, name: &str| {
        methods
            .iter()
            .find(|r| r["parent"] == class && r["Name"] == name)
            .cloned()
    };
    for (class, name, is_class, spec, ret) in want {
        let m = find(class, name).unwrap_or_else(|| panic!("no {class}:{name} on 139"));
        assert_eq!(
            m["ClassMethod"], is_class,
            "{class}:{name} ClassMethod: {m}"
        );
        assert_eq!(m["FormalSpec"], spec, "{class}:{name} FormalSpec: {m}");
        assert_eq!(m["ReturnType"], ret, "{class}:{name} ReturnType: {m}");
    }
    assert!(
        find("%AI.ToolMgr", "GetToolSpecs").is_none(),
        "%AI.ToolMgr has no GetToolSpecs on 139"
    );

    let props = rows(
        &mut mcp,
        "SELECT parent, Name, Type FROM %Dictionary.CompiledProperty WHERE parent IN \
         ('%AI.Agent','%AI.Agent.Skill','%AI.LLM.Response','%AI.RAG.KnowledgeBase','%AI.RAG.VectorStore.IRIS')",
    );
    let has_prop = |class: &str, name: &str| {
        props
            .iter()
            .any(|r| r["parent"] == class && r["Name"] == name)
    };
    for (class, name) in [
        ("%AI.Agent", "Provider"),
        ("%AI.Agent", "Model"),
        ("%AI.Agent", "SystemPrompt"),
        ("%AI.Agent", "ToolManager"),
        ("%AI.Agent", "ParentAgent"),
        ("%AI.LLM.Response", "Content"),
        ("%AI.LLM.Response", "ToolCalls"),
        ("%AI.LLM.Response", "Usage"),
        ("%AI.RAG.KnowledgeBase", "Name"),
        ("%AI.RAG.KnowledgeBase", "Description"),
        ("%AI.RAG.KnowledgeBase", "TopK"),
        ("%AI.RAG.VectorStore.IRIS", "TableName"),
        ("%AI.RAG.VectorStore.IRIS", "Dimensions"),
        ("%AI.RAG.VectorStore.IRIS", "ModelName"),
    ] {
        assert!(has_prop(class, name), "{class} must have property {name}");
    }
    assert!(
        !has_prop("%AI.Agent.Skill", "ParentAgent"),
        "%AI.Agent.Skill has no ParentAgent on 139 (it is on %AI.Agent)"
    );

    let params = rows(
        &mut mcp,
        "SELECT parent, Name, _Default FROM %Dictionary.CompiledParameter WHERE parent IN \
         ('%AI.Agent','%AI.Tool','%AI.ToolSet','%AI.MCP.Service','%AI.Agent.Skill') AND Name IN \
         ('PROVIDER','MODEL','APIKEY','PROVIDERCONFIG','TOOLSETS','SKILLS','DESCRIPTION','QUERYMAXROWS','SPECIFICATION','TOOLS')",
    );
    let param = |class: &str, name: &str| {
        params
            .iter()
            .find(|r| r["parent"] == class && r["Name"] == name)
            .map(|r| r["_Default"].as_str().unwrap_or_default().to_string())
    };
    for name in [
        "PROVIDER",
        "MODEL",
        "APIKEY",
        "PROVIDERCONFIG",
        "TOOLSETS",
        "SKILLS",
    ] {
        assert!(
            param("%AI.Agent", name).is_some(),
            "%AI.Agent must have {name}"
        );
    }
    assert!(param("%AI.MCP.Service", "SPECIFICATION").is_some());
    assert!(param("%AI.Agent.Skill", "TOOLS").is_some());
    assert_eq!(param("%AI.Tool", "QUERYMAXROWS").as_deref(), Some("100"));
    assert_eq!(param("%AI.ToolSet", "QUERYMAXROWS").as_deref(), Some("100"));
    assert!(
        param("%AI.Tool", "DESCRIPTION").is_none(),
        "%AI.Tool has no DESCRIPTION parameter on 139"
    );
}

#[test]
#[ignore = "live iad-aihub-iris"]
fn aihub_139_tool_and_query_tools() {
    let Some(env) = aihub_env() else { return };
    let mut s = Scratch::new(&env, "");
    s.put(fixture!("IadAihub139.Tool"));
    s.put(fixture!("IadAihub139.ToolSet"));
    s.put(fixture!("IadAihub139.QueryTools"));
    let m = s.kv(
        r#"Set t=##class(IadAihub139.Tool).%New() Write "discover=",t.%Discover().%ToJSON(),!
Write "invoke=",t.%Invoke("Reverse",{"text":"abc"}),!
Write "tsdiscover=",##class(IadAihub139.ToolSet).%New().%Discover().%ToJSON(),!
Write "qdiscover=",##class(IadAihub139.QueryTools).%New().%Discover().%ToJSON(),!
Set m=##class(%AI.ToolMgr).%New() Do m.AddTool(##class(IadAihub139.ToolSet).%New()) Write "run=",m.ExecuteTool("Reverse",{"text":"abc"}).%ToJSON(),!
Set m=##class(%AI.ToolMgr).%New() Do m.AddTool(##class(IadAihub139.QueryTools).%New()) Write "query=",m.ExecuteTool("ListTables",{"schema":"INFORMATION_SCHEMA"}).%ToJSON(),!"#,
    );

    // The doc comment is the description; parameters come from the signature.
    let tool = &json_of(&m, "discover")["tools"][0];
    assert_eq!(tool["name"], "Reverse", "{tool}");
    assert_eq!(
        tool["description"], "Return the text reversed.\ntext: the string to reverse",
        "{tool}"
    );
    assert_eq!(tool["parameters"]["properties"]["text"]["type"], "string");
    assert_eq!(tool["parameters"]["required"], serde_json::json!(["text"]));
    assert_eq!(get(&m, "invoke"), "cba");
    assert_eq!(
        json_of(&m, "tsdiscover")["tools"][0]["name"],
        "Reverse",
        "<Include Class> brings the tool in"
    );
    let run = json_of(&m, "run");
    assert_eq!(
        run["value"], "cba",
        "ExecuteTool returns {{timing, value}}: {run}"
    );
    assert!(run["timing"].is_number(), "{run}");

    // Query tool: types, required, Exclude, envelope, MaxRows.
    let q = json_of(&m, "qdiscover");
    let names: Vec<&str> = q["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    assert_eq!(
        names,
        ["ListTables"],
        "<Exclude Tool=\"Reverse\"/> drops Reverse: {q}"
    );
    let props = &q["tools"][0]["parameters"]["properties"];
    assert_eq!(props["schema"]["type"], "string");
    assert_eq!(
        props["minLen"]["type"], "integer",
        "%Integer maps to integer, not number"
    );
    assert_eq!(props["views"]["type"], "boolean");
    assert_eq!(
        q["tools"][0]["parameters"]["required"],
        serde_json::json!(["schema"]),
        "only the argument without a default is required"
    );
    let env_ = &json_of(&m, "query")["value"];
    for k in ["columns", "rows", "row_count", "truncated", "elapsed_ms"] {
        assert!(!env_[k].is_null(), "query envelope lacks {k}: {env_}");
    }
    assert_eq!(env_["row_count"], 1, "MaxRows=\"1\" caps the rows: {env_}");
    assert_eq!(env_["truncated"], true, "{env_}");
}

const Q_POSITIONAL: &str = r#"Class IadAihub139.QPositional Extends %AI.ToolSet
{

XData Definition [ MimeType = application/xml ]
{
<ToolSet Name="IadAihub139QPositional">
  <Query Name="CountTables" Arguments="schema As %String">
    SELECT COUNT(*) AS n FROM INFORMATION_SCHEMA.TABLES WHERE TABLE_SCHEMA = ?
  </Query>
</ToolSet>
}

}
"#;

const Q_UNDECLARED: &str = r#"Class IadAihub139.QUndeclared Extends %AI.ToolSet
{

XData Definition [ MimeType = application/xml ]
{
<ToolSet Name="IadAihub139QUndeclared">
  <Query Name="CountTables" Arguments="schema As %String">
    SELECT COUNT(*) AS n FROM INFORMATION_SCHEMA.TABLES WHERE TABLE_SCHEMA = :schema AND TABLE_TYPE = :kind
  </Query>
</ToolSet>
}

}
"#;

const Q_UNUSED: &str = r#"Class IadAihub139.QUnused Extends %AI.ToolSet
{

XData Definition [ MimeType = application/xml ]
{
<ToolSet Name="IadAihub139QUnused">
  <Query Name="CountTables" Arguments="schema As %String, unused As %Integer = 0">
    SELECT COUNT(*) AS n FROM INFORMATION_SCHEMA.TABLES WHERE TABLE_SCHEMA = :schema
  </Query>
</ToolSet>
}

}
"#;

#[test]
#[ignore = "live iad-aihub-iris"]
fn aihub_139_query_tool_compile_errors() {
    let Some(env) = aihub_env() else { return };
    let mut s = Scratch::new(&env, "");
    for (name, src, code) in [
        ("IadAihub139.QPositional.cls", Q_POSITIONAL, "#6049"),
        ("IadAihub139.QUndeclared.cls", Q_UNDECLARED, "#5431"),
        ("IadAihub139.QUnused.cls", Q_UNUSED, "#5822"),
    ] {
        let got = s.put_raw(name, src);
        assert_eq!(got["compiled"], false, "{name} must not compile: {got}");
        assert!(
            got.to_string().contains(code),
            "{name} fails with ERROR {code}: {got}"
        );
    }
}

const SEE_TOOL: &str = r#"/// Records the tool argument %CanExecute receives, and allows the call.
Class IadAihub139.SeeTool Extends %AI.Policy.Authorization
{

Method %CanExecute(toolref As %String, call As %DynamicObject, metadata As %DynamicObject) As %Status
{
    Set ^IadAihub139("tool") = toolref
    Return $$$OK
}

}
"#;

#[test]
#[ignore = "live iad-aihub-iris"]
fn aihub_139_policies() {
    let Some(env) = aihub_env() else { return };
    let mut s = Scratch::new(&env, "Kill ^IadAihub139");
    s.put(fixture!("IadAihub139.Tool"));
    s.put(fixture!("IadAihub139.ToolSet"));
    s.put(fixture!("IadAihub139.DenyPolicy"));
    s.put(fixture!("IadAihub139.CountAudit"));
    s.put(fixture!("IadAihub139.PolicyToolSet"));
    s.put(fixture!("IadAihub139.OneItemToolSet"));
    s.put(("IadAihub139.SeeTool.cls", SEE_TOOL));
    let m = s.kv(
        r#"Set m=##class(%AI.ToolMgr).%New() Do m.AddTool(##class(IadAihub139.ToolSet).%New()) Set p=##class(IadAihub139.DenyPolicy).%New() Do p.Blocked.Insert("Reverse") Set sc=m.SetAuthPolicy(p) Write "setauth=",$Select($System.Status.IsOK(sc):"ok",1:"err"),!
Try { Set r=m.ExecuteTool("Reverse",{"text":"abc"}) Write "global=ran ",r.%ToJSON(),! } Catch e { Write "global=",e.DisplayString(),! }
Set m=##class(%AI.ToolMgr).%New() Do m.AddTool(##class(IadAihub139.ToolSet).%New()) Set au=##class(IadAihub139.CountAudit).%New() Do m.SetAuditPolicy(au) Set r=m.ExecuteTool("Reverse",{"text":"abc"}) Write "audited=",r.value," calls=",au.Calls," last=",au.LastTool,!
Set m=##class(%AI.ToolMgr).%New() Do m.AddTool(##class(IadAihub139.ToolSet).%New()) Do m.SetAuthPolicy(##class(IadAihub139.SeeTool).%New()) Do m.ExecuteTool("Reverse",{"text":"abc"}) Write "seen=",$Get(^IadAihub139("tool")),!
Set m=##class(%AI.ToolMgr).%New() Do m.AddTool(##class(IadAihub139.PolicyToolSet).%New()) Try { Set r=m.ExecuteTool("Reverse",{"text":"abc"}) Write "local2=ran ",r.value,! } Catch e { Write "local2=",e.DisplayString(),! }
Set m=##class(%AI.ToolMgr).%New() Do m.AddTool(##class(IadAihub139.OneItemToolSet).%New()) Try { Set r=m.ExecuteTool("Reverse",{"text":"abc"}) Write "local1=ran ",r.value,! } Catch e { Write "local1=",e.DisplayString(),! }
Set rd=##class(%XML.Reader).%New() Do rd.OpenString("<Authorization><Blocked>Reverse</Blocked></Authorization>") Do rd.Correlate("Authorization","IadAihub139.DenyPolicy") Do rd.Next(.o,.sc) Write "reader1=",o.Blocked.Count(),!"#,
    );

    let global = get(&m, "global");
    assert!(
        global.contains("ToolAccessDenied"),
        "a global SetAuthPolicy deny throws ToolAccessDenied: {global}"
    );
    assert_eq!(
        get(&m, "audited"),
        "cba calls=1 last=Reverse",
        "SetAuditPolicy sees each call; call.name is the tool name"
    );
    // 139 passed the tool spec as JSON here. 154 passes `toolref`, the string the tool was
    // registered under.
    assert_eq!(
        get(&m, "seen"),
        "Reverse",
        "%CanExecute's first argument is the bare toolref: {m:?}"
    );

    let local2 = get(&m, "local2");
    assert!(
        local2.contains("ToolAccessDenied") && local2.contains("blocked by IadAihub139.DenyPolicy"),
        "a ToolSet-local <Authorization> with two <Blocked> items denies: {local2}"
    );
    assert_eq!(
        get(&m, "local1"),
        "ran cba",
        "M8: with one <Blocked> item 139's loader leaves the list empty, so nothing is denied"
    );
    assert_eq!(
        get(&m, "reader1"),
        "1",
        "%XML.Reader reads the same one-item XML as Count()=1, so the loader is at fault"
    );
}

const PC_BARE: &str = "Class IadAihub139.PCBare Extends %AI.Agent\n{\n\nParameter PROVIDERCONFIG = \"IadAihub139LLM\";\n\n}\n";
const PC_SHORT: &str = "Class IadAihub139.PCShort Extends %AI.Agent\n{\n\nParameter PROVIDERCONFIG = \"@{config:IadAihub139LLM}\";\n\n}\n";
const PC_FULL: &str = "Class IadAihub139.PCFull Extends %AI.Agent\n{\n\nParameter PROVIDERCONFIG = \"@{config:AI.LLM.IadAihub139LLM}\";\n\n}\n";
const PC_DOT: &str = "Class IadAihub139.PCDot Extends %AI.Agent\n{\n\nParameter PROVIDERCONFIG = \"@{config.IadAihub139LLM}\";\n\n}\n";

/// The M6 guard with each comparison parenthesised: a provider passed to `%New()` survives `%Init()`.
const FIXED_INIT: &str = r#"Class IadAihub139.FixedInit Extends %AI.Agent
{

Parameter MODELCONFIGNAME = "IadAihub139LLM";

Method %OnInit() As %Status
{
    Set sc = $$$OK
    Try {
        If (..Provider = "") && (..#MODELCONFIGNAME '= "") {
            Do ##class(%AI.Utils.SettingStore).RegisterDefaults()
            Set settings = {}.%FromJSON(##class(%AI.Utils.SettingStore).Expand("@{config:" _ ..#MODELCONFIGNAME _ "}"))
            Set ..Provider = ##class(%AI.Provider).Create(settings."model_provider", settings)
            Set ..Model = settings.model
        }
    } Catch ex {
        Set sc = ex.AsStatus()
    }
    Return sc
}

}
"#;

/// The fake-key wallet and config, and everything else the agent test leaves.
const AGENT_TEARDOWN: &str = r#"Do ##class(%ConfigStore.Configuration).Delete("AI.LLM.IadAihub139LLM")
Do ##class(%ConfigStore.Configuration).Delete("AI.LLM.IadAihub139NoKey")
Do ##class(%Wallet.KeyValue).Delete("IadAihub139.Fake")
Do ##class(%Wallet.Collection).Delete("IadAihub139")"#;

#[test]
#[ignore = "live iad-aihub-iris"]
fn aihub_139_agent_providerconfig() {
    let Some(env) = aihub_env() else { return };
    let mut s = Scratch::new(&env, AGENT_TEARDOWN);
    s.put(fixture!("IadAihub139.Tool"));
    s.put(fixture!("IadAihub139.ToolSet"));
    s.put(fixture!("IadAihub139.Agent"));
    s.put(fixture!("IadAihub139.BrokenInit"));
    for (name, src) in [
        ("IadAihub139.PCBare.cls", PC_BARE),
        ("IadAihub139.PCShort.cls", PC_SHORT),
        ("IadAihub139.PCFull.cls", PC_FULL),
        ("IadAihub139.PCDot.cls", PC_DOT),
        ("IadAihub139.FixedInit.cls", FIXED_INIT),
    ] {
        s.put((name, src));
    }
    let code = format!(
        r#"Set sc=##class(%ConfigStore.DescriptorManager).RebuildRegistry() Write "registry=",{ST},!
Set sc=##class(%ConfigStore.Configuration).Create("AI","LLM","","IadAihub139NoKey",{{"model_provider":"openai","model":"gpt-4.1-mini"}}) Write "nokey=",$System.Status.GetErrorText(sc),!
Set sc=##class(%Wallet.Collection).Create("IadAihub139",{{"UseResource":"%DB_USER","EditResource":"%DB_USER"}}) Write "wallet=",{ST},!
Set sc=##class(%Wallet.KeyValue).Create("IadAihub139.Fake",{{"Usage":"CUSTOM","Secret":{{"api_key":"sk-fake-not-a-key"}}}}) Write "secret=",{ST},!
Set sc=##class(%ConfigStore.Configuration).Create("AI","LLM","","IadAihub139LLM",{{"model_provider":"openai","model":"gpt-4.1-mini","api_key":"secret://IadAihub139.Fake#api_key"}}) Write "create=",{ST},!
Set sc=##class(%ConfigStore.Configuration).Get("AI.LLM.IadAihub139LLM",.c) Write "get=",{ST}," ",$IsObject($Get(c)),!
Try {{ Set x=##class(%ConfigStore.Configuration).Get("AI","LLM","","IadAihub139LLM") Write "get4=ran",! }} Catch e {{ Write "get4=",e.Name,! }}
Set sc=##class(%ConfigStore.Configuration).GetDetails("AI.LLM.IadAihub139LLM",.d) Write "details=",d.%ToJSON(),!
Set sc=##class(%ConfigStore.Configuration).GetDetails("AI.LLM.IadAihub139LLM",.d,1,1) Write "resolved=",d."api_key",!
Set a=##class(IadAihub139.Agent).%New() Write "new=",$IsObject(a.Provider),!
Set sc=a.%Init() Write "init=",{ST}," ",$IsObject(a.Provider)," ",a.Model,!
Write "agenttools=",a.ToolManager.%Discover().%ToJSON(),!
For k="PCBare","PCShort","PCFull","PCDot" {{ Set x=$ClassMethod("IadAihub139."_k,"%New") Set sc=x.%Init() Write k,"=",{ST}," ",$IsObject(x.Provider),! }}
Set p=##class(%AI.Provider).Create("openai",{{"api_key":"sk-other-not-a-key"}})
Set plain=##class(%AI.Agent).%New(p) Write "plain=",(plain.Provider=p),!
Set b=##class(IadAihub139.BrokenInit).%New(p) Set sc=b.%Init() Write "broken=",{ST}," ",(b.Provider=p)," ",b.Model,!
Set f=##class(IadAihub139.FixedInit).%New(p) Set sc=f.%Init() Write "fixed=",{ST}," ",(f.Provider=p),!
Try {{ Do ##class(%ConfigStore.Configuration).Delete("AI","LLM","","IadAihub139LLM") Write "del4=ran",! }} Catch e {{ Write "del4=",e.Name,! }}
Set sc=##class(%ConfigStore.Configuration).Get("AI.LLM.IadAihub139LLM",.c) Write "still=",{ST},!
Set sc=##class(%ConfigStore.Configuration).Delete("AI.LLM.IadAihub139LLM") Write "del=",{ST},!
Set sc=##class(%ConfigStore.Configuration).Get("AI.LLM.IadAihub139LLM",.c) Write "gone=",$System.Status.IsError(sc),!"#
    );
    let m = s.kv(&code);

    // A fresh 139 or 154 container has an empty descriptor registry, and every AI.LLM Create
    // fails with ERROR #26414 "No descriptor found for AI.LLM" until RebuildRegistry runs.
    assert!(
        get(&m, "nokey").contains("'api_key' is required for provider 'openai'"),
        "an openai entry with no api_key is refused: {m:?}"
    );
    for k in ["registry", "wallet", "secret", "create"] {
        assert_eq!(get(&m, k), "ok", "{k}: {m:?}");
    }
    assert_eq!(get(&m, "get"), "ok 1", "Get(fqn, .config) reads the entry");
    assert_eq!(
        get(&m, "get4"),
        "<PARAMETER>",
        "M1: the four-argument Get does not exist"
    );
    let details = json_of(&m, "details");
    assert_eq!(
        details["api_key"], "secret://IadAihub139.Fake#api_key",
        "{details}"
    );
    assert_eq!(
        get(&m, "resolved"),
        "sk-fake-not-a-key",
        "GetDetails(fqn,.d,1,1) resolves secret:// from the wallet"
    );
    assert_eq!(
        get(&m, "new"),
        "0",
        "M4: %New() alone leaves Provider empty"
    );
    assert_eq!(
        get(&m, "init"),
        "ok 1 gpt-4.1-mini",
        "%Init() builds it from PROVIDERCONFIG"
    );
    let tools = json_of(&m, "agenttools");
    assert!(
        tools.to_string().contains("\"Reverse\""),
        "Parameter TOOLSETS loads the ToolSet at %Init: {tools}"
    );

    let bare = get(&m, "PCBare");
    assert!(
        bare.contains("ProviderConfigError: PROVIDERCONFIG is invalid"),
        "M3: a bare config name is refused: {bare}"
    );
    assert_eq!(get(&m, "PCShort"), "ok 1", "@{{config:Name}} works");
    assert_eq!(get(&m, "PCFull"), "ok 1", "@{{config:AI.LLM.Name}} works");
    let dot = get(&m, "PCDot");
    assert!(
        dot.contains("ProviderConfigError: PROVIDERCONFIG is invalid"),
        "M3: the @{{prefix.key}} dot form is refused: {dot}"
    );

    assert_eq!(get(&m, "plain"), "1", "%New(provider) sets Provider");
    assert_eq!(
        get(&m, "broken"),
        "ok 0 gpt-4.1-mini",
        "M6: the unparenthesised guard is always true, so the caller's provider is replaced"
    );
    assert_eq!(
        get(&m, "fixed"),
        "ok 1",
        "parenthesised, the guard keeps the caller's provider"
    );
    assert_eq!(
        get(&m, "del4"),
        "<PARAMETER>",
        "M2: the four-argument Delete does not exist"
    );
    assert_eq!(get(&m, "still"), "ok", "and the entry is still there");
    assert_eq!(get(&m, "del"), "ok", "Delete(fqn) removes it");
    assert_eq!(get(&m, "gone"), "1");
}

const RAG_TEARDOWN: &str = r#"Do ##class(%SQL.Statement).%ExecDirect(, "DROP TABLE IadAihub139.Rag")
Do ##class(%SQL.Statement).%ExecDirect(, "DROP TABLE IadAihub139.Rag_Config")"#;

#[test]
#[ignore = "live iad-aihub-iris"]
fn aihub_139_rag_fastembed() {
    let Some(env) = aihub_env() else { return };
    let mut s = Scratch::new(&env, RAG_TEARDOWN);
    let m = s.kv(
        r#"Set emb=##class(%AI.RAG.Embedding.FastEmbed).Create()
Set vs=##class(%AI.RAG.VectorStore.IRIS).%New(), vs.TableName="IadAihub139.Rag", vs.Dimensions=384, vs.ModelName="AllMiniLML6V2"
Write "vsbuild=",vs.Build(),!
Set kb=##class(%AI.RAG.KnowledgeBase).%New(), kb.Name="iad139_docs", kb.Description="fixture docs", kb.TopK=2
Write "kbbuild=",kb.Build(emb,vs),!
Set rs=##class(%SQL.Statement).%ExecDirect(,"SELECT COUNT(*) FROM INFORMATION_SCHEMA.TABLES WHERE TABLE_SCHEMA='IadAihub139' AND TABLE_NAME IN ('Rag','Rag_Config')") Do rs.%Next() Write "tables=",rs.%GetData(1),!
Write "add=",kb.AddDocument("The reconciliation job retries three times.",{"source":"a.txt"}),!
Write "add2=",kb.AddDocument("The reconciliation job retries five times.",{"source":"a.txt"}),!
Set rs=##class(%SQL.Statement).%ExecDirect(,"SELECT COUNT(*) FROM IadAihub139.Rag") Do rs.%Next() Write "samesource=",rs.%GetData(1),!
Write "adds=",kb.AddDocuments([["Webhooks retry up to five times.",{"source":"w.txt"}],["Batch jobs run nightly.",{"source":"b.txt"}]]),!
Write "nosrc=",kb.AddDocument("No source here."),kb.AddDocument("No source here."),!
Set rs=##class(%SQL.Statement).%ExecDirect(,"SELECT COUNT(*) FROM IadAihub139.Rag") Do rs.%Next() Write "rows=",rs.%GetData(1),!
Write "reindex=",kb.ReindexDocument("w.txt","Webhooks retry up to seven times."),!
Set rs=##class(%SQL.Statement).%ExecDirect(,"SELECT COUNT(*) FROM IadAihub139.Rag") Do rs.%Next() Write "rows2=",rs.%GetData(1),!
Set a=##class(%AI.Agent).%New() Write "addtoagent=",kb.AddToAgent(a),!
Write "tools=",a.ToolManager.%Discover().%ToJSON(),!
Write "search=",a.ToolManager.ExecuteTool("iad139_docs",{"query":"how often do webhooks retry"}).%ToJSON(),!
Set vs2=##class(%AI.RAG.VectorStore.IRIS).%New(), vs2.TableName="IadAihub139.Rag", vs2.Dimensions=1536, vs2.ModelName="text-embedding-3-small"
Set sc=vs2.Build() Write "mismatch=",$System.Status.GetErrorText(sc),!
Write "fastembed=",emb.%ClassName(1),!"#,
    );

    for k in ["vsbuild", "kbbuild", "add", "add2", "addtoagent"] {
        assert_eq!(get(&m, k), "1", "{k}: {m:?}");
    }
    assert_eq!(
        get(&m, "tables"),
        "2",
        "Build makes the table and <Table>_Config"
    );
    assert_eq!(
        get(&m, "samesource"),
        "1",
        "AddDocument with the same source replaces the earlier chunks; they do not coexist"
    );
    assert_eq!(get(&m, "adds"), "2", "AddDocuments returns the chunk count");
    assert_eq!(get(&m, "nosrc"), "11");
    assert_eq!(
        get(&m, "rows"),
        "5",
        "without a source the same text is stored twice"
    );
    assert_eq!(get(&m, "reindex"), "1");
    assert_eq!(
        get(&m, "rows2"),
        "5",
        "ReindexDocument replaces, it does not add"
    );

    let tools = json_of(&m, "tools");
    let t = tools
        .as_array()
        .and_then(|a| a.iter().find(|t| t["name"] == "iad139_docs"))
        .unwrap_or_else(|| panic!("AddToAgent registers a tool named kb.Name: {tools}"))
        .clone();
    assert!(
        t["description"]
            .as_str()
            .unwrap_or_default()
            .contains("fixture docs"),
        "{t}"
    );
    assert_eq!(
        t["parameters"]["properties"]["query"]["type"], "string",
        "{t}"
    );
    assert_eq!(
        t["parameters"]["properties"]["top_k"]["type"], "integer",
        "{t}"
    );
    assert_eq!(
        t["parameters"]["required"],
        serde_json::json!(["query"]),
        "{t}"
    );

    let hits = json_of(&m, "search")["value"].clone();
    let hits = hits
        .as_array()
        .unwrap_or_else(|| panic!("search value is an array of hits: {hits}"));
    assert_eq!(hits.len(), 2, "TopK=2 is honoured: {hits:?}");
    for k in ["id", "metadata", "score", "text"] {
        assert!(!hits[0][k].is_null(), "hit lacks {k}: {}", hits[0]);
    }
    assert_eq!(hits[0]["metadata"]["source"], "w.txt", "{}", hits[0]);
    assert!(!hits[0]["metadata"]["chunk_index"].is_null());

    let mismatch = get(&m, "mismatch");
    assert!(
        mismatch.contains("Model mismatch") && mismatch.contains("AllMiniLML6V2"),
        "rebuilding over a table from another model is refused: {mismatch}"
    );
    assert_eq!(get(&m, "fastembed"), "%AI.RAG.Embedding.FastEmbed");
}

/// The two web applications the bridge test makes, and the audit global.
const MCP_TEARDOWN: &str = r#"Kill ^IadAihub139
New $Namespace Set $Namespace="%SYS"
For app="/mcp/iad139","/mcp/iad139a" { If ##class(Security.Applications).Exists(app) { Do ##class(Security.Applications).Delete(app) } }"#;

/// Where the bridge's config, input and log live inside the container.
const BRIDGE_DIR: &str = "/tmp/iad139-bridge";

/// Bridge config: one IRIS, both endpoints, over stdio.
const BRIDGE_TOML: &str = r#"[mcp]
transport = "stdio"

[[iris]]
name = "local"
server = { host = "localhost", port = 1972, username = "_SYSTEM", password = "SYS" }
endpoints = [
  { path = "/mcp/iad139", username = "_SYSTEM", password = "SYS" },
  { path = "/mcp/iad139a", username = "_SYSTEM", password = "SYS" },
]
"#;

/// `docker exec -i <container> sh -c <script>` with `stdin`; returns stdout.
fn in_container(env: &AihubEnv, script: &str, stdin: &str) -> String {
    use std::io::Write;
    let mut child = std::process::Command::new("docker")
        .args(["exec", "-i", &env.container, "sh", "-c", script])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("docker exec");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    let out = child.wait_with_output().expect("docker exec output");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Removes the bridge's directory in the container, pass or panic.
struct BridgeDir<'a>(&'a AihubEnv);

impl Drop for BridgeDir<'_> {
    fn drop(&mut self) {
        in_container(self.0, &format!("rm -r {BRIDGE_DIR}"), "");
    }
}

/// Run `iris-mcp-server` inside the container over stdio: initialize, list, then `calls`.
/// Returns the JSON-RPC responses by id, and the bridge's log.
fn bridge(
    env: &AihubEnv,
    calls: &[(&str, serde_json::Value)],
) -> (std::collections::HashMap<i64, serde_json::Value>, String) {
    let init = [
        serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
            "protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"iad-132","version":"0"}}}),
        serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
    ];
    let rest: Vec<serde_json::Value> = calls
        .iter()
        .zip(10..)
        .map(|((name, args), id)| {
            serde_json::json!({"jsonrpc":"2.0","id":id,"method":"tools/call",
                "params":{"name":name,"arguments":args}})
        })
        .collect();
    let lines = |v: &[serde_json::Value]| v.iter().map(|m| format!("{m}\n")).collect::<String>();
    in_container(
        env,
        &format!("cat > {BRIDGE_DIR}/init.jsonl"),
        &lines(&init),
    );
    in_container(
        env,
        &format!("cat > {BRIDGE_DIR}/call.jsonl"),
        &lines(&rest),
    );
    // The bridge answers asynchronously; the sleeps keep stdin open until it has.
    let out = in_container(
        env,
        &format!(
            ": > {d}/mcp.log; (cat {d}/init.jsonl; sleep 4; cat {d}/call.jsonl; sleep 4) | \
             timeout 25 /usr/irissys/bin/iris-mcp-server --config={d}/mcp.toml \
             --log-output=file --log-file={d}/mcp.log run",
            d = BRIDGE_DIR
        ),
        "",
    );
    let log = in_container(env, &format!("cat {BRIDGE_DIR}/mcp.log"), "");
    let by_id = out
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter_map(|m| m["id"].as_i64().map(|id| (id, m)))
        .collect();
    (by_id, log)
}

fn tool_names(list: &serde_json::Value) -> Vec<String> {
    list["result"]["tools"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|t| t["name"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

#[test]
#[ignore = "live iad-aihub-iris"]
fn aihub_139_mcp_bridge() {
    let Some(env) = aihub_env() else { return };
    let mut s = Scratch::new(&env, MCP_TEARDOWN);
    for f in [
        fixture!("IadAihub139.Tool"),
        fixture!("IadAihub139.ToolSet"),
        fixture!("IadAihub139.MCP"),
        fixture!("IadAihub139.GlobalAudit"),
        fixture!("IadAihub139.AuditToolSet"),
        fixture!("IadAihub139.AuditMCP"),
    ] {
        s.put(f);
    }
    in_container(
        &env,
        &format!("mkdir -p {BRIDGE_DIR} && cat > {BRIDGE_DIR}/mcp.toml"),
        BRIDGE_TOML,
    );
    let _dir = BridgeDir(&env);

    let app = |path: &str, class: &str, ty: u32| {
        format!(
            r#"New $Namespace Set $Namespace="%SYS"
Kill p Set p("DispatchClass")="{class}",p("NameSpace")="USER",p("Enabled")=1,p("AutheEnabled")=32,p("MatchRoles")=":%All",p("Type")={ty}
Set sc=##class(Security.Applications).Create("{path}",.p) Write "type{ty}=",{ST},!"#
        )
    };
    // Type 16, the MCP bit alone, is refused.
    let m = s.kv(&app("/mcp/iad139", "IadAihub139.MCP", 16));
    let t16 = get(&m, "type16");
    assert!(
        t16.contains("CSP bit"),
        "an MCP app needs Type 18 (CSP + MCP), 16 alone is refused: {t16}"
    );
    // Type 2, a plain CSP app, is accepted, but the bridge cannot use it.
    let m = s.kv(&app("/mcp/iad139", "IadAihub139.MCP", 2));
    assert_eq!(get(&m, "type2"), "ok");
    let (got, log) = bridge(&env, &[]);
    let names = got.get(&2).map(tool_names).unwrap_or_default();
    assert!(
        !names.iter().any(|n| n.ends_with("_Reverse")),
        "a Type 2 app serves no tools: {names:?}"
    );
    assert!(
        log.contains("failed identity checking"),
        "the bridge log says why: {log}"
    );

    let m = s.kv(&format!(
        r#"New $Namespace Set $Namespace="%SYS"
Kill p Set p("Type")=18 Set sc=##class(Security.Applications).Modify("/mcp/iad139",.p) Write "modify=",{ST},!"#
    ));
    assert_eq!(get(&m, "modify"), "ok");
    let m = s.kv(&app("/mcp/iad139a", "IadAihub139.AuditMCP", 18));
    assert_eq!(get(&m, "type18"), "ok");

    let (got, log) = bridge(
        &env,
        &[
            ("mcp_iad139_Reverse", serde_json::json!({"text": "abc"})),
            ("mcp_iad139a_Reverse", serde_json::json!({"text": "xyz"})),
        ],
    );
    let answer = |id: i64| {
        got.get(&id)
            .unwrap_or_else(|| panic!("no answer to id {id}; got {got:?}; log: {log}"))["result"]
            .clone()
    };
    let init = answer(1);
    assert!(
        init["instructions"]
            .as_str()
            .unwrap_or_default()
            .contains("test_Add"),
        "the bridge's own instructions still name the test_Add example: {init}"
    );
    let list = &got[&2];
    let names = tool_names(list);
    for want in ["mcp_iad139_Reverse", "mcp_iad139a_Reverse"] {
        assert!(
            names.iter().any(|n| n == want),
            "tool names are mcp_<path segment>_<tool>: {names:?}"
        );
    }
    assert!(
        !names.iter().any(|n| n == "iris_status"),
        "with a working app the fallback iris_status is gone: {names:?}"
    );
    let desc = list["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "mcp_iad139_Reverse")
        .and_then(|t| t["description"].as_str())
        .unwrap_or_default()
        .to_string();
    assert!(
        desc.starts_with("[Remote Service: local_mcp_iad139] "),
        "{desc}"
    );

    let call = answer(10);
    assert_eq!(call["isError"], false, "{call}");
    assert_eq!(
        call["content"][0]["text"], "\"cba\"",
        "the result is JSON text: {call}"
    );
    let audited = answer(11);
    assert_eq!(
        audited["isError"], false,
        "a ToolSet with an audit policy still serves over the bridge: {audited}"
    );
    let m = s.kv(r#"Write "audit=",$Get(^IadAihub139("audit"),0),!"#);
    assert!(
        get(&m, "audit").parse::<i64>().unwrap_or(0) >= 1,
        "the audit policy ran for the bridged call: {m:?}"
    );
}

/// What the real turn leaves: its wallet secret and its ConfigStore entry.
const TURN_TEARDOWN: &str = r#"Do ##class(%ConfigStore.Configuration).Delete("AI.LLM.IadAihub139LLM")
Do ##class(%Wallet.KeyValue).Delete("IadAihub139.OpenAI")
Do ##class(%Wallet.Collection).Delete("IadAihub139")"#;

/// The value after `key=` on the first output line that has it (terminal lines carry a prompt).
fn after<'a>(out: &'a str, key: &str) -> &'a str {
    out.lines()
        .find_map(|l| l.split_once(&format!("{key}=")).map(|(_, v)| v.trim()))
        .unwrap_or_else(|| panic!("no {key}= in the session output"))
}

/// One real agent turn: the model must call the tool. Needs Tom's key; costs about a cent.
#[test]
#[ignore = "live iad-aihub-iris + OpenAI"]
fn aihub_139_real_turn() {
    let key = std::env::var("OPENAI_API_KEY").unwrap_or_default();
    if std::env::var("IAD_AIHUB_LLM").as_deref() != Ok("1") || key.is_empty() {
        println!("SKIP aihub_139_real_turn: set IAD_AIHUB_LLM=1 and OPENAI_API_KEY to run it");
        return;
    }
    let Some(env) = aihub_env() else { return };
    let mut s = Scratch::new(&env, TURN_TEARDOWN);
    s.put(fixture!("IadAihub139.Tool"));
    s.put(fixture!("IadAihub139.ToolSet"));
    s.put(fixture!("IadAihub139.Agent"));
    s.put(fixture!("IadAihub139.CountAudit"));

    // The key goes by name into the session's environment; it is never in a command line,
    // in iad's environment, or in anything this test prints.
    let script = r#"Set sc=##class(%ConfigStore.DescriptorManager).RebuildRegistry()
Set k=$System.Util.GetEnviron("OPENAI_API_KEY")
Set sc=##class(%Wallet.Collection).Create("IadAihub139",{"UseResource":"%DB_USER","EditResource":"%DB_USER"})
Set sc=##class(%Wallet.KeyValue).Create("IadAihub139.OpenAI",{"Usage":"CUSTOM","Secret":{"api_key":(k)}}) Kill k
Write "wallet=",$System.Status.IsOK(sc),!
Set sc=##class(%ConfigStore.Configuration).Create("AI","LLM","","IadAihub139LLM",{"model_provider":"openai","model":"gpt-4.1-mini","api_key":"secret://IadAihub139.OpenAI#api_key"})
Write "config=",$System.Status.IsOK(sc),!
Set a=##class(IadAihub139.Agent).%New() Set sc=a.%Init() Write "init=",$System.Status.IsOK(sc),!
Set au=##class(IadAihub139.CountAudit).%New() Do a.ToolManager.SetAuditPolicy(au)
Set ses=a.CreateSession() Try { Set r=a.Chat(ses,"Reverse the text: hello") Write "content=",$Translate(r.Content,$Char(10,13),"  "),! } Catch e { Write "content=ERROR ",e.DisplayString(),! }
Write "calls=",au.Calls," ",au.LastTool,!
Halt
"#;
    let out = {
        use std::io::Write;
        let mut child = std::process::Command::new("docker")
            .args([
                "exec",
                "-i",
                "-e",
                "OPENAI_API_KEY",
                &env.container,
                "iris",
                "session",
                "IRIS",
                "-U",
                &env.namespace,
            ])
            .env("OPENAI_API_KEY", &key)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("docker exec iris session");
        child
            .stdin
            .take()
            .unwrap()
            .write_all(script.as_bytes())
            .unwrap();
        let o = child.wait_with_output().expect("iris session output");
        String::from_utf8_lossy(&o.stdout)
            .chars()
            .filter(|c| *c == '\n' || !c.is_control())
            .collect::<String>()
    };
    let prefix: String = key.chars().take(8).collect();
    assert!(
        !out.contains(&prefix),
        "the key must not appear in the session output"
    );
    for k in ["wallet", "config", "init"] {
        assert_eq!(after(&out, k), "1", "{k} failed:\n{out}");
    }
    let content = after(&out, "content");
    assert!(!content.starts_with("ERROR"), "Chat failed: {content}");
    assert_eq!(
        after(&out, "calls").split_whitespace().nth(1),
        Some("Reverse"),
        "the model called the tool (the call is asserted, not the text): {content}"
    );
}

/// Skill section 4: with no web gateway (`docker_only = true`), the agent reads `%AI` signatures
/// by running ObjectScript over `iris_execute` against `%Dictionary.CompiledMethod`.
#[test]
#[ignore = "live iad-aihub-iris"]
fn aihub_139_docker_only_reads_the_dictionary() {
    let Some(env) = aihub_env() else { return };
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join(".iris-agentic-dev.toml"),
        format!(
            "docker_only = true\ncontainer = \"{}\"\nnamespace = \"{}\"\n",
            env.container, env.namespace
        ),
    )
    .unwrap();
    // No IRIS_HOST or IRIS_WEB_PORT: the only way in is docker exec.
    let mut mcp = McpSession::start_in(
        Some(dir.path()),
        &[("IRIS_CONTAINER".into(), env.container.clone())],
    );
    let out = exec(
        &mut mcp,
        r#"Set m=##class(%Dictionary.CompiledMethod).%OpenId("%AI.Agent||Chat") Write "spec=",m.FormalSpec,!"#,
    );
    assert!(
        out.contains("spec=session:%AI.Agent.Session,input:%String"),
        "docker_only reads %AI.Agent:Chat from the dictionary: {out}"
    );
}
