---
name: aihub-eap
description: >
  InterSystems AI Hub EAP (Early Access Program) — accurate API patterns for
  builds 158/159/161/162 (current). Covers %AI.Agent declarative Parameters, %AI.Provider.Create,
  ConfigStore/GetProviderForConfig, @{env/config/wallet} substitution, session
  management, streaming, tool sets, and known breaking changes from build 141.
  Load when helping EAP participants set up, build, or debug AI Hub projects.
tags: [iris, aihub, eap, ai, configstore, langchain, mcp, docker]
---

# InterSystems AI Hub — EAP Reference (Build 162)

> **Current build: 2026.2.0AI.162** (community image available; enterprise pending)
> `LLMConfig` property is GONE. The pattern is now Parameters + `%Init()` + ConfigStore.
> Build 161 has MCP server breaking changes — see section 13.
> Build 162 adds 13 new `%AI` classes vs 161 — see section 14.

---

## 1. Getting the Build

**EAP Portal**: https://evaluation.intersystems.com/Eval/early-access/AIHub

**Current build: 2026.2.0AI.162.0** (as of 2026-04-26)

Available downloads:
```
iris-community-2026.2.0AI.158.0-docker.tar.gz          # x86_64 Docker (community, no key needed)
iris_arm64-community-2026.2.0AI.158.0-docker.tar.gz    # ARM64 Docker (community)
iris-container-x64.key                                   # License key (removes community limits)
iris-container-arm64.key                                  # ARM64 license key
iris.key                                                  # General license key
langchain_intersystems-0.0.1-py3-none-any.whl           # Python SDK
IRIS_Community-2026.2.0AI.158.0-macx64.tar.gz           # macOS Intel
IRIS_Community-2026.2.0AI.158.0-win_x64.exe             # Windows
```

**Community builds** (`iris-community-*`) run without a license key but have connection/data limits. Use a key to remove limits — request from the EAP portal or from your ISC contact.

---

## 2. Docker Setup

```bash
docker image load -i iris-2026.2.0AI.159-docker.tar.gz

docker run --name iris-ai-hub \
  -p 1972:1972 -p 52773:52773 \
  -d \
  --volume /path/to/key-dir:/external/keys \
  intersystems/iris:2026.2.0AI.159 \
  -k /external/keys/iris-container-x64.key
```

### Fix expired default password
```bash
docker exec -it iris-ai-hub iris session iris -U %SYS
# In session:
#  write ##class(Security.Users).UnExpireUserPasswords("*")
```

### iris-devtester integration
```python
from iris_devtester import IRISContainer
container = IRISContainer("intersystems/iris:2026.2.0AI.159") \
    .with_name("iris-ai-hub") \
    .with_bind_ports(1972, 52773) \
    .with_license_key("/path/to/iris-container-x64.key") \
    .start()
```

---

## 3. %AI.Agent — Build 159 Patterns

### Pattern A: Declarative subclass (recommended)

```objectscript
Class MyApp.AI.MyAgent Extends %AI.Agent
{
    Parameter PROVIDER = "openai";           // provider name
    Parameter MODEL = "gpt-4o";             // model id
    Parameter APIKEY;                        // empty = read from env OPENAI_API_KEY
    Parameter TOOLSETS = "%AI.Tools.SQL";   // comma-separated toolsets

    XData INSTRUCTIONS [ MimeType = text/markdown ]
    {
    You are a helpful IRIS database assistant.
    Use SQL tools to answer questions about the data.
    }

    Method %OnInit() As %Status
    {
        // Optional: additional setup after %Init() completes
        // Set ..Temperature = 0.7
        Return $$$OK
    }
}
```

**Usage:**
```objectscript
Set agent = ##class(MyApp.AI.MyAgent).%New()
$$$ThrowOnError(agent.%Init())           // REQUIRED — initializes provider, tools, prompt

Set session = agent.CreateSession()
Set response = agent.Chat(session, "How many patients are in the database?")
Write response.Content
```

> **DO NOT** call `Chat()` without `%Init()` first — tools and provider won't be wired.

### Pattern B: Programmatic (for dynamic provider selection)

