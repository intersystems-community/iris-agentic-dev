# Contract: what a conforming `inputSchema` must contain

The server's external interface is its MCP tool listing. This is the contract every one of the
81 entries must satisfy after this feature lands, stated in terms of the JSON a client actually
receives from `tools/list` — not in terms of what schemars generates, because the server
rewrites the schema on the way out.

## What the server emits today

`list_tools` is overridden (`src/tools/mod.rs:9190`) and runs `normalize_schema_openapi3` over
every input schema before returning it. That function rewrites a nullable type array into an
`anyOf`, moving `enum`, `format`, `minimum`, `items`, `properties`, `required`, and
`additionalProperties` into the non-null branch. So an **optional** parameter arrives at the
client like this — verified against the running 1.3.2 binary, `iris_query.namespace`:

```json
"namespace": {
  "anyOf": [{ "type": "string" }, { "type": "null" }],
  "default": null,
  "description": "IRIS namespace. Defaults to the connection namespace (IRIS_NAMESPACE)."
}
```

A **required or defaulted** parameter keeps a flat type — `iris_query.force`:

```json
"force": { "type": "boolean", "default": false, "description": "…" }
```

Two consequences that shape every test in this feature:

1. **An enum on an optional parameter is not at the top level of the property.** It lands in
   `anyOf[0].enum`. A test reading `properties.mode.enum` finds nothing and — if written as
   "compare the enum if one is present" — passes vacuously. Principle XI applies directly: the
   test must resolve through `anyOf` and assert it found a non-empty list.
2. **The contract is on the emitted schema**, so unit tests over a struct's generated schema are
   necessary but never sufficient. FR-013's "verify against the listing a running server emits"
   exists because of this rewrite.

`tools/list` is also paginated (`IRIS_LIST_TOOLS_PAGE_SIZE`, default 200). At 81 tools one page
holds everything and `nextCursor` is null, confirmed by probe. A census test must assert
`nextCursor` is null rather than assume it, or it will silently count one page of a larger
future surface.

## Required shape

For a tool that reads parameters:

```json
{
  "name": "iris_system_performance",
  "inputSchema": {
    "type": "object",
    "additionalProperties": false,
    "properties": {
      "mode": {
        "anyOf": [
          { "type": "string", "enum": ["start", "status", "last_runid"] },
          { "type": "null" }
        ],
        "description": "start a collection, check status, or return the last run id"
      },
      "profile": { "anyOf": [{ "type": "string" }, { "type": "null" }] },
      "run_id": { "anyOf": [{ "type": "string" }, { "type": "null" }] },
      "server": { "anyOf": [{ "type": "string" }, { "type": "null" }] }
    }
  }
}
```

Rules, each traceable to a functional requirement:

1. **`properties` is present and non-empty** for every tool that reads a parameter (FR-001,
   FR-002). A tool that reads nothing declares `"properties": {}` explicitly, which is
   distinguishable from having no `properties` key at all.
2. **`additionalProperties` is `false`** on every tool (FR-002, FR-015). **Zero of the 81 tools
   emit this today** — verified — so this rule applies to the already-typed 50 as much as to the
   31 being converted.
3. **Every property declares a type** (FR-001), either flat or as the non-null branch of its
   `anyOf`. `string`, `integer`, `boolean`, `object`, `array`, or `["integer", "string"]` for
   the two parameters that accept both.
4. **`required` lists only unconditionally required parameters** (FR-007). A parameter that
   matters only for certain values of a sibling is optional here, with the per-action rules in
   `docs/tools.md`.
5. **A closed value set appears as `enum`** (FR-005) — at the property level if the parameter is
   required, in `anyOf[0]` if optional — and its members equal the values the handler branches
   on.
6. **`description` appears only where the property name is not self-explanatory.** Schema bytes
   are context the caller pays for every session: `server` needs no sentence, `mode` does.

## Non-goals of this contract

- No output-schema change. `outputSchema` is stripped from `tools/list` entirely (payload-size
  fix for #113) and is untouched here.
- No parameter renamed, removed, or given a new default.
- No `if`/`then`/`oneOf` conditional-requirement modelling. See research decision 4.

## How conformance is checked

| Layer          | What it proves                                                                        |
| -------------- | ------------------------------------------------------------------------------------- |
| Unit           | The generated schema for one struct has the properties, types, and enums expected     |
| Binary (spawn) | A running server emits that schema through `list_tools`, after normalization (FR-013) |
| Live IRIS      | Each advertised parameter is honored by a real call (FR-014)                          |

Three suite-wide assertions guard the whole surface:

- **Zero tools with empty `properties`** while reading parameters (FR-010). This replaces the
  handler-body fallback in `every_documented_tool_parameter_is_in_the_input_schema`.
- **Advertised set equals read set**, per tool, both directions (FR-003).
- **`additionalProperties: false` on all 81** (FR-002), counted, so a new tool cannot omit it.

Plus one gate-suite check: `scripts/gates/antipatterns.py` flags a params struct missing
`deny_unknown_fields`, or a `Parameters<serde_json::Value>` (FR-011). `Parameters<AnyParams>`
needs no scanner rule — the type is deleted, so it stops compiling.

## Admin write operations

`IRIS_ADMIN_TOOLS=1` gates admin **write actions inside** `iris_admin` at runtime
(`src/tools/admin.rs:23`); it does not filter the tool listing. All 81 tools are advertised
unconditionally, so schema conformance tests need no environment variable. Live round-trip tests
that exercise an admin write action do need it, and set it themselves per Principle XII rather
than inheriting it from the operator's shell.
