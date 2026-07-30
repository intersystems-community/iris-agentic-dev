#!/usr/bin/env python3
"""
Check per-file coverage floors.

Parses cargo-llvm-cov --summary-only output and coverage-floors.toml,
then verifies:
  1. Every src/ .rs file that appears in the coverage output (i.e. has executable
     lines) has an entry in coverage-floors.toml.
  2. No registered file is below its floor.

Files absent from the coverage output have no executable lines (pure mod
declarations, struct defs, etc.) and are silently exempt.

Exit 0 on pass, 1 on any violation.
"""

import argparse
import os
import re
import sys


def parse_args():
    p = argparse.ArgumentParser()
    p.add_argument("--floors", required=True, help="Path to coverage-floors.toml")
    p.add_argument("--coverage", required=True, help="Path to llvm-cov --summary-only output")
    p.add_argument("--src", required=True, help="Path to crates/iris-agentic-dev-core/src/")
    return p.parse_args()


def load_floors(path):
    """Parse coverage-floors.toml. Returns dict: rel_path -> int floor."""
    floors = {}
    with open(path) as f:
        for line in f:
            m = re.match(r'\s*"(src/[^"]+\.rs)"\s*=\s*(\d+)', line)
            if m:
                floors[m.group(1)] = int(m.group(2))
    return floors


def load_coverage(path):
    """Parse llvm-cov output. Returns dict: rel_path -> float line coverage %."""
    actual = {}
    with open(path) as f:
        for line in f:
            # Match lines containing our crate's src path.
            # Format: <abs-path>  <total> <missed> <pct>%  ...
            m = re.search(
                r"crates/iris-agentic-dev-core/(src/\S+\.rs)\s+\d+\s+\d+\s+([\d.]+)%",
                line,
            )
            if m:
                actual[m.group(1)] = float(m.group(2))
    return actual


def find_src_files(src_dir):
    """Walk src/ and return paths relative to crates/iris-agentic-dev-core/."""
    crate_root = os.path.dirname(src_dir)  # .../crates/iris-agentic-dev-core
    result = []
    for dirpath, _, filenames in os.walk(src_dir):
        for fname in sorted(filenames):
            if not fname.endswith(".rs"):
                continue
            abs_path = os.path.join(dirpath, fname)
            rel = os.path.relpath(abs_path, crate_root).replace("\\", "/")
            if rel == "src/lib.rs":
                continue
            result.append(rel)
    return sorted(result)


def main():
    args = parse_args()

    floors = load_floors(args.floors)
    actual = load_coverage(args.coverage)
    src_files = find_src_files(args.src)

    violations = 0
    unregistered = []
    below_floor = []
    stale = []

    # Check 1: every src file that has coverage data must have a floor entry
    for rel in src_files:
        if rel not in actual:
            # No executable lines — exempt (pure mod/use declarations, struct defs)
            continue
        if rel not in floors:
            unregistered.append((rel, actual[rel]))

    # Check 2: every registered file meets its floor
    for rel, floor in sorted(floors.items()):
        pct = actual.get(rel)
        if pct is None:
            # Floor entry exists but file not in coverage output.
            # Could be deleted, renamed, or genuinely no-exec-lines.
            stale.append(rel)
            continue
        if int(pct) < floor:
            below_floor.append((rel, floor, pct))

    if unregistered:
        print(f"FAIL: {len(unregistered)} file(s) have coverage data but no floor entry:")
        for rel, pct in sorted(unregistered):
            print(f"  {rel}  (measured: {pct:.2f}%)")
        print()
        print("  Add an entry to coverage-floors.toml. Suggested: floor = int(measured) - 2")
        print()
        violations += len(unregistered)

    if below_floor:
        print(f"FAIL: {len(below_floor)} file(s) below their registered floor:")
        for rel, floor, pct in sorted(below_floor):
            drop = floor - int(pct)
            print(f"  {rel}: floor={floor}%  actual={pct:.2f}%  drop={drop}pp")
        print()
        violations += len(below_floor)

    if stale:
        print(
            f"WARN: {len(stale)} floor entries have no coverage output "
            f"(deleted / renamed / no executable lines):"
        )
        for rel in sorted(stale):
            print(f"  {rel}")
        print()

    if violations == 0:
        covered = len([r for r in floors if r in actual])
        print(f"OK: {covered} files with coverage data all meet their floors.")
        # Warn on files within 3pp of their floor
        near = [
            (rel, floors[rel], actual[rel])
            for rel in floors
            if rel in actual and 0 <= int(actual[rel]) - floors[rel] <= 3
        ]
        if near:
            print()
            print("Files within 3pp of their floor (watch these):")
            for rel, floor, pct in sorted(near):
                margin = int(pct) - floor
                print(f"  {rel}: actual={pct:.2f}%  floor={floor}%  margin={margin}pp")
        sys.exit(0)
    else:
        print(f"Total violations: {violations}")
        sys.exit(1)


if __name__ == "__main__":
    main()
