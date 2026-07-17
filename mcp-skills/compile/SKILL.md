---
name: compile
description: Use after writing or editing any ObjectScript .cls file, after applying a fix to a compile error, or before declaring a task done — uploads the class to IRIS via Atelier REST, compiles it, and returns structured errors for immediate fixing.
---

# /compile — ObjectScript Compile & Fix Loop

Upload and compile an ObjectScript class via Atelier REST. Parses errors into a structured
format so the AI can fix them immediately. **Always run this after writing or editing ObjectScript.**

## When to use this skill

- After writing or editing any `.cls` file
- After applying a fix to a compile error
- To verify that a class compiles clean before declaring a task done
- When you get a runtime `<UNDEFINED>` or `<NOLINE>` and want to re-verify the source

## Step 1 — Upload and compile a .cls file

```bash
CLASS="${1:-MyPackage.MyClass}"
FILE="${2:-${CLASS//./\/}.cls}"   # MyPackage.MyClass → MyPackage/MyClass.cls
HOST="${IRIS_HOST:-localhost}"
PORT="${IRIS_WEB_PORT:-52773}"
USER="${IRIS_USERNAME:-_SYSTEM}"
PASS="${IRIS_PASSWORD:-SYS}"
NS="${IRIS_NAMESPACE:-USER}"
PREFIX="${IRIS_WEB_PREFIX:-}"
BASE_URL="http://${HOST}:${PORT}${PREFIX:+/${PREFIX}}/api/atelier/v1"

# Read file content as a JSON array of lines
CONTENT=$(python3 -c "
import json, sys
with open('${FILE}') as f:
    lines = f.read().splitlines()
print(json.dumps(lines))
")

# Upload (PUT) the class source — use ignoreConflict=1 to bypass timestamp conflicts
HTTP_CODE=$(curl -s -o /tmp/atelier_put.json -w "%{http_code}" -X PUT \
  -u "${USER}:${PASS}" \
  -H "Content-Type: application/json" \
  "${BASE_URL}/${NS}/doc/${CLASS}.cls?ignoreConflict=1" \
  -d "{\"enc\": false, \"content\": ${CONTENT}}")

echo "HTTP: $HTTP_CODE"

# **Always re-read after writing to confirm the content landed**
VERIFY=$(curl -s -u "${USER}:${PASS}" \
  "${BASE_URL}/${NS}/doc/${CLASS}.cls" \
  | python3 -c "import json,sys; d=json.load(sys.stdin); print(len(d['result']['content']), 'lines stored')")
echo "Stored: ${VERIFY}"

# Compile
RESULT=$(curl -s -X POST \
  -u "${USER}:${PASS}" \
  -H "Content-Type: application/json" \
  "${BASE_URL}/${NS}/action/compile" \
  -d "[\"${CLASS}.cls\"]")

# Parse and display errors — IRIS 2025.1 compile response shape:
# - status.errors[] = top-level errors (parse failures, class-not-found)
# - console[] = human-readable lines including per-line error messages
# - result.content[] = always empty for compile (NOT a list of message objects)
echo "$RESULT" | python3 -c "
import json, sys
data = json.load(sys.stdin)
status_errors = data.get('status', {}).get('errors', [])
console = data.get('console', [])

# Top-level errors (e.g. #5559 parse error, #5351 class not found)
if status_errors:
    print('ERRORS:')
    for e in status_errors:
        print(' ', e.get('error', str(e)))
    sys.exit(0)

# Per-line errors and warnings appear in the console array
error_lines = [l for l in console if 'ERROR' in l or 'error' in l.lower()]
warn_lines  = [l for l in console if 'WARNING' in l or 'warning' in l.lower()]
if error_lines:
    print('ERRORS:')
    for l in error_lines: print(' ', l.strip())
elif warn_lines:
    print('WARNINGS:')
    for l in warn_lines: print(' ', l.strip())
elif any('successfully' in l for l in console):
    print('Compiled successfully — no errors.')
else:
    # Print full console for diagnosis
    for l in console:
        if l.strip(): print('CONSOLE:', l.strip())
"
```

