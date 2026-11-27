#!/usr/bin/env python3
"""Writes classes/<F>.cls and seed/<F>.json for the two boundary fixtures of the corpus (spec 173,
T059): F49 and F50 sit on both sides of the 0.95 agreement an index position needs to count as
explained. Run from this directory: `python3 make_seeds_corpus.py [F49 F50]`. Every name, value and
global here is invented for this repository. Helpers come from make_seeds.py."""

import sys

import make_seeds as m

ROWS = 40


def off_by(name, desc, ids, link_verdict, sub):
    """An index over Name where the nodes of `ids` were rewritten after the load: their first
    subscript is a string no field of the master gives, so `ROWS - len(ids)` of the `ROWS` nodes
    agree with Name. Names are all different, so the linker's sample (at most 3 nodes under one
    first-subscript value) reads every node and the agreement is exactly the share that agrees."""
    rows = [list(r) for r in m.people(ROWS)]
    glob = f"IadSm173{name}"
    cls = m.person_class(
        f"IadSm173.{name}", glob, "", m.index_map("X", f"{glob}X", ["{Name}", "{ID}"])
    )
    listed = ",".join(str(i) for i in ids)
    code = (
        f'For id={listed} {{ Set c=$Piece(^{glob}(id),"^",1) Kill ^{glob}X(c,id) '
        f'Set ^{glob}X("zz~"_id,id)="" }}'
    )
    answer = {
        "index_global": f"^{glob}X",
        "id_positions": [2],
        "link_verdict": link_verdict,
        "subscripts": [sub],
    }
    if link_verdict != "inferred":
        answer["no_map"] = True
    m.write(name, desc, cls, rows, answer, extra={"post_load": code})


def f49():
    """38 of 40 nodes agree: exactly 0.95, which explains the position."""
    off_by(
        "F49",
        "index over Name, 2 of 40 nodes rewritten: agreement 0.95 exactly explains the position",
        [5, 17],
        "inferred",
        {"position": 1, "field": "Name", "transform": "exact", "agreement": 0.95},
    )


def f50():
    """37 of 40 nodes agree: 0.925, under the line, so the position is not explained."""
    off_by(
        "F50",
        "index over Name, 3 of 40 nodes rewritten: agreement 0.925 does not explain the position",
        [5, 17, 29],
        "not_explained",
        {"position": 1, "verdict": "not_explained", "agreement": 0.925},
    )


ALL = {"F49": f49, "F50": f50}

if __name__ == "__main__":
    for want in sys.argv[1:] or sorted(ALL):
        ALL[want]()
