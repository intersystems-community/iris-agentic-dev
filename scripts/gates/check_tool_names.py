#!/usr/bin/env python3
"""Report `iris_*` / tool-shaped names in prose and error strings that no tool answers to.

Prints one TSV line per finding: path, line, name. Silent when clean.
Called by scripts/gates/antipatterns.sh (check: tool-name-refs).

Why this is a functional check and not a style one: agents read these strings and act on
them. `CODE_EDIT_BLOCKED` told callers to use `iris_document`, a tool that has never
existed, so the remediation advice was a dead end for every caller who followed it.

The registry is `tools::write_gate::CLASSIFICATION` — the router's own list, which is what
dispatch actually matches on. Deriving the name set from anything else (a doc table, a
second constant) reproduces the defect this catches: two artifacts agreeing with each
other is not evidence that either is right.
"""

import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]

CLASSIFICATION_RS = ROOT / "crates/iris-agentic-dev-core/src/tools/write_gate.rs"

# Names that appear in prose but are not registered tools, each for a stated reason.
# Every entry here is a promise that the name means something else; keep it short.
ALLOWED = {
    # The crate, the binary, and the MCP server share this name.
    "iris_agentic_dev",
    # Env vars and config keys are screaming-snake or lowercase but not tools.
    "iris_agentic_dev_skills_dir",
}

# Path-scoped exemptions: `{relative path: {names}}`. A document whose subject is a wrong
# tool name has to be able to write that name down. Keep each entry to one file and one
# name, so the exemption cannot quietly cover a new mistake in the same file.
ALLOWED_IN = {
    # The postmortem's subject is remediation text that named tools which never existed.
    "docs/postmortem-empty-success.md": {"iris_document"},
    "docs/backlog-empty-success-audit.md": {"iris_document"},
    # The 1.3.2 notes describe fixing that remediation text and the two false positives the
    # tool-name extractor itself produced, so they quote all three names.
    "docs/release-notes/v1.3.2.md": {"iris_document", "iris_graph", "iris_opt"},
}


def registered_tools() -> set[str]:
    text = CLASSIFICATION_RS.read_text()
    start = text.find("pub const CLASSIFICATION")
    if start < 0:
        sys.exit(
            f"{CLASSIFICATION_RS}: no `pub const CLASSIFICATION` — this check cannot "
            "resolve the tool registry, so it would pass vacuously. Fix the path."
        )
    body = text[start : text.find("\n];", start)]
    names = set(re.findall(r'"([a-z0-9_]+)"', body))
    if len(names) < 50:
        sys.exit(
            f"{CLASSIFICATION_RS}: extracted only {len(names)} tool names from "
            "CLASSIFICATION, which is fewer than the registry has ever had. The "
            "extractor is broken and would flag real tools as unknown."
        )
    return names


def targets():
    for pattern in (
        "crates/**/src/**/*.rs",
        "docs/**/*.md",
        "skills/**/*.md",
        "crates/**/skills/**/*.md",
    ):
        yield from ROOT.glob(pattern)


# A Rust identifier being declared or path-qualified is a local symbol, not a tool
# reference. `handle_iris_doc` and `iris::connection` must not be read as tool names.
LOCAL_SYMBOL = re.compile(
    r"(?:\bfn\s+|\blet\s+|\bstruct\s+|\benum\s+|\bmod\s+|\bcrate::|::|_)$"
)

# A binding declared anywhere in the file is a local symbol at every *use* site too, not
# just where it is declared. `let iris_opt = …` was skipped and the `iris_opt.map(…)` on the
# next line was reported — the same identifier, flagged for being used.
DECLARED = re.compile(
    r"\b(?:fn|let|let\s+mut|const|static|struct|enum|mod|type)\s+(iris_[a-z0-9_]+)\b"
)


def main() -> int:
    known = registered_tools() | ALLOWED
    findings = 0
    seen = set()
    for path in sorted(targets()):
        try:
            text = path.read_text()
        except (OSError, UnicodeDecodeError):
            continue
        rel = path.relative_to(ROOT)
        # Rust items and bindings declared in this file, plus any path-scoped exemptions.
        file_known = (
            known | set(DECLARED.findall(text)) | ALLOWED_IN.get(str(rel), set())
        )
        for lineno, line in enumerate(text.splitlines(), 1):
            for match in re.finditer(r"\biris_[a-z0-9_]+\b", line):
                name = match.group(0)
                if name in file_known:
                    continue
                # `..._impl` is the naming convention for the Rust function behind a tool,
                # never a name an agent can call. Comments and section banners reference
                # these constantly.
                if name.endswith("_impl"):
                    continue
                if LOCAL_SYMBOL.search(line[: match.start()]):
                    continue
                # `iris_dev_bin()` is a zero-arg Rust function. No tool is ever referenced
                # with empty parens — a call an agent makes carries arguments — so this
                # form is code even when the identifier is declared in another file.
                if line[match.end() : match.end() + 2] == "()":
                    continue
                # Trailing `_`-joined identifiers (iris_doc_search_impl) are code.
                if re.match(r"[a-z0-9_]", line[match.end() : match.end() + 1] or " "):
                    continue
                key = (rel, name)
                if key in seen:
                    continue
                seen.add(key)
                print(f"{rel}\t{lineno}\t{name}")
                findings += 1
    return 0 if findings == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
