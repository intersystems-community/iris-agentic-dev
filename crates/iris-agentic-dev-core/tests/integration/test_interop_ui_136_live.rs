//! 136: the facts the `ensemble-production` skill states about BPL, DTL and rule classes, each
//! measured on IRIS 2026.2 before the skill says it. Every class lives under `IADTest136` in
//! USER and the package is deleted at the end, whatever happened.
//!
//! ```bash
//! IRIS_HOST=localhost IRIS_WEB_PORT=52780 IRIS_USERNAME=_SYSTEM IRIS_PASSWORD=SYS \
//!   cargo test --features testing --test integration interop_ui_136 -- --ignored --test-threads=1
//! ```

use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};
use iris_agentic_dev_core::tools::interop::{
    handle_iris_business_rule_info, BusinessRuleInfoParams,
};
use serde_json::{json, Value};

/// USER, which CI's stock container also has, and which is interoperability-enabled there and on
/// iris-dev-iris.
const NS: &str = "USER";

struct Live {
    base: String,
    user: String,
    pass: String,
    http: reqwest::Client,
    conn: IrisConnection,
}

fn live() -> Live {
    let host = std::env::var("IRIS_HOST").expect("IRIS_HOST must be set for 136 live tests");
    let port = std::env::var("IRIS_WEB_PORT").unwrap_or_else(|_| "52780".into());
    let user = std::env::var("IRIS_USERNAME").unwrap_or_else(|_| "_SYSTEM".into());
    let pass = std::env::var("IRIS_PASSWORD").unwrap_or_else(|_| "SYS".into());
    let base = format!("http://{host}:{port}");
    let conn = IrisConnection::new(
        base.clone(),
        NS,
        user.clone(),
        pass.clone(),
        DiscoverySource::EnvVar,
    );
    Live {
        base,
        user,
        pass,
        http: iris_agentic_dev_core::testing::live_client(),
        conn,
    }
}

impl Live {
    /// PUT a class and compile it; returns the compile errors (empty when it compiled).
    async fn put(&self, name: &str, source: &str) -> Vec<String> {
        let lines: Vec<&str> = source.lines().collect();
        let put = self
            .http
            .put(format!(
                "{}/api/atelier/v1/{NS}/doc/{name}.cls?ignoreConflict=1",
                self.base
            ))
            .basic_auth(&self.user, Some(&self.pass))
            .json(&json!({"enc": false, "content": lines}))
            .send()
            .await
            .expect("PUT reaches IRIS");
        assert!(put.status().is_success(), "PUT {name}: {}", put.status());
        let r: Value = self
            .http
            .post(format!("{}/api/atelier/v1/{NS}/action/compile", self.base))
            .basic_auth(&self.user, Some(&self.pass))
            .json(&json!([format!("{name}.cls")]))
            .send()
            .await
            .expect("compile reaches IRIS")
            .json()
            .await
            .expect("compile answers JSON");
        let mut errs: Vec<String> = r["status"]["errors"]
            .as_array()
            .map(|a| a.iter().map(|e| e.to_string()).collect())
            .unwrap_or_default();
        if let Some(console) = r["console"].as_array() {
            errs.extend(
                console
                    .iter()
                    .filter_map(|l| l.as_str())
                    .filter(|l| l.contains("ERROR"))
                    .map(str::to_string),
            );
        }
        errs
    }

    /// The HTTP status the editors' REST API gives for an artifact (`bpl`, `dtl`, `rules`).
    async fn editor(&self, kind: &str, name: &str) -> u16 {
        self.http
            .get(format!(
                "{}/api/interop-editors/v6/{NS}/{kind}/{name}",
                self.base
            ))
            .basic_auth(&self.user, Some(&self.pass))
            .send()
            .await
            .map(|r| r.status().as_u16())
            .unwrap_or(0)
    }

    /// Whether this build serves the editors' REST API at all. It arrived after 2025.3 (CI's stock
    /// image answers 404 for every path), so the editor checks run only where it exists, and say
    /// so where it does not. The compile and behaviour checks run everywhere.
    async fn has_editor_api(&self) -> bool {
        let got = self.editor("rules", "").await;
        if got == 404 || got == 0 {
            eprintln!(
                "no /api/interop-editors/v6 on this build (HTTP {got}): editor checks skipped"
            );
            return false;
        }
        true
    }

