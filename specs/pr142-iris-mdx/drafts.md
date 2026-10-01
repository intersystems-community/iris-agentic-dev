# PR 142: IRIS bug and doc-fix drafts

Drafts only. None is filed. Evidence for each is in `evidence.md`; the repro class is `crates/iris-agentic-dev-core/tests/fixtures/mdx142/IADRepro.MDX142.cls`. Source paths are under `//iris/latest/databases/sys/cls/DeepSee/` at change 9628818; doc paths are under `//learning/iris-doc/latest/doc/cache/en-us/src/`.

## IRIS bugs

### B1. A query containing `%MDX` returns nothing after its first run (2026.2)

On 2026.2.0L build 208U, Samples-BI:

```objectscript
Do ##class(IADRepro.MDX142).Run("R1b")
```

Run 1 of a new query text returns 10698212.77. Run 2 of the same text returns `%GetAxisCount()` = 0, `%Print()` shows `*`, and `%Execute` returns OK. The same subquery run as plain MDX returns the value both times (R1d).

Cause, from source: `%PreMDX` sets `%mustCompute`. On run 2 `%CheckResultsCache` flags must-compute, the DP-443341 sanitize in `%InitializeResultsCache` (`Query/query.xml:2983-2991`, change 8846331) deletes `size` and `cells`, and `%ExecuteAxes` takes the "axes are valid" exit at `query.xml:3943-3957` before `size` is written at line 3987. Up to 2026.1, WAL085 set `%recomputeAxes = 1` on this path; change 8830710 removed it. Not measured on 2026.3.x, which also has DP-450485.

### B2. A failing `%MDX` subquery shows `*` and no error (2024.1 and later)

`SELECT %MDX("SELECT MEASURES.[NoSuchMeasure] ON 0 FROM HoleFoods") ON 0 FROM HoleFoods` returns `*` with an OK status. `Query/setFunction.xml:5221-5228` (2024.1.x: 5051-5057) runs `Set tSC = $$$OK` before `Set tChild.value = $System.Status.GetErrorText(tSC)`, so the error text is always empty.

### B3. A compound `NOW` offset on a month level reads only the month part

`[DateOfSale].[Actual].[MonthSold].[NOW-1y10m]` returns the same member as `[NOW-1m]` (R10a, R10c), with no error. `MonthYear.xml:106` computes `tMonth - $P(pValue,"-",2)` and ignores the year part. The docs (`D2RMDX/D2RMDX.xml:7047`) describe compound offsets for day levels only, so the request is an error for an offset the level cannot read, not a silent wrong member. Unchanged since change 2805849.

### B4. A slicer set of tuples that each contain a measure multiplies the first tuple's value

`WHERE {(a, M), (b, M)}` returns the first tuple's value times the number of tuples (166 = 2 × 83 on Patients). Measured on 2026.2; not traced in source.

## Doc fixes

### D1. `MEASURES.MEMBERS` includes `%COUNT` (`D2GMDX/D2GMDX.xml:465`, `1004`)

Both places say `MEASURES.MEMBERS` returns the measures defined in the cube, without `%COUNT`. The generator adds `%COUNT` to the measure list (`Generator.xml:757-765`, `memberMeasure.xml:130-141`), and `SELECT MEASURES.MEMBERS ON 0 FROM HoleFoods` returns `Count` first. Unchanged since Caché 2011.

### D2. State that measures may appear on one axis only (`D2RMDX/D2RMDX.xml:4694-4735`)

`query.xml:700-707` raises "Measures cannot exist on multiple axes", and `%IsMeasure` counts `MAX(set, measure)`, `%MDX` and other value functions with a measure argument as measures. The `MAX` examples are correct but never state the rule, so `{members, MAX(members, measure)}` on rows with a measure on columns looks valid.

### D3. `WHERE {a, b}` no longer double-counts (`D2RMDX/D2RMDX.xml:2460-2473`)

The warning describes behaviour before 2023.3. DP-421810 (change 6049443, `axis.xml:201-222`) fixed it, and DP-429168 (change 7103128, `query.xml:556-574`) wraps the set in `%OR` from 2024.3. Neither applies to a slicer whose tuples contain a measure (see B4).

### D4. `%OR` row label (`D2RMDX/D2RMDX.xml:2483`)

The example shows the label `ant bites+Others`. No DeepSee source file on latest contains `Others`: `setFunction.xml:7003-7014` builds `first...last` when the group has a last member name that differs from the first, and `first+` otherwise. Samples-BI builds give either for the same query.

### D5. A nonexistent member key raises an error (`D2GMDX/D2GMDX.xml:616-640`)

"Nonexistent Members" says a mistyped member returns `*`. A mistyped name does; a nonexistent `&[key]` fails at prepare with `%GetDimensionInfo: Invalid Member spec` (`Utils.xml:5680`).

### D6. `%LABEL` inside `WITH MEMBER` example returns `%COUNT` (`D2RMDX/D2RMDX.xml:1753`)

The example returns the `%COUNT` value (11,719) instead of Units Sold (19,712) on 2026.2. Either the example or the engine is wrong; not traced in source.
