#!/usr/bin/env python3
"""Writes classes/<F>.cls and seed/<F>.json for the candidate-discovery and class-mode fixtures of
spec 173 (F32, F35, F36, F47, F48), with the helpers of make_seeds.py. Run from this directory:
`python3 make_seeds_candidates.py [F32 F47 ...]`. Every name, value and global is invented."""

import sys

from make_seeds import index_map, people, person_class, std, write


def f32():
    # An index inside the master global: ^IadSm173F32("idx", SQLUPPER(Name), ID). The class also
    # files a map X over ^IadSm173F32X, so the call has a name-prefix candidate to link too.
    post = (
        'For id=1:1:40 { Set n=$Piece(^IadSm173F32(id),"^",1) '
        'Set ^IadSm173F32("idx",##class(%SYSTEM.SQL.Functions).SQLUPPER(n),id)="" }'
    )
    std(
        "F32",
        "an index inside the master global under the literal first subscript idx",
        ["$$SQLUPPER({Name})", "{ID}"],
        {
            "id_positions": [2],
            "link_verdict": "inferred",
            "sub_prefix": ["idx"],
            "subscripts": [
                {
                    "position": 1,
                    "verdict": "inferred",
                    "field": "Name",
                    "transform": "sqlupper",
                }
            ],
        },
        extra={"post_load": post},
    )


def f47():
    glob = "IadSm173F47"
    cls = person_class(
        "IadSm173.F47",
        glob,
        "",
        index_map("X", f"{glob}.x", ["$$SQLUPPER({Name})", "{ID}"]),
    )
    write(
        "F47",
        "the index global is the master's name and .x",
        cls,
        people(40),
        {
            "index_global": f"^{glob}.x",
            "id_positions": [2],
            "link_verdict": "inferred",
            "subscripts": [
                {
                    "position": 1,
                    "verdict": "inferred",
                    "field": "Name",
                    "transform": "sqlupper",
                }
            ],
        },
        indexes=[f"^{glob}.x"],
    )


def f48():
    # Ten unrelated globals share the master's prefix: A01 to A10, listed in that order.
    names = [f"^IadSm173F48A{i:02d}" for i in range(1, 11)]
    post = (
        'For i=1:1:10 { Set g="^IadSm173F48A"_$Select(i<10:"0",1:"")_i '
        'For j=1:1:40 { Set @g@("Alpha"_j,1000+j)="" } }'
    )
    cls = person_class("IadSm173.F48", "IadSm173F48", "", "")
    write(
        "F48",
        "ten candidates under the master's name prefix, the cap is eight",
        cls,
        people(40),
        {"candidates": names, "examined": names[:8], "not_examined": names[8:]},
        indexes=names,
        extra={"post_load": post},
    )


ALL = {"F32": f32, "F47": f47, "F48": f48}

if __name__ == "__main__":
    for want in sys.argv[1:] or sorted(ALL):
        ALL[want]()
