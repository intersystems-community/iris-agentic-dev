# PR 142 `iris-mdx`: evidence per discrepancy

Asaf Sinay wrote the `iris-mdx` skill in PR 142 from his work on the Samples-BI cubes. The 131 fixture tests disagreed with ten of its claims. This file takes each disagreement, runs Asaf's own query on Asaf's own cubes (HoleFoods, Patients), and traces the result into the DeepSee source and the product docs in Perforce.

Two of my earlier "false" calls were wrong. I had tested them only on the 8-row 131 fixture cube, which each test rebuilds, so every query there is a first run. On Samples-BI, R1 (`%MDX` on an axis) and R7 (Channel keys) hold.

## How to reproduce

Every case lives in `crates/iris-agentic-dev-core/tests/fixtures/mdx142/IADRepro.MDX142.cls`. Import it into any namespace that has Samples-BI built, then:

```objectscript
Do ##class(IADRepro.MDX142).Run()      // every case
Do ##class(IADRepro.MDX142).Run("R1")  // one case, prefix match
```

Each case prints the claim, the query, and either the failing stage with its error text or the grid with its labels. The live test `tests/integration/test_mdx_142_samples_bi_live.rs` loads the class, asserts each outcome, and deletes the class:

```bash
IRIS_HOST=localhost IRIS_WEB_PORT=52780 IRIS_USERNAME=_SYSTEM IRIS_PASSWORD=SYS \
IAD_SAMPLES_BI_NS=IADSBI \
cargo test --features testing --test integration test_mdx_142_samples_bi_live -- \
  --ignored --test-threads=1 --nocapture
```

Measured on `iris-dev-iris`, IRIS for UNIX (Ubuntu ARM64) 2026.2.0L build 208U, namespace IADSBI, HoleFoods with 1,000,000 facts and Patients with 1,000. Samples-BI data is generated without a seed, so counts differ between builds; the test asserts shapes, labels, errors and relations between cells.

Source paths are under `//iris/latest/databases/sys/cls/DeepSee/` at change 9628818 (2026-09-30) unless a branch is named. Doc paths are under `//learning/iris-doc/latest/doc/cache/en-us/src/`.

## Summary

| Id  | PR claim                                                            | Result on 2026.2                                                                     | Verdict                                            |
| --- | ------------------------------------------------------------------- | ------------------------------------------------------------------------------------ | -------------------------------------------------- |
| R1  | `%MDX()` directly on an axis returns empty, no error                | Empty on every run after the first of a query text, in any position                  | IRIS bug new in 2026.2; symptom right, cause wrong |
| R2  | `MEASURES.MEMBERS` excludes `%COUNT`                                | `%COUNT` is the first column                                                         | Doc error, copied into the skill                   |
| R3  | `MAX(set, measure)` on the same axis avoids the multiple-axes error | Raises that error at prepare                                                         | Skill error                                        |
| R4  | Without `%LABEL`, a PrevMember measure's header shows `DateOfSale`  | A `WITH MEMBER` keeps its own name; inline tuples and sets show `DateOfSale`         | Claim right for inline tuples, wrong trigger       |
| R5  | `WHERE {a, b}` can double-count; IRIS rewrites it to `%OR`          | Equal to `%OR`                                                                       | Version change (fixed in 2023.3); docs stale       |
| R6  | `%OR` on an axis is labelled `asthma+`                              | `asthma...diabetes` here                                                             | Both labels real; depends on build-time member IDs |
| R7  | Channel needs `&[2]`, `&[Online]` returns null                      | Holds                                                                                | Claim right; the stated reason is not              |
| R8  | A Patients dimension in a HoleFoods query gives Invalid Member spec | A member key errors anywhere; `.MEMBERS` of an unknown dimension is dropped silently | Documented; claim half right                       |
| R9  | Unknown measure gives Measure not found                             | Holds, raised by `%Execute`, not `%PrepareMDX`                                       | Claim right; stage detail added                    |
| R10 | Not a PR claim: compound `NOW` offset on a month level              | `[NOW-1y10m]` lands one month back                                                   | IRIS limitation, silent                            |
| S1  | `iris_info what=sa_schema` returns cube structure                   | 404 for any cube name                                                                | iad's fault, not the skill's                       |