## Step 2 — Interpret compile errors

Common error patterns and what they mean:

| Error | Meaning | Fix |
|---|---|---|
| `ERROR #5659: 'Return' does not match return type` | Method declared `As %Status` but returns a non-status value | Return `$$$OK` or an error status |
| `ERROR #5002: <UNDEFINED>varname+N^Class.1` | Variable `varname` used before being set at line N of INT | Add `Set varname = ""` or check logic flow |
| `ERROR #5002: <NOLINE>` | Syntax error above the reported line | Check for missing braces, unbalanced quotes |
| `ERROR #5001: Class 'Foo.Bar' does not exist` | Missing class — wrong namespace or typo | Run `/introspect` to verify the class name |
| `ERROR #6301: SAX XML error... expected '>'` | Malformed class definition header | Check `Class ... Extends ...` line for typos |
| `ERROR #5563: Illegal use of QUIT` | `Quit value` inside TRY/CATCH or loop | Replace with `Return value` |

## Step 3 — Fix and re-compile loop

After fixing an error:
1. Edit the `.cls` file
2. Re-run this skill (`/compile MyPackage.MyClass path/to/MyClass.cls`)
3. Repeat until output is `Compiled successfully — no errors.`
4. Then run tests (see below)

**Do not proceed to the next task until compilation is clean.**

## Compile-only (class already in IRIS, no upload)

```bash
curl -s -X POST \
  -u "${IRIS_USERNAME:-_SYSTEM}:${IRIS_PASSWORD:-SYS}" \
  -H "Content-Type: application/json" \
  "${BASE_URL}/${NS}/action/compile" \
  -d '["MyPackage.MyClass.cls"]'
```

## Run %UnitTest after compile

```bash
# Via iris session (most reliable)
iris session IRIS -U "${IRIS_NAMESPACE:-USER}" \
  "Do ##class(%UnitTest.Manager).RunTest(\"MyPackage.Tests\",,\"/nodelete\")"
```

> **Note**: Use `iris session` above — the Atelier `/action/query` endpoint executes SQL only and cannot run ObjectScript `Do` commands. If you have the full MCP server, use the `iris_test` tool instead.

## Compile a whole package

```bash
# Compile all classes in MyPackage.*
curl -s -X POST \
  -u "${IRIS_USERNAME:-_SYSTEM}:${IRIS_PASSWORD:-SYS}" \
  -H "Content-Type: application/json" \
  "${BASE_URL}/${NS}/action/compile" \
  -d '["MyPackage.*.cls"]'
```

## Notes

- The Atelier web port is **52773** by default, NOT 1972 (that's the superserver/JDBC port).
- In Docker, the web port is often mapped to a random host port. Use `docker port <container> 52773` to find it.
- Severity levels: `1` = informational, `2` = warning, `3+` = error (blocks compilation).
- The `console` field in the response may contain additional `%Status` text from class generators or macros.

## Critical: HTTP 409 Conflict

Atelier returns **HTTP 409** when the server has a newer copy of the document than the timestamp you sent (a standard optimistic concurrency check). It is NOT a permanent `upd` flag — it means "your copy is stale."

**The correct fix is `?ignoreConflict=1`**, not delete+PUT:

```bash
curl -s -X PUT -u _SYSTEM:SYS \
  -H "Content-Type: application/json" \
  "http://localhost:52773/api/atelier/v1/USER/doc/MyDoc.mac?ignoreConflict=1" \
  -d '{"enc":false,"content":[...]}'
```

This tells the server to overwrite regardless of timestamp. Use it when you know your version is the one you want to keep (i.e. you just generated or edited the content).

**Always re-read after writing** to confirm the content landed — the PUT response body is unreliable. A subsequent GET is the only reliable confirmation.

Note: the `upd` field in `docnames` listings is a VS Code workspace-dirty flag, not a write-success indicator. Ignore it.
