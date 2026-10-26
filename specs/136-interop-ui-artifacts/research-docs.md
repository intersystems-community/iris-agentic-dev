# 136 research: rules, BPL and DTL as editor-visible artifacts (IRIS 2026.2)

Date: 2026-10-17. Docs are the IRIS 2026.2 set (Algolia index `group:"IRIS2026.2"`). Live checks ran on `iris-dev-iris` (IRIS for UNIX 2026.2.0L build 208U, Community), namespace BENCHMARK, Atelier REST on `localhost:52780`.

Source tags: **DOCS** (URL), **LIVE** (command and output), **UNKNOWN**.

Live probe classes were all in package `IAD136Probe` and were deleted afterwards (Atelier `DELETE /api/atelier/v1/BENCHMARK/doc/<name>.cls` for all 32 classes, generated ones included; `SELECT Name FROM %Dictionary.ClassDefinition WHERE Name %STARTSWITH 'IAD136'` returns nothing). The rule generator also writes `^IRIS.TempEnsRule("class",<cls>)`; I killed those five probe subscripts. No `Ens_Rule.Log` or `Ens_Rule.DebugLog` rows were written (`SELECT COUNT(*) ... WHERE RuleName %STARTSWITH 'IAD136Probe'` returns 0), because every evaluation used log level `e`.

"Read source" below means the class source fetched read-only with `curl -u _SYSTEM:SYS http://localhost:52780/api/atelier/v1/BENCHMARK/doc/<Class>.cls`.

## F1. How rules are stored, enumerated and loaded

**Answer.** Current rules are classes that extend `Ens.Rule.Definition` and hold an `XData RuleDefinition` block (`XMLNamespace = "http://www.intersystems.com/rule"` by convention). `Ens.Rule.RuleSet` (table `Ens_Rule.RuleSet`) is the pre-2012.1 persistent format. It is legacy, and at runtime it is read only when `^EnsPortal.Settings("UseOldRule")` is set. Enumerate rule classes with the `%Dictionary.ClassDefinition:SubclassOf` query on `Ens.Rule.Definition`, the same call the Rule Editor's REST API makes. Load a rule's structure with `Ens.Rule.Model.ruleDefinition.LoadFromClass()`.

Classes, not rows:

