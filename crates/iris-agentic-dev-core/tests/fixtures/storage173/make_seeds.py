#!/usr/bin/env python3
"""Writes classes/<F>.cls and seed/<F>.json for the spec 173 fixtures from F11 on (F01 to F10 were
written by hand to the same shape). Run from this directory: `python3 make_seeds.py [F11 F12 ...]`.
Every name, value and global here is invented for this repository."""
import json
import sys

FIRST = ["Smith, Anna", "jones, bob", "Lee  ", "O'Neil-Ray", "de la Cruz, Jo", "Zed2", "  Mc Kay, Al",
         "Brown, Cy", "brown, cy", "Ng", "Åberg, Eli", "Vu, Tran", "Quinn, 3rd", "Hart,Ed", "xu",
         "Patel, Raj", "Moore, Kim ", "Diaz, Lu", "Frost, Ann-Marie", "Gray, Sam"]
CITIES = ["Boston", "boston", "Paris", "Lyon", "Oslo", "Rome", "Cork", "Bern", "Nice", "Kyiv"]
COLS = ["Name", "City", "Age", "Born", "Status"]


def people(n):
    rows = []
    for i in range(n):
        name = FIRST[i] if i < len(FIRST) else f"{FIRST[i % len(FIRST)].strip()}{i}"
        rows.append([name, CITIES[(i * 7 + 3) % len(CITIES)], 18 + (i * 13) % 70 + 100,
                     40000 + i * 37, ["A", "B", "C"][i % 3]])
    return rows


def person_class(cls, glob, extra, extra_maps):
    data = "".join(
        f'<Data name="{c}">\n<Delimiter>"^"</Delimiter>\n<Piece>{p}</Piece>\n</Data>\n'
        for c, p in sorted([("Name", 1), ("City", 2), ("Age", 3), ("Born", 4), ("Status", 5)]))
    return f"""Class {cls} Extends %Persistent [ StorageStrategy = SQLStorage ]
{{

Property Name As %String;

Property City As %String;

Property Age As %Integer;

Property Born As %Integer;

Property Status As %String;

{extra}
Storage SQLStorage
{{
<SqlIdExpression>$i(^{glob})</SqlIdExpression>
<SQLMap name="Master">
{data}<Global>^{glob}</Global>
<Subscript name="1">
<Expression>{{ID}}</Expression>
</Subscript>
<Type>data</Type>
</SQLMap>
{extra_maps}<StreamLocation>^{glob}S</StreamLocation>
<Type>%Storage.SQL</Type>
}}

}}
"""


def index_map(name, glob, subs, extra="", sub_extra=None):
    s = f'<SQLMap name="{name}">\n{extra}<Global>^{glob}</Global>\n'
    for i, e in enumerate(subs):
        more = (sub_extra or {}).get(i, "")
        s += f'<Subscript name="{i + 1}">\n<Expression>{e}</Expression>\n{more}</Subscript>\n'
    return s + "<Type>index</Type>\n</SQLMap>\n"


def write(name, desc, cls_text, rows, answer, columns=COLS, extra=None, master="", indexes=None):
    glob = f"IadSm173{name}"
    seed = {"name": name, "description": desc, "class": f"IadSm173.{name}",
            "table": f"IadSm173.{name}", "columns": columns,
            "master_global": master or f"^{glob}",
            "index_globals": indexes or [f"^{glob}X"], "rows": rows, "answer": answer}
    seed.update(extra or {})
    with open(f"classes/{name}.cls", "w") as f:
        f.write(cls_text)
    with open(f"seed/{name}.json", "w") as f:
        json.dump(seed, f, ensure_ascii=False, indent=1)
        f.write("\n")


def std(name, desc, subs, answer, rows=None, extra_props="", extra_map="", sub_extra=None, extra=None):
    glob = f"IadSm173{name}"
    cls = person_class(f"IadSm173.{name}", glob, extra_props,
                       index_map("X", f"{glob}X", subs, extra_map, sub_extra))
    ans = {"index_global": f"^{glob}X", **answer}
    write(name, desc, cls, rows or people(40), ans, extra=extra)


def with_empty(rows, col, every, value):
    out = [list(r) for r in rows]
    for i, r in enumerate(out):
        if i % every == 0:
            r[COLS.index(col)] = value
    return out


def f11():
    std("F11", "index {City}, {Age}, {ID}", ["{City}", "{Age}", "{ID}"],
        {"id_positions": [3], "subscripts": [
            {"position": 1, "field": "City", "transform": "exact"},
            {"position": 2, "field": "Age", "transform": "exact",
             "verdict": "ambiguous", "alternatives": ["alphaup", "upper"]}]})


