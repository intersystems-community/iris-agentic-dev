# CDP Challenge Analysis: iris-agentic-dev Tool Impact

Independent agent replication of the four CDP Discovery Project challenges,
run with and without iris-agentic-dev (IAD) tools. Each condition was a fresh
agent session with no prior IRIS context.

---

## Methodology

- 8 independent agent sessions: 4 challenges × 2 conditions (WITH / WITHOUT IAD)
- Each agent given the same scenario prompt, same live IRIS container (iris-dev-iris, port 52780)
- WITH condition: full IAD MCP tools available (check_config, iris_doc, iris_execute, iris_query, iris_compile, iris_production, iris_interop_query, iris_doc_search, skill, etc.)
- WITHOUT condition: Bash + docker exec + WebFetch only — no IAD tools
- Agents rated difficulty 1–7 (1=hardest, 7=easiest), identical to CDP survey scale
- All 8 agents produced working artifacts

---

## Results Summary

| Challenge                         | WITH IAD | WITHOUT IAD | Delta | Key blocker (without)                                              |
| --------------------------------- | -------- | ----------- | ----- | ------------------------------------------------------------------ |
| 1. Boston 311 (ObjectScript)      | 4/7      | 3/7         | +1    | docs.intersystems.com 403; Atelier compile endpoint silent failure |
| 2. NYC Taxi (Python)              | 4/7      | 3/7         | +1    | docs 403; superserver port vs web port confusion; driver segfaults |
| 3. Patient Intake (Integrations)  | 3/7      | 3/7         | 0     | docs 403; Ens storage rules non-discoverable; "production" jargon  |
| 4. EV Charging (SQL/ObjectScript) | 3/7      | 3/7         | 0     | docs 403; no psql-style CLI; embedded SQL preprocessor quirks      |

**Average ease WITH IAD: 3.5 / 7**
**Average ease WITHOUT IAD: 3.0 / 7**
**CDP cohort reported average: 3.78 / 7** (slightly easier — they had more time and teammate collaboration)

---

## Findings by Challenge

### Challenge 1: Boston 311 Explorer (ObjectScript + IRIS)

**WITH IAD — Rating: 4/7**

Built: 3 ObjectScript classes (ServiceRequest, Loader, Reports), 500 records loaded,
6 report methods including geo proximity search.

IAD tools that mattered:

- `iris_doc` (put + compile=true) — eliminated the edit/compile/check cycle entirely;
  single call writes source and compiles
- `iris_execute` — ad-hoc debugging without leaving the session
- `iris_query` — direct SQL with clean JSON output
- `check_config` — resolved port mismatch in 2 minutes

Friction encountered with IAD:

- `%DynamicObject` underscore key gotcha (`row.case_name` fails; `row.%Get("case_name")` required) — no compile error, runtime UNDEFINED
- SSL not preconfigured in Community container — outbound HTTPS silently fails
- `%TimeStamp` vs `$ZDateTimeH` format mismatch

**WITHOUT IAD — Rating: 3/7**

Built: ServiceRequest class, SQL view, Python loader using Atelier REST API, 50 real records.

Additional blockers vs. WITH:

- docs.intersystems.com returns HTTP 403 — no documentation available
- Atelier `/action/compile` endpoint silently returns "Invalid JSON Content" — class appears loaded but is not live; manifests as "Table not found" on first INSERT
- Load vs. compile are silent distinct steps — no feedback distinguishing "registered" from "live"
- Worked around by: PUT via Atelier + compile via `docker exec iris session`

**Delta:** IAD removes the Atelier compile endpoint failure mode entirely (iris_doc handles compile atomically). The `%DynamicObject` underscore key issue hits both conditions equally.

---

### Challenge 2: NYC Taxi Trip Insights (Python + IRIS)

**WITH IAD — Rating: 4/7**

Built: NYCTaxiTrips table, 4,785 real TLC records loaded via pandas/SQLAlchemy, 4 analytical queries.

IAD tools that mattered:

- `check_config` — immediately showed which port was live and which connection source won
- `iris_execute` — smoke tests before Python setup

Key finding: IAD helped orient the agent to connection state, but the Python-side friction (superserver port, driver segfaults, reserved word HOUR, pandas segfault with IRIS dialect) is outside IAD's scope — it lives in the Python driver layer.

**WITHOUT IAD — Rating: 3/7**

Built: TaxiTrips table, 1,000 synthetic records, 4 queries with ASCII bar chart.

Blockers:

- docs.intersystems.com 403 — no official install/connect docs accessible
- `dbapi.connect(hostname=..., keyword=...)` form (shown on PyPI) segfaults; positional args work — discovered by trial and error only
- `FETCH FIRST n ROWS ONLY` crashes the C extension (SIGSEGV); must use `TOP n`
- Type inconsistency in DB-API return values (Decimal vs str vs float for aggregate results)
- `IRISINSTALLDIR` warning noise on every import

