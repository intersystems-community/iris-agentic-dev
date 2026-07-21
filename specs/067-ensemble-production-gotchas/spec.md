# Spec 067 — ensemble-production skill: discovery hint + CDP-surfaced gotchas

## Summary

Extend the existing `skills/skills/ensemble-production/SKILL.md` with:

1. A scenario-to-pattern decision table so new developers discover productions
   for intake/routing/validation scenarios without prior IRIS knowledge.
2. Five non-obvious implementation rules surfaced by CDP challenge replication.
3. A programmatic test-message pattern using `iris_execute` (no Portal needed).

## Motivation

CDP integration challenge and independent agent replication confirm: new developers
with a file-intake/routing/validation scenario do not discover productions without
a hint. Both WITH-IAD and WITHOUT-IAD agents independently concluded a new developer
would not find "production" for this scenario. The skill store returned nothing on
the dev instance during the challenge.

Five concrete gotchas hit both agents independently:

1. `OnRequest` typed-parameter mistake — must use `%Library.Persistent`, not
   the concrete message class, or IRIS rejects the override.
2. `Ens.Request` storage constraint — `DataLocation` must be `^Ens.MessageBodyD`;
   setting a custom DataLocation in XData Storage causes `#5477`.
3. Global name length limit — IRIS rejects names > 31 chars silently or with
   an opaque error; class name + "D"/"I" suffix must fit.
4. `OnResponse` required — omitting it in a BusinessProcess causes silent
   "Not implemented" errors on every async response.
5. `iris_production` start returns "Invalid Production" on valid classes —
   workaround is `Ens.Director.StartProduction` via `iris_execute`.

## Spec

### Changes to existing skill

`skills/skills/ensemble-production/SKILL.md` — add two new sections.

**Section: "When to use productions — scenario mapping"**

Add before the existing Context Detection section:

| Scenario                                                | Pattern                                |
| ------------------------------------------------------- | -------------------------------------- |
| Receive files/messages, validate, route to destinations | Production                             |
| Transform between formats (HL7 ↔ JSON ↔ XML)            | Production + Business Process          |
| Call external API or write to DB as part of a workflow  | Production + Business Operation        |
| Simple scheduled script                                 | Task Scheduler (not productions)       |
| Real-time REST endpoint                                 | CSP/REST application (not productions) |

One-line description of the four components: Service (data in), Process (routing/
transform), Operation (data out), Message (the carrier — extend Ens.Request).

**Section: "Common gotchas (non-obvious rules)"**

```
OnRequest signature — must be %Library.Persistent
    Wrong:  Method OnRequest(req As MyPkg.MyMsg, ...) As %Status
    Right:  Method OnRequest(req As %Library.Persistent, ...) As %Status
    Then:   Set typedReq = req  // IRIS coerces automatically

Ens.Request storage — never set DataLocation
    Omit <DataLocation> from XData Storage entirely.
    IRIS uses ^Ens.MessageBodyD automatically.

Global name length — 31-char limit
    "MyLongPackage.Data.PatientRecord" → storage global "MyLongPackage.Data.PatientRecordD"
    That's 34 chars → compile error. Shorten the class name.

OnResponse required in BusinessProcess
    Even if unused: Method OnResponse(...) As %Status { Return $$$OK }
    Missing it causes "Not implemented" log noise on every async response.

iris_production start returning "Invalid Production"
    Workaround: iris_execute with
    do ##class(Ens.Director).StartProduction("YourPkg.Production")
```

**Section: "Sending a test message without the Management Portal"**

Show the `iris_execute` pattern for programmatically instantiating and sending
a test message to a running production — replaces the Portal test UI for
automated testing.

## Out of scope

- Full production authoring tutorial (existing skill covers this)
- pyprod declarative API (existing skill covers this)
- HL7 / FHIR specifics

## Acceptance criteria

- [ ] Scenario mapping table added before Context Detection
- [ ] All five gotchas documented with wrong/right examples
- [ ] Test-message pattern via `iris_execute` added
- [ ] markdownlint + prettier clean
- [ ] No regressions to existing sections