- DOCS: "Internally all rule definitions are classes ... `Class Demo.ComplexMap.Rule.SemesterBatchRouting Extends Ens.Rule.Definition { Parameter RuleAssistClass = "EnsLib.MsgRouter.RuleAssist"; XData RuleDefinition [ XMLNamespace = "http://www.intersystems.com/rule" ] {...} }`" (<https://docs.intersystems.com/iris20262/csp/docbook/Doc.View.cls?KEY=EBUS_intro#EBUS_intro_classes>).
- DOCS: "A business rule is a class based on Ens.Rule.Definition" (<https://docs.intersystems.com/iris20262/csp/docbook/Doc.View.cls?KEY=EGIN_busproc#EGIN_busproc_busrules>).
- DOCS, `Ens.Rule.Definition`: "All business rule and routing rule classes are defined from this rule. A rule (definition) class may contain multiple rule sets" (<https://docs.intersystems.com/iris20262/csp/documatic/%25CSP.Documatic.cls?LIBRARY=ENSLIB&CLASSNAME=Ens.Rule.Definition>).

`Ens.Rule.RuleSet` is legacy:

- DOCS, `Ens.Rule.RuleDefinition` (superclass of `Ens.Rule.RuleSet`, table `Ens_Rule.RuleDefinition`): "Legacy: defined the interface for Business rules. Persistent routing rules and business rules were implemented as subclasses of this class. Now the EvaluateRulesEx() method serves as a passthrough to the new (as of Ensemble 2012.1) class-defined routing rules and business rules. The rest of the contents of this class serve for conversion from old rule imports to the new style rule class definitions, which are derived from Ens.Rule.Definition." (<https://docs.intersystems.com/iris20262/csp/documatic/%25CSP.Documatic.cls?LIBRARY=ENSLIB&CLASSNAME=Ens.Rule.RuleDefinition>). `Ens.Rule.RuleSet` is documented as "persistent extends (Ens.Rule.RuleDefinition) SQL Table Name: Ens_Rule.RuleSet" (<https://docs.intersystems.com/iris20262/csp/documatic/%25CSP.Documatic.cls?LIBRARY=ENSLIB&CLASSNAME=Ens.Rule.RuleSet>).
- LIVE, read source of `Ens.Rule.RuleDefinition.EvaluateRulesEx`. It is the only runtime path into the rows, and it leaves for the class engine unless the old-rule switch is on:

  ```objectscript
  if '+$g(^EnsPortal.Settings("UseOldRule")) {
      Quit ##class(Ens.Rule.Definition).EvaluateRules(pRuleName,pSessionId,pContext,pActivityName,.pReturnValue,.pReason,pLogLevel)
  }
  ; ... only below here: Lock +^Ens.Rule.RuleDefinitionD(...), ..OpenEffective(...)
  ```

- LIVE: `docker exec -i iris-dev-iris iris session IRIS -U BENCHMARK` with `Write $G(^EnsPortal.Settings("UseOldRule"),"<undef>")` printed `UseOldRule=<undef>`. On this container nothing at runtime reads `Ens_Rule.RuleSet`.
- LIVE, read source of `Ens.BPL.Rule.GenerateCode`: a BPL `<rule>` compiles to `##class(Ens.Rule.RuleDefinition).EvaluateRulesEx(...)`, which is the passthrough above. So BPL runs class rules too.
- LIVE: the one `Ens_Rule.RuleSet` row in BENCHMARK is iad's own test fixture, not a rule anyone wrote. `SELECT %ID, HostClass, FullName, Version, x__classname FROM Ens_Rule.RuleDefinition` returned `EnsLib.MsgRouter.RoutingEngine||IADFixture.RuleSet||180`, `~Ens.Rule.RuleSet~`. It is seeded by `seed_interop_fixture` in `crates/iris-agentic-dev-core/src/testing.rs:1814-1822` (`##class(Ens.Rule.RuleSet).%New()` + `%Save()`).
- **Finding for 136:** `iris_business_rule_info` (`crates/iris-agentic-dev-core/src/tools/interop.rs:2128-2160`) lists and gets rules from `Ens_Rule.RuleSet`. It therefore never sees a rule class written in the Rule Editor or in an IDE, and its one hit is the fixture row.
- Converting old rows to classes: `Ens.Rule.Upgrade` exists with `ConvertAll(qspec:%String)` and `ConvertOne(pHostName:%String, pRuleLongName:%String, pClassName:%String, qspec:%String)` (LIVE: `SELECT Name, FormalSpec FROM %Dictionary.CompiledMethod WHERE parent='Ens.Rule.Upgrade' AND Origin='Ens.Rule.Upgrade'`). The class has no description.

Enumerating rule classes:

- LIVE, read source of `%Api.InteropEditors.v1.impl.GetRules` (the method behind `GET /api/interop-editors/v6/:namespace/rules`, which every later version inherits through `##super`):

  ```objectscript
  set tSC = tStatement.%PrepareClassQuery("%Dictionary.ClassDefinition","SubclassOf",0)
  set tResult = tStatement.%Execute("Ens.Rule.Definition")
  ```

  It then filters by the user's per-rule-type read permission.

- A second registry is the class projection `Ens.Projection.Rule`, which keeps `^Ens.Rule("class",<class>)` and `^Ens.Rule("alias",<alias>)`. `GetRuleNameList(Output list)` reads both, and `ResolveRuleAlias(alias)` resolves a name (LIVE, read source).
- LIVE comparison, with five probe rule classes, one of which failed to compile (`IAD136Probe.RuleQual`):
  - `SubclassOf("Ens.Rule.Definition")` returned all five, `RuleQual` included.
  - `GetRuleNameList` returned all five, `RuleQual` included.
  - `curl -u _SYSTEM:SYS http://localhost:52780/api/interop-editors/v6/BENCHMARK/rules` returned `["IAD136Probe.BP.Rule","IAD136Probe.RuleBare","IAD136Probe.RuleNoAssist","IAD136Probe.RuleNoNS","IAD136Probe.RuleQual"]`.

  None of the three tells you whether the class compiled. To see that, check `%Dictionary.CompiledClass` and the routine.

Loading a rule's structure (LIVE, `%Dictionary.CompiledMethod.FormalSpec`):

| Class                           | Method                                            | Signature                                                                                                                                                                                                                                                        |
| ------------------------------- | ------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `Ens.Rule.Model.ruleDefinition` | `LoadFromClass` (classmethod)                     | `(pClassname As %String = "", Output pRuleDefinition As Ens.Rule.Model.ruleDefinition, pLoadConstraintCache As %Boolean = 1, pConcurrency As %Integer = -1) As %Status`                                                                                          |
| `Ens.Rule.Model.ruleDefinition` | `SaveAs` (instance)                               | `(pClassName As %String) As %Status`                                                                                                                                                                                                                             |
| `Ens.Rule.Definition`           | `EvaluateRules` (classmethod, Final)              | `(pRuleName As %String, pSessionId As %String = "", pContext As %RegisteredObject, pActivityName As %String = "", Output pReturnValue As %String, Output pReason As %String, pLogLevel As %String = {$$$eRuleLogTypeReturn}) As %Status`                         |
| `Ens.Rule.Definition`           | `evaluateRuleDefinition` (generated per subclass) | `(pContext As %RegisteredObject, Output pRuleSet, Output pEffectiveBegin, Output pEffectiveEnd, Output pReturnValue, Output pReason, pLogLevel = {$$$eRuleLogTypeReturn}, pDebugId As %Integer = "") As %Status`                                                 |
| `Ens.Rule.RuleDefinition`       | `EvaluateRulesEx` (classmethod, Final)            | `(pRuleName As %String, pSessionId As %String = "", pContext As %RegisteredObject, pActivityName As %String = "", Output pReturnValue As %String, Output pReason As %String, pLevel As %Integer = 1, pLogLevel As %String = {$$$eRuleLogTypeReturn}) As %Status` |
| `Ens.Projection.Rule`           | `ResolveRuleAlias`                                | `(alias As %String = "") As %String`                                                                                                                                                                                                                             |
| `Ens.Projection.Rule`           | `GetRuleNameList`                                 | `(Output list As %String) As %String`                                                                                                                                                                                                                            |

- DOCS: `Ens.Rule.Model.ruleDefinition` has properties `alias, assistClass, context, contextEditable, description, hasConstraints, hasSegments, isRoutingRule, production, ruleSets, ruleType, variables` and the methods `LoadFromClass` and `SaveAs` (<https://docs.intersystems.com/iris20262/csp/documatic/%25CSP.Documatic.cls?LIBRARY=ENSLIB&CLASSNAME=Ens.Rule.Model.ruleDefinition>). The class reference prints no argument lists; the signatures above come from the live dictionary.
- LIVE: the Rule Editor's `GetRule` calls `##class(Ens.Rule.Model.ruleDefinition).LoadFromClass(ruleClass, .ruleDef, 0, 0)` and then `ruleDef.%JSONExportToString(.ruleJSON)`. `LoadFromClass` fails with `<Ens>ErrXDataBlockNotDefined` when the class has no XData named `RuleDefinition`. Running it on a code-based BP printed `ERROR <Ens>ErrXDataBlockNotDefined: XDATA block 'RuleDefinition' is not defined in class 'IAD136Probe.CodeBP'`.
- LIVE: the model's tree is `ruleSets` → `actions` (`Ens.Rule.Model.rule`, each with `name` and `whens` → `condition`, `actions`), plus ruleSet-level `return` actions. On a probe rule, `rd.ruleSets.GetAt(1).actions.GetAt(1)` was `Ens.Rule.Model.rule`, `name=dup`. The REST v6 JSON reshapes `whens` into `conditions[].if/then`.

## F2. RuleAssistClass values (rule types)

**Answer.** General business rule: `Ens.Rule.GeneralBusinessRuleAssist`. Routing rule: `EnsLib.MsgRouter.RuleAssist`. HL7 routing rule: `EnsLib.HL7.MsgRouter.RuleAssist`, which exists only on IRIS for Health and HealthConnect. A BPL `<rule>` calls a general business rule.

LIVE: `SubclassOf("Ens.Rule.Assist")`, printing `GetDescription()`, `GetIsRoutingRule()` and `GetContextClass()` for each. `GET /api/interop-editors/v6/BENCHMARK/rules/types` returned the same seven.

| Rule type (`GetDescription`)                                    | RuleAssistClass                             | Routing? | Fixed context class                              |
| --------------------------------------------------------------- | ------------------------------------------- | -------- | ------------------------------------------------ |
| General Business Rule                                           | `Ens.Rule.GeneralBusinessRuleAssist`        | 0        | none (editable; for BPL use `<BPClass>.Context`) |
| General Message Routing Rule                                    | `EnsLib.MsgRouter.RuleAssist`               | 1        | `EnsLib.MsgRouter.RoutingEngine`                 |
| Virtual Document Message Routing Rule                           | `EnsLib.MsgRouter.VDocRuleAssist`           | 1        | `EnsLib.MsgRouter.VDocRoutingEngine`             |
| Virtual Document Message Routing Rule (compatible mode), hidden | `EnsLib.MsgRouter.VDocRuleAssistCompatible` | 1        | `EnsLib.MsgRouter.VDocRoutingEngine`             |
| Segmented Virtual Document Message Routing Rule                 | `EnsLib.EDI.MsgRouter.SegmentedRuleAssist`  | 1        | `EnsLib.EDI.MsgRouter.SegmentedRoutingEngine`    |
| Creation Rule for Managed Alerts                                | `Ens.Alerting.Rule.CreateAlertAssist`       | 0        | `Ens.Alerting.Context.CreateAlert`               |
| Overdue Rule for Managed Alerts                                 | `Ens.Alerting.Rule.OverdueAlertAssist`      | 0        | `Ens.Alerting.Context.OverdueAlert`              |

HL7:

- LIVE: `##class(%Dictionary.CompiledClass).%ExistsId("EnsLib.HL7.MsgRouter.RuleAssist")` returns `0` on `iris-dev-iris`, and so does `EnsLib.HL7.Message`. Community IRIS ships no HL7 classes.
- DOCS (IRIS for Health 2026.2): "EnsLib.HL7.MsgRouter.RuleAssist abstract extends (EnsLib.EDI.MsgRouter.SegmentedRuleAssist)" (<https://docs.intersystems.com/irisforhealth20262/csp/documatic/%25CSP.Documatic.cls?LIBRARY=ENSLIB&CLASSNAME=EnsLib.HL7.MsgRouter.RuleAssist>). There is also `EnsLib.HL7.MsgRouter.RuleAssistCompatible`.
- DOCS, older table (Ensemble 2018.1): "General Business Rule Ens.Rule.GeneralBusinessRuleAssist; HL7 Message Routing Rule EnsLib.HL7.MsgRouter.RuleAssist; General Message Routing Rule EnsLib.MsgRouter.RuleAssist; Virtual Document Message Routing Rule EnsLib.MsgRouter.VDocRuleAssist" (<https://docs.intersystems.com/ens201813/csp/docbook/Doc.View.cls?KEY=EBUS_rules#EBUS_rule_editor>). The HL7 example there uses `context="EnsLib.HL7.MsgRouter.RoutingEngine"`. I found no 2026.2 page that restates this table: UNKNOWN whether the table is still current, but the class reference above matches it.

Which type BPL calls:

- DOCS: "General business rule sets ... You invoke this type of rule set using the BPL `<rule>` element. Routing rule sets ... You use a routing engine business process to invoke this type of rule set." (<https://docs.intersystems.com/iris20262/csp/docbook/Doc.View.cls?KEY=EBUS_editor#EBUS_editor_rule_sets>)
- DOCS: "The send, delete, and delegate actions should not be used within a BPL `<rule>`. If you include them, the action will not be executed and instead a string value will be returned that includes the given action." (<https://docs.intersystems.com/iris20262/csp/docbook/Doc.View.cls?KEY=EBUS_rulesets#EBUS_ruleset_actions>)

## F3. BPL `<rule>` attributes, reasonLocation, multiple return values

**Answer.** The attributes are `name` (required), `rule` (required), `ruleContext`, `resultLocation`, `reasonLocation`, plus the common `disabled`, `xpos`, `ypos`, `xend`, `yend` and an `<annotation>` child. `reasonLocation` receives one string listing every rule and `when` that fired, in order and comma-separated. It is not just the rule that decided the outcome. A rule returns exactly one value, but its `assign` actions can set any number of context properties.

- DOCS, attributes (<https://docs.intersystems.com/iris20262/csp/docbook/Doc.View.cls?KEY=EBPL_rule#EBPL_rule_details>):
  - `rule`: "If the rule is not defined or otherwise cannot be found at runtime, the rule will return a default value of "" (an empty string)."
  - `ruleContext`: "an expression that identifies the object to pass to the rules engine ... By default the rule passes the business process execution context". A replacement object "must have a property called %Process of type Ens.BusinessProcess".
  - `resultLocation`: "The location in which to store the return value of the rule."
  - `reasonLocation`: "The rule reason is a string indicating why a business rule reached its decision. For example, "Rule 1" or "Default". If the business rule is empty ... the reason given for the decision is Rule Missing."
- DOCS, `rule` resolution: "If any `<rule>` element identifies a simple Rule Name, InterSystems IRIS automatically prepends a Package Name that is equal to the full package and class name of the BPL business process" (<https://docs.intersystems.com/iris20262/csp/docbook/Doc.View.cls?KEY=EBPL_rule#EBPL_rule_desc>). For example, `rule="Rule"` inside `IAD136Probe.BP` means the class `IAD136Probe.BP.Rule`.
- DOCS, `Ens.BPL.Rule.ReasonLocation`: "the reason value (i.e., which rule actually fired)" (<https://docs.intersystems.com/iris20262/csp/documatic/%25CSP.Documatic.cls?LIBRARY=ENSLIB&CLASSNAME=Ens.BPL.Rule>).
- DOCS, single value: "All rules return a single value ... The single value returned from a rule may be any literal value such as an integer number, decimal number, or text string." (<https://docs.intersystems.com/iris20262/csp/docbook/Doc.View.cls?KEY=EBPL_rule#EBPL_rule_desc>)
- LIVE, reason format. In `Ens.Rule.Generator`, each `when` that is entered runs `set pReason=pReason_","_<path>`, and the method ends with `set pReason=$e(pReason,2,*)`. The path is `rule#<n>(<name>):when#<m>`, or `:otherwise`.
- LIVE, behaviour. I built a probe general rule (`RuleAssistClass = "Ens.Rule.GeneralBusinessRuleAssist"`, context `IAD136Probe.BP.Context`) with two rules. Rule `dup` has a `when` with an `assign` and no `return`. Rule `card` has a `when` with an `assign` and a `return "routine"`. Then:

  ```objectscript
  Set ctx=##class(IAD136Probe.BP.Context).%New("") Set ctx.Reasons=""
  Set tSC=##class(Ens.Rule.Definition).EvaluateRules("IAD136Probe.BP.Rule","",ctx,"probe",.ret,.reason,"e")
  ```

  Output: `sc=1 ret=routine reason=rule#1(dup):when#1,rule#2(card):when#1 Reasons=dup found; card held;`. `EvaluateRulesEx` (the path BPL takes) gave the same `ret` and `reason`.

- Both of the docs' examples ("Rule 1", "Default") are out of date for class rules. The live format is `rule#N(name):when#M`, and a ruleset-level `<return>` adds no entry. I did not run a rule where nothing fired, so what `reason` holds in that case is UNKNOWN; from the generator source it should be `""`.
- Can a general rule return a list of reasons? Not as a return value: `pReturnValue` is one `%String`. Three ways around that. First, `assign` actions write any context properties: LIVE above, two rules appended to `context.Reasons`, a `%String`, with `Reasons_"..."`. Second, return a delimited or JSON string. Third, let a FunctionSet function (F4) build the string. An `assign` in a rule compiles to `set <prop>=<value>`, so I would not expect `append` on a BPL list property to work from inside a rule. That is not verified; it is UNKNOWN.

## F4. Custom functions: Ens.Rule.FunctionSet

**Answer.** Subclass `Ens.Rule.FunctionSet` and add `Final` classmethods; only methods defined in that class count. A function body is plain ObjectScript, so it can run SQL. In a rule condition you call it by its **bare method name**. In 2026.2 both qualified forms fail to compile.

- DOCS (<https://docs.intersystems.com/iris20262/csp/docbook/Doc.View.cls?KEY=EGDV_adv#EGDV_adv_custom_utilfunctions>):
  - "Create a new class that is a subclass of Ens.Rule.FunctionSet. The class can inherit from other classes as well, but only class methods defined in this class become utility functions."
  - "There is no support for polymorphism, so to be precise, you must mark these class methods as final."
  - The page says both "user-defined method names must be fully qualified with the class that they belong to" and, later, that a business rule "has simply referenced the function by name". These two statements contradict each other; the live test below shows the bare name is the form that works.
  - In DTL the call is written `##class(HP.Util.funciones).normalizaSexo(source.{Sex})`.
- DOCS: `Ens.Rule.Utilities.FindNonFunctionSetMethods` "can be used to look for classmethods in subclasses of Ens.Rule.FunctionSet that might also mistakenly have been set to have other super class(es) Any method names returned cannot be used in Rules" (<https://docs.intersystems.com/iris20262/csp/documatic/%25CSP.Documatic.cls?LIBRARY=ENSLIB&CLASSNAME=Ens.Rule.Utilities>).
- LIVE, SQL inside a function. `IAD136Probe.Fn Extends Ens.Rule.FunctionSet` has `ClassMethod CountClasses(pPrefix) As %Integer [ Final ]`, which runs `%SQL.Statement.%ExecDirect(, "SELECT COUNT(*) AS C FROM %Dictionary.ClassDefinition WHERE Name %STARTSWITH ?", pPrefix)`. The rule condition `CountClasses("IAD136Probe")>0` compiled, evaluated true, and produced `ret=bare-ok reason=rule#1(bare):when#1`.
- LIVE, the qualified forms fail to compile (Atelier `POST /action/compile?flags=cuk`):
  - `IAD136Probe.Fn.CountClasses("IAD136Probe")>0` gave `ERROR <Ens>ErrParsingExpression: ... ERROR <Ens>ErrInvalidToken: Invalid token at offset 28`.
  - `##class(IAD136Probe.Fn).CountClasses("IAD136Probe")>0` gave `ERROR <Ens>ErrInvalidName: Invalid name at offset 1`.
- Restrictions:
  - The comparison table lists "Calling custom code: No" and "Invoking SQL: No" for business rules (<https://docs.intersystems.com/iris20262/csp/docbook/Doc.View.cls?KEY=EGDV_busproc#EGDV_busproc_tools>). A FunctionSet function is the supported way in.
  - The function runs inside the caller's job when the rule is evaluated.
  - The Rule Editor's Test button runs functions for real. DOCS: "any functions in the condition are executed as if the message ran through the production" (<https://docs.intersystems.com/iris20262/csp/docbook/Doc.View.cls?KEY=EBUS_debug#EBUS_debug_test>). A function used in a rule should therefore be read-only: a duplicate check or a lookup by member id is fine, an UPDATE is not.
  - The function returns one scalar, which the condition or assign uses as an expression.

## F5. BPL: synchronous call with timeout, `<sync>`, context properties, `<code>`

**Answer.** Use `<call name target async="0" timeout="secs">` with `<request>` and `<response>` children. A synchronous `<call>` is compiled as a send followed by a generated `<sync>` on that call. On timeout `synctimedout` is set to 1 and the `<response>` block does not run. Declare context properties with `<context><property .../></context>`. A `<code>` activity appears in the BPL editor as a `code` shape.

- DOCS, `<call>` (<https://docs.intersystems.com/iris20262/csp/docbook/Doc.View.cls?KEY=EBPL_call>):
  - `name`, required. `target`, required: a literal or `@` indirection to a context variable. `async`, required: 1 or 0.
  - `timeout`, optional: "Sets a timeout on a synchronous call. The timeout value is used only when the async attribute of the `<call>` is set to 0".
  - `<request type>` is required and `<response type>` optional. "A `<call>` element with async='False' but with no `<response>` block defined behaves as if async='True'."
  - "If the call is synchronous, an optional timeout can be specified using the timeout attribute on the `<call>` element itself. This attribute cannot be used for asynchronous calls."
- LIVE, read source of `Ens.BPL.Call.GenerateCode`: `If ..Async = 0 { Set tSync = ##class(Ens.BPL.Sync).%New() ... Set tSync.Calls=..Name,tSync.Timeout=..Timeout ... tSync.GenerateCode(pCompiler) }`. `Validate` adds `$$$EnsErrBPLASyncTimeoutMustBeOnSync` when `async=1` and `timeout` is set.
- LIVE: a probe BPL with `<call name='Agent' target='IAD136ProbeOp' async='0' timeout='5'>` and `<call name='AgentAsync' ... async='1'>` + `<sync name='Wait' calls='AgentAsync' type='all' timeout='5'/>` + `<if condition='synctimedout'>` compiled. I did not run it in a production, so the timeout was not exercised; I also did not test `timeout` as an expression such as `process.AgentWait`.
- DOCS, `<sync>` (<https://docs.intersystems.com/iris20262/csp/docbook/Doc.View.cls?KEY=EBPL_sync>):
  - Attributes: `calls` (required, comma list or `@`), `type` (`all`, the default, or `any`), `timeout`, `allowresync`.
  - "Any responses received after the timeout period are marked with a status of Discarded."
  - "the business process is saved to disk and the job in which it was running is freed up ... while the `<sync>` element is waiting."
- DOCS: `synctimedout` is 0 when no call timed out, 1 when at least one timed out, and 2 when one was interrupted; responses are read with `syncresponses.GetAt("name")` (<https://docs.intersystems.com/iris20262/csp/docbook/Doc.View.cls?KEY=EBPL_vars#EBPL_vars_synctime>).
- DOCS, a failed call ends the process: "status automatically receives the returned %Status value returned from any `<call>` activity ... if any `<call>` activity fails, the BPL business process immediately exits and writes an Event Log" entry (<https://docs.intersystems.com/iris20262/csp/docbook/Doc.View.cls?KEY=EBPL_call#EBPL_call_use_of_the_ltassign_element>). This matters for F10.
- DOCS, context: `<context><property name="P1" type="%String"/></context>` at the top of `<process>`, with optional `initialexpression`, `collection="list|array"` and `<parameters>`. `<process contextsuperclass="...">` lets several processes share one context class (<https://docs.intersystems.com/iris20262/csp/docbook/Doc.View.cls?KEY=EBPL_context>, <https://docs.intersystems.com/iris20262/csp/docbook/Doc.View.cls?KEY=EBPL_process>).
- LIVE: the probe context `<property name='Reasons' type='%String'><parameters><parameter name='MAXLEN' value='4000'/></parameters></property>` and `<property name='ReasonList' type='%String' collection='list'/>` compiled into `IAD136Probe.BP.Context`. The editor API reported them as `('Reasons','%String',None) ... ('ReasonList','%String','list')`.
- DOCS, `<code>`: it "executes one or more lines of user-written code"; "InterSystems strongly recommends that instead of including multiple lines of code within `<code>`, you invoke a class method" (<https://docs.intersystems.com/iris20262/csp/docbook/Doc.View.cls?KEY=EBPL_code#EBPL_code_desc>).
- LIVE, shown in the editor: `curl -u _SYSTEM:SYS http://localhost:52780/api/interop-editors/v6/BENCHMARK/bpl/IAD136Probe.BP` returned this shape list:
  - `start`
  - `rule Rules {ReasonLocation: context.RuleReason, ResultLocation: context.Outcome, Rule: Rule}`
  - `code Helper` (with the code body)
  - `sql Lookup`
  - `assign Push`
  - `call Agent {Async: 0, Target: IAD136ProbeOp, Timeout: 5}`
  - `call AgentAsync {Async: 1}`
  - `sync Wait {Calls: AgentAsync, Timeout: 5, Type: all, AllowResync: 0}`
  - `if TimedOut`, `join`, `trace T`, `end`

  So `<code>`, `<sql>` and `<rule>` all render as shapes.

## F6. DTL: Ens.DataTransformDTL versus Ens.DataTransform

**Answer.** Confirmed. The DTL editors open subclasses of `Ens.DataTransformDTL` that have `XData DTL`. A class that extends `Ens.DataTransform` and implements `Transform()` is a "custom transformation", which you edit in an IDE; the editors do not draw it.

- DOCS: "A data transformation is a class based on Ens.DataTransform or its subclass, Ens.DataTransformDTL. If you use Ens.DataTransformDTL, the transformation is called a DTL transformation. To create these transformations, you use a graphical editor ... If you use Ens.DataTransform, the transformation is a custom transformation. In this case, you must use a supported IDE." (<https://docs.intersystems.com/iris20262/csp/docbook/Doc.View.cls?KEY=EGIN_busproc#EGIN_busproc_xforms>)
- DOCS: "A DTL transformation is a class based on Ens.DataTransformDTL. In this case, you can create and edit the transformation visually in the DTL editor" (<https://docs.intersystems.com/iris20262/csp/docbook/Doc.View.cls?KEY=EGDV_xform#EGDV_xform_dtl>).
- LIVE, new editor API:
  - `GET /api/interop-editors/v6/BENCHMARK/dtl/IAD136Probe.DTL` (a subclass of `Ens.DataTransformDTL` with `XData DTL`) returned HTTP 200 with `"Actions":[{"ClassName":"Ens.DTL.Assign",...}]`.
  - `GET .../dtl/IAD136Probe.CustomDT` (a subclass of `Ens.DataTransform` with `Transform()`) returned HTTP 400 `'IAD136Probe.CustomDT' is not a DTL data transformation`.
  - Source of `%Api.InteropEditors.v5.impl.GetDTL`: `if '##class(%Api.InteropEditors.Utils).IsSubClass(dtlClass, "Ens.DataTransformDTL") { return ..%ReportRESTError(400,,1, $$$ERROR($$$InteropAPIInvalidDTL, dtlClass)) }`.
- LIVE, the legacy editor and the API both load through `Ens.DTL.UI.DTLDocument.GetTransform` → `GetDTLStream`, which looks for `cdef.XDatas.FindObjectId(cdef.Name_"||DTL")`. When it is missing the error is "Unable to find DTL block within class".

## F7. Which UIs open BPL, DTL and rule classes graphically

**Answer.** All three artifact types have a legacy Zen page and a new Angular application in 2026.2. None of these UIs opens a code-based `Ens.BusinessProcess` or a custom `Ens.DataTransform`.

| Artifact                       | Legacy (Zen) page                                   | New UI                                                               | Opens code-based classes?         |
| ------------------------------ | --------------------------------------------------- | -------------------------------------------------------------------- | --------------------------------- |
| Rule (`Ens.Rule.Definition`)   | `EnsPortal.RuleEditor`                              | `/ui/interop/rule-editor`, REST `/api/interop-editors`               | n/a; needs `XData RuleDefinition` |
| BPL (`Ens.BusinessProcessBPL`) | `EnsPortal.BPLEditor` (Business Process Designer)   | `/ui/interop/bpl-editor` (2026.1+, opt-in; also embedded in VS Code) | No                                |
| DTL (`Ens.DataTransformDTL`)   | `EnsPortal.DTLEditor` (Data Transformation Builder) | `/ui/interop/dtl-editor` (2025.1+, opt-in)                           | No                                |

- LIVE: `docker exec iris-dev-iris ls /usr/irissys/ui/interop/` lists `bpl-editor dtl-editor interop-editor message-viewer rule-editor`. In `Security.Applications` (%SYS) there are `/api/interop-editors` (dispatch `%Api.InteropEditors`) and `/ui/interop`. `%Api.InteropEditors.v1` through `v6` exist. The classes `EnsPortal.RuleEditor`, `EnsPortal.BPLEditor` and `EnsPortal.DTLEditor` are compiled, and the BPL and DTL pages link to `/ui/interop/bpl-editor/index.html?` and `/ui/interop/dtl-editor/index.html?`.
- DOCS, BPL: "Starting with 2026.1, the product includes a new BPL Editor. To access this application, click Open in the new UI on the existing BPL Editor." Access is Interoperability > Build > Business Processes (<https://docs.intersystems.com/iris20262/csp/docbook/Doc.View.cls?KEY=EBPLOLD_editorintro>, <https://docs.intersystems.com/iris20262/csp/docbook/Doc.View.cls?KEY=EBPL_editorintro>). The 2026.1 release notes say: "Users can opt in to use the new BPL user experience ... The new BPL Editor user interface is now embedded in VS Code." (<https://docs.intersystems.com/iris20262/csp/docbook/Doc.View.cls?KEY=GCRN_new20261#GCRN_new20261_modernizeinterop>)
- DOCS, DTL: "Starting with 2025.1, the product includes a new DTL editor. To access this application, click Try the new UI on the existing DTL Editor." Access is Interoperability > Build > Data Transformations (<https://docs.intersystems.com/iris20262/csp/docbook/Doc.View.cls?KEY=EDTLOLD_editorintro>, <https://docs.intersystems.com/iris20262/csp/docbook/Doc.View.cls?KEY=EDTL_editorintro>).
- DOCS, Rule Editor:
  - Reached at "Interoperability > Build > Business Rules", with existing rules at "Interoperability > List > Business Rules" (<https://docs.intersystems.com/iris20262/csp/docbook/Doc.View.cls?KEY=EBUS_editor>).
  - "The Rule Editor includes two web applications: /ui/interop/rule-editor and /api/interop-editors" (IRIS for Supply Chain 2023.1, <https://docs.intersystems.com/supplychain20231/csp/docbook/Doc.View.cls?KEY=EBUS_rules>).
  - "The Zen based Rule Editor has been replaced with a new Rule Editor ... The old Rule Editor is still accessible from the new Rule Editor ... click Open in Zen Rule Editor" (HealthShare 2026.1, <https://docs.intersystems.com/hs20261/csp/docbook/Doc.View.cls?KEY=EBUS_editor#EBUS_editor_zen>).
  - UNKNOWN: I found no equivalent "Zen Rule Editor" section in the IRIS 2026.2 index, though `EnsPortal.RuleEditor` is still present live.
  - The built-in applications page lists `/api/interop-editors` as "Rule Editor REST API" (<https://docs.intersystems.com/iris20262/csp/docbook/Doc.View.cls?KEY=GSA_manage_applications#GSA_manage_applications_systemapps>).
- DOCS: the new Production Configuration page opens the Rule Editor and the DTL Editor as stacked panels (<https://docs.intersystems.com/iris20262/csp/docbook/Doc.View.cls?KEY=ECONFIG_configpage#ECONFIG_intro_ui>).
- Code-based processes:
  - DOCS: "Only BPL processes support the business process execution context and a graphical display of logic." (<https://docs.intersystems.com/iris20262/csp/docbook/Doc.View.cls?KEY=EGDV_busproc#EGDV_busproc_intro>)
  - DOCS: "Custom business processes, which are based on the class Ens.BusinessProcess ... it is necessary to use a supported IDE" (<https://docs.intersystems.com/iris20262/csp/docbook/Doc.View.cls?KEY=EGIN_busproc#EGIN_busproc_types>).
  - LIVE: `GET /api/interop-editors/v6/BENCHMARK/bpl/IAD136Probe.CodeBP` returned HTTP 400 `'IAD136Probe.CodeBP' is not a BPL business process`.
  - LIVE: `GET .../processes?mode=bpl` lists only subclasses of `Ens.BusinessProcessBPL` (here `IAD136Probe.BP` was included and `IAD136Probe.CodeBP` was not).
  - LIVE: the legacy `EnsPortal.BPLEditor` loads through `Ens.BPL.UI.Diagram.Open`, which parses `XData BPL` (or `BPLError`) and returns no diagram without one.

## F8. What the editors need beyond a clean compile

**Answer.** Each editor needs the right superclass and an XData block with the right name. `XMLNamespace` is not needed, and neither is a production. A rule without `RuleAssistClass` still opens, but its type shows as `(unknown)`.

- Rule: the class extends `Ens.Rule.Definition` and has an XData named exactly `RuleDefinition` (LIVE: `LoadFromClass` source, `IDKEYExists(pClassname,"RuleDefinition")`).
  - Without `XMLNamespace`: LIVE, `IAD136Probe.RuleNoNS` (bare `XData RuleDefinition {...}`) compiled, and `GET .../rules/IAD136Probe.RuleNoNS` returned HTTP 200 with `"ruleType":"General Business Rule"`.
  - Without `RuleAssistClass`: LIVE, `IAD136Probe.RuleNoAssist` compiled and `GET .../rules/...` returned HTTP 200 with `"assistClass":"","ruleType":"(unknown)","contextEditable":true`. From the `LoadFromClass` source, with no assist class the flags `hasConstraints`, `isRoutingRule` and `hasSegments` are all 0, so the editor cannot offer type-specific actions or constraints. Set `Parameter RuleAssistClass = "Ens.Rule.GeneralBusinessRuleAssist"`.
  - The `context` attribute names the context class. For a BPL rule that is `<BPClass>.Context`, which exists only after the BPL compiles. I compiled the BPL first. I did not test compiling a rule against a missing context class (UNKNOWN).
  - The `production` attribute is optional. DOCS: "Production Name (Optional) For routing rules, provides the name of the production ... so the Rule Editor can offer predefined options" (<https://docs.intersystems.com/iris20262/csp/docbook/Doc.View.cls?KEY=EBUS_editor#EBUS_editor_rule_defn>). The probes used `production=""`.
- BPL: the class extends `Ens.BusinessProcessBPL` (the API checks `IsSubClass(bplClass, "Ens.BusinessProcessBPL")`, LIVE source) and has `XData BPL`. LIVE: `IAD136Probe.BPNoNS` (no `XMLNamespace`) compiled, and `GET .../bpl/IAD136Probe.BPNoNS` returned HTTP 200 with `"HasErrors":false`.
- DTL: the class extends `Ens.DataTransformDTL` and has `XData DTL`. LIVE: `IAD136Probe.DTLNoNS` (no `XMLNamespace`) compiled, and `GET .../dtl/IAD136Probe.DTLNoNS` returned HTTP 200.
- Permissions:
  - The REST API checks per-artifact permissions (`GetRulePerms`/`HasPermsByRuleClass`, `GetBPLPerms`/`HasBPLPerms`, `GetDTLPerms`/`HasDTLPerms`; LIVE source). I did not map which resource grants each.
  - DOCS, `EnsPortal.BPLEditor`: "User needs READ permissions on either the %Ens_Code or the %Ens_BPL resource to view this page", and WRITE to change it (<https://docs.intersystems.com/iris20262/csp/documatic/%25CSP.Documatic.cls?LIBRARY=ENSLIB&CLASSNAME=EnsPortal.BPLEditor>).
- In rule conditions, call custom functions by bare name (F4); the qualified forms do not compile.

## F9. Creating and deleting Ens.Rule.RuleSet rows cleanly

**Answer.** Rows are created with `%New()` + `%Save()`, or by `Ens.Rule.RuleDefinition.Load()` from XML. Delete them through objects: `##class(Ens.Rule.RuleSet).%DeleteId(HostClass_"||"_FullName_"||"_Version)`, or `##class(Ens.Rule.RuleDefinition).PurgeOldVersions(pHostClass, pName, pKeep)` to drop old versions. Do not use SQL `DELETE`. To respect the "touch nothing else" rule I did not create or delete a row.

- LIVE, read source of `Ens.Rule.RuleDefinition`:
  - `Index ID On (HostClass, FullName, Version) [ IdKey ]`. The middle key is `FullName`, not `Name`. `Name` is `SqlComputed` as `$p({FullName},"$",1)`, because `FullName` is `name$effectiveBegin$effectiveEnd`. The two are equal only when there are no effective dates.
  - `Version` has `InitialExpression = {$I(^Ens.Rule.RuleDefinitionC)}`, a global counter; the iad fixture is at 180. Data lives in `^Ens.Rule.RuleDefinitionD(HostClass,FullName,Version)`.
  - `Ens.Rule.RuleSet` has `Relationship Rules As Rule [ Cardinality = children ]`, so an object delete cascades to the child rules.
  - `%OnDelete` (an object callback) deletes the compiled rule-cache routine (`##class(Compiler).DeleteRoutine(name)`, since `COMPILABLE=1` for RuleSet) and fires `%Notify("Delete",...)` to every class in `^Ens.Rule.Notification` when the last version goes.
  - `LIVE: SELECT Name FROM %Dictionary.CompiledTrigger WHERE parent='Ens.Rule.RuleDefinition'` returns only `%OBJGUID`, `%OBJJRND` and `%OBJJRNIU`. No SQL trigger mirrors `%OnDelete`, so an SQL `DELETE FROM Ens_Rule.RuleSet` would leave the cache routine behind and skip the notification. This is inferred from the source; I did not run it.
  - Other helpers: `Open(pHostClass, pName, pVersion="", Output pStatus)`, `Exists(pHostClass, pName, pVersion="")`, `OpenEffective(...)`, `GetLatestVersion(pHostClass, pName)`, `PurgeOldVersions(pHostClass As %String, pName As %String, pKeep As %Integer = 5) As %Status` (which loops `..%DeleteId(pHostClass_"||"_pName_"||"_tVersion)`), `Load(pType, pSource, pVerbose=1, Output pLastObj, pHostClass="", pName="")` and `LoadFile(pFile, ...)`.
  - Class queries `EnumerateRules()`, `EnumerateRulesForProcess(pHostClass)` and `EnumerateRuleNames()` read the legacy table. They do not list rule classes.
- Bugs in iad (136 input). In `crates/iris-agentic-dev-core/src/tools/interop.rs:2152-2160` the comment says the id is "HostClass||Name||Version"; it is `HostClass||FullName||Version`. The code orders by `ID DESC`, which compares strings: version `99` sorts after `180`. That ordering only matters while the tool still reads the legacy table.
- The iad fixture creates its row with `%New()`/`%Save()` (`testing.rs:1816-1822`). To clean up, call `##class(Ens.Rule.RuleSet).%DeleteId("EnsLib.MsgRouter.RoutingEngine||IADFixture.RuleSet||<Version>")`, or loop `PurgeOldVersions("EnsLib.MsgRouter.RoutingEngine","IADFixture.RuleSet",0)`. I have not tested either.

## F10. What in IntakeBP.cls and Rules.cls has no direct BPL or rule equivalent

Files read, not edited: `~/ws/grievance-demo/src/Grievance/Process/IntakeBP.cls` (a code-based `Ens.BusinessProcess`) and `~/ws/grievance-demo/src/Grievance/Rules.cls` (plain classmethods; `Evaluate` returns `{outcome, reasons[], allowedAction}`).

| #   | Construct in the code                                                                                                       | Direct equivalent?                                                                                    | Closest supported construct                                                                                                                                                                                                                                                                                                           |
| --- | --------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1   | `Evaluate` returns a `%DynamicObject` with three fields                                                                     | No. A rule returns one `%String` (F3)                                                                 | Return `outcome` as the rule value; set `allowedAction` and the reasons with `assign` actions on context properties. LIVE: `assign` from two rules accumulated into `context.Reasons`.                                                                                                                                                |
| 2   | `reasons[]`, a list of human-readable sentences                                                                             | No. `reasonLocation` holds the engine path `rule#1(dup):when#1,...`, not domain text (F3)             | Accumulate a `%String` (or JSON text) in a context property inside the rule, then copy it to a list in BPL with `<assign action="append">` if a list is wanted. Rule `assign` on a list property is UNKNOWN.                                                                                                                          |
| 3   | Rules that add a reason and carry on (the confidential-address reason is added after every branch)                          | Yes, with ordering care                                                                               | A rule with no `<return>` fires and evaluation continues (LIVE). Put such rules before any rule that returns, or have every rule `assign` the outcome and end with one ruleset-level `<return>`.                                                                                                                                      |
| 4   | Early exit: `Invalid()` then `Quit`, type dispatch `If Type="id_card" ... ElseIf "billing"`                                 | Yes                                                                                                   | Rules are evaluated in order and `<return>` stops (DOCS: "evaluated sequentially until one of them is found to be true"). One rule per branch, with `when` conditions on `context` properties.                                                                                                                                        |
| 5   | SQL in rules: duplicate check, card order, address, name history, statements, payments, contacts                            | Not native: "Invoking SQL: No" for rules (F4)                                                         | A FunctionSet function called by bare name can run SQL and return a scalar (LIVE). Alternatively, run BPL `<sql>` before the `<rule>` to load context properties; it is a SELECT that keeps "Only the first row" (<https://docs.intersystems.com/iris20262/csp/docbook/Doc.View.cls?KEY=EBPL_sql>).                                   |
| 6   | Loops over many rows, one reason per row (`BillingRules` over statements and contacts)                                      | No. Rules have no loop; `foreach` exists only for segmented/HL7 routing rules (DOCS EBUS_rulesets)    | A FunctionSet function that loops and returns the joined reason text. Or a BPL `<foreach>` over a context collection filled by a `<code>` or `<call>`, with the per-row test in BPL `<if>`. Or keep the logic in a classmethod called from `<code>`.                                                                                  |
| 7   | `MemberIdIdxOpen` and `Grievance.Case.OpenOrFail(..., .pSC)` (object opens, error status by reference)                      | No rule construct                                                                                     | A FunctionSet function returning a value or "" for not found. In BPL, `<assign value='##class(...).Method(...)'>` or `<code>`. An error status in BPL comes from `<code>` setting `status`, or a `<throw>`.                                                                                                                           |
| 8   | `Update()`: variadic args, `UPDATE ... WHERE Status='new'`, SQLCODE 100 turned into an error (optimistic concurrency guard) | No. BPL `<sql>` is SELECT only ("executes an arbitrary embedded SQL SELECT statement", DOCS EBPL_sql) | A `<code>` activity that calls a classmethod (drawn as a `code` shape, F5), or a business operation that does the write, called with `<call>`. Writes should not live in a rule function, because the Rule Editor's Test runs functions (F4).                                                                                         |
| 9   | `$$$ThrowOnError` + `Try/Catch` turning any exception into the returned status                                              | Partly                                                                                                | BPL `<scope>` with `<catchall>`/`<catch>` and `<throw>`; the `status` variable. Not checked in detail here.                                                                                                                                                                                                                           |
| 10  | Agent call failure or timeout is logged and routing **continues**                                                           | Behaviour differs                                                                                     | In BPL a `<call>` whose status is an error ends the process (DOCS, F5). Wrap the call in `<scope>` + `<catchall>` to continue. A timeout on a synchronous `<call>` sets `synctimedout` and skips the `<response>` block (F5), which maps to the `'$IsObject($Get(tAgentResp))` branch.                                                |
| 11  | `$$$LOGWARNING(...)`                                                                                                        | No warning element                                                                                    | `<trace>` writes a trace, not an Event Log warning; `<alert>` sends an alert. Use `<code>` with `$$$LOGWARNING` to keep the same Event Log entry.                                                                                                                                                                                     |
| 12  | `SendRequestSync(..AgentTarget, req, .resp, ..AgentWait)` with both values as production settings (`Parameter SETTINGS`)    | Mostly                                                                                                | `<call target="@..." async="0" timeout="...">`: DOCS allow `@` indirection for `target` and an expression for `timeout`. A BPL class is still a class, so `Property AgentTarget`/`AgentWait` and `Parameter SETTINGS` can be added to it. Referencing them as `@process.AgentTarget` and `process.AgentWait` is UNKNOWN (not tested). |
| 13  | `Grievance.Audit.Log`/`LogHandoff`, `Grievance.AgentResult.ForCase` (helper classmethods)                                   | No dedicated element                                                                                  | `<assign>` with a `##class()` expression, or `<code>` calling the classmethod (the docs recommend a classmethod over inline code, F5).                                                                                                                                                                                                |
| 14  | Truncating reasons JSON over 1000 characters with `$Length`                                                                 | Yes                                                                                                   | An `<assign>` value expression (`$S($L(x)>1000:...,1:x)`).                                                                                                                                                                                                                                                                            |
| 15  | `Classify()` by keyword match                                                                                               | Partly                                                                                                | The built-in rule function `Contains(value,string)` (<https://docs.intersystems.com/iris20262/csp/docbook/Doc.View.cls?KEY=EBUS_utility_functions>), one `when` per keyword group, or a FunctionSet function.                                                                                                                         |
| 16  | `EvaluateJSON` (the agent tool reads the same answer)                                                                       | No                                                                                                    | Keep it as a classmethod. A rule class can be evaluated outside BPL with `##class(Ens.Rule.Definition).EvaluateRules(name,"",ctx,...)` against a context object (LIVE, F3), but then the reasons live on `ctx`, not in a JSON return.                                                                                                 |
| 17  | Response left unset (async caller)                                                                                          | Yes                                                                                                   | `<process response=...>` is optional (DOCS EBPL_process).                                                                                                                                                                                                                                                                             |

## Open items

- Which Rule Editor (new or Zen) the 2026.2 Interoperability > Build > Business Rules menu opens by default: only the HealthShare 2026.1 doc states that the Zen editor was replaced. Both are installed live.
- HL7 rule types could not be checked live (Community image, no `EnsLib.HL7`); the class reference is from the IRIS for Health 2026.2 docs.
- Not exercised live: a production run of the synchronous `<call timeout>`, `reasonLocation` when no rule fires, a rule `assign` into a BPL list property, `@process.<setting>` in `target`/`timeout`, compiling a rule whose context class is missing, and create/delete of an `Ens.Rule.RuleSet` row.

## F11. pyprod 0.2.0: what a Portal edit does to a Python-defined production (T040)

**Answer.** An edit made the way the Portal's Production Configuration page makes it is overwritten
the next time the Python definition loads. Whether System Default Settings override a setting the
Python definition sets is UNKNOWN: the check below could not tell.

- DOCS: DP-454977 "PyProd round-trip editing" was closed as Won't Do on 2026-09-14 ("Modifications made
  in the GUI would be overwritten by client-side source updates").
- LIVE, 2026-10-17, on a throwaway `intersystemsdc/iris-community:2026.2` container (`iad136-pyprod`,
  port 39773, removed afterwards), pyprod 0.2.0 installed with
  `irispython -m pip install --target /usr/irissys/mgr/python intersystems-pyprod==0.2.0`, USER enabled
  with `%EnsembleMgr.EnableNamespace("USER",1)`:
  - `/tmp/prod136.py` defines `PP136.Prod` with one `OperationItem("Out",
"EnsLib.File.PassthroughOperation", host_settings={"AlertOnError": True})`. Loading it with the
    `intersystems_pyprod` CLI printed `Load finished successfully.`; the item's setting read `True`.
  - The Portal's path: open `Ens.Config.Production` `PP136.Prod`, set the item's `AlertOnError` setting
    to `0`, `%Save()` and `SaveToClass()`. Both returned 1, and the class's `ProductionDefinition` XData
    then held `AlertOnError">0<`.
  - Loading `/tmp/prod136.py` again put the XData back to `AlertOnError">True<`.
  - System Default Settings: an `Ens.Config.DefaultSettings` row for `PP136.Prod`/`Out`/`AlertOnError`
    = 0 saved (1). `Ens.Director.GetHostSettingValue("Out","AlertOnError")` on the running production
    answered `True` with the row and without it, which reads the definition and does not settle
    whether the running host saw 0. pyprod's API reference says system defaults override item settings
    "even when a value is explicitly set"; not measured here, so the skill does not say it.
