# Give Your AI a Live Connection to IRIS — Part 1: The Problem, the Tool, and Getting Started

*Part 1 of a series. Part 2 covers the full tool catalog. Part 3 covers ObjectScript
skills. Part 4 covers benchmarking and measuring what actually improves.*

*This series grew out of a comment thread on Thomas Mazur's
["Frogs, Chickens, AI, and VS Code"](https://community.intersystems.com/post/frogs-chickens-ai-and-vs-code).
If you haven't read it, start there — it's excellent.*

---

## A Problem Hidden in the Comments

Thomas Mazur's post about VS Code productivity sparked a great discussion. Peacock for
color-coded environments, workspace files scoped to subdirectories, GitHub Copilot in
Agent mode — practical, immediately useful stuff.

But buried in the comments, Pietro Di Leo and Mike.W hit something real: when you work
server-side in VS Code — the `isfs://` workspace that most production IRIS shops use —
Copilot can only see the files **open in your editor**. It cannot index the virtual
filesystem. On a mature IRIS application with thousands of classes, that means the AI
is essentially working blindfolded. It can see whatever you've got open in tabs. Nothing
else.

John Murray pointed people at a project I've been building —
[iris-agentic-dev](https://github.com/intersystems-community/iris-agentic-dev) — and
noted that no Developer Community article existed for it. Pietro said "an article about
it would be awesome."

This is that article. Four parts. Starting with why the problem exists, what the tool
does, and how to get running in about five minutes.

---

## Why the AI Can't See Your Namespace

It's worth understanding this clearly, because the fix only makes sense once you see
the root cause.

When you open an `isfs://` workspace, your IRIS classes live on the server, not on
disk. The VS Code ObjectScript extension streams them to you on demand via the Atelier
API — open a class, it fetches it; save it, it writes back. This works beautifully for
editing.

AI assistants like Copilot work differently. They want to build a picture of your whole
codebase — not just the file you're editing, but the surrounding context. How is this
method called? What inherits from this class? What other code touches this global? To
answer those questions, they need to scan the filesystem. On a normal local project that
works fine. On a virtual filesystem that only materializes files on demand, it doesn't.
The AI gets what's open. That's it.

For a greenfield project with a handful of classes this is tolerable. For a production
IRIS system — ten thousand classes, Ensemble productions, custom `%Library` subclasses,
business logic accumulated across years of development — the AI becomes nearly useless
for the hard questions. It can help you write a new method if you paste in the
surrounding context yourself. It cannot help you understand the system.

The fix isn't to open more files. It's to give the AI a different kind of connection —
one that can ask IRIS directly instead of crawling the disk.

---

## What iris-agentic-dev Is

`iris-agentic-dev` is an **MCP server** — a background process that gives AI assistants
a set of tools they can call to interact with a live IRIS instance. It works with GitHub
Copilot (via the VS Code extension), Claude Code, Cursor, and OpenCode.

If you haven't encountered MCP before: it stands for Model Context Protocol, an open
standard for connecting AI assistants to external tools and data sources. You configure
it once, and the AI assistant can use the registered tools directly from chat. It's
already built into recent versions of VS Code for Copilot, and into Claude Code and
OpenCode natively.

`iris-agentic-dev` connects to your IRIS instance via the Atelier REST API — the same
API the ObjectScript extension uses. From the AI's perspective it gains a set of
capabilities it can invoke at any time:

- **Search the entire namespace** — full-text, regex, by category, without opening anything
- **Compile classes** and get errors back with line numbers
- **Run ObjectScript** and see the output
- **Execute SQL queries** against any namespace
- **Introspect class definitions** — properties, methods, parameters, inheritance chains
- **Inspect Ensemble productions** — which items are running, what's wired to what
- **Run unit tests** and report results
- **Debug** — map INT line numbers back to original source lines, pull error logs

The full tool catalog is the subject of Part 2. For now, the key point: the AI stops
guessing about your codebase and starts asking. The namespace is visible. The whole
namespace, not just your open tabs.

---

## Built With the Community

I started this project to solve a problem I kept running into in my own IRIS development
work, and I've shaped it with input from developers at InterSystems and across the
community. Several of the tools exist because someone said "this is where AI always gets
stuck on our codebase" — and the answer turned out to be giving the AI a way to ask IRIS
directly rather than inferring from stale context.

It's open source, under the `intersystems-community` GitHub organization. Contributions,
bug reports, and "it doesn't work on my setup" reports are all welcome — that kind of
feedback is exactly how it's gotten better.

---

## Getting Started: VS Code + GitHub Copilot

If you're already using VS Code with the InterSystems ObjectScript extension, this is
the fastest path.

**Prerequisites**: VS Code, GitHub Copilot subscription, and the
[InterSystems ObjectScript extension](https://marketplace.visualstudio.com/items?itemName=intersystems-community.vscode-objectscript)
(which you almost certainly already have).

**Step 1 — Install the VS Code extension**

Download `vscode-iris-agentic-dev-*.vsix` from the
[releases page](https://github.com/intersystems-community/iris-agentic-dev/releases/latest).

In VS Code: open the Extensions panel (`Ctrl+Shift+X`), click the `...` menu, choose
**Install from VSIX**, and select the file you downloaded. Reload VS Code.

That's the entire install. The extension bundles the MCP server and registers it with
Copilot automatically.

**Step 2 — Verify the connection**

Open Copilot Chat and switch to **Agent mode**. Ask:

> *"Call check_config and show me the result."*

You should see your IRIS connection details — host, port, namespace, Atelier API version.
If the [InterSystems Server Manager](https://marketplace.visualstudio.com/items?itemName=intersystems-community.servermanager)
extension is installed, `iris-agentic-dev` finds your server configuration and retrieves
credentials from the OS keychain automatically. No extra setup.

**Step 3 — Ask something that requires the whole namespace**

Try one of these to see the difference immediately:

> *"Search for all classes in this namespace that extend `%Persistent`. How many are there?"*

> *"What are the properties and methods on `MyApp.SomeClass`?"*

> *"Compile `MyApp.*.cls` and show me any errors."*

These work without opening a single file. The AI is talking to IRIS.

---

## Getting Started: Claude Code

If you use Claude Code instead of Copilot, the setup is a few more steps but still
straightforward.

**Install the binary** (Mac, Apple Silicon):

```bash
brew tap intersystems-community/iris-agentic-dev
brew install iris-agentic-dev
```

Or download directly from the
[releases page](https://github.com/intersystems-community/iris-agentic-dev/releases/latest)
for Mac Intel, Linux, or Windows.

**Add to `~/.claude.json`**:

```json
{
  "mcpServers": {
    "iris-agentic-dev": {
      "command": "iris-agentic-dev",
      "args": ["mcp"],
      "env": {
        "IRIS_HOST": "localhost",
        "IRIS_WEB_PORT": "52773",
        "IRIS_USERNAME": "_SYSTEM",
        "IRIS_PASSWORD": "SYS",
        "IRIS_NAMESPACE": "USER"
      }
    }
  }
}
```

Adjust host, port, credentials, and namespace for your environment. Then verify:

```
> Call check_config and show me the result.
```

---

## What's Coming in the Next Parts

**Part 2 — The Tools**: A practical walkthrough of the full tool catalog — what each tool
does, when you'd reach for it, and the kinds of IRIS-specific problems each one solves.
The search, introspection, and Ensemble tools in particular unlock things that simply
weren't possible with editor-buffer-only AI.

**Part 3 — Skills**: Even with a live IRIS connection, AI models have a well-known blind
spot for ObjectScript — subtle syntax differences, `%Status` propagation patterns,
`$$$` macro usage, COS-specific idioms that don't appear much in general training data.
Skills are short instruction files that correct these patterns. A 205-word checklist
called `objectscript-review` takes the benchmark pass rate from 73% to 100% on a
real-world ObjectScript repair task suite.

**Part 4 — Benchmarking**: How the benchmark harness works, how to run it yourself, and
what the numbers actually mean — including where skills help, where they're neutral, and
one skill that *hurts* performance when loaded globally (−19% lift — a useful reminder
that "more instructions" isn't always better).

---

## Links

- **GitHub**: [intersystems-community/iris-agentic-dev](https://github.com/intersystems-community/iris-agentic-dev)
- **Releases** (VSIX + binaries): [releases page](https://github.com/intersystems-community/iris-agentic-dev/releases/latest)
- **Original thread**: [Frogs, Chickens, AI, and VS Code](https://community.intersystems.com/post/frogs-chickens-ai-and-vs-code)

---

*Thomas Dyar — InterSystems, Developer Community contributor.
`iris-agentic-dev` is open source under the intersystems-community GitHub organization.*