```objectscript
Set settings = {}
Do settings.%Set("api_key", apiKey)
Set provider = ##class(%AI.Provider).Create("openai", settings)

Set agent = ##class(%AI.Agent).%New(provider)
Set agent.Model = "gpt-4o-mini"
Set agent.SystemPrompt = "You are a helpful assistant."

Set session = agent.CreateSession()
Set response = agent.Chat(session, "Hello!")
Write response.Content
```

### Pattern C: ConfigStore integration (production pattern)

```objectscript
Class MyApp.AI.ProdAgent Extends %AI.Agent
{
    Parameter MODELCONFIGNAME = "opsreview";   // name in ConfigStore

    Method %OnInit() As %Status
    {
        Set sc = $$$OK
        Try {
            If ..Provider = "" && ..#MODELCONFIGNAME '= "" {
                Set sc = ..GetProviderForConfig(..#MODELCONFIGNAME, .provider, .model)
                Quit:$$$ISERR(sc)
                Set ..Provider = provider
                Set ..Model = model
            }
        } Catch ex {
            Set sc = ex.AsStatus()
        }
        Return sc
    }

    ClassMethod GetProviderForConfig(
        configName As %String,
        Output provider As %AI.Provider,
        Output model As %String) As %Status
    {
        Set sc = $$$OK
        Try {
            Set sc = ##class(%ConfigStore.Configuration).GetDetails(
                "AI.LLM."_configName, .details, 0, 1)
            Quit:$$$ISERR(sc)
            Set provider = ##class(%AI.Provider).Create(details."model_provider", details)
            Set model = details."model"
        } Catch ex {
            Set sc = ex.AsStatus()
        }
        Quit sc
    }
}
```

**ConfigStore entry for "opsreview":**
```objectscript
Set config = {
    "model_provider": "anthropic",
    "model": "claude-sonnet-4-5@20250929",
    "api_key": "secret://MySecrets.anthropic#apikey"
}
Do ##class(%ConfigStore.Configuration).Create("AI","LLM","","opsreview", config)
```

---

## 4. @{} Variable Substitution

Used inside `PROVIDERCONFIG` parameters and ToolSet XML — NOT in ObjectScript code directly.

| Syntax | Source | Example |
|--------|--------|---------|
| `@{env.VAR}` | OS environment variable | `@{env.OPENAI_API_KEY}` |
| `@{config.Key}` | `^%AI.Config` global | `@{config.VertexSAPath}` |
| `@{wallet.Col.Key}` | IRIS Secure Wallet | `@{wallet.AISecrets.anthropic}` |

```objectscript
// In a declarative agent Parameter:
Parameter PROVIDERCONFIG = "{
    ""project_id"": ""my-gcp-project"",
    ""region"": ""us-east5"",
    ""service_account_path"": ""@{env.VERTEX_SA_PATH}""
}";

// In ToolSet XData (MCP remote server with token from wallet):
<MCP Name="MyServer">
    <Remote URL="https://mcp.example.com/mcp"
            AuthType="bearer"
            Token="@{wallet.MCPSecrets.token}"/>
</MCP>
```

> `@{config.AI.LLM.opsreview.APIKey}` is NOT a valid pattern.
> Use `GetDetails()` + `%AI.Provider.Create()` for ConfigStore-backed API keys in code.

---

## 5. Session Management

```objectscript
// Create session (inherits agent's provider, model, prompt, tools)
Set session = agent.CreateSession()

// With config overrides
Set cfg = {"max_iterations": 10, "temperature": 0.7, "max_tokens": 1000}
Set session = agent.CreateSession(cfg)

// Blocking chat
Set response = agent.Chat(session, "Your question here")
Write response.Content

// Streaming
Set renderer = ##class(%AI.System.StreamRenderer).%New()
Set response = agent.StreamChat(session, "Your question", renderer, "OnChunk")
Do renderer.Flush()

// Multi-modal
Set content = [
    {"type": "text", "text": "What is in this image?"},
    {"type": "image_url", "image_url": {"url": "https://example.com/img.jpg"}}
]
Set response = agent.ChatWithContent(session, content)

// Session stats
Set stats = session.GetStats()
Write stats."total_interactions", " turns, ", stats."total_tool_calls", " tool calls"

// Context inspection
Set messages = session.GetContext()
Set iter = messages.%GetIterator()
While iter.%GetNext(.i, .msg) { Write msg.role, ": ", $EXTRACT(msg.content,1,80), ! }

// Session control
Do session.Reset()           // full reset
Do session.ResetContext()    // messages only
Do session.ResetStats()      // stats only
```

