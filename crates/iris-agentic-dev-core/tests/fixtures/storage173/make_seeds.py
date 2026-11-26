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


def f26():
    rows = [list(r) for r in people(40)]
    for r in rows:
        r[4] = r[1]  # Status holds the same text as City: two identical columns
    std("F26", "two identical columns (City, Status), index over {City}", ["{City}", "{ID}"],
        {"id_positions": [2], "link_verdict": "ambiguous", "subscripts": [
            {"position": 1, "verdict": "ambiguous", "transform": "exact", "fields": ["City", "Status"]}]},
        rows=rows)


def f27():
    std("F27", "5 rows", ["$$SQLUPPER({Name})", "{ID}"],
        {"id_positions": [2], "link_verdict": "too_few_nodes", "no_map": True, "subscripts": []},
        rows=people(5))


def f28():
    rows = [list(r) for r in people(40)]
    for r in rows:
        r[1] = "Boston"
    std("F28", "a constant column indexed", ["{City}", "{ID}"],
        {"id_positions": [2], "link_verdict": "constant", "no_map": True, "subscripts": [
            {"position": 1, "verdict": "constant"}]}, rows=rows)


def f29():
    rows = [list(r) for r in people(40)]
    for i, r in enumerate(rows):
        r[2] = i + 1  # Age equals the row ID ($i counter from 1)
    std("F29", "Age equals ID on every row, index over {Age}", ["{Age}", "{ID}"],
        {"id_positions": [2], "link_verdict": "ambiguous", "subscripts": [
            {"position": 1, "verdict": "ambiguous", "field": "Age", "transform": "exact",
             "alternatives": ["alphaup", "upper"]}]}, rows=rows)


def hand_set(name, desc, post_load, answer, index, rows=None):
    """A fixture whose index global is written by hand (no index map in the class)."""
    glob = f"IadSm173{name}"
    cls = person_class(f"IadSm173.{name}", glob, "", "")
    write(name, desc, cls, rows or people(40), {"index_global": f"^{index}", **answer},
          indexes=[f"^{index}"], extra={"post_load": post_load})


def f30():
    hand_set("F30", "dense IDs, an index whose first subscript is unrelated to every field",
             "For i=1:1:40 Set ^IadSm173F30X(5000+(i*17),i)=\"\"",
             {"id_positions": [2], "link_verdict": "not_explained", "no_map": True, "subscripts": [
                 {"position": 1, "verdict": "not_explained"}]}, "IadSm173F30X")


def f31():
    hand_set("F31", "an unrelated global that shares the master's name prefix",
             "For i=1:1:40 Set ^IadSm173F31Alpha(\"Alpha\"_i,1000+i)=\"\"",
             {"link_verdict": "not_linked", "id_agreement": 0.0, "no_map": True}, "IadSm173F31Alpha")


def f33():
    std("F33", "index over a date format of Born (a computed subscript)",
        ["$zd({Born},3)", "{ID}"],
        {"id_positions": [2], "link_verdict": "not_explained", "no_map": True, "subscripts": [
            {"position": 1, "verdict": "not_explained"}]})


def f34():
    # 1,200 rows. The first 700 names (sorted) are plain letters and digits, where UPPER and
    # ALPHAUP give the same key; the last 500 hold a blank, where they differ. A walk of the first
    # 200 nodes sees only the plain ones and cannot tell the two functions apart.
    rows = []
    for i in range(1200):
        name = f"Aaa{i:04d}" if i < 700 else f"Zzz {i:04d}"
        rows.append([name, CITIES[(i * 7 + 3) % len(CITIES)], 18 + (i * 13) % 70 + 100, 40000 + i * 37,
                     ["A", "B", "C"][i % 3]])
    std("F34", "far-end separation: UPPER against ALPHAUP differ only past the first 700 keys",
        ["$$UPPER({Name})", "{ID}"],
        {"id_positions": [2], "link_verdict": "inferred", "subscripts": [
            {"position": 1, "verdict": "inferred", "field": "Name", "transform": "upper"}],
         "plain_prefix_keys": 700}, rows=rows)



