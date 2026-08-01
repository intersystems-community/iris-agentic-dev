---
name: iris-objectscript-eval
description: Execute, compile, and test ObjectScript code via the objectscript MCP tools. Use when needing to run arbitrary ObjectScript, compile .cls files, or run %UnitTest tests. Prefers MCP tools over docker exec. Falls back to docker exec only when MCP is unavailable.
license: MIT
metadata:
  version: "2.0.0"
  author: Tim Leavitt (InterSystems)
  source: https://gitlab.iscinternal.com/tleavitt/isc-skills
  compatibility: objectscript, iris, mcp
---

# Evaluating ObjectScript via MCP Tools

## Preferred: MCP Tools (when objectscript MCP is connected)

Check with `/mcp` — if `objectscript` is listed, use these tools exclusively.

### Run arbitrary ObjectScript

```text
iris_execute(code="write $ZVERSION,!", namespace="USER")
iris_execute(code="set x=42\nwrite x,!")          # multiline: separate with \n
iris_execute(code="write ##class(My.Pkg).Run()")  # call class methods
```

Returns `{success, output, namespace}`. Runtime errors are returned as structured errors, not exceptions.

### Compile a .cls file

```text
iris_compile(target="MyPackage/MyClass.cls", namespace="USER")
iris_compile(target="*.cls")   # compile all .cls files in workspace
```

### Run %UnitTest tests

```text
iris_test(pattern="MyPackage.Tests.*")
iris_test(pattern="MyPackage.Tests.MyClassTest")   # single class
```

### Discover classes

```text
iris_symbols(query="MyPackage.*")       # live namespace search
iris_symbols_local()                    # parse .cls files on disk, no IRIS needed
docs_introspect(class_name="My.Class") # full method signatures
```

### Route a call to a different server

Pass `server:` to any tool to route that call to a named IRIS instance without changing
the default connection:

```text
iris_servers()                                  # list all registered instances
iris_execute(server="dev", code="Write $ZV")    # target a specific named server
iris_test_server(server="dev")                  # verify connectivity first
```

To switch the default connection to a different Docker container (legacy approach):

```text
iris_list_containers()                          # see running Docker containers
iris_select_container(name="my-iris-container") # reconnect to that container
```

---

## Fallback: docker exec (when MCP is unavailable)

Only use this path when the MCP is not connected.

Also use it when the build has no private web server. Enterprise `2026.2.0AI` builds shipped
without one (DPP-1192), so Atelier REST is unavailable and every tool that depends on it routes
through `docker exec` instead.

You usually do not have to configure this: iris-agentic-dev sniffs the version string for
`2026.2.0AI` and reports `atelier_rest: false` from `check_config` on its own. Set
`docker_only = true` in `.iris-agentic-dev.toml` when detection cannot help you — a NoPWS build
whose version string doesn't match that literal, or a container where you want to force the
`docker exec` path regardless. Run `check_config` first and believe what it reports.

### Wait for IRIS to be ready

IRIS takes 10-30s to start. Poll before running anything:

```bash
docker exec <container> /bin/bash -c \
  'for i in $(seq 1 30); do iris session IRIS -U USER "halt" 2>/dev/null && exit 0; sleep 2; done; exit 1'
```

### Execute ObjectScript non-interactively

```bash
# Single expression
docker exec <container> iris session IRIS -U USER \
  '##class(Sample.Calculator).Add(2, 3)'

# Multi-line via heredoc (always end with halt)
docker exec -i <container> iris session IRIS -U USER <<'EOF'
 do $System.OBJ.LoadDir("/home/irisowner/dev/cls/","ck",,1)
 halt
EOF
```

**Critical rules for heredoc:**

- End every script with `halt` or the session hangs
- Use `-i` not `-it`
- Indent each ObjectScript line with a leading space
- Pass `-U USER` (or `-U NAMESPACE`) to set the namespace

### Script file approach

Instead of a heredoc, write a `.script` file (one ObjectScript command per line,
each indented with a space, ending with `halt`) and run it:

```bash
docker exec <container> iris session IRIS -U USER /home/irisowner/dev/load.script
```

### Compile via docker exec

```bash
docker exec <container> iris session IRIS -U USER \
  'do $System.OBJ.Load("/home/irisowner/dev/cls/MyPackage/MyClass.cls","ck")'

# Load directory recursively (4th arg 1 = recursive; picks up .cls/.mac/.inc/.int)
docker exec -i <container> iris session IRIS -U USER <<'EOF'
 do $System.OBJ.LoadDir("/home/irisowner/dev/cls/","ck",,1)
 halt
EOF

# Restrict to one file type
docker exec <container> iris session IRIS -U USER \
  'do $System.OBJ.LoadDir("/home/irisowner/dev/cls/","ck","*.cls",1)'
```

Load/compile flags: `c` = compile, `k` = keep source, `e` = display errors only
(useful in CI).

### Run tests via docker exec

```bash
docker exec -i <container> iris session IRIS -U USER <<'EOF'
 set ^UnitTestRoot = "/home/irisowner/dev/cls/"
 do ##class(%UnitTest.Manager).RunTest("Test","/loadudl")
 halt
EOF
```

`^UnitTestRoot` must point to the **parent** of the test package directory. The
first `RunTest` argument is the subdirectory/package underneath it. `/loadudl`
tells the test manager to load the UDL-format `.cls` files from disk.

If the test classes are already compiled in IRIS, skip the load instead:

```bash
docker exec -i <container> iris session IRIS -U USER <<'EOF'
 do ##class(%UnitTest.Manager).RunTest("","/noload/run")
 halt
EOF
```

