---
name: compile
description: Upload a .cls file to IRIS, compile it via Atelier REST, and return structured errors for fixing.
args:
  - name: class_name
    description: "Fully-qualified class name (e.g. MyPackage.MyClass)"
  - name: file_path
    description: "Path to the .cls file on disk (optional — if omitted, compiles an already-uploaded class)"
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
USER="${IRIS_USER:-_SYSTEM}"
PASS="${IRIS_PASS:-SYS}"
NS="${IRIS_NS:-USER}"

# Read file content as a JSON array of lines
CONTENT=$(python3 -c "
import json, sys
with open('${FILE}') as f:
    lines = f.read().splitlines()
print(json.dumps(lines))
")

# Upload (PUT) the class source
UPLOAD=$(curl -s -X PUT \
  -u "${USER}:${PASS}" \
  -H "Content-Type: application/json" \
  "http://${HOST}:${PORT}/api/atelier/v1/${NS}/doc/${CLASS}.cls" \
  -d "{\"enc\": false, \"content\": ${CONTENT}}")

echo "Upload status: $(echo $UPLOAD | python3 -c "import json,sys; d=json.load(sys.stdin); print(d.get('status',{}))")"

# Compile
RESULT=$(curl -s -X POST \
  -u "${USER}:${PASS}" \
  -H "Content-Type: application/json" \
  "http://${HOST}:${PORT}/api/atelier/v1/${NS}/action/compile" \
  -d "[\"${CLASS}.cls\"]")

# Parse and display errors
echo "$RESULT" | python3 -c "
import json, sys
data = json.load(sys.stdin)
result = data.get('result', [])
console = data.get('console', [])
errors = []
warnings = []
for item in result:
    for msg in item.get('messages', []):
        severity = msg.get('severity', 0)
        text = msg.get('text', '')
        line = msg.get('line', '?')
        col  = msg.get('col', '?')
        entry = f'  line {line}: {text}'
        if severity >= 3:
            errors.append(entry)
        elif severity == 2:
            warnings.append(entry)
if errors:
    print('ERRORS:')
    for e in errors: print(e)
elif warnings:
    print('WARNINGS:')
    for w in warnings: print(w)
else:
    print('Compiled successfully — no errors.')
# Print console output (includes %Status messages)
for line in console:
    if line.strip():
        print('CONSOLE:', line)
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
  -u "${IRIS_USER:-_SYSTEM}:${IRIS_PASS:-SYS}" \
  -H "Content-Type: application/json" \
  "http://${IRIS_HOST:-localhost}:${IRIS_WEB_PORT:-52773}/api/atelier/v1/${IRIS_NS:-USER}/action/compile" \
  -d '["MyPackage.MyClass.cls"]'
```

## Run %UnitTest after compile

```bash
# Via iris session (most reliable)
iris session IRIS -U "${IRIS_NS:-USER}" \
  "Do ##class(%UnitTest.Manager).RunTest(\"MyPackage.Tests\",,\"/nodelete\")"
```

Or via Atelier action:
```bash
curl -s -X POST \
  -u "${IRIS_USER:-_SYSTEM}:${IRIS_PASS:-SYS}" \
  -H "Content-Type: application/json" \
  "http://${IRIS_HOST:-localhost}:${IRIS_WEB_PORT:-52773}/api/atelier/v1/${IRIS_NS:-USER}/action/query" \
  -d '{"query": "Do ##class(%UnitTest.Manager).RunTest(\"MyPackage.Tests\",,\"/nodelete\")"}'
```

## Compile a whole package

```bash
# Compile all classes in MyPackage.*
curl -s -X POST \
  -u "${IRIS_USER:-_SYSTEM}:${IRIS_PASS:-SYS}" \
  -H "Content-Type: application/json" \
  "http://${IRIS_HOST:-localhost}:${IRIS_WEB_PORT:-52773}/api/atelier/v1/${IRIS_NS:-USER}/action/compile" \
  -d '["MyPackage.*.cls"]'
```

## Notes

- The Atelier web port is **52773** by default, NOT 1972 (that's the superserver/JDBC port).
- In Docker, the web port is often mapped to a random host port. Use `docker port <container> 52773` to find it.
- Severity levels: `1` = informational, `2` = warning, `3+` = error (blocks compilation).
- The `console` field in the response may contain additional `%Status` text from class generators or macros.
