---
name: iris-ai-hub
tier: extra
author: tdyar
version: 0.2.1
managed_by: iris-agentic-dev
description: "IRIS AI Hub (%AI.* classes, EAP builds): where the upstream docs are and which file covers what, how to check them against the installed build, and the %AI.Agent / %AI.Tool / %AI.ToolSet / ConfigStore / Wallet / MCP server facts measured on 2026.3.0AI builds 139 and 154. Load when building or debugging AI Hub agents, tools, providers or MCP servers."
source: >-
  ai-hub-eap master 72749d6dbf0b856a60775378fa88d346bb79d4e4;
  ready-hackathon-dev-template 391c3d5 ai-hub-* skills by Gabriel Ing
---

# iris-ai-hub

AI Hub (`%AI.*`) ships only in Early Access builds, and each build changes names and signatures. Read the docs for concepts; read the installed classes for names. This skill says where each is.

## Where the docs are

- Repo: `https://github.com/intersystems-community/ai-hub-eap`, branch `master`.
- Read a file raw: `https://raw.githubusercontent.com/intersystems-community/ai-hub-eap/master/<path>`.
- The repo moves. Check its current state (latest commit, whether the file you want still exists) before relying on a file; the map below was recorded at `084b250` (2026-10-06).
- If GitHub is unreachable, skip the docs and work from the installed `%AI` classes alone (workflow step 2). Say that you did.

## Topic map

| Topic       | File                           |
| ----------- | ------------------------------ |
| ConfigStore | `Config_Store_Guide.md`        |
| MCP         | `MCP_Server_Guide.md`          |
| MCP         | `MCP_Server_Examples.md`       |
| SDK         | `ObjectScript_SDK_Guide.md`    |
| SDK         | `ObjectScript_SDK_Advanced.md` |
| SDK         | `ObjectScript_SDK_Examples.md` |
| LangChain   | `langchain_SDK.md`             |
| Samples     | `objectscript/cls/`            |

The SDK guide is over 3,000 lines. Fetch it once and search it for the class you need; do not read it top to bottom.

## Workflow

1. Read the build: `iris_info` with `what: "metadata"` (the `version` field), or `Write $ZVERSION` through `iris_execute`. Note the build number (for example `2026.3.0AI (Build 154U)`).
2. Read the installed `%AI` classes for the part you are about to use: `iris_symbols` or `docs_introspect` on the class, or `iris_query` on `%Dictionary.CompiledClass` / `%Dictionary.CompiledMethod` for names and signatures.
3. Fetch the doc file for the topic from the map above and read it for the concept and the calling pattern.
4. Where the doc and the installed class disagree, the installed classes win. Write the code against the installed signature, and tell the user which doc line disagrees with which build.
5. Compile with `iris_doc` (`mode: "put"`, `compile: true`) and run it before calling it done.

## No web gateway

Some AI builds ship without a web server (no private web server, and on Enterprise AI builds no gateway either). Then iad runs `docker_only = true` against the container, and only `iris_execute` and `iris_compile` work: `iris_doc`, `iris_symbols` and `docs_introspect` need Atelier REST and return `NOPWS_ATELIER_REQUIRED`.

Read class metadata through `iris_execute` instead, with SQL over `%Dictionary.CompiledClass` and `%Dictionary.CompiledMethod`:

```objectscript
Set rs = ##class(%SQL.Statement).%ExecDirect(, "SELECT Name, FormalSpec FROM %Dictionary.CompiledMethod WHERE parent = ?", "%AI.Agent")
While rs.%Next() { Write rs.Name, "(", rs.FormalSpec, ")", ! }
```

Do not name `%Dictionary.ClassDefinition` or `%Dictionary.MethodDefinition` in `iris_execute` code: iad blocks every `%Dictionary.*Definition` reference there, reads included (`CODE_EDIT_BLOCKED`).

## What holds on 2026.3

Each fact below has a live test, first run on build 139 and last run on Build 154 (`crates/iris-agentic-dev-core/tests/integration/test_aihub_139_live.rs`); the claim table in `specs/132-aihub-139/research.md` names it. On another build, check the class first (workflow step 2).

### ConfigStore and Wallet

