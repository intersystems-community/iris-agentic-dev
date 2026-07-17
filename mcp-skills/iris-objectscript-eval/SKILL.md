---
author: tleavitt
benchmark_date: '2026-04-02'
benchmark_iris_version: '2025.1'
benchmark_tasks:
- jira-001
- jira-002
- jira-003
- jira-004
- jira-005
- jira-006
- jira-007
- jira-008
- jira-009
- jira-010
- jira-011
- jira-012
- jira-013
- jira-014
- jira-015
- jira-016
- jira-017
- jira-018
- jira-019
- jira-020
- jira-021
- jira-056
description: Load, compile, run, and test ObjectScript code in an IRIS Docker container.
  Use when needing to execute ObjectScript non-interactively, load .cls/.mac/.inc
  files, or run %UnitTest tests via docker exec.
iris_version: '>=2024.1'
name: iris-objectscript-eval
pass_rate: 0.4090909090909091
state: draft
tags:
- objectscript
- eval
- mcp
trigger: Use for tleavitt/iris-objectscript-eval
---

# Evaluating ObjectScript in an IRIS Container

## Overview

Run ObjectScript code in a Docker IRIS container: start the container, load files, compile, run tests, get results.

**Preferred path**: Use the objectscript MCP tools — `iris_compile`, `iris_execute` (via `execute_objectscript`), `iris_test`. These avoid heredoc escaping entirely.

**Fallback path**: `docker exec` + `iris session` when MCP is unavailable.

## Primary Path: MCP Tools + iris-devtester 1.15.0

### Select container and compile
```
iris_select_container(name='kg-iris', namespace='USER', username='SuperUser', password='SYS')
iris_compile(target='MyPackage.MyClass', namespace='USER')
```

### Execute ObjectScript directly (iris-devtester 1.15.0)
```python
from iris_devtester import IRISContainer
c = IRISContainer.attach('kg-iris', username='SuperUser', password='SYS')
result = c.execute_objectscript('Write $ZVERSION,!')
result = c.execute_objectscript('Do ##class(MyPackage.MyClass).MyMethod()', namespace='USER')
```

### Auto-remediate "Password change required" on fresh containers
```python
c = IRISContainer.attach('kg-iris')
c.reset_password(username='SuperUser', new_password='SYS')
conn = c.get_connection()
```

### Run %UnitTest via MCP
```
iris_test(target='MyPackage.Tests', namespace='USER')
```

---

## Fallback Path: docker exec + iris session

### Community Edition (no license key)

```bash
docker run -d --name iris-eval \
  --publish 1972 --publish 52773 \
  -v "$(pwd):/home/irisowner/dev" \
  intersystemsdc/iris-community:latest \
  --check-caps false
```

### Licensed Image (requires irepo auth + iris.key)

```bash
docker run -d --name iris-eval \
  --publish 1972 --publish 52773 \
  -v "$(pwd):/home/irisowner/dev" \
  -v "$(pwd)/iris.key:/usr/irissys/mgr/iris.key" \
  irepo.intersystems.com/intersystems/iris:2025.1 \
  --check-caps false
```

### Wait for IRIS to be ready

```bash
# Poll until healthy (IRIS takes 10-30s to start)
until docker exec iris-eval iris session IRIS -U USER '##class(%SYSTEM.Process).%ClassIsLatestVersion()' 2>/dev/null; do
  sleep 2
done
```

Or simply:

```bash
docker exec iris-eval /bin/bash -c 'for i in $(seq 1 30); do iris session IRIS -U USER "halt" 2>/dev/null && exit 0; sleep 2; done; exit 1'
```

## Executing ObjectScript Non-Interactively

**This is the critical pattern.** Do NOT try to use `docker exec -it` interactively.

### Single command

```bash
docker exec iris-eval iris session IRIS -U USER '##class(Sample.Calculator).Add(2, 3)'
```

Note: single quotes around the ObjectScript expression. The expression is evaluated and its result is printed.

### Multi-line script via heredoc

```bash
docker exec -i iris-eval iris session IRIS -U USER <<'EOF'
 do $System.OBJ.LoadDir("/home/irisowner/dev/cls/","ck",,1)
 halt
EOF
```

**Critical rules:**
- Always end multi-line scripts with `halt` — otherwise the session hangs
- Use `-i` (not `-it`) for heredoc piping
- Use `-U USER` (or `-U NAMESPACE`) to set the namespace
- Indent ObjectScript lines with a space (required by the IRIS terminal)

### Script file approach

Write a `.script` file and execute it:

```bash
docker exec iris-eval iris session IRIS -U USER /home/irisowner/dev/load.script
```

Where `load.script` contains ObjectScript commands (one per line, each indented with a space, ending with `halt`).

## Loading ObjectScript Code

### Load a single file

```bash
docker exec iris-eval iris session IRIS -U USER \
  'do $System.OBJ.Load("/home/irisowner/dev/cls/Sample/Calculator.cls","ck")'
```

Flags: `c` = compile, `k` = keep source.

### Load a directory recursively