**Critical finding:** Both conditions hit the same Python driver bugs. IAD does not currently surface a skill that warns about the keyword-arg segfault, the FETCH FIRST crash, or the superserver-vs-web-port distinction. The skill gap is here.

---

### Challenge 3: Synthetic Patient Data Intake (Interoperability)

**WITH IAD — Rating: 3/7**

Built: Full 7-component production (Service, Router, Store, ErrorHandler, Message class,
PatientRecord, ErrorLog). 3 valid + 3 invalid test messages routed correctly.

IAD tools that mattered:

- `iris_doc` (get mode) — retrieved `Ens.BusinessProcess` source to discover correct `OnRequest` signature; without this the typed-parameter mistake would have taken much longer
- `iris_doc` (put + compile) — the write/compile workflow for all 6 classes
- `iris_interop_query` — diagnosed the retry-on-old-messages bug from the event log
- `iris_production` — update action hot-reloaded compiled code without restarting

IAD tools that didn't help:

- `skill` — skill store empty on dev instance; no interoperability skill returned
- `iris_production` start action — returned "Invalid Production"; had to use `iris_execute` + `Ens.Director.StartProduction` as workaround

**WITHOUT IAD — Rating: 3/7**

Built: Identical 7-component production architecture. 3 valid + 3 invalid test messages.

Blockers:

- docs.intersystems.com 403 — fell back entirely to own knowledge + reading Ens.StringRequest source via Atelier API
- Storage definition for Ens.Request subclasses (`^Ens.MessageBodyD` requirement) — not documented anywhere accessible; found by reading built-in source
- Global name length limit (31 chars) — IRIS rejects silently or with unhelpful error
- `##class` in JSON strings double-escaped by Atelier PUT — switched to docker cp
- `OnResponse` not implemented causes silent retry errors

**Delta:** Nearly identical difficulty. The Ens framework knowledge requirement is the dominant factor in both conditions — IAD's `iris_doc` get mode gave the WITH agent a shortcut to the correct signature, but the WITHOUT agent found it by reading Ens.StringRequest via Atelier. The production start bug hit both agents.

---

### Challenge 4: EV Charging Infrastructure (SQL + ObjectScript)

**WITH IAD — Rating: 3/7**

Built: EV_Stations_Raw (30 rows), EV_Stations_Curated (28 rows after validation),
EVLoader ObjectScript class, 2 analytical queries. All via IAD query/doc/exec tools.

IAD tools that mattered:

- `iris_query` — felt like psql; DDL, DML, and analytics all in one tool
- `iris_doc` (put) + `iris_compile` — clean class authoring workflow
- `iris_execute` — run ETL class method

Friction with IAD:

