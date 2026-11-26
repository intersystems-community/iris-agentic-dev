#!/usr/bin/env python3
"""Writes classes/<F>.cls and seed/<F>.json for the bitmap fixtures (F19, F20, F21 and the two
hand-written variants F19n and F19c) and the sparse fixtures (F22 to F25). Run from this directory:
`python3 make_seeds_p89.py [F19 F20 ...]`. Every name, value and global here is invented for this
repository. Helpers come from make_seeds.py."""

import json
import sys

import make_seeds as m

# Statuses that differ under every collation: a case difference (exact against upper), a blank
# (SQLUPPER against STRING) and a hyphen (SQLUPPER against ALPHAUP).
STATUS_MAP = ["open", "Closed", "on hold"]
STATUS_CLASS = ["In Progress", "on-hold", "Closed Out"]


def bitmap_rows(statuses, n=40):
    rows = [list(r) for r in m.people(n)]
    for i, r in enumerate(rows):
        r[4] = statuses[i % len(statuses)]
    return rows


def ids_by_status(rows):
    """The row IDs per Status, 1-based in insertion order (the master's ID counter starts at 1)."""
    out = {}
    for i, r in enumerate(rows):
        out.setdefault(r[4], []).append(i + 1)
    return out


def bitmap_map_xml(glob):
    return (
        f'<SQLMap name="MBit">\n<Global>^{glob}X</Global>\n<Subscript name="1">\n'
        f"<Expression>{{Status}}</Expression>\n</Subscript>\n<Type>bitmap</Type>\n</SQLMap>\n"
    )


def map_bitmap_class(name):
    glob = f"IadSm173{name}"
    return m.person_class(
        f"IadSm173.{name}",
        glob,
        "Index MBit On Status [ Type = bitmap ];\n",
        bitmap_map_xml(glob),
    )


def f19():
    rows = bitmap_rows(STATUS_MAP)
    m.write(
        "F19",
        "map-compiled bitmap over {Status} only",
        map_bitmap_class("F19"),
        rows,
        {
            "index_global": "^IadSm173F19X",
            "chunk_position": 2,
            "ids_checked": len(rows),
            "ids_found": len(rows),
            "field": "Status",
            "transform": "exact",
            "link_verdict": "inferred",
            "ids_by_status": ids_by_status(rows),
        },
    )


def f19n():
    """The bit strings name IDs the master does not hold: the nodes are rewritten after the load."""
    rows = bitmap_rows(STATUS_MAP)
    glob = "IadSm173F19n"
    code = (
        f'Kill ^{glob}X Set b="" For p=3001:1:3040 {{ Set $Bit(b,p)=1 }} '
        f'Set ^{glob}X("open",1)=b,^{glob}X("Closed",1)=b,^{glob}X("on hold",1)=b'
    )
    m.write(
        "F19n",
        "bitmap whose decoded IDs are not in the master",
        map_bitmap_class("F19n"),
        rows,
        {
            "index_global": f"^{glob}X",
            "chunk_position": 2,
            "ids_found": 0,
            "link_verdict": "not_linked",
            "no_map": True,
        },
        extra={"post_load": code},
    )


def f19c():
    """Chunk 1 is right; a second node of `open` holds bits that decode, at 64000 a chunk, to IDs
    the master does not have (an index built with another chunk size would look like this)."""
    rows = bitmap_rows(STATUS_MAP)
    glob = "IadSm173F19c"
    code = f'Set b="" For p=1:1:3 {{ Set $Bit(b,p)=1 }} Set ^{glob}X("open",2)=b'
    m.write(
        "F19c",
        "bitmap with a chunk-2 node that finds no master row",
        map_bitmap_class("F19c"),
        rows,
        {
            "index_global": f"^{glob}X",
            "chunk_position": 2,
            "ids_checked": len(rows) + 3,
            "ids_found": len(rows),
            "later_checked": 3,
            "later_found": 0,
            "link_verdict": "ambiguous",
            "assumed": "64000",
        },
        extra={"post_load": code},
    )


def f20():
    glob = "IadSm173.F20"
    cls = """Class IadSm173.F20 Extends %Persistent
{

Property Name As %String;

Property City As %String;

Property Age As %Integer;

Property Born As %Integer;

Property Status As %String;

Index StatusIdx On Status [ Type = bitmap ];
}
"""
    rows = bitmap_rows(STATUS_CLASS)
    m.write(
        "F20",
        "class-compiled bitmap StatusIdx over Status (default storage master)",
        cls,
        rows,
        {
            "index_global": f"^{glob}I",
            "chunk_position": 3,
            "lead": "StatusIdx",
            "ids_checked": len(rows),
            "ids_found": len(rows),
            "field": "Status",
            "transform": "sqlupper",
            "link_verdict": "inferred",
            "ids_by_status": ids_by_status(rows),
        },
        master=f"^{glob}D",
        indexes=[f"^{glob}I"],
        extra={"master_columns": ["", "Name", "City", "Age", "Born", "Status"]},
    )


