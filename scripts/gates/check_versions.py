#!/usr/bin/env python3
"""Report files whose declared version disagrees with the Cargo workspace version.

Prints one TSV line per mismatch: path, key, found, expected. Silent when clean.
Called by scripts/gates/antipatterns.sh (check: version-consistency).
"""

import json
import pathlib
import re

root = pathlib.Path(__file__).resolve().parents[2]

cargo = (root / "Cargo.toml").read_text()
m = re.search(
    r"^\[workspace\.package\][^\[]*?^version\s*=\s*\"([^\"]+)\"", cargo, re.M | re.S
)
if not m:
    m = re.search(r"^version\s*=\s*\"([^\"]+)\"", cargo, re.M)
workspace_version = m.group(1)

# (path, dotted key) for every file that must name the release version.
TARGETS = [
    (".claude-plugin/plugin.json", "version"),
    ("vscode-iris-agentic-dev/package.json", "irisAgenticDev.serverVersion"),
]


def dig(obj, dotted):
    for part in dotted.split("."):
        if not isinstance(obj, dict):
            return None
        obj = obj.get(part)
    return obj


for rel, key in TARGETS:
    path = root / rel
    if not path.exists():
        print(f"{rel}\t{key}\t<file missing>\t{workspace_version}")
        continue
    found = dig(json.loads(path.read_text()), key)
    if found != workspace_version:
        print(f"{rel}\t{key}\t{found}\t{workspace_version}")