- Embedded SQL (`&sql`) preprocessor quirks still hit even with IAD (IAD runs the code; it doesn't prevent the bugs)
- No external network inside container — same for both conditions

**WITHOUT IAD — Rating: 3/7**

Built: EV.RawStation, EV.CuratedStation, same ETL pipeline, 3 analytical queries.

Blockers:

- docs.intersystems.com 403
- No `psql`-style CLI — SQL must go through `%SQL.Statement` in ObjectScript or Management Portal
- Multi-line ObjectScript blocks in docker exec heredoc — required MAC routine files
- CASE expression type strictness (both arms must be same type) — rejected with cryptic compile error
- `%Routine.Compile()` is instance method not class method — discovered by error

**Delta:** `iris_query` is the meaningful difference here. The WITHOUT agent had no equivalent of `psql` and had to route all SQL through ObjectScript methods or Management Portal. This is a real productivity gap for SQL-first developers.

---

## Cross-Cutting Findings

### 1. docs.intersystems.com 403 is a systemic blocker for the WITHOUT condition

Every WITHOUT agent was blocked by HTTP 403 from docs.intersystems.com. This was the most consistent finding across all four challenges. Agents worked around it using:

- PyPI page content (Python challenge)
- Reading built-in IRIS class source via Atelier API (Integrations challenge)
- Prior knowledge of ObjectScript/SQL (ObjectScript and SQL challenges)

A true new developer with no prior IRIS knowledge and no authenticated docs access would be harder blocked than these agents. The CDP report's finding that "hard-to-find instructional materials" was a top barrier is directly confirmed — but the agents' experience suggests the barrier is not just discoverability, it's a literal authentication wall for unauthenticated web tooling.

**IAD impact:** `iris_doc_search` bypasses this entirely — it hits the Algolia index that docs.intersystems.com uses internally, returning real content without requiring authentication.

### 2. IAD's write/compile workflow eliminates a significant class of Atelier API bugs

Without IAD, agents using the Atelier REST API to write and compile classes hit:

- Compile endpoint returning "Invalid JSON Content" silently
- `##class` double-escaping in PUT body
- Load vs. compile as distinct silent steps with no differentiated feedback

IAD's `iris_doc` (put + compile=true) handles all of this atomically and correctly. This is a meaningful gap removal.

### 3. Python connector friction is outside IAD's current scope

The biggest WITHOUT blockers for the Python challenge were all in the Python driver layer:

- Keyword arg segfault vs positional arg (not documented)
- FETCH FIRST crash (SIGSEGV in C extension)
- Superserver port vs web port confusion
- Inconsistent DB-API return types

IAD's `check_config` helped with port discovery. But there is no skill that warns about the segfault-triggering patterns. An `irispython` skill covering these gotchas would directly address the CDP Python team's friction.

### 4. The "productions" discovery problem is not solved by IAD in its current state

Both the WITH and WITHOUT agents for Challenge 3 independently concluded: a new developer would not know to use productions for a file intake/routing/validation scenario. The WITH agent noted the `skill` store was empty on the dev instance and no interoperability skill was returned.

The CDP report's Finding #2 ("Teams often understood the business problem before they understood the InterSystems pattern that fit it") is directly replicated. An `iris-interop` or `iris-productions-101` skill that pattern-matches scenarios to the productions concept would address this.

### 5. Difficulty ratings converge, but route to success diverges

Both conditions completed all four challenges. The 0.5-point ease gap (3.5 vs 3.0) understates the qualitative difference:

- WITH IAD: faster iteration, better error messages, less terminal-wrestling; agents stayed at the problem level
- WITHOUT IAD: more time spent on tooling (heredocs, Atelier API quirks, port discovery, compile gaps); agents frequently shifted from problem-solving to tooling-workaround mode

This mirrors the CDP report's finding: "The strongest product moments happened after teams reached the right abstraction or workflow." IAD moves the inflection point earlier.

---

## Confirmed CDP Report Findings

| CDP Finding                                            | Confirmed?           | Evidence                                                                    |
| ------------------------------------------------------ | -------------------- | --------------------------------------------------------------------------- |
| First-mile pathfinding is the largest friction pattern | ✓ Confirmed          | All 8 agents hit setup/orientation friction before reaching productive work |
| Documentation discoverability is a top barrier         | ✓ Strongly confirmed | docs.intersystems.com 403 blocked every WITHOUT agent                       |
| "Not enough examples"                                  | ✓ Confirmed          | Agents resorted to reading built-in class source or trial-and-error         |
| Products showed value after teams found the right path | ✓ Confirmed          | All 8 agents produced working artifacts; friction was pre-path              |
| AI is part of self-service onboarding                  | ✓ Confirmed          | Both conditions relied heavily on prior knowledge and inference             |
| Productions discovery problem                          | ✓ Confirmed          | Both Challenge 3 agents flagged "production" as non-discoverable jargon     |
| Data loading is an accidental curriculum               | ✓ Confirmed          | All 4 challenges required significant data ingestion problem-solving        |

## Refuted or Nuanced CDP Findings

| CDP Finding                             | Status                               | Notes                                                                   |
| --------------------------------------- | ------------------------------------ | ----------------------------------------------------------------------- |
| Setup/Docker/ports are top friction     | ✓ Partially — but docs 403 was worse | Port confusion was real but resolvable; docs inaccessibility was harder |
| Error messages shaped perceived quality | ✓ Confirmed but IAD helps            | IAD's compile errors are cleaner than Atelier API silent failures       |

---

## Recommended IAD Improvements from This Analysis

### High priority (confirmed blockers)

1. **`irispython` skill** — cover the three segfault-triggering patterns (keyword args, FETCH FIRST, pandas read_sql), superserver vs web port distinction, and type normalization gotcha. This directly addresses the Python team's CDP friction.

2. **`iris-interop-101` skill** — scenario-to-pattern mapping: "file intake/routing/validation → productions"; include the production component vocabulary (Service/Process/Operation), the `Ens.Request` storage constraint, global name length limit, `OnResponse` requirement.

3. **`iris_doc_search` promotion** — surface in `check_config` output: "docs.intersystems.com requires authentication for external tools; use iris_doc_search instead." The authentication wall is invisible until you hit it.

### Medium priority (quality-of-life)

1. **`iris_production` start action fix** — "Invalid Production" on a valid production class is a confusing error; the workaround (`Ens.Director.StartProduction` via `iris_execute`) should either be the action's implementation or the error message should suggest it.

2. **ObjectScript gotcha notes in iris_execute output** — When executing code that uses `%DynamicObject`, surface a hint about `%Get("underscore_key")` vs dot notation. The `<UNDEFINED>` error gives no context.

3. **`iris_doc_search` in the default tool description** — New developers looking for "how do I do X in IRIS" don't know to call `iris_doc_search`. The tool description should mention it handles authentication automatically.

### Informational (for ISC, not IAD)

1. **docs.intersystems.com unauthenticated access** — Blocking WebFetch/curl with 403 creates an invisible wall for AI-assisted development. Public content should be publicly accessible. IAD's Algolia search is a workaround but shouldn't be the only path.

2. **Community Edition boundary markers** — No agent (with or without IAD) could confidently identify which features required Enterprise vs Community before hitting a failure. This was a direct CDP finding; IAD could help by surfacing edition in `check_config` output.
