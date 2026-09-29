---
author: asinay
description:
  Use when writing, debugging, or optimizing MDX queries against InterSystems
  IRIS BI cubes — engine choice (MDX vs SQL), hierarchy path syntax, NON EMPTY,
  %FILTER, %OR, calculated members, time series, and common traps. Load explicitly
  for MDX/BI work; do NOT load globally for general ObjectScript tasks.
iris_version: ">=2024.1"
name: iris-mdx
state: draft
tags:
  - iris
  - mdx
  - analytics
  - bi
  - quirks
trigger: Writing, debugging, or optimizing MDX queries against InterSystems IRIS
  BI cubes — engine choice (MDX vs SQL), hierarchy paths, NON EMPTY, %FILTER, %OR,
  calculated members, or time series patterns.
---

# IRIS MDX — Quirks and Patterns for IRIS BI Cube Queries

## HARD GATE

Before writing any IRIS MDX, check these. Every one produces silent wrong results.

- [ ] **Discover cube structure first** — list cubes with `%DeepSee.Utils` `%GetCubeList` and spec paths with `%GetDimensionList`, through `iris_execute` (§14), before writing a single line of MDX
- [ ] **Use exact spec paths** — wrong hierarchy name returns an empty-member row with null value, no error: `[Outlet].[H1].[Region]` not `[Region].[Region]`
- [ ] **NON EMPTY on every axis** — without it, all members are returned regardless of filter
- [ ] **Side-by-side comparison** — put both members in a set `{.&[2023], .&[2024]}` on the axis; two `%FILTER` on the same level ANDs them → null data
- [ ] **Member keys are not always captions** — the level definition decides the key; a caption where the key is an id returns null, no error; discover with `CURRENTMEMBER.PROPERTIES("KEY")`
- [ ] **`%COUNT` is the correct count measure** — never invent names like `Patient Count` or `Transaction Count`
- [ ] **Dimensions belong to one cube** — another cube's dimension in `%FILTER` fails with "Invalid Member spec", but on an axis it returns no rows and no error

---

## 1. MDX vs SQL — When to Use Each

| Use MDX when…                                | Use SQL when…                            |
| -------------------------------------------- | ---------------------------------------- |
| You need aggregated totals, averages, counts | You need individual rows / raw records   |
| The data is modelled in a cube               | The data is only in source tables        |
| You need time-series trends                  | You need JOINs not represented in a cube |
| You need cross-dimensional slicing           | You need to write data (INSERT/UPDATE)   |

**Always check `%GetCubeList` first (§14)** — if an IRIS BI cube exists for the data, prefer MDX for aggregation questions.

---

## 2. Hierarchy Path Syntax — The #1 Source of Wrong Results

MDX dimension references must use the **exact spec path** from the cube definition. A wrong path returns an empty-member row with null value — no error.

```mdx
-- CORRECT: full spec path
SELECT {MEASURES.[Amount Sold]} ON 0,
NON EMPTY [Outlet].[H1].[Region].MEMBERS ON 1
FROM HoleFoods

-- WRONG: invented path — returns empty-member row with null, no error
SELECT {MEASURES.[Amount Sold]} ON 0,
NON EMPTY [Region].[Region].MEMBERS ON 1
FROM HoleFoods
```

**How to get spec paths:** `%DeepSee.Utils` `%GetDimensionList(cube, .info)` through `iris_execute` — see §14. Each `info(d,h,l)` is `$LB(type, DimName, HierName, LevelName)`, which is the spec path `[DimName].[HierName].[LevelName]`.

**Shorthand is allowed** (but use full paths to avoid ambiguity):

```mdx
[GenD].[H1].[Gender].Female -- full
[GenD].[H1].Female -- omit level name
[GenD].Female -- omit hierarchy and level
GenD.Female -- omit brackets when name is alphanumeric
```

Some examples in §8 and §12 use short forms for brevity. In generated queries, use the full path.

---

## 3. NON EMPTY — Always Suppress Empty Members

Without `NON EMPTY`, every member in the level is returned — including those with no data in the current filter context.