## R1: `%MDX()` on an axis returns empty

**PR:** checklist, section 11 and section 13 (`/tmp/pr142-orig.md` line 286).

**Repro:** R1a is the PR's query and is empty. R1b and R1c use a text nobody has run (a nonce in a `WITH MEMBER`), each run twice by `Twice()`. R1d is the control, the same subquery as plain MDX.

```text
R1a: cols=[] rows=0 cells:
R1b: 1: cols=[Results] rows=0 cells: 10698212.77 || 2: cols=[] rows=0 cells:
R1c: 1: cols=[Results] rows=0 cells: 1000 || 2: cols=[] rows=0 cells:
R1d: 1: cols=[Revenue] rows=0 cells: 10698212.77 || 2: cols=[Revenue] rows=0 cells: 10698212.77
```

On the second run `%GetAxisCount()` is 0 and `%Print()` shows `*`, with a `%Status` of OK. The `WITH MEMBER` form and the `{..., %MDX()}` set form break the same way, so position on the axis is not the cause.

**Source:**

- `Query/setFunction.xml:5150` (PreMDX) sets `%mustCompute=1` for any query that contains `%MDX`.
- On the next run of the same text, `%CheckResultsCache` (`Query/query.xml:2713`) sees `MUST-COMPUTE`, and `%InitializeResultsCache` sanitizes the cache node (`query.xml:2983-2991`, DP-443341, change 8846331, 2026-02-20). It kills every `Results` subnode except axis, data, kpi and query, which removes `size` and `cells`.
- `%ExecuteAxes` then takes the "CURRENT AXES ARE VALID" early `Quit` (`query.xml:3943-3957`) before `size` is rewritten at line 3987, so the result reports no axes.
- Traced with `%dstrace=1` on the live instance.

**Docs:** `D2RMDX/D2RMDX.xml:2252-2290` says `%MDX` returns one value and puts no limit on where it can appear.

**Verdict:** IRIS bug. Asaf saw a real failure, and anyone who runs a query twice on 2026.2 will see it. The skill's explanation, that `%MDX` fails when it sits directly on an axis, is wrong: the second run of any query containing `%MDX` is empty. The 131 test `mdx_function_on_an_axis_returns_its_value` passed only because the fixture cube is rebuilt for each test, so each query was a first run. That test measures the fixture, not this bug.

**Older branches** (from source; I ran only 2026.2):

| Branches             | DP-443341 sanitize | DP-450485 ("Check for stale 'No results'", change 9190770, 2026-06-15) | Expected      |
| -------------------- | ------------------ | ---------------------------------------------------------------------- | ------------- |
| 2024.1.x to 2026.1.x | no                 | no                                                                     | no cache bug  |
| 2026.2.x             | yes                | no                                                                     | bug, measured |
| 2026.3.x             | yes                | yes                                                                    | not measured  |

On 2024.1.x to 2026.1.x the must-compute path sets `%recomputeAxes = 1` (WAL085, 2026.1.x `query.xml:3097-3103`, with the comment "%MDX statements create subqueries that deal with caching on their own"), so the axes are rebuilt on every run and never come back empty. Change 8830710 removed that block. In 2026.2, `%recomputeAxes` is set only for the reasons `axis`, `kpi` and `noData`, and `mustCompute` is not one of them.

DP-443341 follows change 8830710 (DTB1282, "Make cache validation more consistent... Move away from timestamps for cell-based result caches").

**A second, older `%MDX` bug (2024.1 onward):** when the subquery fails at `%Execute`, the error is swallowed and the cell shows `*`. `Query/setFunction.xml:5221-5228` (2024.1.x: 5051-5057) resets the status before reading it:

```objectscript
Set tSC = $$$OK
Set tChild.value = $System.Status.GetErrorText(tSC)
```