```bash
docker exec -i iris-eval iris session IRIS -U USER <<'EOF'
 do $System.OBJ.LoadDir("/home/irisowner/dev/cls/","ck",,1)
 halt
EOF
```

The 4th argument `1` means recursive. This loads all `.cls`, `.mac`, `.inc`, `.int` files.

### Load specific file types from directory

```bash
docker exec iris-eval iris session IRIS -U USER \
  'do $System.OBJ.LoadDir("/home/irisowner/dev/cls/","ck","*.cls",1)'
```

## Running Unit Tests

### Quick approach — load and run inline

```bash
docker exec -i iris-eval iris session IRIS -U USER <<'EOF'
 ; Load all source and test classes
 do $System.OBJ.LoadDir("/home/irisowner/dev/cls/","ck",,1)
 ; Set up UnitTest root and run
 set ^UnitTestRoot = "/home/irisowner/dev/cls/"
 do ##class(%UnitTest.Manager).RunTest("Test","/loadudl")
 halt
EOF
```

**Key details:**
- `^UnitTestRoot` points to the **parent directory** containing test packages
- The first argument to `RunTest` is the subdirectory/package under `^UnitTestRoot`
- `/loadudl` qualifier tells the test manager to load UDL-format files (the `.cls` files on disk)
- Tests matching `*.cls` under the specified subdirectory are discovered and run

### Alternative — classes already loaded, just run

If classes are already compiled in IRIS (loaded earlier), skip `/loadudl`:

```bash
docker exec -i iris-eval iris session IRIS -U USER <<'EOF'
 do ##class(%UnitTest.Manager).RunTest("Test")
 halt
EOF
```

But note: without `/loadudl`, `^UnitTestRoot` must point to a directory with the test `.cls` files and they'll be loaded from there. If already loaded, use `/noload`:

```bash
 do ##class(%UnitTest.Manager).RunTest("","/noload/run")
```

### Reading test results programmatically

```bash
docker exec -i iris-eval iris session IRIS -U USER <<'EOF'
 set rs = ##class(%ResultSet).%New("%UnitTest.Result.TestAssert:Assertions")
 do rs.Execute("")
 while rs.Next() { write rs.Get("Name")," | ",rs.Get("Status"),! }
 halt
EOF
```

## Persistent Dev Container

For iterative development, keep the container running and reload as needed:

```bash
# Start once
docker run -d --name iris-dev \
  -p 1972:1972 -p 52773:52773 \
  -v "$(pwd):/home/irisowner/dev" \
  intersystemsdc/iris-community:latest \
  --check-caps false

# Reload after editing files
docker exec -i iris-dev iris session IRIS -U USER <<'EOF'
 do $System.OBJ.LoadDir("/home/irisowner/dev/cls/","ck",,1)
 halt
EOF

# Run tests
docker exec -i iris-dev iris session IRIS -U USER <<'EOF'
 set ^UnitTestRoot = "/home/irisowner/dev/cls/"
 do ##class(%UnitTest.Manager).RunTest("Test","/loadudl")
 halt
EOF

# Stop when done
docker stop iris-dev && docker rm iris-dev
```

## Cleanup

```bash
docker stop iris-eval && docker rm iris-eval
```

## ZPM Package Loading — Critical Post-Install Step

After `zpm load` or `zpm install`, the package's ObjectScript classes may NOT be compiled. **Always run `CompilePackage` explicitly** after a `zpm load`:

```bash
docker exec -i iris-eval iris session IRIS -U USER <<'EOF'
 zpm "load /home/irisowner/dev"
 do $system.OBJ.CompilePackage("MyPackage","ck")
 halt
EOF
```

Or for loading from a ZPM registry:

```bash
docker exec -i iris-eval iris session IRIS -U USER <<'EOF'
 zpm "install some-package"
 do $system.OBJ.CompilePackage("SomePackage","ck")
 halt
EOF
```

**Why this matters**: `zpm load` imports source files but doesn't guarantee recompilation if the classes already exist (even with stale bytecode). Without the explicit `CompilePackage` call, class lookup succeeds but method dispatch can fail silently with `<NOROUTINE>` or return stale results.

Flags: `c` = compile, `k` = keep source, `e` = display errors only (useful in CI).

---

## Common Mistakes

| Mistake | Fix |
|---------|-----|
| Session hangs after heredoc | Add `halt` as the last line |
| Using `-it` with heredoc | Use `-i` only (no `-t`) |
| Missing leading space on ObjectScript lines | Indent each line with at least one space in heredoc/script |
| Wrong namespace | Add `-U USER` or `-U NAMESPACENAME` |
| `^UnitTestRoot` points to wrong dir | Must be the **parent** of the test package directory |
| Tests not found | Check that RunTest argument matches subdirectory under `^UnitTestRoot` |
| Container not ready | Wait for health check before executing commands |
| Windows path issues in volume mount | Use forward slashes or `$(pwd)` in Git Bash |

## Windows-Specific Notes

- Docker Desktop must be running with Linux containers mode
- Volume paths: use `//c/Users/...` or `$(pwd)` in Git Bash, or `C:\Users\...` in PowerShell
- Line endings: ObjectScript `.cls` files must use Unix line endings (LF) — configure Git accordingly