```mdx
-- WRONG: returns all 12 months even when filtered to a single year with sparse data
SELECT {MEASURES.[Amount Sold]} ON 0,
[DateOfSale].[Actual].[MonthSold].MEMBERS ON 1
FROM HoleFoods

-- CORRECT: only months that have data
SELECT {MEASURES.[Amount Sold]} ON 0,
NON EMPTY [DateOfSale].[Actual].[MonthSold].MEMBERS ON 1
FROM HoleFoods
```

**Rule:** put `NON EMPTY` before every `.MEMBERS` axis expression.

---

## 4. %FILTER and WHERE are Equivalent in IRIS

`WHERE` and `%FILTER` produce identical MDXText internally — the engine rewrites both to the same `WHERE` clause. Both work correctly for all filter patterns.

```mdx
-- These two queries produce identical results:
SELECT {MEASURES.[Amount Sold]} ON 0,
NON EMPTY [DateOfSale].[Actual].[MonthSold].MEMBERS ON 1
FROM HoleFoods
WHERE [DateOfSale].[Actual].[YearSold].&[2024]

SELECT {MEASURES.[Amount Sold]} ON 0,
NON EMPTY [DateOfSale].[Actual].[MonthSold].MEMBERS ON 1
FROM HoleFoods
%FILTER [DateOfSale].[Actual].[YearSold].&[2024]
```

**Prefer `%FILTER`** for programmatic query building — easier to append conditions incrementally, and composes cleanly with `%OR`.

Multiple `%FILTER` clauses chain as AND (across **different** dimensions):

```mdx
-- Revenue for Asia region, Snack category only
SELECT MEASURES.[Amount Sold] ON 0,
NON EMPTY [Product].[P1].[Product Category].MEMBERS ON 1
FROM HoleFoods
%FILTER [Outlet].[H1].[Region].&[Asia]
%FILTER [Product].[P1].[Product Category].&[Snack]
```

---

## 5. Side-by-Side Comparison — Never Double %FILTER on Same Level

Two `%FILTER` on the same dimension AND together. Year=2023 AND Year=2024 simultaneously = impossible → returns an empty-member row with null values, no error.

```mdx
-- WRONG: AND logic → empty-member row, null values
SELECT {MEASURES.[Amount Sold]} ON 0,
NON EMPTY [Product].[P1].[Product Category].MEMBERS ON 1
FROM HoleFoods
%FILTER [DateOfSale].[Actual].[YearSold].&[2023]
%FILTER [DateOfSale].[Actual].[YearSold].&[2024]

-- CORRECT: both years as a set on the axis — side-by-side columns
SELECT {MEASURES.[Amount Sold]} ON 0,
NON EMPTY {[DateOfSale].[Actual].[YearSold].&[2023],
[DateOfSale].[Actual].[YearSold].&[2024]} ON 1
FROM HoleFoods
```

**Rule:** for any "compare A vs B" question, put both members in a set `{m1, m2}` on an axis.

---

## 6. %OR — OR Without Double-Counting

On IRIS, `WHERE {a, b}` and `%FILTER %OR({a, b})` return the same count, with no double-counting. Use `%OR` anyway: it states the intent and composes with further `%FILTER` clauses.

```mdx
-- OR two members — a patient with both is counted once
SELECT MEASURES.[%COUNT] ON 0
FROM Patients
%FILTER %OR({[DiagD].[H1].[Diagnoses].&[asthma],
[DiagD].[H1].[Diagnoses].&[diabetes]})

-- AND of ORs — chain %FILTER with %OR inside each clause
SELECT MEASURES.[%COUNT] ON 0
FROM Patients
%FILTER %OR({[ColorD].[H1].[Favorite Color].&[Orange],
[ColorD].[H1].[Favorite Color].&[Purple]})
%FILTER [GenD].[H1].[Gender].&[Female]
-- Result: Female AND (Orange OR Purple)

-- %OR on an axis — combines members into one row, labelled "asthma...diabetes"
SELECT {MEASURES.[%COUNT], MEASURES.[Avg Age]} ON 0,
NON EMPTY %OR({[DiagD].[H1].[Diagnoses].&[asthma],
[DiagD].[H1].[Diagnoses].&[diabetes]}) ON 1
FROM Patients
-- Returns one row with the union's count and average age
```

---

## 7. NONEMPTYCROSSJOIN — AND Across Dimensions