### Read test results programmatically

```bash
docker exec -i <container> iris session IRIS -U USER <<'EOF'
 set rs = ##class(%ResultSet).%New("%UnitTest.Result.TestAssert:Assertions")
 do rs.Execute("")
 while rs.Next() { write rs.Get("Name")," | ",rs.Get("Status"),! }
 halt
EOF
```

Note: the `while ... { }` block above only works from a `.script` file or as a
single-line expression — the interactive REPL cannot take block-structured code
line by line (see Common Mistakes).

### ZPM package loading — always compile afterward

After `zpm load` or `zpm install`, the package's classes may not be compiled.
Run `CompilePackage` explicitly:

```bash
docker exec -i <container> iris session IRIS -U USER <<'EOF'
 zpm "load /home/irisowner/dev"
 do $system.OBJ.CompilePackage("MyPackage","ck")
 halt
EOF
```

`zpm load` imports source files but does not guarantee recompilation when the
classes already exist with stale bytecode. Without the explicit `CompilePackage`,
class lookup succeeds but method dispatch can fail with `<NOROUTINE>` or return
stale results.

---

## Start a container (when none is running)

```python
# Via iris-devtester (preferred — handles password auto-remediation)
from iris_devtester import IRISContainer
with IRISContainer.community(version="2025.1").with_name("my-iris") as iris:
    conn = iris.get_connection()
```

```bash
# Via idt CLI
idt container up --name my-iris --image intersystemsdc/iris-community:2025.1
```

Attach to a container that is already running, and clear the "Password change
required" state a fresh container starts in:

```python
from iris_devtester import IRISContainer

c = IRISContainer.attach("my-iris", username="SuperUser", password="SYS")
c.reset_password(username="SuperUser", new_password="SYS")  # fresh containers only
conn = c.get_connection()
c.execute_objectscript("Write $ZVERSION,!", namespace="USER")
```

```bash
# Via docker run (manual — requires password fix afterward)
docker run -d --name my-iris -p 0:1972 \
  intersystemsdc/iris-community:2025.1 --check-caps false
```

Mount the workspace when you plan to load code from disk:

```bash
docker run -d --name my-iris \
  --publish 1972 --publish 52773 \
  -v "$(pwd):/home/irisowner/dev" \
  intersystemsdc/iris-community:latest --check-caps false
```

A licensed image needs `irepo` auth and a mounted key:

```bash
docker run -d --name my-iris \
  --publish 1972 --publish 52773 \
  -v "$(pwd):/home/irisowner/dev" \
  -v "$(pwd)/iris.key:/usr/irissys/mgr/iris.key" \
  irepo.intersystems.com/intersystems/iris:2025.1 --check-caps false
```

Clean up when done:

```bash
docker stop my-iris && docker rm my-iris
```

---

## Windows notes

- Docker Desktop must be running in Linux containers mode.
- Volume paths: use `//c/Users/...` or `$(pwd)` in Git Bash, `C:\Users\...` in
  PowerShell.
- `.cls` files must use Unix line endings (LF) — configure Git accordingly.

---

## Common Mistakes

| Mistake                                                     | Fix                                                                                                                                                                                                                                                                   |
| ----------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Using `docker exec` when MCP is connected                   | Use `iris_execute`, `iris_compile`, `iris_test` instead                                                                                                                                                                                                               |
| Session hangs after heredoc                                 | Add `halt` as last line                                                                                                                                                                                                                                               |
| Using `-it` with heredoc                                    | Use `-i` only                                                                                                                                                                                                                                                         |
| Missing leading space in heredoc lines                      | Indent each ObjectScript line with at least one space                                                                                                                                                                                                                 |
| `^UnitTestRoot` wrong dir                                   | Must be the **parent** of the test package directory                                                                                                                                                                                                                  |
| Tests not found                                             | The `RunTest` argument must match a subdirectory under `^UnitTestRoot`                                                                                                                                                                                                |
| Wrong namespace                                             | Add `-U USER` or `-U NAMESPACENAME`                                                                                                                                                                                                                                   |
| Commands fail right after `docker run`                      | Poll for readiness first — IRIS needs 10-30s                                                                                                                                                                                                                          |
| Classes load via `zpm` but methods `<NOROUTINE>`            | Run `do $system.OBJ.CompilePackage("Pkg","ck")` after `zpm load`                                                                                                                                                                                                      |
| All tools fail, `check_config` shows `atelier_rest: false`  | NoPWS build — expected on `2026.2.0AI`; use the `docker exec` path, no config change needed                                                                                                                                                                           |
| Password change required on new container                   | Use `iris-devtester` — auto-remediates in 1.15.0+                                                                                                                                                                                                                     |
| Block-structured code via stdin fails                       | `iris session` REPL processes one line at a time — `If x { }` and `While x { }` cause `<SYNTAX>`. Put logic in class methods and call `do ##class(X).Method()`                                                                                                        |
| **Partial execution trap**: block fails but inner calls run | When a block errors in the REPL, statements INSIDE the block may still have executed. Never assume a `<SYNTAX>` means nothing ran — check side effects before retrying.                                                                                               |
| **Windows IIS: `/api` web application missing**             | `/api/atelier` returns 404 even when Management Portal works. In IIS Manager: Add Application at alias `/api`, map to Web Gateway dir, add wildcard script handler `CSPms.dll`. Also verify `CSP.ini` contains `[APP_PATH:/api]`. See `iris-windows-iis-setup` skill. |