def f12():
    glob = "IadSm173F12"
    cols = ["A", "B", "Name", "City"]
    rows = [["xyz"[i % 3], i + 1, FIRST[i] if i < len(FIRST) else f"{FIRST[i % len(FIRST)].strip()}{i}",
             CITIES[(i * 7 + 3) % len(CITIES)]] for i in range(40)]
    cls = f"""Class IadSm173.F12 Extends %Persistent [ StorageStrategy = SQLStorage ]
{{

Property A As %String;

Property B As %Integer;

Property Name As %String;

Property City As %String;

Index IDKEY On (A, B) [ IdKey, Unique ];

Storage SQLStorage
{{
<SQLMap name="Master">
<Data name="City">
<Delimiter>"^"</Delimiter>
<Piece>2</Piece>
</Data>
<Data name="Name">
<Delimiter>"^"</Delimiter>
<Piece>1</Piece>
</Data>
<Global>^{glob}</Global>
<RowIdSpec name="1">
<Expression>{{L1}}</Expression>
<Field>A</Field>
</RowIdSpec>
<RowIdSpec name="2">
<Expression>{{L2}}</Expression>
<Field>B</Field>
</RowIdSpec>
<Subscript name="1">
<Expression>{{A}}</Expression>
</Subscript>
<Subscript name="2">
<Expression>{{B}}</Expression>
</Subscript>
<Type>data</Type>
</SQLMap>
{index_map("X", glob + "X", ["$$SQLUPPER({Name})", "{A}", "{B}"])}<StreamLocation>^{glob}S</StreamLocation>
<Type>%Storage.SQL</Type>
}}

}}
"""
    write("F12", "composite key (A, B), index (Name, A, B)", cls, rows,
          {"index_global": f"^{glob}X", "id_positions": [2, 3], "subscripts": [
              {"position": 1, "field": "Name", "transform": "sqlupper"}]},
          columns=cols, extra={"master_columns": ["Name", "City"], "key_map": {"A": "Key1", "B": "ID"}})


def f13():
    extra = ('<Data name="Id"/>\n<RowIdSpec name="1">\n<Expression>{Id}</Expression>\n'
             '<Field>Id</Field>\n</RowIdSpec>\n')
    std("F13", "index $$SQLUPPER({Name}) with the ID in the node value", ["$$SQLUPPER({Name})"],
        {"id_form": "value", "id_positions": [], "subscripts": [
            {"position": 1, "field": "Name", "transform": "sqlupper"}]}, extra_map=extra)


def f14():
    extra = ('<Data name="City">\n<Piece>1</Piece>\n</Data>\n<Data name="Age">\n<Piece>2</Piece>\n</Data>\n')
    std("F14", "covering: node value is a $LIST of City and Age", ["$$SQLUPPER({Name})", "{ID}"],
        {"id_positions": [2], "subscripts": [{"position": 1, "field": "Name", "transform": "sqlupper"}],
         "data_in_value": [{"field": "City", "element": 1}, {"field": "Age", "element": 2}]},
        extra_map=extra)


def f15():
    std("F15", "empty-string City under {City}", ["{City}", "{ID}"],
        {"id_positions": [2], "subscripts": [{"position": 1, "field": "City", "transform": "exact"}],
         "null": {"position": 1, "field": "City", "kind": "c0", "empty": "empty_string", "count": 8, "sampled": 3}},
        rows=with_empty(people(40), "City", 5, ""))


def f16():
    std("F16", "NULL Age under {Age}", ["{Age}", "{ID}"],
        {"id_positions": [2], "subscripts": [{"position": 1, "field": "Age", "transform": "exact",
                         "verdict": "ambiguous", "alternatives": ["alphaup", "upper"]}],
         "null": {"position": 1, "field": "Age", "kind": "minus1e14", "empty": "null", "count": 8, "sampled": 3}},
        rows=with_empty(people(40), "Age", 5, None))


def f17():
    glob = "IadSm173.F17"
    cls = """Class IadSm173.F17 Extends %Persistent
{

Property Name As %String;

Property City As %String;

Property Age As %Integer;

Property Born As %Integer;

Property Status As %String;

Index CityIdx On City;
}
"""
    seed_rows = with_empty(people(40), "City", 5, None)
    # A space inside a city separates SQLUPPER (keeps it) from STRING (drops it).
    for i, r in enumerate(seed_rows):
        if r[1] is not None and i % 2 == 0:
            r[1] = ["Salt Lake", "New York", "San Jose", "El Paso"][i % 4]
    write("F17", "class-compiled index CityIdx on City, NULL City", cls, seed_rows,
          {"index_global": f"^{glob}I", "id_positions": [3], "subscripts": [
              {"position": 2, "field": "City", "transform": "sqlupper"}],
           "null": {"position": 2, "field": "City", "kind": "space", "empty": "null", "count": 8, "sampled": 8}},
          master=f"^{glob}D", indexes=[f"^{glob}I"],
          extra={"master_columns": ["", "Name", "City", "Age", "Born", "Status"]})


def f18():
    std("F18", "hand-written NullMarker of length 5 under {City}", ["{City}", "{ID}"],
        {"id_positions": [2], "subscripts": [{"position": 1, "field": "City", "transform": "exact"}],
         "null": {"position": 1, "field": "City", "kind": "discovered", "length": 5, "empty": "null",
                  "count": 8, "sampled": 3, "literal": "~NIL~"}},
        rows=with_empty(people(40), "City", 5, None), sub_extra={0: '<NullMarker>"~NIL~"</NullMarker>\n'})


ALL = {k[1:].join(["F", ""]) if False else "F" + k[1:]: v for k, v in list(globals().items())
       if k.startswith("f") and k[1:].isdigit()}

if __name__ == "__main__":
    for want in (sys.argv[1:] or sorted(ALL)):
        ALL[want]()