    async fn run(&self, code: &str) -> String {
        self.conn
            .execute_via_generator(code, NS, &self.http)
            .await
            .unwrap_or_else(|e| format!("EXEC_ERROR {e}"))
    }

    async fn cleanup(&self) {
        self.run(
            r#" Try { Do ##class(Ens.Director).StopProduction(10,1) } Catch { }
 Kill ^IADTest136
 Do $system.OBJ.DeletePackage("IADTest136","-d")
 Write "ok",$C(10)"#,
        )
        .await;
    }
}

const OP: &str = r#"Class IADTest136.Op Extends Ens.BusinessOperation
{

Method OnMessage(pRequest As Ens.StringRequest, Output pResponse As Ens.StringResponse) As %Status
{
    Set ^IADTest136(..%ConfigName,$Increment(^IADTest136(..%ConfigName)))=pRequest.StringValue
    If pRequest.StringValue="SLOW" Hang 8
    Set pResponse=##class(Ens.StringResponse).%New()
    Set pResponse.StringValue="ok:"_..%ConfigName
    Quit $$$OK
}

}"#;

const SVC: &str = r#"Class IADTest136.Svc Extends Ens.BusinessService
{

}"#;

const FN: &str = r#"Class IADTest136.Fn Extends Ens.Rule.FunctionSet
{

/// Read-only lookup: the Rule Editor's Test button runs functions for real.
ClassMethod IsDup(pId As %String) As %Boolean [ Final ]
{
    Quit pId="D"
}

}"#;

const BP: &str = r#"Class IADTest136.BP Extends Ens.BusinessProcessBPL
{

XData BPL [ XMLNamespace = "http://www.intersystems.com/bpl" ]
{
<process language='objectscript' request='Ens.StringRequest' response='Ens.StringResponse'>
<context>
<property name='Input' type='%String' instantiate='0'/>
<property name='Outcome' type='%String' instantiate='0'/>
<property name='Reasons' type='%String' instantiate='0'/>
<property name='Why' type='%String' instantiate='0'>
<parameters><parameter name='MAXLEN' value='500'/></parameters>
</property>
<property name='Agent' type='%String' instantiate='0'/>
</context>
<sequence>
<assign property='context.Input' value='request.StringValue' action='set'/>
<rule name='decide' rule='IADTest136.Rule' resultLocation='context.Outcome' reasonLocation='context.Why'/>
<scope>
<call name='agent' target='Agent' async='0' timeout='3'>
<request type='Ens.StringRequest'><assign property='callrequest.StringValue' value='context.Input' action='set'/></request>
<response type='Ens.StringResponse'><assign property='context.Agent' value='callresponse.StringValue' action='set'/></response>
</call>
<faulthandlers>
<catchall><assign property='context.Agent' value='"failed"' action='set'/></catchall>
</faulthandlers>
</scope>
<code><![CDATA[ Set context.Reasons=context.Reasons_"seen;"]]></code>
<assign property='response.StringValue' value='context.Outcome_"|"_context.Reasons_"|"_context.Agent_"|"_context.Why' action='set'/>
</sequence>
</process>
}

}"#;

const RULE: &str = r#"Class IADTest136.Rule Extends Ens.Rule.Definition
{

Parameter RuleAssistClass = "Ens.Rule.GeneralBusinessRuleAssist";

XData RuleDefinition [ XMLNamespace = "http://www.intersystems.com/rule" ]
{
<ruleDefinition alias="" context="IADTest136.BP.Context" production="">
<ruleSet name="" effectiveBegin="" effectiveEnd="">
<rule name="dup">
<when condition="IsDup(Input)">
<assign property="Reasons" value="Reasons_&quot;dup;&quot;"></assign>
<return>&quot;invalid&quot;</return>
</when>
</rule>
<rule name="card">
<when condition="Input=&quot;C&quot;">
<assign property="Reasons" value="Reasons_&quot;card;&quot;"></assign>
<return>&quot;routine&quot;</return>
</when>
</rule>
<return>&quot;further_review&quot;</return>
</ruleSet>
</ruleDefinition>
}

}"#;