`NONEMPTYCROSSJOIN(setA, setB)` returns the cross-product of two sets restricted to non-empty cells — the AND-across-dimensions counterpart to `%OR`'s OR-within-a-dimension. Use it inside `%FILTER`/`WHERE` instead of a tuple when either side is a multi-member set.

```mdx
-- Patients with mold allergy AND orange as favorite color
SELECT MEASURES.[%COUNT] ON 0
FROM Patients
%FILTER NONEMPTYCROSSJOIN([AllerD].[H1].[Allergies].&[mold],
[ColorD].[H1].[Favorite Color].&[Orange])
```

**Combine with `%OR` for "(A AND B) OR (C AND D)"** — not expressible with a plain tuple:

```mdx
SELECT FROM Patients
%FILTER %OR({
NONEMPTYCROSSJOIN([AllerD].[H1].[Allergies].&[mold],
[ColorD].[H1].[Favorite Color].&[Orange]),
NONEMPTYCROSSJOIN([AllerD].[H1].[Allergies].&[cat hair],
[ColorD].[H1].[Favorite Color].&[Purple])
})
```

A plain tuple `(a, b)` is simpler and gives the same result for a single AND pair — reach for `NONEMPTYCROSSJOIN` when either side is itself a set, or when composing with `%OR`.

---

## 8. Member Key Syntax — Captions vs Keys

Member key syntax: `[Dim].[Hier].[Level].&[key]`

**The key is not always the display name.** The level definition decides it, not the type of the source column:

- A level whose source is an id, with the caption from a separate name property, is keyed by the id. `&[12]` works; `&[Jones]` returns null, no error.
- A level that maps codes to names with `rangeExpression` (for example `1:Retail;2:Online;`) is keyed by the mapped name. `&[Online]` works; `&[2]` returns null, no error.
- A level whose source is the caption itself is keyed by the caption.

```mdx
-- Caption is the key
[Outlet].[H1].[Region].&[Asia]
[DateOfSale].[Actual].[YearSold].&[2024]

-- Id-keyed level with a name property
[DocD].[H1].[Doctor].&[12] -- CORRECT for "Jones"
[DocD].[H1].[Doctor].&[Jones] -- WRONG — null, no error

-- The null-keyed member (records with no value for this level):
[Channel].[H1].[Channel Name].&[<null>] -- or shown as "No Channel" in results
```

**Discover actual keys before filtering:**

```mdx
SELECT [Channel].[H1].CURRENTMEMBER.PROPERTIES("KEY") ON 0,
[Channel].[H1].[Channel Name].MEMBERS ON 1
FROM HoleFoods
-- One row per member, with its key in the cell
```

**Member names are not required to be unique.** Referring to a member by caption (`docd.doctor.[Smith]`) silently returns the _first_ matching member if two share a name — no error, no warning. Use `&[key]` for an unambiguous reference whenever a level could plausibly have duplicate captions:

```mdx
-- Ambiguous — returns first "Smith" found
docd.h1.doctor.[Smith]

-- Unambiguous — uses the stored key
docd.h1.doctor.&[42]
```

---

## 9. % Prefix — IRIS Extensions

Any MDX keyword starting with `%` is an InterSystems extension.

| Extension                     | Use instead of         | Why                                                                  |
| ----------------------------- | ---------------------- | -------------------------------------------------------------------- |
| `%OR({a, b})`                 | `{a, b}` in WHERE      | Explicit union, composes with `%FILTER`                              |
| `%NOT`                        | `EXCEPT`               | Single-member exclusion, no intermediate set. e.g. `.&[asthma].%NOT` |
| `%FILTER`                     | `WHERE`                | Composable, chains as AND, same semantics in IRIS                    |
| `%COUNT`                      | invented count names   | Native fact count, always present in every cube                      |
| `%TIMERANGE(s, e)`            | `{start:end}` colon    | Open-ended ranges, INCLUSIVE/EXCLUSIVE control                       |
| `%MDX("SELECT FROM cube")`    | No standard equivalent | Scalar subquery immune to cell context                               |
| `%LAST(set, measure)`         | Manual iteration       | Last non-null value across time — use for snapshot measures          |
| `%CELL(col, row)`             | No standard equivalent | Positional cell reference for running totals                         |
| `%LABEL(member, caption, "")` | No standard equivalent | Override auto-generated column header                                |

