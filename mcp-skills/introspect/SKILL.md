---
name: introspect
description: Use before writing code that calls, extends, or modifies an ObjectScript class you did not write — fetches the class source from IRIS via Atelier REST and summarizes its API (methods, properties, parameters, inheritance).
---

# /introspect — ObjectScript Class Inspector

Fetch the source and structure of an IRIS class **before writing any code that touches it**.
Works against any IRIS instance with the web server running. No Python, no MCP server needed.

## When to use this skill

- You are about to call, extend, or modify a class you did not write.
- You need to know method signatures, parameter names, return types, or property definitions.
- The class might be private (custom app code, TrakCare, EnsLib, HS*) and not in public docs.
- You see a compile error about a method or property and need to verify the real signature.

## Step 1 — Fetch the class source

```bash
CLASS="${1:-MyPackage.MyClass}"
HOST="${IRIS_HOST:-localhost}"
PORT="${IRIS_WEB_PORT:-52773}"
USER="${IRIS_USERNAME:-_SYSTEM}"
PASS="${IRIS_PASSWORD:-SYS}"
NS="${IRIS_NAMESPACE:-USER}"
PREFIX="${IRIS_WEB_PREFIX:-}"
BASE_URL="http://${HOST}:${PORT}${PREFIX:+/${PREFIX}}/api/atelier/v1"

curl -s \
  -u "${USER}:${PASS}" \
  "${BASE_URL}/${NS}/doc/${CLASS}.cls" \
  | python3 -c "
import json, sys
data = json.load(sys.stdin)
result = data.get('result', {})
content = result.get('content', [])
# content is a list of lines
print('\n'.join(content))
"
```

**If you get a 404**: the class doesn't exist in this namespace. Try another namespace
or check the class name spelling. IRIS class names are case-sensitive at the filesystem level
but case-insensitive at runtime — use exact case from the source.

**If you get a 401**: wrong credentials. Check `IRIS_USERNAME` / `IRIS_PASSWORD`.

## Step 2 — Summarize the API for the coding task

After fetching the source, extract and list:

1. **Class hierarchy** — `Extends` clause (single or multiple inheritance)
2. **Properties** — name, type, any constraints (MAXLEN, MINVAL, etc.)
3. **ClassMethods** — signature: name, all parameter names+types, return type
4. **Instance Methods** — same
5. **Parameters** — class-level constants defined with `Parameter`
6. **Indices** — name, properties indexed

Do NOT include the full implementation body in the summary — signatures and types only.

## Step 3 — Apply what you learned

Before writing code that calls this class:
- Use exact method names (case matters at compile time in some contexts)
- Pass arguments in the correct order
- Respect `%Status` return types — check with `$$$ISOK` / `$$$ISERR`
- Note any `ByRef` or `Output` parameters — they must be passed by reference with `.`

## Example output format

```
Class: EnsLib.HTTP.OutboundAdapter
Extends: Ens.OutboundAdapter

Parameters:
  SETTINGS = "HTTPServer,HTTPPort,URL,SSLConfig,..."

Properties:
  HTTPServer  As %String
  HTTPPort    As %String (default "80")
  URL         As %String
  SSLConfig   As %String

ClassMethods:
  (none beyond inherited)

Methods:
  SendFormDataArray(ByRef pHttpResponse As %Net.HttpResponse,
                    pFormVarNames As %String,
                    ByRef pData,
                    pUrl As %String = "") As %Status
  Get(ByRef pHttpResponse As %Net.HttpResponse,
      pUrl As %String = "") As %Status
  Post(ByRef pHttpResponse As %Net.HttpResponse,
       pUrl As %String = "",
       pData As %GlobalCharacterStream = "") As %Status
```

## Bulk fetch — multiple classes

```bash
for cls in MyPackage.ClassA MyPackage.ClassB Ens.BusinessOperation; do
  echo "=== $cls ==="
  curl -s -u "${IRIS_USERNAME:-_SYSTEM}:${IRIS_PASSWORD:-SYS}" \
    "${BASE_URL}/${NS}/doc/${cls}.cls" \
    | python3 -c "import json,sys; d=json.load(sys.stdin); print('\n'.join(d.get('result',{}).get('content',[])))"
  echo ""
done
```

## Notes

- The Atelier API returns the full UDL source (the `.cls` file format). It includes `///` doc comments above each method — read them.
- If a class uses `Include` macros (e.g. `Include %occStatus`), macro names like `$$$OK` are defined there, not in the class itself.
- Inherited methods are NOT returned in the class source — if you need a parent class's methods, fetch that class too.
- For very large classes, the content array may be truncated. Use `/compile` to check if the full source round-trips correctly.