---

## 6. %AI.Provider — Supported Providers

```objectscript
// OpenAI
Set provider = ##class(%AI.Provider).Create("openai",
    {"api_key": key, "organization": orgId})

// Anthropic
Set provider = ##class(%AI.Provider).Create("anthropic", {"api_key": key})

// AWS Bedrock — bearer token (ISC SSO)
Set provider = ##class(%AI.Provider).Create("bedrock",
    {"region": "us-east-1", "bearer_token": token})
// MUST use cross-region inference IDs: "us.anthropic.claude-sonnet-4-6"
// NOT raw model IDs: "anthropic.claude-sonnet-4-6"

// AWS Bedrock — SigV4 (AWS keys)
Set provider = ##class(%AI.Provider).Create("bedrock",
    {"region": "us-east-1"})   // reads AWS_ACCESS_KEY_ID etc from env

// Google Vertex AI
Set provider = ##class(%AI.Provider).Create("vertex",
    {"project_id": pid, "region": "us-east5",
     "service_account_path": saPath})

// Google Gemini
Set provider = ##class(%AI.Provider).Create("gemini", {"api_key": key})

// xAI Grok
Set provider = ##class(%AI.Provider).Create("grok", {"api_key": key})

// NVIDIA NIM
Set provider = ##class(%AI.Provider).Create("nim", {"base_url": url})
```

---

## 7. ConfigStore API

```objectscript
// Store
Set config = {"model_provider":"anthropic","model":"claude-sonnet-4-5@20250929",
              "api_key":"secret://Secrets.ant#apikey"}
Do ##class(%ConfigStore.Configuration).Create("AI","LLM","","myconfig", config)

// Retrieve (raw)
Set cfg = ##class(%ConfigStore.Configuration).Get("AI","LLM","","myconfig")

// Retrieve with secret resolution (resolveSecrets=1)
Set sc = ##class(%ConfigStore.Configuration).GetDetails(
    "AI.LLM.myconfig", .details, 0, 1)
// details."model_provider", details."model", details."api_key" all resolved

// Delete
Do ##class(%ConfigStore.Configuration).Delete("AI.LLM.myconfig")
```

---

## 8. Wallet (Secrets)

```objectscript
// Create security resource first (%SYS namespace)
Set $NAMESPACE = "%SYS"
Do ##class(Security.Resources).Create("AI.Secrets")

// Create collection
Do ##class(%Wallet.Collection).Create("AISecrets",
    {"UseResource": "AI.Secrets", "EditResource": "AI.Secrets"})

// Store secret
Do ##class(%Wallet.KeyValue).Create("AISecrets.anthropic",
    {"Usage": "CUSTOM", "Secret": {"apikey": "sk-ant-...actual-key..."}})

// Reference in ConfigStore: "api_key": "secret://AISecrets.anthropic#apikey"
```

---

## 9. Python (langchain-intersystems)

```bash
python -m venv .venv && source .venv/bin/activate
pip install ./langchain_intersystems-0.0.1-py3-none-any.whl
pip install mcp langchain-openai langchain-ollama
```

```python
from langchain_intersystems.chat_models import init_chat_model
from langchain_intersystems import init_mcp_client

llm = init_chat_model("AI.LLM.myconfig")    # uses ConfigStore entry
mcp = init_mcp_client("AI.MCP.my-server")
```

**Common issues:**
- Version mismatch: check `pip show langchain-intersystems` matches the whl filename
- Platform errors: add `--force-reinstall` if wheel metadata conflicts

---

## 10. Breaking Changes: Build 141 → 159