const RULE_QUALIFIED: &str = r#"Class IADTest136.RuleQual Extends Ens.Rule.Definition
{

Parameter RuleAssistClass = "Ens.Rule.GeneralBusinessRuleAssist";

XData RuleDefinition [ XMLNamespace = "http://www.intersystems.com/rule" ]
{
<ruleDefinition alias="" context="IADTest136.BP.Context" production="">
<ruleSet name="" effectiveBegin="" effectiveEnd="">
<rule name="dup">
<when condition="IADTest136.Fn.IsDup(Input)">
<return>&quot;invalid&quot;</return>
</when>
</rule>
</ruleSet>
</ruleDefinition>
}

}"#;

const DTL: &str = r#"Class IADTest136.DTL Extends Ens.DataTransformDTL [ DependsOn = Ens.StringRequest ]
{

Parameter IGNOREMISSINGSOURCE = 1;

XData DTL [ XMLNamespace = "http://www.intersystems.com/dtl" ]
{
<transform sourceClass='Ens.StringRequest' targetClass='Ens.StringResponse' create='new' language='objectscript'>
<assign value='$ZCONVERT(source.StringValue,"U")' property='target.StringValue' action='set'/>
</transform>
}

}"#;

const CODE_DTL: &str = r#"Class IADTest136.CodeDTL Extends Ens.DataTransform
{

ClassMethod Transform(source As Ens.StringRequest, ByRef target As Ens.StringResponse) As %Status
{
    Set target=##class(Ens.StringResponse).%New()
    Set target.StringValue=$ZCONVERT(source.StringValue,"U")
    Quit $$$OK
}

}"#;

const CODE_BP: &str = r#"Class IADTest136.CodeBP Extends Ens.BusinessProcess [ ClassType = persistent ]
{

Method OnRequest(pRequest As %Library.Persistent, Output pResponse As %Library.Persistent) As %Status
{
    Quit $$$OK
}

}"#;

const ROUTE_RULE: &str = r#"Class IADTest136.RouteRule Extends Ens.Rule.Definition
{

Parameter RuleAssistClass = "EnsLib.MsgRouter.RuleAssist";

XData RuleDefinition [ XMLNamespace = "http://www.intersystems.com/rule" ]
{
<ruleDefinition alias="" context="EnsLib.MsgRouter.RoutingEngine" production="IADTest136.Prod">
<ruleSet name="" effectiveBegin="" effectiveEnd="">
<rule name="byFirstLetter">
<constraint name="msgClass" value="Ens.StringRequest"></constraint>
<when condition="StartsWith(Document.StringValue,&quot;A&quot;)">
<send transform="" target="OpA"></send>
<return></return>
</when>
<otherwise>
<send transform="" target="OpB"></send>
<return></return>
</otherwise>
</rule>
</ruleSet>
</ruleDefinition>
}

}"#;

const PROD: &str = r#"Class IADTest136.Prod Extends Ens.Production
{

XData ProductionDefinition
{
<Production Name="IADTest136.Prod" LogGeneralTraceEvents="false">
<Item Name="Svc" ClassName="IADTest136.Svc" PoolSize="0" Enabled="true"/>
<Item Name="BP" ClassName="IADTest136.BP" PoolSize="1" Enabled="true"/>
<Item Name="Agent" ClassName="IADTest136.Op" PoolSize="1" Enabled="true"/>
<Item Name="Router" ClassName="EnsLib.MsgRouter.RoutingEngine" PoolSize="1" Enabled="true">
<Setting Target="Host" Name="BusinessRuleName">IADTest136.RouteRule</Setting>
</Item>
<Item Name="OpA" ClassName="IADTest136.Op" PoolSize="1" Enabled="true"/>
<Item Name="OpB" ClassName="IADTest136.Op" PoolSize="1" Enabled="true"/>
</Production>
}

}"#;