def orphans(name, desc, killed, verdict, extra_answer):
    """100 rows, an index map over Name, and `killed` master rows deleted after the load, so the
    index keeps their nodes (deleted rows leave index nodes with no master row)."""
    glob = f"IadSm173{name}"
    cls = person_class(f"IadSm173.{name}", glob, "",
                       index_map("X", f"{glob}X", ["$$SQLUPPER({Name})", "{ID}"]))
    code = " ".join(f"Kill ^{glob}({i})" for i in killed)
    write(name, desc, cls, people(100),
          {"index_global": f"^{glob}X", "id_positions": [2], "link_verdict": verdict,
           "killed_rows": killed, **extra_answer},
          extra={"post_load": code})


def f44():
    orphans("F44", "3 of 100 master rows deleted: under the 5% share", [7, 23, 61], "inferred",
            {"orphan_reported": False})


def f45():
    orphans("F45", "8 of 100 master rows deleted: above 5%, linked, share reported",
            [3, 15, 27, 39, 51, 63, 75, 87], "inferred", {"orphan_reported": True})


def f46():
    orphans("F46", "11 of 100 master rows deleted: ID agreement 0.89, under the 0.9 floor",
            [3, 12, 21, 30, 39, 48, 57, 66, 75, 84, 93], "not_linked", {"orphan_reported": False})



def f38():
    glob = "IadSm173F38"
    maps = index_map("X", glob + "X", ["$$SQLUPPER({Name})", "{ID}"]) + \
        index_map("Y", glob + "Y", ["{City}", "{ID}"])
    cls = person_class("IadSm173.F38", glob, "", maps)
    write("F38", "1,200 rows, two index globals over Name and City", cls, people(1200),
          {"index_global": f"^{glob}X", "id_positions": [2],
           "indexes": [
               {"global": f"^{glob}X", "field": "Name", "transform": "sqlupper", "id_positions": [2]},
               {"global": f"^{glob}Y", "field": "City", "transform": "exact", "id_positions": [2]}]},
          indexes=[f"^{glob}X", f"^{glob}Y"])


def f39():
    glob = "IadSm173F39"
    subs = ["{City}", "{Age}", "{Status}", "{Born}", "{Name}", "$$SQLUPPER({Name})", "$$ALPHAUP({Name})",
            "{ID}"]
    cls = person_class("IadSm173.F39", glob, "", index_map("X", glob + "X", subs))
    write("F39", "an index with seven field subscripts and the ID: six are examined", cls, people(60),
          {"index_global": f"^{glob}X", "id_positions": [8], "examined_positions": [1, 2, 3, 4, 5, 6],
           "unexamined_positions": [7]})


def f40():
    glob = "IadSm173F40"
    cls = person_class("IadSm173.F40", glob, "", index_map("X", glob + "X", ["{Age}", "{ID}"]))
    cls = cls.replace("Property Age As %Integer;", "Property Age As %String;")
    rows = [list(r) for r in people(40)]
    for i, r in enumerate(rows):
        r[2] = str(34 + i % 40)
    # Stored as written: a leading zero, a sign and a fraction are strings, not the numbers 34 and 7.
    rows[5][2], rows[11][2], rows[17][2] = "034", "+34", "07.5"
    write("F40", "Age text holding numbers and a few non-canonical strings, indexed as written", cls, rows,
          {"index_global": f"^{glob}X", "id_positions": [2], "link_verdict": "ambiguous",
           # UPPER keeps "+" and "."; only ALPHAUP separates from EXACT on these strings, and no letter
           # can split EXACT from UPPER in a column of numbers.
           "subscripts": [{"position": 1, "verdict": "ambiguous", "field": "Age", "transform": "exact",
                           "alternatives": ["upper"]}],
           "non_canonical": ["034", "+34", "07.5"]})