`%MDX("SELECT MEASURES.[NoSuchMeasure] ON 0 FROM HoleFoods")` shows `*`. A bad cube name fails at prepare instead and does show its error. Also, `%IsMeasure` counts `%MDX` as a measure (`query.xml:6925-6947`), so `%MDX` on axis 1 with a measure on axis 0 fails with "Measures cannot exist on multiple axes".

## R2: `MEASURES.MEMBERS` excludes `%COUNT`

**PR:** section 9 (line 271).

**Repro:** `R2: cols=[Count|Revenue|Units Sold|Max Units|Big Sale Count]`. The PR's workaround `{MEASURES.[%COUNT], MEASURES.MEMBERS}` shows Count twice.

**Source:**

- `Generator.xml:757-765` (//iris/latest#61) registers `%COUNT` as measure 1 of dimension 0 with the hidden flag 0.
- `Query/memberMeasure.xml:130-141` (`%GetMembers`, #3) keeps every node where `'tHidden && (tType="m")`. It has no `%COUNT` exception.

**Docs:** `D2GMDX/D2GMDX.xml:465` says `MEASURES.MEMBERS` shows "all measures (except for `%COUNT`)", and line 1004 says "(apart from %COUNT)". The sample output under both has no count row. The wording is the same in 2024.1.x, 2025.1.x and latest.

**Older branches:** the code is the same in Caché //dev/2011.1.x, 2012.1.x and 2013.1.x and in //iris 2022.1.x, 2024.1.x, 2025.1.x and latest.

**Verdict:** doc error. The skill repeats the docs, and no release has ever behaved that way. Doc bug report drafted below.

## R3: `MAX` on the same axis

**PR:** section 11 (lines 370-378).

**Repro:** `R3: PREPARE ERROR ERROR #5001: Measures cannot exist on multiple axes`.

**Source:** `Query/query.xml:700-707` raises the error when `%HasMeasure` finds a measure on more than one axis (change 2805849, every branch from 2024.1.x to 2026.3.x). `%IsMeasure` (`query.xml:6861` onward) treats `MAX(set, MEASURES.x)` as a measure.

What works on Patients: `MAX(set)` without the measure argument on axis 1 with the measure on axis 0 (five rows plus a MAX row of 826). `{MEASURES.[%COUNT], MAX(set, MEASURES.[%COUNT])} ON 0` with the members on axis 1 runs, but gives each row's own value, not a benchmark.

**Docs:** `D2RMDX/D2RMDX.xml:4694-4735` shows `MAX(set) ON 1` with measures on axis 0, or the two-argument form alone on axis 0. No doc states the one-axis rule for measures.

**Verdict:** skill error, plus a doc gap. The workaround produces the error it is meant to avoid.

## R4: PrevMember header without `%LABEL`

**PR:** section 10 (lines 300-307).

**Repro:** R4, the PR's `WITH MEMBER` query, is headed `[Units Sold|PrevUnits]` for 69 months, and PrevUnits holds the previous month's value. More forms, measured on months 202201 and 202202:

| Query form                                                                                   | Column label         |
| -------------------------------------------------------------------------------------------- | -------------------- |
| `WITH MEMBER MEASURES.[PrevUnits] AS '(...CurrentMember.PrevMember, MEASURES.[Units Sold])'` | `PrevUnits`          |
| The same, inside `%LABEL(..., "Units (Prev Month)", "")`                                     | `Units (Prev Month)` |
| The tuple inline on the axis                                                                 | `DateOfSale`         |
| The tuple inside a `WITH SET`                                                                | `DateOfSale`         |
| A literal member, `(...&[2023].PREVMEMBER, MEASURES.[Units Sold])`                           | `2022`               |

**Source:**

- `Query/query.xml:1341` and `1379` (`%RewriteForCurrentMember`, #61): "try to find best caption for currentMember, otherwise use dimension name". Lines 1654-1689 handle the tuple case: the measure child is skipped and the tuple takes the CurrentMember's label.
- `Query/calculatedMember.xml:214-227` relabels a calculated member's node with its own name.
- All of these lines date from change 2805849 (2017) and are unchanged in 2022.1.x, 2024.1.x and 2025.1.x.

**Docs:** nothing documents the dimension-name default.

**Verdict:** the header behaviour is real and by design, but it applies to an inline tuple or set, not to a `WITH MEMBER`. The skill should move the `%LABEL` advice to the inline form.

## R5: `WHERE {a, b}` double-counts

**PR:** section 7 (line 160). The PR also says IRIS rewrites it to `%OR`.

**Repro:** R5a (`WHERE {asthma, diabetes}`) and R5b (`%FILTER %OR(...)`) both give 139. Asthma is 83, diabetes 59, and both 3, so 139 is the union. HoleFoods `WHERE {Asia, Europe}` equals its `%OR` form (571,595).

**Source:**

- InterSystems' own test reference `.../TestPatients/Test1Tests/MDXQueries/TestFilters/TestDataFilters/reference.log` gives `WHERE {mold,fish}` as 5 (one patient counted twice) against 4 for `%OR` in //iris/2022.1.x and 2023.1.x.
- Change 6049443 (DP-421810, "Engine optimization: slicer decouple", 2023-06-02) changed that reference to 4. It added `Query/axis.xml:201-222`, which converts every set directly under a slicer to an orset. It is in 2023.3.0 and 2024.1.x onward, not in 2023.1.x or 2023.2.0.
- Change 7103128 (DP-429168, 2024-08-08, 2024.3.0 and later) wraps the whole slicer in `%OR` (`Query/query.xml:556-574`).
- Neither conversion applies to a slicer that contains a measure (the `'%HasMeasure` guard).

**Docs:** `D2RMDX/D2RMDX.xml:2460-2473` still says the set form "can double-count items" and gives 56/59 against 55/57.

**Verdict:** version change. Double counting was real through 2023.2 and fixed in 2023.3. The skill's floor is 2024.1, so the warning is stale for every version it covers. Its statement that IRIS rewrites the set to `%OR` is correct. Not a discrepancy with the PR; the docs need the fix.

## R6: `%OR` row label

**PR:** section 7 (lines 177-182), where the label is `asthma+` with count 119.

**Repro:** `R6: ...rows=1; asthma...diabetes: 139|44.83`. Other orders on the same cube:

| `%OR` set            | Row label           |
| -------------------- | ------------------- |
| `{asthma, diabetes}` | `asthma...diabetes` |
| `{asthma, CHD}`      | `asthma+`           |
| `{CHD, asthma}`      | `CHD...asthma`      |
| `{diabetes, asthma}` | `diabetes+`         |

**Source:** `Query/setFunction.xml:7003-7014` (ORSET, #38):

```objectscript
ElseIf ((tLastName'="")&&(tLastName'=tName)) { Set tName = tName_"..."_tLastName }
Else { Set tName = tName _ $S((tName'="")&&(tCount>1):"+",1:"") }
```

`tName` is the first member as written. `tLastName` is the member with the highest axis-node key (lines 6947-6953), which follows the dimension table IDs. Here CHD is 2, asthma 3 and diabetes 4, which matches every label above. The IDs are assigned when the cube is built, so Asaf's build could label `{asthma, diabetes}` as `asthma+`.

**Older branches:** Caché 2013.1 and earlier used `name+Others`. The `...` and `+` forms came in change 1537804 (2013-11-25) and have not changed.

**Docs:** `D2RMDX/D2RMDX.xml:2481` still shows the pre-2014 `ant bites+Others` output.

**Verdict:** both labels are real. The skill should say that the label depends on member order and build-time IDs, and should not present `asthma+` as the normal form.

## R7: Channel member keys

**PR:** section 8 (line 226).

**Repro:** `&[2]` gives 846,001. `&[Online]` gives an empty cell, with no error. `PROPERTIES("KEY")` gives No Channel `<null>`, Online `2`, Retail `1`. The name form `[Online]` also returns 846,001.

**Source:**

- HoleFoods cube: `<level name="Channel Name" sourceProperty="Channel" nullReplacement="No Channel">` with `<property name="Name" sourceProperty="Channel" useDisplayValue="true" isName="true"/>`. `HoleFoods.Transaction.Channel` is `%String(DISPLAYLIST=",Retail,Online", VALUELIST=",1,2")`.
- `Generator.xml:636-642`: an `isName` property sets the caption ("do not let name override key"). The key stays the logical value.
- `Generator.xml:3980-3997` applies a `rangeExpression` before the value is stored, so on such a level the replacement text is the key. The Discount Type keys are `None`, `1-19%`, and so on.

**Docs:** `D2RMDX/D2RMDX.xml:6904`: the key is the "Source value of the member (which is also used as the member name by default)". `D2MODEL/D2MODEL.xml:2972` names the Channel Name level as an example of "Use as member names".

**Older branches:** the same Channel definition in //iris/2022.1.x `databases/samples/cls/HoleFoods/Cube.xml#1` and in GitHub `intersystems/Samples-BI` (one commit, 2020).

**Verdict:** the example is right and my 131 edit that removed it was wrong. The rule behind it is not "integer-keyed dimensions": the key is the level's source value after any range expression, and an `isName` property makes the caption differ from it. Suggested wording: "`&[key]` uses the level's source value. When the level has an `isName` property the caption differs from the key; check with `.PROPERTIES("KEY")`. A wrong key returns an empty cell, not an error."

## R8: a dimension from another cube

**PR:** section 13 (lines 435-439).

**Repro:**

```text
R8a (on an axis): cols=[Count] rows=0 cells: 1000000
R8b (%FILTER):    PREPARE ERROR ERROR #5001: %GetDimensionInfo: Invalid Member spec: HOLEFOODS:[GenD].[H1].[Gender].&[Female]
```

A member key from another cube, or a key that does not exist (`&[Candy]`), fails at prepare with Invalid Member spec, whether it is on an axis, in `WHERE` or in `%FILTER`. `.MEMBERS` of a dimension from another cube, or of one that does not exist (`[NoSuchDim]`), runs, and the axis is dropped: R8a returns the cube total. With `NON EMPTY` the result is `*`. One earlier run of R8a printed one empty-label row with an empty cell; I could not reproduce that.

**Source:**

- The error comes from `%GetDimensionInfo` (`Utils.xml:5680`), reached through `%IsFactEnabled` (`Utils.xml:13040`) and `%IsExecutable` (`Query/query.xml:578`).
- The dropped axis comes from `%LookupCalculatedMember` (`query.xml:5296-5325`, change 2805849; 2024.1.x line 5321). For an unknown dimension it builds the set of that dimension's calculated members, which is empty.

**Docs:** `D2GMDX/D2GMDX.xml:616-640` says a mistyped member or dimension is ignored and returns null. `D2RMDX/D2RMDX.xml:365-424` says a query reaches another cube only through a relationship.

**Verdict:** documented behaviour, and not specific to other cubes. The skill should split it: a bad member key errors, and a bad dimension's `.MEMBERS` vanishes without an error. The docs have a gap of their own: they promise `*` for a nonexistent member, and a nonexistent `&[key]` raises an error instead.

## R9: unknown measure

**PR:** section 13.

**Repro:** `R9: EXECUTE ERROR ERROR #5001: Measure not found: HOLEFOODS:Nope`. `%PrepareMDX` returns OK and the error comes from `%Execute`.

**Source:** `%Execute` → `%InitializeResultsCache` (`ResultSet.xml:1000`) → `%PreProcessQuery` (`Query/query.xml:3011`, `3162`) → `memberSpec.%PreProcess` (`Query/memberSpec.xml:253`) → `%SpecToMember`, which raises the error at `memberSpec.xml:774`. A cube that does not exist fails earlier, at `%PrepareMDX` (`Parser.xml:828`, "Cannot find Subject Area"). Same in 2024.1.x.

**Docs:** `D2GMDX/D2GMDX.xml:616-625` gives the example `ERROR #5001: Measure not found: pat count`.

**Verdict:** claim right. Code that checks only the status of `%PrepareMDX` misses it.

## R10: compound `NOW` offset on a month level

Not a PR claim. The PR's compound example `[NOW-4y3m2d]` is on DaySold, where it works. I found this one while checking it.

**Repro:**

```text
R10a [MonthSold].[NOW-1y10m]: Aug-2026
R10b [MonthSold].[NOW-22m]:   Nov-2024
R10c [MonthSold].[NOW-1m]:    Aug-2026
```

On MonthSold, `[NOW-1y]` and `[NOW-2y]` give Aug-2026 and Jul-2026, and on YearSold `[NOW-30m]` gives 1996. DaySold parses every form correctly.

**Source:**

- `Query/memberSpec.xml:135` passes the raw `NOW...` token to the level class's `%ValueToKey`.
- `Time/MonthYear.xml:106`: `Set tMonth = tMonth - $P(pValue,"-",2)`. ObjectScript reads `"1y10m"` as the number 1, so the offset is one month. The `+` branch at lines 98-99 has the same flaw. `Time/Year.xml:96-101` does the same in years.
- `Time/DayMonthYear.xml:87-100` is the only level class that parses units. It calls `%DeepSee.Utils.%AddTimeInterval` (`Utils.xml:10261-10336`, "The interval is of the form "99y99m99d"").

**Docs:** `D2RMDX/D2RMDX.xml:6976-6978` gives the general form `[NOW-integer]`. Line 7047 limits the year, month and day combinations to "a level that is based on the DayMonthYear time function". No doc says what other levels do with unit letters.

**Older branches:** the MonthYear block is byte-identical in //iris/2022.1.x, 2024.1.x and latest and dates from change 2805849 (2017).

**Verdict:** documented limitation with a silent wrong answer. IRIS should reject unit letters on a level that does not parse them. Suggested skill line: "Compound `NyNmNd` offsets work only on DayMonthYear levels. On other levels use `[NOW-n]` in the level's own unit; unit letters there are silently misread."

## S1: `iris_info what=sa_schema`

**PR:** checklist and lines 96, 139 and 522 tell the agent to call `iris_info(what=sa_schema, name=HoleFoods)` for cube structure.

**Repro:** `GET /api/atelier/v8/IADSBI/saschema/HoleFoods` returns 404. `.../saschema/http://www.intersystems.com/deepsee` returns 200 with the XData grammar for cube definitions, which names no cube.

**Source:** `//iris/latest/databases/sys/cls/Api/Atelier/v2.xml#12` line 1365, `GetSASchemaDefinition`, marked Internal: "returns the textual definition of a Studio Assist Schema. Pass the url of the schema namespace". It reads `%Studio.SASchemaUtil` (`Studio/SASchemaUtil.xml#6`), which knows only XML namespace URLs.

**iad history:** on `master`, which PR 142 is based on, the tool description says "what=sa_schema returns SQL Analytics schema" (`src/tools/mod.rs`, since a61cc41) and the `name` field says "Schema/cube name". iad also encoded the slashes, so the call returned 404 for every input, including a correct URL. Commit ecd61f3 on 131 fixes both; it is not on `master` yet.

**Verdict:** iad's fault. Asaf did what iad's schema told him to do. The documented ways to list cubes and levels are `POST /api/deepsee/v1/<ns>/Info/Cubes` and `Info/Filters/<cube>` (`D2CLIENT/D2CLIENT.xml`), and `%DeepSee.Utils` `%GetCubeList` / `%GetDimensionList` through `iris_execute`.

## Other bugs found along the way

None is in the PR. They came up while tracing R1, R4 and R5 on 2026.2.

- The `%MDX` error swallowing under R1.

- `D2RMDX/D2RMDX.xml:1753` puts `%LABEL((...CurrentMember.PrevMember, MEASURES.[units sold]),"Units (Prev Period)")` inside a `WITH MEMBER` body. On 2026.2 that returns `%COUNT` (11,719 for Dec-2021) instead of Units Sold (19,712), headed with the member's name. Not traced in source.
- `WHERE {(asthma, MEASURES.[%COUNT]), (diabetes, MEASURES.[%COUNT])}` returns 166, the first tuple's value times the number of tuples. Reversed it returns 118 (2 × 59). Possibly the `'%HasMeasure` guard that skips the orset conversion; not confirmed in source.
