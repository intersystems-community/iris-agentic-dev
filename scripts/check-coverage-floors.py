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
    p.add_argument("--coverage", help="Path to llvm-cov --summary-only output")
    p.add_argument("--lcov", help="Path to lcov file (alternative to --coverage)")
    p.add_argument("--src", required=True, help="Path to crates/iris-agentic-dev-core/src/")
    return p.parse_args()


def load_floors(path):
    """Parse coverage-floors.toml. Returns (file_floors dict, overall_floor int or None)."""
    floors = {}
    overall = None
    with open(path) as f:
        for line in f:
            m = re.match(r'\s*"(src/[^"]+\.rs)"\s*=\s*(\d+)', line)
            if m:
                floors[m.group(1)] = int(m.group(2))
                continue
            m = re.match(r'\s*overall\s*=\s*(\d+)', line)
            if m:
                overall = int(m.group(1))
    return floors, overall


def load_coverage(path):
    """Parse llvm-cov --summary-only output. Returns dict: rel_path -> float line coverage %.

    cargo-llvm-cov --summary-only emits short paths relative to the crate src/
    directory (e.g. "iris/workspace_config.rs" or full abs paths depending on
    version). We normalise both forms to "src/<subpath>" to match coverage-floors.toml.
    """
    actual = {}
    with open(path) as f:
        for line in f:
            # Long-form: .../crates/iris-agentic-dev-core/src/foo/bar.rs  ...
            m = re.search(
                r"crates/iris-agentic-dev-core/(src/\S+\.rs)\s+\d+\s+\d+\s+([\d.]+)%",
                line,
            )
            if m:
                actual[m.group(1)] = float(m.group(2))
                continue
            # Short-form (common with --summary-only): "iris/foo.rs  123  45  63.41%  ..."
            # These are src-relative paths without the "src/" prefix.
            m = re.match(
                r"^([a-z_][a-z0-9_/]*\.rs)\s+\d+\s+\d+\s+([\d.]+)%",
                line.strip(),
            )
            if m:
                actual["src/" + m.group(1)] = float(m.group(2))
    return actual


def load_lcov(path):
    """Parse an lcov file. Returns (file_dict, overall_pct).

    file_dict: rel_path -> float line coverage %
    overall_pct: float or None if no lines found
    """
    actual = {}
    total_lines = 0
    total_covered = 0

    cur_file = None
    file_lines = 0
    file_covered = 0

    crate_marker = "iris-agentic-dev-core/"

    with open(path) as f:
        for raw in f:
            line = raw.strip()
            if line.startswith("SF:"):
                cur_file = None
                file_lines = 0
                file_covered = 0
                full_path = line[3:]
                if crate_marker in full_path:
                    idx = full_path.index(crate_marker)
                    rel = full_path[idx + len(crate_marker):]
                    if rel.startswith("src/") and rel.endswith(".rs"):
                        cur_file = rel
            elif line.startswith("DA:") and cur_file is not None:
                parts = line[3:].split(",")
                if len(parts) >= 2:
                    file_lines += 1
                    total_lines += 1
                    if int(parts[1]) > 0:
                        file_covered += 1
                        total_covered += 1
            elif line == "end_of_record":
                if cur_file is not None and file_lines > 0:
                    actual[cur_file] = 100.0 * file_covered / file_lines
                cur_file = None

    overall = (100.0 * total_covered / total_lines) if total_lines > 0 else None
    return actual, overall


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

    if not args.coverage and not args.lcov:
        print("ERROR: one of --coverage or --lcov is required")
        sys.exit(1)

    floors, overall_floor = load_floors(args.floors)

    if args.lcov:
        actual, measured_overall = load_lcov(args.lcov)
    else:
        actual = load_coverage(args.coverage)
        measured_overall = None

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

    # Check overall floor (only when using --lcov and overall= is set in toml)
    if overall_floor is not None and measured_overall is not None:
        if int(measured_overall) < overall_floor:
            print(
                f"FAIL: overall line coverage {measured_overall:.2f}% is below "
                f"floor {overall_floor}% (gap: {overall_floor - int(measured_overall)}pp)"
            )
            violations += 1
        else:
            margin = int(measured_overall) - overall_floor
            print(
                f"Overall: {measured_overall:.2f}%  floor={overall_floor}%  margin={margin}pp"
            )
    elif measured_overall is not None:
        print(f"Overall: {measured_overall:.2f}%  (no overall floor set)")

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