def f41():
    stems = ["Éloïse", "Ångström", "Ólafur", "Zoë", "Núñez", "Åse", "Çelik", "Øystein", "Müller", "Łukasz"]
    rows = [list(r) for r in people(40)]
    for i, r in enumerate(rows):
        r[0] = f"{stems[i % len(stems)]} {i}"
    std("F41", "non-ASCII names under $$SQLUPPER", ["$$SQLUPPER({Name})", "{ID}"],
        {"id_positions": [2], "link_verdict": "inferred", "subscripts": [
            {"position": 1, "verdict": "inferred", "field": "Name", "transform": "sqlupper"}]}, rows=rows)


def f42():
    glob = "IadSm173F42"
    maps = index_map("X", glob + "X", ["{City}", "{ID}"]) + index_map("Y", glob + "Y", ["{City}", "{ID}"])
    cls = person_class("IadSm173.F42", glob, "", maps)
    write("F42", "two index globals over City", cls, people(40),
          {"index_global": f"^{glob}X", "id_positions": [2],
           "indexes": [{"global": f"^{glob}X", "field": "City", "transform": "exact", "map": "IadSm173F42X"},
                       {"global": f"^{glob}Y", "field": "City", "transform": "exact", "map": "IadSm173F42Y"}]},
          indexes=[f"^{glob}X", f"^{glob}Y"])


def f43():
    glob = "IadSm173F43"
    cls = person_class("IadSm173.F43", glob, "", index_map("X", glob + "X", ["$$SQLUPPER({Name})", "{ID}"]))
    other = f"""Class IadSm173.F43B Extends %Persistent [ StorageStrategy = SQLStorage ]
{{

Property Label As %String;

Storage SQLStorage
{{
<SqlIdExpression>$i(^{glob}Bc)</SqlIdExpression>
<SQLMap name="Master">
<Data name="Label">
<Delimiter>"^"</Delimiter>
<Piece>1</Piece>
</Data>
<Global>^{glob}X</Global>
<Subscript name="1">
<Expression>{{ID}}</Expression>
</Subscript>
<Type>data</Type>
</SQLMap>
<StreamLocation>^{glob}Bs</StreamLocation>
<Type>%Storage.SQL</Type>
}}

}}
"""
    with open("classes/F43B.cls", "w") as f:
        f.write(other)
    write("F43", "the index global is also another class's master", cls, people(40),
          {"index_global": f"^{glob}X", "id_positions": [2], "link_verdict": "inferred",
           "subscripts": [{"position": 1, "verdict": "inferred", "field": "Name", "transform": "sqlupper"}],
           "other_class": "IadSm173.F43B"},
          extra={"extra_classes": ["F43B"]})