**Percentage literals need no space before `%`:** `{10%, 20%, 30%}` is valid; `{10 %, 20 %}` is not.

**`MEASURES.MEMBERS` includes `%COUNT`**, headed "Count", alongside every measure defined in the cube. `{MEASURES.[%COUNT], MEASURES.MEMBERS}` shows the count twice.

---

## 10. Calculated Members — WITH MEMBER Patterns

No comma between multiple `WITH` clauses:

```mdx
WITH MEMBER MEASURES.[a] AS '...'
MEMBER MEASURES.[b] AS '...' -- no comma before MEMBER
SELECT ...
```

### Percent-of-total with %MDX()

`%MDX()` returns a scalar from a separate query, immune to the current cell context. On an axis by itself it returns the subquery's value; inside `WITH MEMBER` it combines with the cell's own measure, as below.

```mdx
WITH MEMBER MEASURES.[Pct] AS
'100 \* MEASURES.[Amount Sold] / %MDX("SELECT MEASURES.[Amount Sold] ON 0 FROM HoleFoods")'
SELECT {MEASURES.[Amount Sold], MEASURES.[Pct]} ON 0,
NON EMPTY [Outlet].[H1].[Region].MEMBERS ON 1
FROM HoleFoods
-- One row per region: its amount and its percent of the total
```

### Period-over-period with PrevMember

```mdx
WITH MEMBER MEASURES.[PrevUnits] AS
'([DateOfSale].[Actual].CurrentMember.PrevMember, MEASURES.[Units Sold])'
SELECT {MEASURES.[Units Sold],
%LABEL(MEASURES.[PrevUnits], "Units (Prev Month)", "")} ON 0,
NON EMPTY [DateOfSale].[Actual].[MonthSold].MEMBERS ON 1
FROM HoleFoods
```

**Bug:** without `%LABEL`, the auto-generated header for a PrevMember measure shows the dimension name (`DateOfSale`) instead of the measure name. Always use `%LABEL` on PrevMember calculated measures.

### YTD / rolling window

```mdx
-- Last 90 days rolled into one member
WITH MEMBER CalcD.[Last90] AS
'%OR([DateOfSale].[Actual].[DaySold].[NOW-90]:[DateOfSale].[Actual].[DaySold].[NOW])'
SELECT MEASURES.[Amount Sold] ON 0, CalcD.[Last90] ON 1
FROM HoleFoods

-- Year-to-date through today
WITH MEMBER CalcD.[YTD] AS
'%OR(PERIODSTODATE([DateOfSale].[Actual].[YearSold],
[DateOfSale].[Actual].[DaySold].[NOW]))'
SELECT MEASURES.[Amount Sold] ON 0, CalcD.[YTD] ON 1
FROM HoleFoods
```

---

## 11. FILTER, ORDER, and Aggregation Functions

### COUNT — includes null members unless told otherwise

`COUNT(set)` counts every element of the set, including members with no data in the current context. Use `EXCLUDEEMPTY` to count only members that actually have data:

```mdx
-- Counts every diagnosis member, including those with no patients in this context
COUNT([DiagD].[H1].[Diagnoses].MEMBERS)

-- Counts only diagnoses with patients in this context
COUNT([DiagD].[H1].[Diagnoses].MEMBERS, EXCLUDEEMPTY)
```

### FILTER — aggregate HAVING, not row-level WHERE

`FILTER(set, condition)` evaluates the condition against each member's **aggregated** value:

```mdx
-- Regions where total revenue > 2000 — equivalent to SQL HAVING
SELECT MEASURES.[Amount Sold] ON 0,
NON EMPTY FILTER([Outlet].[H1].[Region].MEMBERS,
MEASURES.[Amount Sold] > 2000) ON 1
FROM HoleFoods
-- Returns only the regions above 2000
```

### ORDER — preserve vs break hierarchy

```mdx
-- DESC: sort within parent groups (ZIP parent stays with its city children)
ORDER([HomeD].[H1].MEMBERS, MEASURES.[%COUNT], DESC)

-- BDESC: sort globally — hierarchy broken, flat ranked list
ORDER([HomeD].[H1].MEMBERS, MEASURES.[%COUNT], BDESC)
```