| Feature | Build 141 | Build 159 |
|---------|-----------|-----------|
| `LLMConfig` property | Existed | **REMOVED** |
| Agent instantiation | `##class(%AI.Agent).%New(provider)` only | Also: declarative Parameters subclass |
| Config Store | Partial WIP | Full `%ConfigStore.Configuration` support |
| `%Init()` requirement | Optional | **REQUIRED** before first `Chat()` |
| Streaming | Not available | `StreamChat()` + `%AI.System.StreamRenderer` |
| Session reset | Not available | `Reset()`, `ResetContext()`, `ResetStats()` |
| Checkpoints | Not available | `AddCheckpoint()`, `RewindTo()` |
| Session fork | Not available | `Fork()`, `ForkAndSummarize()` |
| Nested agents | Not available | `CreateSubAgent()`, `DelegateTask` tool |
| Skills | Not available | `%AI.Agent.Skill` with XData |
| RAG | Not available | `%AI.KnowledgeBase`, `EnableSmartDiscovery()` |
| Prompt caching | Not available | `"cache": {"enabled": 1}` in session config |
| `@{wallet.*}` substitution | Not available | `@{wallet.Collection.Key}` |

**DEAD CODE from build 141 (do not use):**
```objectscript
// ❌ LLMConfig is gone
Set agent.LLMConfig = "AI.LLM.openai"

// ❌ Must call %Init() first
Set agent = ##class(MyAgent).%New()
Set response = agent.Chat(session, "hi")   // will fail — no provider wired
```

---

## 11. Quick Smoke Test

```objectscript
// Verify %AI.* classes are available
Write ##class(%AI.Provider).%IsA("%RegisteredObject"), !   // → 1

// Minimal working agent (env var for key)
Set env("OPENAI_API_KEY") = "sk-..."      // or set in OS env before starting IRIS
Set provider = ##class(%AI.Provider).Create("openai", {"api_key": $SYSTEM.Util.GetEnviron("OPENAI_API_KEY")})
Set agent = ##class(%AI.Agent).%New(provider)
Set agent.Model = "gpt-4o-mini"
Set session = agent.CreateSession()
Set r = agent.Chat(session, "Say hi")
Write r.Content
```

---

## 13. Build 161 MCP Server Breaking Changes (2026-04-26)

> **Proven working** in `~/mindwalk-test2/` on EC2 with 18/18 tools discovered and `%Invoke` executing correctly.

### `%AI.MCP.Service.Utils` is GONE in build 161

`LoadToolSetsToManager` was available in builds 147–159. It does not exist in 161.

```objectscript
// ❌ Build 161 — class does not exist:
do ##class(%AI.MCP.Service.Utils).LoadToolSetsToManager("My.ToolSet","/mcp/myapp")
// → <CLASS DOES NOT EXIST> *%AI.MCP.Service.Utils
```

**This is fine** — iris-mcp-server in build 161 discovers tools directly from the compiled class
via wgproto without needing the cache pre-populated. **Do not call it, do not add workarounds.**

### Compile order is critical in build 161

`%AI.ToolSet.Specification.Compiler` validates all referenced classes **at compile time**.
If `ToolSet` compiles before its `<Include Class="...">` targets are loaded, it discovers 0 tools.

```objectscript
// ❌ WRONG — compiles ToolSet too early (flag "ck" = load + compile):
do $system.OBJ.LoadDir("/src/Mindwalk","ck",,1)

// ✅ CORRECT — three-step pattern:
do $system.OBJ.LoadDir("/src/Mindwalk","k",,0)          // 1. load all, no compile
do $system.OBJ.CompilePackage("Mindwalk","ck")           // 2. compile everything
do $system.OBJ.Compile("Mindwalk.ToolSet","ck")          // 3. recompile ToolSet last
```

The batch compile (`CompilePackage`) may still emit `<CLASS DOES NOT EXIST> *Mindwalk.GraphToolsPy`
during the ToolSet sub-compile — this is benign as long as the final explicit `Compile("Mindwalk.ToolSet","ck")`
succeeds. Check that the last compile outputs `Compilation finished successfully`.

### `iris session IRIS -U USER "..."` hangs in build 161

Inline single-command `iris session` calls hang indefinitely in build 161.