def f21():
    """No class: the master global `^IadSm173Bits(0, chunk)` holds $BIT strings, written by hand."""
    chunks = {1: [3, 5, 8, 64000], 2: [1, 2]}
    code = "Kill ^IadSm173Bits " + " ".join(
        'Set b="" '
        + "".join(f"Set $Bit(b,{p})=1 " for p in ps)
        + f"Set ^IadSm173Bits(0,{c})=b"
        for c, ps in chunks.items()
    )
    ids = sorted((c - 1) * 64000 + p - 1 for c, ps in chunks.items() for p in ps)
    seed = {
        "name": "F21",
        "description": "master ^IadSm173Bits(0, chunk) = $BIT",
        "master_global": "^IadSm173Bits",
        "index_globals": [],
        "post_load": code,
        "answer": {
            "constant": "0",
            "ids": ids,
            "id_count": len(ids),
            "id_sum": sum(ids),
            "chunks": 2,
        },
    }
    with open("seed/F21.json", "w") as f:
        json.dump(seed, f, ensure_ascii=False, indent=1)
        f.write("\n")


# ---- sparse and conditional indexes (F22 to F25) -----------------------------------------------


def conditional_class(name, condition, fields):
    """A class whose index over {City} carries a Condition and PopulationType conditional."""
    glob = f"IadSm173{name}"
    extra = (f"<Condition>{condition}</Condition>\n<ConditionFields>{fields}</ConditionFields>\n"
             "<PopulationType>conditional</PopulationType>\n")
    return m.person_class(f"IadSm173.{name}", glob, "",
                          m.index_map("X", f"{glob}X", ["{City}", "{ID}"], extra))


def sparse_answer(name, rows, missing_ids, **more):
    return {"index_global": f"^IadSm173{name}X", "rows": len(rows), "missing_ids": missing_ids,
            "missing_share": len(missing_ids) / len(rows), **more}


def f22():
    rows = m.with_empty(m.people(60), "City", 5, None)
    ids = [i + 1 for i, r in enumerate(rows) if r[1] is None]
    m.write("F22", "sparse: the index skips NULL City", conditional_class("F22", "{City}'=\"\"", "City"),
            rows, sparse_answer("F22", rows, ids, rule="null", field="City",
                                condition="{City}'=\"\"", link_verdict="inferred"))


def f23():
    rows = m.with_empty(m.people(60), "City", 5, "")
    ids = [i + 1 for i, r in enumerate(rows) if r[1] == ""]
    m.write("F23", "sparse: the index skips empty-string City",
            conditional_class("F23", "{City}'=$c(0)", "City"), rows,
            sparse_answer("F23", rows, ids, rule="empty_string", field="City",
                          condition="{City}'=$c(0)", link_verdict="inferred"))


def f24():
    rows = [list(r) for r in m.people(90)]
    ids = [i + 1 for i, r in enumerate(rows) if r[4] != "A"]
    m.write("F24", "sparse: the index skips Status other than A",
            conditional_class("F24", "{Status}=\"A\"", "Status"), rows,
            sparse_answer("F24", rows, ids, rule="fixed", field="Status", literal="A",
                          condition="{Status}=\"A\"", link_verdict="inferred"))


def f25():
    rows = [list(r) for r in m.people(80)]
    ids = [i for i in range(7, len(rows) + 1, 7)]
    glob = "IadSm173F25"
    cls = m.person_class("IadSm173.F25", glob, "", m.index_map("X", f"{glob}X", ["{City}", "{ID}"]))
    code = (f'For id=7:7:{len(rows)} {{ Set c=$Piece(^{glob}(id),"^",2) Kill ^{glob}X(c,id) }}')
    m.write("F25", "sparse: the index skips every 7th row", cls, rows,
            sparse_answer("F25", rows, ids, rule=None, link_verdict="inferred"),
            extra={"post_load": code})


ALL = {"F19": f19, "F19n": f19n, "F19c": f19c, "F20": f20, "F21": f21, "F22": f22, "F23": f23,
       "F24": f24, "F25": f25}

if __name__ == "__main__":
    for want in sys.argv[1:] or sorted(ALL):
        ALL[want]()