Use `BDESC`/`BASC` for ranked lists. Use `DESC`/`ASC` when parent-child grouping must be preserved.

### Measures on one axis only

Measures on two axes raise "Measures cannot exist on multiple axes". Appending `MAX(set, measure)` to the member set on rows, with a measure on columns, raises the same error, because the `MAX` names a measure.

### Axis skipping — ROWS without COLUMNS

IRIS allows omitting ON 0 entirely. The implicit column is `%COUNT`:

```mdx
SELECT [GenD].[H1].[Gender].MEMBERS ON ROWS FROM Patients
-- Returns Female/Male rows; column header is empty string (not a measure name)
```

**The implicit measure column's header is `""`, not `"%COUNT"`.** Code that reads result columns by name will see an empty string for that column — don't assume the header text matches the measure name.

---

## 12. Time Navigation

### NOW member — relative offsets

```mdx
[DateOfSale].[Actual].[DaySold].[NOW] -- today
[DateOfSale].[Actual].[DaySold].[NOW-30] -- 30 days ago
[DateOfSale].[Actual].[YearSold].[NOW-1] -- last year
[DateOfSale].[Actual].[DaySold].[NOW-4y3m2d] -- compound offset
```

**Only works on timeline-based levels** (`YearSold`, `MonthSold`, `DaySold`). Does not work on date-part levels (`Quarter`, `Month` — fixed cycle members).

### Timeline-based vs date-part levels

| Type            | Example members       | PREVMEMBER crosses parent boundary?                   |
| --------------- | --------------------- | ----------------------------------------------------- |
| Timeline-based  | `Q1 2024`, `Jan 2024` | Yes — Q1 2024 PREVMEMBER → Q4 2023                    |
| Date-part-based | `Q1`, `January`       | No — Q1 PREVMEMBER → null (no "before Q1" in a cycle) |

Use **timeline-based** levels for period-over-period comparisons.
Use **date-part** levels for grouping across all years (e.g. "all January months combined").

### COUSIN — same relative position under a different parent

`COUSIN(member, new_parent)` finds a member's position within its own parent, then returns the member at that same position under a different parent. Useful for period comparisons that `PrevMember`/`PREVMEMBER` can't express directly, e.g. "the same quarter in a different year":

```mdx
-- Q1 1943 → same relative position in year 1990 → Q1 1990
COUSIN([BirthD].[Q1 1943], [BirthD].1990)
```

---

## 13. Common Silent Failures

| Situation                                                          | Result                                                             | How to diagnose                                          |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | -------------------------------------------------------- |
| Wrong hierarchy path                                               | Empty-member row, null value, no error                             | Verify spec path with `%GetDimensionList` (§14)          |
| Typo in dimension name                                             | Dimension silently ignored, unexpected totals                      | Compare result to a known count                          |
| Wrong member key (caption where the level is keyed by id)          | Null / no data, no error                                           | Run `CURRENTMEMBER.PROPERTIES("KEY")` query first        |
| Ambiguous member caption (duplicate name in a level)               | Returns first match, no error                                      | Use `&[key]` instead of the caption                      |
| Two `%FILTER` on same level                                        | Empty-member row, null value, no error                             | Use set `{m1, m2}` on axis                               |
| Cross-cube dimension reference in `%FILTER`                        | `Invalid Member spec` at `%PrepareMDX`                             | Each query targets one cube only                         |
| Cross-cube dimension reference on an axis                          | No rows, no error                                                  | Each query targets one cube only                         |
| Measures on two axes, including `MAX(set, measure)` beside members | `ERROR: Measures cannot exist on multiple axes`                    | Keep every measure on one axis                           |
| Nonexistent measure                                                | `ERROR #5001: Measure not found`, at `%Execute`, not `%PrepareMDX` | Check measure names under DimNo 0 of `%GetDimensionList` |
| Nonexistent cube                                                   | `ERROR #5001: Cannot find Subject Area`, at `%PrepareMDX`          | Check available cubes via `%GetCubeList`                 |

---

## 14. IRIS BI Cube Discovery Workflow

