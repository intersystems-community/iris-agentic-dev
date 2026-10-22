# Changelog

This extension installs the iris-agentic-dev server, and each entry below is a server release. Extension 0.4.39 installs server 1.6.3. Releases with no entry here have their notes on the [releases page](https://github.com/intersystems-community/iris-agentic-dev/releases).

## v1.6.3

Upgrade if you run iad against a production instance, run tests through it, or use
`iris_symbols_local`. Most of the fixes below close ways around the write gate or ways to leak your
IRIS password; the rest stop `iris_test`, `iris_execute` and `iris_symbols_local` from reporting
wrong results. One behaviour changes: a workspace `.iris-agentic-dev.toml` now outranks `IRIS_*`.

### Notable fixes

- **A cloned repository cannot send your IRIS password to its own server.** A workspace
  `.iris-agentic-dev.toml` that named a host and no credentials got your `IRIS_USERNAME` and
  `IRIS_PASSWORD`, wherever that host was. Now they go only to this machine or to the host in
  `IRIS_HOST`; for any other host, put the credentials in the file. `check_config` reports
  `credentials_withheld` when this happens. The same holds for `IRIS_SERVICE_USERNAME` and
  `IRIS_SERVICE_PASSWORD`: a tool that runs as the service account refuses rather than running as you.
  A `scheme` other than `http` or `https` makes the file's connection unusable, because one such as
  `http://elsewhere/#` moved the URL past the host check. With the precedence change below, a
  repository's file would otherwise have won over `IRIS_HOST` too.
- **The write gate reads `mode` and `action` the way the tools do.** It compared them without
  trimming, so `mode: " put"` passed as a read and `iris_doc` then wrote, with writes off.
  `call_tool` now trims and lowercases both before any gate runs.
- **A leading `^` no longer gets past the system blocklist or the PHI name gate.** The gates
  compared the name as typed and the tools stripped the caret, so `iris_global` refused `ROUTINE`
  and then read or wrote `^ROUTINE`. Every gate now reads the same name the tool uses. The
  blocklist also covers `^rMAC`, `^rINC` and 16 other code-storage globals it had missed.
- **`iris_query` read mode refuses `GRANT` and `REVOKE`.** A read with `force: true` now counts as
  a write for the live and test templates, and the refusal no longer suggests `force`. Read mode
  still sends `CALL`, and the tool description and `docs/tools.md` now say that read mode does not
  stop a stored procedure with side effects, called with `CALL` or inside a `SELECT`; only the IRIS
  user's privileges do.
- **A Live instance is read as Live.** IRIS stores `^%SYS("SystemMode")` as `LIVE`, `TEST`,
  `DEVELOPMENT` or `FAILOVER`. iad matched `Live`, so on every real Live instance it found no
  mode, fell back to the namespace name, and left writes on in `USER` and any namespace not named
  like production. `LIVE` and `FAILOVER` now turn writes off unless you declare them on.
- **`iris_coverage` treats `test_path` as data.** It went into the generated `RunTest` call
  without escaping, so a crafted value ran ObjectScript at the write tier, past the check
  `iris_execute` makes for `Kill ^global`. The 1.6.0 injection sweep had covered `classes` and
  `package` but missed this field.
- **`iris_doc` refuses `CodeMode = generator`.** The compile-time execution check refused
  `objectgenerator` but not the older `generator`, which also runs code when the class compiles.
- **`iris_symbols_local` reads real classes.** The lockfile held tree-sitter-objectscript 1.9.13,
  which left a parse error in 32.5% of 5,485 documents from a USER namespace and missed 22% of
  class members, mostly at member headers such as `[ InitialExpression = 0 ]`, `[ ZenMethod ]`
  and `[ CodeMode = generator ]`. 1.10.3 leaves one in 1.1%. JavaScript `ClientMethod`s are listed
  too.
- **`iris_test` and `iris_coverage` survive a growing `messages.log`.** `%UnitTest.Manager` checks
  `messages.log` during a run, and if it grew it closes the file without putting the caller's
  device back. Over HTTP that lost the run's output: `iris_test` answered `NO_TESTS_FOUND` for a
  test that had passed, whenever any process wrote to `messages.log` mid-run. Both tools now run
  the manager in a background job and read its output back.
- **A garbled answer from IRIS is an error.** iad read an Atelier answer that was not JSON, an
  HTTP error, or an error in Atelier's status as an empty success, so `iris_execute` could report
  success with no output. It now returns an error that shows the start of the answer. Once IRIS
  has run the code, iad does not run it again: `iris_execute` answers `QUERY_FAILED_AFTER_RUN`
  instead of retrying or falling back to docker exec. Code whose output passed the string limit
  ran twice when a container was set, and a test run that hit the HTTP timeout could start again
  alongside itself.
- **Output that looks like JSON comes back as written.** Atelier hands a result that starts with
  `[` or `{` back as JSON, so output such as `[1,2` that was not valid JSON failed inside Atelier
  and reached the caller as an empty success. iad now marks its result so Atelier always returns it
  as text.
- **`resolve_dynamic_dispatch` finds methods whose signature holds a quote.** It joined its JSON by
  hand, so a default such as `pX As %String = "a"` made the whole answer unreadable and the tool
  reported no candidates. It now uses IRIS's JSON serializer.
- **An empty `IRIS_CONTAINER` means no container.** iad ran `docker exec ''`, and that error hid
  the reason the call had failed.
- **A server named by `IRIS_HOST` stays the server.** When it did not answer, discovery moved on
  to `IRIS_CONTAINER` and the localhost scan, so a session could run against a different instance
  from the one named. Calls now fail against the named server.
- **`objectscript-unit-test`** says not to run `%UnitTest.Manager` through `iris_execute`, and why.

### Breaking changes

- **`.iris-agentic-dev.toml` outranks `IRIS_*` at startup**, as `docs/connecting.md` has always
  said: a `--host` flag, then the workspace file, then the environment. Before, any `IRIS_HOST`
  overrode the file when the server started, while a live edit of the file won. If you set
  `IRIS_HOST` to reach a different server than the workspace file names, remove the file or pass
  `--host`. A `.iris-agentic-dev.toml` in your home directory is the exception: it is a personal
  default for every project without its own file, so it yields to `IRIS_HOST` and supplies the
  connection only when that is unset. Its tool and gate settings still apply, a live edit of it
  follows the same rule, and `check_config` then reports `connection_source: env_vars`.
  iris-interop-dev made the same change for workspace files (their #434). A workspace file that
  names only a `container` still loses to `IRIS_HOST`; I fix that next.

[v1.6.2...v1.6.3](https://github.com/intersystems-community/iris-agentic-dev/compare/v1.6.2...v1.6.3)

## v1.6.2

Upgrade if your agents use `iris_search`. Before 1.6.2 it stopped at 200 matches without saying so,
and `regex: true` never searched by regex. Both are fixed, and two skills that sent agents to the
wrong tool for a class's members are corrected.

### Notable fixes

- **`iris_search` says when it stopped short.** Atelier stops a search at 200 matches unless told
  otherwise, and says so only in a field iad never read, so a search could stop at 200 and look
  complete. iad now sends a limit (`limit`, default 200, up to 10000) and the response carries
  `limit_reached: true` and a `warning` when Atelier hit it. On `%Library.*.cls`, `Quit` stops at
  200 matches in 11 files; with `limit: 1000` it returns 1000 in 43.
- **`iris_search` with `regex: true` searches by pattern.** iad sent `regex=true`, which Atelier
  reads as off, so every regex search was a literal one: `Test.*Passes` looked for the text
  `Test.*Passes`. iad now sends `regex=1`, and wraps the pattern so it matches anywhere in a line,
  as in grep, because Atelier matches a regex against the whole line. `^` and `$` still anchor it.
- **`objectscript-navigation` and `objectscript-repair`** said `iris_symbols(query="ClassName.*")`
  lists a class's members. It matches class names only, and that query returns nothing. Both
  skills now send member lookups to `docs_introspect`, and `objectscript-repair` checks a rename
  with `iris_search`, because a call to a renamed method still compiles and fails only when it
  runs, with `<METHOD DOES NOT EXIST>`.
- **`docs/tools.md`** said `iris_doc put` strips Storage blocks because IRIS 2025.1+ rejects them.
  iad never stripped them, and IRIS 2025.3 and 2026.2 both accept a Storage block in a PUT. The guard that
  exists refuses a `put` that would drop a class's Storage block, unless
  `allow_storage_regeneration` is set.

### Breaking changes

- **`iris_search` with `regex: true` now matches by pattern.** A script that passed a regex and
  got literal matches back will get pattern matches.

[v1.6.1...v1.6.2](https://github.com/intersystems-community/iris-agentic-dev/compare/v1.6.1...v1.6.2)

## v1.6.1

Upgrade if iad reaches IRIS over `https` (`IRIS_SCHEME=https`, or `scheme = "https"` in
`.iris-agentic-dev.toml`). 1.6.0 shipped with rustls 0.23.45, and RustSec has since published four
advisories against it and one against rustls-webpki. iad uses rustls for every TLS connection it
makes: HTTPS and WebSocket calls to IRIS, and the fetches from GitHub behind `skill install` and
`skill_community`. A connection over plain `http`, the default, never touches it.

### Security

- **rustls 0.23.46 and rustls-webpki 0.103.16.** They fix:
  - RUSTSEC-2026-0346: the `Debug` output of a client session value printed TLS secrets.
  - RUSTSEC-2026-0347: data buffered before an ECH rejection went to the outer server.
  - RUSTSEC-2026-0348: Brotli certificate decompression could allocate up to 1 GiB before the
    server was authenticated.
  - RUSTSEC-2026-0349: an unauthenticated ServerHello could downgrade later connections to
    classical key exchange.
  - RUSTSEC-2026-0382 (rustls-webpki): certificate verification could accept a distrusted
    intermediate.

`cargo audit` is clean again.

### Notable fixes

- **`iris_doc_search` reads the docs site's Algolia key at run time.** iad carried a copy of the
  key docs.intersystems.com publishes in its pages, and that key turned out to carry write rights
  as well as search (TPRM-6602). When the docs team rotates it, the copy would have stopped the
  tool working. iad now reads the key from a docs page's meta tags once per process, and answers
  `DOC_SEARCH_KEY_UNAVAILABLE` when it cannot. The key is no longer in the repository, and the
  `iris-docs` skill's manual recipe reads it the same way.

[v1.6.0...v1.6.1](https://github.com/intersystems-community/iris-agentic-dev/compare/v1.6.0...v1.6.1)

## v1.6.0

Upgrade from 1.5.0 soon if you run iad against an instance with data you care about. In 1.5.0 and
earlier, about forty tool parameters went into generated ObjectScript, SQL, a URL or a remote shell
command without escaping. A crafted value could close the literal it landed in and run code. With
write tools off, such a value deleted a test global through `iris_global` (`global_name`),
`iris_database_stats` (`db`, which runs in `%SYS`), `iris_coverage` (`classes`) and `stream_inspect`
(`oid`), and reached generated code through about ten more read-only tools. The caller is an AI
agent, and text it reads can carry instructions, so read-only mode was not a boundary. In 1.6.0 every
site I found is fixed, each with a live test that failed before the fix, and a check in the release
gates fails the build when a new one appears.

The same work found three tools that skipped the policy gate. `global_preview` read `^oddDEF` past the
system blocklist and a PHI-named global with no `acknowledgePhi`, and the standalone `journal_search`
returned journal contents under the default `dataPolicy = "block"`. All three are gated now. Read
**Breaking changes** if a script calls `journal_search`.

For anyone reviewing iad before deploying it, the release now ships a `SHA256SUMS` file and a
CycloneDX SBOM, and the repository has a `SECURITY.md` and a production profile.

### Security

- **Tool parameters are data.** iad checks names against IRIS syntax before it calls IRIS: global, class,
  routine and document, method, namespace and SQL table names, and journal timestamps. Values go
  through one escaper that doubles quotes and splices control characters with `$CHAR`, so a quote or
  a line break in a value stays in the value. SQL values in `mermaid_class`, `mermaid_production`,
  `resolve_storage` and the legacy rule lookup are bound as parameters. iad checks every `namespace` argument once, before any tool's handler runs. Tools fixed include `iris_global`,
  `global_preview`, `global_kill`, `iris_database_stats`, `journal_search`, `iris_admin`
  (`journal_search`, `database_status`, `mirror_add_async`), `iris_debug`, `iris_doc`
  (`mode=compiled`), `iris_coverage`, `stream_inspect`, `hl7_schema_inspect`, `iris_table_info`,
  `resolve_dynamic_dispatch`, `skill`, `kb`, `skill_describe`, `skill_forget`, `iris_test`,
  `iris_execute_method`, `iris_compile` on the docker path, `iris_production` start,
  `iris_namespace_create`, `iris_macro` and `iris_search`. The full list, with the commit and the
  test for each, is in `specs/137-objectscript-injection/research.md`.
- **`global_preview`, `global_kill` and `journal_search` go through the policy gate.** The system
  blocklist and the PHI-name gate also read a parameter named `global` now, which is what the two
  global tools call it.
- **The literal `Kill` check reads every argument.** `Kill x,^Y` passed the destructive-tier check on
  `iris_execute`, because the check looked only at the first argument.
- **The ssh route checks what it hands the remote shell.** The container and namespace are checked
  and single-quoted, and a host starting with `-` is refused, since ssh reads it as an option.
- **`iris_query mode=count` is checked like any read.** It skipped the read-only SQL check the other
  modes apply.
- **`iris_interop_query` `body_where`** is held to the same read-only rules as `iris_query`, and may
  not contain a comment.
- **Dependencies.** Nine RustSec advisories are cleared (`h2`, `quinn-proto`, `rustls`, `quick-xml`).
  `cargo audit` and `npm audit` now run on every pull request, every push to master and weekly, and a
  known vulnerability fails the job.
- **Release integrity.** Each release attaches `SHA256SUMS` for the five binaries and
  `iris-agentic-dev-<version>.cdx.json`, a CycloneDX SBOM of every locked dependency. The binaries are
  still not signed with a publisher certificate; `SECURITY.md` says how to check one or build from a
  tag.
- **Docs for reviewers.** `SECURITY.md` gives the reporting route and fix-time targets.
  `docs/production-profile.md` lists every place the binary can send data, the settings that reduce
  the risk, what no setting protects, and a pinned install that downloads nothing at run time.

### What's new

- **`ensemble-production` writes what the Portal's editors can open.** Analysts open processes,
  transforms and rules in the BPL, DTL and Rule editors, and those open only BPL, DTL and rule classes.
  The skill now makes those the default for ObjectScript work and shows how code maps onto them: a
  synchronous call with a timeout, a `<scope>` so a failed call does not end the process, a rule's
  return value and the rules that fired, lookups as read-only rule functions called by bare name, and
  writes kept out of rules. The skill writes a code-based process only when you ask for one. Every fact in
  the section, and every example, is checked by a live test on IRIS 2026.2. This came from Benjamin de
  Boe and the team building the grievance demo. On three new held-out tasks, gpt-oss-120b passed 2 of
  9 sessions with the skill and 0 of 8 without it; gpt-6-luna passed nearly all of them either way, and
  with the skill used about half the calls on the rewrite task. The numbers and their limits are in
  `specs/136-interop-ui-artifacts/results.md`.
- **New skill: `iris-mdx`** (#142), for MDX queries against IRIS BI cubes: hierarchy paths, `NON EMPTY`,
  `%FILTER` and `%OR`, member keys against captions, calculated members, time navigation, and the
  queries that return wrong numbers with no error. Asaf Sinay wrote it from his work on the Samples-BI
  cubes. Each disputed claim was run on 2026.2, which also turned up an IRIS bug: a query with `%MDX`
  returns nothing after its first run on that build. It is an extra skill, so a bare
  `iris-agentic-dev skill install` leaves it out; name it, or pass `--all`.
- **`iris_business_rule_info` reads rule classes.** It used to read only the `Ens.Rule.RuleSet` table
  from before 2012, so it answered `RULE_NOT_FOUND` for every rule the Rule Editor opens. `list` now
  returns rule classes and any legacy rows, each with `kind: class` or `kind: legacy`; `get` on a rule
  class returns its rule type, context class, rule sets, and each rule's conditions and actions.
- **The VS Code extension has a changelog** (#136). The extension page now has a Changelog tab with
  these release notes, newest first. John Murray asked for it.
- **Tool hints.** A delete callback with the wrong signature is told its argument is an OID; a
  compiled `%OnBeforeDelete`, or any `%On` method `%Library.Persistent` does not define, is told IRIS
  never calls it; `iris_execute_method` says an object does not survive the call, since each call is a
  new process.

### Notable fixes

- **Native Windows IRIS** (#139). Every call that captures output (`iris_execute`, `iris_execute_method`,
  `iris_doc put`, `iris_production` and others) failed there from 1.3.2 on with `ERROR($DEVICE)`.
  Windows gives `$IO` back lower-cased while the temp-file name keeps its case, and the capture check
  compared them exactly. It now compares without case. I could not test on Windows; the check is the
  one the issue points at, and the Linux suite still passes.
- **Non-ASCII output** (#138). Japanese, Chinese and emoji written by `iris_execute` code came back as
  `?`, and so did any other output iad captures. The capture file was written and read with no
  translation table, so IRIS fell back to 8-bit. Both ends use UTF-8 now. Tomo Okuyama found it and
  sent the fix (#145).
- **`mermaid_production`** used item names as Mermaid node IDs, so a name with a space, such as
  `From DEMO LabService ADT`, gave a diagram that does not parse (#144). Node IDs are now letters,
  digits and `_`, unique within the diagram, and the label keeps the real name.
- **`iris_global get subtree=true`** returned nothing below a subscripted node (#146): the walk
  compared each node against the whole base reference, closing parenthesis included. It now also
  returns the base node's own value first when the node has one.
- **`iris_query` in write mode** on the docker path broke on a statement spread over several lines.
- **The SCM helpers** wrote a line break in a document name or credential as the text `$Char(10)`
  inside the literal.
- **`iris_test`** with a quote in its pattern produced code that did not compile.
- **The generated workspace TOML** quotes values with the TOML library, so a value with a quote or a
  backslash still parses.
- **`objectscript-guardrails`** tells the agent its checklist is settled. With the skill loaded, the
  agent had made 4.7 doc searches and 30 calls per session against 1.1 and 19 without it, for the same
  passes. On one task after the change, the skill arm went from 2.6 to 1.8 times the baseline's calls.
- **`ensemble-production`** taught `##class(Ens.Director).SendRequestSync(...)`, which does not exist.
  It now sends a test message through an adapterless service's `SendRequestSync`, and says that a
  pyprod production's Python file overwrites settings changed in the Portal.
- **`iris-ai-hub`** no longer sends the agent to search a documentation index that has no AI Hub pages.

### Breaking changes

- **`journal_search` is refused under `dataPolicy = "block"`**, the default, with
  `DATA_POLICY_BLOCKED`, the same answer `iris_admin action=journal_search` already gave. Journal
  records carry global values. Set `dataPolicy = "allow"` in the connection's `[policy.<server>]`
  block to use it.
- **`global_preview` and `global_kill` refuse** system globals (`SYSTEM_BLOCKLIST`) and PHI-named
  globals without `acknowledgePhi` (`PHI_GATE_BLOCKED`), as `iris_global` does.
- **New refusal codes**, returned before any IRIS call for a value that is not a name of its kind:
  `INVALID_GLOBAL_NAME`, `INVALID_CLASS_NAME`, `INVALID_ROUTINE_NAME`, `INVALID_NAMESPACE`,
  `INVALID_METHOD_NAME`, `INVALID_SQL_IDENTIFIER`, `INVALID_TIMESTAMP`. A script that passed a
  malformed name and parsed the IRIS error it got back will see one of these instead. `iris_search`
  refuses a `category` other than `CLS`, `MAC`, `INT`, `INC` or `ALL` with `INVALID_PARAMS`.
- **`iris_business_rule_info`** results carry `kind`, and `list` includes rule classes.
- **`iris_global get subtree=true`** includes the base node as the first row when it holds a value.

[v1.5.0...v1.6.0](https://github.com/intersystems-community/iris-agentic-dev/compare/v1.5.0...v1.6.0)

## v1.5.0

The bundled skills changed more in this release than the tools did. `iris-ai-hub` is now the one AI
Hub skill, rewritten against the 2026.3 EAP build, and the vendored `aihub-eap` copy is gone. Two
skills are new, `iris-query-plans` and the setup skill, and I corrected the claims in the others
that IRIS 2026.2 contradicts. Every skill now has a tier, core, extra or internal, and a bare
`skill install` and the Claude Code plugin give you the ten core skills only. If you ran
`skill install` before, read **Skill tiers** below: nothing is deleted, but the next install
tells you what an older install left behind and how to remove it or keep it. If you
submit skills, the way to show a skill works has changed: the ladder replaces `benchmark/021`,
and the published skills figures are withdrawn.

Several tools in 1.4.2 never did what their description said. `iris_macro` called an Atelier route
that does not exist, so every call returned `{}`. `iris_info what=sa_schema` encoded the slashes in
its URL and got a 404 for every input. `iris_execute` rewrote `&sql` on the HTTP path in a way that
made `SELECT COUNT(*) INTO` fail with `<PROPERTY DOES NOT EXIST>`. Each iad process held two or three
CSP sessions open for an hour after it exited, which on a small license key ends in
`<LICENSE LIMIT EXCEEDED>`. All four are fixed, and each has a live test against a real IRIS.

Fixing them changed some response shapes, so read **Breaking changes** before upgrading a script
that parses `iris_macro`, `iris_info what=documents`, `iris_execute_method` or `iris_execute` output.

### Skills

#### Skill tiers

A bare `skill install` now installs ten skills, not 34. Every bundled skill has a `tier:` line in
its frontmatter:

- **core** (10): `iris-agentic-dev`, `objectscript-guardrails`, `objectscript-review`,
  `objectscript-sql-patterns`, `iris-sql`, `objectscript-tdd`, `objectscript-unit-test`,
  `objectscript-debugging`, `objectscript-list-patterns`, `iris-connectivity`.
- **extra** (23): everything else, `iris-query-plans` and `iris-ai-hub` included.
- **internal** (1): `opencode-introspect`, which reads opencode's session database and is only
  useful for working on iad itself.

What each install path gives you:

| Install                                              | 1.4.2                          | 1.5.0                                         |
| ---------------------------------------------------- | ------------------------------ | --------------------------------------------- |
| `iris-agentic-dev skill install` (no arguments)      | all 34                         | the 10 core skills                            |
| `iris-agentic-dev skill install --all`               | no such flag                   | core and extra, 33 skills                     |
| `iris-agentic-dev skill install <name> ...`          | the named skills               | the named skills, at any tier                 |
| Claude Code plugin (`/plugin install iris-dev`)      | did not install; then 3 skills | 15: the 10 core, `nopws-setup`, 4 plugin-only |
| MCP server only (`claude mcp add`, Homebrew, binary) | 34 skills, loaded on request   | 34 skills, loaded on request                  |

The extra skills did not go anywhere. The binary still embeds all 34, and `skill_describe` reads
any of them by name without installing anything. `skill_list` and `skill_search` show core and
extra with their tier, and leave out the internal one. The core `iris-agentic-dev` skill now ends
its task map with one line per extra skill saying when to load it, so an agent that has only the
core set still knows the rest exist.

**If you ran `skill install` on 1.4.x**, you have managed copies of all 34 skills in
`~/.claude/skills` and the OpenCode skills directory. Upgrading does not touch them. A bare
`skill install` on 1.5.0 updates the ten core skills, deletes nothing, and prints the extra and
internal skills it found, with the two ways to deal with them:

```bash
iris-agentic-dev skill install --all              # keep the extra skills and keep them updated
iris-agentic-dev skill install --prune --dry-run  # list what --prune would remove
iris-agentic-dev skill install --prune            # remove them
```

`--prune` deletes only files that carry the `managed_by: "iris-agentic-dev"` marker the installer
writes. If you edited one of those skills and kept the marker, `--prune` deletes your edit, so run
`--dry-run` first. A `SKILL.md` without the marker stays, and so does any other file in the skill's
directory. `--prune --all` removes only `opencode-introspect`. `--prune` with skill names is
refused. Copilot's `.github/instructions/` is never pruned; it lives in your repo.

The plugin used to declare the whole `skills/skills` directory. In 1.4.2 that did not load at all,
since the plugin had no `.claude-plugin/marketplace.json` and the loader looks one level higher.
`plugin.json` now names the ten core skills one path each, plus `nopws-setup`, the file two tools
point at when a NoPWS build refuses a connection. The four plugin-only skills at `skills/` root
(`iris-agentic-dev-setup`, `iris-coverage-run`, `iris-coverage-setup`, `pyprod`) load as before.

The MCP server path works as before. The server's instructions still tell the agent to load
`objectscript-guardrails` and `objectscript-review` before it writes ObjectScript.

One installer bug went with this. `skill install` decided whether it owned a file by looking for
the marker in the first 512 bytes, and it writes the marker at the end of the frontmatter. For
skills with long frontmatter, `objectscript-guardrails` and `iris-query-plans` among them, every
re-install said "user-authored — use --force to overwrite" and left the 1.4 copy in place. It now
reads the whole frontmatter, so those skills update.

#### `iris-ai-hub` 0.2.0 replaces both AI Hub skills

I rewrote `iris-ai-hub` against the 2026.3 EAP build (IRISHealth 2026.3.0AI.139.0). It names
`github.com/intersystems-community/ai-hub-eap` as the doc source, maps each topic to the file that
covers it (ConfigStore, MCP server, ObjectScript SDK, LangChain, samples), and tells the agent that
the `%AI` classes installed on the instance decide names and signatures when the docs disagree. A
section lists what held on build 139 under a live test, and another lists seven places the
ai-hub-eap docs do not match it. The vendored `aihub-eap` skill is gone; `skill_community` installs
upstream's own copy if you want it.

#### New skill: `iris-query-plans`

How to read an IRIS plan, and four things that make a query slow without an error: an index added
to a class that already has rows stays empty until `%BuildIndices`; `INSERT %NOINDEX` skips index
maintenance the same way; `TUNE TABLE` has to run before the optimizer knows the data; and one
outlier value can make a column look selective when it is not. Every claim has a live test.

#### A setup skill an agent can follow from a bare machine

The plugin ships `iris-agentic-dev-setup`: find or install the binary, find an IRIS before starting
one, clear the expired default password, read the web port off the container, register the server,
verify. I wrote it against a throwaway container, not the docs. It installs the right release asset
for all five platforms a tag builds, and `tests/e2e/test_setup_skill.py` checks every subcommand,
tool name and `--args` payload it names against the real CLI.

#### Skill corrections

I ran the factual claims in the bundled skills against IRIS 2026.2 and corrected the ones that did
not hold. The ones most likely to have produced wrong code:

- `objectscript-sql-patterns`: `If SQLCODE` is true for 100 (no row) as well as for an error, so it
  cannot tell them apart. SQLCODE -114 is a lock timeout, not a miss, so check for any negative
  code rather than only 100. `COUNT(*) INTO` with no matching rows sets SQLCODE 0 and the count
  to 0. Call `%Next(.tSC)` and check `tSC`.
- `ensemble-production`: the method is `GetProductionStatus`, with five states;
  `GetProductionState` does not exist. Neither stop mode loses messages. A hand-written
  `DataLocation` fails at compile time with #5477.
- `iris-objectscript-eval`: the compile flag `e` is "Delete extent". The skill had called it
  "display errors only".
- `objectscript-list-patterns`: appending with `_$LISTBUILD` is linear (80,000 items in 0.002 s);
  `$LIST(lst,*+1)` is quadratic (1.71 s). The skill had called `_$LISTBUILD` quadratic.
- `iris-sql`: `%Execute` does bind arrays. The skill had said it does not.
- `objectscript-guardrails`, `objectscript-review`: `New $NAMESPACE` before `Set $NAMESPACE`, and
  roll back with `TROLLBACK:$TLevel>entry 1`.
- `objectscript-guardrails`, `objectscript-loop-patterns`, `objectscript-fewshot-fixes`: a space
  in a postfix condition is #1054; `Quit value` inside a loop is `<COMMAND>`, or #1043 inside
  `Try`/`Catch`.
- `objectscript-mac-routines`: `Try`/`Catch` and `Return` are allowed in a `.mac`.
- `objectscript-unit-test`, `objectscript-tdd`: test specs take `:Package.Class`, only methods named
  `Test*` run, and `NO_TESTS_FOUND` is a failure.
- `iris-agentic-dev`: what `<CLASS DOES NOT EXIST>` means for a system class, and how to load an XML
  export (#16006, #16021).

### For skill contributors

#### Showing that a skill works

`benchmark/021/` is retired, and the ladder in `tests/e2e/skill_eval/` is the one harness.
`CONTRIBUTING.md` describes the new route:

- For a skill on something the corpus already covers, run the ladder with and without the skill
  and paste the comparison line it prints into the PR. That line carries the item count and the
  interval; a lift figure without them cannot be checked.
- For a new domain, add tasks to `tests/e2e/tasks/benchmark/corpus/` with the skill. A task needs a
  check that prints exactly `PASS` or `FAIL` against live IRIS, a reference solution that passes
  it, and a fixture that fails before the agent starts. No model grades a task.
- Put each new task ID on the `holdout` side of `tests/e2e/tasks/benchmark/split.toml`. The loader
  refuses a corpus where an ID is on neither side, and CI now runs that load, so a task without a
  side fails the PR.
- The ladder spends real money. `--dry-run` prints the session count and cost estimate, and
  `--cap` stops a run on measured spend; see below for why the estimate alone is not enough.

Figures citing `benchmark/021` or an older `lift-results.md` came from an LLM judge on a different
corpus and are not comparable to a ladder figure.

CI also runs the whole `tests/e2e` Python suite now, minus tests that need IRIS, the network or a
paid session. It had run three named files out of about a thousand tests.

The nightly `skill-regression` job no longer gates on a per-skill delta. It runs three
assertion-scored canary sessions and a routing check that re-scores the shipped skill descriptions
on a frozen holdout and fails when Recall@1 leaves the band in
`tests/e2e/tasks/routing/drift-interval.json`. A description change that makes agents pick the wrong
skill shows up there.

#### The published skills and tools figures are withdrawn

The README and `specs/121-benchmark-program/results.md` said the skills measured +0.098 on a
41-pair graded holdout and the tools +0.829. Both came from harness sessions that were not
isolated: opencode reads `~/.claude/CLAUDE.md` and `~/.claude/skills` into every session unless
`OPENCODE_DISABLE_CLAUDE_CODE=1` is set, and `IsolatedEnv` did not set it. Every session in that
run, in every arm, had my global instructions and about 160 personal skills loaded, including an
old copy of `ensemble-production` in the "tools only" arm. I treat every skill-arm and tools-arm
figure recorded before 2026-09-26 as void. The finding is in `specs/130-content-skills/research.md`,
and the README, `results.md` and the 121 quickstart mark the figures withdrawn.

`IsolatedEnv` sets the variable now, and a billable test fails if a personal skill shows up in a
session. On clean isolation, a 19-task ladder at three repeats passed 11 of 19 with tools alone and
12 of 19 with tools and the matching skill: b=3, c=2, below the run's power floor. The AI Hub tasks
in spec 132 gave b=1, c=0, also underpowered. So there is no measured skills effect in either
direction yet, and no current tools figure.

#### Harness repairs

From the same work:

- The judge reads tool arguments and results up to 8000 characters; they had been cut at 120 and
  200, which hid the evidence it was asked to grade.
- `_reached_idle` accepts a final `step_finish` with `reason: stop`. `opencode run --format json`
  never emits `session.status`, so every baseline session that answered in text had gone unscored
  while skill-arm sessions were scored.
- A graded session deletes every class it created under the fixture's packages. One leftover class
  ended two later sessions at 3 calls and produced a false `needs_fix` flag.
- The ladder stops on opencode's measured `step_finish.cost` with `--cap`. The per-session
  estimate was 3 to 35 times low, and one run spent $12.40 against a $3 cap.

### What's new in the tools

#### Errors say where the fix is when iad knows it

`iris_query`'s four `SQL_ERROR` sites and `iris_execute`'s three `IRIS_RUNTIME_ERROR` sites add a
`hint` field when the error matches one of seven rules, and leave it out otherwise:

| Rule                        | Fires on                                                                |
| --------------------------- | ----------------------------------------------------------------------- |
| `sql_objectscript_function` | an ObjectScript function such as `$ZDATETIME` inside SQL                |
| `sys_only_class`            | `<CLASS DOES NOT EXIST>` for a class that resolves only in `%SYS`       |
| `sys_only_table`            | a `Security` or `Config` table queried outside `%SYS`                   |
| `deep_package_table`        | a three-part class name used as a table name with its dots unchanged    |
| `double_quoted_string`      | a string in double quotes, which IRIS SQL reads as an identifier        |
| `reserved_word`             | a reserved word used as a column or table name                          |
| `nonstandard_insert`        | `INSERT OR REPLACE`, `INSERT OR IGNORE`, `INSERT IGNORE`, `ON CONFLICT` |

The wording lives in `crates/iris-agentic-dev-core/src/tools/hints.toml`, and every rule came from
an error I captured on a live instance. Each hinted response also carries
`hint_ref: {skill, section, why}` naming the skill section that covers the mistake. The hint text
itself never names a skill, so it still reads correctly without the skill installed.
`IAD_CODING_PACK=off` drops `hint_ref`; it is read on every call, so no restart is needed.

#### `iris_macro` works, and finds the include for you

It calls the real `getmacrosignature`, `getmacrolocation`, `getmacrodefinition` and
`getmacroexpansion` routes now. Without an `includes` parameter iad searches every `.inc` in the
namespace; an include that does not compile is found by halving the list and reported in
`skipped_includes` rather than failing the whole call. A leading `$$$` on the name is stripped.
`action=list` lists include names, or with `includes`, the macros those files define, where a
trailing `(` marks one that takes arguments. A macro that is not there answers `MACRO_NOT_FOUND`.

#### `iris_info what=documents` and `iris_doc mode=list` hide scratch classes

Both hide the `IrisDevTmp.*` classes iad creates for its own work, and take `include_scratch: true`
to show them. `iris_info` reports how many it hid in `scratch_hidden`. `iris_doc` shows them
anyway when the pattern itself starts with `IrisDevTmp`.

`iris_info what=documents inline=true` stops at 500 names, and a cut list carries `truncated`,
`total_count` and a `hint`. On USER it had returned 83,546 names, about 20 MB, in one response.
`IRIS_INFO_MAX_DOCUMENTS` changes the cap.

#### Docs

- `docs/getting-started.md` is new: six steps from a running IRIS to Claude Code calling iad, and
  a section for when a step fails.
- README shows the plugin install (`/plugin marketplace add`, `/plugin install iris-dev`) and
  `claude mcp add` registration.
- `docs/connecting.md` has a section on what the tiers guard and what they do not: they stop an
  agent from doing something by accident, and they are not a security boundary.
- `docs/examples/todo-app/` is the demo that exposed several fixes below: two classes, `STEPS.md`
  and a transcript. A live test puts and compiles both classes, creates the web application, adds,
  toggles and deletes rows, and tears it all down.

### Notable fixes

#### Every iad process leaked CSP sessions into the license

Each client in a process kept its own cookie jar, so one iad process opened two or three CSP
sessions, and none were logged out when it exited. Each held a license slot for 3600 s. On the
IRISHealth 2026.3 EAP key, two benchmark tasks were enough to reach `<LICENSE LIMIT EXCEEDED>`. Every
client in a process now shares one cookie store, and the exit path sends `?IRISLogout=end` for each
session it opened.

#### `iris_execute` and `&sql`

On the HTTP path `iris_execute` rewrote `&sql(...)` into dynamic SQL before sending it, and the
rewrite read result columns by expression text, so `&sql(SELECT COUNT(*) INTO :n FROM ...)` failed
with `<PROPERTY DOES NOT EXIST>`. IRIS compiles the code inside a class method, where embedded SQL
works natively, so the HTTP path sends `&sql` untouched now. The rewrite still runs on the docker
exec path, which has no class to compile into. There it reads columns by position and sets
`SQLCODE`, `%msg` and `%ROWCOUNT` the way embedded SQL does.

#### `iris_info what=sa_schema` had 404ed for every URL

iad encoded the slashes in the XData namespace URL, so Atelier found nothing at any input. It sends
them raw now. The description said it returned an "SQL Analytics schema"; it returns the Studio
Assist grammar for an XData namespace URL, and says so. A `name` that is not an http(s) URL is
refused with `INVALID_PARAMS` before any IRIS call, and a missing grammar answers
`SA_SCHEMA_NOT_FOUND`. Both errors point at `%DeepSee.Utils` `%GetCubeList` and
`%GetDimensionList` through `iris_execute`, since anyone who passed a cube name wanted the cube list.

When the cached grammar is missing or stale, as on a fresh instance or after any cube class
compiles, Atelier starts a background build and answers 202 with an empty result. iad now asks
again every 500 ms for up to ten seconds, and answers `SA_SCHEMA_PENDING` if the build still has
not finished. Before, that first call came back as `SA_SCHEMA_NOT_FOUND`.

#### `iris_execute_method` reported an error `%Status` as success

A method that returned an error status came back as `success: true` with the raw `%Status` bytes in
`return_value`. It returns `METHOD_RETURNED_ERROR` with the decoded `GetErrorText` text now.

#### `iris_ws_exec` ran only the first line of multi-line code

The whole block went to the terminal as one input, so line 1 ran and the rest was lost or ended in
`<SYNTAX>`. Each non-blank line is sent as its own input now. A braced block has to sit on one line,
the same as at a terminal prompt.

#### `iris_doc mode=list` for routines

`MAC`, `INT`, `INC` and `ALL` answered HTTP 400 or missed routines, because they went to the class
route. They use the `RTN/<type>` routes now, and an unknown category passes through to IRIS instead
of quietly becoming `CLS`.

#### Scratch classes leaked into the namespace

iad compiles short-lived `IrisDevTmp` classes to run code, and a cancelled call left its class
behind. One instance had 78,183 of them in USER, nearly all from telemetry. User code now runs
under the `IrisDevRun` prefix and telemetry under `IrisDevTel`, a cancelled call deletes its class,
and a failed delete logs a warning instead of passing silently.

#### The Claude Code plugin could not be installed, and then launched the wrong binary

The repo had no `.claude-plugin/marketplace.json`, so `plugin marketplace add` failed. With that
fixed, the plugin launched `iris-dev`, a binary no release has installed; on a machine with an old
`~/.local/bin/iris-dev` it started 0.9.10 silently. It launches `iris-agentic-dev` now, and a test
checks the command against both the Cargo binary name and the name the Homebrew formula installs.
The `env` block that set six connection variables to empty strings is gone, so the server resolves
its connection from `.iris-agentic-dev.toml` like every other client.

#### The CLI handed out WebSocket tokens nothing could use

`iris-agentic-dev tool iris_ws_open` opened a session, printed a token, and exited. The token was a
handle into a pool of live sockets held in that process's memory, so the next command answered
`SESSION_STALE: Session token references an unknown server or expired session`, describing a
timeout that had not happened against a server that was fine.

`tool` refuses `iris_ws_open`, `iris_ws_exec` and `iris_ws_close` now, before it resolves a
connection, and the refusal carries the `batch` script that works:

```bash
echo '[{"tool":"iris_ws_open","args":{}},
       {"tool":"iris_ws_exec","args":{"session":"{{0.session}}","code":"Set x=1"}},
       {"tool":"iris_ws_exec","args":{"session":"{{0.session}}","code":"Write x"}},
       {"tool":"iris_ws_close","args":{"session":"{{0.session}}"}}]' | iris-agentic-dev batch
```

`batch` has been there since 1.3.0 and appeared in no documentation. It is in the README command
list and in `docs/troubleshooting.md` now, and `docs/tools.md` explains the process boundary where
the session tools are documented. `iris_get_log` and `iris_doc`'s checkout prompt keep state the
same per-process way and need `batch` for the same reason.

`SESSION_STALE` has two texts now, read from one function: the CLI one names the process boundary
and `batch`, the MCP one names a closed session or a restarted server.

`docs/tools.md` also promised that closing an already-closed session returns `already_closed: true`.
The handler has never returned that. It returns `SESSION_STALE`, and the doc says so.

#### Shutdown

In 1.4.2 a SIGTERM ended `iris-agentic-dev mcp` on the spot, before it finished writing telemetry
for the last call, and that write's scratch class stayed in USER. It handles SIGTERM and SIGINT now
and logs `SIGTERM: shutting down`. It then flushes telemetry, logs out its CSP sessions, and gives
calls still running up to 2 s to delete their scratch classes. Stopping the server takes about
2.2 s as a result, where it used to be instant. Every CLI exit flushes telemetry (up to 3 s) and
logs out its CSP sessions first, so a command can take up to about 3 s longer to return. Exit codes
are unchanged. `iris-agentic-dev exec` printed nothing when a gate refused the code; it prints
`error_code: message` now.

#### Tool descriptions name the tier the gate enforces

Eighteen tool/action descriptions disagreed with the gate table. `iris_admin` listed seven
destructive actions as write, `global_kill` and `iris_namespace_create` said WRITE-GATED, and four
destructive actions said nothing. A unit test reads every description against the table. No gate
behaviour changed. `iris_admin` also says which actions need `IRIS_ADMIN_TOOLS=1`, and
`iris_execute` says its destructive gate catches only a literal `Kill ^global`.

#### Adding a server no longer tells the agent to restart

`iris_add_server`, `iris_remove_server` and `iris_import_servers` said to restart iad. An agent in
another repo took that at face value and stopped using iad. They point at `iris_reload_pool` now,
which rebuilds the pool in the running server.

#### `iris_servers` reports the URL it calls

Every entry carries `base_url`, prefix included. The 1.4.2 notes and `docs/tools.md` said it did,
and it did not, so two instances behind one gateway still came back as the same host and port.
I corrected the 1.4.2 notes.

### Breaking changes

- **`iris-agentic-dev tool iris_ws_open`, `iris_ws_exec`, `iris_ws_close`** exit 1 with a refusal.
  A script that read the token out of `iris_ws_open` was already broken: its next call returned
  `SESSION_STALE`. Under MCP the three tools are unchanged.
- **`iris_macro`** returns `namespace`, `includes`, `document` and `line`, plus `definition`,
  `signature` or `expansion` depending on the action, in place of `result`. A missing macro is an
  error (`MACRO_NOT_FOUND`), and a missing `name` is `INVALID_PARAM`. The old shape only ever held
  `{}`.
- **`iris_info what=documents`**: the list appears once, as top-level `documents`; `result.content`
  is gone. The default `doc_type` `ALL` means every type, where it used to mean classes only.
  `inline=true` stops at 500 names unless `IRIS_INFO_MAX_DOCUMENTS` says otherwise.
- **`iris_info what=sa_schema`**: a `name` that is not an http(s) URL is `INVALID_PARAMS`. Passing a
  cube name had returned an empty 404 body before, so nothing that worked stops working.
- **`iris_execute_method`**: an error `%Status` is `METHOD_RETURNED_ERROR`, not `success: true`.
- **`iris_execute`** over HTTP no longer returns `sql_translated` or `translated_code`, since
  nothing is translated there. The docker exec path still returns both.
- **Plugin**: the MCP command is `iris-agentic-dev`, not `iris-dev`, and the empty `IRIS_*` env
  block is gone. The plugin registers 15 skills with Claude Code, not 3; see **Skill tiers**
  under Skills.
- **`skill install`** with no arguments installs the 10 core skills, not all 34. `--all` gives the
  old set minus `opencode-introspect`. Nothing already installed is deleted; `--prune` removes
  managed copies of skills outside the tier you installed. See **Skill tiers** under Skills.
- **`skill_list` / `skill_search`** leave out `opencode-introspect` and add a `tier` field to each
  bundled entry. `skill_describe` still finds it by name.
- **Skills**: `aihub-eap` is removed. Use `iris-ai-hub`, or install upstream's skill with
  `skill_community`.

**Full changelog**: <https://github.com/intersystems-community/iris-agentic-dev/compare/v1.4.2...v1.5.0>

## v1.4.2

Through v1.4.1, `iris_ws_exec` ran none of the gates. A WebSocket session could delete a class or
kill a global, both of which `iris_execute` refuses. The only thing stopping it was
`write_tools_enabled`, which nobody has to set — iad infers it true unless the instance is in `Live`
mode or its namespace is named like production.

New in this release: `web_prefix`, so you can register an instance served under a URL path.

### Notable fixes

#### The WebSocket terminal ran no gates

`iris_ws_exec` reached the session pool without calling `dispatch_gate` at all, so a terminal
session accepted every payload the code-edit hard block exists to refuse:
`Do $system.OBJ.Delete("MyApp.Foo")`, a write to `^oddDEF`, `Kill ^SomeGlobal`. All three are
refused on `iris_execute`.

The write tier was the only thing covering the tool, and `write_tools_enabled` is inferred rather
than defaulted: `Live` system mode or a namespace named like PROD infers it off, a development or
test namespace infers it on. So on a normal development instance the whole policy surface was
reachable through a session, with nobody having declared the flag. Reported by @devecchijr
([#137](https://github.com/intersystems-community/iris-agentic-dev/issues/137)), reproducible on
v1.3.0 and v1.4.1.

Two independent gaps, either one enough on its own:

- The handler called no gate. The gate preamble that had been written inline in `iris_execute` is
  now `arbitrary_exec_gate()`, and both handlers call it, so the next tool cannot end up with a
  partial copy of it. `iris_ws_exec` takes a `confirmed` parameter for the role gate now, the same
  as `iris_execute`.
- `dispatch_gate` step `[0]` branched on three literal tool names, so a handler that did call the
  gate would still have been waved through. The tool set is a declared constant
  (`CODE_EXEC_TOOLS`), and a unit test walks `IrisTools::registered_tool_names()` and fails if any
  tool with a `code` parameter is missing from it. That test is what did not exist before:
  `iris_ws_exec` was the fourth tool to ship past this gate, after `iris_global` set/kill,
  `iris_lookup_manage` set/delete, and `iris_execute_method`.

The check runs before exec, so a refused call sends nothing to IRIS and the session's variables are
exactly as they were. A live-container test asserts that second part.

#### Prefixed instances probed the gateway root

`iris_test_server` and `iris_servers(probe: true)` built their own URL from host and port, so an
instance served under a path came back healthy on the strength of the gateway root answering while
every real call went somewhere else. Both now probe the pool connection's own `base_url`.

`iris_servers` reports each entry's `web_prefix` where there is one. Two instances behind one
gateway share a host and port, so the old listing gave you no way to tell them apart.

#### Two documented commands that never parsed

The CLI dispatch section showed `iris-agentic-dev tool iris_query '{"sql":…}'`. The JSON goes in
`--args`, and the tool name is the only positional, so that line has exited 2 with
`unexpected argument` in every release since v1.2.0. The Commands table had it wrong the same way,
as `tool <name> <json>`. It is the first CLI example in the file, which makes it the first thing
someone copies.

A test now pulls every `iris-agentic-dev …` line out of the code fences in README and `docs/`, and
runs each one through the real parser, so a documented invocation that does not parse fails the
suite. Widening it past README caught a second one:
`docs/troubleshooting.md` had `doc get %Dictionary.ClassDefinition --namespace %SYS`, and `get` is
its own subcommand, so the connection flags have to precede it — `doc --namespace %SYS get …`.

README also points at `tool --list` and `tool <name> --schema` where it teaches `--args`, since
neither needs a connection, and the Copilot section shows an `objectscript.conn` block for people
who have VS Code but no IRIS connection in it yet.

### What's new

#### `web_prefix` in the native registry

`iris_add_server` takes a `web_prefix` parameter and `servers.json` stores it. An instance published
under a path — `http://gateway:8080/hs20261/api/atelier/`, the usual HealthShare layout — could not
be described in the registry at all, so registering one produced an entry pointing at the gateway
root. VS Code Server Manager profiles already carried the prefix as `webServer.pathPrefix`, which
iad read; the native registry was the gap.

```json
{
  "version": 1,
  "servers": {
    "hs-prod": {
      "host": "gateway",
      "port": 8080,
      "scheme": "http",
      "web_prefix": "/hs20261",
      "namespace": "HSCUSTOM",
      "username": "_SYSTEM"
    }
  }
}
```

`pathPrefix` is accepted as an alias on the JSON key, so a prefix copied out of a Server Manager
profile loads unchanged, and `iris_import_servers` carries the prefix across. Slashes are optional and
multi-segment prefixes work. Re-registering an existing name updates it in place and keeps the
stored password, so you can add a forgotten prefix without re-entering the credential. A prefix
carrying a scheme or a host is refused at registration rather than becoming a connection error
later. Reported by @isc-ndittber
([#129](https://github.com/intersystems-community/iris-agentic-dev/issues/129)).

### Breaking changes

None. `web_prefix` is new and optional, and an entry without one resolves exactly as it did in
1.4.1.

### For contributors

The nightly skill-regression suite is running again, and its numbers now mean something. A scoring
call that could not reach the model was recorded as `score: 0` — a failing grade rather than no
grade — so six of nine skills reported 0% against 0%, the run passed, and $3.70 a night bought a
table of zeros. Nine fabricated entries went into the committed baseline that way in August.

An unreachable scorer now returns `scored: false` with no score, pass rates are computed over
scored items only, and a skill with nothing scored prints `not comparable` instead of a number. A
preflight makes one real scoring call before the first billable session, so a missing credential
costs a cent and exits 2 rather than spending an hour. The baseline is schema 2: every entry records
the task set, scoring mode, scorer model and tool surface it was measured under, and a Δ is refused
when those differ. Writes merge by skill now. The old writer rebuilt the file from whatever it was
handed, so a one-skill `--update-baseline` deleted the other eight entries.

Separately, `tests/e2e/isolated_env.py` hard-coded `/opt/homebrew/bin/iris-agentic-dev`, which does
not exist on a GitHub runner, so the nightly had been evaluating skills with no iad tools in the
loop at all. I rebaselined all nine skills on the repaired harness, with 170 of 170 items scored.

**Full changelog**: <https://github.com/intersystems-community/iris-agentic-dev/compare/v1.4.1...v1.4.2>

## v1.4.1

A bug-fix release: three issues reported against 1.3.x and 1.4.0, all confirmed against the source.

### Notable fixes

#### The CodeMode write gate fired on comments and string literals

The gate that refuses a compile of `CodeMode = objectgenerator` without an explicit
`load-source` grepped the whole document for that text. So a class whose doc comment warned
against generators, or a method that returned the phrase as a string, was refused. That is source
which compiled fine before the gate existed
([#135](https://github.com/intersystems-community/iris-agentic-dev/issues/135)).

The scan now blanks comments and string literals before looking, and requires the keyword to sit
inside UDL brackets, where a real class-member declaration lives. Real generators are still gated.
When the gate does fire it reports the line number and the line text, so you can see what tripped
it instead of searching 400 lines for the match.

#### HTTPS against a locally-installed CA failed

`reqwest` was built with bundled webpki roots and never read the OS trust store. A gateway serving
a certificate from a CA you installed yourself — mkcert, a corporate internal CA — failed the
handshake while `curl` on the same machine accepted it
([#127](https://github.com/intersystems-community/iris-agentic-dev/issues/127)). The websocket
client in the same binary already used native roots, so it trusted certificates the Atelier client
refused. Both now read the OS trust store.

The same issue turned up a second defect nobody had reported. `IRIS_INSECURE` and `IRIS_TLS_VERIFY`
were resolved in four separate places, and two of them read only `IRIS_INSECURE`. Setting
`IRIS_TLS_VERIFY=false` gave you working single `iris_doc` gets, failing batch gets, and failing
websockets: the same setting honoured or ignored depending on which code path served the request.
That variable is what the VS Code extension sets for `iris-agentic-dev.tlsVerify` and for VS Code's
own `http.proxyStrictSSL`, so the extension's TLS switch was two-thirds wired. All four sites now
call one resolver.

New in this release: `tls_verify` in `.iris-agentic-dev.toml`, so a project with a self-signed
gateway can say so once instead of exporting a variable.

```toml
host = "iris.internal"
web_port = 443
scheme = "https"
tls_verify = false
```

`check_config` now reports the resolved state as `tls_verify`. It makes no network calls, so it was
the one place you could not see that validation was off, and a bad certificate never complained.

Precedence, since there are now three ways to say it: the config file beats `IRIS_TLS_VERIFY`,
`IRIS_INSECURE=true` beats both, and anything unrecognised means validate. A typo fails closed with
a handshake error rather than quietly skipping the check.

#### Docs: using the binary outside VS Code

Someone installed the VSIX into Antigravity, watched the extension activate, and had no way to reach
the tools ([#118](https://github.com/intersystems-community/iris-agentic-dev/issues/118)). The docs
only covered the VS Code extension and Claude Code. `docs/connecting.md` now has a section for any
MCP client: the `mcpServers` shape, where the extension parks the binary it downloaded (a
version-named subdirectory under the editor's `globalStorage`, which forks rename), and a check that
runs before any client is involved.

### Breaking changes

None. `tls_verify` is new and optional; leaving it out validates, as before.

**Full changelog**: <https://github.com/intersystems-community/iris-agentic-dev/compare/v1.4.0...v1.4.1>

## v1.4.0

### What's new

#### Every tool declares its parameters

Thirty-one of the 81 tools took an untyped catch-all called `AnyParams`. To an MCP client they
advertised `{"type": "object"}`: no properties, no types, no `required`, no enums. Between them
those tools read 132 parameter slots that existed only as English prose in the tool description.

Two things followed from that, and both had shipped. An agent had to read prose to find a
parameter name. And a misspelled or invented key was silently ignored instead of rejected:
`stream_inspect` was documented with a `max_chars` that no code read, so a caller asking
for 10,000 characters got the whole stream and no explanation.

All 81 tools now have a real params struct. `iris_admin` declares its 14 properties,
`iris_interop_query` its 15. Closed value sets are enums, so a client can validate `mode=statuz`
before the call instead of after. Every struct carries `deny_unknown_fields`, so all 81 advertise
`additionalProperties: false` and an unknown key comes back as `UNKNOWN_PARAMETER` from one
validation site rather than vanishing.

Runtime answers did not move. This is a declaration change: anything accepted before is still
accepted.

`AnyParams` is deleted, and a new `undeclared-params` gate fails the build if tool 82 tries to
reintroduce the pattern.

#### Tool discovery without a connection

`iris-agentic-dev tool --list` prints every tool name with a one-sentence summary.
`iris-agentic-dev tool <name> --schema` prints one tool's full description and `inputSchema`. Add
`--json` for a single JSON document. Both read the same router the MCP transport uses, and neither
opens a connection to IRIS, so you can see what exists before deciding whether to start a
container.

The whole surface is 7,648 bytes through `tool --list` against 106,658 bytes for an MCP
`tools/list`. A shell-based harness can discover every tool for 7% of the context the full listing
costs.

#### SystemPerformance profile management

`iris_system_performance` could start a run and poll it. You could not see which profiles existed,
define one, or find the HTML a finished run wrote. Five modes close that:

- `list_profiles`: name, description, sample interval, sample count, and the run duration those two
  imply
- `add_profile` / `delete_profile`: create and remove profile definitions
- `list_runs`: completed runs, newest first, capped at 100
- `report`: resolves a run to its `.html` on disk, defaulting to the newest run

`$addprofile^SystemPerformance` has a trap if you call it directly. It silently strips characters
outside `[A-Za-z0-9_]` and still returns success. Ask for a profile named `bad name` and it reports
1 and stores `badname`. Nothing on the IRIS side tells you the
rename happened, so `add_profile` validates the name first and then checks that the node it reads
back is the one you asked for.

Two other things measured against a live instance rather than assumed, because
`SystemPerformance.INT` is not an Atelier document: the report file name uses `$ZU(110)`, not
`%SYS.System.GetNodeName()`, which is uppercase and does not match what the routine writes; and
when the constructed path is missing, the directory is scanned for `*_<runid>.html` instead of
reporting nothing found.

### Notable fixes

#### find_subclass_implementations returned nothing for every input

Every call returned no results, regardless of class or method. Two defects plus an intermittent
failure, all now covered by tests.

#### iris_interop_query returned the wrong Event Log severity

The `log_type` filter sat one place off the `Ens.Util.Log.Type` valuelist, so every severity came
back as its neighbour: `error` selected Warning rows, `warning` selected Error rows, `info` selected
Assert. Nothing mapped to `assert` or `trace` at all, so those two were unreachable. The default asks
for `error,warning` and gets both rows whichever way round the two codes are, which is why nobody
tripped over it. A name that is not one of the six now comes back under `unknown_log_types` instead
of contributing no filter and quietly widening the query to every severity.

#### Nightly skill regression gate stopped gating

The workflow ran all skills in one job and hit the runner time limit, so it reported cancelled
rather than failed and nobody noticed. It now shards per skill.

#### Dependency updates

Six Dependabot alerts closed on the VS Code extension's lockfile: four high-severity on `fast-uri`
(override raised to 4.1.3) and two medium on `qs` (bumped to 6.16.0). No open alerts remain.

### Breaking changes

None. Parameter names, semantics, and return shapes are unchanged. Tools now reject unknown
parameters instead of ignoring them, which is a behavior change only for callers that were sending
keys no code read. Those calls were already not doing what they appeared to do.

**Full changelog**: <https://github.com/intersystems-community/iris-agentic-dev/compare/v1.3.2...v1.4.0>

## v1.3.2

### What's new

#### pbuttons collection works end to end

`iris_system_performance` shipped in 1.3.0 and could not start a run. Three defects sat between
the tool and IRIS's `SystemPerformance` routine, and each one hid the next.

The system blocklist pattern `^IRIS.Sys*` prefix-matched `^IRIS.SystemPerformance`, so
`mode=last_runid` came back as `CODE_EDIT_BLOCKED`. That global holds pbuttons run history and
profile definitions, which is diagnostic data, not code storage. The pattern is now
`^IRIS.Sys.*`, and `^IRIS.Sys.Config` writes are still blocked.

The code-edit gate then read `$$run^SystemPerformance("test")` as a reference to a global named
`^SystemPerformance`, which `^SYS*` blocks. Carets do double duty in ObjectScript, and the gate
only knew the global form. It now recognizes routine references: `label^routine`,
`$$label^routine`, and a bare `^routine` as the argument of `Do`, `Goto`, or `Job`. Writes to
code globals sitting next to a routine call are still caught.

Finally, `run^SystemPerformance` and `waittime^SystemPerformance` switch the current device. The
generator captures output by pointing `$IO` at a temp file, so the `Write` after the call landed
nowhere and the tool returned an empty result with a leftover
`<NAMESPACE>GetCurrentSrcLine+10^%SYS.ProcessQuery.1` in `$ZError`. The run had started fine;
nothing came back to say so. Both call sites now snapshot `$IO` and re-select it afterward.

While fixing that, `mode=start` gained a `profile` parameter (`test`, the 5-minute default, plus
`30mins`, `4hours`, `8hours`, `12hours`, `24hours`) and rejects anything that isn't letters,
digits, or underscore, so a profile name cannot break out of the generated ObjectScript literal.
`mode=status` validates `run_id` the same way. `mode=last_runid` now reads
`^IRIS.SystemPerformance("run")` before `("history")` and returns `in_progress: true` when the
run is still collecting, because an in-flight run has an ID but no report yet.

Against the 2026.2 community container: `mode=start` returns `20260904_161604_test`,
`mode=status` returns `"7 minutes"`, and the report lands at
`<mgr>/<host>_<instance>_<run_id>.html`.

#### IRIS errors reported as success

`execute_via_generator` emits four different failure prefixes. `ERROR: <exception>` comes from its
`Catch` block, `ERROR($ZERROR): <code>` covers the case where the body wrote nothing but left
`$ZError` set, `ERROR($DEVICE): <code>` covers a callee that moved the current device, and
tool-generated ObjectScript writes `ERROR:<sentinel>` with no space after the colon.

Fourteen call sites across five files hand-rolled `starts_with("ERROR:")`. That catches two of the
four and is blind to both parenthesized shapes, so an IRIS error came back as a successful result
with the error text sitting in the payload. `dict.rs` parsed the error text as JSON and returned
`[]`; `admin.rs` reported `flag_cleared: true` for a password change that never ran. All fourteen
now call `generator_error_message` in `iris/connection.rs`, which returns the message with whichever
prefix matched removed. It returns the text rather than a bool on purpose: a caller needs the text
for its own error code, and if getting it means redoing the prefix work by hand then the shared
check buys nothing. Unit tests pin each shape, including the space-less sentinel that a naive
tightening of the prefix would break.

#### Six holes in the security gates

Chasing the empty-success bug meant reading every gate, and the gates had the same problem in a
worse place. Five ways past them, one thing they wrongly refused:

`iris_execute_method` never reached the code-edit gate at all. The gate keys off a `code`
parameter, and that tool has a `class` and a `method` instead, so
`class="%SYSTEM.OBJ", method="Delete"` deleted a class with nothing recorded. The gate now
reassembles the call as `##class(C).M()` and runs it through the same check.

The dotted token list matched `%SYSTEM.OBJ` but not `##class(%SYSTEM.OBJ)` or
`$classmethod("%SYSTEM.OBJ","Compile",…)`, and `check_sql_code_edit` compared raw uppercased text,
so `%Dictionary . ClassDefinition` and `"%Dictionary"."ClassDefinition"` reached the dictionary
ungated. Both now flatten punctuation before matching, which makes one token list cover every call
form instead of one.

`iris_query(mode="read", force=true)` skipped `validate_read_only_sql` because of the `force`, and
skipped the SQL code-edit gate because that only looked at `mode="write"`, so `force=true` was the
way to send DML at the code dictionary with neither check running. The gate is now keyed on
`is_write || forced`.

A connection with no `[policy.<server>]` section skipped gates [1] through [4] entirely, which made
the "non-configurable" system blocklist and the bulk-PHI hard block opt-in. Unconfigured
connections now get `ConnectionPolicy::default()`, whose own field defaults are the strict ones.

And the false refusal: `^SYS*` in the system blocklist matched `^SYSCONFIG`, `^SYSDATA`,
`^SYSTOTALS` — any application global starting with those three letters. The blocklist has no
bypass, so each of those was a permanent, unexplainable refusal. Enumerating `^$GLOBAL` across
`%SYS`, `USER`, `IRISLIB` and `HSLIB` on 2026.2 finds exactly one global in that space, so the
pattern is now `^SYS` and `^SYS.*`.

Separately, no gate set `success: false` in its rejection JSON. Every other error path in the
server does, and a caller that branches on `success` reads a missing field as "not false",
which is to say a pass. All five gates set it now, with a test that walks one rejection per gate and
asserts the shape.

#### The bulk-PHI gate had been dead since it was written

`BULK_PHI_TOOLS` listed `view_message_body`. The tool is `iris_message_body`. The gate matched
against the name the router dispatches on, so it never fired for the one tool it most needed to
cover, and two tests agreed with the constant because they compared it to a literal list in the
test rather than to the router's registry. There is now a test asserting every entry in
`BULK_PHI_TOOLS` appears in `tools::write_gate::CLASSIFICATION`.

Fixing the name surfaced a second thing: `iris_message_body` was reading `dataPolicy` from the
caller's own parameters. Passing `dataPolicy: "allow"` unlocked it. The policy comes from
`[policy.<server>]` and nothing else now.

`iris_admin` had the same hole and a bigger one behind it. The policy gates are applied per
handler, and this handler never called `dispatch_gate`, so gates [1] through [4] were absent for
every admin action. `action="journal_search"` runs the same journal reader as the standalone tool
and took its effective policy from the caller's `dataPolicy` parameter, which is a caller
authorizing itself to read PHI in bulk. `iris_admin` calls the gate now, gate [2] matches on the
`action` value as well as the tool name so routing through a dispatcher is not a way around it, and
`journal_search` and `view_processes` read the policy from `[policy.<server>]`.

#### iris_table_info invented global names

The DDL branch derives `^Schema.TableD`, `^Schema.TableI` and `^Schema.TableC` from the table name
alone. It ran whenever the lookup output was not `NOT_FOUND` and had no `CLASS:` line, which
included an IRIS error, and included empty output. So a failed lookup came back as
`success: true` with three globals that may not exist. The handler now rejects generator errors,
empty output, and output it cannot parse, and only takes the DDL branch on an explicit `DDL_TABLE`.

#### The source-control probe failed open

Before writing a document, `iris_doc` asks IRIS whether source-control hooks are configured and
whether the document is checked out. When that probe could not be answered, the old code wrote
anyway. A write that bypasses a source-control hook is the failure mode the probe exists to
prevent, so an unanswerable probe now returns `SCM_PROBE_FAILED`. A document already held via the
checkout cache still takes the documented fast path and skips the probe.

#### Four more tools whose output went to the wrong device

`SystemPerformance` was not the only routine that moves `$IO`. `Config.Namespaces.Create` and
`.Delete` open the CPF, and `Ens.Director.UpdateProduction` re-jobs the production's worker jobs.
In all four cases the `Write` that reports the verdict ran after the device had moved, so
`iris_namespace_create`, the matching delete, and both `iris_production_item` branches that call
`UpdateProduction` returned `INTEROP_ERROR` with an empty message for work that had completed.
Each one now snapshots `$IO` before the call and re-selects it before writing anything.

#### Binary-invocation tests that could not find the binary

Six test files in `iris-agentic-dev-bin` read `IAD_BINARY` with a bare `std::env::var` and handed
the result straight to `Command::new`. Every doc comment in the repo tells you to pass
`./target/debug/iris-agentic-dev`, and a workspace member's test binary runs with the _member_
directory as its working directory, so the relative path resolved to
`crates/iris-agentic-dev-bin/target/debug/iris-agentic-dev`, which does not exist. All four tests
in `binary_098_server_probe` failed with "binary not found" against a binary sitting in
`target/debug`. Three more sites in the core crate's live tests had the same shape but reported the
miss by skipping.

`core::testing::iad_binary_path` already resolved relative values against the workspace root. All
nine sites call it now, and there is a gate that fails on a tenth.

#### The coverage gate had not run since 1.3.1

`.cargo/config.toml` sets `build.rustc-wrapper` to keep the global sccache setting out of this repo,
and it was set to `""`. Cargo reads an empty wrapper as no wrapper, so `cargo build` and
`cargo test` were unaffected. cargo-llvm-cov reads the key itself, does not apply that special
case, and builds the command `" " + rustc`, so every invocation died before compiling anything:

```text
error: could not execute process ` /Users/…/bin/rustc --print cfg` (never executed)
```

Running `scripts/coverage.sh` produced that line, a "coverage will be partial" warning, and no
report.

The gate had been unrunnable since a720d2f and nothing said so, which makes it the same shape as
everything else in these notes. The key names `/usr/bin/env` now — a passthrough that is a real
executable — and the eleventh detector, `empty-config-value`, fails on a cargo config key set to
the empty string.

#### The coverage number was wrong once the gate could run

With the gate running again it reported 75.64% overall against a floor of 88, and nine files below
their per-file floors. Nearly all of them were files this release had touched, so the reading was
plausible: new gate code, error paths, no tests. The floors exist to catch exactly that, and the
obvious next move was to write the missing tests or lower the floors.

Both would have been wrong. `policy/data_policy_gate.rs` read 50.00% — the same number for
regions, functions, and lines, on a 92-line file with 18 passing tests against its two functions.
Exactly half, three ways, is not what a coverage gap looks like. The function records said why:
four entries for two functions, three of them never executed, each carrying a different crate
disambiguator. llvm-cov reports on every object file under `llvm-cov-target`, not the ones the
current run built, and a test binary left from an earlier build carries its own instrumented copy
of the library that nothing executes. Every source file was being counted once covered and up to
three times dark.

`cargo llvm-cov clean --workspace` first, and the same file reads 98.04%. Overall reads 87.68%,
four files are below floor instead of nine, and the drops are 2 to 9 points rather than up to 46.
Both `scripts/coverage.sh` and `scripts/check-coverage-floors.sh` clean before measuring now, with
`COVERAGE_NO_CLEAN=1` for local iteration and nothing else.

A gate that answers from whatever is lying in the target directory is worse than an absent one. It
produces a number, the number looks like a finding, and acting on that finding means lowering the
floor that would have caught a real regression. The twelfth detector, `stale-coverage-objects`,
fails on a coverage script that generates a report without cleaning first.

Two things fell out of trusting the numbers again. Coverage of a file that keeps its tests inline
in `#[cfg(test)] mod tests` is capped below 100% by its own assertion messages, which only execute
when a test fails — so adding an inline test lowers the measured coverage of the code it tests.
`policy/patterns.rs` had no uncovered production lines at all; its 96.97% was entirely
assert-message lines. Its tests moved to `tests/unit/test_policy_patterns.rs` and it reads 100.00%
now. `policy/code_edit_gate.rs` was the same shape and went the same way, behind a
`#[cfg(feature = "testing")]` accessor so the moved tests can still reach `extract_globals`; it
reads 99.48%. New tests go in `tests/unit/` from here on. And the real gaps, once visible, were
small: the `%Dictionary` match that only appears after punctuation flattening, the `$$d ^ROUTINE`
case where a word merely ends with a command name, `Kill:1^Foo`, and the unquoted-brace scanner,
which had no test at all. Fifteen tests in `tests/unit/`, and one unreachable branch in
`contains_global_kill` deleted because no test could enter it.

Clearing the per-file floors left overall at 87.90% against a floor of 88, roughly thirty covered
lines short, so forty-six more tests went in where the uncovered blocks were biggest:

- `test_testing_helpers.rs` (9) — `iad_binary_path`, `require_iad_binary`, `clean_command`. Nine
  call sites depend on these and nothing asserted any of them, which is how the relative-path bug
  above survived.
- `test_workspace_config_paths.rs` (11) — `docker_only`/`nopws` with no `container` (the a720d2f
  routing), the legacy `.iris-dev.toml` name, a malformed config, and `OBJECTSCRIPT_WORKSPACE`
  outranking the path argument. Each config is parsed from TOML text, not built as a struct
  literal.
- `test_llm_usage.rs` (8) — `LlmClient::complete_with_usage`, the twin of `complete` that the
  benchmark harness actually calls and that carries the token accounting every per-task budget is
  enforced against. A wrong field name in `usage` would have read as every task costing zero
  tokens. The wiremock server here stands in for the Anthropic and OpenAI HTTP APIs; nothing about
  IRIS is mocked.
- `test_cli_dispatch_helpers.rs` (14) — `iris_dev_bin()`, `parse_tool_invocations`,
  `extract_sentinel_class`, `run_tool_subprocess`. A parser that drops a malformed call shows up as
  "the model did not call any tools".
- `test_probe_server_offline.rs` (4) — nothing listening, an HTTP error status, a 401, and the
  5-second timeout branch, that last one driven by a listener that accepts and then says nothing.
  It costs 5 seconds of wall clock; faking the clock means pausing the timer reqwest also uses, and
  then the two timeouts race.

Overall is 88.24% and all 63 files with coverage data meet their floors.

Two of those floors were the wrong number for the job that enforces them. `coverage-floors.toml`
says per-file floors are the unit-only baseline minus 2pp, and the `coverage-floors` CI job runs
unit-only with no container, but `src/tools/admin.rs` was registered at 81 and
`src/tools/server_tools.rs` at 90 — both merged-mode readings. Unit-only they measure 71.21% and
85.61%, so the job failed on them, and no test can close the gap without IRIS: admin.rs is mostly
handler bodies that talk to a live instance, and what is left of server_tools.rs is the probe path
that needs a server answering 200. Both entries now carry their unit-only numbers, 69 and 83, and a
comment naming where the higher figure comes from. Merged mode reads them at 82.84% and 91.72%, so
the coverage those floors were protecting is still there — it was the mode that was wrong, not the
tests. This one only surfaced because the gate started running again.

The antipattern gate flagged three of those new tests. Two were its own fault and are fixed in the
detectors rather than the baseline: `#[should_panic]` counts as an assertion now, and the
`IAD_BINARY` resolver's own test file is exempt from `binary-path` alongside `testing.rs`, because
testing a path resolver means writing the raw path it exists to fix. The third was real — a test
that returned early when the instrumented build was absent, so it asserted nothing on an ordinary
run. It computes the expected path and always asserts now.

A fourth false positive came from these notes: `tool-name-refs` read the Rust function
`iris_dev_bin()` as a tool an agent could call. An `iris_*` identifier followed by empty parens is
code now — no agent calls a tool with no arguments — which also retired two baseline entries,
`iris_arc()` in `output_schemas.rs` and a Python helper quoted in the pgwire skill.

One thing the audit found and this release does not fix: the floors themselves are measured against
files that still contain inline test modules, so test bodies count as covered production lines.
Production-only coverage is about 4 points lower than the reported number. Fixing the measurement
invalidates every floor in `coverage-floors.toml` at once, which is not a release-week change. It
is written up in `docs/backlog-empty-success-audit.md`.

#### CODE_EDIT_BLOCKED says what it actually blocked

The gate matches code-storage globals by name, which means it stops reads as well as writes. It
cannot tell them apart without parsing ObjectScript well enough to resolve indirection, and it
does not try. The old error message described the attempt as a code edit and sent the caller to
`iris_document`, a tool that does not exist. It now says reads are covered too, and the
remediation names `iris_doc` (`mode="get"`) and `iris_symbols` for reading alongside
`iris_doc` (`mode="put"`) plus `iris_compile` for writing. The gate itself remains
non-configurable, and the message says that as well.

#### Gate-refusal tests that were not testing the gate

`admin_e2e_tests` spawns the binary as a subprocess, and the child inherited
`IRIS_WRITE_TOOLS_ENABLED` and `IRIS_DESTRUCTIVE_TOOLS_ENABLED` from whatever shell ran the
tests. The CI e2e job sets both at job level, so every gate-refusal assertion in that file was
running against live IRIS with the gate open and passing for the wrong reason. The harness now
strips both variables and each test declares the gate state it means to exercise. Same fix
4254748 made for the `live_reload` tests.

#### Five NoPWS tests that never ran

The `nopws_101` binary and integration tests spawned the bare binary instead of
`iris-agentic-dev mcp`, so the child printed its usage banner and exited. The tests read that as
empty stdout. They only ever passed by taking the skip branch when no binary was found, which is
what happens with the default relative `IAD_BINARY` path and a test working directory of the crate
root. Point `IAD_BINARY` at a real binary and all five failed.

One of them had a second problem. `test_binary_iris_compile_has_execution_path_in_docker_only_mode`
depends on a config file to select docker_only mode, and inherited `IRIS_HOST`/`IRIS_WEB_PORT`
outrank the config, so the call went out over Atelier, where `iris_compile` has no
`execution_path` field. It now clears the connection variables and lets the config decide.

Once they ran, the CI e2e job found a third. All of them pinned `localhost:52780` and the container
name `iris-dev-iris`, which describe my laptop; the e2e container answers on 52773 and is called
`iris-e2e`. So `iris_execute` could not reach Atelier there at all, fell back to `docker exec`, and
`test_iris_execute_atelier_path_has_execution_path_field` reported the fallback as a broken Atelier
path. The endpoint comes from the environment now and defaults to the local container only when the
environment names nothing. The companion test that asserts the docker fallback was passing in CI
against a container name that does not exist on the runner, which says something separate about
what the docker path returns when `docker exec` has nothing to talk to — that one is open.

#### A gate per bug class

Every defect above was findable by grep once someone knew what to grep for. So each one now has a
detector in `scripts/gates/antipatterns.py`, twelve in total: `vacuous-tests`, `empty-tests`,
`mcp-subcommand`, `env-pinning`, `error-sentinels`, `device-capture`, `self-referential-gates`,
`version-consistency`, `tool-name-refs`, `binary-path`, `empty-config-value`,
`stale-coverage-objects`. Each message says which shipped bug it comes from, so a hit reads as
"this is how that happened" rather than a style complaint. CI runs the suite directly in
`gates.yml`, and the accept block in `specs/112-antipattern-gates/tasks.md` runs it at whichever
boundaries `.specify/gates/verify.sh` is wired into — the agent Stop hook and git pre-commit on a
checkout that has those hooks installed.

The six classes with no instances left — `error-sentinels`, `self-referential-gates`,
`version-consistency`, `binary-path`, `empty-config-value`, `stale-coverage-objects` — fail the
gate outright. The other six have a baseline of 222 known instances in
`scripts/gates/antipatterns-baseline.txt`, and the gate fails both on a finding that is not in the
file and on a line in the file that no longer fires. The list can only shrink.

It shrank by one on the way here. Editing `write_gate.rs` moved a baselined line, the gate flagged
it, and the flagged test turned out to be worth more than a re-anchored line number:
`test_contains_terminal_block_syntax_elseif_not_a_keyword` called
`contains_terminal_block_syntax("ElseIf x=1 { Write 1 }")`, discarded the result, and said in a
comment that it was documenting the behavior rather than asserting it. The behavior it declined
to assert was a false negative. `else` does not match `ElseIf` because `If` follows without a
separator, so a braced `ElseIf` was not block syntax as far as the gate was concerned. `elseif` is
in the keyword list now and the test asserts.

I narrowed two of them after they proved noisy. `device-capture` matched the bare
`##class(Ens.Director)` and flagged eight read-only call sites for every two real ones; it now
matches only the methods that start, stop, or re-job worker jobs. `tool-name-refs` reported
`iris_opt.map(…)` as an unknown tool because it only skipped a binding where it was declared, not
where it was used, and it flagged `..._impl` function names, which no agent can call. Six of its
seven findings were noise, and the seventh was real: a shipped tool description told agents to feed
`telemetry_export_trace` output to `iris_graph`, which exists only in a shelved spec.

`.specify/memory/constitution.md` gained a Bug Class Registry, one row per detector, so the write-up
and the check stay next to each other. `docs/postmortem-empty-success.md` has the analysis of how
the whole class got past review, and `docs/backlog-empty-success-audit.md` lists what is still open,
including the 84 `call_for_test_*_no_iris` tests that assert nothing beyond "the call returned" and
the baseline's line anchoring, which turns any edit that shifts a line number into a false finding.

#### VS Code extension binary version

`irisAgenticDev.serverVersion` in the extension's `package.json` was still `1.3.0` while the
workspace was at 1.3.1, so the extension would have installed the older binary. It is `1.3.2`
now, and the extension version moves to 0.4.31. The check that catches this drift lives in the
node test suite, which cargo does not run; the release workflow also compares `serverVersion`
against the tag before publishing.

#### Turning sccache off broke the Windows build

The coverage-gate fix above swapped `rustc-wrapper = ""` in `.cargo/config.toml` for
`/usr/bin/env`, a real executable that cargo-llvm-cov can put in front of rustc. It is also a path
Windows does not have. `cargo build --locked` on windows-latest quit before compiling anything, with
`could not execute process D:/usr/bin/env …rustc.exe -vV (never executed)`, so the
windows-handshake job went red for a reason that has nothing to do with the code it tests. No test
caught it, because the only machine that could was the one CI job that runs on Windows.

The key is out of the committed config. `scripts/coverage.sh` and
`scripts/check-coverage-floors.sh` export `CARGO_BUILD_RUSTC_WRAPPER=/usr/bin/env` instead: they
are bash, they only run on macOS and Linux, and they are the only things that need the passthrough.
`tests/unit/test_cargo_config.rs` fails if `build.rustc-wrapper` is a path again, and fails if
either script stops exporting one.

#### The prettier gate answered differently on every machine

Prettier searches upward from each file for a config and does not stop at the repo root. This repo
had none, so on my machine it found `~/ws/.prettierrc` with printWidth 100, and on a CI runner it
found nothing and used its own defaults. The gate measured two different things: ten files were
dirty locally and clean in CI, three the other way round. CI had been failing on prettier since
before 1.3.1, and the log showed only `Checking formatting...` because the runner truncates the
gate's output to its first line.

`.prettierrc.yaml` writes down prettier's defaults, which are what CI was already enforcing; the
point of the file is that the search now ends inside the repo. `.claude-plugin/plugin.json`,
`docs/connecting.md` and `skills/skills/iris-product-features/SKILL.md` are formatted to it.
`.prettierignore` also picked up `**/.pytest_cache/`: pytest writes that README and gitignores it,
so it only existed on machines that had run the Python tests, and only those machines saw the gate
fail on it.

## v1.2.7

### What's new

#### Agent attribution and opt-in audit emission (spec 086)

IRIS now sees who called it. Every IRIS-bound request carries a `User-Agent` header:

```text
iris-agentic-dev/1.2.7 (mcp; my-claude-agent)
```

The second token is `mcp` or `cli`, telling you whether the request came from an MCP client or the
CLI. The third token is the value of `IRIS_AGENT_LABEL`—set it to name a specific agent, model, or
deployment. The header is readable from ObjectScript as
`%request.CgiEnvs("HTTP_USER_AGENT")` and shows up in IIS/Apache/Web Gateway access logs, so the
Postman-vs-agent distinction the customer asked about works now.

Before this release, iad sent no `User-Agent` at all. The Web Gateway module (`CSPa24.so`) is
identical for every HTTP caller, so IRIS could not tell iad from curl from a human.

Enabling `RoutineChange` in `Security.Events` gives operators a tamper-resistant audit trail of
every `iris_execute` call, because every execution compiles a temp routine with a predictable
`IrisDevTmp.IrisDevRun<hash>` name. That record carries username and namespace and is written by
IRIS, not by iad. Add
[`irisAudit = true`](https://github.com/intersystems-community/iris-agentic-dev/blob/master/docs/connecting.md#irisaudit) to a `[policy.<server>]` block and iad will
also write a user-defined audit record per tool call—it carries the tool name, caller mode, agent
label, and MCP client identity when available. That record is self-reported; the RoutineChange
record is not. Both belong in the answer but they are not equal evidence, and the
[docs/agent-attribution.md](https://github.com/intersystems-community/iris-agentic-dev/blob/master/docs/agent-attribution.md) page says so plainly.

`docs/agent-attribution.md` is new. It covers what IRIS can and cannot see (measured, not inferred),
the exact `Security.Events` recipe for RoutineChange and SQL auditing, how to filter agent traffic
in access logs, the trust asymmetry between self-reported and IRIS-native records, and the honest
statement that distinct per-environment credentials are still the right tool for environment-level
agent restriction.

#### The write gate now enforces what the config declares (spec 085)

A reporter walked the released 1.2.6 binary against a live IRIS instance with
`write_tools_enabled = false` and wrote to it anyway. Five separate defects, all reproducible:

1. `iris_ws_exec` had no gate. Open a session and it runs arbitrary ObjectScript, which bypasses the
   `iris_execute` gate entirely. So did `iris_global` set/kill, `iris_lookup_manage`,
   `iris_execute_method`, `iris_source_control`, `iris_production`, `iris_test`, the three
   generators, `kb_index`, and four `skill` actions. Only seven tools were ever gated.
2. Editing the toml from `true` to `false` did nothing for the life of the process. The gate was
   routed through the `IRIS_WRITE_TOOLS_ENABLED` env var and only exported when the var was unset,
   so the first value won forever while `check_config` kept reporting it and `config_loaded_at` kept
   advancing.
3. `destructive_tools_enabled = true` with `write_tools_enabled = false` failed **open**. It logged
   `DESTRUCTIVE_REQUIRES_WRITES`, returned early, never set the gate, fell through to the namespace
   heuristic, and started with writes enabled, the inverse of the declaration.
4. `IRIS_DESTRUCTIVE_TOOLS_ENABLED` was written and never read. `write_allowed_servers` was
   documented with its own error code and was not a field on the config struct at all.
5. Every 1.2.x release advertised `server_version 1.2.6+v1.2.6-dirty`, because the committed
   lockfile disagreed with the manifests and cargo reconciles it during resolution, before
   `build.rs` runs `git describe --dirty`.

Every tool now has a declared entry in one classification table, and the gate is resolved from the
config and checked once, in `call_tool`. A tool added without an entry fails CI, and so does an entry
whose tool was renamed. The router's `destructive_hint` annotations and the table are cross-checked
against each other, so a mislabelled tool has to be mislabelled twice. `check_config` reports
`write_tools_source` and `destructive_tools_source` (`config_file`, `operator_env`,
`inferred_namespace`), so what it reports and what the server enforces come from the same value.

Config edits change the gate in a running process now, in both directions. The contradictory
combination exits 2 instead of serving requests.

#### Breaking: the destructive tier defaults to off

Seven items need `destructive_tools_enabled = true` on top of `write_tools_enabled = true`:

- `global_kill`
- `iris_admin`
- `iris_credential_manage`
- `iris_lookup_manage` set, delete, clear, import (get, list_keys, list_tables and export stay
  read-only)
- `iris_namespace_create`
- `iris_remove_server`
- `skill(action="forget")`

The key was documented in 1.2.1 and had no reader, so anything relying on those seven was relying on
the absence of enforcement. Declaring the tier with writes off is a startup error now, not a silent
inversion.

Write-capable tools also stay in `tools/list` when writes are off and refuse at call time with
`WRITE_TOOLS_DISABLED`. Two of them, `iris_production_item` and `iris_credential_manage`, used to
vanish from the advertised list instead, which left an agent to infer why a documented tool did not
exist.

#### Removed from the docs

`write_allowed_servers`, `IRIS_WRITE_ALLOWED_SERVERS`, and `WRITE_SERVER_NOT_ALLOWED` are gone from
`docs/tools.md`, along with the six-step check order whose steps 2 and 3 described code that was
never written. `IRIS_WS_TIMEOUT_SECS` is gone from `docs/connecting.md` for the same reason. A
documented security control with no reader is worse than no control. It invites an operator to
believe writes are confined to one server when nothing confines them.

I still want per-server write policy. `specs/074-write-server-allowlist/` is the design of record,
the predicate has a marked seam in `write_gate.rs`, and it is open, not shipped.

#### Lockfile and version string

`--locked` is on every `cargo` invocation in CI and in the release workflow, and a unit test runs
`cargo metadata --locked` against the workspace. Drift fails the build instead of quietly renaming
the release. `check_config`'s own description tells operators to read `server_version` to tell an
official build from a fork, so this was not cosmetic.

`docs/tools.md`, `docs/connecting.md`, and the bundled `iris-agentic-dev` skill are checked in CI
against the code: every documented error code has to be emitted somewhere in `crates/*/src`, every
config key has to deserialize and be read, every env var has to be read rather than merely written,
and every documented tool parameter has to be reachable. That pass found 18 stale parameter names
beyond the four this feature set out to fix.

Thanks to the reporter who reproduced all five defects against released binaries and measured what
landed in IRIS rather than what error code came back. The tests assert that distinction now.

### Community contributions

#### Block compile-time code generation (PR #122)

`iris_doc` now refuses writes where `CodeMode` is `objectgenerator`, `expression`, or `call`.
Those three modes execute ObjectScript at compile time, not just at runtime, so they let a
document write sneak arbitrary execution through the compile path. The refusal returns
`COMPILE_TIME_EXEC_BLOCKED` with a message pointing to the safer `iris_execute` path.

#### Fleet config accepts a file path or a directory (PR #124)

`load_fleet_config` used to require the path to end in a directory. Passing the `.toml` file
directly now works. Useful when the fleet file lives outside the default search path.

#### Cursor IDE quickstart (PR #126)

`docs/cursor-quickstart.md` covers Cursor's agent mode, the `composer` interface, and
the MCP server wiring for both the desktop app and the CLI. Includes a working
`.cursor/mcp.json` snippet and a quick smoke test to confirm iad is connected.

#### Synthesized skills surface in `skill_list` (PR #120 — pending)

Skills stored in the IRIS `^Skills` global now appear in `skill_list` alongside bundled
skills. Previously only bundled skills were returned. This PR is still open pending a
small test-ordering fix.

## v1.2.6

### What's new

**Fix: synthesized skills (`^SKILLS` global) invisible to `skill_list` and `skill_search` (#119)**

`skill_list`, `skill_search`, and `skill_describe` merge two sources: skills bundled with
the binary, and skills synthesized into the IRIS `^SKILLS` global via `skill(action="propose")`
or `skill_community(action="install")`. In practice the synthesized side never worked — any
skill in `^SKILLS` was silently invisible, and the response incorrectly reported
`sources.synthesized.searched: false` even when IRIS was reachable.

Root cause: `synthesized_skills()` built its JSON by concatenating raw `^SKILLS` pipe-delimited
values directly into an array literal. The result was invalid JSON the moment any entry existed,
`serde_json::from_str` failed silently, and the function returned an empty list.

Fixed by switching to `%DynamicArray`/`%DynamicObject` — already an established idiom in this
codebase — so each entry is properly serialized as `{"name":...,"description":...,"body":...}`
regardless of what characters the skill contains.

Thanks to mwaseem75 for the thorough diagnosis in #119.

## v1.2.5

### What's new

**Fix: full MCP protocol `2026-07-28` support (#117)**

v1.2.4 capped protocol negotiation at `2025-11-25` as a workaround. This release
implements the actual requirement: the `tools/list` response now carries
`ttlMs: 0` and `cacheScope: "public"` as required by the `2026-07-28` spec (SEP-2549).
`ttlMs: 0` means the client should not cache the result — correct for tools that
query live IRIS state.

The server now negotiates `2026-07-28` when the client requests it.

**Fix: `check-sm-credential` crashes on Windows when VS Code stores credentials as TEXT**

VS Code stores Server Manager credentials as a BLOB on most installs but as TEXT on some.
The `check-sm-credential` command failed with "Invalid column type Text at index: 0" in
the TEXT case. Both column types are now handled.

The command also gains a `--db-path` flag to point at an arbitrary `state.vscdb` — used
by CI to verify the fix without a real VS Code installation.

## v1.2.4

### What's new

**Fix: tools become unavailable after Claude Code negotiates MCP protocol `2026-07-28` (#117)**

rmcp 3.1.3 includes `2026-07-28` in its known protocol version list. When a Claude Code
client (gated by the `tengu_mcp_protocol_negotiation_stdio` flag) requests that version
during `initialize`, the server was echoing it back. The `2026-07-28` draft requires every
tool in `tools/list` to carry `cache: {ttlMs, cacheScope}` metadata — fields rmcp doesn't
add automatically — so Claude Code rejected the entire tool list, leaving no tools available.

The server now caps protocol negotiation at `2025-11-25`. Clients requesting `2026-07-28`
fall back to `2025-11-25`; clients on older versions are unaffected.

## v1.2.3

Released 2026-08-21

### Thanks

- **[@rluman](https://github.com/rluman)** — reported that `IRIS_SERVER_NAME` fails on
  Windows with "No credential found in OS keychain" and confirmed the diagnosis that VS Code
  stores extension secrets as DPAPI-encrypted blobs inaccessible to external processes.

### New

- **`check-sm-credential` diagnostic subcommand**: On Windows, VS Code's `SecretStorage`
  stores Server Manager credentials in a local encrypted database (`state.vscdb`) rather
  than the Windows Credential Manager — so iris-agentic-dev can't read them through the
  normal keychain path. This new subcommand reads `state.vscdb` directly and decrypts via
  DPAPI (which works because DPAPI is per-user and the MCP binary runs as the same user
  as VS Code):

  ```text
  iris-agentic-dev check-sm-credential <server-name> <username>
  ```

  Prints `OK` with a password length, or a specific `FAIL` reason. On macOS/Linux the OS
  keychain works directly so this subcommand isn't needed there.

### Docs

- **`docs/connecting.md`**: Added a Windows limitation note to the Server Manager section
  explaining why zero-config doesn't work on Windows and pointing to the `.toml` workaround.

## v1.2.2

Released 2026-08-20

### Thanks

- **[@devecchijr](https://github.com/devecchijr)** — retested v1.2.1 and caught that
  `write_tools_enabled = false` was reporting correctly in `check_config` but not
  actually blocking writes
  ([#110](https://github.com/intersystems-community/iris-agentic-dev/issues/110)).

### Fixes

- **`write_tools_enabled = false` now blocks writes**
  ([#110](https://github.com/intersystems-community/iris-agentic-dev/issues/110)):
  v1.2.1 fixed the reporting half — `check_config` returned `false` when the config said
  so. The enforcement half was missing. `iris_doc` (put/delete/insert/delete_lines),
  `iris_compile`, `iris_execute`, and `iris_query` (write mode) now return
  `WRITE_TOOLS_DISABLED` before making any IRIS call when the flag is off.

### New

- **`--no-skills` / `IRIS_NO_SKILLS=true`**: pass this flag to `iris-agentic-dev mcp`
  to strip all 15 skill, KB, and learning-agent tools from `tools/list`. `--subscribe`
  fetching is also skipped. Useful when skills are installed as separate MCP servers and
  you don't want duplicate tool names in the client.

  ```bash
  iris-agentic-dev mcp --no-skills
  # or
  IRIS_NO_SKILLS=true iris-agentic-dev mcp
  ```

## v1.2.1

Released 2026-08-19

### Thanks

- **[@isc-ging](https://github.com/isc-ging)** (Gabriel Ing) — filed
  [#103](https://github.com/intersystems-community/iris-agentic-dev/issues/103)
  (`iris_production` namespace hardcoded to USER) and
  [#105](https://github.com/intersystems-community/iris-agentic-dev/issues/105)
  (Storage-stripping breaking agentic edits).

- **[@CraigRegester](https://github.com/CraigRegester)** (Craig Regester) — filed
  [#111](https://github.com/intersystems-community/iris-agentic-dev/issues/111)
  (`enabled_tools`/`disabled_tools` ignored when config is passed via `--config`) and
  [#113](https://github.com/intersystems-community/iris-agentic-dev/issues/113) (Cursor
  showed 0 tools due to `outputSchema` size).

### Fixes

- **`iris_production` now accepts a `namespace` parameter**
  ([#103](https://github.com/intersystems-community/iris-agentic-dev/issues/103)):
  previously all `iris_production` actions ran in USER regardless of the configured
  namespace. An explicit `namespace` argument now takes precedence; if omitted, the
  connection's configured namespace is used. This was the specific gap left after the
  general namespace fix in v1.2.0 (#96).

- **`iris_debug` works on HTTP-only connections**
  ([#98](https://github.com/intersystems-community/iris-agentic-dev/issues/98)):
  `map_int`, `capture`, and `source_map` returned `DOCKER_REQUIRED` on any connection
  without a container name, even though the underlying code runs fine over HTTP. All
  three now use `execute_via_generator` (the HTTP path).

- **Storage blocks are no longer stripped from document saves**
  ([#105](https://github.com/intersystems-community/iris-agentic-dev/issues/105)):
  the Atelier `PUT` path was removing `Storage` blocks before saving, which silently
  discarded class storage definitions on every agentic edit. Storage is now preserved.
  The `reset_globals` behavior (which resets global mappings to defaults) is now a
  separate opt-in flag, defaulting to false.

- **`--config` now honors `enabled_tools` and `disabled_tools`**
  ([#111](https://github.com/intersystems-community/iris-agentic-dev/issues/111)):
  tool allowlists and blocklists from a config file passed via `--config` were silently
  ignored at startup — the server registered all tools regardless. The config is now
  loaded before tool registration so the lists take effect.

- **Cursor shows tools correctly**
  ([#113](https://github.com/intersystems-community/iris-agentic-dev/issues/113)):
  `tools/list` responses included `outputSchema` for all 90 tools, producing a ~220 KB
  payload that exceeded Cursor's parser limit and caused it to see 0 tools. `outputSchema`
  is now stripped from list responses. It remains available in `tools/call` responses
  where clients that use it expect it.

- **`enabled_tools`/`disabled_tools` ignored when `IRIS_HOST` is set**: if you had
  `IRIS_HOST` in your environment and `enabled_tools` in your `.iris-agentic-dev.toml`,
  the tool list was silently ignored — the server registered all tools. The TOML's
  tool-list settings now apply regardless of how the connection is established.

- **Shared-gateway fleet configs disambiguate by namespace**
  ([#114](https://github.com/intersystems-community/iris-agentic-dev/issues/114)):
  two fleet instances sharing the same hostname (common in HTTPS gateway setups) were
  previously matched by host alone, so the first entry always won regardless of which
  namespace was active. Instance matching now checks namespace first; host-only matching
  is the fallback for instances without a `namespace` field, so existing configs are
  unaffected.

### Under the hood

#### Binary invocation tests

Seven new `#[ignore]` tests in `test_mcp_binary_config.rs` spawn the actual
`iris-agentic-dev` binary and drive it over stdio JSON-RPC. These catch wiring bugs
(flag exists but ignored, config field parsed but never applied) that unit tests and
integration tests both miss. CI builds the binary first and runs them via
`IAD_BINARY=./target/debug/iris-agentic-dev`.

[CLAUDE.md](https://github.com/intersystems-community/iris-agentic-dev/blob/master/CLAUDE.md) and [CONTRIBUTING.md](https://github.com/intersystems-community/iris-agentic-dev/blob/master/CONTRIBUTING.md) document this
as a required test layer for any new CLI flag or MCP wiring.

## v1.2.0

Released 2026-08-18

### Thanks

Most of the fixes in this release came from community bug reports. Thank you to:

- **[@PYDuquesnoy](https://github.com/PYDuquesnoy)** (Pierre Duquesnoy) — filed
  [#95](https://github.com/intersystems-community/iris-agentic-dev/issues/95) (`isError`
  never set), [#96](https://github.com/intersystems-community/iris-agentic-dev/issues/96)
  (namespace silently defaults to USER),
  [#97](https://github.com/intersystems-community/iris-agentic-dev/issues/97) (body
  content search missing), and
  [#99](https://github.com/intersystems-community/iris-agentic-dev/issues/99)
  (`%UnitTest.TestProduction` suites not running). Four sharp reports, all fixed.

- **[@isc-ging](https://github.com/isc-ging)** (Gabriel Ing) — filed
  [#104](https://github.com/intersystems-community/iris-agentic-dev/issues/104)
  (`.iris-agentic-dev.toml` not loaded on launch).

- **[@CraigRegester](https://github.com/CraigRegester)** (Craig Regester) — filed
  [#107](https://github.com/intersystems-community/iris-agentic-dev/issues/107)
  (Server Manager credentials unavailable in headless/remote environments).

- **[@devecchijr](https://github.com/devecchijr)** — filed
  [#94](https://github.com/intersystems-community/iris-agentic-dev/issues/94)
  (`read_only_hint` set on mutating tools).

- **[@PYDuquesnoy](https://github.com/PYDuquesnoy)** also submitted
  [PR #93](https://github.com/intersystems-community/iris-agentic-dev/pull/93)
  directly fixing the ObjectScript codegen string escaping bug ([#92](https://github.com/intersystems-community/iris-agentic-dev/issues/92)).

### Install / upgrade

**Homebrew install command changed.** The old raw-URL form no longer tracks releases.
Use the tap instead:

```bash
brew tap intersystems-community/tap
brew install iris-agentic-dev
```

If you installed with the old command, uninstall and reinstall via the tap to get
future updates automatically.

Direct download, Linux, and Windows installs are unchanged — see the
[README](https://github.com/intersystems-community/iris-agentic-dev/blob/master/README.md#install).

### What's new

#### CLI dispatch benchmark mode

`iris-agentic-dev benchmark` now runs in two modes: `mcp` (the existing session-based
mode) and `cli_dispatch` (new). CLI dispatch runs the same 22-task agentic loop but
drives IRIS entirely through the binary's subcommands, with no MCP session and no
running server. The model calls tools by emitting structured JSON, the harness routes
each call to the right `iris-agentic-dev <subcommand>`, and the result goes back as
text.

Baseline numbers for `cli_dispatch` on gpt-4o-mini: 81.8% pass rate (18/22 tasks),
16,167 tokens total over 74.7 seconds.

New flags: `--mode mcp|cli_dispatch`, `--max-iterations`, `--max-task-tokens`,
`--compare <baseline.json>`. The `--compare` flag prints a delta table showing pass rate
and token count differences between two runs.

All benchmark results now record per-task token counts (`tokens_input`, `tokens_output`,
`tokens_total`). Existing baseline JSON files are backward-compatible; the fields
default to `null` when absent.

See [BENCHMARKING.md](https://github.com/intersystems-community/iris-agentic-dev/blob/master/skills/BENCHMARKING.md) for full usage and flag reference.

#### Manifest `provides.tools`

The `[provides]` block in `iris-agentic-dev.toml` now accepts a `tools` list:

```toml
[provides]
tools = ["iris_query", "iris_search", "iris_compile"]
```

When the install command resolves a manifest, it validates every name against the full
tool registry. An unknown name is an error, not silently dropped. Fix the typo at
declaration time, not when someone else runs it.

It sets the same allowlist as `IRIS_ENABLED_TOOLS`, but in version control so every
developer on the project gets the same tool surface without copying environment
variables. See [ecosystem-integration.md — Pattern D](https://github.com/intersystems-community/iris-agentic-dev/blob/master/docs/ecosystem-integration.md#pattern-d-tool-subset-consumer)
and [connecting.md — Tool subset](https://github.com/intersystems-community/iris-agentic-dev/blob/master/docs/connecting.md#tool-subset-allowlist).

### Fixes

- **`iris_compile` FILE_NOT_FOUND**: when you pass a local file path that doesn't exist,
  `iris_compile` (and the `compile` shortcut) now returns `FILE_NOT_FOUND` immediately
  instead of forwarding the nonexistent path to Atelier and getting a vacuous success
  back. The error includes the path that was checked. See
  [troubleshooting.md](https://github.com/intersystems-community/iris-agentic-dev/blob/master/docs/troubleshooting.md#symptom-table).

- **`iris_interop_query` body search**
  ([#97](https://github.com/intersystems-community/iris-agentic-dev/issues/97)): message
  body content is now searchable, not just message headers and metadata.

- **`iris_test` with `%UnitTest.TestProduction`**
  ([#99](https://github.com/intersystems-community/iris-agentic-dev/issues/99)): test
  suites that inherit from `%UnitTest.TestProduction` now run correctly. They were
  previously miscategorized and skipped.

- **Linux keychain init**
  ([#107](https://github.com/intersystems-community/iris-agentic-dev/issues/107)):
  `iris-agentic-dev` no longer crashes on Linux hosts where the keychain backend is
  unavailable (headless CI, containers). It surfaces `keychain_unavailable` in
  `check_config` output and falls through to other credential sources.

- **`check_config` hot-reload**
  ([#104](https://github.com/intersystems-community/iris-agentic-dev/issues/104)): the
  initial-load trigger is now narrowed to `AutoDiscovered` connections with no explicit
  IRIS config, preventing spurious reloads when a `.iris-agentic-dev.toml` is already
  present at startup.

- **Namespace resolution**
  ([#96](https://github.com/intersystems-community/iris-agentic-dev/issues/96)): an
  omitted namespace now resolves to the namespace from the active connection, not a
  hardcoded `USER`. This matters when connecting to a non-USER namespace via Server
  Manager or environment variables. See
  [connecting.md](https://github.com/intersystems-community/iris-agentic-dev/blob/master/docs/connecting.md#connection-discovery-order).

- **`isError` and `read_only_hint`**
  ([#95](https://github.com/intersystems-community/iris-agentic-dev/issues/95),
  [#94](https://github.com/intersystems-community/iris-agentic-dev/issues/94)):
  `isError` is now set correctly on tool failures; `read_only_hint` annotations are
  removed from tools that modify IRIS state.

- **ObjectScript codegen string escaping**
  ([#92](https://github.com/intersystems-community/iris-agentic-dev/issues/92),
  [PR #93](https://github.com/intersystems-community/iris-agentic-dev/pull/93)):
  user strings embedded in generated ObjectScript are now escaped correctly. Previously
  certain characters caused the generated code to be syntactically invalid.

### Under the hood

#### rmcp 1.4 → 3.1.3

The MCP SDK upgrade brought several breaking API changes: `Content` renamed to
`ContentBlock`, `.raw` removed from content blocks, `schema_for_output` made
infallible, `ListToolsResult` constructor changed, `CallToolResponse` return type
updated. All callsites updated.

#### MCP output schemas (all 90 tools)

Every tool now declares a JSON Schema for its output. MCP clients that support output
schema validation (AI Hub, future Claude Code versions) can use these to constrain
responses and catch schema drift between server versions. See
[tools.md](https://github.com/intersystems-community/iris-agentic-dev/blob/master/docs/tools.md) for the full tool reference.

#### CLI tool parity

The `iris-agentic-dev tool <name> --args <json>` dispatch path now matches the MCP
server's tool routing exactly. Previously some tools available via MCP were silently
absent from CLI dispatch. See [troubleshooting.md — CLI commands](https://github.com/intersystems-community/iris-agentic-dev/blob/master/docs/troubleshooting.md#cli-commands).

#### Policy gates and tool categories

All 90 tools are now categorized (`compile`, `execute`, `query`, `search`, `docs`,
`source_control`, `debug`, `admin`, `skill`, `kb`). Policy gates (`allow = [...]` in
`[policy.<server>]` blocks) now enforce these categories correctly. Previously, 55
uncategorized tools bypassed policy and env-var gates entirely. See
[connecting.md — Per-connection policy](https://github.com/intersystems-community/iris-agentic-dev/blob/master/docs/connecting.md#per-connection-policy-fleet--operate-mode).