```bash
# ❌ Hangs — do not use for readiness checks:
iris session IRIS -U USER "write 1,! halt" >/dev/null 2>&1 && break

# ✅ TCP probe — works reliably:
bash -c 'cat < /dev/null > /dev/tcp/localhost/1972' 2>/dev/null && break

# ✅ Script file via irissession — works for class loading:
cat > /tmp/init.script << 'EOF'
zn "MINDWALK"
do $system.OBJ.LoadDir("/src/Mindwalk","k",,0)
halt
EOF
/usr/irissys/bin/irissession IRIS < /tmp/init.script 2>&1
```

### iris-mcp-server binary is architecture-specific

The binary at `services/iris-mcp-sidecar/iris-mcp-server` in some repos is **ARM64**.
It will silently fail on x86_64 EC2. Always use the binary inside the IRIS image:

```dockerfile
# ✅ Sidecar pattern — gets the right binary for the right arch:
FROM ${IRIS_IMAGE}    # same image as the IRIS container
ENTRYPOINT ["/usr/irissys/bin/iris-mcp-server"]
CMD ["--config", "/etc/iris-mcp/config.toml", "run"]
```

### `AutheEnabled=96` — must patch if iris.script ran at image build time

If your `iris.script` baked `AutheEnabled=64` into the image, patch it at container start:

```objectscript
zn "%SYS"
set appProps("AutheEnabled") = 96
do ##class(Security.Applications).Modify("/mcp/myapp", .appProps)
```

The iris-mcp-server `GET /mcp/myapp/v1/services` request arrives **unauthenticated**.
`AutheEnabled=64` (Password only) returns HTTP 500. `96` (Password + Unauthenticated) allows it through.

---

## 12. Verified-on-Build-159 Findings (2026-04-25)

### `@{}` substitution — `wallet` prefix NOT registered on 159

Only `env` and `config` prefixes work. `wallet` causes an error at runtime.

```objectscript
// ✅ Works on 159:
Parameter APIKEY = "@{env.OPENAI_API_KEY}";

// ❌ Fails on 159 — wallet not registered:
Parameter APIKEY = "@{wallet.AISecrets.openai}";
```

Use env vars or store keys directly in `PROVIDERCONFIG` referencing `@{env.*}`.

### `%AI.LLM.Response` — use `.Content`, never `.%Get("content")`

`response.Content` is a typed `%String` property. `response.%Get()` throws `<METHOD DOES NOT EXIST>`.

```objectscript
// ✅ Correct:
Write response.Content

// ❌ Throws <METHOD DOES NOT EXIST>:
Write response.%Get("content")
```

Available properties on the response object:
- `.Content` — `%String` — the assistant's text reply
- `.ToolCalls` — `%DynamicArray` — tool call requests from the model
- `.Usage` — `%DynamicObject` — token usage (`prompt_tokens`, `completion_tokens`)

### `LLMConfig` property is gone on 159 (confirmed)

The `LLMConfig` property documented for build 141 does NOT exist on 159.
Using it causes `<PROPERTY DOES NOT EXIST>`. Use declarative Parameters or programmatic `%AI.Provider.Create()`.

### `irishealth` vs `iris` image — use `irishealth` for MCP over HTTP

| Image | WebServer | CSPServer binary | Use for |
|-------|-----------|-----------------|---------|
| `irishealth-community-*` | 1 (enabled) | Yes | MCP over HTTP, REST, web apps |
| `iris-community-*` | 0 (disabled) | No | ObjectScript/CLI only, no HTTP tools |

```bash
# ✅ For MCP over HTTP:
docker image load -i irishealth-community-2026.2.0AI.158.0-docker.tar.gz
docker run --name iris-ai-hub -p 1972:1972 -p 52773:52773 -d \
  irishealth/iris-community:2026.2.0AI.158.0

# ⚠️ Plain iris image has no web gateway — iris_execute HTTP path won't work
```

### Empty `Parameter APIKEY` auto-reads from OS environment

When `APIKEY` parameter is empty (the default), `%Init()` automatically resolves
`@{env.OPENAI_API_KEY}` (or the provider-appropriate env var).

```objectscript
Class MyAgent Extends %AI.Agent {
    Parameter PROVIDER = "openai";
    Parameter APIKEY;   // empty = reads OPENAI_API_KEY from OS env automatically
}

// Set in shell before starting IRIS:
// export OPENAI_API_KEY=sk-...

Set agent = ##class(MyAgent).%New()
$$$ThrowOnError(agent.%Init())   // picks up OPENAI_API_KEY from env
```