- `%ConfigStore.Configuration` class methods: `Create(area, type, subtype, name, details)` (six optional args follow), `Get(fqn, .config)`, `GetDetails(fqn, .details, checkValid, resolveSecrets)`, `Delete(fqn)`. All return `%Status`. `fqn` is the dotted name, and an empty subtype drops out: `("AI","LLM","","openai")` is `AI.LLM.openai` (from Gabriel Ing's hackathon skills).
- An LLM entry holds `model_provider`, `model` and `api_key` (from Gabriel Ing's hackathon skills). Create refuses an `openai` entry with no `api_key`.
- On a fresh instance every `AI.LLM` Create fails with ERROR #26414 "No descriptor found for AI.LLM": the descriptor registry is empty. Run `Do ##class(%ConfigStore.DescriptorManager).RebuildRegistry()` once, then Create works. Seen on fresh 139 and 154 containers.
- On 154, Create's `readResource` and `editResource` default to `$$$AdminConfigStoreResourceName`. On 139 they defaulted to empty.
- Keep the key in the wallet: `%Wallet.Collection` `Create(name, {"UseResource":..,"EditResource":..})`, then `%Wallet.KeyValue` `Create("Coll.Key", {"Usage":"CUSTOM","Secret":{"api_key":..}})`, and put `"api_key": "secret://Coll.Key#api_key"` in the entry. `GetDetails(fqn, .d, 1, 1)` resolves it (from Gabriel Ing's hackathon skills).
- Wallet and ConfigStore writes work from USER. Only `Security.Resources` needs `%SYS`.

### Provider and agent

- `%AI.Provider` `Create(name, settings)` is a class method; `"openai"` is a valid name. `%AI.Agent` `%New(provider)` sets `Provider` (from Gabriel Ing's hackathon skills).
- An `%AI.Agent` subclass sets `Parameter PROVIDERCONFIG = "@{config:Name}"` (or `"@{config:AI.LLM.Name}"`) and `Parameter TOOLSETS = "<ToolSet class>"`. Nothing is built until `%Init()`: after `%New()` alone `Provider` is empty. `%Init()` builds the provider, loads the toolsets and then calls your `%OnInit()` (from Gabriel Ing's hackathon skills).
- In `%OnInit()`, parenthesise each comparison: `If (..Provider = "") && (..#X '= "")`. ObjectScript runs left to right, so without the parentheses the test is always true.
- `%AI.Agent` properties: `Provider`, `Model`, `SystemPrompt`, `ToolManager` (an `%AI.ToolMgr`), `ParentAgent` (from Gabriel Ing's hackathon skills).
- `CreateSession(config="")` returns `%AI.Agent.Session`. `Chat(session, input)`, `StreamChat(session, input, callbackObj)`, `ChatWithContent(session, content As %DynamicArray)` and `Run(session, goal)` return `%AI.LLM.Response` (`Content`, `ToolCalls`, `Usage`). `session` is required. On 154 the `StreamChat` callback must extend `%AI.Shell.StreamRenderer`: `OnChunk()` fires per delta and `OnMessage()` once per turn. 139 also took a `callbackMethod` name; 154 does not. `session.GetStats()` returns a `%DynamicObject` (from Gabriel Ing's hackathon skills).
- Sub-agents: `%AI.Agent.SubAgent` `Create(parentAgent, systemPrompt, config)` returns an `%AI.Agent`, so it has `Chat`, `Run` and `CreateSession`. Skills are `%AI.Agent.Skill`: `Parameter TOOLS`, `ExportSkill(target)` returns a path string, `GetSkillFromURI(uri, subpath, cacheDir, authProvider)` is a class method (from Gabriel Ing's hackathon skills).

### Tools and toolsets

- An `%AI.Tool` subclass: each class method is a tool. Its doc comment is the description and its signature gives the parameters. There is no `DESCRIPTION` parameter.
- `%AI.ToolMgr` `AddTool(obj)` takes an instance. `ExecuteTool(name, args)` returns `{"timing":..,"value":..}`. `%AI.ToolMgr` `%Discover()` returns a bare array of tool specs; a tool's own `%Discover()` returns `{"tools":[...]}` (from Gabriel Ing's hackathon skills).
- `%AI.ToolSet` reads `XData Definition [ MimeType = application/xml ]` with root `<ToolSet Name="..">` and children `<Description>`, `<Policies>`, `<Include Class=".."/>`, `<Exclude Tool=".."/>` and `<Query>` (from Gabriel Ing's hackathon skills).
- `<Query Name=".." Arguments="schema As %String, minLen As %Integer = 0" MaxRows="..">` takes SQL with `:name` parameters. `%Integer` becomes JSON `integer`, `%Boolean` `boolean`, `%String` `string`; an argument without a default is required. The row cap is `MaxRows`, else `Parameter QUERYMAXROWS` (100). A result is `{columns, rows, row_count, truncated, elapsed_ms}` (from Gabriel Ing's hackathon skills).
- These do not compile: a `?` placeholder (ERROR #6049), a `:name` not in `Arguments` (#5431), an argument the SQL never uses (#5822) (from Gabriel Ing's hackathon skills).

### Policies

- `%AI.Policy.Authorization` `%CanExecute(toolref, call, metadata) As %Status`. On 154 `toolref` is the plain string the tool was registered under (`Reverse`); 139 passed the tool spec as JSON there. Read the tool name from `call.name` on either build. Deny with `$$$ERROR($$$AICoreToolAccessDenied, ..)` after `Include %AI` (from Gabriel Ing's hackathon skills).
- `%AI.Policy.Audit` `%LogExecution(call As %DynamicObject, metadata As %DynamicObject, result As %DynamicObject, duration As %Integer, status As %Status) As %Status`; `call.name` is the tool name. `%AI.Policy.ConsoleAudit` and `%AI.Policy.Discovery` exist (from Gabriel Ing's hackathon skills).
- Global: `ToolManager.SetAuthPolicy(obj)`, `SetAuditPolicy(obj)`, `SetDiscoveryPolicy(obj)` (from Gabriel Ing's hackathon skills).
- ToolSet-local: `<Policies><Authorization Class=".."><Item>..</Item></Authorization><Audit Class=".."/></Policies>`. The policy class extends `%XML.Adaptor`, sets `Parameter XMLNAME`, and projects list properties with `XMLITEMNAME` and `XMLPROJECTION = "ELEMENT"` (from Gabriel Ing's hackathon skills). A list with one item loads empty on 139 and 154, so give a deny list at least two entries.

### RAG

- `%AI.RAG.Embedding.FastEmbed` `Create()` needs no key: local, 384 dims, model `AllMiniLML6V2` (from Gabriel Ing's hackathon skills).
- `%AI.RAG.VectorStore.IRIS`: set `TableName`, `Dimensions`, `ModelName`, then `Build()`. It makes the table and `<Table>_Config`, and refuses a later build with another model ("Model mismatch") (from Gabriel Ing's hackathon skills).
- `%AI.RAG.KnowledgeBase`: set `Name`, `Description`, `TopK`, then `Build(embedding, vectorStore)`. `AddDocument(text, metadata)` returns `%Status`; `AddDocuments([[text, meta], ...])` and `ReindexDocument(source, text)` return a chunk count. A second `AddDocument` with the same `source` replaces the first; without a `source` the same text is stored twice (from Gabriel Ing's hackathon skills).
- `kb.AddToAgent(agent)` registers a tool named `kb.Name` with parameters `query` (string, required) and `top_k` (integer). It returns hits with `id`, `text`, `score` and `metadata` (`source`, `chunk_index`) (from Gabriel Ing's hackathon skills).

### MCP server

- A service is a subclass of `%AI.MCP.Service` with `Parameter SPECIFICATION = "<ToolSet class>"` (from Gabriel Ing's hackathon skills).
- Its web application uses the subclass as dispatch class and needs `Type` 18 (CSP + MCP). `Security.Applications.Create` refuses 16 ("CSP bit"); a Type 2 app is accepted but the bridge gets no tools from it and logs "failed identity checking".
- The bridge, `iris-mcp-server` in `<install>/bin`, reaches IRIS on the superserver port (1972), not HTTP. Config: `[mcp] transport = "stdio"`, and under `[[iris]]` a `server = {host, port, username, password}` plus `endpoints = [{path, username, password}]` (from Gabriel Ing's hackathon skills).
- Bridged tool names are `mcp_<path segments>_<tool>`: `/mcp/myapp` serves `mcp_myapp_GetCustomer`. The description starts `[Remote Service: <iris name>_mcp_myapp]`. A tool result arrives as JSON text (`"\"cba\""`). The bridge's fallback status tool is gone once an endpoint serves tools (from Gabriel Ing's hackathon skills).
- A ToolSet audit policy that subclasses `%AI.Policy.ConsoleAudit` still serves over the bridge.

## Corrections

Where the ai-hub-eap docs and 2026.3 disagree, trust these lines. Each one is a live test, last run on Build 154.

- `ObjectScript_SDK_Guide.md:250` says `Parameter PROVIDERCONFIG = "MyConfigName"`; on 2026.3 it is `"@{config:MyConfigName}"` (or `"@{config:AI.LLM.MyConfigName}"`). A bare name, or `@{config.MyConfigName}` with a dot, fails `%Init()` with `PROVIDERCONFIG is invalid`.
- Upstream's own skill, `SKILL.md:148`, says `If ..Provider = "" && ..#MODELCONFIGNAME '= ""`; on 2026.3 it is always true, because ObjectScript reads left to right, so it replaces a provider passed to `%New()`. Write `If (..Provider = "") && (..#MODELCONFIGNAME '= "")`.
- `ObjectScript_SDK_Advanced.md:364` says repeated child elements fill a policy's list property; on 2026.3 it is true for two or more, but a single child loads an empty list. A deny policy with one `<Blocked>` tool blocks nothing. Add a second item or set the list in code.

## Upstream's own skill

ai-hub-eap carries its own agent skill. Read it raw: `https://raw.githubusercontent.com/intersystems-community/ai-hub-eap/master/skills/aihub-eap/SKILL.md`. It was written for an older build; check what it says against the installed classes as in the workflow above.

`skill_community` cannot install it: iad subscribes to a repo through an `iris-agentic-dev.toml` at the repo root, and ai-hub-eap has none.