iad has no MDX tool, and `iris_query` runs SQL only. Discovery and queries both go through `iris_execute`. The snippets use block syntax, which works on the HTTP path; on a `docker_only` connection, put them in a routine.

Always run discovery before writing MDX against an unfamiliar cube:

```objectscript
// Step 1 — list cubes: list(NAME) = $LB(name, caption, moddate, type)
Set sc = ##class(%DeepSee.Utils).%GetCubeList(.list)
Set k = "" For { Set k = $Order(list(k)) Quit:k=""  Write $ListGet(list(k),1),! }

// Step 2 — spec paths and measures: info(d,h,l) = $LB(type, DimName, HierName, LevelName)
// type is "d", "h", "l", "m" (measure, under d=0), "r" or "all"
Set sc = ##class(%DeepSee.Utils).%GetDimensionList("HoleFoods", .info)
Set d = "" For { Set d = $Order(info(d)) Quit:d=""
    Set h = "" For { Set h = $Order(info(d,h)) Quit:h=""
        Set l = "" For { Set l = $Order(info(d,h,l)) Quit:l=""  Write $ListToString(info(d,h,l)),! } } }
```

Step 3 — discover member keys (§8):

```mdx
SELECT [Dim].[Hier].CURRENTMEMBER.PROPERTIES("KEY") ON 0,
[Dim].[Hier].[Level].MEMBERS ON 1
FROM Cube
```

Step 4 — write MDX using exact spec paths and verified member keys, and run it:

```objectscript
Set rs = ##class(%DeepSee.ResultSet).%New()
Set sc = rs.%PrepareMDX("SELECT MEASURES.[Amount Sold] ON 0, NON EMPTY [Outlet].[H1].[Region].MEMBERS ON 1 FROM HoleFoods")
If $$$ISERR(sc) { Write $System.Status.GetErrorText(sc),! Quit }
Set sc = rs.%Execute()
If $$$ISERR(sc) { Write $System.Status.GetErrorText(sc),! Quit }
For r = 1:1:rs.%GetAxisSize(2) {
    Kill lab Set n = rs.%GetOrdinalLabel(.lab, 2, r)
    Write $Get(lab(1)), ": ", rs.%GetOrdinalValue(1, r),!
}
```

Check the status after both steps. An unknown cube fails at `%PrepareMDX`; an unknown measure passes prepare and fails only at `%Execute`. `rs.%Print()` writes the whole grid when you only need to look at it.

---

## EXAMPLE: Monthly Revenue for a Specific Year

```mdx
SELECT {MEASURES.[Amount Sold]} ON 0,
NON EMPTY [DateOfSale].[Actual].[MonthSold].MEMBERS ON 1
FROM HoleFoods
%FILTER [DateOfSale].[Actual].[YearSold].&[2024]
```

## EXAMPLE: Year-over-Year Comparison

```mdx
-- Both years in a set on the axis — not two %FILTER clauses
SELECT {MEASURES.[Amount Sold]} ON 0,
NON EMPTY {[DateOfSale].[Actual].[YearSold].&[2023],
[DateOfSale].[Actual].[YearSold].&[2024]} ON 1
FROM HoleFoods
```

## EXAMPLE: Revenue by Region with Percentage of Total

```mdx
WITH MEMBER MEASURES.[Pct] AS
'100 \* MEASURES.[Amount Sold] / %MDX("SELECT MEASURES.[Amount Sold] ON 0 FROM HoleFoods")'
SELECT {MEASURES.[Amount Sold], MEASURES.[Pct]} ON 0,
NON EMPTY [Outlet].[H1].[Region].MEMBERS ON 1
FROM HoleFoods
```

## EXAMPLE: Top 5 Products by Revenue

```mdx
SELECT {MEASURES.[Amount Sold]} ON 0,
TOPCOUNT([Product].[P1].[Product Name].MEMBERS, 5, MEASURES.[Amount Sold]) ON 1
FROM HoleFoods
```

## EXAMPLE: %NOT Exclusion

```mdx
-- All patients except those with asthma
SELECT MEASURES.[%COUNT] ON 0
FROM Patients
%FILTER [DiagD].[H1].[Diagnoses].&[asthma].%NOT
```
