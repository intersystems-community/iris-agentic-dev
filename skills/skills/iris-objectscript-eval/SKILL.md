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

```
iris_execute(code="write $ZVERSION,!", namespace="USER")
iris_execute(code="set x=42\nwrite x,!")          # multiline: separate with \n
iris_execute(code="write ##class(My.Pkg).Run()")  # call class methods
```

Returns `{success, output, namespace}`. Runtime errors are returned as structured errors, not exceptions.

### Compile a .cls file

```
iris_compile(target="MyPackage/MyClass.cls", namespace="USER")
iris_compile(target="*.cls")   # compile all .cls files in workspace
```

### Run %UnitTest tests

```
iris_test(pattern="MyPackage.Tests.*")
iris_test(pattern="MyPackage.Tests.MyClassTest")   # single class
```

### Discover classes

```
iris_symbols(query="MyPackage.*")       # live namespace search
iris_symbols_local()                    # parse .cls files on disk, no IRIS needed
docs_introspect(class_name="My.Class") # full method signatures
```

### Switch containers mid-session

```
iris_list_containers()                          # see all running IRIS containers
iris_select_container(name="my-iris-container") # reconnect without restart
```

---

## Fallback: docker exec (when MCP is unavailable)

Only use this path when the MCP is not connected.

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

### Compile via docker exec

```bash
docker exec <container> iris session IRIS -U USER \
  'do $System.OBJ.Load("/home/irisowner/dev/cls/MyPackage/MyClass.cls","ck")'

# Load directory recursively
docker exec -i <container> iris session IRIS -U USER <<'EOF'
 do $System.OBJ.LoadDir("/home/irisowner/dev/cls/","ck",,1)
 halt
EOF
```

### Run tests via docker exec

```bash
docker exec -i <container> iris session IRIS -U USER <<'EOF'
 set ^UnitTestRoot = "/home/irisowner/dev/cls/"
 do ##class(%UnitTest.Manager).RunTest("Test","/loadudl")
 halt
EOF
```

`^UnitTestRoot` must point to the **parent** of the test package directory.

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

```bash
# Via docker run (manual — requires password fix afterward)
docker run -d --name my-iris -p 0:1972 \
  intersystemsdc/iris-community:2025.1 --check-caps false
```

---

## Common Mistakes

| Mistake | Fix |
|---------|-----|
| Using `docker exec` when MCP is connected | Use `iris_execute`, `iris_compile`, `iris_test` instead |
| Session hangs after heredoc | Add `halt` as last line |
| Using `-it` with heredoc | Use `-i` only |
| Missing leading space in heredoc lines | Indent each ObjectScript line with at least one space |
| `^UnitTestRoot` wrong dir | Must be the **parent** of the test package directory |
| Password change required on new container | Use `iris-devtester` — auto-remediates in 1.15.0+ |
| Block-structured code via stdin fails | `iris session` REPL processes one line at a time — `If x { }` and `While x { }` cause `<SYNTAX>`. Put logic in class methods and call `do ##class(X).Method()` |
| **Partial execution trap**: block fails but inner calls run | When a block errors in the REPL, statements INSIDE the block may still have executed. Never assume a `<SYNTAX>` means nothing ran — check side effects before retrying. |
| **Windows IIS: `/api` web application missing** | `/api/atelier` returns 404 even when Management Portal works. In IIS Manager: Add Application at alias `/api`, map to Web Gateway dir, add wildcard script handler `CSPms.dll`. Also verify `CSP.ini` contains `[APP_PATH:/api]`. See `iris-windows-iis-setup` skill. |