def f37():
    """Typing columns (spec 173 US6): integer, decimal with scale 2, a day number, ISO date text, free
    text with NULLs, and three text columns each indexed through a different transform so the
    class draft's COLLATION has linking evidence for each."""
    import datetime
    cols = ["Name", "Alias", "City", "Qty", "Price", "Placed", "Stamp", "Note"]
    rows = []
    for i in range(40):
        name = FIRST[i] if i < len(FIRST) else f"{FIRST[i % len(FIRST)].strip()}{i}"
        j = (i * 7 + 3) % 40
        alias = FIRST[j] if j < len(FIRST) else f"{FIRST[j % len(FIRST)].strip()}{j}"
        rows.append([name, alias, CITIES[(i * 7 + 3) % len(CITIES)],
                     (i * 17) % 97 + 1,
                     round((500 + (i * 137) % 1000) / 100, 2),
                     45000 + i * 29,
                     f"2024-{1 + i % 12:02d}-{1 + i % 28:02d}",
                     None if i % 8 == 0 else f"note {i} " + "x" * (i % 30)])
    glob = "IadSm173F37"
    props = {"Name": "%String", "Alias": "%String", "City": "%String", "Qty": "%Integer",
             "Price": "%Numeric(SCALE = 2)", "Placed": "%Integer", "Stamp": "%String",
             "Note": "%String(MAXLEN = 255)"}
    prop_text = "".join(f"Property {c} As {props[c]};\n\n" for c in cols)
    data = "".join(
        f'<Data name="{c}">\n<Delimiter>"^"</Delimiter>\n<Piece>{p}</Piece>\n</Data>\n'
        for c, p in sorted((c, k + 1) for k, c in enumerate(cols)))
    maps = (index_map("X", glob + "X", ["$$SQLUPPER({Name})", "{ID}"])
            + index_map("XA", glob + "A", ["$$SQLSTRING({Alias})", "{ID}"])
            + index_map("XC", glob + "C", ["{City}", "{ID}"]))
    cls = f"""Class IadSm173.F37 Extends %Persistent [ StorageStrategy = SQLStorage ]
{{

{prop_text}Storage SQLStorage
{{
<SqlIdExpression>$i(^{glob})</SqlIdExpression>
<SQLMap name="Master">
{data}<Global>^{glob}</Global>
<Subscript name="1">
<Expression>{{ID}}</Expression>
</Subscript>
<Type>data</Type>
</SQLMap>
{maps}<StreamLocation>^{glob}S</StreamLocation>
<Type>%Storage.SQL</Type>
}}

}}
"""
    # The known answer, computed here from the rows and nothing else: counts per column, the day
    # numbers of 1900-01-01 and 2100-12-31 counted from 1840-12-31 (day 0 of $HOROLOG).
    zero = datetime.date(1840, 12, 31)
    lo = (datetime.date(1900, 1, 1) - zero).days
    hi = (datetime.date(2100, 12, 31) - zero).days
    def canon(v):
        # How IRIS files the value: a whole number without a fraction, no trailing zeros.
        return ("%.2f" % v).rstrip("0").rstrip(".") if isinstance(v, float) else str(v)

    def number(text):
        try:
            float(text)
        except ValueError:
            return False
        return text == canon(float(text)) or text.lstrip("-").isdigit()

    typing = {}
    for k, c in enumerate(cols):
        vals = [canon(r[k]) for r in rows if r[k] is not None and r[k] != ""]
        nums = [v for v in vals if number(v)]
        ints = [v for v in nums if "." not in v]
        typing[c] = {"of": len(vals), "integer": len(ints), "numeric": len(nums),
                     "max_scale": max((len(v.split(".")[1]) for v in nums if "." in v), default=0),
                     "date_fit": sum(1 for v in ints if lo <= int(v) <= hi),
                     "max_length": max(len(v) for v in vals)}
    answer = {"typing": typing, "date_window": [lo, hi],
              "types": {"Name": "%String", "Alias": "%String", "City": "%String", "Qty": "%Integer",
                        "Price": "%Numeric", "Placed": "%Date", "Stamp": "%String",
                        "Note": "%String"},
              "collations": {"Name": None, "Alias": "COLLATION = \"SQLSTRING\"",
                             "City": "COLLATION = \"EXACT\""},
              "sums": {"Qty": sum(r[3] for r in rows), "Price": round(sum(r[4] for r in rows), 2)},
              "placed_max": max(r[5] for r in rows),
              "index_global": f"^{glob}X"}
    write("F37", "typing columns: integer, decimal 2, day numbers, ISO text, text with NULLs", cls,
          rows, answer, columns=cols, indexes=[f"^{glob}X", f"^{glob}A", f"^{glob}C"])


ALL = {k[1:].join(["F", ""]) if False else "F" + k[1:]: v for k, v in list(globals().items())
       if k.startswith("f") and k[1:].isdigit()}

if __name__ == "__main__":
    for want in (sys.argv[1:] or sorted(ALL)):
        ALL[want]()
