# When the Frog Doesn't See the Whole Pond: AI Coding Assistants and Server-Side IRIS

*This article is a follow-up to Thomas Mazur's excellent
["Frogs, Chickens, AI, and VS Code"](https://community.intersystems.com/post/frogs-chickens-ai-and-vs-code)
and the comment thread that followed it.*

---

Thomas Mazur's article sparked a great discussion about using GitHub Copilot with VS Code
for IRIS development. Buried in the comments, though, is a limitation that anyone working
server-side (`isfs://` workspaces) will recognize immediately. Pietro Di Leo and Mike.W
both noticed the same thing: Copilot in an ISFS workspace can only see the files that are
**open in your editor**. It cannot index the virtual filesystem. On a legacy Caché/IRIS
namespace with thousands of classes and routines, that's the difference between having an
AI that actually understands your codebase and one that's essentially working blindfolded.

John Murray pointed people at a tool I've been building —
[iris-agentic-dev](https://github.com/intersystems-community/iris-agentic-dev) — and
noted no Developer Community article existed for it yet. Pietro said "an article about it
would be awesome." This is that article.

---

## Why the Buffer-Only Problem Is Architectural, Not a Model Problem

When you open an `isfs://` workspace in VS Code, your classes live on the IRIS server,
not on disk. Copilot's indexing — and Cursor's, and Claude Code's — works by scanning the
local filesystem. A virtual file system backed by the Atelier API exposes the current file
to the editor just fine, but the AI's broader "what's in this codebase?" machinery never
gets a full picture. Only the classes you have open at that moment are in context.

For a greenfield project with a handful of classes, this is tolerable. For a production
IRIS application — ten thousand classes, decades of accumulated `%Library` overrides,
Ensemble productions wired together by message routing tables you haven't touched in
years — the AI becomes nearly useless for the hard questions: *Why is this dispatch
resolving to the wrong class? What calls this method? Which classes extend this abstract
type?*

The answer isn't to open every class. It's to give the AI a **live API connection** to
IRIS instead of relying on the editor buffer.

---

## What iris-agentic-dev Does

`iris-agentic-dev` is an MCP server — a background process that exposes IRIS capabilities
as tools that Copilot, Claude Code, Cursor, and OpenCode can call directly in chat. It
connects to your IRIS instance via the Atelier REST API (the same one VS Code's
ObjectScript extension uses). No source-code checkout required, no virtual filesystem
crawl. The AI asks IRIS directly.

The tool set covers the full development loop:

**Navigation and inspection**
- Full-text search across the entire namespace (`iris_search`) — regex, category filter, no
  "open it first" prerequisite
- Class definition introspection (`docs_introspect`) — properties, methods, parameters,
  superclasses
- Dynamic dispatch resolution (`resolve_dynamic_dispatch`) — turns
  `$classmethod(className, methodName)` into the actual compiled candidates with
  confidence scores
- Symbol search across all classes and routines, or just your local workspace files

**Compile and execute**
- Compile a class, routine, or wildcard and get errors back with line numbers
  (`iris_compile`)
- Run ObjectScript and return the output (`iris_execute`)
- Execute SQL queries (`iris_query`)
- Invoke a ClassMethod directly by name with arguments (`iris_execute_method`)

**Debug**
- Capture Atelier protocol packets, map INT line numbers back to original class lines,
  pull IRIS error logs

**Source control and interop**
- SCM status, checkout, and action execution via `iris_source_control`
- Ensemble production introspection, message routing table extraction, business rule
  inspection

The AI doesn't guess about your codebase. It asks.

---

## Setup: VS Code + GitHub Copilot (Five Minutes)

**Prerequisites**: VS Code, GitHub Copilot, the InterSystems ObjectScript extension (you
already have this if you're reading this article).

**1. Install the binary**

On macOS or Linux:

```bash
curl -fsSL https://raw.githubusercontent.com/intersystems-community/iris-agentic-dev/master/install.sh | bash
```

On Windows (PowerShell):

```powershell
irm https://raw.githubusercontent.com/intersystems-community/iris-agentic-dev/master/install.ps1 | iex
```

**2. Add it to VS Code's MCP config** (`~/.claude.json` or VS Code's MCP settings):

```json
{
  "mcpServers": {
    "iris-agentic-dev": {
      "command": "iris-agentic-dev",
      "args": ["mcp"]
    }
  }
}
```

**3. That's it.** The server reads your existing `objectscript.conn` or
`intersystems.servers` VS Code configuration — the same connection you already use for
the ObjectScript extension. No separate credentials to manage.

Verify it works: open Copilot Chat in Agent mode and ask *"Call check_config and show me
the result."* You should see your IRIS connection details confirmed.

---

## The Conversation That's Now Possible

With `iris-agentic-dev` connected, the kinds of questions that were previously impossible
in a server-side workspace become straightforward:

> *"Search the INTEGRATIONS namespace for all classes that extend
> `Ens.BusinessOperation`. Show me which ones override `OnMessage`."*

> *"I'm getting a `<UNDEFINED>myVar>` error at line 47 of `MyPkg.SomeClass`. Map that
> back to the original source line and show me the context."*

> *"Compile `MyPkg.*.cls` and fix any errors."*

> *"What Ensemble productions are running in PROD? What items are enabled in each?"*

These work regardless of whether you have any files open in your editor. The AI is talking
to IRIS, not to your buffer.

---

## ObjectScript Skills: Teaching the AI Your Language

One more thing worth mentioning. Even with a live IRIS connection, AI models have a
well-known blind spot: they produce plausible-looking ObjectScript that contains subtle
bugs — missing `$$$` macro prefixes, wrong `%Status` propagation patterns, COS-specific
syntax the model learned from sparse training data.

`iris-agentic-dev` ships a set of **skills** — short, focused instruction files that
correct these patterns before the AI writes any code. They work with or without the MCP
server.

The benchmark results are concrete. Against a 22-task ObjectScript repair suite:

| Baseline (no skills) | With `objectscript-review` skill | Lift |
|---------------------|----------------------------------|------|
| 73% pass rate | **100% pass rate** | +27% |

The `objectscript-review` skill is 205 words. It's a checklist of the 10 most common
ObjectScript mistakes. That's it.

In VS Code + Copilot, skills install automatically when you install the extension. For
Claude Code, three curl commands grab the top skills into `~/.claude/skills/`.

---

## Try It

- GitHub: [intersystems-community/iris-agentic-dev](https://github.com/intersystems-community/iris-agentic-dev)
- Quick start for VS Code + Copilot: [README → Quick start](https://github.com/intersystems-community/iris-agentic-dev#quick-start-vs-code--github-copilot)
- Skill benchmarks and how to contribute: [light-skills/BENCHMARKING.md](https://github.com/intersystems-community/iris-agentic-dev/blob/master/light-skills/BENCHMARKING.md)

Thanks to Thomas Mazur for writing the original post that surfaced this problem so
clearly, to John Murray for pointing people this direction in the comments, and to Pietro
Di Leo and Mike.W for articulating exactly what was broken. The comment thread was the
best possible spec for what this article needed to say.

---

*Thomas Dyar is a developer at InterSystems. `iris-agentic-dev` is an open-source project
under the intersystems-community organization.*