async fn body(l: &Live) -> Result<(), String> {
    let mut problems = Vec::new();
    for (name, src) in [
        ("IADTest136.Op", OP),
        ("IADTest136.Svc", SVC),
        ("IADTest136.Fn", FN),
        ("IADTest136.BP", BP),
        ("IADTest136.Rule", RULE),
        ("IADTest136.DTL", DTL),
        ("IADTest136.CodeDTL", CODE_DTL),
        ("IADTest136.CodeBP", CODE_BP),
        ("IADTest136.RouteRule", ROUTE_RULE),
        ("IADTest136.Prod", PROD),
    ] {
        let errs = l.put(name, src).await;
        if !errs.is_empty() {
            problems.push(format!("{name} did not compile: {errs:?}"));
        }
    }
    if !problems.is_empty() {
        return Err(problems.join("\n"));
    }

    // Fact 1 and 4: the editors open BPL and DTL classes, and refuse the code-based kinds.
    let editors = l.has_editor_api().await;
    for (kind, name, want) in [
        ("bpl", "IADTest136.BP", 200),
        ("bpl", "IADTest136.CodeBP", 400),
        ("dtl", "IADTest136.DTL", 200),
        ("dtl", "IADTest136.CodeDTL", 400),
        ("rules", "IADTest136.Rule", 200),
    ]
    .into_iter()
    .filter(|_| editors)
    {
        let got = l.editor(kind, name).await;
        if got != want {
            problems.push(format!("editor {kind}/{name}: HTTP {got}, expected {want}"));
        }
    }

    // Fact 2: a rule function is called by its bare name; the qualified form does not compile.
    let qual = l.put("IADTest136.RuleQual", RULE_QUALIFIED).await;
    if qual.is_empty() {
        problems.push("a qualified rule-function call compiled; the skill says it does not".into());
    }
    let info = handle_iris_business_rule_info(
        Some(&l.conn),
        &BusinessRuleInfoParams {
            action: "get".into(),
            rule_name: Some("IADTest136.Rule".into()),
            namespace: NS.into(),
        },
    )
    .await
    .expect("Ok");
    let info: Value =
        serde_json::from_str(&info.content.first().unwrap().as_text().unwrap().text).unwrap();
    if info["rule_type"] != "Ens.Rule.GeneralBusinessRuleAssist" {
        problems.push(format!("the rule did not load as a general rule: {info}"));
    }

    // Fact 4: the DTL transforms.
    let dtl = l
        .run(
            r#" Set src=##class(Ens.StringRequest).%New("abc")
 Set sc=##class(IADTest136.DTL).Transform(src,.tgt)
 Write $Select($System.Status.IsOK(sc):tgt.StringValue,1:"ERR"),$C(10)"#,
        )
        .await;
    if dtl.trim() != "ABC" {
        problems.push(format!("the DTL did not transform: {dtl:?}"));
    }

    // Facts 3 and 5: run the production. One call per step: StartProduction moves the device of
    // the job that calls it, so the calls that report come after it, each in its own job.
    // `CleanProduction` clears a production an earlier test left suspended: IRIS will not start one
    // of a different name while it is, and a force stop does not clear it.
    let started = l
        .run(
            r#" Set tIO=$IO
 Try { Do ##class(Ens.Director).StopProduction(10,1) } Catch { }
 Do ##class(Ens.Director).CleanProduction()
 Kill ^IADTest136
 Set sc=##class(Ens.Director).StartProduction("IADTest136.Prod")
 Use tIO
 Write $Select(sc:"STARTED",1:"START|"_$System.Status.GetErrorText(sc)),$C(10)"#,
        )
        .await;
    let mut out = started.clone();
    for input in ["D", "C", "X", "SLOW"] {
        out += &l
            .run(&format!(
                r#" Set tIO=$IO
 Set sc=##class(Ens.Director).CreateBusinessService("Svc",.svc)
 Set:sc sc=svc.SendRequestSync("BP",##class(Ens.StringRequest).%New("{input}"),.resp,30)
 Use tIO
 Write "BP|{input}|",$Select('sc:"ERR "_$System.Status.GetErrorText(sc),$IsObject($Get(resp)):resp.StringValue,1:"NORESP"),$C(10)"#
            ))
            .await;
    }
    out += &l
        .run(
            r#" Set tIO=$IO
 Set sc=##class(Ens.Director).CreateBusinessService("Svc",.svc)
 For in="Apple","Banana" { Set:sc sc=svc.SendRequestAsync("Router",##class(Ens.StringRequest).%New(in)) }
 Hang 3
 Use tIO
 Write "ROUTE|OpA|",$Get(^IADTest136("OpA",1)),$C(10)
 Write "ROUTE|OpB|",$Get(^IADTest136("OpB",1)),$C(10)"#,
        )
        .await;
    for (want, why) in [
        (
            "BP|D|invalid|dup;seen;|ok:Agent|rule#1(dup):when#1",
            "rule return, assign, function, reason",
        ),
        (
            "BP|C|routine|card;seen;|ok:Agent|rule#2(card):when#1",
            "second rule fires",
        ),
        (
            "BP|X|further_review|seen;|ok:Agent|",
            "ruleset-level return",
        ),
        ("BP|SLOW|further_review|seen;||", "timeout skips <response>"),
        ("ROUTE|OpA|Apple", "routing rule, when"),
        ("ROUTE|OpB|Banana", "routing rule, otherwise"),
    ] {
        if !out.lines().any(|line| line.trim() == want) {
            problems.push(format!("{why}: expected {want:?} in\n{out}"));
        }
    }

    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems.join("\n"))
    }
}

#[tokio::test]
#[ignore]
async fn interop_ui_136_facts_the_skill_states() {
    let l = live();
    l.cleanup().await;
    let result = body(&l).await;
    // IAD136_KEEP=1 leaves the classes compiled for a look in the Portal; it is never set in CI.
    if std::env::var("IAD136_KEEP").is_err() {
        l.cleanup().await;
    }
    assert!(
        result.is_ok(),
        "{}",
        result.as_ref().err().cloned().unwrap_or_default()
    );
}

/// The skill's own examples compile and open in the editors. They are read from SKILL.md, with
/// `MyApp.` renamed into the test package, so an edit that breaks an example fails here.
#[tokio::test]
#[ignore]
async fn interop_ui_136_the_skill_examples_compile() {
    let l = live();
    l.cleanup().await;
    let skill = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../skills/skills/ensemble-production/SKILL.md"),
    )
    .expect("SKILL.md");
    let blocks: Vec<String> = skill
        .split("```xml\n")
        .skip(1)
        .filter_map(|b| b.split("\n```").next())
        .map(|b| b.replace("MyApp.", "IADTest136."))
        .collect();
    let find = |marker: &str| {
        blocks
            .iter()
            .find(|b| b.contains(marker))
            .unwrap_or_else(|| panic!("no example containing {marker}"))
            .lines()
            .filter(|line| !line.trim_start().starts_with("<!--"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let bpl = find("XData BPL of");
    let rule = find("XData RuleDefinition of");
    let dtl = find("XData DTL of");
    let classes = [
        ("IADTest136.Fn", FN.to_string()),
        (
            "IADTest136.IntakeBP",
            format!(
                "Class IADTest136.IntakeBP Extends Ens.BusinessProcessBPL\n{{\n\nXData BPL [ XMLNamespace = \"http://www.intersystems.com/bpl\" ]\n{{\n{bpl}\n}}\n\n}}"
            ),
        ),
        (
            "IADTest136.IntakeRule",
            format!(
                "Class IADTest136.IntakeRule Extends Ens.Rule.Definition\n{{\n\nParameter RuleAssistClass = \"Ens.Rule.GeneralBusinessRuleAssist\";\n\nXData RuleDefinition [ XMLNamespace = \"http://www.intersystems.com/rule\" ]\n{{\n{rule}\n}}\n\n}}"
            ),
        ),
        (
            "IADTest136.UpperDTL",
            format!(
                "Class IADTest136.UpperDTL Extends Ens.DataTransformDTL [ DependsOn = Ens.StringRequest ]\n{{\n\nXData DTL [ XMLNamespace = \"http://www.intersystems.com/dtl\" ]\n{{\n{dtl}\n}}\n\n}}"
            ),
        ),
    ];
    let mut problems = Vec::new();
    for (name, src) in &classes {
        let errs = l.put(name, src).await;
        if !errs.is_empty() {
            problems.push(format!("{name}: {errs:?}"));
        }
    }
    let editors = l.has_editor_api().await;
    for (kind, name) in [
        ("bpl", "IADTest136.IntakeBP"),
        ("rules", "IADTest136.IntakeRule"),
        ("dtl", "IADTest136.UpperDTL"),
    ]
    .into_iter()
    .filter(|_| editors)
    {
        let got = l.editor(kind, name).await;
        if got != 200 {
            problems.push(format!("editor {kind}/{name}: HTTP {got}"));
        }
    }
    l.cleanup().await;
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}