This is the recommended pattern for local dev. For production, use ConfigStore + Wallet.

---

## 14. Build 162 New Classes (community 2026-04-26; enterprise pending)

Build 162 community has **51 `%AI` classes** vs **38 in enterprise 161**. The 13 additions:

### RAG stack (new in 162)
```
%AI.RAG.Embedding
%AI.RAG.Embedding.FastEmbed
%AI.RAG.Embedding.OpenAI
%AI.RAG.KnowledgeBase
%AI.RAG.VectorStore.IRIS
```

### MCP client in ToolSet XData (consume external MCP servers, new in 162)
```
%AI.ToolSet.Specification.MCP
%AI.ToolSet.Specification.MCP.Remote
%AI.ToolSet.Specification.MCP.Stdio
%AI.ToolSet.Specification.Utils
```

### Built-in tool providers (new in 162)
```
%AI.Tools.FileSystem
%AI.Tools.ShellTools
%AI.Tools.SQL
```

### Agent composition (new in 162)
```
%AI.Agent.Skill
%AI.Agent.SubAgent
%AI.Tool
%AI.Tool.Resolver
%AI.Tool.Schema
```

### ConfigStore/Wallet API (new in 162, NOT in enterprise 161)
```
%AI.Utils.ConfigStore
%AI.Utils.SettingStore
%AI.Utils.WalletStore
```

**Important**: The `aihub-eap` skill documents `%AI.Utils.ConfigStore` patterns — these
only work on **community 162+** or when enterprise catches up. On enterprise 161, calling
any `%AI.Utils.*` method throws `<CLASS DOES NOT EXIST>`.

### MCP client in ToolSet (162 only) — consume external MCP servers

Build 162 adds `<MCP>` elements to the ToolSet XData, letting IRIS act as an MCP **client**
and expose remote MCP server tools as local tools:

```xml
<ToolSet Name="MyToolSet">
    <Include Class="MyApp.LocalTools"/>
    <MCP>
        <Remote Name="external" URL="https://example.com/mcp"/>
        <Stdio Name="local-tool" Command="/usr/bin/my-mcp-tool"/>
    </MCP>
</ToolSet>
```

### docker pull for 162 community

The tags list API has a pagination bug — `162.0` doesn't appear but pulls fine directly:

```bash
docker pull docker.iscinternal.com/docker-intersystems/intersystems/irishealth-community:2026.2.0AI.162.0
```

---

## 13. Linux Docker Volume Permissions (from READY 2026 hackathon — Anthony Master)

**Symptom**: IRIS container exits immediately on Linux with:
```
terminate called after throwing an instance of 'std::runtime_error'
what(): Unable to find/open file iris-main.log in current directory /home/irisowner/dev
```

**Root cause**: ALL IRIS container editions (community, enterprise, irishealth, ai_hub) run as UID 51773 (`irisowner`). When you bind-mount a host directory owned by UID 1000 (typical Linux user), the container can read the volume but cannot write to it — and IRIS needs to write `iris-main.log` at startup.

**Not affected**: macOS (VirtioFS translates permissions transparently).

### Fix options

**Option 1 — POSIX ACLs (recommended, minimal footprint)**
```bash
setfacl -R -m u:51773:rwX <repo-dir>
setfacl -R -d -m u:51773:rwX <repo-dir>
```
The `-d` flag makes new files/dirs inherit the rule automatically.
Verify with: `getfacl <repo-dir>`

**Option 2 — tmpfs (no persistence)**
```yaml
# docker-compose.yml
volumes:
  - type: tmpfs
    target: /home/irisowner/dev
```

**Option 3 — chown on host (broad)**
```bash
sudo chown -R 51773:51773 <repo-dir>
```

**Option 4 — Docker named volume (avoid bind-mount entirely)**
```yaml
volumes:
  iris-data:
services:
  iris:
    volumes:
      - iris-data:/home/irisowner/dev
```

**Team note**: If you re-clone or a new team member sets up the repo on Linux, they must re-run the `setfacl` commands. Add this to your project README or Makefile setup target.
